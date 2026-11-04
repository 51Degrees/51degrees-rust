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

//! Property name lookups on a shared Device Detection manager must add up
//! across threads. Every property read resolves its name through
//! `Manager::required_property_index`, so a lock on that path makes threads
//! queue on one another and total throughput falls as threads are added.
//!
//! This is its own test binary so no other test competes for the CPU while it
//! measures. It needs the Lite Hash data file and skips when the file is
//! absent or the machine has a single CPU. It also skips in an unoptimised
//! build, where hashing the name costs so much more than the lock that a
//! shared lock still lets four threads do more than one. Run it with
//! `cargo test --release -p fiftyone-native --test lookup_scaling`, as the
//! pull request workflow does.

#![cfg(feature = "dd")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use fiftyone_native::{dd, PerformanceProfile};

/// How long each measurement runs.
const WINDOW_MILLISECONDS: u64 = 300;

/// Measurements per thread count. The best is kept, so a moment of noise on a
/// shared machine does not decide the result.
const ROUNDS: usize = 3;

/// The most threads measured, which keeps the test meaningful on the four CPU
/// machines continuous integration runs on.
const MAX_THREADS: usize = 4;

/// The Lite Hash file named by `51DEGREES_DD_PATH`, else the copy in the
/// `cxx` submodule.
fn lite_data_file() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("51DEGREES_DD_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .to_path_buf();
    let relative = ["cxx", "data", "device-detection", "51Degrees-LiteV4.1.hash"]
        .iter()
        .collect::<PathBuf>();
    for root in [Some(workspace.as_path()), workspace.parent()]
        .into_iter()
        .flatten()
    {
        let path = root.join(&relative);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// Lookups per second from `threads` threads resolving every property name
/// in turn for one window.
fn lookups_per_second(manager: &Arc<dd::Manager>, names: &Arc<Vec<String>>, threads: usize) -> f64 {
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicU64::new(0));
    let start = Arc::new(Barrier::new(threads + 1));
    let workers: Vec<_> = (0..threads)
        .map(|_| {
            let (manager, names) = (Arc::clone(manager), Arc::clone(names));
            let (stop, done, start) = (Arc::clone(&stop), Arc::clone(&done), Arc::clone(&start));
            std::thread::spawn(move || {
                start.wait();
                let mut count = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    for name in names.iter() {
                        std::hint::black_box(manager.required_property_index(name));
                    }
                    count += names.len() as u64;
                }
                done.fetch_add(count, Ordering::Relaxed);
            })
        })
        .collect();
    start.wait();
    let began = Instant::now();
    std::thread::sleep(Duration::from_millis(WINDOW_MILLISECONDS));
    stop.store(true, Ordering::Relaxed);
    for worker in workers {
        worker.join().expect("a lookup thread should not panic");
    }
    done.load(Ordering::Relaxed) as f64 / began.elapsed().as_secs_f64()
}

/// The best of several measurements.
fn best_lookups_per_second(
    manager: &Arc<dd::Manager>,
    names: &Arc<Vec<String>>,
    threads: usize,
) -> f64 {
    (0..ROUNDS)
        .map(|_| lookups_per_second(manager, names, threads))
        .fold(0.0, f64::max)
}

/// Several threads together resolve at least as many property names a second
/// as one thread alone. A shared lock on the lookup fails this by a wide
/// margin, because every lookup then writes to the same cache line.
#[test]
fn property_lookups_add_up_across_threads() {
    if cfg!(debug_assertions) {
        eprintln!("the lookup scaling test measures optimised builds only; skipping");
        return;
    }
    let Some(data_file) = lite_data_file() else {
        eprintln!("no Lite Hash data file found; skipping the lookup scaling test");
        return;
    };
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(MAX_THREADS);
    if threads < 2 {
        eprintln!("a single CPU cannot show lookups scaling; skipping");
        return;
    }
    let manager = dd::Manager::open(&data_file, PerformanceProfile::Default)
        .expect("the Lite Hash data file should load");
    let names = Arc::new(manager.property_names());
    assert!(!names.is_empty(), "the data file should list properties");
    assert!(
        names
            .iter()
            .all(|name| manager.required_property_index(name).is_some()),
        "every listed property should resolve to an index"
    );

    let one = best_lookups_per_second(&manager, &names, 1);
    let many = best_lookups_per_second(&manager, &names, threads);
    eprintln!(
        "property lookups a second: 1 thread {one:.0}, {threads} threads {many:.0} ({:.2}x)",
        many / one
    );
    assert!(
        many >= one,
        "{threads} threads resolved {many:.0} property names a second against \
         {one:.0} for one thread, so lookups are queueing on a shared lock"
    );
}
