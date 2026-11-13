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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-ip-intelligence-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-ip-intelligence-lib.rs&utm_term=logo)
//!
//! The IP-intelligence facade.
//!
//! This crate is the one an application depends on to use IP
//! Intelligence, whether against the 51Degrees cloud service or a local `.ipi`
//! data file. It pulls the cloud and on-premise engines together and gives a
//! single fluent entry point, [`IpIntelligencePipelineBuilder`], that assembles a
//! ready-to-process [`Pipeline`](fiftyone_pipeline_core::Pipeline) for the chosen
//! deployment.
//!
//! # What it provides
//!
//! - **Re-exports.** The element-data read trait
//!   [`IpIntelligenceData`], the concrete [`IpIntelligenceDataBase`], the shared
//!   [`IP_DATA_KEY`] used to read the result out of a flow data, the
//!   [`WeightedValue`] and [`AspectPropertyValue`] value wrappers, and the
//!   underlying engine builders. An application can therefore depend only on this
//!   crate and never name the sub-crates directly.
//! - **A pipeline builder.** [`IpIntelligencePipelineBuilder`] with two
//!   constructors, one per deployment:
//!   - [`IpIntelligencePipelineBuilder::cloud`] takes a resource key and yields a
//!     [`CloudIpIntelligencePipelineBuilder`] that assembles
//!     `[CloudRequestEngine, IpIntelligenceCloudEngine]`.
//!   - [`IpIntelligencePipelineBuilder::on_premise`] takes a `.ipi` data file path
//!     and yields an [`OnPremiseIpIntelligencePipelineBuilder`] that assembles a
//!     single [`IpIntelligenceOnPremiseEngine`].
//!
//!   Usage sharing is on by default in both builders, which then add a
//!   sequence element before the engines and a usage-sharing element after
//!   them. A console or off-line tool turns it off with `share_usage(false)`.
//!
//! Both deployments populate the same [`IpIntelligenceDataBase`] under the same
//! [`IP_DATA_KEY`], so an application can swap one for the other without touching
//! its read code.
//!
//! # Data updates are on by default
//!
//! An on-premise pipeline keeps its data file current, as the .NET and Java
//! pipelines do. The builder creates a `DataUpdateService` the pipeline owns,
//! which polls the 51Degrees Distributor every thirty minutes, staggered by up
//! to ten minutes, and refreshes the engine when the file changes on disk. The
//! Distributor serves nothing without the licence keys set with
//! `.data_update_license_keys(..)`, so a build with polling on and no key
//! fails with an error that says so, as it does in .NET, and a deployment
//! without a key turns automatic updates off with `.auto_update(false)`, which
//! stops the polling and the file watcher together, as the specification's
//! master switch does. `.file_system_watcher(false)` turns only the watcher
//! off, and `pipeline.resource::<DataUpdateService>()` reaches the service for
//! a check on demand.
//!
//! # From a configuration file
//!
//! [`configuration::registry`] makes a pipeline from a configuration file
//! written for .NET's `FiftyOne.IpIntelligence` package, finding each
//! builder and setting by .NET's names, and adding the Sequence element and
//! usage sharing as .NET's `FiftyOnePipelineBuilder` does. See the
//! [`configuration`] module.
//!
//! # Features
//!
//! The two deployments are behind cargo features so a build that only needs one
//! does not compile (or link) the other. Both are on by default.
//!
//! - `cloud` (default): the cloud engine and the cloud request engine.
//! - `on-premise` (default): the native on-premise engine.
//! - `share-usage` (default): usage sharing on in both builders and in
//!   pipelines made from a configuration, and the built-in transport it
//!   sends with.
//!
//! See [`IpIntelligencePipelineBuilder::cloud`] and
//! [`IpIntelligencePipelineBuilder::on_premise`] for runnable examples of each
//! deployment.

#![warn(missing_docs)]

// ---------------------------------------------------------------------------
// Re-exports
// ---------------------------------------------------------------------------

// The shared element-data model. These are the types an application reads its
// results through, regardless of which engine produced them.
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

pub use fiftyone_ip_intelligence_shared::{
    IpIntelligenceData, IpIntelligenceDataBase, WeightedStore, IP_DATA_KEY, IP_DATA_KEY_NAME,
};

// The value wrappers the typed accessors and the by-name getters return.
// Re-exported here so a consumer does not have to name the core or engines
// crate to handle a result.
pub use fiftyone_pipeline_core::{WeightedValue, WktString};
pub use fiftyone_pipeline_engines::AspectPropertyValue;

// The performance profile selects how the on-premise data file is opened. It is
// re-exported so a consumer can tune the on-premise builder without naming the
// native crate.
#[cfg(feature = "on-premise")]
pub use fiftyone_native::PerformanceProfile;

