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

//! Cloud engine: replace the hard-coded star-sign logic with a call to a remote
//! service.

use std::any::Any;
use std::sync::Arc;

use anyhow::Result;
use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, PropertyValueType,
    TypedKey,
};
use pipeline_examples::star_sign::{
    StarSignData, STAR_SIGN_DATA_KEY, STAR_SIGN_PROPERTY, UNKNOWN_STAR_SIGN,
};

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Pipeline API", "https://51degrees.com/documentation/_pipeline_api__index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-ss-cloud.rs&utm_term=find-out-more"),
    ("Cloud service", "https://51degrees.com/documentation/_services__cloud__index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-ss-cloud.rs&utm_term=find-out-more"),
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-ss-cloud.rs&utm_term=find-out-more"),
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-pipeline-examples-src-bin-ss-cloud.rs&utm_term=find-out-more"),
];

/// The evidence key the request engine sends to the remote service.
const DATE_OF_BIRTH_EVIDENCE: &str = "date-of-birth";

/// The data key the request engine stores the raw cloud response under.
const CLOUD_RESPONSE_DATA_KEY: &str = "cloud-response";

/// The resource key the example would authenticate the cloud request with. A
/// real deployment reads this from `51DEGREES_RESOURCE_KEY`; the example stubs
/// the call, so the value is illustrative only.
const EXAMPLE_RESOURCE_KEY: &str = "!!YOUR_RESOURCE_KEY!!";

/// The endpoint the example would call. Documented for realism; the example does
/// not actually reach the network.
const EXAMPLE_ENDPOINT: &str = "https://cloud.51degrees.com/api/v4/";

/// The element data the request engine produces: the raw JSON payload returned by
/// the remote service.
#[derive(Debug, Clone, Default)]
struct CloudResponseData {
    inner: MapElementData,
}

impl CloudResponseData {
    /// The data key for the raw cloud response.
    const KEY: TypedKey<CloudResponseData> = TypedKey::new(CLOUD_RESPONSE_DATA_KEY);

    /// Wrap a raw JSON response string.
    fn new(json: impl Into<String>) -> Self {
        CloudResponseData {
            inner: MapElementData::new().set("json", json.into()),
        }
    }

    /// The raw JSON response, if present.
    fn json(&self) -> Option<&str> {
        self.inner.get_value("json").and_then(PropertyValue::as_str)
    }
}

