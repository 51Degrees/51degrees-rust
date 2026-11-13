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

//! End-to-end tests for the cloud 51Did engine.
//!
//! These drive a real pipeline (cloud request engine followed by the identifier
//! cloud engine) against a fake HTTP transport, so no network is touched. A
//! representative `fodid` response is fed through and the resulting
//! [`FodIdDataBase`] is asserted on, both as the raw base64 envelope and as the
//! parsed [`FodId`]. The global identifier in the fixture is a genuinely signed
//! OWID envelope, created here with the `owid` library, so the parsed path is
//! exercised against real bytes rather than a hand-rolled string.

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::sync::{Arc, Mutex};

use fiftyone_pipeline_core::{ElementData, Evidence, FlowData, Pipeline};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine, HttpMethod, LruCache,
};
use fiftyone_fodid_cloud::{FodIdCloudEngine, FodIdData, FODID_DATA_KEY};
use fiftyone_pipeline_engines::AspectData;
use fodid::{Creator, Crypto, IdType, Owid, Usage};

/// A fake transport that answers the cloud endpoints from in-memory fixtures.
struct FakeCloud {
    accessible_properties: String,
    evidence_keys: String,
    data: String,
}

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let body = if request.url.contains("accessibleproperties") {
            self.accessible_properties.clone()
        } else if request.url.contains("evidencekeys") {
            self.evidence_keys.clone()
        } else {
            assert_eq!(request.method, HttpMethod::Post);
            self.data.clone()
        };
        Ok(CloudHttpResponse {
            status: 200,
            body,
            retry_after: None,
        })
    }
}

const TEST_DOMAIN: &str = "51degrees.com";
const FLAGS: u8 = 0b0000_0001;
const LICENSE_ID: u32 = 0x1234_5678;

/// A canonical 37-byte 51Did payload: flags, little-endian license id and a
/// recognizable 32-byte hash (0x40..0x5F).
fn canonical_payload() -> Vec<u8> {
    // 1-byte flags + 4-byte license id + 32-byte hash = 37 bytes.
    let mut payload = vec![0u8; 37];
    payload[0] = FLAGS;
    payload[1..5].copy_from_slice(&LICENSE_ID.to_le_bytes());
    for (i, b) in payload[5..37].iter_mut().enumerate() {
        *b = 0x40 + i as u8;
    }
    payload
}

/// Create a genuinely signed 51Did envelope and return its base64 form together
/// with the public key PEM, so the test can both parse and (optionally) verify.
fn signed_global_id() -> (String, String) {
    let crypto = Crypto::new();
    let public_pem = crypto.public_key_pem().expect("export public key");
    let private_pem = crypto.private_key_pem().expect("export private key");

    let signer = Crypto::new_sign_only(&private_pem).expect("import private key");
    let creator = Creator::new(TEST_DOMAIN, signer).expect("create creator");
    let owid: Owid = creator
        .create(canonical_payload())
        .expect("create and sign the envelope");
    (owid.as_base64().expect("encode owid"), public_pem)
}

/// The accessible-properties body advertising the identifier product.
const ACCESSIBLE_PROPERTIES: &str = r#"{
    "Products": {
        "fodid": {
            "DataTier": "CloudV4",
            "Properties": [
                { "Name": "IdProbGlobal", "Type": "String", "Category": "Identifier" },
                { "Name": "IdProbLic", "Type": "String", "Category": "Identifier" },
                { "Name": "IdRandGlobal", "Type": "String", "Category": "Identifier" },
                { "Name": "IdRandLic", "Type": "String", "Category": "Identifier" },
                { "Name": "IdHemGlobal", "Type": "String", "Category": "Identifier" },
                { "Name": "IdHemLic", "Type": "String", "Category": "Identifier" }
            ]
        }
    }
}"#;

const EVIDENCE_KEYS: &str = r#"["header.user-agent","query.client-ip","query.id.usage"]"#;

