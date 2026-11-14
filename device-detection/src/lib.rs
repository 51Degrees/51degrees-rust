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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-lib.rs&utm_term=logo)
//!
//! # 51Degrees device detection (the unified facade)
//!
//! The single entry point for device detection in Rust. It re-exports
//! everything a consumer needs so that one dependency line is enough, and it
//! adds a convenience [`DeviceDetectionPipelineBuilder`] that assembles a
//! ready-to-run [`Pipeline`] for either deployment.
//!
//! ## What this crate gathers up
//!
//! Device detection has two deployments behind one result contract. Both write
//! a [`DeviceDataBase`] under [`DEVICE_DATA_KEY`], so application code that
//! reads the result is identical whichever deployment produced it.
//!
//! - On-premise: the native Hash engine in
//!   [`fiftyone_device_detection_onpremise`], re-exported here as
//!   [`DeviceDetectionOnPremiseEngineBuilder`] (behind the `on-premise` feature).
//! - Cloud: the cloud engine in [`fiftyone_device_detection_cloud`] fed by a
//!   [`CloudRequestEngine`], re-exported here as [`DeviceDetectionCloudEngine`]
//!   and [`DeviceDetectionCloudEngineBuilder`] (behind the `cloud` feature).
//! - The shared model: [`DeviceData`], [`DeviceDataBase`], [`DEVICE_DATA_KEY`]
//!   and [`AspectPropertyValue`], and the deprecated UA-CH high-entropy
//!   decoder [`UachJsConversionElement`], which no engine needs.
//! - The core types a caller needs to drive a pipeline: [`Pipeline`],
//!   [`FlowData`], [`Evidence`] and friends.
//!
//! ## Cargo features
//!
//! - `cloud` (default): the cloud engine and its request engine.
//! - `on-premise` (default): the native Hash engine.
//! - `share-usage` (default): usage sharing on in both builders and in
//!   pipelines made from a configuration, and the built-in transport it
//!   sends with.
//!
//! The default `["cloud", "on-premise", "share-usage"]` builds the full SDK. A
//! cloud-only consumer that does not want to link the native library sets
//! `default-features = false, features = ["cloud"]`.
//!
//! ## A note on the convenience builder
//!
//! The device-detection specification recommends building pipelines explicitly
//! so the element wiring is visible. [`DeviceDetectionPipelineBuilder`] is a
//! convenience for the common console-style case and the examples. Usage
//! sharing is on by default, as it is in the other languages' builders, and
//! with it on the builder also adds a [`SequenceElement`]. A web deployment
//! typically adds the JSON and JavaScript builders and a
//! [`SetHeadersElement`](fiftyone_pipeline_engines_fiftyone::SetHeadersElement)
//! through the web integration crate rather than this builder.
//!
//! ## Usage sharing is on by default
//!
//! With the default `share-usage` feature, both sub-builders add a
//! [`ShareUsageElement`], which sends evidence from the requests the pipeline
//! processes to 51Degrees to improve detection. A console or off-line tool
//! turns it off with `.share_usage(false)`, because evidence 51Degrees already
//! holds adds nothing. With the feature off, the builders leave usage sharing
//! off unless `.share_usage(true)` is called, and the element sends nothing
//! unless the `fiftyone-pipeline-engines-fiftyone/share-usage-transport`
//! feature is enabled. See the [`fiftyone_pipeline_engines_fiftyone`] crate
//! docs for what the element sends and how.
//!
//! ## Data updates are on by default
//!
//! An on-premise pipeline keeps its data file current, as the .NET and Java
//! pipelines do. The builder creates a `DataUpdateService` the pipeline owns,
//! which polls the 51Degrees Distributor every thirty minutes, staggered by up
//! to ten minutes, and refreshes the engine when the file changes on disk. The
//! Distributor serves nothing without the licence keys set with
//! `.data_update_license_keys(..)`, so a build with polling on and no key
//! fails with an error that says so, as it does in .NET, and a deployment of
//! the free Lite file turns automatic updates off with `.auto_update(false)`,
//! which stops the polling. The file watcher runs on its own setting and
//! needs no key, so a new data file put in place still refreshes the engine,
//! and `.file_system_watcher(false)` turns it off.
//! `pipeline.resource::<DataUpdateService>()` reaches the service for a check
//! on demand.
//!
//! ## From a configuration file
//!
//! [`configuration::registry`] makes a pipeline from a configuration file
//! written for .NET's `FiftyOne.DeviceDetection` package, finding each
//! builder and setting by .NET's names, and adding the Sequence element and
//! usage sharing as .NET's `FiftyOnePipelineBuilder` does. See the
//! [`configuration`] module.
//!
//! ## Examples
//!
//! See [`DeviceDetectionPipelineBuilder::on_premise`] for an on-premise example
//! and [`DeviceDetectionPipelineBuilder::cloud`] for a cloud example. Each
//! example is gated to the feature that provides it.

