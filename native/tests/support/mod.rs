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

//! Counts the blocks a native library holds through its pluggable
//! allocator, and measures stages of work against that count.
//!
//! The count is of blocks as well as bytes, so a block of no bytes that is
//! never freed still shows. A test file that uses this installs the
//! allocator before any native call, and holds only one test, because every
//! test in a process shares the native allocator.

use std::collections::HashMap;
use std::os::raw::c_void;
use std::sync::{Mutex, PoisonError};

/// The size of every block allocated since counting started and not yet
/// freed, by address.
static LIVE: Mutex<Option<HashMap<usize, usize>>> = Mutex::new(None);

fn with_live(action: impl FnOnce(&mut HashMap<usize, usize>)) {
    let mut live = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
    action(live.get_or_insert_with(HashMap::new));
}

/// Record a block the native library allocated.
pub fn allocated(pointer: *mut c_void, size: usize) {
    if !pointer.is_null() {
        with_live(|live| {
            live.insert(pointer as usize, size);
        });
    }
}

/// Record a block the native library freed. A block allocated before
/// counting started is ignored.
pub fn freed(pointer: *mut c_void) {
    with_live(|live| {
        live.remove(&(pointer as usize));
    });
}

/// The blocks and bytes held.
pub fn held() -> (usize, usize) {
    let mut held = (0, 0);
    with_live(|live| held = (live.len(), live.values().sum()));
    held
}

/// The stages that left blocks allocated.
#[derive(Default)]
pub struct Leaks(pub Vec<String>);

impl Leaks {
    /// Run `stage` `repeats` times so caches and buffers that grow to fit
    /// settle, then `repeats` more times, recording any blocks the second
    /// run left allocated.
    pub fn measure(&mut self, name: &str, repeats: usize, mut stage: impl FnMut()) {
        for _ in 0..repeats {
            stage();
        }
        let before = held();
        for _ in 0..repeats {
            stage();
        }
        self.compare(&format!("{name} {repeats} times"), before);
    }

    /// Record any change from `before` in the blocks held.
    pub fn compare(&mut self, name: &str, before: (usize, usize)) {
        let after = held();
        println!("{name}: {before:?} -> {after:?}");
        if after != before {
            self.0.push(format!(
                "{name}: {} blocks, {} bytes",
                after.0 as i64 - before.0 as i64,
                after.1 as i64 - before.1 as i64
            ));
        }
    }

    /// Fail with every stage that left blocks allocated.
    pub fn assert_none(&self) {
        assert!(
            self.0.is_empty(),
            "native memory left allocated:\n{}",
            self.0.join("\n")
        );
    }
}
