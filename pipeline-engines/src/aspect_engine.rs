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

//! The aspect engine trait and a base that cuts the boilerplate.
//!
//! An [`AspectEngine`] is a [`FlowElement`] that produces [`AspectData`]. On top
//! of a flow element it adds the things every 51Degrees engine needs: a data
//! source tier, aspect-aware property metadata, missing-property reason
//! resolution, an optional results cache, and optional lazy loading.
//!
//! # The cache
//!
//! Element data is not `Clone`, so the cache cannot store `dyn AspectData`
//! directly. Instead an engine chooses a concrete, `Send + Sync + Clone` aspect
//! data type `D` and caches that. [`AspectEngineBase`] holds a
//! [`DataKeyedCache<D>`], keyed by the engine's evidence, exactly as the caching
//! crate's documentation requires. On a cache hit the cached `D` is cloned back
//! into the flow data, so equivalent requests skip the engine's work.

use std::sync::Arc;

use fiftyone_pipeline_core::{
    DataKey, Error, EvidenceKeyFilter, FlowData, FlowElement, MissingPropertyReason,
    PropertyMetaData, Result, TypedKey,
};

use fiftyone_caching::{CacheBuilder, DataKeyedCache, PutCache};

use crate::aspect_data::AspectData;
use crate::aspect_property_metadata::AspectPropertyMetaData;
use crate::lazy_loading::LazyLoadingConfiguration;
use crate::missing_property::{resolve, EngineDeployment, EngineView, MissingPropertyResult};

/// A flow element that produces aspect data for a single 51Degrees aspect.
///
/// It is a [`FlowElement`], so it slots into a pipeline like any other element,
/// plus the aspect-specific surface described below. The aspect property
/// metadata is
/// returned alongside (not instead of) the core
/// [`FlowElement::properties`], so the engine still satisfies the core contract
/// while exposing the richer aspect view through
/// [`AspectEngine::aspect_properties`].
pub trait AspectEngine: FlowElement {
    /// The tier of the engine's current data source, for example `Lite`,
    /// `Premium` or `Enterprise`. Cloud engines return an empty string, as they
    /// have no tier.
    fn data_source_tier(&self) -> &str;

    /// The deployment kind of this engine. Drives which missing-property rules
    /// apply. Defaults to [`EngineDeployment::OnPremise`]; cloud engines
    /// override it to [`EngineDeployment::Cloud`].
    fn deployment(&self) -> EngineDeployment {
        EngineDeployment::OnPremise
    }

    /// The aspect-aware metadata for the properties this engine populates.
    ///
    /// This is the richer view of the same properties the core
    /// [`FlowElement::properties`] returns, carrying the description and data
    /// tiers.
    fn aspect_properties(&self) -> &[AspectPropertyMetaData];

    /// True once the engine's property metadata is populated. A cloud engine
    /// may answer `false` before its first request, when its metadata is loaded
    /// lazily from the cloud. On-premise engines load metadata when the data
    /// file is loaded, so they answer `true`.
    fn has_loaded_properties(&self) -> bool {
        true
    }

    /// True when a cloud engine has no property metadata because its request
    /// for the property list failed, so a missing property's reason is
    /// [`MissingPropertyReason::CloudRequestFailed`]. Defaults to `false`.
    fn property_list_request_failed(&self) -> bool {
        false
    }

    /// The lazy-loading configuration, if lazy loading is enabled for this
    /// engine.
    fn lazy_loading(&self) -> Option<&LazyLoadingConfiguration> {
        None
    }

    /// True if the engine records, on each aspect data, whether the result came
    /// from a cache hit.
    fn records_cache_hits(&self) -> bool {
        false
    }

    /// The name of this engine's type, which the message for a data file
    /// upgrade names. Defaults to the name of the implementing type without
    /// its path.
    fn engine_type_name(&self) -> &str {
        short_type_name(std::any::type_name::<Self>())
    }

    /// Whether this engine's data type has a typed accessor for the property,
    /// matched ignoring case, counting every accessor the type offers,
    /// including those of the shared data type for its aspect.
    ///
    /// For an on-premise engine, a property with an accessor that the engine's
    /// metadata does not list is one another data file supplies, so the
    /// missing property rules give a data file upgrade as the reason. The
    /// rules do not ask a cloud engine. Defaults to `false`.
    fn has_typed_property(&self, _property_name: &str) -> bool {
        false
    }

