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

//! @page fodid-web-creator-context-example Creator Context (51Did, Web)
//!
//! 51Did web example: the creator context and its two-step verification.
//!
//! Every 51Did the 51Degrees cloud issues carries a creator context, which
//! binds the identifier to the browser and connection it was created on. This
//! demo serves a page that runs the full flow the way production does, in
//! three steps.
//!
//! 1. Create. The browser calls the cloud `json` endpoint, which issues a
//!    51Did for the calling connection.
//! 2. Verify. The browser calls `verify-full`, so the cloud observes the
//!    browser's live connection. The answer is only an encrypted `result`
//!    that the browser can neither read nor forge, with the signature
//!    outcome and the creator context verdict sealed inside it.
//! 3. Redeem. The page hands the encrypted result to this server, which
//!    parses the 51Did, checks its signature offline against the cloud's
//!    public key for its date, then calls `redeem` with the 51Did, the
//!    encrypted result and the account's licence key, and receives the
//!    signature outcome, the true creator context verdict, when the
//!    verification happened (`verifiedAt`) and how long ago that was
//!    (`secondsSinceVerified`).
//!
//! Step 3 is the `fodid` crate's cloud client, `DidClient`, so the server
//! writes no HTTP or key handling of its own.
//!
//! The licence key lives on this server and only here. The browser never sees
//! it. A fresh single-use challenge is issued per page load and bound through
//! both verification steps by the cloud. A production server would also
//! remember the value it issued and reject a redemption carrying any other,
//! which this demo keeps out of scope.
//!
//! What a run costs. Every call the page or this server makes to the cloud is
//! one use against the subscription behind the resource key. A browser
//! checking a 51Did makes two, `verify-full` from the page and `redeem` from
//! this server, so a browser-based context check is two uses every time. The
//! offline signature check costs nothing per identifier, because the public
//! keys are fetched once and cached.
//!
//! Environment: `_51DEGREES_RESOURCE_KEY` is required (the same name without
//! the leading underscore and the CI names `_51DEGREES_RESOURCE_KEY_BESPOKE`
//! and `_51DEGREES_RESOURCE_KEY_BASIC` are read after it),
//! `_51DEGREES_LICENSE_KEY` is optional, `51DEGREES_CLOUD_ENDPOINT` overrides
//! the cloud API base and `PORT` overrides the default port of 5100.
//!
//! Run with `cargo run -p fodid-examples --bin fodid-web-creator-context` and
//! open <http://localhost:5100/>.
//!
//! @snippet fodid-web-creator-context.rs example

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use examples_web_shared::serve_css;
use fodid::FodId;
use fodid_client::{DidClient, Error as ClientError, RedeemResult, SignatureOutcome};
use serde::Deserialize;

/// The demo page, embedded so the binary is self-contained. It is byte for byte
/// the page every 51Degrees language example serves for this demo. Its relative
/// stylesheet URL is served from the shared Rust example asset.
const PAGE: &str = include_str!("../../assets/page.html");

/// The links the page and the console output end with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("51Did", "https://51degrees.com/documentation/_identifiers_51_did.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-fodid-examples-src-bin-fodid-web-creator-context.rs&utm_term=find-out-more"),
    ("51Did inspector", "https://51degrees.com/developers/51did-inspector?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-fodid-examples-src-bin-fodid-web-creator-context.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-fodid-examples-src-bin-fodid-web-creator-context.rs&utm_term=find-out-more"),
];

/// The route the demo page's relative stylesheet URL resolves to.
const PAGE_CSS_ROUTE: &str = "/examples-main.min.css";

/// The environment variable holding the optional licence key, checked first.
///
/// The leading underscore makes the name a valid shell identifier, so
/// `export _51DEGREES_LICENSE_KEY=...` works, and it is the name every
/// 51Degrees language reads.
const LICENCE_KEY_ENV_VAR: &str = "_51DEGREES_LICENSE_KEY";

