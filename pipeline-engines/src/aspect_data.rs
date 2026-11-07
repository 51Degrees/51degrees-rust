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

//! Aspect data: the element data produced by an aspect engine.
//!
//! An aspect engine writes an [`AspectData`] into the flow data. This is a
//! specialisation of the core [`ElementData`]. In addition to being a property
//! bag, it records which engine (by data key) produced it and whether it came
//! from a cache hit. See the
//! [aspect-data section](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/conceptual-overview.md#aspect-data)
//! of the conceptual overview.
//!
//! [`AspectDataBase`] is a ready-made backing an engine's concrete data struct
//! can embed. It forwards the core [`ElementData`] property bag to a
//! [`MapElementData`] and tracks the engine keys and cache-hit flag for the
//! aspect layer. With a [`MissingPropertySource`] from each engine that wrote
//! to it, it also gives the reason a property it does not hold is missing,
//! and logs a warning for each read of such a property. A cloud engine whose
//! request failed marks its data, so each such read gives
//! [`MissingPropertyReason::CloudRequestFailed`](crate::MissingPropertyReason::CloudRequestFailed)
//! instead and logs at info level.

use std::any::Any;
use std::sync::Arc;

use fiftyone_pipeline_core::{
    ElementData, MapElementData, MissingPropertyReason, NoValueError, PropertyValue,
};

use crate::missing_property::{
    cloud_request_failed, first_known_reason, MissingPropertyResult, MissingPropertySource,
};

/// Element data produced by an aspect engine.
///
/// This trait extends the core [`ElementData`] with aspect-only state. It adds
/// the list of engines (by data key) that contributed to the data, and whether
/// the data was served from a cache hit rather than freshly processed.
///
/// Implementations are usually built by embedding an [`AspectDataBase`] and
/// delegating to it, so the trait has no required methods beyond the two
/// accessors below. Like [`ElementData`], it is `Send + Sync`. Only the engine
/// that adds it writes it, and the elements of a parallel group can then read
/// it from several threads at once. The
/// [thread-safety specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/thread-safety.md#aspect-data)
/// says aspect data should be thread-safe.
pub trait AspectData: ElementData {
    /// The data keys of the engines that contributed to this data.
    ///
    /// Usually a single key, but a flow data may be processed by more than one
    /// engine writing to the same key (for example a primary engine and a
    /// secondary one), in which case each is recorded here. Engines are
    /// identified by their string data key rather than by object reference to
    /// keep the data free of back-references.
    fn engine_keys(&self) -> &[String];

    /// True if this data instance was the result of a cache hit rather than the
    /// engine processing the evidence.
    ///
    /// Only meaningful when the engine is configured to record cache hits (see
    /// [`crate::AspectEngine::records_cache_hits`]).
    fn cache_hit(&self) -> bool;
}

/// A reusable backing for aspect-data structs.
///
/// Concrete aspect data embeds one of these and forwards the [`ElementData`]
/// and [`AspectData`] traits to it. It holds the property bag (a
/// [`MapElementData`]), the contributing engine keys, the cache-hit flag, the
/// [`MissingPropertySource`] of each engine that added one, whether the
/// cloud request failed and whether a read of a missing property is logged.
/// The lazy-loading task list lives on the engine wrapper rather than the data
/// (see [`crate::AspectEngineBase`]).
#[derive(Debug, Clone)]
pub struct AspectDataBase {
    values: MapElementData,
    engine_keys: Vec<String>,
    cache_hit: bool,
    missing_property_sources: Vec<Arc<MissingPropertySource>>,
    cloud_request_failed: bool,
    log_missing_properties: bool,
}

impl AspectDataBase {
    /// Create an empty aspect-data backing attributed to a single engine,
    /// identified by its data key.
    pub fn new(engine_key: impl Into<String>) -> Self {
        AspectDataBase {
            values: MapElementData::new(),
            engine_keys: vec![engine_key.into()],
            cache_hit: false,
            missing_property_sources: Vec::new(),
            cloud_request_failed: false,
            log_missing_properties: true,
        }
    }

    /// Create an aspect-data backing from an existing property bag, attributed
    /// to a single engine.
    pub fn with_values(engine_key: impl Into<String>, values: MapElementData) -> Self {
        AspectDataBase {
            values,
            engine_keys: vec![engine_key.into()],
            cache_hit: false,
            missing_property_sources: Vec::new(),
            cloud_request_failed: false,
            log_missing_properties: true,
        }
    }

    /// Set a property value, overwriting any existing value for that name.
    ///
    /// The name is lowercased so lookups are case-insensitive, matching the
    /// core property bag. Returns `self` for chaining during construction.
    pub fn set(mut self, name: impl AsRef<str>, value: impl Into<PropertyValue>) -> Self {
        self.values = self.values.set(name, value);
        self
    }

