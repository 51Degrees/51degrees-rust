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
//! data produced as it is processed, and any errors and warnings recorded
//! along the way. It
//! keeps an [`Arc`] back-reference to the pipeline that created it, so it can be
//! processed by simply calling [`FlowData::process`]. See the
//! [conceptual overview](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/conceptual-overview.md#flow-data).
//!
//! A flow data belongs to exactly one pipeline and is created by it
//! ([`crate::Pipeline::create_flow_data`]). It is intended to be used on a
//! single thread, per the
//! [thread-safety specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/thread-safety.md#flow-data),
//! so its accessors take `&self`/`&mut self` and need no internal locking.
//!
//! A parallel group gives each of its elements a flow data of its own, which
//! reads the evidence, element data, errors and warnings from before the group
//! through a shared view and keeps what the element adds. The pipeline moves
//! what each element added back into the original flow data once they have all
//! finished.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;

use ahash::RandomState;

use crate::ci_map::ci_get;
use crate::element_data::ElementData;
use crate::error::{Error, FlowError, NoValueError, Result};
use crate::evidence::{DataKey, Evidence, EvidenceKeyFilter};
use crate::flow_element::FlowElement;
use crate::pipeline::Pipeline;
use crate::typed_key::TypedKey;
use crate::value::PropertyValue;
use crate::warning::FlowWarning;

/// The data that is processed by a pipeline.
///
/// Construct one via [`crate::Pipeline::create_flow_data`], add evidence when
/// creating it, call [`FlowData::process`], then read results with
/// [`FlowData::get`] (typed) or [`FlowData::get_str`] (by key).
pub struct FlowData {
    source: Source,
    evidence: Evidence,
    data: DataMap,
    /// The keys of `data` in the order their element data was added.
    order: Vec<String>,
    errors: Vec<FlowError>,
    warnings: Vec<FlowWarning>,
    processed: bool,
}

/// Where a flow data finds its pipeline.
enum Source {
    /// The pipeline that created the flow data.
    Pipeline(Arc<Pipeline>),
    /// For an element in a parallel group, what the group's flow data held
    /// before the group, read but never changed. The pipeline, evidence,
    /// errors and warnings are read from here in place of the flow data's own.
    Group(Arc<BeforeGroup>),
}

/// The pipeline, evidence, element data, errors and warnings of a flow data
/// before a parallel group runs, shared by every element in the group. Holding
/// the pipeline here means an element of the group does not write the
/// pipeline's reference count, which every thread processing with it shares.
pub(crate) struct BeforeGroup {
    pipeline: Arc<Pipeline>,
    evidence: Evidence,
    data: DataMap,
    order: Vec<String>,
    errors: Vec<FlowError>,
    warnings: Vec<FlowWarning>,
}

/// What one element of a parallel group added to its flow data.
pub(crate) struct GroupMemberAdded {
    /// The element data it added, keyed by lowercased data key.
    pub(crate) data: DataMap,
    /// The keys of `data` in the order the element added them.
    pub(crate) order: Vec<String>,
    /// The errors it recorded.
    pub(crate) errors: Vec<FlowError>,
    /// The warnings it recorded.
    pub(crate) warnings: Vec<FlowWarning>,
}

impl FlowData {
    /// Create a new flow data for the given pipeline and evidence.
    ///
    /// This is called by [`crate::Pipeline::create_flow_data`]; application code
    /// uses that method (or its builder) rather than calling this directly.
    pub(crate) fn new(pipeline: Arc<Pipeline>, evidence: Evidence) -> Self {
        FlowData {
            source: Source::Pipeline(pipeline),
            evidence,
            data: HashMap::default(),
            order: Vec::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            processed: false,
        }
    }

    /// The pipeline that created this flow data.
    pub fn pipeline(&self) -> &Arc<Pipeline> {
        match &self.source {
            Source::Pipeline(pipeline) => pipeline,
            Source::Group(before) => &before.pipeline,
        }
    }

    /// The immutable evidence supplied to this flow data.
    pub fn evidence(&self) -> &Evidence {
        match self.before_group() {
            Some(before) => &before.evidence,
            None => &self.evidence,
        }
    }

    /// The errors recorded during processing. Empty if processing has not run
    /// or completed without error.
    ///
    /// An element in a parallel group sees the errors recorded before the
    /// group. Errors from the group, including those its elements record
    /// with [`FlowData::add_error`], are recorded once every element in it
    /// has finished, in the order the elements were given.
    pub fn errors(&self) -> &[FlowError] {
        match self.before_group() {
            Some(before) => &before.errors,
            None => &self.errors,
        }
    }

    /// The warnings recorded during processing, in the order they were added.
    /// Empty when there are none.
    ///
    /// An element in a parallel group sees the warnings recorded before the
    /// group. The group's own are added once every element in it has
    /// finished, in the order the elements were given.
    pub fn warnings(&self) -> &[FlowWarning] {
        match self.before_group() {
            Some(before) => &before.warnings,
            None => &self.warnings,
        }
    }