/// The same name without the leading underscore, checked second. A POSIX shell
/// cannot set a name starting with a digit, so it is not the documented one.
const LICENCE_KEY_ENV_VAR_NO_UNDERSCORE: &str = "51DEGREES_LICENSE_KEY";

/// The port the demo listens on when `PORT` is not set.
const DEFAULT_PORT: u16 = 5100;

/// Options that drive [`run`].
pub struct Options {
    /// The cloud client the server redeems through, built once at start-up
    /// from the resource key, the optional licence key and the endpoint.
    pub client: DidClient,
    /// The socket address the server binds to.
    pub address: SocketAddr,
}

/// What every handler needs, carried as the router's state. The client is
/// shared because the redeem handler moves a handle to a blocking thread.
#[derive(Clone)]
struct Demo {
    client: Arc<DidClient>,
}

/// Build the demo router, ready to serve.
///
/// It is returned rather than served so the test can drive it in process with
/// `tower::ServiceExt::oneshot` while [`run`] serves it over TCP. The test
/// builds the client over an injected transport, so no cloud is called.
pub fn build_app(client: DidClient) -> Router {
    Router::new()
        .route("/", get(home))
        .route(PAGE_CSS_ROUTE, get(serve_css))
        .route("/redeem", get(redeem))
        .with_state(Demo {
            client: Arc::new(client),
        })
}

// [example]
/// Serve the demo over TCP until interrupted.
pub async fn run(options: Options) -> anyhow::Result<()> {
    let app = build_app(options.client);
    let listener = tokio::net::TcpListener::bind(options.address)
        .await
        .with_context(|| format!("binding {}", options.address))?;
    let bound = listener.local_addr().context("reading the bound address")?;
    println!(
        "51Did creator context demo listening on http://localhost:{}/",
        bound.port()
    );
    axum::serve(listener, app)
        .await
        .context("serving the application")
}
// [example]

/// The page, with a fresh single-use challenge per load. The challenge is bound
/// through both verification steps by the cloud, so a result verified under one
/// page load cannot be redeemed under another.
async fn home(State(demo): State<Demo>) -> Response {
    let mut random = [0u8; 16];
    if let Err(error) = getrandom::fill(&mut random) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("no random source for the challenge: {error}"),
        )
            .into_response();
    }
    let challenge: String = random.iter().map(|b| format!("{b:02x}")).collect();
    Html(
        page_with_find_out_more()
            .replace("__RESOURCE__", demo.client.resource_key())
            .replace("__CHALLENGE__", &challenge)
            .replace("__API__", demo.client.endpoint()),
    )
    .into_response()
}

/// [`PAGE`] with a "Find out more" section, set apart by a rule, before its
/// closing body tag. The section is added here so the page file stays the
/// same as every other language's copy.
fn page_with_find_out_more() -> String {
    let mut html = format!(
        "<div class=\"c-eg-page\">\n<hr>\n\
         <h3 class=\"c-eg-page__heading\">{}</h3>\n<ul>\n",
        examples_shared::FIND_OUT_MORE_HEADING
    );
    for (label, url) in FIND_OUT_MORE {
        let href = url.replace('&', "&amp;");
        html.push_str(&format!("<li><a href=\"{href}\">{label}</a></li>\n"));
    }
    for url in examples_shared::CLOUD_REPOSITORIES {
        html.push_str(&format!("<li><a href=\"{url}\">{url}</a></li>\n"));
    }
    html.push_str("</ul>\n</div>\n</body>");
    PAGE.replacen("</body>", &html, 1)
}

/// The three parameters the page sends to `/redeem`. Each defaults to empty
/// rather than failing the request, so a missing one is diagnosed by the
/// parse or by the cloud's own error message, which the page then shows.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RedeemQuery {
    /// The 51Did itself, in the URL-safe alphabet the page's links use. The
    /// parameter is named `51did` on the wire, which is not a legal field
    /// name, so it is renamed here.
    #[serde(rename = "51did")]
    fodid: String,
    result: String,
    challenge: String,
}

