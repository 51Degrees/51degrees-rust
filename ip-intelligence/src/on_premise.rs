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

//! The on-premise IP-intelligence pipeline builder.
//!
//! It opens a local `.ipi` data file and assembles a pipeline around an
//! [`IpIntelligenceOnPremiseEngine`], which looks IP Intelligence up from the
//! client IP carried in a request's evidence.

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_ip_intelligence_onpremise::IpIntelligenceOnPremiseEngine;
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Pipeline, Result};
use fiftyone_pipeline_engines::DataUpdateService;
use fiftyone_pipeline_engines_fiftyone::{SequenceElement, ShareUsageElement};

/// A fluent builder that opens an `.ipi` data file and assembles an on-premise
/// IP-intelligence [`Pipeline`](fiftyone_pipeline_core::Pipeline).
///
/// Create one with [`IpIntelligencePipelineBuilder::on_premise`](crate::IpIntelligencePipelineBuilder::on_premise),
/// chain the configuration methods (each delegates to the underlying
/// [`IpIntelligenceOnPremiseEngineBuilder`](fiftyone_ip_intelligence_onpremise::IpIntelligenceOnPremiseEngineBuilder)),
/// then call [`build`](OnPremiseIpIntelligencePipelineBuilder::build).
///
/// The built pipeline contains the on-premise engine, with a sequence element
/// before it and a usage-sharing element after it when usage sharing is on.
/// Unlike the cloud deployment there is no request engine, because the lookup
/// is local.
pub struct OnPremiseIpIntelligencePipelineBuilder {
    data_file_path: PathBuf,
    profile: PerformanceProfile,
    requested_properties: Vec<String>,
    data_source_tier: Option<String>,
    auto_update: bool,
    file_system_watcher: bool,
    update_on_startup: bool,
    data_update_url: Option<String>,
    data_update_license_keys: Vec<String>,
    polling_interval_seconds: Option<u64>,
    max_randomisation_seconds: Option<u64>,
    data_update_service: Option<Arc<DataUpdateService>>,
    suppress_process_exceptions: bool,
    share_usage: bool,
    concurrency: Option<u16>,
}

impl OnPremiseIpIntelligencePipelineBuilder {
    /// Start an on-premise builder for the `.ipi` data file at `data_file_path`.
    /// Called by [`IpIntelligencePipelineBuilder::on_premise`](crate::IpIntelligencePipelineBuilder::on_premise).
    pub(crate) fn new(data_file_path: impl Into<PathBuf>) -> Self {
        OnPremiseIpIntelligencePipelineBuilder {
            data_file_path: data_file_path.into(),
            profile: PerformanceProfile::Default,
            requested_properties: Vec::new(),
            data_source_tier: None,
            auto_update: true,
            file_system_watcher: true,
            update_on_startup: false,
            data_update_url: None,
            data_update_license_keys: Vec::new(),
            polling_interval_seconds: None,
            max_randomisation_seconds: None,
            data_update_service: None,
            suppress_process_exceptions: false,
            share_usage: cfg!(feature = "share-usage"),
            concurrency: None,
        }
    }

    /// Set the performance profile the data file is opened with. Mirrors the
    /// engine builder's `performance_profile`.
    pub fn performance_profile(mut self, profile: PerformanceProfile) -> Self {
        self.profile = profile;
        self
    }

    /// Set the expected concurrency, which is the number of threads that will
    /// look up through the pipeline at once. Mirrors the engine builder's
    /// `concurrency`, which says what it sizes under each profile. Unset, the
    /// profile's own expected concurrency is kept.
    pub fn concurrency(mut self, concurrency: u16) -> Self {
        self.concurrency = Some(concurrency);
        self
    }

    /// Restrict the engine to a single named property, adding to any already
    /// requested. With no properties requested the engine populates every
    /// property the data file defines. A name is matched ignoring case, and
    /// one the data file lacks is neither populated nor listed.
    pub fn property(mut self, property: impl Into<String>) -> Self {
        self.requested_properties.push(property.into());
        self
    }

    /// Restrict the engine to the named properties, adding to any already
    /// requested.
    pub fn properties<I, S>(mut self, properties: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.requested_properties
            .extend(properties.into_iter().map(Into::into));
        self
    }

    /// Override the data-source tier the engine reports. When unset the engine
    /// uses its own default tier.
    pub fn data_source_tier(mut self, tier: impl Into<String>) -> Self {
        self.data_source_tier = Some(tier.into());
        self
    }

