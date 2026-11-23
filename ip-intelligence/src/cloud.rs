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

//! The cloud IP-intelligence pipeline builder.
//!
//! It assembles the two-element cloud pipeline, a [`CloudRequestEngine`] that
//! makes the single HTTP call to the 51Degrees cloud service, followed by an
//! [`IpIntelligenceCloudEngine`] that deserializes the `ip` member of the
//! response into an
//! [`IpIntelligenceDataBase`](fiftyone_ip_intelligence_shared::IpIntelligenceDataBase).

use std::sync::Arc;
use std::time::Duration;

use fiftyone_cloud_request_engine::{CloudEngineState, CloudRequestEngine};
use fiftyone_ip_intelligence_cloud::IpIntelligenceCloudEngine;
use fiftyone_pipeline_core::{Pipeline, Result};
use fiftyone_pipeline_engines_fiftyone::{SequenceElement, ShareUsageElement};

/// A fluent builder that assembles a cloud IP-intelligence
/// [`Pipeline`](fiftyone_pipeline_core::Pipeline).
///
/// Create one with [`IpIntelligencePipelineBuilder::cloud`](crate::IpIntelligencePipelineBuilder::cloud),
/// optionally tune the cloud request (endpoint, origin, timeout), then call
/// [`build`](CloudIpIntelligencePipelineBuilder::build).
///
/// The built pipeline runs `[CloudRequestEngine, IpIntelligenceCloudEngine]`,
/// with a sequence element before them and a usage-sharing element after them
/// when usage sharing is on. The cloud request engine makes one HTTP call and
/// stores the raw JSON, and the IP-intelligence engine reads the `ip` part of
/// that response.
pub struct CloudIpIntelligencePipelineBuilder {
    resource_key: String,
    endpoint: Option<String>,
    cloud_request_origin: Option<String>,
    timeout: Option<Duration>,
    suppress_process_exceptions: bool,
    cloud_state: Option<CloudEngineState>,
    share_usage: bool,
}

impl CloudIpIntelligencePipelineBuilder {
    /// Start a cloud builder for the given resource key. Called by
    /// [`IpIntelligencePipelineBuilder::cloud`](crate::IpIntelligencePipelineBuilder::cloud).
    pub(crate) fn new(resource_key: impl Into<String>) -> Self {
        CloudIpIntelligencePipelineBuilder {
            resource_key: resource_key.into(),
            endpoint: None,
            cloud_request_origin: None,
            timeout: None,
            suppress_process_exceptions: false,
            cloud_state: None,
            share_usage: cfg!(feature = "share-usage"),
        }
    }

    /// Toggle usage sharing. On by default with the `share-usage` feature,
    /// which is a default feature. When enabled, a [`SequenceElement`] is
    /// added first and a [`ShareUsageElement`] with default settings last, as
    /// for the on-premise builder. The element only transmits if the
    /// `fiftyone-pipeline-engines-fiftyone/share-usage-transport` feature is
    /// enabled, which the `share-usage` feature does.
    pub fn share_usage(mut self, enabled: bool) -> Self {
        self.share_usage = enabled;
        self
    }

    /// Override the base cloud endpoint. When unset the cloud request engine
    /// uses the 51Degrees public cloud endpoint.
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Set the value of the `Origin` header sent with each cloud request. The
    /// cloud service checks this against the origins the resource key permits.
    pub fn cloud_request_origin(mut self, origin: impl Into<String>) -> Self {
        self.cloud_request_origin = Some(origin.into());
        self
    }

    /// Set the cloud request timeout. Defaults to the cloud request engine's own
    /// default (two seconds) when unset.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Set whether the pipeline suppresses processing exceptions, recording them
    /// on the flow data instead of returning them. Defaults to `false`, matching
    /// the pipeline default.
    pub fn suppress_process_exceptions(mut self, suppress: bool) -> Self {
        self.suppress_process_exceptions = suppress;
        self
    }

