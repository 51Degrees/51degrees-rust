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

//! An idle usage-sharing element uses no processor time, whatever its take
//! timeout. In a test binary of its own, so no other test's work is counted.

#![cfg(any(windows, target_os = "linux"))]

use std::time::Duration;

use fiftyone_pipeline_engines_fiftyone::{ShareUsageConfig, ShareUsageElement};

/// How long the element is left with nothing to do.
const IDLE_MILLISECONDS: u64 = 1_000;

/// The most processor time the process may use while the element is idle. A
/// thread that spins uses all of the idle time.
const MAX_BUSY_MILLISECONDS: u64 = 250;

#[test]
fn an_idle_element_uses_no_processor_time() {
    for take_timeout in [Duration::ZERO, Duration::from_millis(100)] {
        let element = ShareUsageElement::new(
            ShareUsageConfig::builder()
                .take_timeout(take_timeout)
                // Nothing is processed, so nothing is sent here.
                .share_usage_url("http://127.0.0.1:9/")
                .build(),
        );
        let before = process_milliseconds();
        std::thread::sleep(Duration::from_millis(IDLE_MILLISECONDS));
        let busy = process_milliseconds() - before;
        drop(element);
        assert!(
            busy < MAX_BUSY_MILLISECONDS,
            "{busy} ms of processor time in {IDLE_MILLISECONDS} ms idle, with a take \
             timeout of {take_timeout:?}"
        );
    }
}

/// The processor time this process has used, user and kernel, in
/// milliseconds.
#[cfg(windows)]
fn process_milliseconds() -> u64 {
    /// The `FILETIME` structure, in hundreds of nanoseconds.
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn GetProcessTimes(
            process: isize,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }

    let mut creation = FileTime::default();
    let mut exit = FileTime::default();
    let mut kernel = FileTime::default();
    let mut user = FileTime::default();
    // SAFETY: the pseudo handle of the current process is always valid, and
    // the four pointers are to writable structures of the expected layout.
    let succeeded = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    };
    assert_ne!(succeeded, 0, "GetProcessTimes failed");
    let hundreds_of_nanoseconds =
        |time: &FileTime| (u64::from(time.high) << 32) | u64::from(time.low);
    (hundreds_of_nanoseconds(&kernel) + hundreds_of_nanoseconds(&user)) / 10_000
}

/// The processor time this process has used, user and kernel, in
/// milliseconds.
#[cfg(target_os = "linux")]
fn process_milliseconds() -> u64 {
    // Fields 14 and 15 of /proc/self/stat are the user and kernel time in
    // clock ticks, a hundredth of a second each. The process name in field 2
    // can hold spaces, so the fields are counted from its closing bracket,
    // after which field 3 comes first.
    let stat = std::fs::read_to_string("/proc/self/stat").expect("/proc/self/stat reads");
    let after_name = &stat[stat.rfind(')').expect("the name is bracketed") + 1..];
    let fields: Vec<&str> = after_name.split_whitespace().collect();
    let ticks = |field: usize| -> u64 { fields[field - 3].parse().expect("a tick count") };
    (ticks(14) + ticks(15)) * 10
}