    /// Enable or disable automatic data file updates. On by default, as in
    /// .NET and Java. When on, the pipeline owns a [`DataUpdateService`] that
    /// polls the update URL on the polling interval and refreshes the engine
    /// with each new file. The 51Degrees Distributor serves nothing without a
    /// licence key, so [`Self::build`] refuses with this on and no key set, as
    /// .NET does, and a deployment without one turns this off or sets
    /// [`Self::data_update_license_keys`]. This is the master switch for
    /// automatic updates, so with it off the file-system watcher does not run
    /// either.
    pub fn auto_update(mut self, enabled: bool) -> Self {
        self.auto_update = enabled;
        self
    }

    /// Enable or disable the file-system watcher, which refreshes the engine
    /// when the data file changes on disk. On by default, and it runs only
    /// while [`Self::auto_update`] is on.
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
    pub fn data_update_service(mut self, service: Arc<DataUpdateService>) -> Self {
        self.data_update_service = Some(service);
        self
    }

    /// Set whether the pipeline suppresses processing exceptions, recording them
    /// on the flow data instead of returning them. Defaults to `false`, matching
    /// the pipeline default.
    pub fn suppress_process_exceptions(mut self, suppress: bool) -> Self {
        self.suppress_process_exceptions = suppress;
        self
    }

    /// Toggle usage sharing. On by default with the `share-usage` feature,
    /// which is a default feature, so evidence from the requests the pipeline
    /// processes reaches 51Degrees. A console or off-line tool turns it off.
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

    /// Open the data file, build the engine and assemble the pipeline.
    ///
    /// Returns the native error if the data file fails to load. Refuses when
    /// automatic updates or update on startup are on and no licence key is
    /// set.
    pub fn build(self) -> Result<Arc<Pipeline>> {
        // The service that keeps the data file current: the one the caller
        // shares, or one of this pipeline's own when any automatic mode is on.
        let updates_wanted = self.auto_update || self.file_system_watcher || self.update_on_startup;
        let service = match (self.data_update_service, updates_wanted) {
            (Some(service), _) => Some(service),
            (None, true) => Some(Arc::new(DataUpdateService::new())),
            (None, false) => None,
        };

        let mut engine_builder = IpIntelligenceOnPremiseEngine::builder(self.data_file_path)
            .performance_profile(self.profile)
            .properties(self.requested_properties)
            .auto_update(self.auto_update)
            .file_system_watcher(self.file_system_watcher)
            .update_on_startup(self.update_on_startup)
            .data_update_license_keys(self.data_update_license_keys);
        if let Some(tier) = self.data_source_tier {
            engine_builder = engine_builder.data_source_tier(tier);
        }
        if let Some(concurrency) = self.concurrency {
            engine_builder = engine_builder.concurrency(concurrency);
        }
        if let Some(url) = self.data_update_url {
            engine_builder = engine_builder.data_update_url(url);
        }
        if let Some(seconds) = self.polling_interval_seconds {
            engine_builder = engine_builder.polling_interval_seconds(seconds);
        }
        if let Some(seconds) = self.max_randomisation_seconds {
            engine_builder = engine_builder.max_randomisation_seconds(seconds);
        }
        if let Some(service) = &service {
            engine_builder = engine_builder.data_update_service(Arc::clone(service));
        }

        let engine = engine_builder.build_shared()?;

        let mut pipeline_builder = Pipeline::builder();
        if let Some(service) = service {
            pipeline_builder = pipeline_builder.add_resource(service);
        }
        if self.share_usage {
            pipeline_builder = pipeline_builder.add_element(Arc::new(SequenceElement::new()));
        }
        pipeline_builder = pipeline_builder.add_element(engine);
        if self.share_usage {
            pipeline_builder =
                pipeline_builder.add_element(Arc::new(ShareUsageElement::with_defaults()));
        }
        pipeline_builder
            .suppress_process_exceptions(self.suppress_process_exceptions)
            .build()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fiftyone_ip_intelligence_shared::IP_DATA_KEY;
    use fiftyone_native::PerformanceProfile;
    use fiftyone_pipeline_core::Evidence;
    use fiftyone_pipeline_engines::AspectData;

    use crate::IpIntelligencePipelineBuilder;

    /// The autonomous-system properties an ASN data file carries. The on-premise
    /// real-lookup test requests these so the engine populates them.
    const ASN_PROPERTIES: &[&str] = &["Asn", "AsnName"];

    /// Resolve the ASN IP-intelligence data file at run time.
    ///
    /// The default test/CI data file is the ASN file from the data repository
    /// (small, current and loads reliably). Set `_51DEGREES_IPI_PATH`
    /// to override the path. Returns [`None`] when no data file is present, so
    /// the test skips cleanly on a machine without it.
    fn asn_data_file() -> Option<PathBuf> {
        if let Ok(path) = crate::deprecated_names::var("_51DEGREES_IPI_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Some(path);
            }
        }
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .to_path_buf();
        let name = "51Degrees-IPIV4AsnIpiV41.ipi";
        // The data repository is in the `cxx` submodule, or checked out in the
        // wider Workspace tree beside the Rust workspace.
        let candidates = [
            workspace
                .join("cxx")
                .join("data")
                .join("ip-intelligence")
                .join(name),
            workspace
                .parent()
                .map(|p| p.join("data").join("ip-intelligence").join(name))
                .unwrap_or_default(),
        ];
        candidates.into_iter().find(|p| p.is_file())
    }

