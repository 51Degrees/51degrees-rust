/* *********************************************************************
 * This Original Work is copyright of 51 Degrees Mobile Experts Limited.
 * Copyright 2026 51 Degrees Mobile Experts Limited, Davidson House,
 * Forbury Square, Reading, Berkshire, United Kingdom RG1 3EU.
 *
 * This Original Work is licensed under the European Union Public Licence
 * (EUPL) v.1.2 and is subject to its terms as set out below.
 *
 * If a copy of the EUPL was not distributed with this file, You can obtain
 * one at https://opensource.org/licenses/EUPL-1.2.
 *
 * The 'Compatible Licences' set out in the Appendix to the EUPL (as may be
 * amended by the European Commission) shall be deemed incompatible for
 * the purposes of the Work and the provisions of the compatibility
 * clause in Article 5 of the EUPL shall not apply.
 *
 * If using the Work as, or as part of, a network application, by
 * including the attribution notice(s) required under Article 5 of the EUPL
 * in the end user terms of the application under an appropriate heading,
 * such notice(s) shall fulfill the requirements of that article.
 * ********************************************************************* */

//! The pipeline and its builder.
//!
//! A [`Pipeline`] groups an ordered list of flow elements into a single process.
//! It is immutable once built and shared via [`Arc`], so the same pipeline
//! serves many concurrent requests. See the
//! [conceptual overview](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/conceptual-overview.md#pipeline).
//!
//! Build one with [`PipelineBuilder`]: add elements in execution order, then
//! call [`PipelineBuilder::build`] to obtain an `Arc<Pipeline>`. Elements added
//! with [`PipelineBuilder::add_parallel_elements`] run at the same time, as the
//! [parallel processing specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/advanced-features/parallel-processing.md)
//! describes.

use std::any::Any;
use std::ops::Range;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::thread;

use crate::constants;
use crate::error::{Error, FlowError, Result};
use crate::evidence::{Evidence, EvidenceKeyFilter, EvidenceKeyFilterAggregator};
use crate::flow_data::{BeforeGroup, FlowData};
use crate::flow_element::FlowElement;
use crate::group_threads::Job;

/// An immutable, shareable container of ordered flow elements.
///
/// A pipeline runs its elements in the order they were added, for each flow
/// data it processes, and the elements of a parallel group at the same time.
/// It exposes a pipeline-wide evidence filter (the union of its elements'
/// filters) and the `suppress_process_exceptions` flag that controls error
/// handling.
pub struct Pipeline {
    elements: Vec<Arc<dyn FlowElement>>,
    steps: Vec<Step>,
    evidence_key_filter: EvidenceKeyFilterAggregator,
    suppress_process_exceptions: bool,
    concurrent: bool,
    /// Objects the pipeline keeps alive for as long as it lives, such as the
    /// data update service that refreshes an engine's data file.
    resources: Vec<Arc<dyn Any + Send + Sync>>,
}

impl Pipeline {
    /// Start building a pipeline.
    pub fn builder() -> PipelineBuilder {
        PipelineBuilder::new()
    }

    /// The flow elements in this pipeline, in the order they were added. The
    /// elements of a parallel group are listed in the order given to the
    /// group, and the group itself is not an element.
    pub fn flow_elements(&self) -> &[Arc<dyn FlowElement>] {
        &self.elements
    }