    /// Resolve why `property_name` is missing from this engine's results.
    ///
    /// Applies the rules in [`crate::MissingPropertyService`] using this
    /// engine's deployment, data tier, aspect metadata, type name, typed
    /// accessors and [`AspectEngine::property_list_request_failed`]. The
    /// default implementation is correct for every engine, so
    /// an engine rarely overrides it. One that must do some work first, such
    /// as loading its metadata, can override it and then call
    /// [`default_missing_property_reason`].
    fn missing_property_reason(&self, property_name: &str) -> MissingPropertyResult {
        default_missing_property_reason(self, property_name)
    }

    /// Build a core [`Error::PropertyMissing`] for `property_name`, using
    /// [`AspectEngine::missing_property_reason`] to fill in the reason.
    ///
    /// Engines and their callers use this to turn "the property is not here"
    /// into the canonical pipeline error.
    fn property_missing_error(&self, property_name: &str) -> Error {
        let result = self.missing_property_reason(property_name);
        Error::PropertyMissing {
            property: property_name.to_owned(),
            element_data_key: self.data_key().to_owned(),
            reason: result.reason,
        }
    }
}

/// A reusable base that wires the cache and centralises the cached-process flow.
///
/// An engine embeds one of these. It is generic over the engine's concrete
/// aspect data type `D`, which must be `Send + Sync + Clone` so it can live in
/// the cache.
/// The engine supplies its own evidence key filter and the
/// [`TypedKey<D>`] under which its data is stored, then calls
/// [`AspectEngineBase::process_with_cache`] from its `FlowElement::process`,
/// passing a closure that does the real work on a cache miss.
///
/// # Example
///
/// ```
/// use std::sync::Arc;
/// use fiftyone_pipeline_core::{
///     ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist,
///     FlowData, FlowElement, MapElementData, NoValueError, Pipeline,
///     PropertyMetaData, PropertyValue, PropertyValueType, Result, TypedKey,
/// };
/// use fiftyone_pipeline_engines::{
///     AspectData, AspectDataBase, AspectEngine, AspectEngineBase,
///     AspectPropertyMetaData,
/// };
/// use std::any::Any;
///
/// // Concrete aspect data wrapping the reusable base. It must be Clone so it
/// // can be cached.
/// #[derive(Clone)]
/// struct DeviceData(AspectDataBase);
/// impl ElementData for DeviceData {
///     fn get(&self, n: &str) -> std::result::Result<PropertyValue, NoValueError> { self.0.get(n) }
///     fn keys(&self) -> Vec<String> { self.0.keys() }
///     fn as_any(&self) -> &dyn Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn Any { self }
/// }
/// impl AspectData for DeviceData {
///     fn engine_keys(&self) -> &[String] { self.0.engine_keys() }
///     fn cache_hit(&self) -> bool { self.0.cache_hit() }
/// }
///
/// struct DeviceEngine {
///     base: AspectEngineBase<DeviceData>,
///     filter: EvidenceKeyFilterWhitelist,
///     properties: Vec<PropertyMetaData>,
///     aspect_properties: Vec<AspectPropertyMetaData>,
/// }
/// impl DeviceEngine {
///     const KEY: TypedKey<DeviceData> = TypedKey::new("device");
///     fn new() -> Self {
///         DeviceEngine {
///             base: AspectEngineBase::new().with_cache(1000, 1),
///             filter: EvidenceKeyFilterWhitelist::new(["header.user-agent"]),
///             properties: vec![PropertyMetaData::new("IsMobile", "device", PropertyValueType::Bool)],
///             aspect_properties: vec![AspectPropertyMetaData::new("IsMobile", "device", PropertyValueType::Bool)],
///         }
///     }
/// }
/// impl FlowElement for DeviceEngine {
///     fn process(&self, data: &mut FlowData) -> Result<()> {
///         self.base.process_with_cache(data, &self.filter, Self::KEY, |data| {
///             let ua = data.evidence().get("header.user-agent").unwrap_or("");
///             Ok(DeviceData(AspectDataBase::new("device").set("IsMobile", ua.contains("Mobile"))))
///         })
///     }
///     fn data_key(&self) -> &str { "device" }
///     fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter { &self.filter }
///     fn properties(&self) -> &[PropertyMetaData] { &self.properties }
/// }
/// impl AspectEngine for DeviceEngine {
///     fn data_source_tier(&self) -> &str { "Lite" }
///     fn aspect_properties(&self) -> &[AspectPropertyMetaData] { &self.aspect_properties }
/// }
///
/// let pipeline = Pipeline::builder().add_element(Arc::new(DeviceEngine::new())).build().unwrap();
/// let mut data = pipeline.create_flow_data_with(
///     Evidence::builder().add("header.user-agent", "Mobile Safari").build(),
/// );
/// data.process().unwrap();
/// assert_eq!(data.get(DeviceEngine::KEY).unwrap().get("IsMobile").unwrap().as_bool(), Some(true));
/// ```
pub struct AspectEngineBase<D>
where
    D: AspectData + Clone + Send + Sync + 'static,
{
    cache: Option<DataKeyedCache<D>>,
    record_cache_hits: bool,
    lazy_loading: Option<LazyLoadingConfiguration>,
}

