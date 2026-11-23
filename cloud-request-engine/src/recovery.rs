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

//! The recovery-mode gate that suspends cloud requests after repeated failures.
//!
//! This implements the
//! [recovery-mode section](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/cloud-request-engine.md#recovery-mode)
//! of the specification.
//!
//! The gate counts failures within a sliding window. Once the count reaches the
//! configured threshold, it opens a recovery period during which every request
//! is short-circuited and reported as temporarily unavailable. After the
//! recovery period elapses, requests are allowed again and the failure count is
//! reset, so a single later failure does not immediately re-trip the gate.
//!
//! The gate is shared across threads (one engine, many concurrent flow data),
//! so its state lives behind a [`Mutex`]. The lock is held only for the brief
//! check or record, never across an HTTP call.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Configuration for the [`RecoveryGate`].
///
/// All three knobs come straight from the engine builder and ultimately the
/// specification's configuration table.
#[derive(Debug, Clone, Copy)]
pub struct RecoveryConfig {
    /// The number of failures within [`RecoveryConfig::window`] that opens a
    /// recovery period.
    pub failures_to_enter_recovery: u32,
    /// The sliding window within which the failures must occur.
    pub window: Duration,
    /// The recovery-period duration. A zero duration disables recovery entirely,
    /// so the gate never blocks a request.
    pub recovery: Duration,
}

impl RecoveryConfig {
    /// True if recovery mode is enabled (the recovery period is non-zero).
    pub fn enabled(&self) -> bool {
        !self.recovery.is_zero()
    }
}

/// Internal mutable state, guarded by a single mutex.
#[derive(Debug)]
struct State {
    /// The timestamps of failures inside the current window. Pruned on each
    /// access so the slice only ever holds recent failures.
    failures: Vec<Instant>,
    /// When set, requests are suspended until this instant.
    recovering_until: Option<Instant>,
}

/// A thread-safe gate that opens a recovery period after repeated failures.
#[derive(Debug)]
pub struct RecoveryGate {
    config: RecoveryConfig,
    state: Mutex<State>,
}

impl RecoveryGate {
    /// Create a gate with the given configuration.
    pub fn new(config: RecoveryConfig) -> Self {
        RecoveryGate {
            config,
            state: Mutex::new(State {
                failures: Vec::new(),
                recovering_until: None,
            }),
        }
    }

    /// The configuration the gate was made with.
    pub(crate) fn config(&self) -> RecoveryConfig {
        self.config
    }

    /// Check whether a request is currently allowed.
    ///
    /// Returns `Ok(())` when the request may proceed. Returns `Err` with an
    /// explanatory message while the gate is in a recovery period. The check is
    /// evaluated against the supplied `now` so callers can test deterministically;
    /// production code passes [`Instant::now`].
    pub fn check_at(&self, now: Instant) -> Result<(), String> {
        if !self.config.enabled() {
            return Ok(());
        }
        let mut state = self.state.lock().expect("recovery gate mutex poisoned");
        if let Some(until) = state.recovering_until {
            if now < until {
                let remaining = until.saturating_duration_since(now);
                return Err(format!(
                    "sending requests to the cloud service is temporarily \
                     restricted due to recent failures; recovery resumes in \
                     {:.1}s",
                    remaining.as_secs_f64()
                ));
            }
            // The recovery period has elapsed: leave recovery and reset the
            // failure history so a single later failure does not re-trip it.
            state.recovering_until = None;
            state.failures.clear();
        }
        Ok(())
    }

    /// Record a failed request at the supplied `now`.
    ///
    /// Old failures outside the window are pruned, the new failure is added, and
    /// if the count has reached the threshold a recovery period is opened. Has no
    /// effect when recovery is disabled.
    pub fn record_failure_at(&self, now: Instant) {
        if !self.config.enabled() {
            return;
        }
        let mut state = self.state.lock().expect("recovery gate mutex poisoned");
        let window = self.config.window;
        state
            .failures
            .retain(|t| now.saturating_duration_since(*t) < window);
        state.failures.push(now);
        if state.failures.len() as u32 >= self.config.failures_to_enter_recovery {
            state.recovering_until = Some(now + self.config.recovery);
        }
    }

    /// Record a successful request, clearing the failure history at `now`.
    ///
    /// A success is taken as evidence the service is healthy, so the window is
    /// emptied. This matches the windowed handler resetting on a good response.
    pub fn record_success(&self) {
        if !self.config.enabled() {
            return;
        }
        let mut state = self.state.lock().expect("recovery gate mutex poisoned");
        state.failures.clear();
        state.recovering_until = None;
    }

    /// Convenience wrapper around [`RecoveryGate::check_at`] using the current
    /// instant.
    pub fn check(&self) -> Result<(), String> {
        self.check_at(Instant::now())
    }

    /// Convenience wrapper around [`RecoveryGate::record_failure_at`] using the
    /// current instant.
    pub fn record_failure(&self) {
        self.record_failure_at(Instant::now());
    }

    /// True if the gate is currently in a recovery period at `now`.
    pub fn is_recovering_at(&self, now: Instant) -> bool {
        let state = self.state.lock().expect("recovery gate mutex poisoned");
        match state.recovering_until {
            Some(until) => now < until,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(threshold: u32, window_s: u64, recovery_s: f64) -> RecoveryConfig {
        RecoveryConfig {
            failures_to_enter_recovery: threshold,
            window: Duration::from_secs(window_s),
            recovery: Duration::from_secs_f64(recovery_s),
        }
    }

    #[test]
    fn disabled_recovery_never_blocks() {
        let gate = RecoveryGate::new(config(1, 100, 0.0));
        let now = Instant::now();
        gate.record_failure_at(now);
        gate.record_failure_at(now);
        assert!(gate.check_at(now).is_ok());
    }

    #[test]
    fn opens_recovery_after_threshold() {
        let gate = RecoveryGate::new(config(3, 100, 60.0));
        let now = Instant::now();
        gate.record_failure_at(now);
        gate.record_failure_at(now);
        assert!(gate.check_at(now).is_ok(), "below threshold still allowed");
        gate.record_failure_at(now);
        assert!(gate.check_at(now).is_err(), "threshold reached, blocked");
        assert!(gate.is_recovering_at(now));
    }

    #[test]
    fn recovery_expires_and_resets() {
        let gate = RecoveryGate::new(config(2, 100, 10.0));
        let start = Instant::now();
        gate.record_failure_at(start);
        gate.record_failure_at(start);
        assert!(gate.check_at(start).is_err());

        // After the recovery period, requests are allowed again.
        let later = start + Duration::from_secs(11);
        assert!(gate.check_at(later).is_ok());
        assert!(!gate.is_recovering_at(later));

        // A single failure now does not immediately re-trip the gate, because
        // the history was reset on leaving recovery.
        gate.record_failure_at(later);
        assert!(gate.check_at(later).is_ok());
    }

    #[test]
    fn old_failures_fall_out_of_window() {
        let gate = RecoveryGate::new(config(2, 10, 60.0));
        let start = Instant::now();
        gate.record_failure_at(start);
        // The first failure is now outside the 10s window, so the second is
        // counted as the only recent failure and does not reach the threshold.
        let much_later = start + Duration::from_secs(20);
        gate.record_failure_at(much_later);
        assert!(gate.check_at(much_later).is_ok());
    }

    #[test]
    fn success_clears_history() {
        let gate = RecoveryGate::new(config(2, 100, 60.0));
        let now = Instant::now();
        gate.record_failure_at(now);
        gate.record_success();
        gate.record_failure_at(now);
        assert!(gate.check_at(now).is_ok(), "success reset the count");
    }
}
