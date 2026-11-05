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

//! Caching: give an engine a result cache so repeated, equivalent requests are
//! served from memory instead of recomputed.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use fiftyone_caching::{CacheBuilder, DataKeyedCache};
use fiftyone_pipeline_core::{
    Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData, PropertyValueType, TypedKey,
};
use pipeline_examples::star_sign::{
    parse_day_month, star_sign_for, StarSignData, STAR_SIGNS, STAR_SIGN_DATA_KEY,
    STAR_SIGN_PROPERTY, UNKNOWN_STAR_SIGN,
};

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Pipeline API", "https://51degrees.com/documentation/_pipeline_api__index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-caching.rs&utm_term=find-out-more"),
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-caching.rs&utm_term=find-out-more"),
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-caching.rs&utm_term=find-out-more"),
];

/// The evidence key the engine reads the birth date from.
const DATE_OF_BIRTH_EVIDENCE: &str = "date-of-birth";

/// A star-sign engine with a result cache.
///
/// On each request it derives a deterministic key from the evidence and looks it
/// up in the cache. A hit reuses the previous result and skips the (here trivial,
/// in production expensive) computation; a miss computes the sign and stores it.
/// The cached value is the sign string, which is `Send + Sync + Clone`, as the
/// cache requires. A real engine caches an `Arc` around its aspect data instead.
struct CachedStarSignEngine {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    /// The result cache, keyed by the engine's accepted evidence.
    cache: DataKeyedCache<String>,
    /// Counts how many times the sign was actually computed, so the example can
    /// show how many requests the cache served.
    computations: AtomicU64,
}

impl CachedStarSignEngine {
    const KEY: TypedKey<StarSignData> = TypedKey::new(STAR_SIGN_DATA_KEY);

    /// Build the engine with a cache sized by the supplied builder.
    fn new(cache_builder: CacheBuilder) -> Self {
        // The cache derives its key from the same evidence the engine reads, so
        // two requests with an equivalent birth date share a cache entry
        // regardless of key casing or ordering.
        let filter: Arc<dyn EvidenceKeyFilter> =
            Arc::new(EvidenceKeyFilterWhitelist::new([DATE_OF_BIRTH_EVIDENCE]));
        CachedStarSignEngine {
            filter: EvidenceKeyFilterWhitelist::new([DATE_OF_BIRTH_EVIDENCE]),
            properties: vec![PropertyMetaData::new(
                STAR_SIGN_PROPERTY,
                STAR_SIGN_DATA_KEY,
                PropertyValueType::String,
            )],
            cache: DataKeyedCache::new(cache_builder, filter),
            computations: AtomicU64::new(0),
        }
    }

    /// How many times the engine computed a result rather than serving it from
    /// the cache.
    fn computation_count(&self) -> u64 {
        self.computations.load(Ordering::Relaxed)
    }

    /// Compute the sign from the flow data's evidence, counting the call.
    fn compute_sign(&self, data: &FlowData) -> String {
        self.computations.fetch_add(1, Ordering::Relaxed);
        data.evidence()
            .get(DATE_OF_BIRTH_EVIDENCE)
            .and_then(parse_day_month)
            .and_then(|(month, day)| star_sign_for(&STAR_SIGNS, month, day))
            .unwrap_or(UNKNOWN_STAR_SIGN)
            .to_owned()
    }
}

