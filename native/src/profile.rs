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

//! The performance profile that selects a native engine configuration.
//!
//! Both Device Detection and IP Intelligence ship the same family of predefined
//! native configurations. They trade memory footprint against lookup speed in
//! the same way, so a single profile enum picks the right configuration global
//! for whichever engine is being initialized. The selection itself lives in the
//! engine specific submodules ([`crate::dd`] and [`crate::ipi`]) because each
//! engine links its own copy of the configuration globals.

/// The performance profile applied when a native engine manager is initialized.
///
/// The variants mirror the predefined native configuration globals exported by
/// each engine (`...InMemoryConfig`, `...HighPerformanceConfig`,
/// `...LowMemoryConfig`, `...BalancedConfig` and `...DefaultConfig`).
///
/// [`PerformanceProfile::Default`] is the recommended starting point. It uses a
/// balanced operating mode without creating a temporary copy of the data file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PerformanceProfile {
    /// Load the data set entirely into memory with no caching. The data file is
    /// not held open after initialization. This is the fastest to query but uses
    /// the most memory.
    InMemory,
    /// Load into memory everything a lookup walks, and keep the data file
    /// open. With some properties named, only their values and the strings
    /// those use are loaded, and the values of the others are read from the
    /// file when asked for. The highest sustained query performance.
    HighPerformance,
    /// Keep a connection to the data file and use no caching, for the smallest
    /// memory footprint at the cost of query speed.
    LowMemory,
    /// Use caching to trade some memory for performance. This is the standard
    /// operating mode for most applications.
    Balanced,
    /// The engine default. Equivalent to [`PerformanceProfile::Balanced`] but
    /// selected through the engine's own default configuration global so it
    /// always tracks whatever the native library treats as its default.
    #[default]
    Default,
}

impl PerformanceProfile {
    /// The lowercase canonical name of this profile, matching the spelling used
    /// in pipeline configuration files across the other 51Degrees ports.
    pub fn as_str(&self) -> &'static str {
        match self {
            PerformanceProfile::InMemory => "inmemory",
            PerformanceProfile::HighPerformance => "highperformance",
            PerformanceProfile::LowMemory => "lowmemory",
            PerformanceProfile::Balanced => "balanced",
            PerformanceProfile::Default => "default",
        }
    }

    /// Parse a profile from a configuration string, ignoring case and any
    /// embedded spaces or hyphens. Returns [`None`] for an unrecognised name.
    ///
    /// For example `"HighPerformance"`, `"high-performance"` and
    /// `"high performance"` all resolve to
    /// [`PerformanceProfile::HighPerformance`].
    pub fn parse(name: &str) -> Option<PerformanceProfile> {
        let normalized: String = name
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match normalized.as_str() {
            "inmemory" => Some(PerformanceProfile::InMemory),
            "highperformance" => Some(PerformanceProfile::HighPerformance),
            "lowmemory" => Some(PerformanceProfile::LowMemory),
            "balanced" => Some(PerformanceProfile::Balanced),
            "default" => Some(PerformanceProfile::Default),
            _ => None,
        }
    }
}