impl<D> AspectEngineBase<D>
where
    D: AspectData + Clone + Send + Sync + 'static,
{
    /// Create a base with no cache, cache-hit recording disabled and lazy
    /// loading disabled. Use the `with_*` methods to opt into each feature.
    pub fn new() -> Self {
        AspectEngineBase {
            cache: None,
            record_cache_hits: false,
            lazy_loading: None,
        }
    }

    /// Attach a results cache of the given `size` (total entries) and
    /// `concurrency` (shard count), keyed by the evidence the engine reads.
    ///
    /// The filter used to derive cache keys is the one passed to
    /// [`AspectEngineBase::process_with_cache`] on each request, so it is always
    /// the engine's own evidence key filter.
    pub fn with_cache(mut self, size: usize, concurrency: usize) -> Self {
        let builder = CacheBuilder::new().size(size).concurrency(concurrency);
        // The filter is supplied per request, so a placeholder whitelist is
        // installed here and overwritten on first use. The cache only consults
        // its filter lazily through `process_with_cache`, which always rebuilds
        // the key with the engine's live filter, so this is never read.
        let placeholder: Arc<dyn EvidenceKeyFilter> = Arc::new(
            fiftyone_pipeline_core::EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        );
        self.cache = Some(DataKeyedCache::new(builder, placeholder));
        self
    }

    /// Attach a results cache built from an explicit [`CacheBuilder`] and the
    /// engine's evidence key filter, for callers that want full control over
    /// the cache configuration and keying.
    pub fn with_cache_from(
        mut self,
        builder: CacheBuilder,
        filter: Arc<dyn EvidenceKeyFilter>,
    ) -> Self {
        self.cache = Some(DataKeyedCache::new(builder, filter));
        self
    }

    /// Attach a results cache backed by an injected [`PutCache`], instead of the
    /// built-in in-process [`fiftyone_caching::LruCache`].
    ///
    /// This lets a consumer back the engine's results cache with its own store,
    /// for example a durable external key/value store so cache hits persist
    /// across requests (useful on a request-scoped edge runtime where a
    /// process-local cache rarely survives between requests). The backing is
    /// shared as an [`Arc`] and must use the same `Send + Sync + Clone` value
    /// type the engine caches.
    ///
    /// As with [`AspectEngineBase::with_cache`], the real evidence key filter is
    /// supplied per request through [`AspectEngineBase::process_with_cache`],
    /// which derives the [`DataKey`] with the engine's live filter, so a
    /// placeholder filter is installed here and never read for keying.
    pub fn with_cache_backing(mut self, backing: Arc<dyn PutCache<DataKey, D>>) -> Self {
        let placeholder: Arc<dyn EvidenceKeyFilter> = Arc::new(
            fiftyone_pipeline_core::EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        );
        self.cache = Some(DataKeyedCache::with_backing(backing, placeholder));
        self
    }

    /// Enable recording, on each cached aspect data, whether it came from a
    /// cache hit. Returns `self` for chaining.
    pub fn record_cache_hits(mut self, enabled: bool) -> Self {
        self.record_cache_hits = enabled;
        self
    }

    /// Enable lazy loading with the supplied configuration. Returns `self` for
    /// chaining.
    pub fn with_lazy_loading(mut self, config: LazyLoadingConfiguration) -> Self {
        self.lazy_loading = Some(config);
        self
    }

    /// The configured lazy-loading settings, if any.
    pub fn lazy_loading(&self) -> Option<&LazyLoadingConfiguration> {
        self.lazy_loading.as_ref()
    }

    /// True if cache-hit recording is enabled.
    pub fn records_cache_hits(&self) -> bool {
        self.record_cache_hits
    }

    /// True if a results cache is attached.
    pub fn has_cache(&self) -> bool {
        self.cache.is_some()
    }

    /// The number of entries currently held in the results cache, or `None` if
    /// no cache is attached.
    pub fn cache_len(&self) -> Option<usize> {
        self.cache.as_ref().map(|c| c.len())
    }

    /// Run the engine for `data`, consulting and populating the results cache.
    ///
    /// This centralises the cached-process flow:
    ///
    /// 1. Build the cache key from `data` using `filter` (the engine's evidence
    ///    key filter).
    /// 2. On a cache hit, clone the cached aspect data into the flow data under
    ///    `key`, optionally flagging the cache hit, and return.
    /// 3. On a miss, call `produce` to do the real work, store the resulting
    ///    aspect data under `key`, and write a clone into the cache.
    ///
    /// `produce` is only invoked on a miss. It is given the flow data so it can
    /// read evidence and earlier element data, and returns the engine's
    /// concrete aspect data on success.
    pub fn process_with_cache<F>(
        &self,
        data: &mut FlowData,
        filter: &dyn EvidenceKeyFilter,
        key: TypedKey<D>,
        produce: F,
    ) -> Result<()>
    where
        F: FnOnce(&FlowData) -> Result<D>,
    {
        // Compute the key once so the lookup and the later write agree.
        let cache_key = self.cache.as_ref().map(|_| data.generate_key(filter));

        if let (Some(cache), Some(cache_key)) = (self.cache.as_ref(), cache_key.as_ref()) {
            if let Some(mut hit) = cache.get_by_key(cache_key) {
                if self.record_cache_hits {
                    // Flag the cache hit on the concrete data through its
                    // backing. This is best-effort: data types that do not
                    // expose a mutable base simply skip the flag.
                    flag_cache_hit(&mut hit);
                }
                data.get_or_add(key, || hit)?;
                return Ok(());
            }
        }

        // Cache miss (or no cache): do the real work.
        let produced = produce(data)?;

        // Write a clone into the cache before moving the value into the flow
        // data, so the cache holds an independent copy.
        if let (Some(cache), Some(cache_key)) = (self.cache.as_ref(), cache_key) {
            cache.put_by_key(cache_key, produced.clone());
        }

        data.get_or_add(key, || produced)?;
        Ok(())
    }

    /// Clear the results cache, if one is attached.
    pub fn clear_cache(&self) {
        if let Some(cache) = self.cache.as_ref() {
            cache.clear();
        }
    }
}