/// The server-side step. The client's futures are not Send, because its
/// transport is written for hosts without threads, and axum needs a handler
/// whose future is, so the calls run on a blocking thread with a runtime of
/// their own.
async fn redeem(State(demo): State<Demo>, Query(query): Query<RedeemQuery>) -> Response {
    let client = Arc::clone(&demo.client);
    let redeemed = tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread runtime starts");
        runtime.block_on(redeem_with(&client, &query))
    })
    .await;
    redeemed.unwrap_or_else(|error| {
        errors_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("the redeem step did not complete: {error}"),
        )
    })
}

/// The lines a developer copies into their own server. The licence key is
/// held by the client and only there, so the browser never sees it, and the
/// page receives the cloud's status and a JSON body in the cloud's own shape
/// (`signature`, `context`, `factors` when present, `verifiedAt`,
/// `secondsSinceVerified`) plus one extra field, `serverSignature`, with the
/// outcome of this server's own offline signature check.
async fn redeem_with(client: &DidClient, query: &RedeemQuery) -> Response {
    // 1. Parse. The page sends the URL-safe alphabet, which is accepted.
    let fod_id = match FodId::from_base64(&query.fodid) {
        Ok(fod_id) => fod_id,
        Err(error) => {
            return errors_response(
                StatusCode::BAD_REQUEST,
                &format!("Value for 51did is not a valid 51Did: {error}"),
            );
        }
    };
    // 2. Check the signature here, against the cloud's public key for the
    //    identifier's date. The keys are fetched once and cached.
    let server_signature = match client.verify_signature(&fod_id).await {
        Ok(true) => "verified",
        Ok(false) => "invalid",
        Err(error) => return client_error(error),
    };
    // 3. Redeem the sealed result with the licence key and pass the typed
    //    verdict to the page.
    match client
        .redeem(&fod_id, &query.result, Some(query.challenge.as_str()))
        .await
    {
        Ok(result) => redeem_response(&result, server_signature),
        Err(error) => client_error(error),
    }
}

/// The cloud's answer in its own shape, built from the typed result, with
/// `serverSignature` added. A field the cloud did not send is not invented.
fn redeem_response(result: &RedeemResult, server_signature: &str) -> Response {
    let mut body = serde_json::Map::new();
    match result.signature() {
        SignatureOutcome::Verified => {
            body.insert("signature".to_owned(), "verified".into());
        }
        SignatureOutcome::Invalid => {
            body.insert("signature".to_owned(), "invalid".into());
        }
        // The cloud sent no signature, and one is not invented.
        SignatureOutcome::Unknown => {}
    }
    body.insert(
        "context".to_owned(),
        result.context_value().map(|v| v.to_owned()).into(),
    );
    if let Some(factors) = result.factors() {
        let factors: serde_json::Map<String, serde_json::Value> = factors
            .iter()
            .map(|(name, outcome)| (name.clone(), outcome.as_cloud().into()))
            .collect();
        body.insert("factors".to_owned(), factors.into());
    }
    if let Some(verified_at) = result.verified_at() {
        body.insert(
            "verifiedAt".to_owned(),
            verified_at.format("%Y-%m-%dT%H:%M:%SZ").to_string().into(),
        );
    }
    if let Some(seconds) = result.seconds_since_verified() {
        body.insert("secondsSinceVerified".to_owned(), seconds.into());
    }
    body.insert("serverSignature".to_owned(), server_signature.into());
    json_response(
        StatusCode::from_u16(result.status()).unwrap_or(StatusCode::OK),
        serde_json::Value::Object(body),
    )
}