/// Build a pipeline of `CloudRequestEngine` -> `FodIdCloudEngine` over a fake
/// transport returning the supplied data body.
fn fake_pipeline(data: &str) -> Arc<Pipeline> {
    let fake = Arc::new(FakeCloud {
        accessible_properties: ACCESSIBLE_PROPERTIES.to_owned(),
        evidence_keys: EVIDENCE_KEYS.to_owned(),
        data: data.to_owned(),
    });

    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .http_client(fake)
            .build()
            .expect("request engine builds"),
    );

    let fodid_engine = FodIdCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .build()
        .expect("identifier engine builds");

    Pipeline::builder()
        .add_element(request_engine)
        .add_element(Arc::new(fodid_engine))
        .build()
        .expect("pipeline builds")
}

fn sample_evidence() -> Evidence {
    Evidence::builder()
        .add("header.user-agent", "Mozilla/5.0")
        .add("query.client-ip", "185.28.167.78")
        .add("query.id.usage", "non-marketing")
        .build()
}

#[test]
fn unpacks_global_identifier_raw_and_parsed() {
    let (global_base64, public_pem) = signed_global_id();
    let data_body = format!(
        r#"{{
            "fodid": {{
                "idprobglobal": "{global_base64}",
                "idproblic": null,
                "idproblicnullreason": "The usage policy does not permit a licence identifier."
            }},
            "javascriptProperties": [],
            "warnings": []
        }}"#
    );

    let pipeline = fake_pipeline(&data_body);
    let mut data = pipeline.create_flow_data_with(sample_evidence());
    data.process().expect("processing succeeds");

    let fodid = data
        .get(FODID_DATA_KEY)
        .expect("identifier data is present under the shared key");

    // Raw envelope comes through verbatim.
    assert_eq!(fodid.id_prob_global().value().unwrap(), &global_base64);

    // Parsed form unpacks the payload fields from the real envelope.
    let parsed = fodid.id_prob_global_fod_id();
    let fod_id = parsed.value().expect("the global identifier parses");
    // FLAGS is 0b0000_0001, so the usage bits grant non-marketing only,
    // bit 3 is clear, and bits 6-7 leave the type probabilistic.
    assert_eq!(fod_id.usage(), Usage::NonMarketing);
    assert!(!fod_id.usage_is_indirect());
    assert_eq!(fod_id.id_type(), IdType::Probabilistic);
    assert_eq!(fod_id.license_id(), LICENSE_ID);
    assert_eq!(fod_id.domain(), TEST_DOMAIN);

    // The created envelope verifies against its own public key, proving the
    // parsed value carries the full OWID envelope intact.
    assert!(fod_id
        .verify_with_public_key(&public_pem)
        .expect("verification runs"));
}

#[test]
fn null_licence_identifier_is_no_value_with_reason() {
    let (global_base64, _) = signed_global_id();
    let data_body = format!(
        r#"{{
            "fodid": {{
                "idprobglobal": "{global_base64}",
                "idproblic": null,
                "idproblicnullreason": "The usage policy does not permit a licence identifier."
            }}
        }}"#
    );

    let pipeline = fake_pipeline(&data_body);
    let mut data = pipeline.create_flow_data_with(sample_evidence());
    data.process().expect("processing succeeds");
    let fodid = data.get(FODID_DATA_KEY).expect("identifier data present");

    // The license-scoped identifier was null, so both the raw and parsed
    // accessors report a no-value.
    assert!(!fodid.id_prob_lic().has_value());
    assert!(!fodid.id_prob_lic_fod_id().has_value());

    // The cloud's explanation is preserved verbatim under the sibling key.
    assert_eq!(
        fodid.get("idproblicnullreason").unwrap().as_str(),
        Some("The usage policy does not permit a licence identifier.")
    );
}

