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

//! A signer that signs requests with the Ed25519 test key of RFC 9421
//! Appendix B.1.4, and an HTTP client that serves key directories from
//! memory, so the element is tested end to end with no network.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use fiftyone_agent_signature::{
    constants, AgentSignatureData, AgentSignatureElement, AgentSignatureElementBuilder, HttpClient,
    HttpRequest, HttpResponse,
};
use fiftyone_pipeline_core::{Evidence, Pipeline};
use ring::signature::Ed25519KeyPair;

/// The private part of the Ed25519 test key of RFC 9421 Appendix B.1.4.
pub const ED25519_SEED: [u8; 32] = [
    0x9f, 0x83, 0x62, 0xf8, 0x7a, 0x48, 0x4a, 0x95, 0x4e, 0x6e, 0x74, 0x0c, 0x5b, 0x4c, 0x0e, 0x84,
    0x22, 0x91, 0x39, 0xa2, 0x0a, 0xa8, 0xab, 0x56, 0xff, 0x66, 0x58, 0x6f, 0x6a, 0x7d, 0x29, 0xc5,
];

/// The public part of that key, as RFC 9421 Appendix B.1.4 prints it.
pub const ED25519_X: &str = "JrQLj5P_89iXES9-vFgrIy29clF9CC_oPPsw3c5D0bs";

/// The thumbprint of that key.
pub const ED25519_THUMBPRINT: &str = "poqkLGiymh_W0uP6PZFw-dvez3QJT5SolqXBCW38r0U";

/// The origin the test agent publishes its keys at.
pub const AGENT: &str = "https://signature-agent.test";

/// Where the test agent's key directory is fetched from.
pub const DIRECTORY_URL: &str =
    "https://signature-agent.test/.well-known/http-message-signatures-directory";

/// When the test signatures are made, 1 January 2025.
pub const CREATED: i64 = 1_735_689_600;

/// When the test signatures stop being valid, which is in 2124.
pub const EXPIRES: i64 = 4_889_289_600;

/// The key directory holding the test key.
pub fn test_key_directory(purpose: Option<&str>) -> String {
    let purpose = purpose
        .map(|purpose| format!(",\"purpose\":\"{purpose}\""))
        .unwrap_or_default();
    format!(
        "{{\"keys\":[{{\"kty\":\"OKP\",\"crv\":\"Ed25519\",\"kid\":\"test-key-ed25519\",\
         \"x\":\"{ED25519_X}\"}}]{purpose}}}"
    )
}

/// What to sign, with the defaults a Web Bot Auth agent uses.
#[derive(Debug, Clone)]
pub struct Signer {
    pub label: String,
    pub host: String,
    pub cover_authority: bool,
    pub agent: Option<String>,
    pub agent_label: Option<String>,
    /// Parameters of the `Signature-Agent` member, such as `;type=cimd`,
    /// written into the header and into the signed member value alike.
    pub agent_parameters: String,
    pub created: Option<i64>,
    pub expires: Option<i64>,
    pub key_id: Option<String>,
    pub algorithm: Option<String>,
    pub nonce: Option<String>,
    pub tag: Option<String>,
    /// More components, as their identifier and their value.
    pub extra: Vec<(String, String)>,
}

impl Default for Signer {
    fn default() -> Self {
        Signer {
            label: "sig1".to_owned(),
            host: "example.com".to_owned(),
            cover_authority: true,
            agent: Some(AGENT.to_owned()),
            agent_label: Some("agent1".to_owned()),
            agent_parameters: String::new(),
            created: Some(CREATED),
            expires: Some(EXPIRES),
            key_id: Some(ED25519_THUMBPRINT.to_owned()),
            algorithm: Some("ed25519".to_owned()),
            nonce: None,
            tag: Some("web-bot-auth".to_owned()),
            extra: Vec::new(),
        }
    }
}

/// The three headers of a signed request.
#[derive(Debug, Clone)]
pub struct Signed {
    pub signature: String,
    pub signature_input: String,
    pub signature_agent: Option<String>,
}