/// The error paths, as the page sees them. A host without the creator
/// context answers 404 with a text body, which the page reports as not
/// supported by this host. A 51Did the cloud refused answers 400 with the
/// cloud's `errors`. Any other status the cloud sent is relayed with its
/// body, and a cloud that could not be reached answers 502 with a JSON
/// error naming the fault.
fn client_error(error: ClientError) -> Response {
    match error {
        // The host does not offer the creator context at all.
        ClientError::NotSupported(_) => (
            StatusCode::NOT_FOUND,
            "The service does not offer the creator context.",
        )
            .into_response(),
        // Something the caller sent was not usable, which is the page's
        // fault rather than the cloud's, so it reads as a bad request.
        ClientError::InvalidArgument(message) => errors_response(StatusCode::BAD_REQUEST, &message),
        // Any other status the cloud sent, relayed with its own body.
        ClientError::UnexpectedStatus { status, body, .. } => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
            let content_type = if body.trim_start().starts_with(['{', '[']) {
                "application/json"
            } else {
                "text/plain; charset=utf-8"
            };
            Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(body))
                .unwrap_or_else(|error| gateway_error(&error.to_string()))
        }
        other => gateway_error(&other.to_string()),
    }
}

/// A JSON body in the cloud's `errors` shape.
fn errors_response(status: StatusCode, message: &str) -> Response {
    json_response(status, serde_json::json!({ "errors": [message] }))
}

/// A 502 with a JSON body naming the fault, for when the cloud could not be
/// reached or its answer could not be read. The page shows the message.
fn gateway_error(message: &str) -> Response {
    json_response(
        StatusCode::BAD_GATEWAY,
        serde_json::json!({ "error": message }),
    )
}

