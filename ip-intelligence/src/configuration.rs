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

//! Pipelines made from a configuration file written for .NET, with the
//! builders .NET's `FiftyOne.IpIntelligence` package gives. See
//! [`fiftyone_pipeline_core::configuration`] for the file's shape and how
//! each builder and setting is found.
//!
//! [`registry`] holds the builders and makes pipelines as .NET's
//! `FiftyOnePipelineBuilder` does, as .NET's console examples and ASP.NET
//! Core integration make a pipeline from a configuration with that
//! builder. So a Sequence element runs first and usage sharing last,
//! unless the configuration has them, and the pipeline setting
//! `SetShareUsage` turns usage sharing off.
//! Without this crate's `share-usage` feature, usage sharing is off unless
//! the configuration sets `SetShareUsage` to true, as in this crate's
//! pipeline builders.
//!
//! The builders are .NET's, under .NET's names:
//!
//! - `SequenceElementBuilder`, `SetHeadersElementBuilder` and
//!   `ShareUsageBuilder` (see [`fiftyone_pipeline_engines_fiftyone::configuration`]).
//! - With the `cloud` feature, `CloudRequestEngineBuilder` (see
//!   [`fiftyone_cloud_request_engine::configuration`]) and
//!   `IpiCloudEngineBuilder` (see
//!   [`fiftyone_ip_intelligence_cloud::configuration`]).
//! - With the `on-premise` feature, `IpiOnPremiseEngineBuilder` (see
//!   [`fiftyone_ip_intelligence_onpremise::configuration`]).
//!
//! .NET's package also has `CloudCountriesTranslationEngineBuilder`,
//! `CountriesTranslationEngineBuilder`, `CountryCodeTranslationEngineBuilder`
//! and `TranslationEngineBuilder`, whose engines this crate does not have.
//! [`add_builders`] adds the builders to a registry holding others, such as
//! the JSON and JavaScript builders a web application adds, or the Device
//! Detection builders. A builder that two crates both add is found once.
//!
//! ```
//! use fiftyone_ip_intelligence::configuration::{self, PipelineOptions};
//! use fiftyone_pipeline_core::PipelineBuilder;
//!
//! let options = PipelineOptions::from_json(
//!     r#"{
//!         "PipelineOptions": {
//!             "Elements": [ { "BuilderName": "SetHeadersElementBuilder" } ],
//!             "BuildParameters": { "SetShareUsage": false }
//!         }
//!     }"#,
//! )?;
//! let pipeline =
//!     PipelineBuilder::new().build_from_configuration(&configuration::registry(), &options)?;
//!
//! let keys: Vec<&str> = pipeline.flow_elements().iter().map(|e| e.data_key()).collect();
//! assert_eq!(keys, ["sequence", "set-headers"]);
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

pub use fiftyone_pipeline_core::configuration::{BuilderRegistry, ElementOptions, PipelineOptions};

/// Whether a pipeline made from a configuration shares usage when the
/// configuration does not say, as this crate's pipeline builders do.
const SHARE_USAGE_BY_DEFAULT: bool = cfg!(feature = "share-usage");

/// Add the builders .NET's `FiftyOne.IpIntelligence` package gives, those
/// of this crate's features, to `registry`. Returns `registry` for
/// chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    fiftyone_pipeline_engines_fiftyone::configuration::add_builders(registry);
    #[cfg(feature = "cloud")]
    {
        fiftyone_cloud_request_engine::configuration::add_builders(registry);
        fiftyone_ip_intelligence_cloud::configuration::add_builders(registry);
    }
    #[cfg(feature = "on-premise")]
    fiftyone_ip_intelligence_onpremise::configuration::add_builders(registry);
    registry
}

/// A registry holding the builders [`add_builders`] adds, which makes
/// pipelines as .NET's `FiftyOnePipelineBuilder` does (see the module
/// documentation).
pub fn registry() -> BuilderRegistry {
    let mut registry = BuilderRegistry::new();
    add_builders(&mut registry);
    fiftyone_pipeline_engines_fiftyone::configuration::use_fiftyone_pipeline_builder_with_share_usage(
        &mut registry,
        SHARE_USAGE_BY_DEFAULT,
    );
    registry
}