#[test]
fn unpacks_all_six_identifiers() {
    // Use one real signed envelope for every identifier so each raw accessor
    // can be checked and the new random / hashed-email accessors can be parsed.
    let (envelope, _) = signed_global_id();
    let data_body = format!(
        r#"{{
            "fodid": {{
                "idprobglobal": "{envelope}",
                "idproblic": "{envelope}",
                "idrandglobal": "{envelope}",
                "idrandlic": "{envelope}",
                "idhemglobal": "{envelope}",
                "idhemlic": "{envelope}"
            }}
        }}"#
    );

    let pipeline = fake_pipeline(&data_body);
    let mut data = pipeline.create_flow_data_with(sample_evidence());
    data.process().expect("processing succeeds");
    let fodid = data.get(FODID_DATA_KEY).expect("identifier data present");

    // All six raw identifiers come through verbatim.
    for raw in [
        fodid.id_prob_global(),
        fodid.id_prob_lic(),
        fodid.id_rand_global(),
        fodid.id_rand_lic(),
        fodid.id_hem_global(),
        fodid.id_hem_lic(),
    ] {
        assert_eq!(raw.value().unwrap(), &envelope);
    }

    // The new random and hashed-email accessors also parse to a FodId.
    assert_eq!(
        fodid.id_rand_global_fod_id().value().unwrap().license_id(),
        LICENSE_ID
    );
    assert_eq!(
        fodid.id_hem_lic_fod_id().value().unwrap().license_id(),
        LICENSE_ID
    );
}

#[test]
fn response_without_fodid_block_yields_empty_data() {
    // A resource key without the identifier product returns a `device` block and
    // no `fodid`. The engine must produce an empty result rather than fail.
    let data_body = r#"{ "device": { "ismobile": false }, "javascriptProperties": [] }"#;
    let pipeline = fake_pipeline(data_body);
    let mut data = pipeline.create_flow_data_with(sample_evidence());
    data.process().expect("processing succeeds");

    let fodid = data
        .get(FODID_DATA_KEY)
        .expect("identifier data is present even when empty");
    assert!(!fodid.id_prob_global().has_value());
    assert!(!fodid.id_prob_lic().has_value());
}

/// A transport whose data endpoint fails, as in a cloud outage.
struct DataDownCloud;

impl CloudHttpClient for DataDownCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let (status, body) = if request.url.contains("accessibleproperties") {
            (200, ACCESSIBLE_PROPERTIES.to_owned())
        } else if request.url.contains("evidencekeys") {
            (200, EVIDENCE_KEYS.to_owned())
        } else {
            (500, r#"{"errors":["the service failed"]}"#.to_owned())
        };
        Ok(CloudHttpResponse {
            status,
            body,
            retry_after: None,
        })
    }
}

/// When the cloud request fails, the 51Did engine records that it did not
/// process, without failing processing, as .NET's cloud aspect engines do,
/// and still gives empty identifier data.
#[test]
fn a_failed_request_records_that_the_51did_engine_did_not_process() {
    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .http_client(Arc::new(DataDownCloud))
            .build()
            .expect("request engine builds"),
    );
    let fodid_engine = FodIdCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .build()
        .expect("identifier engine builds");
    let pipeline = Pipeline::builder()
        .add_element(request_engine)
        .add_element(Arc::new(fodid_engine))
        .suppress_process_exceptions(true)
        .build()
        .expect("pipeline builds");
    let mut data = pipeline.create_flow_data_with(sample_evidence());
    data.process().expect("errors are suppressed");

    let recorded: Vec<(&str, bool)> = data
        .errors()
        .iter()
        .map(|error| (error.element_data_key.as_str(), error.should_throw))
        .collect();
    assert_eq!(recorded, vec![("cloud-response", true), ("fodid", false)]);
    assert_eq!(
        data.errors()[1].source.to_string(),
        "pipeline error: The 'FodIdCloudEngine' did not process because the JSON \
         response from the CloudRequestEngine was null or empty. This indicates \
         that the cloud request failed."
    );
    let fodid = data
        .get(FODID_DATA_KEY)
        .expect("identifier data is present after a failed request");
    assert!(!fodid.id_prob_global().has_value());
    assert!(!fodid.base().cloud_request_failed());
}

/// The flags byte of a random 51Did created for non-marketing use. Bits 6 and
/// 7 select the random type and bit 0 grants non-marketing.
const RANDOM_FLAGS: u8 = 0b0100_0001;