impl Signer {
    /// Sign the request. The signature base is written out here line by
    /// line rather than with the element's own code, so a fault in how the
    /// element builds it shows as a failed test.
    pub fn sign(&self) -> Signed {
        let mut components: Vec<(String, String)> = Vec::new();
        if self.cover_authority {
            components.push(("\"@authority\"".to_owned(), self.host.to_lowercase()));
        }
        let mut signature_agent = None;
        if let Some(agent) = &self.agent {
            let member = format!("\"{agent}\"{}", self.agent_parameters);
            match &self.agent_label {
                Some(label) => {
                    signature_agent = Some(format!("{label}={member}"));
                    components.push((format!("\"signature-agent\";key=\"{label}\""), member));
                }
                None => {
                    signature_agent = Some(member.clone());
                    components.push(("\"signature-agent\"".to_owned(), member));
                }
            }
        }
        components.extend(self.extra.iter().cloned());
        let identifiers: Vec<&str> = components.iter().map(|(id, _)| id.as_str()).collect();
        let mut params = format!("({})", identifiers.join(" "));
        if let Some(created) = self.created {
            params.push_str(&format!(";created={created}"));
        }
        if let Some(expires) = self.expires {
            params.push_str(&format!(";expires={expires}"));
        }
        for (name, value) in [
            ("keyid", &self.key_id),
            ("alg", &self.algorithm),
            ("nonce", &self.nonce),
            ("tag", &self.tag),
        ] {
            if let Some(value) = value {
                params.push_str(&format!(";{name}=\"{value}\""));
            }
        }
        let mut base = String::new();
        for (identifier, value) in &components {
            base.push_str(&format!("{identifier}: {value}\n"));
        }
        base.push_str(&format!("\"@signature-params\": {params}"));
        let pair = Ed25519KeyPair::from_seed_unchecked(&ED25519_SEED).unwrap();
        let signature = STANDARD.encode(pair.sign(base.as_bytes()));
        Signed {
            signature: format!("{}=:{signature}:", self.label),
            signature_input: format!("{}={params}", self.label),
            signature_agent,
        }
    }
}

/// One response the fake client gives.
#[derive(Debug, Clone)]
pub struct FakeResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: String,
    pub delay: Duration,
    pub headers: Vec<(String, String)>,
    /// The address the client says answered, where it is not the address
    /// asked for, as a client that follows a redirect would say.
    pub final_url: Option<String>,
}

impl FakeResponse {
    /// A key directory served as the directory media type.
    pub fn directory(body: String) -> Self {
        FakeResponse {
            status: 200,
            content_type: Some(constants::DIRECTORY_MEDIA_TYPE.to_owned()),
            body,
            delay: Duration::ZERO,
            headers: Vec::new(),
            final_url: None,
        }
    }

    /// A JSON document, as an agent card or a set of keys named directly is
    /// served.
    pub fn json(body: String) -> Self {
        FakeResponse {
            content_type: Some(constants::JSON_MEDIA_TYPE.to_owned()),
            ..FakeResponse::directory(body)
        }
    }
}

/// An HTTP client that answers from memory, counts the requests it gets
/// for each address, and fails for any address it has no answer for.
#[derive(Default)]
pub struct FakeClient {
    responses: Mutex<HashMap<String, FakeResponse>>,
    calls: Mutex<HashMap<String, usize>>,
    total: AtomicUsize,
}

impl FakeClient {
    pub fn with(self, url: &str, response: FakeResponse) -> Self {
        self.responses
            .lock()
            .unwrap()
            .insert(url.to_owned(), response);
        self
    }

    pub fn calls(&self, url: &str) -> usize {
        self.calls.lock().unwrap().get(url).copied().unwrap_or(0)
    }

    pub fn total(&self) -> usize {
        self.total.load(Ordering::SeqCst)
    }
}

impl HttpClient for FakeClient {
    fn get(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
        self.total.fetch_add(1, Ordering::SeqCst);
        *self
            .calls
            .lock()
            .unwrap()
            .entry(request.url.to_owned())
            .or_insert(0) += 1;
        let response = self.responses.lock().unwrap().get(request.url).cloned();
        let Some(response) = response else {
            return Err(format!("nothing is served at {}", request.url));
        };
        thread::sleep(response.delay);
        let mut headers = response.headers;
        if let Some(content_type) = response.content_type {
            headers.push(("Content-Type".to_owned(), content_type));
        }
        Ok(HttpResponse {
            status: response.status,
            url: response.final_url.unwrap_or_else(|| request.url.to_owned()),
            headers,
            body: response.body.into_bytes(),
        })
    }
}

/// A pipeline holding only the element, built with the fake client.
pub struct Harness {
    pub client: Arc<FakeClient>,
    pub pipeline: Arc<Pipeline>,
}