#![warn(missing_docs)]

// ---------------------------------------------------------------------------
// Re-exports: the shared result model, present in every configuration.
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

#[allow(deprecated)]
pub use fiftyone_device_detection_shared::UachJsConversionElement;
pub use fiftyone_device_detection_shared::{
    DeviceData, DeviceDataBase, MultiDeviceData, DEVICE_DATA_KEY, DEVICE_ELEMENT_DATA_KEY,
    HARDWARE_DATA_KEY, HARDWARE_ELEMENT_DATA_KEY,
};

// The typed result wrapper a `DeviceData` accessor returns. Re-exported so a
// consumer can name it (for example to write a function over a property value)
// without depending on the engines crate directly.
pub use fiftyone_pipeline_engines::AspectPropertyValue;

// ---------------------------------------------------------------------------
// Re-exports: the core pipeline types a consumer needs to drive a pipeline.
// ---------------------------------------------------------------------------

pub use fiftyone_pipeline_core::{
    Error, Evidence, EvidenceBuilder, FlowData, FlowElement, Pipeline, PipelineBuilder, Result,
    TypedKey,
};

#[cfg(any(feature = "cloud", feature = "on-premise"))]
use fiftyone_pipeline_engines_fiftyone::{SequenceElement, ShareUsageElement};

#[cfg(feature = "on-premise")]
use fiftyone_pipeline_engines::{DataFileConfiguration, DataUpdateService};

// ---------------------------------------------------------------------------
// Re-exports: the on-premise engine and the native performance profile.
// ---------------------------------------------------------------------------

#[cfg(feature = "on-premise")]
pub use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};

/// The native performance profile used when loading an on-premise data file.
///
/// Re-exported from [`fiftyone_native`] so a consumer can pick a profile
/// without adding the native wrapper as a direct dependency.
#[cfg(feature = "on-premise")]
pub use fiftyone_native::PerformanceProfile;

// ---------------------------------------------------------------------------
// Re-exports: the cloud engine and the request engine it reads from.
// ---------------------------------------------------------------------------

#[cfg(feature = "cloud")]
pub use fiftyone_device_detection_cloud::{
    DeviceDetectionCloudEngine, DeviceDetectionCloudEngineBuilder, HardwareProfileCloudEngine,
    HardwareProfileCloudEngineBuilder,
};

#[cfg(feature = "cloud")]
pub use fiftyone_cloud_request_engine::{
    CloudEngineState, CloudRequestEngine, CloudRequestEngineBuilder,
};

pub mod configuration;

/// The examples in the README are compiled as documentation tests, so the
/// documented way to use this crate cannot quietly stop working.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeExamples;

// ---------------------------------------------------------------------------
// The convenience top-level pipeline builder.
// ---------------------------------------------------------------------------

/// The unified entry point for building a device-detection pipeline.
///
/// This is a tiny factory with two constructors, each handing back a
/// deployment-specific builder.
///
/// - [`DeviceDetectionPipelineBuilder::on_premise`] for the native Hash engine
///   (behind the `on-premise` feature).
/// - [`DeviceDetectionPipelineBuilder::cloud`] for the 51Degrees cloud (behind
///   the `cloud` feature).
///
/// Both sub-builders produce an `Arc<`[`Pipeline`]`>` with sensible defaults
/// and share usage on (see the crate-level note). Configure performance,
/// properties or share usage on the sub-builder before calling `build`.
pub struct DeviceDetectionPipelineBuilder;

