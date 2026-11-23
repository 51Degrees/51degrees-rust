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

//! The source of time.

use std::sync::Arc;
use std::time::SystemTime;

/// Tells a cache and an [`LruStore`](crate::LruStore) the current time.
///
/// Every time the crate uses comes from a clock, so a host whose platform
/// has no system clock supplies one, and tests move time on without
/// sleeping. Wall clock time is used because store entries record when they
/// were written, and those times are read back by other processes.
///
/// Any `Fn() -> SystemTime` closure is a clock.
pub trait Clock: Send + Sync {
    /// The current time.
    fn now(&self) -> SystemTime;
}

impl<F> Clock for F
where
    F: Fn() -> SystemTime + Send + Sync,
{
    fn now(&self) -> SystemTime {
        self()
    }
}

/// The system clock, [`SystemTime::now`].
///
/// Not available on `wasm32-unknown-unknown`, where the standard library
/// has no clock and `SystemTime::now` panics. A host there supplies its own
/// [`Clock`], for example one reading the JavaScript `Date.now()`.
#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// The clock a builder was given, or the system clock.
///
/// # Panics
///
/// On `wasm32-unknown-unknown` when no clock was given, because there is no
/// system clock to fall back on.
pub(crate) fn given_or_system(clock: Option<Arc<dyn Clock>>) -> Arc<dyn Clock> {
    match clock {
        Some(clock) => clock,
        None => system(),
    }
}

#[cfg(not(all(target_family = "wasm", target_os = "unknown")))]
fn system() -> Arc<dyn Clock> {
    Arc::new(SystemClock)
}

#[cfg(all(target_family = "wasm", target_os = "unknown"))]
fn system() -> Arc<dyn Clock> {
    panic!(
        "wasm32-unknown-unknown has no system clock, so give the builder \
         a Clock"
    )
}
