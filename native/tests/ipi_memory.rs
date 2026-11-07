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

//! Every IP Intelligence entry point leaves no native memory allocated.
//!
//! The test counts the blocks the native library allocates by giving it a
//! counting allocator, which every test in the same process would share. It
//! is therefore the only test in this file.

#![cfg(feature = "ipi")]

mod support;

use std::ffi::CString;
use std::os::raw::{c_int, c_void};
use std::path::{Path, PathBuf};

use fiftyone_ip_intelligence_sys as sys;
use fiftyone_native::{ipi, PerformanceProfile};
use fiftyone_pipeline_core::Evidence;
use support::Leaks;

extern "C" {
    #[link_name = "ipi_fiftyoneDegreesMalloc"]
    static mut MALLOC: unsafe extern "C" fn(usize) -> *mut c_void;
    #[link_name = "ipi_fiftyoneDegreesMallocAligned"]
    static mut MALLOC_ALIGNED: unsafe extern "C" fn(c_int, usize) -> *mut c_void;
    #[link_name = "ipi_fiftyoneDegreesFree"]
    static mut FREE: unsafe extern "C" fn(*mut c_void);
    #[link_name = "ipi_fiftyoneDegreesFreeAligned"]
    static mut FREE_ALIGNED: unsafe extern "C" fn(*mut c_void);
    #[link_name = "ipi_fiftyoneDegreesMemoryStandardMalloc"]
    fn standard_malloc(size: usize) -> *mut c_void;
    #[link_name = "ipi_fiftyoneDegreesMemoryStandardMallocAligned"]
    fn standard_malloc_aligned(alignment: c_int, size: usize) -> *mut c_void;
    #[link_name = "ipi_fiftyoneDegreesMemoryStandardFree"]
    fn standard_free(pointer: *mut c_void);
    #[link_name = "ipi_fiftyoneDegreesMemoryStandardFreeAligned"]
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

/// The IP Intelligence data file `51DEGREES_IPI_PATH` names.
fn data_file() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var("51DEGREES_IPI_PATH").ok()?);
    path.is_file().then_some(path)
}

fn measure_profile(leaks: &mut Leaks, path: &Path, profile: PerformanceProfile) {
    let label = |stage: &str| format!("{} {stage}", profile.as_str());
    let open = || ipi::Manager::open(path, profile).expect("the data file loads");

    leaks.measure(&label("open and free"), 2, || drop(open()));

    let manager = open();
    let names = manager.available_property_names();
    let indexes: Vec<_> = names
        .iter()
        .map(|name| manager.required_property_index(name).expect("an index"))
        .collect();
    leaks.measure(&label("property names"), 5, || {
        manager.available_property_names();
    });
    leaks.measure(&label("data set name and date"), 5, || {
        manager.data_set_name();
        manager.data_set_published();
    });
    leaks.measure(&label("create and free results"), 5, || {
        drop(manager.create_results().expect("results allocate"));
    });

    let mut results = manager.create_results().expect("results allocate");
    let mut address = 0;
    leaks.measure(&label("process an address"), 20, || {
        address ^= 1;
        let value = ["1.1.1.1", "2606:4700:4700::1111"][address];
        results.process_ip(value).expect("an address is processed");
    });
    let evidence = Evidence::builder()
        .add("server.client-ip", "9.9.9.9")
        .add("query.client-ip-51d", "1.1.1.1:443")
        .build();
    leaks.measure(&label("process evidence"), 20, || {
        results
            .process_evidence(&evidence)
            .expect("evidence is processed");
    });
    leaks.measure(&label("read every value"), 3, || {
        for name in &names {
            results.value_as_string(name, ",").expect("read by name");
            results.values_weighted(name).expect("weighted by name");
            results.read_property(name).expect("typed by name");
        }
        for index in &indexes {
            results
                .value_as_string_by_index(*index, c",")
                .expect("read by index");
            results.values_weighted_by_index(*index).expect("weighted");
            results.read_property_by_index(*index).expect("typed");
        }
    });
    // Addresses the data file may hold few or no values for.
    leaks.measure(&label("read values of reserved addresses"), 3, || {
        for address in [
            "0.0.0.0",
            "10.0.0.1",
            "127.0.0.1",
            "240.0.0.1",
            "::1",
            "fe80::1",
        ] {
            results
                .process_ip(address)
                .expect("an address is processed");
            for index in &indexes {
                results.values_weighted_by_index(*index).expect("weighted");
                results.read_property_by_index(*index).expect("typed");
                results
                    .value_as_string_by_index(*index, c",")
                    .expect("read by index");
            }
        }
    });
    leaks.measure(&label("process an address that is not one"), 5, || {
        results
            .process_ip("not an address")
            .expect_err("text is not an address");
    });

    // A new data set loaded while results from the old one are alive, as an
    // engine refresh does.
    leaks.measure(&label("reload with results alive"), 2, || {
        let old = open();
        let mut alive = old.create_results().expect("results allocate");
        alive.process_ip("1.1.1.1").expect("an address");
        let new = open();
        drop(old);
        let asn = alive.value_as_string("Asn", ",").expect("a value");
        assert!(asn.unwrap_or_default().contains("AS13335"));
        let mut fresh = new.create_results().expect("results allocate");
        fresh.process_ip("9.9.9.9").expect("an address");
        drop(alive);
        drop(fresh);
        drop(new);
    });

    drop(results);
    drop(manager);
}

