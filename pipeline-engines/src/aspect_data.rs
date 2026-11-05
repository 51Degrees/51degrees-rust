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
//! and logs a warning for each read of such a property.

use std::any::Any;
use std::sync::Arc;

use fiftyone_pipeline_core::{ElementData, MapElementData, NoValueError, PropertyValue};

use crate::missing_property::{first_known_reason, MissingPropertyResult, MissingPropertySource};

/// Element data produced by an aspect engine.
///
/// This trait extends the core [`ElementData`] with aspect-only state. It adds
/// the list of engines (by data key) that contributed to the data, and whether
/// the data was served from a cache hit rather than freshly processed.
///
/// Implementations are usually built by embedding an [`AspectDataBase`] and
/// delegating to it, so the trait has no required methods beyond the two
/// accessors below. Like [`ElementData`], it is `Send` but not `Sync`: aspect
/// data is accessed from a single thread per request.
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
/// [`MapElementData`]), the contributing engine keys, the cache-hit flag and
/// the [`MissingPropertySource`] of each engine that added one.
/// The lazy-loading task list lives on the engine wrapper rather than the data
/// (see [`crate::AspectEngineBase`]).
#[derive(Debug, Clone)]
pub struct AspectDataBase {
    values: MapElementData,
    engine_keys: Vec<String>,
    cache_hit: bool,
    missing_property_sources: Vec<Arc<MissingPropertySource>>,
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

    /// Why `property_name` is missing, from the sources in the order they were
    /// added, the first reason other than
    /// [`MissingPropertyReason::Unknown`](crate::MissingPropertyReason::Unknown)
    /// deciding. [`None`] when no source was added.
    ///
    /// The bag is not consulted, so ask only about a property this data does
    /// not hold.
    pub fn missing_property_reason(&self, property_name: &str) -> Option<MissingPropertyResult> {
        first_known_reason(
            self.missing_property_sources
                .iter()
                .map(|source| source.reason(property_name)),
        )
    }

    /// Why `property_name` is missing, as
    /// [`missing_property_reason`](Self::missing_property_reason) gives it,
    /// logging a warning that names `data_type` when there is a reason.
    ///
    /// Call this for a read of a property the data does not hold, so each
    /// such read logs once, as .NET's aspect data does.
    pub fn missing_property(
        &self,
        property_name: &str,
        data_type: &str,
    ) -> Option<MissingPropertyResult> {
        let missing = self.missing_property_reason(property_name)?;
        log::warn!(
            "Property '{property_name}' missing from aspect data '{data_type}'. {:?}",
            missing.reason
        );
        Some(missing)
    }

    /// True when an engine added a [`MissingPropertySource`], so a property
    /// the data does not hold is known to be missing.
    pub fn has_missing_property_sources(&self) -> bool {
        !self.missing_property_sources.is_empty()
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

    /// Whether the bag holds the property. Without a
    /// [`MissingPropertySource`] it answers `true`, because a cloud engine
    /// leaves a property with no value out of the bag, so the bag alone
    /// cannot tell that property from one the data lacks.
    fn has_property(&self, name: &str) -> bool {
        !self.has_missing_property_sources() || self.values.has_property(name)
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
        data.add_missing_property_source(unknown);
        data.add_missing_property_source(Arc::clone(&known));
        assert_eq!(
            data.missing_property_reason("IsMobile"),
            Some(known.reason("IsMobile"))
        );
        assert_eq!(
            data.missing_property_reason("NotAProperty").unwrap().reason,
            MissingPropertyReason::Unknown
        );
    }

    #[test]
    fn a_source_lets_the_data_say_what_it_holds() {
        let mut data = AspectDataBase::new("device").set("PlatformName", "iOS");
        assert!(!data.has_missing_property_sources());
        assert!(data.has_property("NotAProperty"));

        data.add_missing_property_source(source("device", "Lite"));
        assert!(data.has_missing_property_sources());
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