impl<D> Default for AspectEngineBase<D>
where
    D: AspectData + Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        AspectEngineBase::new()
    }
}

/// Set the cache-hit flag on cloned aspect data when its backing is an
/// [`crate::AspectDataBase`].
///
/// The cache stores the engine's concrete data type, which may wrap an
/// [`crate::AspectDataBase`]. Because the wrapper is opaque, the flag is set by
/// downcasting through [`fiftyone_pipeline_core::ElementData::as_any_mut`] to
/// the base when the concrete type *is* the base. Engines that wrap the base in
/// a newtype and want the flag set should call
/// [`crate::AspectDataBase::set_cache_hit`] themselves on a hit; this best-
/// effort path covers the common case of caching the base directly.
fn flag_cache_hit<D: AspectData>(data: &mut D) {
    if let Some(base) = data
        .as_any_mut()
        .downcast_mut::<crate::aspect_data::AspectDataBase>()
    {
        base.set_cache_hit();
    }
}

/// Convenience alias used in documentation and downstream crates for the
/// canonical no-reflection metadata pairing. The core
/// [`PropertyMetaData`] and the aspect [`AspectPropertyMetaData`] always travel
/// together, the former satisfying [`FlowElement::properties`] and the latter
/// [`AspectEngine::aspect_properties`].
pub type EnginePropertyPair = (PropertyMetaData, AspectPropertyMetaData);