impl ElementData for CloudResponseData {
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
        self.inner.get(name)
    }
    fn keys(&self) -> Vec<String> {
        self.inner.keys()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A stand-in for the shared cloud request engine.
///
/// In production this is the real `fiftyone_pipeline_cloud_request_engine`
/// `CloudRequestEngine`, configured with a resource key and an endpoint, which
/// posts the flow data's evidence to the 51Degrees cloud and stores the JSON
/// response in the flow data. Here it calls a local function in place of the HTTP
/// request so the example runs offline; the boundary it sits on (evidence in,
/// JSON out) is identical.
struct StubCloudRequestEngine {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    resource_key: String,
    endpoint: String,
}

impl StubCloudRequestEngine {
    fn new(resource_key: impl Into<String>, endpoint: impl Into<String>) -> Self {
        StubCloudRequestEngine {
            filter: EvidenceKeyFilterWhitelist::new([DATE_OF_BIRTH_EVIDENCE]),
            properties: vec![PropertyMetaData::new(
                "json",
                CLOUD_RESPONSE_DATA_KEY,
                PropertyValueType::String,
            )],
            resource_key: resource_key.into(),
            endpoint: endpoint.into(),
        }
    }
}

impl FlowElement for StubCloudRequestEngine {
    fn process(&self, data: &mut FlowData) -> Result<(), fiftyone_pipeline_core::Error> {
        let date = data
            .evidence()
            .get(DATE_OF_BIRTH_EVIDENCE)
            .unwrap_or_default()
            .to_owned();

        // This is where the real CloudRequestEngine performs a synchronous HTTP
        // POST to `self.endpoint`, authenticated with `self.resource_key`,
        // sending the request evidence and receiving a JSON document. We call a
        // local stub instead so the example needs no network or key.
        let json = call_remote_star_sign_service(&self.resource_key, &self.endpoint, &date);

        data.get_or_add(CloudResponseData::KEY, || CloudResponseData::new(json))?;
        Ok(())
    }
    fn data_key(&self) -> &str {
        CLOUD_RESPONSE_DATA_KEY
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// The star-sign cloud engine: turns the request engine's raw JSON into the typed
/// `starsign` property.
///
/// A cloud aspect engine does no detection itself; it reads the JSON the request
/// engine fetched and surfaces the slice of it that belongs to this aspect. That
/// is exactly what the production device-detection and IP-intelligence cloud
/// engines do, so this mirrors their shape while keeping the parsing trivial.
struct StarSignCloudEngine {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl StarSignCloudEngine {
    const KEY: TypedKey<StarSignData> = TypedKey::new(STAR_SIGN_DATA_KEY);

    fn new() -> Self {
        StarSignCloudEngine {
            // The cloud engine reads no request evidence directly; it depends on
            // the request engine's response, so its filter is empty.
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: vec![PropertyMetaData::new(
                STAR_SIGN_PROPERTY,
                STAR_SIGN_DATA_KEY,
                PropertyValueType::String,
            )],
        }
    }
}

impl FlowElement for StarSignCloudEngine {
    fn process(&self, data: &mut FlowData) -> Result<(), fiftyone_pipeline_core::Error> {
        // Pull the request engine's response out of the flow data and extract the
        // sign from it. The response engine ran earlier in the pipeline.
        let sign = data
            .get(CloudResponseData::KEY)
            .and_then(CloudResponseData::json)
            .and_then(extract_star_sign_from_json)
            .unwrap_or_else(|| UNKNOWN_STAR_SIGN.to_owned());

        let star_sign_data = data.get_or_add(Self::KEY, StarSignData::new)?;
        star_sign_data.set_star_sign(sign);
        Ok(())
    }
    fn data_key(&self) -> &str {
        STAR_SIGN_DATA_KEY
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// Stand-in for the remote star-sign web service.
///
/// In place of an HTTP call this computes the sign locally (reusing the shared
/// table) and formats it as the kind of JSON document the cloud would return. The
/// resource key and endpoint are accepted only so the signature matches the real
/// request, and to show where they would be used.
fn call_remote_star_sign_service(_resource_key: &str, _endpoint: &str, date: &str) -> String {
    let sign = pipeline_examples::star_sign::parse_day_month(date)
        .and_then(|(month, day)| {
            pipeline_examples::star_sign::star_sign_for(
                &pipeline_examples::star_sign::STAR_SIGNS,
                month,
                day,
            )
        })
        .unwrap_or(UNKNOWN_STAR_SIGN);
    format!("{{\"starsign\":{{\"starsign\":\"{sign}\"}}}}")
}

/// Pull the `starsign` value out of the stubbed JSON document. A real cloud
/// engine parses the full JSON with serde; this does a minimal extraction so the
/// example carries no extra parsing machinery.
fn extract_star_sign_from_json(json: &str) -> Option<String> {
    let marker = "\"starsign\":\"";
    let start = json.rfind(marker)? + marker.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

/// Options controlling one run of the example.
pub struct ExampleOptions {
    /// The birth date as a `dd/mm/yyyy` string.
    pub date_of_birth: String,
    /// The resource key the request engine would authenticate with.
    pub resource_key: String,
    /// The cloud endpoint the request engine would call.
    pub endpoint: String,
}

impl Default for ExampleOptions {
    fn default() -> Self {
        ExampleOptions {
            date_of_birth: "18/12/1992".to_owned(),
            resource_key: EXAMPLE_RESOURCE_KEY.to_owned(),
            endpoint: EXAMPLE_ENDPOINT.to_owned(),
        }
    }
}

/// Run the example: build a two-element cloud-shaped pipeline (request engine
/// then aspect engine), process a birth date and read the sign the "cloud"
/// returned.
pub fn run(options: &ExampleOptions) -> Result<()> {
    let pipeline: Arc<Pipeline> = Pipeline::builder()
        .add_element(Arc::new(StubCloudRequestEngine::new(
            options.resource_key.clone(),
            options.endpoint.clone(),
        )))
        .add_element(Arc::new(StarSignCloudEngine::new()))
        .build()?;

    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add(DATE_OF_BIRTH_EVIDENCE, options.date_of_birth.clone())
            .build(),
    );
    data.process()?;

    let sign = data
        .get(StarSignCloudEngine::KEY)
        .and_then(StarSignData::star_sign)
        .unwrap_or(UNKNOWN_STAR_SIGN)
        .to_owned();
    println!(
        "With a date of birth of {}, your star sign is {sign} (via the cloud).",
        options.date_of_birth
    );
    Ok(())
}

/// Read an optional birth date from the command line, then run the example.
fn main() -> Result<()> {
    let mut options = ExampleOptions::default();
    if let Some(date) = std::env::args().nth(1) {
        options.date_of_birth = date;
    }
    // A real cloud example would read the resource key and the endpoint from the
    // environment, so this one does the same. A host other than
    // cloud.51degrees.com is an on premise web server or a privately hosted
    // 51Degrees cloud (see examples_shared::CLOUD_ENDPOINT_ENV_VAR), normalised
    // to end in one slash as the cloud request engine does.
    if let Some(key) = examples_shared::resource_key_from_env() {
        options.resource_key = key;
    }
    if let Some(endpoint) = examples_shared::cloud_endpoint_from_env() {
        options.endpoint = format!("{}/", endpoint.trim_end_matches('/'));
    }
    let result = run(&options);
    examples_shared::print_find_out_more(FIND_OUT_MORE, pipeline_examples::REPOSITORIES);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_with_default_options() {
        run(&ExampleOptions::default()).expect("the cloud example should run");
    }

    #[test]
    fn surfaces_the_cloud_response() {
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(StubCloudRequestEngine::new(
                EXAMPLE_RESOURCE_KEY,
                EXAMPLE_ENDPOINT,
            )))
            .add_element(Arc::new(StarSignCloudEngine::new()))
            .build()
            .unwrap();
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add(DATE_OF_BIRTH_EVIDENCE, "18/12/1992")
                .build(),
        );
        data.process().unwrap();
        assert_eq!(
            data.get(StarSignCloudEngine::KEY)
                .and_then(StarSignData::star_sign),
            Some("Sagittarius")
        );
    }
}

/* ---------------------------------------------------------------------------
 * Example: Cloud Engine (star sign from a remote service)
 *
 * This example replaces the hard-coded star-sign logic with a call to a remote
 * service. It shows the two-element shape every 51Degrees cloud integration
 * uses: a cloud request engine that talks to the network, followed by one or
 * more aspect engines that turn the response into typed properties.
 *
 * The two elements
 * ----------------
 *   1. The cloud request engine (`StubCloudRequestEngine`). It collects the
 *      relevant evidence, sends it to the cloud and stores the raw JSON response
 *      in the flow data. In production this is the shared
 *      `fiftyone_pipeline_cloud_request_engine::CloudRequestEngine`, built with a
 *      resource key and an endpoint. To keep the example offline and key-free,
 *      this stub calls a local function (`call_remote_star_sign_service`) that
 *      returns the same JSON the cloud would. The evidence-in, JSON-out boundary
 *      is identical to the real engine.
 *
 *   2. The aspect engine (`StarSignCloudEngine`). It reads the JSON the request
 *      engine fetched and surfaces just the `starsign` property. It does no
 *      detection of its own. The real device-detection and IP-intelligence cloud
 *      engines work exactly this way: one request engine fetches a combined JSON
 *      response, and each aspect engine projects out its own properties.
 *
 * Why split the work in two
 * -------------------------
 * Separating the network call from the result parsing means a single cloud
 * request can serve several aspect engines (device, IP, location and so on) from
 * one round trip, and the request engine alone owns retries, caching and error
 * handling.
 *
 * Configuration
 * -------------
 * A real run reads the resource key from `51DEGREES_RESOURCE_KEY` (the example's
 * `main` does this) and points the engine at the 51Degrees cloud endpoint. The
 * stub ignores both, but accepts them so the wiring matches production.
 *
 * Usage sharing
 * -------------
 * This is a console example, so it does NOT add usage sharing. The cloud service
 * itself improves from the evidence it receives; a production application can
 * additionally run the usage-sharing element (see the `usage-sharing` example).
 *
 * Running it
 * ----------
 *   cargo run -p pipeline-examples --bin ss-cloud [dd/mm/yyyy]
 *
 * With no argument it uses 18/12/1992 and prints "Sagittarius (via the cloud)".
 * ------------------------------------------------------------------------- */