    /// Insert a property value by mutable reference (for use after the backing
    /// has been created), overwriting any existing value for that name.
    pub fn insert(&mut self, name: impl AsRef<str>, value: impl Into<PropertyValue>) {
        self.values.insert(name, value);
    }

    /// Record that another engine (identified by its data key) also contributed
    /// to this data. Has no effect if the key is already recorded.
    pub fn add_engine_key(&mut self, engine_key: impl Into<String>) {
        let key = engine_key.into();
        if !self.engine_keys.contains(&key) {
            self.engine_keys.push(key);
        }
    }

    /// Mark this data as having been served from a cache hit.
    pub fn set_cache_hit(&mut self) {
        self.cache_hit = true;
    }

    /// Borrow the underlying property bag.
    pub fn values(&self) -> &MapElementData {
        &self.values
    }

    /// Mutably borrow the underlying property bag.
    pub fn values_mut(&mut self) -> &mut MapElementData {
        &mut self.values
    }

    /// The data keys of the engines that contributed to this data.
    pub fn engine_keys(&self) -> &[String] {
        &self.engine_keys
    }

    /// True if this data was served from a cache hit.
    pub fn cache_hit(&self) -> bool {
        self.cache_hit
    }

    /// Add the details of an engine that wrote to this data, so that a
    /// property this data does not hold gives that engine's reason.
    pub fn add_missing_property_source(&mut self, source: Arc<MissingPropertySource>) {
        self.missing_property_sources.push(source);
    }

    /// Mark this data as written for a flow data whose cloud request failed,
    /// so every property it does not hold gives
    /// [`MissingPropertyReason::CloudRequestFailed`]. A cloud engine calls this
    /// when the cloud request engine left no response.
    pub fn mark_cloud_request_failed(&mut self) {
        self.cloud_request_failed = true;
    }

    /// True when a cloud engine marked this data with
    /// [`mark_cloud_request_failed`](Self::mark_cloud_request_failed).
    pub fn cloud_request_failed(&self) -> bool {
        self.cloud_request_failed
    }

    /// Stop a read of a property this data does not hold logging a record.
    /// The read still gives the reason. An engine calls this for data that
    /// .NET builds with no logger, such as a cloud hardware profile.
    pub fn stop_logging_missing_properties(&mut self) {
        self.log_missing_properties = false;
    }

    /// Why `property_name` is missing. When the cloud request failed that is
    /// the reason, naming the first engine key. Otherwise it is from the
    /// sources in the order they were added, the first reason other than
    /// [`MissingPropertyReason::Unknown`] deciding. [`None`] when no source
    /// was added and the cloud request did not fail.
    ///
    /// The bag is not consulted, so ask only about a property this data does
    /// not hold.
    pub fn missing_property_reason(&self, property_name: &str) -> Option<MissingPropertyResult> {
        if self.cloud_request_failed {
            return Some(cloud_request_failed(
                property_name,
                self.engine_keys.first().map(String::as_str),
            ));
        }
        first_known_reason(
            self.missing_property_sources
                .iter()
                .map(|source| source.reason(property_name)),
        )
    }

    /// Why `property_name` is missing, as
    /// [`missing_property_reason`](Self::missing_property_reason) gives it,
    /// logging a record that names `data_type` when there is a reason.
    ///
    /// Call this for a read of a property the data does not hold, so each
    /// such read logs once, as .NET's aspect data does. A failed cloud request
    /// logs at info level, because the cloud request engine has already
    /// recorded the failure and one failure leads to many such reads, and
    /// every other reason logs a warning. Nothing is logged after
    /// [`stop_logging_missing_properties`](Self::stop_logging_missing_properties).
    pub fn missing_property(
        &self,
        property_name: &str,
        data_type: &str,
    ) -> Option<MissingPropertyResult> {
        let missing = self.missing_property_reason(property_name)?;
        if !self.log_missing_properties {
            return Some(missing);
        }
        if missing.reason == MissingPropertyReason::CloudRequestFailed {
            log::info!(
                "Property '{property_name}' missing from aspect data '{data_type}' because \
                 the upstream cloud request failed for this request ({:?}). See \
                 FlowData::errors for details.",
                missing.reason
            );
        } else {
            log::warn!(
                "Property '{property_name}' missing from aspect data '{data_type}'. {:?}",
                missing.reason
            );
        }
        Some(missing)
    }

    /// True when an engine added a [`MissingPropertySource`] or the cloud
    /// request failed, so a property the data does not hold is known to be
    /// missing.
    pub fn knows_missing_properties(&self) -> bool {
        self.cloud_request_failed || !self.missing_property_sources.is_empty()
    }
}