impl DeviceDetectionPipelineBuilder {
    /// Start building an on-premise pipeline that loads the Hash data file at
    /// `data_file_path`.
    ///
    /// The returned [`DeviceDetectionOnPremisePipelineBuilder`] has share
    /// usage on.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use fiftyone_device_detection::{
    ///     DeviceData, DeviceDetectionPipelineBuilder, PerformanceProfile, DEVICE_DATA_KEY,
    /// };
    /// use fiftyone_pipeline_core::Evidence;
    ///
    /// # fn main() -> fiftyone_pipeline_core::Result<()> {
    /// let pipeline = DeviceDetectionPipelineBuilder::on_premise("51Degrees-LiteV4.1.hash")
    ///     .performance_profile(PerformanceProfile::HighPerformance)
    ///     // Off because the free Lite file has no licence key for updates.
    ///     .auto_update(false)
    ///     .build()?;
    ///
    /// let mut data = pipeline.create_flow_data_with(
    ///     Evidence::builder()
    ///         .add("header.user-agent", "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)")
    ///         .build(),
    /// );
    /// data.process()?;
    ///
    /// let device = data.get(DEVICE_DATA_KEY).expect("device data produced");
    /// println!("IsMobile = {:?}", device.is_mobile().value());
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "on-premise")]
    pub fn on_premise(
        data_file_path: impl Into<std::path::PathBuf>,
    ) -> DeviceDetectionOnPremisePipelineBuilder {
        DeviceDetectionOnPremisePipelineBuilder::new(data_file_path)
    }

    /// Start building a cloud pipeline against the 51Degrees cloud for the given
    /// `resource_key`.
    ///
    /// Obtain a resource key from
    /// <https://51degrees.com/configure?utm_source=code&utm_medium=comment&utm_campaign=51degrees-rust&utm_content=device-detection-src-lib.rs&utm_term=cloud>.
    /// The returned [`DeviceDetectionCloudPipelineBuilder`] assembles a
    /// [`CloudRequestEngine`] followed by a [`DeviceDetectionCloudEngine`], with
    /// share usage on.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use fiftyone_device_detection::{DeviceDetectionPipelineBuilder, DEVICE_DATA_KEY};
    /// use fiftyone_pipeline_core::Evidence;
    ///
    /// # fn main() -> fiftyone_pipeline_core::Result<()> {
    /// let pipeline = DeviceDetectionPipelineBuilder::cloud("my-resource-key").build()?;
    ///
    /// let mut data = pipeline.create_flow_data_with(
    ///     Evidence::builder().add("header.user-agent", "Mozilla/5.0").build(),
    /// );
    /// data.process()?;
    /// # let _ = DEVICE_DATA_KEY;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "cloud")]
    pub fn cloud(resource_key: impl Into<String>) -> DeviceDetectionCloudPipelineBuilder {
        DeviceDetectionCloudPipelineBuilder::new(resource_key)
    }
}

// ---------------------------------------------------------------------------
// On-premise pipeline builder.
// ---------------------------------------------------------------------------

/// Builds an `Arc<`[`Pipeline`]`>` around the on-premise Hash engine.
///
/// Defaults: the [`PerformanceProfile::Default`] profile, every property the
/// data file supports, no UA-CH high-entropy decoder, share usage enabled
/// with the `share-usage` feature, and automatic data updates on.
/// Override any of these before [`Self::build`].
#[cfg(feature = "on-premise")]
pub struct DeviceDetectionOnPremisePipelineBuilder {
    data_file_path: std::path::PathBuf,
    profile: PerformanceProfile,
    properties: Vec<String>,
    data_source_tier: Option<String>,
    concurrency: Option<u16>,
    difference: Option<i32>,
    allow_unmatched: Option<bool>,
    use_performance_graph: Option<bool>,
    use_predictive_graph: Option<bool>,
    use_uach: bool,
    share_usage: bool,
    auto_update: bool,
    file_system_watcher: bool,
    update_on_startup: bool,
    data_update_url: Option<String>,
    data_update_license_keys: Vec<String>,
    polling_interval_seconds: Option<u64>,
    max_randomisation_seconds: Option<u64>,
    data_update_service: Option<std::sync::Arc<DataUpdateService>>,
}

#[cfg(feature = "on-premise")]
impl DeviceDetectionOnPremisePipelineBuilder {
    /// Create a builder for the data file at `data_file_path`.
    pub fn new(data_file_path: impl Into<std::path::PathBuf>) -> Self {
        DeviceDetectionOnPremisePipelineBuilder {
            data_file_path: data_file_path.into(),
            profile: PerformanceProfile::Default,
            properties: Vec::new(),
            data_source_tier: None,
            concurrency: None,
            difference: None,
            allow_unmatched: None,
            use_performance_graph: None,
            use_predictive_graph: None,
            use_uach: false,
            share_usage: cfg!(feature = "share-usage"),
            auto_update: true,
            file_system_watcher: true,
            update_on_startup: false,
            data_update_url: None,
            data_update_license_keys: Vec::new(),
            polling_interval_seconds: None,
            max_randomisation_seconds: None,
            data_update_service: None,
        }
    }

