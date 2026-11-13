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

//! Reading IP Intelligence values leaves no native memory allocated.
//!
//! The test counts the native library's allocations by switching it to its
//! tracking allocator, which every test in the same process would share. It
//! is therefore the only test in this file, and it switches tracking on
//! before the native library allocates anything, because the tracking free
//! refuses memory it did not record.

#![cfg(feature = "ipi")]

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::path::PathBuf;

use fiftyone_native::{ipi, PerformanceProfile};

extern "C" {
    #[link_name = "ipi_fiftyoneDegreesSetUpMemoryTracking"]
    fn set_up_memory_tracking();
    #[link_name = "ipi_fiftyoneDegreesUnsetMemoryTracking"]
    fn unset_memory_tracking() -> usize;
    #[link_name = "ipi_fiftyoneDegreesMemoryTrackingGetAllocated"]
    fn memory_tracking_allocated() -> usize;
}

/// The ASN data file: `_51DEGREES_IPI_PATH`, or the one in the
/// `cxx` submodule.
fn asn_data_file() -> Option<PathBuf> {
    if let Ok(path) = crate::deprecated_names::var("_51DEGREES_IPI_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let relative = "cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi";
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join(relative);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[test]
fn reading_values_leaves_no_native_memory_allocated() {
    let Some(file) = asn_data_file() else {
        eprintln!("no IP Intelligence data file found; skipping read memory test");
        return;
    };
    // Safety: no native allocation has been made in this process yet.
    unsafe { set_up_memory_tracking() };

    let manager =
        ipi::Manager::open(&file, PerformanceProfile::InMemory).expect("the data file loads");
    let names = manager.available_property_names();
    let indexes: Vec<_> = names
        .iter()
        .map(|name| manager.required_property_index(name).expect("an index"))
        .collect();
    let mut results = manager.create_results().expect("results allocate");
    results.process_ip("1.1.1.1").expect("the lookup succeeds");
    let read_all = |results: &ipi::Results| {
        for (name, index) in names.iter().zip(&indexes) {
            let _ = results.values_weighted(name);
            let _ = results.read_property_by_index(*index);
        }
    };
    // The results keep buffers that grow to fit the values, so one pass first.
    read_all(&results);

    // Safety: tracking is on.
    let before = unsafe { memory_tracking_allocated() };
    for _ in 0..50 {
        read_all(&results);
    }
    // Safety: tracking is on.
    let after = unsafe { memory_tracking_allocated() };
    drop(results);
    drop(manager);
    // Safety: every native object was dropped above.
    unsafe { unset_memory_tracking() };

    assert_eq!(
        after,
        before,
        "reading {} properties 50 times left {} bytes allocated",
        names.len(),
        after.saturating_sub(before)
    );
}