impl ElementData for AspectDataBase {
    /// The value of a property in the bag. For a property the bag does not
    /// hold, the error carries the reason it is missing when a source was
    /// added, and a warning is logged, and otherwise the bag's own
    /// explanation.
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
        self.values.get(name).map_err(|absent| {
            self.missing_property(name, "AspectDataBase")
                .map_or(absent, |missing| NoValueError::new(missing.description))
        })
    }

    /// Whether the bag holds the property. Unless
    /// [`knows_missing_properties`](Self::knows_missing_properties) it answers
    /// `true`, because a cloud engine leaves a property with no value out of
    /// the bag, so the bag alone cannot tell that property from one the data
    /// lacks.
    fn has_property(&self, name: &str) -> bool {
        !self.knows_missing_properties() || self.values.has_property(name)
    }

    fn keys(&self) -> Vec<String> {
        self.values.keys()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl AspectData for AspectDataBase {
    fn engine_keys(&self) -> &[String] {
        &self.engine_keys
    }

    fn cache_hit(&self) -> bool {
        self.cache_hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AspectPropertyMetaData, EngineDeployment, MissingPropertyReason};
    use fiftyone_pipeline_core::PropertyValueType;

    #[test]
    fn property_bag_round_trips() {
        let data = AspectDataBase::new("device")
            .set("IsMobile", true)
            .set("PlatformName", "iOS");
        assert_eq!(data.get("ismobile").unwrap().as_bool(), Some(true));
        assert_eq!(data.get("PlatformName").unwrap().as_str(), Some("iOS"));
        assert!(data.get("Unknown").is_err());
    }

    #[test]
    fn tracks_engines_and_cache_hit() {
        let mut data = AspectDataBase::new("device");
        assert_eq!(data.engine_keys(), ["device"]);
        assert!(!data.cache_hit());

        data.add_engine_key("device");
        assert_eq!(data.engine_keys(), ["device"], "duplicate ignored");
        data.add_engine_key("location");
        assert_eq!(data.engine_keys(), ["device", "location"]);

        data.set_cache_hit();
        assert!(data.cache_hit());
    }

    fn source(element_data_key: &str, tier: &str) -> Arc<MissingPropertySource> {
        Arc::new(
            MissingPropertySource::new(
                element_data_key,
                "TestEngine",
                EngineDeployment::OnPremise,
                tier,
                vec![AspectPropertyMetaData::new(
                    "IsMobile",
                    element_data_key,
                    PropertyValueType::Bool,
                )
                .with_data_tiers(["Premium"])],
            )
            .with_typed_properties(|name: &str| name.eq_ignore_ascii_case("IsMobile")),
        )
    }

    #[test]
    fn without_a_source_the_bag_explains_a_missing_property() {
        let data = AspectDataBase::new("device").set("PlatformName", "iOS");
        assert_eq!(data.missing_property_reason("IsMobile"), None);
        assert_eq!(
            data.get("IsMobile").unwrap_err().message,
            "No value for property 'IsMobile'."
        );
    }

    #[test]
    fn a_source_explains_a_missing_property() {
        let mut data = AspectDataBase::new("device").set("PlatformName", "iOS");
        let source = source("device", "Lite");
        data.add_missing_property_source(Arc::clone(&source));

        let missing = data.missing_property_reason("IsMobile").unwrap();
        assert_eq!(missing, source.reason("IsMobile"));
        assert_eq!(
            missing.reason,
            MissingPropertyReason::DataFileUpgradeRequired
        );
        assert_eq!(
            data.get("IsMobile").unwrap_err().message,
            missing.description
        );
        assert_eq!(
            data.get("NotAProperty").unwrap_err().message,
            source.reason("NotAProperty").description
        );
        // A property the bag holds is still read from it.
        assert_eq!(data.get("PlatformName").unwrap().as_str(), Some("iOS"));
    }

    #[test]
    fn the_first_source_with_a_known_reason_decides() {
        let mut data = AspectDataBase::new("device");
        let unknown = Arc::new(MissingPropertySource::new(
            "device",
            "FirstEngine",
            EngineDeployment::OnPremise,
            "Lite",
            Vec::new(),
        ));
        let known = source("device", "Lite");
        let later = Arc::new(MissingPropertySource::new(
            "device",
            "LaterEngine",
            EngineDeployment::Cloud,
            "",
            Vec::new(),
        ));
        assert_ne!(
            later.reason("IsMobile").reason,
            known.reason("IsMobile").reason,
            "the later source must give a different known reason"
        );
        assert_ne!(
            later.reason("IsMobile").reason,
            MissingPropertyReason::Unknown
        );
        data.add_missing_property_source(unknown);
        data.add_missing_property_source(Arc::clone(&known));
        data.add_missing_property_source(Arc::clone(&later));
        assert_eq!(
            data.missing_property_reason("IsMobile"),
            Some(known.reason("IsMobile"))
        );
        // Neither earlier source knows why NotAProperty is missing, so the
        // later one decides.
        assert_eq!(
            known.reason("NotAProperty").reason,
            MissingPropertyReason::Unknown
        );
        assert_eq!(
            data.missing_property_reason("NotAProperty"),
            Some(later.reason("NotAProperty"))
        );
    }

    #[test]
    fn printing_a_source_gives_the_number_of_properties_not_the_properties() {
        let printed = format!("{:?}", source("device", "Lite"));
        assert!(printed.contains("properties: 1"), "{printed}");
        assert!(!printed.contains("IsMobile"), "{printed}");
    }

    #[test]
    fn a_source_lets_the_data_say_what_it_holds() {
        let mut data = AspectDataBase::new("device").set("PlatformName", "iOS");
        assert!(!data.knows_missing_properties());
        assert!(data.has_property("NotAProperty"));

        data.add_missing_property_source(source("device", "Lite"));
        assert!(data.knows_missing_properties());
        assert!(data.has_property("platformname"));
        assert!(!data.has_property("IsMobile"));
        assert!(!data.has_property("NotAProperty"));
    }

    #[test]
    fn missing_property_gives_the_same_reason() {
        let mut data = AspectDataBase::new("device");
        assert_eq!(data.missing_property("IsMobile", "AspectDataBase"), None);
        data.add_missing_property_source(source("device", "Lite"));
        assert_eq!(
            data.missing_property("IsMobile", "AspectDataBase"),
            data.missing_property_reason("IsMobile")
        );
    }

    /// The description of a property of data whose cloud request failed.
    const IS_MOBILE_FAILED: &str = "Property 'IsMobile' not found in data for element \
         'device'. This is because an upstream cloud request failed, so no data was \
         available to populate the property.";

    #[test]
    fn a_failed_cloud_request_explains_every_property_the_data_lacks() {
        let mut data = AspectDataBase::new("device").set("PlatformName", "iOS");
        assert!(!data.cloud_request_failed());
        data.mark_cloud_request_failed();
        assert!(data.cloud_request_failed());
        assert!(data.knows_missing_properties());

        let missing = data.missing_property_reason("IsMobile").unwrap();
        assert_eq!(missing.reason, MissingPropertyReason::CloudRequestFailed);
        assert_eq!(missing.description, IS_MOBILE_FAILED);
        assert_eq!(data.get("IsMobile").unwrap_err().message, IS_MOBILE_FAILED);
        assert_eq!(
            data.missing_property("IsMobile", "AspectDataBase"),
            Some(missing)
        );
        assert!(!data.has_property("IsMobile"));
        // A property the bag holds is still read from it.
        assert!(data.has_property("PlatformName"));
        assert_eq!(data.get("PlatformName").unwrap().as_str(), Some("iOS"));
    }

    #[test]
    fn a_failed_cloud_request_comes_before_any_source() {
        // .NET's cloud missing property service looks at the mark first.
        let mut data = AspectDataBase::new("device");
        data.add_missing_property_source(source("device", "Lite"));
        data.mark_cloud_request_failed();
        assert_eq!(
            data.missing_property_reason("IsMobile").unwrap().reason,
            MissingPropertyReason::CloudRequestFailed
        );
        assert_eq!(
            data.missing_property_reason("NotAProperty").unwrap().reason,
            MissingPropertyReason::CloudRequestFailed
        );
    }

    #[test]
    fn a_failed_cloud_request_names_the_first_engine() {
        let mut data = AspectDataBase::new("location");
        data.add_engine_key("device");
        data.mark_cloud_request_failed();
        assert!(data
            .missing_property_reason("Town")
            .unwrap()
            .description
            .starts_with("Property 'Town' not found in data for element 'location'. "));
        assert!(
            data.clone().cloud_request_failed(),
            "a clone keeps the mark"
        );
    }

    #[test]
    fn a_clone_keeps_its_sources() {
        let mut data = AspectDataBase::new("device");
        data.add_missing_property_source(source("device", "Lite"));
        assert!(data.clone().missing_property_reason("IsMobile").is_some());
    }

    #[test]
    fn usable_through_trait_objects() {
        let mut data = AspectDataBase::new("device");
        data.insert("k", 1i64);
        let as_element: &dyn ElementData = &data;
        assert_eq!(as_element.get("k").unwrap().as_integer(), Some(1));
        let as_aspect: &dyn AspectData = &data;
        assert_eq!(as_aspect.engine_keys(), ["device"]);
    }
}