// The underlying engine builders, for an application that wants to assemble its
// own pipeline rather than use the convenience builder, or to add a 51Degrees
// element (sequence, share-usage) around the engine.
#[cfg(feature = "cloud")]
pub use fiftyone_cloud_request_engine::{
    CloudEngineState, CloudRequestEngine, CloudRequestEngineBuilder,
};
#[cfg(feature = "cloud")]
pub use fiftyone_ip_intelligence_cloud::{
    IpIntelligenceCloudEngine, IpIntelligenceCloudEngineBuilder,
};
#[cfg(feature = "on-premise")]
#[allow(deprecated)]
pub use fiftyone_ip_intelligence_onpremise::IP_EVIDENCE_KEYS;
#[cfg(feature = "on-premise")]
pub use fiftyone_ip_intelligence_onpremise::{
    IpIntelligenceOnPremiseEngine, IpIntelligenceOnPremiseEngineBuilder, DEFAULT_DATA_SOURCE_TIER,
};

pub mod configuration;

/// The examples in the README are compiled as documentation tests, so the
/// documented way to use this crate cannot quietly stop working.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeExamples;

#[cfg(feature = "cloud")]
mod cloud;
#[cfg(feature = "on-premise")]
mod on_premise;

#[cfg(feature = "cloud")]
pub use cloud::CloudIpIntelligencePipelineBuilder;
#[cfg(feature = "on-premise")]
pub use on_premise::OnPremiseIpIntelligencePipelineBuilder;

/// The entry point for building an IP-intelligence
/// [`Pipeline`](fiftyone_pipeline_core::Pipeline).
///
/// It carries no state. It exists only to expose the two deployment
/// constructors, each returning a deployment-specific builder that assembles
/// the right engines into an
/// `Arc<`[`Pipeline`](fiftyone_pipeline_core::Pipeline)`>`:
///
/// - [`IpIntelligencePipelineBuilder::cloud`] for the 51Degrees cloud service.
/// - [`IpIntelligencePipelineBuilder::on_premise`] for a local `.ipi` data file.
pub struct IpIntelligencePipelineBuilder;

impl IpIntelligencePipelineBuilder {
    /// Start building a pipeline that uses the 51Degrees cloud service for IP
    /// Intelligence.
    ///
    /// `resource_key` authenticates the request and selects which properties are
    /// returned. Create one for free at <https://51degrees.com/configure?utm_source=code&utm_medium=comment&utm_campaign=51degrees-rust&utm_content=ip-intelligence-src-lib.rs&utm_term=cloud>.
    ///
    /// Returns a [`CloudIpIntelligencePipelineBuilder`] to configure (endpoint,
    /// origin, timeout) and [`build`](CloudIpIntelligencePipelineBuilder::build)
    /// into a pipeline of `[CloudRequestEngine, IpIntelligenceCloudEngine]`.
    ///
    /// ```no_run
    /// use fiftyone_ip_intelligence::{IpIntelligencePipelineBuilder, IpIntelligenceData, IP_DATA_KEY};
    /// use fiftyone_pipeline_core::Evidence;
    ///
    /// # fn main() -> fiftyone_pipeline_core::Result<()> {
    /// let pipeline = IpIntelligencePipelineBuilder::cloud("your-resource-key").build()?;
    ///
    /// let mut data = pipeline.create_flow_data_with(
    ///     Evidence::builder().add("query.client-ip-51d", "185.28.167.78").build(),
    /// );
    /// data.process()?;
    ///
    /// if let Some(ip) = data.get(IP_DATA_KEY) {
    ///     if let Ok(country_code) = ip.country_code().value() {
    ///         println!("country code = {country_code}");
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "cloud")]
    pub fn cloud(resource_key: impl Into<String>) -> CloudIpIntelligencePipelineBuilder {
        CloudIpIntelligencePipelineBuilder::new(resource_key)
    }

    /// Start building a pipeline that uses a local `.ipi` data file for IP
    /// Intelligence.
    ///
    /// `data_file_path` is the full path to the `.ipi` data file. Returns an
    /// [`OnPremiseIpIntelligencePipelineBuilder`] to configure (performance
    /// profile, property restriction, automatic updates) and
    /// [`build`](OnPremiseIpIntelligencePipelineBuilder::build) into a pipeline
    /// containing a single on-premise engine.
    ///
    /// ```no_run
    /// use fiftyone_ip_intelligence::{IpIntelligencePipelineBuilder, IP_DATA_KEY, PerformanceProfile};
    /// use fiftyone_pipeline_core::Evidence;
    ///
    /// # fn main() -> fiftyone_pipeline_core::Result<()> {
    /// let pipeline = IpIntelligencePipelineBuilder::on_premise("51Degrees-IPIV4AsnIpiV41.ipi")
    ///     .performance_profile(PerformanceProfile::HighPerformance)
    ///     .property("Asn")
    ///     // Off because the free ASN file has no licence key for updates.
    ///     .auto_update(false)
    ///     .build()?;
    ///
    /// let mut data = pipeline.create_flow_data_with(
    ///     Evidence::builder().add("server.client-ip", "1.1.1.1").build(),
    /// );
    /// data.process()?;
    ///
    /// if let Some(ip) = data.get(IP_DATA_KEY) {
    ///     // Asn is a plain string property; read it through the dynamic bag.
    ///     if let Ok(asn) = ip.string("Asn").value() {
    ///         println!("asn = {asn}");
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "on-premise")]
    pub fn on_premise(
        data_file_path: impl Into<std::path::PathBuf>,
    ) -> OnPremiseIpIntelligencePipelineBuilder {
        OnPremiseIpIntelligencePipelineBuilder::new(data_file_path)
    }
}
