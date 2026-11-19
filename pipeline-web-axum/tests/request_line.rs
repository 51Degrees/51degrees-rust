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

//! The request line and the authority through the axum adapter, over a real
//! socket, compared with the same request given to the web crate directly.
//!
//! Each request is signed over `@method`, `@authority`, `@path`, `@query` and
//! `@target-uri` with the Ed25519 test key of RFC 9421 Appendix B.1.4, and
//! checked by the Agent Signature element, which can only verify it where the
//! adapter gives every one of those parts byte for byte. Over HTTP/2 the
//! authority arrives as `:authority`, with no Host header.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::Request;
use axum::http::Version;
use axum::routing::get;
use axum::Router;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use fiftyone_agent_signature::constants::{DIRECTORY_MEDIA_TYPE, REASON_VERIFIED, STATUS_VERIFIED};
use fiftyone_agent_signature::{
    AgentSignatureData, AgentSignatureElement, HttpClient, HttpRequest, HttpResponse,
};
use fiftyone_pipeline_core::FlowElement;
use fiftyone_pipeline_web::{build_evidence, RequestData, WebIntegrationOptions, WebPipeline};
use fiftyone_pipeline_web_axum::{register, FiftyOneResult, FiftyOneState};
use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper_util::rt::{TokioExecutor, TokioIo};
use ring::signature::Ed25519KeyPair;
use tokio::net::{TcpListener, TcpStream};

/// The private part of the Ed25519 test key of RFC 9421 Appendix B.1.4.
const ED25519_SEED: [u8; 32] = [
    0x9f, 0x83, 0x62, 0xf8, 0x7a, 0x48, 0x4a, 0x95, 0x4e, 0x6e, 0x74, 0x0c, 0x5b, 0x4c, 0x0e, 0x84,
    0x22, 0x91, 0x39, 0xa2, 0x0a, 0xa8, 0xab, 0x56, 0xff, 0x66, 0x58, 0x6f, 0x6a, 0x7d, 0x29, 0xc5,
];

/// The public part of that key, as RFC 9421 Appendix B.1.4 prints it.
const ED25519_X: &str = "JrQLj5P_89iXES9-vFgrIy29clF9CC_oPPsw3c5D0bs";

/// The thumbprint of that key, which a signature names it by.
const ED25519_THUMBPRINT: &str = "poqkLGiymh_W0uP6PZFw-dvez3QJT5SolqXBCW38r0U";

/// The agent the test requests name.
const AGENT: &str = "https://signature-agent.test";

/// Where that agent's keys are fetched from.
const DIRECTORY_URL: &str =
    "https://signature-agent.test/.well-known/http-message-signatures-directory";

/// When the test signatures are made, 1 January 2025.
const CREATED: i64 = 1_735_689_600;

/// When the test signatures stop being valid, which is in 2124.
const EXPIRES: i64 = 4_889_289_600;

/// The authority every test request is made to.
const AUTHORITY: &str = "example.com";

/// The path every request but the nested router's is made to, with an escape
/// a decoder would change.
const PATH: &str = "/a%2Fb/c";

/// The query of those requests, with an escape and a repeated name.
const QUERY: &str = "x=1&y=%20&x=2";

/// How long a request may wait for the key directory. The test client
/// answers at once, so this only matters on a slow machine.
const WAIT_BUDGET_SECONDS: u64 = 30;

/// Serves the test key's directory from memory.
struct DirectoryClient;

impl HttpClient for DirectoryClient {
    fn get(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
        if request.url != DIRECTORY_URL {
            return Err(format!("nothing is served at {}", request.url));
        }
        let body = format!(
            "{{\"keys\":[{{\"kty\":\"OKP\",\"crv\":\"Ed25519\",\
             \"kid\":\"test-key-ed25519\",\"x\":\"{ED25519_X}\"}}]}}"
        );
        Ok(HttpResponse {
            status: 200,
            url: request.url.to_owned(),
            headers: vec![("Content-Type".to_owned(), DIRECTORY_MEDIA_TYPE.to_owned())],
            body: body.into_bytes(),
        })
    }
}