    /// Enable or disable automatic data file updates. On by default, as in
    /// .NET and Java. When on, the pipeline owns a [`DataUpdateService`] that
    /// polls the update URL on the polling interval and refreshes the engine
    /// with each new file. The 51Degrees Distributor serves nothing without a
    /// licence key, so [`Self::build`] refuses with this on and no key set, as
    /// .NET does, and a deployment of the Lite file turns this off or sets
    /// [`Self::data_update_license_keys`]. This is the master switch for
    /// polling, and the file-system watcher runs on its own setting whether
    /// this is on or off.
    pub fn auto_update(mut self, enabled: bool) -> Self {
        self.auto_update = enabled;
        self
    }

    /// Enable or disable the file-system watcher, which refreshes the engine
    /// when the data file changes on disk. On by default, and it runs whether
    /// [`Self::auto_update`] is on or off and needs no licence key.
    pub fn file_system_watcher(mut self, enabled: bool) -> Self {
        self.file_system_watcher = enabled;
        self
    }

    /// Set whether the update service polls once while the pipeline is built,
    /// so the engine starts with the newest data. Off by default.
    pub fn update_on_startup(mut self, enabled: bool) -> Self {
        self.update_on_startup = enabled;
        self
    }

    /// Set the URL polled for a new data file. The 51Degrees Distributor by
    /// default.
    pub fn data_update_url(mut self, url: impl Into<String>) -> Self {
        self.data_update_url = Some(url.into());
        self
    }