    /// Supply a previously exported [`CloudEngineState`], so the request engine
    /// uses it instead of fetching the accepted evidence keys and accessible
    /// properties from the cloud as it builds.
    ///
    /// This lets the pipeline be constructed offline, and avoids the build-time
    /// cloud round-trip on a short-lived or frequently-restarted host. Obtain the
    /// state from a built engine with
    /// [`CloudRequestEngineBuilder::export_state`](fiftyone_cloud_request_engine::CloudRequestEngineBuilder::export_state).
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

    /// Build the cloud request engine, the IP-intelligence cloud engine, and the
    /// pipeline that runs them in order.
    ///
    /// Unless a [`CloudEngineState`] was supplied with
    /// [`set_state`](Self::set_state), the request engine fetches its accepted
    /// evidence keys and accessible properties from the cloud as it builds, so
    /// this makes a network call and returns an error if the cloud is
    /// unavailable. Returns an error if the resource key is empty or the cloud
    /// request engine cannot be constructed (for example its HTTP client fails to
    /// build).
    pub fn build(self) -> Result<Arc<Pipeline>> {
        let mut request_builder = CloudRequestEngine::builder()
            .resource_key(self.resource_key)
            .set_state_opt(self.cloud_state);
        if let Some(endpoint) = self.endpoint {
            request_builder = request_builder.endpoint(endpoint);
        }
        if let Some(origin) = self.cloud_request_origin {
            request_builder = request_builder.cloud_request_origin(origin);
        }
        if let Some(timeout) = self.timeout {
            request_builder = request_builder.timeout(timeout);
        }

        let request_engine = Arc::new(request_builder.build()?);
        let ipi_engine = IpIntelligenceCloudEngine::builder()
            .cloud_request_engine(Arc::clone(&request_engine))
            .build()?;

        let mut pipeline_builder = Pipeline::builder();
        if self.share_usage {
            pipeline_builder = pipeline_builder.add_element(Arc::new(SequenceElement::new()));
        }
        pipeline_builder = pipeline_builder
            .add_element(request_engine)
            .add_element(Arc::new(ipi_engine));
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
    use std::sync::Arc;

    use fiftyone_cloud_request_engine::{
        CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine, HttpMethod,
    };
    use fiftyone_ip_intelligence_cloud::IpIntelligenceCloudEngine;
    use fiftyone_ip_intelligence_shared::{IpIntelligenceData, IP_DATA_KEY};
    use fiftyone_pipeline_core::{Evidence, Pipeline};

    use crate::IpIntelligencePipelineBuilder;

    /// A stub HTTP client that answers the cloud request engine's evidence-keys
    /// discovery GET and the data POST with canned bodies, so the cloud pipeline
    /// can be exercised end to end without a network. The accessible-properties
    /// discovery GET is left to fail, which exercises the IPI cloud engine's
    /// graceful fall back to value-kind inference.
    struct StubClient;

    impl CloudHttpClient for StubClient {
        fn send(
            &self,
            request: &CloudHttpRequest,
        ) -> std::result::Result<CloudHttpResponse, String> {
            let ok = |body: &str| {
                Ok(CloudHttpResponse {
                    status: 200,
                    body: body.to_owned(),
                    retry_after: None,
                })
            };
            // The data request is the POST to the json endpoint; discovery
            // requests are GETs distinguished by their URL.
            if request.method == HttpMethod::Post {
                // RegisteredCountry and CountryCode are plain string properties;
                // CountryCodesGeographical is a genuinely weighted list. With no
                // accessible-properties metadata the engine infers these kinds
                // from the JSON shape (a bare scalar is plain; an array of
                // weighted objects is weighted).
                ok(r#"{
                    "ip": {
                        "RegisteredCountry": "US",
                        "CountryCode": "US",
                        "CountryCodesGeographical": [
                            { "rawweighting": 20000, "value": "GB" },
                            { "rawweighting": 60000, "value": "US" }
                        ]
                    }
                }"#)
            } else if request.url.contains("evidencekeys") {
                // The evidence-keys body is a flat JSON array of accepted keys.
                ok(r#"["query.client-ip-51d","server.client-ip"]"#)
            } else {
                // The accessible-properties discovery returns no products, so the
                // engine has no metadata and infers value kinds from the JSON
                // instead. (Returning an empty body rather than an error lets the
                // builder's build-time discovery succeed.)
                ok(r#"{"Products":{}}"#)
            }
        }
    }

    #[test]
    fn cloud_builder_assembles_a_two_element_pipeline() {
        // Build the request engine with the stub client, then assemble the same
        // two-element pipeline the facade builds, so the canned response can be
        // processed offline.
        let request_engine = Arc::new(
            CloudRequestEngine::builder()
                .resource_key("test-key")
                .http_client(Arc::new(StubClient))
                .build()
                .expect("request engine should build"),
        );
        let ipi_engine = IpIntelligenceCloudEngine::builder()
            .cloud_request_engine(Arc::clone(&request_engine))
            .build()
            .expect("ipi cloud engine should build");

        let pipeline = Pipeline::builder()
            .add_element(request_engine)
            .add_element(Arc::new(ipi_engine))
            .build()
            .expect("cloud pipeline should build");

        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("query.client-ip-51d", "185.28.167.78")
                .build(),
        );
        data.process().expect("processing should not error");

        let ip = data.get(IP_DATA_KEY).expect("ip data should be present");
        // CountryCode and RegisteredCountry are plain string properties.
        assert_eq!(
            ip.country_code()
                .into_value()
                .expect("country code should resolve"),
            "US"
        );
        assert_eq!(
            ip.registered_country()
                .value()
                .expect("registered country")
                .as_str(),
            "US"
        );
    }

    #[test]
    fn cloud_builder_requires_a_resource_key() {
        // An empty resource key must surface as a configuration error from the
        // cloud request engine, propagated through the facade builder.
        let result = IpIntelligencePipelineBuilder::cloud("").build();
        assert!(
            result.is_err(),
            "an empty resource key must be a configuration error"
        );
    }

    // Gated on `reqwest-client`: a successful cloud build needs an HTTP
    // transport, which the built-in reqwest client supplies. Without it the
    // build deliberately errors for want of a transport (covered by
    // `cloud_builder_requires_a_resource_key` and the engine's no-transport
    // check), so this success case applies only when the client is available.
    #[cfg(feature = "reqwest-client")]
    #[test]
    fn cloud_builder_keeps_configuration() {
        // The fluent setters return the builder, so a configured chain still
        // builds. An injected state keeps the build offline (no discovery fetch),
        // so the test does not touch the network.
        let pipeline = IpIntelligencePipelineBuilder::cloud("test-key")
            .cloud_request_origin("https://example.51degrees.com")
            .timeout(std::time::Duration::from_secs(5))
            .set_state(fiftyone_cloud_request_engine::CloudEngineState::default())
            .build();
        assert!(pipeline.is_ok(), "a configured cloud builder should build");
    }

    #[cfg(all(feature = "reqwest-client", feature = "share-usage"))]
    #[test]
    fn usage_sharing_is_on_by_default() {
        // Built but never processed, so nothing is queued or sent.
        let pipeline = IpIntelligencePipelineBuilder::cloud("test-key")
            .set_state(fiftyone_cloud_request_engine::CloudEngineState::default())
            .build()
            .expect("a cloud pipeline should build offline from an injected state");
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
        let pipeline = IpIntelligencePipelineBuilder::cloud("test-key")
            .set_state(fiftyone_cloud_request_engine::CloudEngineState::default())
            .share_usage(true)
            .build()
            .expect("a cloud pipeline should build offline from an injected state");

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
}
