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

//! A builder base that captures the options every engine builder shares.
//!
//! [`AspectEngineBuilderOptions`] gathers the cache, lazy-loading, cache-hit
//! and property-restriction settings common to all 51Degrees engine builders.
//! A concrete engine builder embeds one of these and reads the captured options
//! when it constructs the engine's [`crate::AspectEngineBase`].

use std::collections::BTreeSet;

use crate::aspect_data::AspectData;
use crate::aspect_engine::AspectEngineBase;
use crate::lazy_loading::LazyLoadingConfiguration;

/// The default engine cache size, matching the caching crate default.
pub const DEFAULT_CACHE_SIZE: usize = 1000;

/// The shared options captured by every aspect engine builder.
///
/// A concrete builder holds one of these and exposes fluent setters that
/// delegate to it, then calls [`AspectEngineBuilderOptions::build_base`] to make
/// the [`AspectEngineBase`] for its engine.
#[derive(Debug, Clone)]
pub struct AspectEngineBuilderOptions {
    cache_size: Option<usize>,
    cache_concurrency: Option<usize>,
    cache_hit_or_miss: bool,
    lazy_loading: Option<LazyLoadingConfiguration>,
    properties: BTreeSet<String>,
}

impl AspectEngineBuilderOptions {
    /// Create options with the defaults: no cache, cache-hit recording off,
    /// lazy loading off, and no property restriction (all properties).
    pub fn new() -> Self {
        AspectEngineBuilderOptions {
            cache_size: None,
            cache_concurrency: None,
            cache_hit_or_miss: false,
            lazy_loading: None,
            properties: BTreeSet::new(),
        }
    }

    /// Enable a results cache of the given total size, with one shard per CPU
    /// core.
    pub fn cache_size(mut self, size: usize) -> Self {
        self.cache_size = Some(size);
        self
    }

    /// Set the cache shard count (concurrency level). Has no effect unless a
    /// cache size is also set.
    pub fn cache_concurrency(mut self, concurrency: usize) -> Self {
        self.cache_concurrency = Some(concurrency);
        self
    }

    /// Enable the default-size results cache. Convenience over
    /// [`AspectEngineBuilderOptions::cache_size`] with [`DEFAULT_CACHE_SIZE`].
    pub fn with_cache(self) -> Self {
        self.cache_size(DEFAULT_CACHE_SIZE)
    }

    /// Enable or disable recording, on each aspect data, whether it came from a
    /// cache hit.
    pub fn cache_hit_or_miss(mut self, enabled: bool) -> Self {
        self.cache_hit_or_miss = enabled;
        self
    }

    /// Enable lazy loading with the supplied configuration.
    pub fn lazy_loading(mut self, config: LazyLoadingConfiguration) -> Self {
        self.lazy_loading = Some(config);
        self
    }

    /// Enable lazy loading with a timeout in milliseconds.
    pub fn lazy_loading_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.lazy_loading = Some(LazyLoadingConfiguration::from_millis(timeout_ms));
        self
    }

    /// Restrict the engine to a single named property, adding to any already
    /// requested.
    pub fn property(mut self, property: impl Into<String>) -> Self {
        self.properties.insert(property.into());
        self
    }

    /// Restrict the engine to the named properties, adding to any already
    /// requested. An empty set means all properties.
    pub fn properties<I, S>(mut self, properties: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.properties
            .extend(properties.into_iter().map(Into::into));
        self
    }

    /// The set of properties the engine has been restricted to. Empty means all
    /// properties, the engine should populate everything its data supports.
    pub fn requested_properties(&self) -> &BTreeSet<String> {
        &self.properties
    }

    /// True if a property has been requested, or if no restriction is in place
    /// (in which case every property is wanted).
    pub fn wants_property(&self, name: &str) -> bool {
        self.properties.is_empty() || self.properties.iter().any(|p| p.eq_ignore_ascii_case(name))
    }

    /// Whether lazy loading is configured.
    pub fn lazy_loading_config(&self) -> Option<&LazyLoadingConfiguration> {
        self.lazy_loading.as_ref()
    }

    /// Build the [`AspectEngineBase`] an engine should embed, applying the
    /// cache, cache-hit and lazy-loading options captured here.
    ///
    /// `D` is the engine's concrete aspect data type. The base is returned ready
    /// for the engine to use from its `process` implementation.
    pub fn build_base<D>(&self) -> AspectEngineBase<D>
    where
        D: AspectData + Clone + Send + Sync + 'static,
    {
        let mut base = AspectEngineBase::new().record_cache_hits(self.cache_hit_or_miss);
        if let Some(size) = self.cache_size {
            let concurrency = self
                .cache_concurrency
                .unwrap_or_else(fiftyone_caching::default_concurrency);
            base = base.with_cache(size, concurrency);
        }
        if let Some(config) = self.lazy_loading {
            base = base.with_lazy_loading(config);
        }
        base
    }
}

impl Default for AspectEngineBuilderOptions {
    fn default() -> Self {
        AspectEngineBuilderOptions::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aspect_data::AspectDataBase;

    #[test]
    fn defaults_are_empty() {
        let opts = AspectEngineBuilderOptions::new();
        assert!(opts.requested_properties().is_empty());
        assert!(
            opts.wants_property("Anything"),
            "no restriction = wants all"
        );
        assert!(opts.lazy_loading_config().is_none());
    }

    #[test]
    fn property_restriction() {
        let opts = AspectEngineBuilderOptions::new()
            .property("IsMobile")
            .properties(["PlatformName", "BrowserName"]);
        assert!(opts.wants_property("ismobile"), "case-insensitive");
        assert!(opts.wants_property("BrowserName"));
        assert!(!opts.wants_property("ScreenWidth"));
        assert_eq!(opts.requested_properties().len(), 3);
    }

    #[test]
    fn builds_base_with_cache_and_lazy() {
        let opts = AspectEngineBuilderOptions::new()
            .cache_size(50)
            .cache_concurrency(2)
            .cache_hit_or_miss(true)
            .lazy_loading_timeout_ms(500);
        let base: AspectEngineBase<AspectDataBase> = opts.build_base();
        assert!(base.has_cache());
        assert!(base.records_cache_hits());
        assert!(base.lazy_loading().is_some());
    }

    #[test]
    fn builds_base_without_cache() {
        let opts = AspectEngineBuilderOptions::new();
        let base: AspectEngineBase<AspectDataBase> = opts.build_base();
        assert!(!base.has_cache());
        assert!(!base.records_cache_hits());
    }
}