/// The web pipeline with the Agent Signature element in it.
fn web_pipeline() -> WebPipeline {
    let element = AgentSignatureElement::builder()
        .set_http_client(Arc::new(DirectoryClient))
        .set_wait_budget(Duration::from_secs(WAIT_BUDGET_SECONDS))
        .build()
        .unwrap();
    let elements: Vec<Arc<dyn FlowElement>> = vec![Arc::new(element)];
    WebPipeline::build(elements, WebIntegrationOptions::default()).unwrap()
}

/// The status and the reason, as one string.
fn outcome(data: &AgentSignatureData) -> String {
    let status = data.agent_signature().value().cloned();
    let reason = data.agent_signature_reason().value().cloned();
    format!(
        "{}/{}",
        status.unwrap_or_default(),
        reason.unwrap_or_default()
    )
}

/// What an application route reads: the outcome, the protocol the request
/// arrived over and whether it carried a Host header.
async fn report(result: FiftyOneResult, request: Request) -> String {
    let answer = result
        .get(AgentSignatureElement::KEY, outcome)
        .unwrap_or_default();
    format!(
        "{answer} {:?} host:{}",
        request.version(),
        request.headers().contains_key("host")
    )
}

/// The application, with every path answered by [`report`].
fn app(web: &WebPipeline) -> Router {
    let state = FiftyOneState::from_web_pipeline(web);
    register(Router::new().fallback(report), state)
}

/// Serve the application on a loopback port, over HTTP/1.1 and HTTP/2.
async fn serve(app: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    address
}

/// The three headers of a GET request signed over the five request
/// components and the agent.
fn sign(path: &str, query: &str) -> Vec<(&'static str, String)> {
    let member = format!("\"{AGENT}\"");
    let target_uri = if query.is_empty() {
        format!("http://{AUTHORITY}{path}")
    } else {
        format!("http://{AUTHORITY}{path}?{query}")
    };
    let components = [
        ("\"@method\"", "GET".to_owned()),
        ("\"@authority\"", AUTHORITY.to_owned()),
        ("\"@path\"", path.to_owned()),
        ("\"@query\"", format!("?{query}")),
        ("\"@target-uri\"", target_uri),
        ("\"signature-agent\";key=\"agent1\"", member.clone()),
    ];
    let identifiers: Vec<&str> = components.iter().map(|(id, _)| *id).collect();
    let params = format!(
        "({});created={CREATED};expires={EXPIRES};keyid=\"{ED25519_THUMBPRINT}\";\
         alg=\"ed25519\";tag=\"web-bot-auth\"",
        identifiers.join(" ")
    );
    let mut base = String::new();
    for (identifier, value) in &components {
        base.push_str(&format!("{identifier}: {value}\n"));
    }
    base.push_str(&format!("\"@signature-params\": {params}"));
    let pair = Ed25519KeyPair::from_seed_unchecked(&ED25519_SEED).unwrap();
    let signature = STANDARD.encode(pair.sign(base.as_bytes()));
    vec![
        ("signature", format!("sig1=:{signature}:")),
        ("signature-input", format!("sig1={params}")),
        ("signature-agent", format!("agent1={member}")),
    ]
}

/// The same request given to the web crate directly, with its Host header,
/// its method and its target as the client sent them.
struct DirectRequest {
    headers: Vec<(String, String)>,
    target: String,
}

impl RequestData for DirectRequest {
    fn headers(&self) -> Vec<(String, String)> {
        self.headers.clone()
    }

