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
//! call [`PipelineBuilder::build`] to obtain an `Arc<Pipeline>`.

use std::any::Any;
use std::sync::Arc;

use crate::constants;
use crate::error::{Error, FlowError, Result};
use crate::evidence::{Evidence, EvidenceKeyFilter, EvidenceKeyFilterAggregator};
use crate::flow_data::FlowData;
use crate::flow_element::FlowElement;

/// An immutable, shareable container of ordered flow elements.
///
/// A pipeline runs its elements sequentially, in the order they were added, for
/// each flow data it processes. It exposes a pipeline-wide evidence filter (the
/// union of its elements' filters) and the `suppress_process_exceptions` flag
/// that controls error handling.
pub struct Pipeline {
    elements: Vec<Arc<dyn FlowElement>>,
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

    /// The flow elements in this pipeline, in execution order.
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
    /// returns `Ok`. When `false`, it logs each at error level, runs every
    /// element and then returns `Err(Error::Aggregate(..))` if any failed.
    /// This is the `SuppressProcessExceptions` flag from the
    /// [exception-handling specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/exception-handling.md#process-function).
    pub fn is_suppress_process_exceptions(&self) -> bool {
        self.suppress_process_exceptions
    }

    /// True if any element in this pipeline runs multiple threads internally.
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
    /// Each element's `process` is called in turn. On an element error the
    /// behavior depends on `suppress_process_exceptions`:
    ///
    /// - **Suppressed** (`true`): the error is recorded on the flow data via
    ///   [`FlowData::errors`], logged at debug level, and the remaining
    ///   elements still run. The method returns `Ok(())`.
    /// - **Not suppressed** (`false`, the default): the error is recorded and
    ///   logged at error level. Every element is still run so that all errors
    ///   are gathered, then the method returns
    ///   `Err(Error::Aggregate(..))` containing the errors whose
    ///   [`FlowError::should_throw`] is `true`. If none qualify, it returns
    ///   `Ok(())`.
    ///
    /// This follows the
    /// [exception-handling specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/exception-handling.md#process-function).
    pub(crate) fn process(&self, data: &mut FlowData) -> Result<()> {
        for element in &self.elements {
            if let Err(error) = element.process(data) {
                self.record_error(data, element.data_key(), error);
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

    /// Record an element's error on the flow data and log it, as .NET's
    /// pipeline does. Error level when the error is returned to the caller,
    /// and debug level when it is suppressed, because suppressing says the
    /// caller carries on past failures.
    fn record_error(&self, data: &mut FlowData, data_key: &str, error: Error) {
        if self.suppress_process_exceptions {
            log::debug!("Suppressed an error in element '{data_key}': {error}");
        } else {
            log::error!("An error occurred in element '{data_key}': {error}");
        }
        data.add_error(FlowError::new(data_key, error));
    }
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
    suppress_process_exceptions: bool,
    resources: Vec<Arc<dyn Any + Send + Sync>>,
}

impl PipelineBuilder {
    /// Create an empty builder. `suppress_process_exceptions` defaults to the
    /// specification default of `false`.
    pub fn new() -> Self {
        PipelineBuilder {
            elements: Vec::new(),
            suppress_process_exceptions: constants::DEFAULT_SUPPRESS_PROCESS_EXCEPTIONS,
            resources: Vec::new(),
        }
    }

    /// Add a flow element. Elements execute in the order they are added.
    /// Returns `self` for chaining.
    pub fn add_element(mut self, element: Arc<dyn FlowElement>) -> Self {
        self.elements.push(element);
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
    /// Each element's [`FlowElement::add_pipeline`] is called with the new
    /// pipeline, in order. Returns an error if no elements were added, since an
    /// empty pipeline cannot do useful work and almost always indicates a
    /// misconfiguration, or the first error an element returns from
    /// `add_pipeline`.
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

        let concurrent = self.elements.iter().any(|e| e.is_concurrent());

        let pipeline = Arc::new(Pipeline {
            elements: self.elements,
            evidence_key_filter: aggregator,
            suppress_process_exceptions: self.suppress_process_exceptions,
            concurrent,
            resources: self.resources,
        });
        for element in &pipeline.elements {
            element.add_pipeline(&pipeline)?;
        }
        Ok(pipeline)
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