    /// True once [`FlowData::process`] has been called.
    pub fn is_processed(&self) -> bool {
        self.processed
    }

    /// Process this flow data using its pipeline.
    ///
    /// Each element is run in order, and the elements of a parallel group at
    /// the same time. Whether errors propagate or are recorded
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
        // `self.pipeline()`) while `process` needs `&mut self`.
        let pipeline = Arc::clone(self.pipeline());
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

    /// Record a message for the caller about something that was wrong with the
    /// request but did not stop it being served, and log it at warning level.
    ///
    /// Use this rather than [`FlowData::add_error`] for a problem that should
    /// not fail the request, such as an evidence value an element could not
    /// use. `element` is the element the warning relates to, if any, and the
    /// log record names its type.
    pub fn add_warning(&mut self, message: impl Into<String>, element: Option<&dyn FlowElement>) {
        let message = message.into();
        match element {
            Some(element) => {
                log::warn!("{message} (from {})", short_type_name(element.type_name()))
            }
            None => log::warn!("{message}"),
        }
        let element_data_key = element.map(|element| element.data_key().to_owned());
        self.warnings
            .push(FlowWarning::new(message, element_data_key));
    }

    /// Add the warnings one element of a parallel group recorded, which
    /// [`FlowData::add_warning`] logged as they were recorded.
    pub(crate) fn add_group_member_warnings(&mut self, warnings: Vec<FlowWarning>) {
        self.warnings.extend(warnings);
    }

    /// Move the evidence, element data, errors and warnings into the view
    /// every element of a parallel group shares. [`FlowData::end_group`] puts
    /// them back.
    pub(crate) fn begin_group(&mut self) -> Arc<BeforeGroup> {
        let evidence = self.evidence.empty_with_same_hasher();
        let data = DataMap::with_hasher(self.data.hasher().clone());
        Arc::new(BeforeGroup {
            pipeline: Arc::clone(self.pipeline()),
            evidence: std::mem::replace(&mut self.evidence, evidence),
            data: std::mem::replace(&mut self.data, data),
            order: std::mem::take(&mut self.order),
            errors: std::mem::take(&mut self.errors),
            warnings: std::mem::take(&mut self.warnings),
        })
    }

    /// A flow data for one element of a parallel group, reading what the
    /// group's flow data held before the group. It counts as processed, so
    /// the element cannot process it again. Its maps copy the hasher of the
    /// group's, because seeding a new hasher writes a counter every thread
    /// shares.
    pub(crate) fn group_member(before: &Arc<BeforeGroup>) -> Self {
        FlowData {
            source: Source::Group(Arc::clone(before)),
            evidence: before.evidence.empty_with_same_hasher(),
            data: DataMap::with_hasher(before.data.hasher().clone()),
            order: Vec::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            processed: true,
        }
    }

    /// What a group member's element added. Dropping the member releases its
    /// share of the view.
    pub(crate) fn into_added(self) -> GroupMemberAdded {
        GroupMemberAdded {
            data: self.data,
            order: self.order,
            errors: self.errors,
            warnings: self.warnings,
        }
    }

    /// Put back what [`FlowData::begin_group`] took.
    ///
    /// # Panics
    ///
    /// When an element of the group kept the flow data it was given after
    /// processing, for example by swapping it for another, because the view
    /// cannot then be taken back.
    pub(crate) fn end_group(&mut self, before: Arc<BeforeGroup>) {
        let before = Arc::into_inner(before).expect(
            "an element of a parallel group kept the flow data it was given \
             after processing",
        );
        self.evidence = before.evidence;
        self.data = before.data;
        self.order = before.order;
        self.errors = before.errors;
        self.warnings = before.warnings;
    }

