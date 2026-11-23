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

//! Every Device Detection entry point leaves no native memory allocated.
//!
//! The test counts the blocks the native library allocates by giving it a
//! counting allocator, which every test in the same process would share. It
//! is therefore the only test in this file.

#![cfg(feature = "dd")]

mod support;

use std::os::raw::{c_int, c_void};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fiftyone_native::{dd, PerformanceProfile};
use fiftyone_pipeline_core::Evidence;
use support::Leaks;

extern "C" {
    #[link_name = "fiftyoneDegreesMalloc"]
    static mut MALLOC: unsafe extern "C" fn(usize) -> *mut c_void;
    #[link_name = "fiftyoneDegreesMallocAligned"]
    static mut MALLOC_ALIGNED: unsafe extern "C" fn(c_int, usize) -> *mut c_void;
    #[link_name = "fiftyoneDegreesFree"]
    static mut FREE: unsafe extern "C" fn(*mut c_void);
    #[link_name = "fiftyoneDegreesFreeAligned"]
    static mut FREE_ALIGNED: unsafe extern "C" fn(*mut c_void);
    #[link_name = "fiftyoneDegreesMemoryStandardMalloc"]
    fn standard_malloc(size: usize) -> *mut c_void;
    #[link_name = "fiftyoneDegreesMemoryStandardMallocAligned"]
    fn standard_malloc_aligned(alignment: c_int, size: usize) -> *mut c_void;
    #[link_name = "fiftyoneDegreesMemoryStandardFree"]
    fn standard_free(pointer: *mut c_void);
    #[link_name = "fiftyoneDegreesMemoryStandardFreeAligned"]
    fn standard_free_aligned(pointer: *mut c_void);
}

unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    let pointer = standard_malloc(size);
    support::allocated(pointer, size);
    pointer
}

unsafe extern "C" fn malloc_aligned(alignment: c_int, size: usize) -> *mut c_void {
    let pointer = standard_malloc_aligned(alignment, size);
    support::allocated(pointer, size);
    pointer
}

unsafe extern "C" fn free(pointer: *mut c_void) {
    support::freed(pointer);
    standard_free(pointer);
}

unsafe extern "C" fn free_aligned(pointer: *mut c_void) {
    support::freed(pointer);
    standard_free_aligned(pointer);
}

/// Route the native library's allocations through the counting allocator.
fn count_native_allocations() {
    // Safety: no native call is running, and each replacement allocates and
    // frees with the standard functions the library uses by default.
    unsafe {
        MALLOC = malloc;
        MALLOC_ALIGNED = malloc_aligned;
        FREE = free;
        FREE_ALIGNED = free_aligned;
    }
}

const DESKTOP: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const MOBILE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

/// Base64 of a Chrome on Windows high entropy values result.
const GHEV: &str = "eyJicmFuZHMiOlt7ImJyYW5kIjoiR29vZ2xlIENocm9tZSIsInZlcnNpb24iOiIxMjQifV0s\
    ImZ1bGxWZXJzaW9uTGlzdCI6W3siYnJhbmQiOiJHb29nbGUgQ2hyb21lIiwidmVyc2lvbiI6IjEy\
    NC4wLjYzNjcuOTEifV0sIm1vYmlsZSI6ZmFsc2UsIm1vZGVsIjoiIiwicGxhdGZvcm0iOiJXaW5k\
    b3dzIiwicGxhdGZvcm1WZXJzaW9uIjoiMTUuMC4wIiwiYXJjaGl0ZWN0dXJlIjoieDg2IiwiYml0\
    bmVzcyI6IjY0In0=";

/// The Hash data file `_51DEGREES_DD_PATH` names.
fn data_file() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var("_51DEGREES_DD_PATH").ok()?);
    path.is_file().then_some(path)
}

/// Evidence that takes every processing path: headers, client hints, a high
/// entropy values result, values measured in the browser and a query value.
fn rich_evidence() -> Evidence {
    Evidence::builder()
        .add("header.user-agent", DESKTOP)
        .add("header.sec-ch-ua", "\"Google Chrome\";v=\"124\"")
        .add("header.sec-ch-ua-mobile", "?0")
        .add("header.sec-ch-ua-platform", "\"Windows\"")
        .add("query.51d_gethighentropyvalues", GHEV)
        .add("query.51d_screenpixelswidth", "1234")
        .add("cookie.51d_screenpixelsheight", "567")
        .add("query.user-agent", MOBILE)
        .build()
}

