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

//! Flow data: the unit of work that flows through a pipeline.
//!
//! A flow data holds the immutable [`Evidence`] for one request, the element
//! data produced as it is processed, and any errors recorded along the way. It
//! keeps an [`Arc`] back-reference to the pipeline that created it, so it can be
//! processed by simply calling [`FlowData::process`]. See the
//! [conceptual overview](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/conceptual-overview.md#flow-data).
//!
//! A flow data belongs to exactly one pipeline and is created by it
//! ([`crate::Pipeline::create_flow_data`]). It is intended to be used on a
//! single thread, per the
//! [thread-safety specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/thread-safety.md#flow-data),
//! so its accessors take `&self`/`&mut self` and need no internal locking.

use std::collections::HashMap;
use std::sync::Arc;

use ahash::RandomState;

use crate::ci_map::ci_get;
use crate::element_data::ElementData;
use crate::error::{Error, FlowError, NoValueError, Result};
use crate::evidence::{DataKey, Evidence, EvidenceKeyFilter};
use crate::pipeline::Pipeline;
use crate::typed_key::TypedKey;
use crate::value::PropertyValue;

/// The data that is processed by a pipeline.
///
/// Construct one via [`crate::Pipeline::create_flow_data`], add evidence when
/// creating it, call [`FlowData::process`], then read results with
/// [`FlowData::get`] (typed) or [`FlowData::get_str`] (by key).
pub struct FlowData {
    pipeline: Arc<Pipeline>,
    evidence: Evidence,
    /// Element data keyed by lowercased data key, matching the
    /// case-insensitive access rule.
    data: HashMap<String, Box<dyn ElementData>, RandomState>,
    errors: Vec<FlowError>,
    processed: bool,
}

impl FlowData {
    /// Create a new flow data for the given pipeline and evidence.
    ///
    /// This is called by [`crate::Pipeline::create_flow_data`]; application code
    /// uses that method (or its builder) rather than calling this directly.
    pub(crate) fn new(pipeline: Arc<Pipeline>, evidence: Evidence) -> Self {
        FlowData {
            pipeline,
            evidence,
            data: HashMap::default(),
            errors: Vec::new(),
            processed: false,
        }
    }

    /// The pipeline that created this flow data.
    pub fn pipeline(&self) -> &Arc<Pipeline> {
        &self.pipeline
    }

    /// The immutable evidence supplied to this flow data.
    pub fn evidence(&self) -> &Evidence {
        &self.evidence
    }

    /// The errors recorded during processing. Empty if processing has not run
    /// or completed without error.
    pub fn errors(&self) -> &[FlowError] {
        &self.errors
    }

    /// True once [`FlowData::process`] has been called.
    pub fn is_processed(&self) -> bool {
        self.processed
    }

    /// Process this flow data using its pipeline.
    ///
    /// Each element is run in order. Whether errors propagate or are recorded
    /// is governed by the pipeline's `suppress_process_exceptions` flag (see
    /// [`crate::Pipeline::is_suppress_process_exceptions`] for the exact
    /// behavior).
    ///
    /// A flow data is processed once, as the
    /// [automated testing specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/automated-testing.md#test-cases)
    /// requires. A second call runs no element and returns
    /// [`Error::PipelineConfiguration`], so create a new flow data for each
    /// set of evidence.
    pub fn process(&mut self) -> Result<()> {
        if self.processed {
            return Err(Error::configuration(
                "This flow data has already been processed. A flow data is \
                 processed once, so create a new one for each set of evidence.",
            ));
        }
        self.processed = true;
        // Clone the Arc so we do not hold an immutable borrow of `self` (via
        // `self.pipeline`) while `process` needs `&mut self`.
        let pipeline = Arc::clone(&self.pipeline);
        pipeline.process(self)
    }

    /// Record an error against this flow data, and log it at error level when
    /// `should_log` is set.
    ///
    /// The pipeline records each error an element returns, logging it unless
    /// errors are suppressed. An element that carries on past a failure
    /// records it here instead, using [`FlowError::with_should_throw`] with
    /// `false` when processing should not fail because of it.
    pub fn add_error(&mut self, error: FlowError, should_log: bool) {
        if should_log {
            log::error!(
                "An error occurred in element '{}': {}",
                error.element_data_key,
                error.source
            );
        }
        self.errors.push(error);
    }