    /// Data updates are on by default, so a build with no licence key refuses
    /// and says which switch turns them off. With a key the pipeline owns a
    /// data update service with the engine's data file registered. With every
    /// automatic mode off, no service is created.
    #[test]
    fn on_premise_pipeline_owns_a_data_update_service_by_default() {
        use fiftyone_pipeline_engines::DataUpdateService;

        let Some(path) = asn_data_file() else {
            eprintln!("no ASN data file found; skipping the data update service test");
            return;
        };

        let error = IpIntelligencePipelineBuilder::on_premise(&path)
            .properties(ASN_PROPERTIES.iter().copied())
            .share_usage(false)
            .build()
            .err()
            .expect("the default build needs a licence key");
        assert!(error.to_string().contains("auto_update(false)"), "{error}");

        let pipeline = IpIntelligencePipelineBuilder::on_premise(&path)
            .properties(ASN_PROPERTIES.iter().copied())
            .share_usage(false)
            .data_update_license_keys(["KEY1"])
            .build()
            .expect("the on-premise pipeline should build with a licence key");
        let service = pipeline
            .resource::<DataUpdateService>()
            .expect("the pipeline owns a data update service");
        assert_eq!(service.registered_count(), 1);

        let quiet = IpIntelligencePipelineBuilder::on_premise(&path)
            .properties(ASN_PROPERTIES.iter().copied())
            .share_usage(false)
            .auto_update(false)
            .file_system_watcher(false)
            .build()
            .expect("the quiet pipeline should build");
        assert!(quiet.resource::<DataUpdateService>().is_none());
    }

    /// The pipeline opens with more expected concurrency than the square
    /// root of the Balanced profiles cache of 20,000 entries, 141. The
    /// engine's own tests show the value reaching the data set.
    #[test]
    fn on_premise_builder_opens_with_any_concurrency() {
        let Some(data_file) = asn_data_file() else {
            eprintln!("no usable IP-intelligence data file; skipping concurrency");
            return;
        };
        let pipeline = IpIntelligencePipelineBuilder::on_premise(&data_file)
            .performance_profile(PerformanceProfile::Balanced)
            .concurrency(500)
            // Off so the test sends nothing to 51Degrees.
            .share_usage(false)
            // Off so the build needs no licence key.
            .auto_update(false)
            .file_system_watcher(false)
            .build();
        assert!(
            pipeline.is_ok(),
            "more than the profiles cache's square root opens"
        );
    }

    #[test]
    fn on_premise_builder_assembles_a_single_engine_pipeline() {
        let Some(data_file) = asn_data_file() else {
            eprintln!("no usable IP-intelligence data file; skipping on-premise lookup");
            return;
        };

        // The facade builds the engine and the pipeline. Once the file is found
        // the build must succeed, so a failure is a hard error rather than a skip.
        let pipeline = IpIntelligencePipelineBuilder::on_premise(&data_file)
            .performance_profile(PerformanceProfile::HighPerformance)
            .properties(ASN_PROPERTIES.iter().copied())
            // Off so the test sends nothing to 51Degrees.
            .share_usage(false)
            // Off so the build needs no licence key.
            .auto_update(false)
            .build()
            .expect("the ASN data file should build an on-premise pipeline");

        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                // A Cloudflare public IPv4, mapped to autonomous system AS13335.
                .add("server.client-ip", "1.1.1.1")
                .build(),
        );
        data.process().expect("processing should not error");

        let ip = data.get(IP_DATA_KEY).expect("ip data should be present");
        assert_eq!(ip.engine_keys(), ["ip"]);

        // The ASN data file maps the IP to its autonomous system, a plain string
        // property, which must read back through the shared model.
        let asn = ip.string("Asn");
        let value = asn.value().expect("Asn should resolve to a value");
        eprintln!("Asn = {value}");
        assert!(
            value.contains("AS13335"),
            "the Cloudflare IPv4 should resolve to AS13335, got {value}"
        );
    }
}