    /// Set the licence keys the update URL carries, which the Distributor
    /// needs before it serves a file. One is required when automatic updates
    /// or update on startup are on.
    pub fn data_update_license_keys<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.data_update_license_keys = keys.into_iter().map(Into::into).collect();
        self
    }

    /// Set the interval between polls of the update URL, in seconds. Thirty
    /// minutes by default.
    pub fn polling_interval_seconds(mut self, seconds: u64) -> Self {
        self.polling_interval_seconds = Some(seconds);
        self
    }

    /// Set the most that is added at random to each polling interval, in
    /// seconds, so many instances do not poll together. Ten minutes by default.
    pub fn max_randomisation_seconds(mut self, seconds: u64) -> Self {
        self.max_randomisation_seconds = Some(seconds);
        self
    }

    /// Register the engine with `service` instead of a service this builder
    /// creates, so several pipelines share one. The pipeline keeps it alive
    /// as well.
    pub fn data_update_service(mut self, service: std::sync::Arc<DataUpdateService>) -> Self {
        self.data_update_service = Some(service);
        self
    }

    /// Set the performance profile used to load the data file. Defaults to
    /// [`PerformanceProfile::Default`].
    pub fn performance_profile(mut self, profile: PerformanceProfile) -> Self {
        self.profile = profile;
        self
    }

    /// Set the expected concurrency: the number of threads that will process
    /// through the built pipeline at once, as
    /// [`DeviceDetectionOnPremiseEngineBuilder::concurrency`] describes. Unset,
    /// it is the number of processors, held at the most the profile's caches
    /// take. Every profile but `InMemory` keeps that many handles to the data
    /// file open, `HighPerformance` included, although detection there never
    /// reads through them.
    pub fn concurrency(mut self, concurrency: u16) -> Self {
        self.concurrency = Some(concurrency);
        self
    }

    /// Set the most difference in hash value a match may have, as
    /// [`DeviceDetectionOnPremiseEngineBuilder::difference`] describes.
    pub fn difference(mut self, difference: i32) -> Self {
        self.difference = Some(difference);
        self
    }

    /// Set whether a User-Agent that matches nothing gives each component's
    /// default profile, as
    /// [`DeviceDetectionOnPremiseEngineBuilder::allow_unmatched`] describes.
    pub fn allow_unmatched(mut self, allow: bool) -> Self {
        self.allow_unmatched = Some(allow);
        self
    }

    /// Set whether detection tries the performance graph, as
    /// [`DeviceDetectionOnPremiseEngineBuilder::use_performance_graph`]
    /// describes.
    pub fn use_performance_graph(mut self, use_graph: bool) -> Self {
        self.use_performance_graph = Some(use_graph);
        self
    }

    /// Set whether detection tries the predictive graph, as
    /// [`DeviceDetectionOnPremiseEngineBuilder::use_predictive_graph`]
    /// describes.
    pub fn use_predictive_graph(mut self, use_graph: bool) -> Self {
        self.use_predictive_graph = Some(use_graph);
        self
    }

    /// Restrict the engine to a single named property, adding to any already
    /// requested. With no restriction every property the data file supports is
    /// loaded.
    ///
    /// On premise, a component with no named property reads as `0` in the device
    /// id. Naming `DeviceId` resolves every component, so the device id is
    /// complete.
    pub fn property(mut self, name: impl Into<String>) -> Self {
        let name = name.into();
        if !self
            .properties
            .iter()
            .any(|p| p.eq_ignore_ascii_case(&name))
        {
            self.properties.push(name);
        }
        self
    }

    /// Restrict the engine to the named properties, adding to any already
    /// requested.
    pub fn properties<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for name in names {
            self = self.property(name);
        }
        self
    }

    /// Override the data-source tier the engine reports, for example `Premium`.
    /// Defaults to the engine's own default (`Lite`, the free tier).
    pub fn data_source_tier(mut self, tier: impl Into<String>) -> Self {
        self.data_source_tier = Some(tier.into());
        self
    }

    /// Place the deprecated UA-CH high-entropy decoder before the Hash engine
    /// when `enabled` is true, for a caller that reads its `uach` data, which
    /// the JSON builder then writes too. Off by default. The Hash engine does
    /// not read that data, because the C library turns
    /// `51D_GetHighEntropyValues` evidence into the `sec-ch-ua*` headers
    /// itself, so the device answers are the same either way.
    #[deprecated(
        note = "the Hash engine reads high entropy values itself, so the decoder is only for a caller that reads its data"
    )]
    pub fn use_uach(mut self, enabled: bool) -> Self {
        self.use_uach = enabled;
        self
    }

    /// Toggle usage sharing. On by default with the `share-usage` feature,
    /// which is a default feature, so evidence from the requests the pipeline
    /// processes reaches 51Degrees and improves detection. A console or
    /// off-line tool turns it off. See the crate-level note.
    ///
    /// When enabled, a [`SequenceElement`] is added first, so each shared
    /// record carries its session id and sequence, and a [`ShareUsageElement`]
    /// with default settings is added last. The element only transmits if the
    /// `fiftyone-pipeline-engines-fiftyone/share-usage-transport` feature is
    /// enabled, which the `share-usage` feature does.
    pub fn share_usage(mut self, enabled: bool) -> Self {
        self.share_usage = enabled;
        self
    }

    /// Assemble the pipeline.
    ///
    /// The element order is: the sequence element when usage sharing is on, the
    /// UA-CH decoder when [`Self::use_uach`] asked for it, the Hash engine,
    /// then usage sharing when it is on. Returns the same kind of
    /// `Arc<`[`Pipeline`]`>` that [`Pipeline::builder`] does, so a caller
    /// drives it identically whichever deployment built it. Refuses when
    /// automatic updates or update on startup are on and no licence key is
    /// set.
    pub fn build(self) -> Result<std::sync::Arc<Pipeline>> {
        use std::sync::Arc;

        // The service that keeps the data file current: the one the caller
        // shares, or one of this pipeline's own when any automatic mode is on.
        let updates_wanted = self.auto_update || self.file_system_watcher || self.update_on_startup;
        let service = match (self.data_update_service, updates_wanted) {
            (Some(service), _) => Some(service),
            (None, true) => Some(Arc::new(DataUpdateService::new())),
            (None, false) => None,
        };

        let mut engine_builder =
            DeviceDetectionOnPremiseEngineBuilder::new(self.data_file_path.clone())
                .performance_profile(self.profile);
        engine_builder = engine_builder.properties(self.properties);
        if let Some(tier) = self.data_source_tier {
            engine_builder = engine_builder.data_source_tier(tier);
        }
        if let Some(concurrency) = self.concurrency {
            engine_builder = engine_builder.concurrency(concurrency);
        }
        if let Some(difference) = self.difference {
            engine_builder = engine_builder.difference(difference);
        }
        if let Some(allow) = self.allow_unmatched {
            engine_builder = engine_builder.allow_unmatched(allow);
        }
        if let Some(use_graph) = self.use_performance_graph {
            engine_builder = engine_builder.use_performance_graph(use_graph);
        }
        if let Some(use_graph) = self.use_predictive_graph {
            engine_builder = engine_builder.use_predictive_graph(use_graph);
        }
        if let Some(service) = &service {
            let mut config = DataFileConfiguration::builder(self.data_file_path)
                .automatic_updates_enabled(self.auto_update)
                .file_system_watcher_enabled(self.file_system_watcher)
                .update_on_startup(self.update_on_startup)
                .data_update_license_keys(self.data_update_license_keys);
            if let Some(url) = self.data_update_url {
                config = config.data_update_url(url);
            }
            if let Some(seconds) = self.polling_interval_seconds {
                config = config.polling_interval_seconds(seconds);
            }
            if let Some(seconds) = self.max_randomisation_seconds {
                config = config.max_randomisation_seconds(seconds);
            }
            engine_builder = engine_builder.auto_update(config.build(), Some(Arc::clone(service)));
        }
        let engine = engine_builder.build()?;

        let mut pipeline_builder = Pipeline::builder();
        if let Some(service) = service {
            pipeline_builder = pipeline_builder.add_resource(service);
        }
        if self.share_usage {
            pipeline_builder = pipeline_builder.add_element(Arc::new(SequenceElement::new()));
        }
        if self.use_uach {
            #[allow(deprecated)]
            let decoder = Arc::new(UachJsConversionElement::new());
            pipeline_builder = pipeline_builder.add_element(decoder);
        }
        pipeline_builder = pipeline_builder.add_element(engine);
        if self.share_usage {
            pipeline_builder =
                pipeline_builder.add_element(Arc::new(ShareUsageElement::with_defaults()));
        }
        pipeline_builder.build()
    }
}