    fn cookies(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    fn query_params(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    fn client_ip(&self) -> Option<String> {
        None
    }

    fn is_https(&self) -> bool {
        false
    }

    fn method(&self) -> Option<String> {
        Some("GET".to_owned())
    }

    fn request_target(&self) -> Option<String> {
        Some(self.target.clone())
    }
}

/// The outcome for the GET request given to the web crate directly.
fn direct(web: &WebPipeline, target: &str, signed: &[(&str, String)]) -> String {
    let mut headers = vec![("host".to_owned(), AUTHORITY.to_owned())];
    headers.extend(
        signed
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone())),
    );
    let request = DirectRequest {
        headers,
        target: target.to_owned(),
    };
    let pipeline = web.pipeline();
    let evidence = build_evidence(&request, pipeline.evidence_key_filter());
    let mut flow_data = pipeline.create_flow_data_with(evidence);
    let _ = flow_data.process();
    flow_data
        .get(AgentSignatureElement::KEY)
        .map(outcome)
        .unwrap_or_default()
}

/// Send a GET over HTTP/1.1, as a client does, with the target in the
/// request line and the authority in the Host header.
async fn send_http1(address: SocketAddr, target: &str, signed: &[(&str, String)]) -> String {
    let stream = TcpStream::connect(address).await.unwrap();
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(connection);
    let mut request = hyper::Request::get(target)
        .version(Version::HTTP_11)
        .header("host", AUTHORITY);
    for (name, value) in signed {
        request = request.header(*name, value);
    }
    let response = sender
        .send_request(request.body(Empty::<Bytes>::new()).unwrap())
        .await
        .unwrap();
    body_text(response).await
}

/// Send a GET over HTTP/2 with prior knowledge, as a client does, with the
/// authority in `:authority` and no Host header.
async fn send_http2(address: SocketAddr, target: &str, signed: &[(&str, String)]) -> String {
    let stream = TcpStream::connect(address).await.unwrap();
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(stream))
            .await
            .unwrap();
    tokio::spawn(connection);
    let mut request =
        hyper::Request::get(format!("http://{AUTHORITY}{target}")).version(Version::HTTP_2);
    for (name, value) in signed {
        request = request.header(*name, value);
    }
    let response = sender
        .send_request(request.body(Empty::<Bytes>::new()).unwrap())
        .await
        .unwrap();
    body_text(response).await
}

/// The response body as text.
async fn body_text(response: hyper::Response<hyper::body::Incoming>) -> String {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(body.to_vec()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_signed_request_over_http1_gives_the_same_answer_as_the_web_crate() {
    let web = web_pipeline();
    let target = format!("{PATH}?{QUERY}");
    let signed = sign(PATH, QUERY);

    let expected = direct(&web, &target, &signed);
    assert_eq!(expected, format!("{STATUS_VERIFIED}/{REASON_VERIFIED}"));

    let address = serve(app(&web)).await;
    assert_eq!(
        send_http1(address, &target, &signed).await,
        format!("{expected} HTTP/1.1 host:true")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_signed_request_over_http2_gives_the_same_answer_as_the_web_crate() {
    let web = web_pipeline();
    let target = format!("{PATH}?{QUERY}");
    let signed = sign(PATH, QUERY);

    let expected = direct(&web, &target, &signed);
    assert_eq!(expected, format!("{STATUS_VERIFIED}/{REASON_VERIFIED}"));

    // The request arrives with no Host header, so the authority the
    // signature covers can only have come from :authority.
    let address = serve(app(&web)).await;
    assert_eq!(
        send_http2(address, &target, &signed).await,
        format!("{expected} HTTP/2.0 host:false")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_nested_router_gives_the_path_the_client_sent() {
    let web = web_pipeline();
    let path = "/app/articles/rust";
    let signed = sign(path, "");

    let expected = direct(&web, path, &signed);
    assert_eq!(expected, format!("{STATUS_VERIFIED}/{REASON_VERIFIED}"));

    // The routes under /app see /articles/rust, which is not the path the
    // client signed.
    let state = FiftyOneState::from_web_pipeline(&web);
    let inner = register(Router::new().route("/articles/rust", get(report)), state);
    let address = serve(Router::new().nest("/app", inner)).await;
    for answer in [
        send_http1(address, path, &signed).await,
        send_http2(address, path, &signed).await,
    ] {
        assert!(answer.starts_with(&format!("{expected} ")), "{answer}");
    }
}
