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

//! A cache keyed by a flow data's evidence.
//!
//! An engine holds one of these. On each request it derives a deterministic
//! [`DataKey`]
//! from the flow data's evidence (filtered by the evidence the engine accepts)
//! and uses that as the cache key. Equivalent requests therefore share a cache
//! entry regardless of evidence ordering or key casing, exactly as the
//! [caching specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/caching.md#generation-of-keys)
//! requires.

use std::sync::Arc;

use fiftyone_pipeline_core::{DataKey, EvidenceKeyFilter, FlowData};

use crate::cache::PutCache;
use crate::config::CacheBuilder;

/// A cache whose key is derived from a [`FlowData`]'s evidence.
///
/// `V` is the engine's cached result. It must be `Send + Sync + Clone`, so it
/// is typically an [`Arc`] around the engine's aspect-data struct, or the raw
/// cloud JSON string. Element data itself is not `Clone` and so cannot be
/// cached directly; an engine caches a representation it can clone and rebuilds
/// (or shares) its element data from that on a hit.
///
/// The cache derives a [`DataKey`] from a flow data using the
/// [`EvidenceKeyFilter`] supplied at construction. That filter is normally the
/// engine's own [`fiftyone_pipeline_core::FlowElement::evidence_key_filter`], so
/// only the evidence the engine actually reads contributes to the key.
///
/// # Example
///
/// ```
/// use std::sync::Arc;
/// use fiftyone_caching::{CacheBuilder, DataKeyedCache};
/// use fiftyone_pipeline_core::{
///     Evidence, EvidenceKeyFilterWhitelist, Pipeline, FlowElement, FlowData,
///     EvidenceKeyFilter, PropertyMetaData, Result,
/// };
/// # struct Noop { f: EvidenceKeyFilterWhitelist, p: Vec<PropertyMetaData> }
/// # impl FlowElement for Noop {
/// #     fn process(&self, _d: &mut FlowData) -> Result<()> { Ok(()) }
/// #     fn data_key(&self) -> &str { "noop" }
/// #     fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter { &self.f }
/// #     fn properties(&self) -> &[PropertyMetaData] { &self.p }
/// # }
///
/// let filter = Arc::new(EvidenceKeyFilterWhitelist::new(["query.user-agent"]));
/// let cache: DataKeyedCache<String> = DataKeyedCache::new(
///     CacheBuilder::new(),
///     filter,
/// );
///
/// let pipeline = Pipeline::builder()
///     .add_element(Arc::new(Noop {
///         f: EvidenceKeyFilterWhitelist::new(["query.user-agent"]),
///         p: vec![],
///     }))
///     .build()
///     .unwrap();
/// let data = pipeline.create_flow_data_with(
///     Evidence::builder().add("query.user-agent", "abc").build(),
/// );
///
/// assert!(cache.get(&data).is_none());
/// cache.put(&data, "result".to_owned());
/// assert_eq!(cache.get(&data), Some("result".to_owned()));
/// ```
pub struct DataKeyedCache<V>
where
    V: Clone + Send + Sync + 'static,
{
    inner: Arc<dyn PutCache<DataKey, V>>,
    filter: Arc<dyn EvidenceKeyFilter>,
}

impl<V> DataKeyedCache<V>
where
    V: Clone + Send + Sync + 'static,
{
    /// Create a flow-data-keyed cache from a [`CacheBuilder`] and the
    /// [`EvidenceKeyFilter`] used to derive keys.
    ///
    /// This is the convenience path: it builds the default in-process
    /// [`crate::LruCache`] and wraps it as the [`PutCache`] backing. A consumer
    /// that wants a different cache (for example a durable external store) builds
    /// it themselves and passes it to [`DataKeyedCache::with_backing`].
    ///
    /// The filter is shared (held in an [`Arc`]), so the same filter instance an
    /// engine advertises can be passed in without cloning its contents.
    pub fn new(builder: CacheBuilder, filter: Arc<dyn EvidenceKeyFilter>) -> Self {
        let backing: Arc<dyn PutCache<DataKey, V>> = Arc::new(builder.build());
        Self::with_backing(backing, filter)
    }

    /// Create a flow-data-keyed cache over an injected [`PutCache`] backing and
    /// the [`EvidenceKeyFilter`] used to derive keys.
    ///
    /// This lets a consumer supply its own cache implementation in place of the
    /// built-in [`crate::LruCache`], for example a durable external key/value
    /// store so cache entries survive beyond a single process. The backing is
    /// shared (held in an [`Arc`]) and used behind `&self`; its value type is
    /// therefore `Clone + Send + Sync`, the same bound the engine's cached data
    /// already satisfies. The cache key derivation is unchanged: the
    /// [`EvidenceKeyFilter`] is used to build the deterministic [`DataKey`].
    pub fn with_backing(
        backing: Arc<dyn PutCache<DataKey, V>>,
        filter: Arc<dyn EvidenceKeyFilter>,
    ) -> Self {
        DataKeyedCache {
            inner: backing,
            filter,
        }
    }

    /// The evidence key filter this cache derives keys with.
    pub fn filter(&self) -> &Arc<dyn EvidenceKeyFilter> {
        &self.filter
    }

    /// Derive the deterministic [`DataKey`] for a flow data using this cache's
    /// filter. Exposed so an engine can pre-compute a key once and reuse it for
    /// both the lookup and the later write.
    pub fn key_for(&self, data: &FlowData) -> DataKey {
        data.generate_key(self.filter.as_ref())
    }

    /// Look up the cached value for a flow data, if present.
    pub fn get(&self, data: &FlowData) -> Option<V> {
        self.inner.get(&self.key_for(data))
    }

    /// Store a value for a flow data, replacing any existing entry.
    pub fn put(&self, data: &FlowData, value: V) {
        self.inner.put(self.key_for(data), value);
    }

    /// Look up the cached value directly by a pre-computed [`DataKey`].
    pub fn get_by_key(&self, key: &DataKey) -> Option<V> {
        self.inner.get(key)
    }

    /// Store a value directly under a pre-computed [`DataKey`].
    pub fn put_by_key(&self, key: DataKey, value: V) {
        self.inner.put(key, value);
    }

    /// The number of entries currently held.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// True if the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Remove every entry from the cache.
    pub fn clear(&self) {
        self.inner.clear();
    }

    /// Borrow the cache backing as the plain [`crate::Cache`]/[`PutCache`] trait
    /// surface, keyed by [`DataKey`], for callers that prefer to work through
    /// the traits directly. With the default [`DataKeyedCache::new`] this is the
    /// built-in [`crate::LruCache`]; with [`DataKeyedCache::with_backing`] it is
    /// whatever the consumer injected.
    pub fn inner(&self) -> &dyn PutCache<DataKey, V> {
        self.inner.as_ref()
    }
}