impl Harness {
    /// A harness whose client serves the test key's directory.
    pub fn with_test_key() -> Self {
        Self::new(
            FakeClient::default().with(
                DIRECTORY_URL,
                FakeResponse::directory(test_key_directory(None)),
            ),
            |builder| builder,
        )
    }

    pub fn new(
        client: FakeClient,
        configure: impl FnOnce(AgentSignatureElementBuilder) -> AgentSignatureElementBuilder,
    ) -> Self {
        let client = Arc::new(client);
        let pipeline = pipeline_with(Arc::clone(&client) as Arc<dyn HttpClient>, configure);
        Harness { client, pipeline }
    }

    /// Process the evidence and return the element's data.
    pub fn process(&self, evidence: &[(&str, &str)]) -> AgentSignatureData {
        process(&self.pipeline, evidence)
    }

    /// Process a signed request arriving at `example.com` over HTTPS, with
    /// the request line given.
    pub fn process_signed(&self, signed: &Signed, extra: &[(&str, &str)]) -> AgentSignatureData {
        process_signed(&self.pipeline, signed, extra)
    }
}

/// A pipeline holding only the element, built with the client given.
pub fn pipeline_with(
    client: Arc<dyn HttpClient>,
    configure: impl FnOnce(AgentSignatureElementBuilder) -> AgentSignatureElementBuilder,
) -> Arc<Pipeline> {
    let builder = AgentSignatureElement::builder().set_http_client(client);
    let element = configure(builder).build().unwrap();
    Pipeline::builder()
        .add_element(Arc::new(element))
        .build()
        .unwrap()
}

/// Process the evidence and return the element's data.
pub fn process(pipeline: &Arc<Pipeline>, evidence: &[(&str, &str)]) -> AgentSignatureData {
    let mut builder = Evidence::builder();
    for (key, value) in evidence {
        builder = builder.add(key, *value);
    }
    let mut data = pipeline.create_flow_data_with(builder.build());
    data.process().unwrap();
    data.get(AgentSignatureElement::KEY).unwrap().clone()
}

/// Process a signed request arriving at `example.com` over HTTPS, with the
/// request line given.
pub fn process_signed(
    pipeline: &Arc<Pipeline>,
    signed: &Signed,
    extra: &[(&str, &str)],
) -> AgentSignatureData {
    let mut evidence = vec![
        ("header.signature", signed.signature.as_str()),
        ("header.signature-input", signed.signature_input.as_str()),
        ("header.host", "example.com"),
        ("header.protocol", "https"),
    ];
    if let Some(agent) = &signed.signature_agent {
        evidence.push(("header.signature-agent", agent.as_str()));
    }
    evidence.extend_from_slice(extra);
    process(pipeline, &evidence)
}

/// Join several signed requests into one request carrying every signature.
/// Each was signed under its own label, so the two headers join as ordinary
/// dictionary members, and the first `Signature-Agent` header is kept.
pub fn merge(signed: &[Signed]) -> Signed {
    let signatures: Vec<&str> = signed.iter().map(|one| one.signature.as_str()).collect();
    let inputs: Vec<&str> = signed
        .iter()
        .map(|one| one.signature_input.as_str())
        .collect();
    Signed {
        signature: signatures.join(", "),
        signature_input: inputs.join(", "),
        signature_agent: signed[0].signature_agent.clone(),
    }
}

/// The request with one bit of its signature changed, so the signature no
/// longer checks out against the key.
pub fn corrupt(signed: Signed) -> Signed {
    let (label, rest) = signed.signature.split_once("=:").unwrap();
    let mut bytes = STANDARD.decode(rest.trim_end_matches(':')).unwrap();
    bytes[0] ^= 1;
    let signature = format!("{label}=:{}:", STANDARD.encode(bytes));
    Signed {
        signature,
        ..signed
    }
}

/// The status and the reason.
pub fn outcome(data: &AgentSignatureData) -> (String, String) {
    (
        data.agent_signature().value().unwrap().clone(),
        data.agent_signature_reason().value().unwrap().clone(),
    )
}

/// The text of a property, or the message saying why it has none.
pub fn text_or_reason(
    value: &fiftyone_pipeline_engines::AspectPropertyValue<String>,
) -> Result<String, String> {
    value.value().cloned().map_err(|error| error.message)
}