// ---------------------------------------------------------------------------
// Cloud pipeline builder.
// ---------------------------------------------------------------------------

/// Builds an `Arc<`[`Pipeline`]`>` around the cloud device-detection engine.
///
/// The pipeline is a [`CloudRequestEngine`] (which makes the HTTP call to the
/// 51Degrees cloud) followed by a [`DeviceDetectionCloudEngine`] (which turns
/// the response into a [`DeviceDataBase`]). Share usage is enabled by default
/// with the `share-usage` feature.
#[cfg(feature = "cloud")]
pub struct DeviceDetectionCloudPipelineBuilder {
    resource_key: String,
    endpoint: Option<String>,
    share_usage: bool,
    hardware_profile: bool,
    cloud_state: Option<CloudEngineState>,
}

#[cfg(feature = "cloud")]
impl DeviceDetectionCloudPipelineBuilder {
    /// Create a builder for the given resource key.
    pub fn new(resource_key: impl Into<String>) -> Self {
        DeviceDetectionCloudPipelineBuilder {
            resource_key: resource_key.into(),
            endpoint: None,
            share_usage: cfg!(feature = "share-usage"),
            hardware_profile: false,
            cloud_state: None,
        }
    }

    /// Override the cloud endpoint URL. By default the request engine's own
    /// default endpoint (the public 51Degrees cloud) is used.
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Toggle usage sharing. On by default with the `share-usage` feature,
    /// which is a default feature. When enabled, a sequence element is added
    /// first and a usage-sharing element last, as for the on-premise builder.
    /// Transmission needs the
    /// `fiftyone-pipeline-engines-fiftyone/share-usage-transport` feature, which
    /// the `share-usage` feature enables. See the crate-level note.
    pub fn share_usage(mut self, enabled: bool) -> Self {
        self.share_usage = enabled;
        self
    }

    /// Build a hardware-profile lookup pipeline instead of a single-device one.
    ///
    /// In this mode the pipeline carries a [`HardwareProfileCloudEngine`] in place
    /// of the single-device [`DeviceDetectionCloudEngine`], so a lookup by TAC
    /// (`query.tac`) or native model name (`query.nativemodel`) returns every
    /// matching device profile under [`HARDWARE_DATA_KEY`]. Requires a resource
    /// key with the hardware-profile-lookup product (a paid subscription).
    pub fn hardware_profile(mut self) -> Self {
        self.hardware_profile = true;
        self
    }

    /// Supply a previously exported [`CloudEngineState`], so the request engine
    /// uses it instead of fetching the accepted evidence keys and accessible
    /// properties from the cloud as it builds.
    ///
    /// This lets the pipeline be constructed offline, and avoids the build-time
    /// cloud round-trip on a short-lived or frequently-restarted host. Obtain
    /// the state from a built engine with
    /// [`CloudRequestEngineBuilder::export_state`].
    pub fn set_state(mut self, state: CloudEngineState) -> Self {
        self.cloud_state = Some(state);
        self
    }

    /// Supply a [`CloudEngineState`] when one is available, otherwise let the
    /// request engine fetch it from the cloud at build time.
    pub fn set_state_opt(mut self, state: Option<CloudEngineState>) -> Self {
        self.cloud_state = state;
        self
    }