/// A fake transport that answers the data endpoint as the cloud does.
///
/// The cloud creates a new 51Did for every request that states a usage, and
/// leaves the `fodid` member out of a response to one that does not, while the
/// device answer is the same for the same evidence. Each 51Did created here is
/// a random one whose GUID carries the number of the request it answered, and
/// each is recorded in the order it was created.
struct CreatingCloud {
    private_pem: String,
    created: Mutex<Vec<String>>,
    data_requests: Mutex<usize>,
}

impl CreatingCloud {
    fn new() -> Self {
        let crypto = Crypto::new();
        CreatingCloud {
            private_pem: crypto.private_key_pem().expect("export private key"),
            created: Mutex::new(Vec::new()),
            data_requests: Mutex::new(0),
        }
    }

    /// The 51Dids created so far, oldest first.
    fn created(&self) -> Vec<String> {
        self.created.lock().unwrap().clone()
    }

    /// The number of requests that reached the data endpoint.
    fn data_requests(&self) -> usize {
        *self.data_requests.lock().unwrap()
    }

    /// Create a signed random 51Did whose GUID starts with `sequence`.
    fn create(&self, sequence: u32) -> String {
        let mut payload = vec![0u8; 21];
        payload[0] = RANDOM_FLAGS;
        payload[1..5].copy_from_slice(&LICENSE_ID.to_le_bytes());
        payload[5..9].copy_from_slice(&sequence.to_le_bytes());
        let signer = Crypto::new_sign_only(&self.private_pem).expect("import private key");
        let creator = Creator::new(TEST_DOMAIN, signer).expect("create creator");
        let owid = creator
            .create(payload)
            .expect("create and sign the envelope");
        owid.as_base64().expect("encode owid")
    }
}

impl CloudHttpClient for CreatingCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let body = if request.url.contains("accessibleproperties") {
            ACCESSIBLE_PROPERTIES.to_owned()
        } else if request.url.contains("evidencekeys") {
            EVIDENCE_KEYS.to_owned()
        } else {
            let sequence = {
                let mut count = self.data_requests.lock().unwrap();
                *count += 1;
                *count as u32
            };
            let states_usage = request.form.iter().any(|(name, _)| name == "id.usage");
            if states_usage {
                let created = self.create(sequence);
                self.created.lock().unwrap().push(created.clone());
                format!(
                    r#"{{"device":{{"ismobile":false}},"fodid":{{"idrandglobal":"{created}"}},"javascriptProperties":[]}}"#
                )
            } else {
                r#"{"device":{"ismobile":false},"javascriptProperties":[]}"#.to_owned()
            }
        };
        Ok(CloudHttpResponse {
            status: 200,
            body,
            retry_after: None,
        })
    }
}

/// A pipeline of a request engine with the in-process response cache and the
/// identifier engine, over the creating fake cloud.
fn caching_pipeline(cloud: Arc<CreatingCloud>) -> Arc<Pipeline> {
    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .http_client(cloud)
            .cache(Arc::new(LruCache::<String, String>::with_defaults()))
            .build()
            .expect("request engine builds"),
    );
    let fodid_engine = FodIdCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .build()
        .expect("identifier engine builds");
    Pipeline::builder()
        .add_element(request_engine)
        .add_element(Arc::new(fodid_engine))
        .build()
        .expect("pipeline builds")
}

/// The random 51Did a processed flow data carries, raw and as its match key.
fn random_51did(data: &FlowData) -> (String, Vec<u8>) {
    let fodid = data.get(FODID_DATA_KEY).expect("identifier data present");
    let raw = fodid
        .id_rand_global()
        .value()
        .expect("a 51Did was received")
        .clone();
    let parsed = fodid.id_rand_global_fod_id();
    let parsed = parsed.value().expect("the 51Did parses");
    (raw, parsed.match_key().to_vec())
}