/// Build the canonical property pair from an [`AspectPropertyMetaData`], so an
/// engine can declare each property once and expose both views.
pub fn property_pair(aspect: AspectPropertyMetaData) -> EnginePropertyPair {
    (aspect.core().clone(), aspect)
}

/// The last segment of a type's path, without generic arguments, for example
/// `DeviceDetectionOnPremiseEngine` from
/// `fiftyone_device_detection_onpremise::engine::DeviceDetectionOnPremiseEngine`.
fn short_type_name(full: &str) -> &str {
    let path = full.split('<').next().unwrap_or(full);
    path.rsplit("::").next().unwrap_or(path)
}

/// Resolve why `property_name` is missing from `engine`'s results with the
/// rules [`AspectEngine::missing_property_reason`] applies by default, using
/// the engine's deployment, data tier, aspect metadata, type name, typed
/// accessors and [`AspectEngine::property_list_request_failed`].
pub fn default_missing_property_reason<E: AspectEngine + ?Sized>(
    engine: &E,
    property_name: &str,
) -> MissingPropertyResult {
    let has_typed_property = |name: &str| engine.has_typed_property(name);
    let view = EngineView {
        element_data_key: engine.data_key(),
        type_name: engine.engine_type_name(),
        deployment: engine.deployment(),
        data_source_tier: engine.data_source_tier(),
        properties_loaded: engine.has_loaded_properties(),
        property_list_request_failed: engine.property_list_request_failed(),
        properties: engine.aspect_properties(),
        has_typed_property: Some(&has_typed_property),
    };
    resolve(property_name, &view)
}