    /// Assemble the pipeline.
    ///
    /// Unless a [`CloudEngineState`] was supplied with
    /// [`set_state`](Self::set_state), the request engine fetches its accepted
    /// evidence keys and accessible properties from the cloud as it builds, so
    /// this makes a network call and returns an error if the cloud is
    /// unavailable. Supply a cached state to construct the pipeline offline.
    /// Element order is: the sequence element when usage sharing is on, the
    /// cloud request engine, the cloud device engine (or, in
    /// [`hardware_profile`](Self::hardware_profile) mode, the hardware-profile
    /// engine), then usage sharing when it is on.
    pub fn build(self) -> Result<std::sync::Arc<Pipeline>> {
        use std::sync::Arc;

        let mut request_builder = CloudRequestEngine::builder()
            .resource_key(self.resource_key)
            .set_state_opt(self.cloud_state);
        if let Some(endpoint) = self.endpoint {
            request_builder = request_builder.endpoint(endpoint);
        }
        let request_engine = Arc::new(request_builder.build()?);

        let mut pipeline_builder = Pipeline::builder();
        if self.share_usage {
            pipeline_builder = pipeline_builder.add_element(Arc::new(SequenceElement::new()));
        }
        pipeline_builder = pipeline_builder.add_element(request_engine.clone());

        // Select the result engine: a multi-profile hardware lookup or the
        // standard single-device detection. Both read the request engine's JSON.
        if self.hardware_profile {
            let hardware_engine = HardwareProfileCloudEngine::builder()
                .cloud_request_engine(request_engine)
                .try_build()?;
            pipeline_builder = pipeline_builder.add_element(Arc::new(hardware_engine));
        } else {
            let device_engine = DeviceDetectionCloudEngine::builder()
                .cloud_request_engine(request_engine)
                .try_build()?;
            pipeline_builder = pipeline_builder.add_element(Arc::new(device_engine));
        }
        if self.share_usage {
            pipeline_builder =
                pipeline_builder.add_element(Arc::new(ShareUsageElement::with_defaults()));
        }
        pipeline_builder.build()
    }
}

#[cfg(test)]
mod tests {
    // Gated on `reqwest-client`, not just `cloud`: constructing the cloud
    // request engine requires an HTTP transport even when an injected state
    // means no network call is made, and the built-in transport is the
    // reqwest client this feature enables. Without it the build deliberately
    // errors (see the CloudRequestEngine no-transport check), so this success
    // case only applies when the built-in client is available. Run it with
    // `--features reqwest-client` or `--all-features`.
    #[cfg(feature = "reqwest-client")]
    #[test]
    fn cloud_pipeline_builds_offline_with_injected_state() {
        // The request engine fetches its discovery state from the cloud as it
        // builds, so a cloud pipeline can only be constructed offline when a
        // previously exported state is supplied. With one injected, construction
        // makes no network call.
        let pipeline = super::DeviceDetectionPipelineBuilder::cloud("resource-key-placeholder")
            .set_state(super::CloudEngineState::default())
            .build()
            .expect("a cloud pipeline should construct offline from an injected state");

        // Confirm we got a usable pipeline back and can create flow data from it.
        let _data = pipeline.create_flow_data();
    }

    #[cfg(all(feature = "reqwest-client", feature = "share-usage"))]
    #[test]
    fn usage_sharing_is_on_by_default() {
        // Built but never processed, so nothing is queued or sent.
        let pipeline = super::DeviceDetectionPipelineBuilder::cloud("resource-key-placeholder")
            .set_state(super::CloudEngineState::default())
            .build()
            .expect("a cloud pipeline should construct offline from an injected state");
        assert_eq!(
            pipeline
                .flow_elements()
                .last()
                .map(|element| element.type_name()),
            Some("fiftyone_pipeline_engines_fiftyone::share_usage_element::ShareUsageElement")
        );
    }

    #[cfg(feature = "reqwest-client")]
    #[test]
    fn usage_sharing_puts_the_sequence_element_first_and_itself_last() {
        // Built but never processed, so nothing is queued or sent.
        let pipeline = super::DeviceDetectionPipelineBuilder::cloud("resource-key-placeholder")
            .set_state(super::CloudEngineState::default())
            .share_usage(true)
            .build()
            .expect("a cloud pipeline should construct offline from an injected state");

        let names: Vec<&str> = pipeline
            .flow_elements()
            .iter()
            .map(|element| element.type_name())
            .collect();
        assert_eq!(names.len(), 4, "{names:?}");
        assert_eq!(
            names.first().copied(),
            Some("fiftyone_pipeline_engines_fiftyone::sequence_element::SequenceElement")
        );
        assert_eq!(
            names.last().copied(),
            Some("fiftyone_pipeline_engines_fiftyone::share_usage_element::ShareUsageElement")
        );
    }