fn json_response(status: StatusCode, body: serde_json::Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // The resource key is read from the aligned/legacy/CI environment
    // variables. Without one there is nothing to demonstrate, so say which
    // variables were looked at and stop.
    let Some(resource_key) = examples_shared::resource_key_from_env() else {
        eprintln!(
            "Set _51DEGREES_RESOURCE_KEY (or _51DEGREES_RESOURCE_KEY_BESPOKE or \
             _51DEGREES_RESOURCE_KEY_BASIC) to a resource key from \
             https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-fodid-examples-src-bin-fodid-web-creator-context.rs&utm_term=resource-key-required \
             and run again."
        );
        examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::CLOUD_REPOSITORIES);
        std::process::exit(1);
    };

    let licence_key = [LICENCE_KEY_ENV_VAR, LICENCE_KEY_ENV_VAR_NO_UNDERSCORE]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    if licence_key.is_empty() {
        // Only an account that holds licence keys needs one to redeem,
        // because the licence key is what keeps redemption to the acting
        // party's own servers. An account holding none has nothing to
        // check against, so the demo runs without it. Saying so here
        // means an account that DOES hold licence keys, run without one,
        // is diagnosed at start-up rather than by an unreadable verdict
        // three steps later that looks like a cryptographic failure.
        println!(
            "No {LICENCE_KEY_ENV_VAR} set. Redemption will work where the \
             account holds no licence keys, and will report the context \
             unreadable where it holds some."
        );
    }

    // One client for the life of the server. The endpoint is the same
    // variable the cloud request engine honours, so a developer who has set
    // it once points every 51Degrees example at the same place, and the
    // client normalises a trailing slash.
    let mut builder = DidClient::builder(resource_key).licence_key(licence_key);
    if let Some(endpoint) = examples_shared::cloud_endpoint_from_env() {
        builder = builder.endpoint(endpoint);
    }
    // build() answers a Result now, and a demo that cannot construct its
    // client has nothing to show, so it says why and stops.
    let client = builder.build().context("building the 51Did client")?;

    // A fixed local port for the runnable binary, which PORT overrides when
    // that port is taken.
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    // Every interface, not only loopback, so the copy-and-paste proof can be
    // opened from a second device on the same network, as the page explains.
    let address = SocketAddr::from(([0, 0, 0, 0], port));

    run(Options { client, address }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::{HeaderValue, Request};
    use fodid_client::{DidHttpClient, DidHttpRequest, DidHttpResponse, LocalBoxFuture};
    use fodid::{Creator, Crypto};
    use tower::ServiceExt;

    const RESOURCE_KEY: &str = "resource-key-placeholder";

    /// A cloud that answers the key list with one key in force since 2020,
    /// so it covers any identifier the test signs, and answers redeem with
    /// whatever the test scripted. Nothing else is offered.
    struct FakeCloud {
        public_key_pem: String,
        redeem_status: u16,
        redeem_body: &'static str,
    }

    impl DidHttpClient for FakeCloud {
        fn send<'a>(
            &'a self,
            request: &'a DidHttpRequest,
        ) -> LocalBoxFuture<'a, Result<DidHttpResponse, String>> {
            Box::pin(async move {
                if request.url.contains("/id/key") {
                    let body = serde_json::json!([{
                        "startsAt": "2020-01-01T00:00:00Z",
                        "publicKey": self.public_key_pem,
                    }]);
                    return Ok(DidHttpResponse {
                        status: 200,
                        body: body.to_string(),
                    });
                }
                if request.url.contains("/id/redeem") {
                    return Ok(DidHttpResponse {
                        status: self.redeem_status,
                        body: self.redeem_body.to_owned(),
                    });
                }
                Err(format!("unexpected request to {}", request.url))
            })
        }
    }

    /// A signed 51Did, as the cloud would issue one, with the key that
    /// signed it. The payload is the five byte header and the 32 byte match
    /// key, with usage bit 0 set, which is the least a 51Did states.
    fn signed_51did() -> (FodId, Crypto) {
        let crypto = Crypto::new();
        let creator = Creator::new("51degrees.com", crypto.clone()).unwrap();
        let mut payload = vec![0u8; 5 + 32];
        payload[0] = 0b0000_0001;
        for (i, b) in payload[5..].iter_mut().enumerate() {
            *b = 0x20 + i as u8;
        }
        let owid = creator.create(payload).unwrap();
        (FodId::from_owid(owid).unwrap(), crypto)
    }

    fn app_over(cloud: FakeCloud) -> Router {
        build_app(
            DidClient::builder(RESOURCE_KEY)
                .endpoint("http://cloud.example/api/v4/")
                .licence_key("licence-key-placeholder")
                .http_client(Arc::new(cloud))
                .build()
                .expect("the client builds"),
        )
    }

    fn app_with_redeem(crypto: &Crypto, status: u16, body: &'static str) -> Router {
        app_over(FakeCloud {
            public_key_pem: crypto.public_key_pem().unwrap(),
            redeem_status: status,
            redeem_body: body,
        })
    }

    /// Drive one GET request through a clone of the app and return the response.
    async fn get(app: &Router, uri: &str) -> Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// Read a response body to an owned UTF-8 string.
    async fn body_string(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    async fn body_json(response: Response) -> serde_json::Value {
        let body = body_string(response).await;
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("a JSON body, got: {body}"))
    }

    /// The challenge the page carries, read back from its script.
    fn challenge_of(page: &str) -> String {
        let marker = "var CHALLENGE = \"";
        let start = page.find(marker).unwrap() + marker.len();
        let end = page[start..].find('"').unwrap();
        page[start..start + end].to_owned()
    }

    fn redeem_uri(fod_id: &FodId) -> String {
        // The page sends the URL-safe alphabet.
        format!(
            "/redeem?51did={}&result=sealed&challenge=abc",
            fod_id.as_base64_url().unwrap()
        )
    }

    #[tokio::test]
    async fn page_is_filled_in_with_a_fresh_challenge_per_load() {
        // No cloud call is made to serve the page, so placeholder values are
        // enough to check the substitutions. The client normalises the
        // endpoint, so a value without the trailing slash reaches the page
        // with one.
        let app = build_app(
            DidClient::builder(RESOURCE_KEY)
                .endpoint("http://cloud.example/api/v4")
                .http_client(Arc::new(FakeCloud {
                    public_key_pem: String::new(),
                    redeem_status: 500,
                    redeem_body: "",
                }))
                .build()
                .expect("the client builds"),
        );

        let response = get(&app, "/").await;
        assert_eq!(response.status(), StatusCode::OK);
        let page = body_string(response).await;
        for placeholder in ["__API__", "__RESOURCE__", "__CHALLENGE__"] {
            assert!(
                !page.contains(placeholder),
                "{placeholder} was not substituted"
            );
        }
        assert!(page.contains("var API = \"http://cloud.example/api/v4/\""));
        assert!(page.contains("var RESOURCE = \"resource-key-placeholder\""));
        assert!(page.contains("href=\"examples-main.min.css\""));
        // The page ends with the "Find out more" section.
        let section = page
            .find(examples_shared::FIND_OUT_MORE_HEADING)
            .expect("the page has a Find out more section");
        assert!(section < page.find("</body>").unwrap());

        // 16 random bytes as 32 lowercase hex characters, different each load.
        let first = challenge_of(&page);
        assert_eq!(first.len(), 32);
        assert!(first
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        let second = challenge_of(&body_string(get(&app, "/").await).await);
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn redeem_answers_in_the_clouds_shape_with_the_server_signature_added() {
        let (fod_id, crypto) = signed_51did();
        let app = app_with_redeem(
            &crypto,
            200,
            r#"{"signature":"verified","context":"mismatch","factors":{"transport":"verified","device":"mismatch","browserip":"verified","connectionip":"verified","asn":"verified","browser":"mismatch"},"verifiedAt":"2026-08-12T12:00:30Z","secondsSinceVerified":2}"#,
        );
        let response = get(&app, &redeem_uri(&fod_id)).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json"))
        );
        let json = body_json(response).await;
        assert_eq!(json["signature"], "verified");
        assert_eq!(json["context"], "mismatch");
        assert_eq!(json["factors"]["device"], "mismatch");
        assert_eq!(json["factors"]["asn"], "verified");
        assert_eq!(json["factors"].as_object().unwrap().len(), 6);
        assert_eq!(json["verifiedAt"], "2026-08-12T12:00:30Z");
        assert_eq!(json["secondsSinceVerified"], 2);
        assert_eq!(json["serverSignature"], "verified");
    }

    #[tokio::test]
    async fn redeem_reports_an_invalid_signature_from_the_servers_own_check() {
        // The identifier is signed by a key the cloud never published, so the
        // offline check fails while the cloud's sealed answer is relayed as
        // it is.
        let (fod_id, _) = signed_51did();
        let (_, other) = signed_51did();
        let app = app_with_redeem(&other, 200, r#"{"context":"replayed"}"#);
        let response = get(&app, &redeem_uri(&fod_id)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let json = body_json(response).await;
        assert_eq!(json["context"], "replayed");
        assert_eq!(json["serverSignature"], "invalid");
        // Fields the cloud did not send are not invented.
        assert!(json.get("signature").is_none());
        assert!(json.get("factors").is_none());
        assert!(json.get("verifiedAt").is_none());
    }

    #[tokio::test]
    async fn redeem_relays_503_unconfirmed() {
        let (fod_id, crypto) = signed_51did();
        let app = app_with_redeem(&crypto, 503, r#"{"context":"unconfirmed"}"#);
        let response = get(&app, &redeem_uri(&fod_id)).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let json = body_json(response).await;
        assert_eq!(json["context"], "unconfirmed");
        assert_eq!(json["serverSignature"], "verified");
    }

    #[tokio::test]
    async fn host_without_the_creator_context_answers_404() {
        let (fod_id, crypto) = signed_51did();
        let app = app_with_redeem(&crypto, 404, "");
        let response = get(&app, &redeem_uri(&fod_id)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = body_string(response).await;
        assert!(body.contains("does not offer the creator context"));
    }

    #[tokio::test]
    async fn malformed_51did_answers_400_with_the_errors_shape() {
        let (_, crypto) = signed_51did();
        let app = app_with_redeem(&crypto, 200, r#"{"context":"verified"}"#);
        let response = get(&app, "/redeem?51did=x&result=y&challenge=z").await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let json = body_json(response).await;
        assert!(json["errors"][0]
            .as_str()
            .is_some_and(|m| m.contains("not a valid 51Did")));
    }

    #[tokio::test]
    async fn unreachable_cloud_answers_502_with_a_json_error() {
        // Port 9 is the discard service, which nothing listens on here, so
        // the key fetch fails to connect rather than reaching any cloud.
        let (fod_id, _) = signed_51did();
        let app = build_app(
            DidClient::builder(RESOURCE_KEY)
                .endpoint("http://127.0.0.1:9/api/v4/")
                .build()
                .expect("the client builds"),
        );
        let response = get(&app, &redeem_uri(&fod_id)).await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json"))
        );
        let body = body_string(response).await;
        let json: serde_json::Value = serde_json::from_str(&body).expect("a JSON body");
        assert!(
            json["error"].as_str().is_some_and(|m| !m.is_empty()),
            "the error names the fault, got: {body}"
        );
    }

    #[tokio::test]
    async fn page_stylesheet_is_served() {
        let (_, crypto) = signed_51did();
        let app = app_with_redeem(&crypto, 200, "");
        let css = get(&app, PAGE_CSS_ROUTE).await;
        assert_eq!(css.status(), StatusCode::OK);
        assert_eq!(
            css.headers().get(header::CONTENT_TYPE),
            Some(&HeaderValue::from_static("text/css"))
        );
    }
}

/*
 * @example fodid-web-creator-context.rs
 *
 * 51Did creator context web example (browser plus server side).
 *
 * This example demonstrates the creator context every 51Did carries and its
 * two-step verification, inside a small axum web server. It shows three
 * things working together:
 *
 *   1. Creation in the browser. The page calls the cloud `json` endpoint,
 *      which issues a 51Did for the browser's own connection and returns
 *      every kind of identifier at once, shown in a table.
 *
 *   2. Verification in the browser. The page calls `verify-full`, so the
 *      cloud observes the browser's live connection. The answer is only an
 *      encrypted result the browser cannot read or forge, with the signature
 *      outcome and the creator context verdict sealed inside it.
 *
 *   3. Redemption on this server. The page hands the encrypted result to
 *      `/redeem`, and this server uses the `fodid` crate's cloud client to
 *      parse the 51Did, check its signature offline, and call the cloud
 *      `redeem` endpoint with the account's licence key, which the browser
 *      never sees, returning the signature outcome and the true context
 *      verdict to the page.
 *
 * Once the 51Did has fully validated, the page offers a link carrying the
 * same identifier. Opened in a different browser, the signature still
 * verifies because the identifier is genuine, but the creator context does
 * not validate, because the context binds the identifier to the browser and
 * connection it was created on. That visible failure is the demonstration, a
 * copied or stolen identifier caught at presentation with nothing stored
 * server side.
 *
 * The page is written to the shared 51Degrees example design system. It
 * references the vendored `examples-main.min.css`, embedded in the
 * `examples-web-shared` crate and served at the page's relative URL. It uses
 * the `.c-eg-*` class contract. The server adds a "Find out more" section
 * before the page's closing body tag, so the page file stays the same as
 * every other language's copy.
 *
 * Build a resource key at https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-fodid-examples-src-bin-fodid-web-creator-context.rs&utm_term=fodid-web-creator-context and export it as
 * `_51DEGREES_RESOURCE_KEY`, then run the binary and open http://localhost:5100/.
 */