/// Load the data file with the native API, reload it in place and free it.
fn native_reload(path: &Path) {
    let file = CString::new(path.to_str().expect("a UTF-8 path")).expect("no nul");
    let mut manager = sys::ResourceManager::zeroed();
    let mut exception = sys::Exception::cleared();
    // Safety: the manager is zeroed and every pointer is valid for the calls.
    unsafe {
        let status = sys::fiftyoneDegreesIpiInitManagerFromFile(
            &mut manager,
            std::ptr::addr_of_mut!(sys::fiftyoneDegreesIpiInMemoryConfig),
            std::ptr::null_mut(),
            file.as_ptr(),
            &mut exception,
        );
        assert_eq!(status, sys::StatusCode::Success);
        assert!(exception.is_okay());
        let status = sys::fiftyoneDegreesIpiReloadManagerFromFile(
            &mut manager,
            file.as_ptr(),
            &mut exception,
        );
        assert_eq!(status, sys::StatusCode::Success);
        assert!(exception.is_okay());
        sys::fiftyoneDegreesResourceManagerFree(&mut manager);
    }
}

#[test]
fn every_entry_point_leaves_no_native_memory_allocated() {
    let Some(path) = data_file() else {
        eprintln!("51DEGREES_IPI_PATH names no IP Intelligence data file; skipping");
        return;
    };
    count_native_allocations();
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
    leaks.measure("native reload", 2, || native_reload(&path));
    let bytes = std::fs::read(&path).expect("the data file reads");
    for profile in [PerformanceProfile::InMemory, PerformanceProfile::LowMemory] {
        leaks.measure(
            &format!("{} load from memory, look up and free", profile.as_str()),
            2,
            || {
                let manager = ipi::Manager::open_from_memory(bytes.clone(), profile, None)
                    .expect("the data loads");
                manager.data_set_published();
                manager.data_set_next_update();
                let mut results = manager.create_results().expect("results allocate");
                drop(manager);
                results
                    .process_ip("1.1.1.1")
                    .expect("an address is processed");
                let asn = results.value_as_string("Asn", ",").expect("a value");
                assert!(asn.unwrap_or_default().contains("AS13335"));
            },
        );
    }
    leaks.measure("refuse half the data from memory", 3, || {
        let half = bytes[..bytes.len() / 2].to_vec();
        let opened = ipi::Manager::open_from_memory(half, PerformanceProfile::InMemory, None);
        assert!(opened.is_err(), "half the data does not load");
    });
    leaks.measure("open a missing file", 3, || {
        let opened =
            ipi::Manager::open(path.with_extension("missing"), PerformanceProfile::Default);
        assert!(opened.is_err(), "a missing file does not load");
    });
    let not_data = std::env::temp_dir().join("fiftyone-native-ipi-not-a-data-file.ipi");
    std::fs::write(&not_data, vec![7u8; 64 * 1024]).expect("a temporary file");
    for profile in [PerformanceProfile::InMemory, PerformanceProfile::LowMemory] {
        leaks.measure(
            &format!("{} open a file that is not a data file", profile.as_str()),
            3,
            || {
                let opened = ipi::Manager::open(&not_data, profile);
                assert!(opened.is_err(), "the file does not load");
            },
        );
    }
    std::fs::remove_file(&not_data).ok();

    leaks.compare("everything freed", baseline);
    leaks.assert_none();
}