    /// Get the element data stored under the given string data key, if any.
    ///
    /// This is mechanism 1 of the
    /// [access-to-results specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/access-to-results.md#accessing-results)
    /// (get element data by string key). The key is matched
    /// case-insensitively. The data is returned as a `&dyn ElementData` so the
    /// caller can use its dynamic property bag without knowing the concrete
    /// type.
    pub fn get_str(&self, data_key: &str) -> Option<&dyn ElementData> {
        self.lookup(data_key)
    }

    /// Get the element data for a [`TypedKey`], downcast to its concrete type.
    ///
    /// This is mechanism 2 of the
    /// [access-to-results specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/access-to-results.md#accessing-results)
    /// (get element data by type). The compile-time type travels in the typed
    /// key, and the value is recovered with an [`std::any::Any`] downcast, so no
    /// reflection is used. Returns `None` if there is no data under the key, or
    /// the stored data is not of type `T`.
    pub fn get<T: ElementData>(&self, key: TypedKey<T>) -> Option<&T> {
        self.lookup(key.name())
            .and_then(|data| data.as_any().downcast_ref::<T>())
    }

    /// Get the element data for a [`TypedKey`], or insert it using `create` if
    /// it is not already present.
    ///
    /// Elements call this from `process` to add their data exactly once. If
    /// data already exists under the key but is not of type `T`, this returns a
    /// configuration error rather than overwriting it.
    pub fn get_or_add<T, F>(&mut self, key: TypedKey<T>, create: F) -> Result<&mut T>
    where
        T: ElementData,
        F: FnOnce() -> T,
    {
        let stored_key = key.name().to_lowercase();
        let boxed = self
            .data
            .entry(stored_key)
            .or_insert_with(|| Box::new(create()));
        boxed.as_any_mut().downcast_mut::<T>().ok_or_else(|| {
            Error::configuration(format!(
                "Element data already present under key '{}' is not of the \
                 expected type.",
                key.name()
            ))
        })
    }

    /// List the data keys present in this flow data. Order is unspecified.
    pub fn data_keys(&self) -> Vec<String> {
        self.data.keys().cloned().collect()
    }

    /// Get a property value, preferring element data and falling back to
    /// evidence.
    ///
    /// This realises the helper described in the
    /// [evidence specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/evidence.md#adding-evidence-values):
    /// because evidence is immutable, an element that wants to expose a value
    /// for later elements writes it to its element data, and downstream lookups
    /// use this method to read either source transparently.
    ///
    /// The search is: every element data that holds the property, as
    /// [`ElementData::has_property`] says, is asked for it by its full name,
    /// so data known to lack it is not asked and logs nothing. If exactly one
    /// element data has a value, it is returned. If more
    /// than one does, that is ambiguous and an error is returned, as the
    /// specification allows. If none do, the value is taken from evidence (as a
    /// [`PropertyValue::String`]). Returns `Err(NoValueError)` if nothing
    /// matches.
    pub fn get_evidence_or_property(
        &self,
        name: &str,
    ) -> std::result::Result<PropertyValue, Error> {
        let mut found: Option<PropertyValue> = None;
        for boxed in self.data.values() {
            if !boxed.has_property(name) {
                continue;
            }
            if let Ok(value) = boxed.get(name) {
                if found.is_some() {
                    return Err(Error::configuration(format!(
                        "Multiple element data instances contain a property \
                         named '{name}'; the value is ambiguous."
                    )));
                }
                found = Some(value);
            }
        }
        if let Some(value) = found {
            return Ok(value);
        }
        if let Some(value) = self.evidence.get(name) {
            return Ok(PropertyValue::String(value.to_owned()));
        }
        Err(Error::NoValue(NoValueError::new(format!(
            "No element data property or evidence value found for '{name}'."
        ))))
    }

    /// A filter including the evidence keys usable by any element in this flow
    /// data's pipeline. Delegates to [`crate::Pipeline::evidence_key_filter`].
    pub fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.pipeline.evidence_key_filter()
    }

    /// Build a deterministic [`DataKey`] from this flow data's evidence using
    /// the supplied filter. See [`Evidence::generate_key`].
    pub fn generate_key(&self, filter: &dyn EvidenceKeyFilter) -> DataKey {
        self.evidence.generate_key(filter)
    }

    /// Case-insensitive lookup of the element data for a data key.
    fn lookup(&self, data_key: &str) -> Option<&dyn ElementData> {
        ci_get(&self.data, data_key).map(|boxed| boxed.as_ref())
    }
}
