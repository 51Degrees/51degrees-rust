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

//! Lazy-loading configuration.
//!
//! When lazy loading is enabled, an engine starts processing on a background
//! thread and returns control to the caller immediately. The first property
//! access then blocks until processing completes or the configured timeout
//! expires. This follows the
//! [lazy-loading specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/advanced-features/lazy-loading.md).
//!
//! This crate carries the configuration and the wiring point on the engine
//! ([`crate::AspectEngine::lazy_loading`]). The default synchronous baseline in
//! [`crate::AspectEngineBase`] runs the engine inline; a concrete engine that
//! wants true background processing reads this configuration to decide whether
//! to spawn a worker and how long to wait on it.

use std::time::Duration;

/// The default lazy-loading property timeout, one second.
pub const LAZY_LOADING_DEFAULT_TIMEOUT_MS: u64 = 1000;

/// Configuration for lazy loading of an engine's results.
///
/// The single tunable is the per-property timeout, which is how long a property
/// access will wait for background processing to complete before giving up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LazyLoadingConfiguration {
    /// The timeout to wait for processing to complete when a property is
    /// accessed. If exceeded, the access fails rather than blocking forever.
    property_timeout: Duration,
}

impl LazyLoadingConfiguration {
    /// Create a configuration with the supplied per-property timeout.
    pub fn new(property_timeout: Duration) -> Self {
        LazyLoadingConfiguration { property_timeout }
    }

    /// Create a configuration with a timeout given in milliseconds.
    pub fn from_millis(timeout_ms: u64) -> Self {
        LazyLoadingConfiguration::new(Duration::from_millis(timeout_ms))
    }

    /// The per-property timeout.
    pub fn property_timeout(&self) -> Duration {
        self.property_timeout
    }
}

impl Default for LazyLoadingConfiguration {
    fn default() -> Self {
        LazyLoadingConfiguration::from_millis(LAZY_LOADING_DEFAULT_TIMEOUT_MS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_one_second() {
        assert_eq!(
            LazyLoadingConfiguration::default().property_timeout(),
            Duration::from_millis(1000)
        );
    }

    #[test]
    fn from_millis_round_trips() {
        let cfg = LazyLoadingConfiguration::from_millis(250);
        assert_eq!(cfg.property_timeout(), Duration::from_millis(250));
    }
}
