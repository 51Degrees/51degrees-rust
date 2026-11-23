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

//! The case-insensitive, deterministic keying of [`DataKeyedCache`] from a
//! flow data's evidence. `DataKeyedCache` comes with the `pipeline` feature,
//! so these tests build only with it.
#![cfg(feature = "pipeline")]

use std::sync::Arc;

use fiftyone_caching::{CacheBuilder, DataKeyedCache};
use fiftyone_pipeline_core::{
    Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData, Result,
};

// -------------------------------------------------------------------------
// DataKeyedCache: keying from flow data evidence.
// -------------------------------------------------------------------------

/// A flow element that does nothing but advertise the evidence keys used to
/// derive a cache key. Required only to stand up a pipeline that can create
/// flow data in the tests.
struct AdvertiseElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl AdvertiseElement {
    fn new() -> Self {
        AdvertiseElement {
            filter: EvidenceKeyFilterWhitelist::new(["query.user-agent"]),
            properties: Vec::new(),
        }
    }
}

impl FlowElement for AdvertiseElement {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "advertise"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

fn pipeline() -> Arc<Pipeline> {
    Pipeline::builder()
        .add_element(Arc::new(AdvertiseElement::new()))
        .build()
        .expect("pipeline build")
}

fn flow_data_with(pipeline: &Arc<Pipeline>, ua: &str) -> FlowData {
    pipeline.create_flow_data_with(Evidence::builder().add("query.user-agent", ua).build())
}

#[test]
fn data_keyed_hit_and_miss() {
    let pipeline = pipeline();
    let filter = Arc::new(EvidenceKeyFilterWhitelist::new(["query.user-agent"]));
    let cache: DataKeyedCache<String> = DataKeyedCache::new(CacheBuilder::new(), filter);

    let data = flow_data_with(&pipeline, "agent-1");
    assert!(cache.get(&data).is_none());
    cache.put(&data, "value-1".to_owned());
    assert_eq!(cache.get(&data), Some("value-1".to_owned()));
    assert_eq!(cache.len(), 1);
}

#[test]
fn data_keyed_distinguishes_different_evidence() {
    let pipeline = pipeline();
    let filter = Arc::new(EvidenceKeyFilterWhitelist::new(["query.user-agent"]));
    let cache: DataKeyedCache<String> = DataKeyedCache::new(CacheBuilder::new(), filter);

    let a = flow_data_with(&pipeline, "agent-a");
    let b = flow_data_with(&pipeline, "agent-b");
    cache.put(&a, "A".to_owned());
    cache.put(&b, "B".to_owned());
    assert_eq!(cache.get(&a), Some("A".to_owned()));
    assert_eq!(cache.get(&b), Some("B".to_owned()));
    assert_eq!(cache.len(), 2);
}

#[test]
fn data_keyed_key_is_case_insensitive_on_evidence_key() {
    // The spec requires evidence key comparison to be case-insensitive, so two
    // flow datas differing only in evidence key casing must share a cache entry.
    let pipeline = pipeline();
    let filter = Arc::new(EvidenceKeyFilterWhitelist::new(["query.user-agent"]));
    let cache: DataKeyedCache<String> = DataKeyedCache::new(CacheBuilder::new(), filter);

    let lower =
        pipeline.create_flow_data_with(Evidence::builder().add("query.user-agent", "abc").build());
    let upper =
        pipeline.create_flow_data_with(Evidence::builder().add("query.User-Agent", "abc").build());

    cache.put(&lower, "stored".to_owned());
    // Looked up via the differently-cased key, this must hit.
    assert_eq!(cache.get(&upper), Some("stored".to_owned()));
    assert_eq!(cache.len(), 1);
}

#[test]
fn data_keyed_value_is_case_sensitive_on_evidence_value() {
    // Evidence values are case-sensitive, so different value casing must be a
    // distinct cache key.
    let pipeline = pipeline();
    let filter = Arc::new(EvidenceKeyFilterWhitelist::new(["query.user-agent"]));
    let cache: DataKeyedCache<String> = DataKeyedCache::new(CacheBuilder::new(), filter);

    let lower = flow_data_with(&pipeline, "abc");
    let upper = flow_data_with(&pipeline, "ABC");
    cache.put(&lower, "lower".to_owned());
    assert!(cache.get(&upper).is_none());
}

#[test]
fn data_keyed_key_for_is_deterministic() {
    let pipeline = pipeline();
    let filter = Arc::new(EvidenceKeyFilterWhitelist::new(["query.user-agent"]));
    let cache: DataKeyedCache<String> = DataKeyedCache::new(CacheBuilder::new(), filter);

    let a = flow_data_with(&pipeline, "same");
    let b = flow_data_with(&pipeline, "same");
    assert_eq!(cache.key_for(&a), cache.key_for(&b));
}

#[test]
fn data_keyed_caches_arc_aspect_data() {
    // The intended pattern: cache an Arc around a Send + Sync struct rather than
    // the (non-Sync) element data itself.
    #[derive(PartialEq, Eq, Debug)]
    struct AspectData {
        hardware: String,
    }

    let pipeline = pipeline();
    let filter = Arc::new(EvidenceKeyFilterWhitelist::new(["query.user-agent"]));
    let cache: DataKeyedCache<Arc<AspectData>> = DataKeyedCache::new(CacheBuilder::new(), filter);

    let data = flow_data_with(&pipeline, "agent");
    let value = Arc::new(AspectData {
        hardware: "phone".to_owned(),
    });
    cache.put(&data, Arc::clone(&value));

    let hit = cache.get(&data).expect("expected a cache hit");
    assert_eq!(hit.hardware, "phone");
    // The cached Arc points at the same allocation we stored.
    assert!(Arc::ptr_eq(&hit, &value));
}

#[test]
fn data_keyed_inner_exposes_plain_cache_traits() {
    let pipeline = pipeline();
    let filter = Arc::new(EvidenceKeyFilterWhitelist::new(["query.user-agent"]));
    let cache: DataKeyedCache<String> = DataKeyedCache::new(CacheBuilder::new(), filter);

    let data = flow_data_with(&pipeline, "agent");
    let key = cache.key_for(&data);
    // Drive the underlying LruCache through the Cache / PutCache traits.
    cache.inner().put(key.clone(), "via-trait".to_owned());
    assert_eq!(cache.inner().get(&key), Some("via-trait".to_owned()));
}