fn measure_profile(leaks: &mut Leaks, path: &Path, profile: PerformanceProfile) {
    let label = |stage: &str| format!("{} {stage}", profile.as_str());
    let open = || dd::Manager::open(path, profile).expect("the data file loads");

    leaks.measure(&label("open and free"), 2, || drop(open()));

    let manager = open();
    let names = manager.property_names();
    let indexes: Vec<_> = names
        .iter()
        .map(|name| manager.required_property_index(name).expect("an index"))
        .collect();
    leaks.measure(&label("property names"), 5, || {
        manager.property_names();
    });
    leaks.measure(&label("evidence keys"), 5, || {
        manager.evidence_keys();
    });
    leaks.measure(&label("data set name"), 5, || {
        manager.data_set_name();
    });

    let mut indexing = dd::HashOptions::default();
    indexing
        .indexed_properties
        .push(String::from("BrowserName"));
    let open_indexed = || {
        dd::Manager::open_with_hash_options(path, profile, None, &indexing)
            .expect("the data file loads")
    };
    leaks.measure(&label("open with an index and free"), 2, || {
        drop(open_indexed())
    });
    let indexed = open_indexed();
    leaks.measure(&label("look up profiles from the index"), 3, || {
        for value in ["Chrome", "No such browser"] {
            indexed
                .profile_ids_for_property_value("BrowserName", value)
                .expect("the lookup succeeds");
        }
        indexed.indexed_properties();
    });
    drop(indexed);
    // Reading every profile passes each through the caches of the profiles
    // that have them, whose entries grow to fit the largest profile they
    // hold, so this lookup runs on a data set opened and freed in the stage.
    leaks.measure(
        &label("look up profiles by reading every profile"),
        2,
        || {
            let unindexed = open();
            for value in ["Chrome", "No such browser"] {
                unindexed
                    .profile_ids_for_property_value("BrowserName", value)
                    .expect("the lookup succeeds");
            }
        },
    );
    leaks.measure(&label("create and free results"), 5, || {
        drop(manager.create_results().expect("results allocate"));
    });

    let mut results = manager.create_results().expect("results allocate");
    let mut user_agent = 0;
    leaks.measure(&label("process a User-Agent"), 20, || {
        user_agent ^= 1;
        let value = [DESKTOP, MOBILE][user_agent];
        results
            .process_user_agent(value)
            .expect("a User-Agent is processed");
    });
    let evidence = rich_evidence();
    leaks.measure(&label("process evidence"), 20, || {
        results
            .process_evidence(&evidence)
            .expect("evidence is processed");
    });
    let read = |results: &dd::Results| {
        for name in &names {
            results.value_as_string(name, ",").expect("read by name");
            results.has_values(name).expect("has values");
        }
        for index in &indexes {
            results
                .value_as_string_by_index(*index, c",")
                .expect("read by index");
        }
        results.match_metrics();
        results.device_id().expect("the device id");
        results.matched_user_agents();
    };
    leaks.measure(&label("read evidence results"), 3, || read(&results));
    results.process_user_agent(MOBILE).expect("a User-Agent");
    leaks.measure(&label("read User-Agent results"), 3, || read(&results));
    let mut unmatched = manager.create_results().expect("results allocate");
    unmatched
        .process_user_agent("matches nothing")
        .expect("a User-Agent");
    leaks.measure(&label("read results that match nothing"), 3, || {
        read(&unmatched)
    });
    drop(unmatched);
    let device_id = results
        .device_id()
        .expect("the device id")
        .expect("a device id");
    leaks.measure(&label("process a device id"), 20, || {
        results.process_device_id(&device_id).expect("a device id");
    });
    leaks.measure(&label("read device id results"), 3, || read(&results));

    // A new data set loaded while results from the old one are alive, as an
    // engine refresh does.
    leaks.measure(&label("reload with results alive"), 2, || {
        let old = open();
        let mut alive = old.create_results().expect("results allocate");
        alive.process_user_agent(MOBILE).expect("a User-Agent");
        let new = open();
        drop(old);
        assert_eq!(
            alive.value_as_string("IsMobile", ",").expect("a value"),
            Some(String::from("True"))
        );
        let mut fresh = new.create_results().expect("results allocate");
        fresh.process_user_agent(DESKTOP).expect("a User-Agent");
        drop(alive);
        drop(fresh);
        drop(new);
    });

    drop(results);
    drop(manager);
}

#[test]
fn every_entry_point_leaves_no_native_memory_allocated() {
    let Some(path) = data_file() else {
        eprintln!("_51DEGREES_DD_PATH names no Hash data file; skipping");
        return;
    };
    count_native_allocations();

    // The thread's pooled evidence array is created on first use and kept,
    // so it is made before the baseline is read.
    let warm: Arc<dd::Manager> =
        dd::Manager::open(&path, PerformanceProfile::InMemory).expect("the data file loads");
    let mut results = warm.create_results().expect("results allocate");
    results
        .process_evidence(&rich_evidence())
        .expect("evidence is processed");
    drop(results);
    drop(warm);
    let baseline = support::held();

    let mut leaks = Leaks::default();
    for profile in [
        PerformanceProfile::InMemory,
        PerformanceProfile::HighPerformance,
        PerformanceProfile::LowMemory,
        PerformanceProfile::Balanced,
        PerformanceProfile::Default,
    ] {
        measure_profile(&mut leaks, &path, profile);
        leaks.compare(&format!("{} everything freed", profile.as_str()), baseline);
    }
    leaks.measure("open a missing file", 3, || {
        let opened = dd::Manager::open(path.with_extension("missing"), PerformanceProfile::Default);
        assert!(opened.is_err(), "a missing file does not load");
    });
    let not_data = std::env::temp_dir().join("fiftyone-native-dd-not-a-data-file.hash");
    std::fs::write(&not_data, vec![7u8; 64 * 1024]).expect("a temporary file");
    for profile in [PerformanceProfile::InMemory, PerformanceProfile::LowMemory] {
        leaks.measure(
            &format!("{} open a file that is not a data file", profile.as_str()),
            3,
            || {
                let opened = dd::Manager::open(&not_data, profile);
                assert!(opened.is_err(), "the file does not load");
            },
        );
    }
    std::fs::remove_file(&not_data).ok();

    leaks.compare("everything freed", baseline);
    leaks.assert_none();
}