#[test]
fn two_visitors_with_the_same_evidence_never_share_a_51did() {
    // Two people on one browser version behind one network address present
    // the same evidence, so their requests have the same response cache key.
    // The cloud creates a different 51Did for each request, and each visitor
    // must receive the one created for their own.
    let cloud = Arc::new(CreatingCloud::new());
    let pipeline = caching_pipeline(cloud.clone());

    let mut first = pipeline.create_flow_data_with(sample_evidence());
    first.process().expect("the first visitor is processed");
    let mut second = pipeline.create_flow_data_with(sample_evidence());
    second.process().expect("the second visitor is processed");

    let (first_raw, first_key) = random_51did(&first);
    let (second_raw, second_key) = random_51did(&second);
    assert_ne!(
        second_key, first_key,
        "the second visitor was handed the 51Did created for the first"
    );

    let created = cloud.created();
    assert_eq!(
        created.len(),
        2,
        "the cloud created a 51Did for each visitor"
    );
    assert_eq!(first_raw, created[0]);
    assert_eq!(second_raw, created[1]);
    let cloud_data = second
        .get(CloudRequestEngine::DATA_KEY)
        .expect("cloud data present");
    assert!(
        !cloud_data.cache_hit(),
        "a response carrying a 51Did is not served from the cache"
    );
}

#[test]
fn requests_that_ask_for_no_51did_are_still_served_from_the_cache() {
    // Without a usage the cloud creates no 51Did and the answer depends on the
    // evidence alone, so the cache still saves the second call.
    let cloud = Arc::new(CreatingCloud::new());
    let pipeline = caching_pipeline(cloud.clone());
    let evidence = || {
        Evidence::builder()
            .add("header.user-agent", "Mozilla/5.0")
            .add("query.client-ip", "185.28.167.78")
            .build()
    };

    let mut first = pipeline.create_flow_data_with(evidence());
    first.process().expect("the first request is processed");
    let mut second = pipeline.create_flow_data_with(evidence());
    second.process().expect("the second request is processed");

    assert_eq!(
        cloud.data_requests(),
        1,
        "the second answer came from the cache"
    );
    let cloud_data = second
        .get(CloudRequestEngine::DATA_KEY)
        .expect("cloud data present");
    assert!(cloud_data.cache_hit(), "the second request is a cache hit");
    let fodid = second.get(FODID_DATA_KEY).expect("identifier data present");
    assert!(
        !fodid.id_rand_global().has_value(),
        "no 51Did was asked for"
    );
}

/// Resolve a cloud resource key from the environment for the live test,
/// honouring the aligned name first and then the CI-exported bespoke and basic
/// names, so the live test runs from an explicit key or the keys CI exports.
fn live_resource_key() -> Option<String> {
    [
        "_51DEGREES_RESOURCE_KEY",
        "_51DEGREES_RESOURCE_KEY_BESPOKE",
        "_51DEGREES_RESOURCE_KEY_BASIC",
    ]
    .into_iter()
    .find_map(|name| match crate::deprecated_names::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        _ => None,
    })
}

#[test]
#[ignore = "requires network and a resource key (_51DEGREES_RESOURCE_KEY or the _51DEGREES_RESOURCE_KEY_BESPOKE/_BASIC tiered names)"]
fn live_cloud_returns_a_parseable_identifier() {
    let Some(resource_key) = live_resource_key() else {
        eprintln!("no resource key in the environment; skipping live cloud test");
        return;
    };

    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key(resource_key)
            .build()
            .expect("request engine builds"),
    );
    let fodid_engine = FodIdCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .build()
        .expect("identifier engine builds");
    let pipeline = Pipeline::builder()
        .add_element(request_engine)
        .add_element(Arc::new(fodid_engine))
        .build()
        .expect("pipeline builds");

    let mut data = pipeline.create_flow_data_with(sample_evidence());
    // Processing must succeed regardless of the key's product tier: this catches
    // transport, authentication and deserialization regressions.
    data.process().expect("live cloud processing succeeds");

    let fodid = data.get(FODID_DATA_KEY).expect("identifier data present");
    // The identifier is only returned when the resource key grants the FODid
    // product. A key without it processes cleanly but yields no identifier, so
    // the content assertion is skipped in that case rather than failing.
    match fodid.id_prob_global_fod_id().value() {
        Ok(fod_id) => {
            assert_eq!(
                fod_id.match_key().len(),
                32,
                "a parsed 51Did has a 32-byte hash"
            );
        }
        Err(_) => eprintln!(
            "the resource key returned no 51Degrees identifier (it may not grant the \
             FODid product); skipping the live identifier assertion"
        ),
    }
}