    /// A filter including the evidence keys usable by any element in this
    /// pipeline. This is the union (logical OR) of every element's filter and
    /// is used, for example, to build the web `Vary` whitelist.
    pub fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.evidence_key_filter
    }

    /// Whether the pipeline suppresses processing exceptions.
    ///
    /// When `true`, processing (via [`crate::FlowData::process`]) records
    /// per-element errors on the flow data, logs each at debug level and
    /// returns `Ok`. When `false`, it runs every element and then returns
    /// `Err(Error::Aggregate(..))` if any failed. This is the
    /// `SuppressProcessExceptions` flag from the
    /// [exception-handling specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/exception-handling.md#process-function).
    pub fn is_suppress_process_exceptions(&self) -> bool {
        self.suppress_process_exceptions
    }

    /// True if this pipeline has a parallel group, or any element in it runs
    /// multiple threads internally.
    pub fn is_concurrent(&self) -> bool {
        self.concurrent
    }

    /// The first resource of type `T` attached with
    /// [`PipelineBuilder::add_resource`], such as the data update service that
    /// keeps an on-premise engine's data file current.
    pub fn resource<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.resources
            .iter()
            .find_map(|resource| Arc::clone(resource).downcast::<T>().ok())
    }

    /// Create a new, unprocessed flow data bound to this pipeline, with no
    /// evidence. Add evidence to it before processing.
    pub fn create_flow_data(self: &Arc<Self>) -> FlowData {
        FlowData::new(Arc::clone(self), Evidence::default())
    }

    /// Create a new, unprocessed flow data bound to this pipeline, with the
    /// given evidence.
    pub fn create_flow_data_with(self: &Arc<Self>, evidence: Evidence) -> FlowData {
        FlowData::new(Arc::clone(self), evidence)
    }

    /// Run every element against the flow data in order.
    ///
    /// Each element's `process` is called in turn, and the elements of a
    /// parallel group at the same time. On an element error the behavior
    /// depends on `suppress_process_exceptions`:
    ///
    /// - **Suppressed** (`true`): the error is recorded on the flow data via
    ///   [`FlowData::errors`], logged at debug level, and the remaining
    ///   elements still run. The method returns `Ok(())`.
    /// - **Not suppressed** (`false`, the default): every element is still run
    ///   so that all errors are gathered, then the method returns
    ///   `Err(Error::Aggregate(..))` containing the errors whose
    ///   [`FlowError::should_throw`] is `true`. If none qualify, it returns
    ///   `Ok(())`.
    ///
    /// A parallel group's errors are recorded in the order its elements were
    /// given, whichever finished first.
    ///
    /// This follows the
    /// [exception-handling specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/exception-handling.md#process-function).
    pub(crate) fn process(&self, data: &mut FlowData) -> Result<()> {
        for step in &self.steps {
            match step {
                Step::Element(index) => {
                    let element = &self.elements[*index];
                    if let Err(error) = element.process(data) {
                        self.record_error(data, element.data_key(), error);
                    }
                }
                Step::Group(range) => self.process_group(&self.elements[range.clone()], data),
            }
        }

        if !self.suppress_process_exceptions {
            // Collect the errors that should propagate. They stay on the flow
            // data as well, so a caller can read them from either place.
            let to_throw: Vec<FlowError> = data
                .errors()
                .iter()
                .filter(|e| e.should_throw)
                .map(|e| {
                    FlowError::with_should_throw(
                        e.element_data_key.clone(),
                        clone_error(&e.source),
                        e.should_throw,
                    )
                })
                .collect();
            if !to_throw.is_empty() {
                return Err(Error::Aggregate(to_throw));
            }
        }
        Ok(())
    }

    /// Record an element's error on the flow data. A suppressed error is
    /// logged as well, so it can be seen without reading the flow data. Debug
    /// level, because suppressing says the caller carries on past failures.
    fn record_error(&self, data: &mut FlowData, data_key: &str, error: Error) {
        if self.suppress_process_exceptions {
            log::debug!("Suppressed an error in element '{data_key}': {error}");
        }
        data.add_error(FlowError::new(data_key, error));
    }

    /// Run a parallel group's elements at the same time, each on a flow data
    /// of its own that reads what `data` held before the group, then add what
    /// each one added and record each one's errors, in the order given.
    ///
    /// A panic in an element is raised again here once every element has
    /// finished and `data` holds its evidence and earlier results again.
    fn process_group(&self, elements: &[Arc<dyn FlowElement>], data: &mut FlowData) {
        let before = data.begin_group();
        let outcomes: Vec<_> = run_at_once(elements, &before)
            .into_iter()
            // Dropping each member's flow data releases its share of `before`.
            .map(|outcome| outcome.map(|(member, result)| (member.into_added_data(), result)))
            .collect();
        data.end_group(before);

        let mut first_panic = None;
        for (element, outcome) in elements.iter().zip(outcomes) {
            match outcome {
                Ok((added, result)) => {
                    if let Err(error) = result {
                        self.record_error(data, element.data_key(), error);
                    }
                    for key in data.add_group_member_data(added) {
                        let error = Error::configuration(format!(
                            "Element data under key '{key}' was added by more \
                             than one element of a parallel group, so only \
                             the first is kept."
                        ));
                        self.record_error(data, element.data_key(), error);
                    }
                }
                Err(payload) => {
                    first_panic.get_or_insert(payload);
                }
            }
        }
        if let Some(payload) = first_panic {
            panic::resume_unwind(payload);
        }
    }
}

