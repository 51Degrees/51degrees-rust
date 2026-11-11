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

//! A manager shared by more threads than its data set has file handles
//! serves every one of them.
//!
//! A data set that reads from its file after it is loaded holds a fixed pool
//! of file handles. Each test shares one opened with the `LowMemory` profile,
//! which reads everything from the file, across more threads than the pool
//! holds. The IP Intelligence library ends the process when a read of a
//! property's values finds no free handle, so a failure of that test shows
//! as this test binary exiting rather than as a failed assertion.

use std::path::PathBuf;
use std::sync::{Arc, Barrier};

use fiftyone_native::PerformanceProfile;

/// Threads sharing one manager, more than the handles in either pool below.
const THREADS: usize = 32;

/// Detections or lookups each thread runs.
const ROUNDS: usize = 10;

fn data_file(variable: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var(variable).ok()?);
    path.is_file().then_some(path)
}

/// Run `work` on [`THREADS`] threads at once and return the errors they
/// report.
fn on_every_thread(work: impl Fn(usize) -> Vec<String> + Send + Sync + 'static) -> Vec<String> {
    let work = Arc::new(work);
    let start = Arc::new(Barrier::new(THREADS));
    let threads: Vec<_> = (0..THREADS)
        .map(|thread| {
            let (work, start) = (Arc::clone(&work), Arc::clone(&start));
            std::thread::spawn(move || {
                start.wait();
                work(thread)
            })
        })
        .collect();
    threads
        .into_iter()
        .flat_map(|thread| thread.join().expect("a thread does not panic"))
        .collect()
}

#[cfg(feature = "dd")]
#[test]
fn device_detection_serves_more_threads_than_file_handles() {
    use fiftyone_native::dd;
    let Some(path) = data_file("_51DEGREES_DD_PATH") else {
        eprintln!("_51DEGREES_DD_PATH names no Hash data file; skipping");
        return;
    };
    // Two handles, so the threads must share them.
    let manager =
        dd::Manager::open_with_options(&path, PerformanceProfile::LowMemory, None, Some(2))
            .expect("the data file loads");
    let names = manager.property_names();
    let user_agents = [
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
        "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 \
         (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1",
    ];
    let errors = on_every_thread(move |thread| {
        let mut errors = Vec::new();
        for round in 0..ROUNDS {
            let mut results = manager.create_results().expect("results allocate");
            let user_agent = user_agents[(thread + round) % user_agents.len()];
            if let Err(error) = results.process_user_agent(user_agent) {
                errors.push(error.to_string());
                continue;
            }
            for name in &names {
                if let Err(error) = results.value_as_string(name, ",") {
                    errors.push(error.to_string());
                }
            }
        }
        errors
    });
    assert!(
        errors.is_empty(),
        "{} calls failed, the first: {}",
        errors.len(),
        errors[0]
    );
}

#[cfg(feature = "ipi")]
#[test]
fn ip_intelligence_serves_more_threads_than_file_handles() {
    use fiftyone_native::ipi;
    let Some(path) = data_file("_51DEGREES_IPI_PATH") else {
        eprintln!("_51DEGREES_IPI_PATH names no IP Intelligence data file; skipping");
        return;
    };
    // The profile's own pool of ten handles.
    let manager =
        ipi::Manager::open(&path, PerformanceProfile::LowMemory).expect("the data file loads");
    let names = manager.available_property_names();
    let addresses = ["1.1.1.1", "9.9.9.9", "2606:4700:4700::1111", "8.8.8.8"];
    let errors = on_every_thread(move |thread| {
        let mut errors = Vec::new();
        for round in 0..ROUNDS {
            let mut results = manager.create_results().expect("results allocate");
            let address = addresses[(thread + round) % addresses.len()];
            if let Err(error) = results.process_ip(address) {
                errors.push(error.to_string());
                continue;
            }
            for name in &names {
                if let Err(error) = results.values_weighted(name) {
                    errors.push(error.to_string());
                }
            }
        }
        errors
    });
    assert!(
        errors.is_empty(),
        "{} calls failed, the first: {}",
        errors.len(),
        errors[0]
    );
}