/// Why `property_name` is missing from `engine`'s results, for callers holding
/// a `&dyn AspectEngine` that need only the reason.
///
/// Calls [`AspectEngine::missing_property_reason`], so an engine's override
/// applies. [`default_missing_property_reason`] applies the default rules
/// without the trait method.
pub fn engine_missing_property_reason(
    engine: &dyn AspectEngine,
    property_name: &str,
) -> MissingPropertyReason {
    engine.missing_property_reason(property_name).reason
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aspect_data::AspectDataBase;
    use fiftyone_pipeline_core::{
        ElementData, Evidence, EvidenceKeyFilterWhitelist, Pipeline, PropertyValueType,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    // A concrete aspect data that simply wraps the base.
    type DeviceData = AspectDataBase;

    struct DeviceEngine {
        base: AspectEngineBase<DeviceData>,
        filter: EvidenceKeyFilterWhitelist,
        properties: Vec<PropertyMetaData>,
        aspect_properties: Vec<AspectPropertyMetaData>,
        calls: Arc<AtomicUsize>,
        tier: String,
    }

    impl DeviceEngine {
        const KEY: TypedKey<DeviceData> = TypedKey::new("device");

        fn from_base(base: AspectEngineBase<DeviceData>, calls: Arc<AtomicUsize>) -> Self {
            DeviceEngine {
                base,
                filter: EvidenceKeyFilterWhitelist::new(["header.user-agent"]),
                properties: vec![PropertyMetaData::new(
                    "IsMobile",
                    "device",
                    PropertyValueType::Bool,
                )],
                aspect_properties: vec![AspectPropertyMetaData::new(
                    "IsMobile",
                    "device",
                    PropertyValueType::Bool,
                )
                .with_data_tiers(["Lite"])],
                calls,
                tier: "Lite".to_owned(),
            }
        }

        fn new(with_cache: bool, calls: Arc<AtomicUsize>) -> Self {
            let base = if with_cache {
                AspectEngineBase::new()
                    .with_cache(100, 1)
                    .record_cache_hits(true)
            } else {
                AspectEngineBase::new()
            };
            Self::from_base(base, calls)
        }
    }

    impl FlowElement for DeviceEngine {
        fn process(&self, data: &mut FlowData) -> Result<()> {
            self.base
                .process_with_cache(data, &self.filter, Self::KEY, |data| {
                    self.calls.fetch_add(1, Ordering::SeqCst);
                    let ua = data.evidence().get("header.user-agent").unwrap_or("");
                    Ok(AspectDataBase::new("device").set("IsMobile", ua.contains("Mobile")))
                })
        }
        fn data_key(&self) -> &str {
            "device"
        }
        fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
            &self.filter
        }
        fn properties(&self) -> &[PropertyMetaData] {
            &self.properties
        }
    }

    impl AspectEngine for DeviceEngine {
        fn data_source_tier(&self) -> &str {
            &self.tier
        }
        fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
            &self.aspect_properties
        }
        fn records_cache_hits(&self) -> bool {
            self.base.records_cache_hits()
        }
    }

    fn pipeline_with(engine: DeviceEngine) -> Arc<Pipeline> {
        Pipeline::builder()
            .add_element(Arc::new(engine))
            .build()
            .unwrap()
    }

    #[test]
    fn produces_aspect_data() {
        let calls = Arc::new(AtomicUsize::new(0));
        let pipeline = pipeline_with(DeviceEngine::new(false, calls.clone()));
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("header.user-agent", "Mobile X")
                .build(),
        );
        data.process().unwrap();
        let device = data.get(DeviceEngine::KEY).unwrap();
        assert_eq!(device.get("IsMobile").unwrap().as_bool(), Some(true));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!device.cache_hit());
    }

    #[test]
    fn cache_hit_skips_processing_and_is_flagged() {
        let calls = Arc::new(AtomicUsize::new(0));
        let pipeline = pipeline_with(DeviceEngine::new(true, calls.clone()));

        // First request: a miss, the engine runs.
        let mut first = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("header.user-agent", "Mobile X")
                .build(),
        );
        first.process().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!first.get(DeviceEngine::KEY).unwrap().cache_hit());

        // Second equivalent request: a hit, the engine does not run again.
        let mut second = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("header.user-agent", "Mobile X")
                .build(),
        );
        second.process().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "engine not re-run on hit");
        let device = second.get(DeviceEngine::KEY).unwrap();
        assert_eq!(device.get("IsMobile").unwrap().as_bool(), Some(true));
        assert!(device.cache_hit(), "hit flagged");
    }

    #[test]
    fn different_evidence_is_a_miss() {
        let calls = Arc::new(AtomicUsize::new(0));
        let pipeline = pipeline_with(DeviceEngine::new(true, calls.clone()));
        for ua in ["Mobile A", "Desktop B"] {
            let mut data = pipeline
                .create_flow_data_with(Evidence::builder().add("header.user-agent", ua).build());
            data.process().unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn injected_backing_serves_hits() {
        use fiftyone_caching::{Cache, PutCache};
        use std::collections::HashMap;
        use std::sync::Mutex;

        // A non-LRU PutCache test double backed by a plain map, used to prove the
        // engine reads and writes through an injected backing rather than an
        // LruCache.
        struct MapCache {
            map: Mutex<HashMap<DataKey, DeviceData>>,
        }
        impl Cache<DataKey, DeviceData> for MapCache {
            fn get(&self, key: &DataKey) -> Option<DeviceData> {
                self.map.lock().unwrap().get(key).cloned()
            }
            fn len(&self) -> usize {
                self.map.lock().unwrap().len()
            }
        }
        impl PutCache<DataKey, DeviceData> for MapCache {
            fn put(&self, key: DataKey, value: DeviceData) {
                self.map.lock().unwrap().insert(key, value);
            }
        }

        let backing = Arc::new(MapCache {
            map: Mutex::new(HashMap::new()),
        });
        let calls = Arc::new(AtomicUsize::new(0));
        let base = AspectEngineBase::new()
            .with_cache_backing(backing.clone())
            .record_cache_hits(true);
        let pipeline = pipeline_with(DeviceEngine::from_base(base, calls.clone()));

        // First request: a miss, so the engine runs and writes the result
        // through to the injected backing.
        let mut first = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("header.user-agent", "Mobile X")
                .build(),
        );
        first.process().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(backing.len(), 1, "result stored in the injected backing");

        // Second equivalent request: served from the injected backing, so the
        // engine does not run again.
        let mut second = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("header.user-agent", "Mobile X")
                .build(),
        );
        second.process().unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second request served from the injected backing, not re-run"
        );
        let device = second.get(DeviceEngine::KEY).unwrap();
        assert_eq!(device.get("IsMobile").unwrap().as_bool(), Some(true));
        assert!(device.cache_hit(), "served as a hit");
    }

    #[test]
    fn missing_property_reason_uses_tier() {
        let engine = DeviceEngine::new(false, Arc::new(AtomicUsize::new(0)));
        // A property that exists only in a higher tier.
        let mut e = engine;
        e.aspect_properties =
            vec![
                AspectPropertyMetaData::new("ScreenWidth", "device", PropertyValueType::Integer)
                    .with_data_tiers(["Enterprise"]),
            ];
        let err = e.property_missing_error("ScreenWidth");
        match err {
            Error::PropertyMissing { reason, .. } => {
                assert_eq!(reason, MissingPropertyReason::DataFileUpgradeRequired);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn the_upgrade_message_names_the_engine_type() {
        let mut engine = DeviceEngine::new(false, Arc::new(AtomicUsize::new(0)));
        engine.aspect_properties =
            vec![
                AspectPropertyMetaData::new("ScreenWidth", "device", PropertyValueType::Integer)
                    .with_data_tiers(["Enterprise"]),
            ];
        assert_eq!(engine.engine_type_name(), "DeviceEngine");
        let result = engine.missing_property_reason("ScreenWidth");
        assert!(result
            .description
            .contains("It is available with the Enterprise license/data for the DeviceEngine."));
        // Through a trait object the name is still the implementing type's.
        let engine: &dyn AspectEngine = &engine;
        assert_eq!(engine.engine_type_name(), "DeviceEngine");
    }

    #[test]
    fn the_first_engine_with_a_reason_decides() {
        let unknown = DeviceEngine::new(false, Arc::new(AtomicUsize::new(0)));
        let mut upgrade = DeviceEngine::new(false, Arc::new(AtomicUsize::new(0)));
        upgrade.aspect_properties =
            vec![
                AspectPropertyMetaData::new("ScreenWidth", "device", PropertyValueType::Integer)
                    .with_data_tiers(["Enterprise"]),
            ];

        let reason = |engines: &[&dyn AspectEngine]| {
            crate::missing_property_reason_for_engines("ScreenWidth", engines)
                .map(|result| result.reason)
        };

        assert_eq!(
            reason(&[&unknown, &upgrade]),
            Some(MissingPropertyReason::DataFileUpgradeRequired)
        );
        assert_eq!(
            reason(&[&upgrade, &unknown]),
            Some(MissingPropertyReason::DataFileUpgradeRequired)
        );
        assert_eq!(
            reason(&[&unknown, &unknown]),
            Some(MissingPropertyReason::Unknown)
        );
        assert_eq!(reason(&[]), None);

        // Two engines with different known reasons, so only the order decides.
        let mut excluded = DeviceEngine::new(false, Arc::new(AtomicUsize::new(0)));
        excluded.aspect_properties =
            vec![
                AspectPropertyMetaData::new("ScreenWidth", "device", PropertyValueType::Integer)
                    .with_data_tiers(["Lite"])
                    .map_core(|core| core.with_available(false)),
            ];
        assert_eq!(
            excluded.missing_property_reason("ScreenWidth").reason,
            MissingPropertyReason::PropertyExcludedFromConfig
        );
        let result = |engines: &[&dyn AspectEngine]| {
            crate::missing_property_reason_for_engines("ScreenWidth", engines)
        };
        assert_eq!(
            result(&[&unknown, &upgrade, &excluded]),
            Some(upgrade.missing_property_reason("ScreenWidth"))
        );
        assert_eq!(
            result(&[&unknown, &excluded, &upgrade]),
            Some(excluded.missing_property_reason("ScreenWidth"))
        );
    }

    #[test]
    fn short_type_names_drop_the_path_and_generics() {
        assert_eq!(short_type_name("a::b::Engine"), "Engine");
        assert_eq!(short_type_name("a::Engine<b::Data, c::D<e::F>>"), "Engine");
        assert_eq!(short_type_name("Engine"), "Engine");
    }
}