/// One step of processing, as positions in the pipeline's elements.
enum Step {
    /// One element on its own.
    Element(usize),
    /// A parallel group, whose elements run at the same time.
    Group(Range<usize>),
}

/// What one element of a parallel group gave, being its flow data and its
/// result, or what it panicked with.
type Outcome = thread::Result<(FlowData, Result<()>)>;

/// Run each element on a group member flow data, all at the same time, and
/// return the outcomes in element order.
///
/// Every element but the first is offered to the group threads, and the first
/// runs on this thread. This thread then runs any element no group thread has
/// started, so the group finishes even when no group thread is free or the
/// platform cannot start threads.
fn run_at_once(elements: &[Arc<dyn FlowElement>], before: &Arc<BeforeGroup>) -> Vec<Outcome> {
    let Some((first, others)) = elements.split_first() else {
        return Vec::new();
    };
    let jobs: Vec<_> = others
        .iter()
        .map(|element| {
            let element = Arc::clone(element);
            let mut member = FlowData::group_member(before);
            Job::offer(move || {
                let result = element.process(&mut member);
                (member, result)
            })
        })
        .collect();
    let mut member = FlowData::group_member(before);
    let mut outcomes = Vec::with_capacity(elements.len());
    outcomes.push(panic::catch_unwind(AssertUnwindSafe(move || {
        let result = first.process(&mut member);
        (member, result)
    })));
    outcomes.extend(jobs.iter().map(Job::finish));
    outcomes
}

/// Produce a value-equivalent copy of an [`Error`] for aggregation.
///
/// [`Error`] is not [`Clone`] because some variants may carry non-clonable
/// sources in future. For the aggregate we only need to preserve the
/// user-visible message, so we rebuild a configuration-style error carrying the
/// original `Display` output. The dedicated variants that are clonable are
/// preserved as themselves.
fn clone_error(error: &Error) -> Error {
    match error {
        Error::PropertyMissing {
            property,
            element_data_key,
            reason,
        } => Error::PropertyMissing {
            property: property.clone(),
            element_data_key: element_data_key.clone(),
            reason: reason.clone(),
        },
        Error::NoValue(no_value) => Error::NoValue(no_value.clone()),
        Error::PipelineConfiguration { message } => Error::PipelineConfiguration {
            message: message.clone(),
        },
        Error::NotProcessed { message } => Error::NotProcessed {
            message: message.clone(),
        },
        Error::CloudRequest {
            status_code,
            retry_after_seconds,
            message,
        } => Error::CloudRequest {
            status_code: *status_code,
            retry_after_seconds: *retry_after_seconds,
            message: message.clone(),
        },
        Error::Native { status, message } => Error::Native {
            status: status.clone(),
            message: message.clone(),
        },
        // Nested aggregates and any future non-clonable variants collapse to a
        // configuration error carrying the rendered message.
        other => Error::PipelineConfiguration {
            message: other.to_string(),
        },
    }
}

/// Builder for an immutable [`Pipeline`].
///
/// Add elements with [`PipelineBuilder::add_element`] in the order they should
/// execute, optionally toggle [`PipelineBuilder::suppress_process_exceptions`],
/// then [`PipelineBuilder::build`] to consume the builder into an
/// `Arc<Pipeline>`.
#[derive(Default)]
pub struct PipelineBuilder {
    elements: Vec<Arc<dyn FlowElement>>,
    steps: Vec<Step>,
    suppress_process_exceptions: bool,
    resources: Vec<Arc<dyn Any + Send + Sync>>,
}

impl PipelineBuilder {
    /// Create an empty builder. `suppress_process_exceptions` defaults to the
    /// specification default of `false`.
    pub fn new() -> Self {
        PipelineBuilder {
            elements: Vec::new(),
            steps: Vec::new(),
            suppress_process_exceptions: constants::DEFAULT_SUPPRESS_PROCESS_EXCEPTIONS,
            resources: Vec::new(),
        }
    }