    /// Add the element data one element of a parallel group added, in the
    /// order it added it. Data under a key this flow data already holds is
    /// left as it is, and its key is returned so the pipeline can record an
    /// error.
    pub(crate) fn add_group_member_data(
        &mut self,
        mut added: DataMap,
        order: Vec<String>,
    ) -> Vec<String> {
        let mut refused = Vec::new();
        for key in order {
            let Some(value) = added.remove(&key) else {
                continue;
            };
            match self.data.entry(key) {
                Entry::Occupied(entry) => refused.push(entry.key().clone()),
                Entry::Vacant(entry) => {
                    self.order.push(entry.key().clone());
                    entry.insert(value);
                }
            }
        }
        refused
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
    ///
    /// An element in a parallel group can read the data added before the
    /// group but not change it, so for a key added before the group this
    /// returns a configuration error.
    pub fn get_or_add<T, F>(&mut self, key: TypedKey<T>, create: F) -> Result<&mut T>
    where
        T: ElementData,
        F: FnOnce() -> T,
    {
        let stored_key = key.name().to_lowercase();
        if let Some(before) = self.before_group() {
            if before.data.contains_key(&stored_key) {
                return Err(Error::configuration(format!(
                    "Element data under key '{}' was added before the parallel \
                     group, so an element in the group cannot change it.",
                    key.name()
                )));
            }
        }
        let boxed = match self.data.entry(stored_key) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                self.order.push(entry.key().clone());
                entry.insert(Box::new(create()))
            }
        };
        boxed.as_any_mut().downcast_mut::<T>().ok_or_else(|| {
            Error::configuration(format!(
                "Element data already present under key '{}' is not of the \
                 expected type.",
                key.name()
            ))
        })
    }

    /// List the data keys present in this flow data, lowercased, in the order
    /// their element data was added. The JSON builder writes the element data
    /// in this order.
    ///
    /// The data a parallel group adds comes after the group, in the order its
    /// elements were given. An element in a group sees the keys added before
    /// the group, then its own.
    pub fn data_keys(&self) -> Vec<String> {
        self.before_group()
            .into_iter()
            .flat_map(|before| before.order.iter())
            .chain(self.order.iter())
            .cloned()
            .collect()
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
        for (_, boxed) in self.all_data() {
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
        if let Some(value) = self.evidence().get(name) {
            return Ok(PropertyValue::String(value.to_owned()));
        }
        Err(Error::NoValue(NoValueError::new(format!(
            "No element data property or evidence value found for '{name}'."
        ))))
    }

    /// A filter including the evidence keys usable by any element in this flow
    /// data's pipeline. Delegates to [`crate::Pipeline::evidence_key_filter`].
    pub fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.pipeline().evidence_key_filter()
    }

    /// Build a deterministic [`DataKey`] from this flow data's evidence using
    /// the supplied filter. See [`Evidence::generate_key`].
    pub fn generate_key(&self, filter: &dyn EvidenceKeyFilter) -> DataKey {
        self.evidence().generate_key(filter)
    }

    /// Case-insensitive lookup of the element data for a data key, in this
    /// flow data and then in what it held before a parallel group.
    fn lookup(&self, data_key: &str) -> Option<&dyn ElementData> {
        ci_get(&self.data, data_key)
            .or_else(|| {
                self.before_group()
                    .and_then(|before| ci_get(&before.data, data_key))
            })
            .map(|boxed| boxed.as_ref())
    }

    /// Every element data, with what was held before a parallel group last.
    fn all_data(&self) -> impl Iterator<Item = (&String, &Box<dyn ElementData>)> {
        let before = self
            .before_group()
            .into_iter()
            .flat_map(|before| before.data.iter());
        self.data.iter().chain(before)
    }

    /// For an element in a parallel group, what the group's flow data held
    /// before the group.
    fn before_group(&self) -> Option<&BeforeGroup> {
        match &self.source {
            Source::Pipeline(_) => None,
            Source::Group(before) => Some(before),
        }
    }
}

/// The last segment of a type's path, without generic arguments, as .NET's
/// type name gives it.
fn short_type_name(full: &str) -> &str {
    let path = full.split('<').next().unwrap_or(full);
    path.rsplit("::").next().unwrap_or(path)
}

/// Element data keyed by lowercased data key, matching the case-insensitive
/// access rule.
pub(crate) type DataMap = HashMap<String, Box<dyn ElementData>, RandomState>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EvidenceKeyFilterWhitelist, FlowElement, PropertyMetaData};

    /// An element that does nothing.
    struct Idle(EvidenceKeyFilterWhitelist);

    impl FlowElement for Idle {
        fn process(&self, _data: &mut FlowData) -> Result<()> {
            Ok(())
        }
        fn data_key(&self) -> &str {
            "idle"
        }
        fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
            &self.0
        }
        fn properties(&self) -> &[PropertyMetaData] {
            &[]
        }
    }

    /// The element data maps of a group's flow data and of each member copy
    /// the hasher of the map from before the group, so a key hashes the same
    /// through each, where a freshly seeded hasher hashes it differently.
    #[test]
    fn group_maps_copy_the_hasher_of_the_data_before_the_group() {
        const KEY: &str = "device";
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(Idle(EvidenceKeyFilterWhitelist::default())))
            .build()
            .unwrap();
        let mut data = pipeline.create_flow_data();
        let hash = data.data.hasher().hash_one(KEY);
        assert_ne!(RandomState::new().hash_one(KEY), hash);

        let before = data.begin_group();
        let member = FlowData::group_member(&before);
        assert_eq!(before.data.hasher().hash_one(KEY), hash);
        assert_eq!(data.data.hasher().hash_one(KEY), hash);
        assert_eq!(member.data.hasher().hash_one(KEY), hash);

        drop(member);
        data.end_group(before);
    }
}