impl FlowElement for CachedStarSignEngine {
    fn process(&self, data: &mut FlowData) -> Result<(), fiftyone_pipeline_core::Error> {
        // Try the cache first. On a miss, compute and store. The lookup and the
        // store both use the deterministic key derived from the evidence.
        let sign = match self.cache.get(data) {
            Some(cached) => cached,
            None => {
                let computed = self.compute_sign(data);
                self.cache.put(data, computed.clone());
                computed
            }
        };

        data.get_or_add(Self::KEY, StarSignData::new)?
            .set_star_sign(sign);
        Ok(())
    }
    fn data_key(&self) -> &str {
        STAR_SIGN_DATA_KEY
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// Options controlling one run of the example.
pub struct ExampleOptions {
    /// The birth dates to process, in order. Repeats demonstrate cache hits.
    pub dates_of_birth: Vec<String>,
    /// The maximum number of entries the cache holds.
    pub cache_size: usize,
}

impl Default for ExampleOptions {
    fn default() -> Self {
        ExampleOptions {
            // Process the same two dates twice over, so the second pass is served
            // entirely from the cache.
            dates_of_birth: vec![
                "18/12/1992".to_owned(),
                "15/07/1988".to_owned(),
                "18/12/1992".to_owned(),
                "15/07/1988".to_owned(),
            ],
            cache_size: 100,
        }
    }
}

/// Run the example: build a pipeline with a cached engine, process a batch of
/// requests (including repeats) and report how many were computed versus cached.
pub fn run(options: &ExampleOptions) -> Result<()> {
    let engine = Arc::new(CachedStarSignEngine::new(
        CacheBuilder::new().size(options.cache_size),
    ));
    let pipeline: Arc<Pipeline> = Pipeline::builder()
        .add_element(engine.clone() as Arc<dyn FlowElement>)
        .build()?;

    for date in &options.dates_of_birth {
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add(DATE_OF_BIRTH_EVIDENCE, date.clone())
                .build(),
        );
        data.process()?;
        let sign = data
            .get(CachedStarSignEngine::KEY)
            .and_then(StarSignData::star_sign)
            .unwrap_or(UNKNOWN_STAR_SIGN)
            .to_owned();
        println!("Date of birth {date} -> {sign}.");
    }

    let total = options.dates_of_birth.len() as u64;
    let computed = engine.computation_count();
    println!(
        "Processed {total} requests; computed {computed}, served {} from the cache.",
        total - computed
    );
    Ok(())
}

/// Run the example with the default batch of dates.
fn main() -> Result<()> {
    let result = run(&ExampleOptions::default());
    examples_shared::print_find_out_more(FIND_OUT_MORE, pipeline_examples::REPOSITORIES);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_with_default_options() {
        run(&ExampleOptions::default()).expect("the caching example should run");
    }

    #[test]
    fn repeated_requests_are_served_from_the_cache() {
        // Drive the engine with two distinct dates each repeated once, then check
        // that only the two distinct dates were ever computed.
        let engine = Arc::new(CachedStarSignEngine::new(CacheBuilder::new().size(100)));
        let pipeline = Pipeline::builder()
            .add_element(engine.clone() as Arc<dyn FlowElement>)
            .build()
            .unwrap();
        for date in ["18/12/1992", "15/07/1988", "18/12/1992", "15/07/1988"] {
            let mut data = pipeline.create_flow_data_with(
                Evidence::builder()
                    .add(DATE_OF_BIRTH_EVIDENCE, date)
                    .build(),
            );
            data.process().unwrap();
        }
        // Four requests, two distinct: exactly two computations.
        assert_eq!(engine.computation_count(), 2);
    }
}

/* ---------------------------------------------------------------------------
 * Example: Caching (a result cache for an engine)
 *
 * This example gives an engine a result cache, so repeated requests with
 * equivalent evidence are served from memory instead of recomputed. Caching is
 * most valuable in front of an expensive engine, above all the cloud request
 * engine, where a cache hit avoids a network round trip entirely.
 *
 * What it shows
 * -------------
 *   1. Holding a `DataKeyedCache` inside an engine. The cache is constructed from
 *      a `CacheBuilder` (which sets the total size and the shard count) and the
 *      engine's evidence-key filter. Using the engine's own filter means the
 *      cache key is derived only from the evidence the engine actually reads.
 *
 *   2. The get-or-compute pattern in `process`: look the request up in the cache;
 *      on a hit reuse the stored result; on a miss compute it (here a trivial
 *      table lookup, in production the expensive work) and store it. The example
 *      counts computations so it can report how many requests the cache served.
 *
 *   3. Deterministic keys. The cache derives its key from the relevant evidence
 *      in a fixed order with case-insensitive names, so two requests that differ
 *      only in evidence ordering or key casing still share one entry.
 *
 * What gets cached
 * ----------------
 * The cached value must be `Send + Sync + Clone`. This example caches the sign
 * string directly. A real engine caches a thread-safe handle to its aspect data
 * (typically an `Arc` around the data struct) because element data itself is not
 * `Sync`, then rebuilds or shares its element data from the cached handle on a
 * hit.
 *
 * Eviction
 * --------
 * The cache is a sharded least-recently-used cache: once a shard is full it
 * evicts its least-recently-used entry, so memory stays bounded. The size is set
 * through the options.
 *
 * Usage sharing
 * -------------
 * This is a console example and does NOT add usage sharing; a production
 * deployment should (see the `usage-sharing` example).
 *
 * Running it
 * ----------
 *   cargo run -p pipeline-examples --bin caching
 *
 * It processes two dates twice and reports that two of the four requests were
 * served from the cache.
 * ------------------------------------------------------------------------- */