    /// Add a flow element. Elements execute in the order they are added.
    /// Returns `self` for chaining.
    pub fn add_element(mut self, element: Arc<dyn FlowElement>) -> Self {
        self.steps.push(Step::Element(self.elements.len()));
        self.elements.push(element);
        self
    }

    /// Add elements that run at the same time, as a parallel group. The
    /// pipeline waits for all of them before it runs the next element, and
    /// records their errors as it does for any element, in the order given
    /// here. Returns `self` for chaining.
    ///
    /// Each element of the group reads the evidence, element data and errors
    /// from before the group, and none sees what another element of the group
    /// adds. One element is added on its own and no elements add nothing, as
    /// the 51Degrees .NET pipeline does. See the
    /// [parallel processing specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/advanced-features/parallel-processing.md).
    ///
    /// A group pays for elements that wait, for example on a call to another
    /// service. It pays for elements that use the processor only when each
    /// takes much longer than waking another thread, and only while
    /// processors are idle. Under load every element still needs a processor
    /// and the group adds the hand over between threads, so a group of short
    /// elements that only use the processor lowers throughput. Add those with
    /// [`PipelineBuilder::add_element`] instead.
    pub fn add_parallel_elements(
        mut self,
        elements: impl IntoIterator<Item = Arc<dyn FlowElement>>,
    ) -> Self {
        let start = self.elements.len();
        self.elements.extend(elements);
        match self.elements.len() - start {
            0 => {}
            1 => self.steps.push(Step::Element(start)),
            _ => self.steps.push(Step::Group(start..self.elements.len())),
        }
        self
    }

    /// Attach a resource the pipeline keeps alive for as long as it lives, such
    /// as the data update service that refreshes an engine's data file. The
    /// pipeline drops it with its elements, and [`Pipeline::resource`] finds it
    /// again by type. Returns `self` for chaining.
    pub fn add_resource(mut self, resource: Arc<dyn Any + Send + Sync>) -> Self {
        self.resources.push(resource);
        self
    }

    /// Set whether processing exceptions are suppressed. See
    /// [`Pipeline::is_suppress_process_exceptions`]. Returns `self` for
    /// chaining.
    pub fn suppress_process_exceptions(mut self, suppress: bool) -> Self {
        self.suppress_process_exceptions = suppress;
        self
    }

    /// Consume the builder and produce the shared, immutable pipeline.
    ///
    /// Returns an error if no elements were added, since an empty pipeline
    /// cannot do useful work and almost always indicates a misconfiguration.
    pub fn build(self) -> Result<Arc<Pipeline>> {
        if self.elements.is_empty() {
            return Err(Error::configuration(
                "A pipeline must contain at least one flow element.",
            ));
        }

        // Build the pipeline-wide evidence filter by ORing each element's
        // filter. Each element's filter is wrapped so the aggregator can query
        // it through the trait object.
        let mut aggregator = EvidenceKeyFilterAggregator::new();
        for element in &self.elements {
            aggregator.add_filter(Box::new(ElementEvidenceFilter {
                element: Arc::clone(element),
            }));
        }

        let concurrent = self.elements.iter().any(|e| e.is_concurrent())
            || self.steps.iter().any(|step| matches!(step, Step::Group(_)));

        Ok(Arc::new(Pipeline {
            elements: self.elements,
            steps: self.steps,
            evidence_key_filter: aggregator,
            suppress_process_exceptions: self.suppress_process_exceptions,
            concurrent,
            resources: self.resources,
        }))
    }
}

/// Adapts an element's borrowed evidence filter into an owned
/// [`EvidenceKeyFilter`] that the aggregator can hold.
///
/// The element exposes its filter as `&dyn EvidenceKeyFilter` tied to the
/// element's lifetime. To store it in the pipeline-wide aggregator we keep an
/// `Arc` to the element and forward filter queries to it.
struct ElementEvidenceFilter {
    element: Arc<dyn FlowElement>,
}

impl EvidenceKeyFilter for ElementEvidenceFilter {
    fn include(&self, key: &str) -> bool {
        self.element.evidence_key_filter().include(key)
    }

    fn order(&self, key: &str) -> Option<i32> {
        self.element.evidence_key_filter().order(key)
    }

    fn named_keys(&self) -> Option<Vec<String>> {
        self.element.evidence_key_filter().named_keys()
    }
}