    /// Data updates are on by default, so a build with no licence key refuses
    /// and says which switch turns them off. With a key the pipeline owns a
    /// data update service with the engine's data file registered. With every
    /// automatic mode off, no service is created.
    #[cfg(feature = "on-premise")]
    #[test]
    fn on_premise_pipeline_owns_a_data_update_service_by_default() {
        use super::DeviceDetectionPipelineBuilder;
        use fiftyone_pipeline_engines::DataUpdateService;

        let Some(data_file) = resolve_dd_data_file() else {
            eprintln!(
                "skipping on_premise_pipeline_owns_a_data_update_service_by_default: no Lite \
                 Hash data file found (set _51DEGREES_DD_PATH to enable this test)"
            );
            return;
        };

        let error = DeviceDetectionPipelineBuilder::on_premise(&data_file)
            .share_usage(false)
            .build()
            .err()
            .expect("the default build needs a licence key");
        assert!(error.to_string().contains("auto_update(false)"), "{error}");

        let pipeline = DeviceDetectionPipelineBuilder::on_premise(&data_file)
            .share_usage(false)
            .data_update_license_keys(["KEY1"])
            .build()
            .expect("the on-premise pipeline should build with a licence key");
        let service = pipeline
            .resource::<DataUpdateService>()
            .expect("the pipeline owns a data update service");
        assert_eq!(service.registered_count(), 1);

        let quiet = DeviceDetectionPipelineBuilder::on_premise(&data_file)
            .share_usage(false)
            .auto_update(false)
            .file_system_watcher(false)
            .build()
            .expect("the quiet pipeline should build");
        assert!(quiet.resource::<DataUpdateService>().is_none());
    }

    #[cfg(feature = "on-premise")]
    #[test]
    fn on_premise_detects_mobile_user_agent() {
        use super::{
            DeviceData, DeviceDetectionPipelineBuilder, PerformanceProfile, DEVICE_DATA_KEY,
        };
        use fiftyone_pipeline_core::Evidence;

        // Resolve the Lite Hash data file. Honour an explicit override first,
        // then fall back to the location it sits at in the
        // cxx submodule at the root of the workspace. Skip (not fail)
        // only when the file is genuinely absent, so CI without the data file is
        // green while a developer with it gets real coverage.
        let Some(data_file) = resolve_dd_data_file() else {
            eprintln!(
                "skipping on_premise_detects_mobile_user_agent: no Lite Hash data file found \
                 (set _51DEGREES_DD_PATH to enable this test)"
            );
            return;
        };

        // Usage sharing is off so the test sends nothing to 51Degrees, and
        // automatic updates are off so the build needs no licence key.
        let pipeline = DeviceDetectionPipelineBuilder::on_premise(&data_file)
            .performance_profile(PerformanceProfile::HighPerformance)
            .share_usage(false)
            .auto_update(false)
            .build()
            .expect("the on-premise pipeline should build against the Lite data file");

        // A modern iPhone Safari user agent is unambiguously a mobile device.
        let user_agent = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
                          AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 \
                          Mobile/15E148 Safari/604.1";
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("header.user-agent", user_agent)
                .build(),
        );
        data.process().expect("processing should succeed");

        let device = data
            .get(DEVICE_DATA_KEY)
            .expect("the Hash engine should have produced device data");
        let is_mobile = device.is_mobile();
        let is_mobile = is_mobile
            .value()
            .expect("IsMobile should have a value for an iPhone user agent");
        assert!(
            *is_mobile,
            "an iPhone user agent should be detected as mobile"
        );
    }

    /// Find a Lite Hash data file to run the on-premise test against, or `None`
    /// when none is present so the test can skip rather than fail.
    #[cfg(feature = "on-premise")]
    fn resolve_dd_data_file() -> Option<std::path::PathBuf> {
        use std::path::PathBuf;

        // 1. Explicit override.
        if let Ok(path) = crate::deprecated_names::var("_51DEGREES_DD_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(path);
            }
        }

        // 2. Walk up from the crate directory looking for the data file in a
        //    cxx submodule. The crate lives at
        //    <workspace>/device-detection, and the data file at
        //    <workspace>/cxx/data/device-detection/...
        let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        loop {
            let candidate = dir
                .join("cxx")
                .join("data")
                .join("device-detection")
                .join("51Degrees-LiteV4.1.hash");
            if candidate.is_file() {
                return Some(candidate);
            }
            if !dir.pop() {
                break;
            }
        }
        None
    }
}
