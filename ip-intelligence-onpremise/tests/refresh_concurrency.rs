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

//! Refreshes run at once from several threads, and while other threads look
//! up, as the data updates specification requires: "This mechanism MUST be
//! thread safe." and "The Pipeline that contains the Aspect Engine MUST
//! still be capable of processing Flow Data with minimal performance impact
//! while data refresh is happening." No test here measures time. Each checks
//! that every refresh and every lookup made while refreshes run succeeds and
//! gives the answer it gives with no refresh running. The tests use the ASN
//! file in the data repository inside the `cxx` submodule, and skip with a
//! note without it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use fiftyone_ip_intelligence_onpremise::{IpIntelligenceOnPremiseEngine, IP_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{ElementData, Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::OnPremiseAspectEngine;

/// The ASN file in the data repository.
const ASN_FILE_NAME: &str = "51Degrees-IPIV4AsnIpiV41.ipi";

/// The addresses looked up, one for each processing thread in turn.
const ADDRESSES: [&str; 2] = ["1.1.1.1", "8.8.8.8"];

/// The threads that refresh, and the refreshes each makes.
const REFRESHING_THREADS: usize = 4;
const REFRESHES_EACH: usize = 3;

/// The threads that look up while the refreshes run.
const PROCESSING_THREADS: usize = 4;

/// The ASN file in the data repository inside the `cxx` submodule.
fn asn_file() -> Option<PathBuf> {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join("cxx/data/ip-intelligence").join(ASN_FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// An engine over the ASN file under `profile`, in a pipeline of its own.
fn engine_and_pipeline(
    path: &PathBuf,
    profile: PerformanceProfile,
) -> (Arc<IpIntelligenceOnPremiseEngine>, Arc<Pipeline>) {
    let engine = Arc::new(
        IpIntelligenceOnPremiseEngine::builder(path)
            .performance_profile(profile)
            .auto_update(false)
            .file_system_watcher(false)
            .build()
            .expect("the data file loads"),
    );
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder().add_element(element).build().unwrap();
    (engine, pipeline)
}

/// The `AsnName` the pipeline gives for `ip`, written out.
fn asn_name(pipeline: &Arc<Pipeline>, ip: &str) -> String {
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.client-ip", ip).build());
    data.process().expect("processing succeeds");
    let ip_data = data.get(IP_DATA_KEY).expect("the IP data is there");
    format!("{:?}", ip_data.get("AsnName"))
}

/// Run `REFRESHES_EACH` refreshes on each of `REFRESHING_THREADS` threads at
/// once, half from the file and half from memory.
fn refresh_from_several_threads(engine: &IpIntelligenceOnPremiseEngine, bytes: &[u8]) {
    std::thread::scope(|scope| {
        for thread in 0..REFRESHING_THREADS {
            scope.spawn(move || {
                for refresh in 0..REFRESHES_EACH {
                    let result = if thread % 2 == 0 {
                        engine.refresh(None)
                    } else {
                        engine.refresh_from_memory(None, bytes)
                    };
                    result.unwrap_or_else(|error| {
                        panic!("refresh {refresh} on thread {thread} fails: {error}")
                    });
                }
            });
        }
    });
}

/// Refreshes made at once from several threads, from the file and from
/// memory, all succeed, and the engine then looks up as before.
#[test]
fn refreshes_made_at_once_from_several_threads_all_succeed() {
    let Some(path) = asn_file() else {
        eprintln!("no ASN data file found; skipping refreshes at once");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    for profile in [PerformanceProfile::InMemory, PerformanceProfile::LowMemory] {
        let (engine, pipeline) = engine_and_pipeline(&path, profile);
        let expected: Vec<String> = ADDRESSES.iter().map(|ip| asn_name(&pipeline, ip)).collect();

        refresh_from_several_threads(&engine, &bytes);

        let after: Vec<String> = ADDRESSES.iter().map(|ip| asn_name(&pipeline, ip)).collect();
        assert_eq!(after, expected, "{profile:?}");
    }
}

/// Threads go on looking up while other threads refresh, and every lookup
/// made meanwhile succeeds and gives the answer a lookup gives with no
/// refresh running.
#[test]
fn processing_goes_on_while_refreshes_run() {
    let Some(path) = asn_file() else {
        eprintln!("no ASN data file found; skipping processing during refresh");
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    for profile in [PerformanceProfile::InMemory, PerformanceProfile::LowMemory] {
        let (engine, pipeline) = engine_and_pipeline(&path, profile);
        let expected: Vec<String> = ADDRESSES.iter().map(|ip| asn_name(&pipeline, ip)).collect();
        let refreshed = AtomicBool::new(false);
        let lookups = AtomicUsize::new(0);

        std::thread::scope(|scope| {
            for thread in 0..PROCESSING_THREADS {
                let (pipeline, expected) = (&pipeline, &expected);
                let (refreshed, lookups) = (&refreshed, &lookups);
                scope.spawn(move || loop {
                    // Read before looking up, so the last loop starts after
                    // every refresh has ended.
                    let last = refreshed.load(Ordering::Acquire);
                    let which = thread % ADDRESSES.len();
                    assert_eq!(
                        asn_name(pipeline, ADDRESSES[which]),
                        expected[which],
                        "{profile:?} thread {thread}"
                    );
                    lookups.fetch_add(1, Ordering::Relaxed);
                    if last {
                        break;
                    }
                });
            }
            refresh_from_several_threads(&engine, &bytes);
            refreshed.store(true, Ordering::Release);
        });

        assert!(
            lookups.load(Ordering::Relaxed) >= PROCESSING_THREADS,
            "{profile:?}: every processing thread looked up"
        );
    }
}
