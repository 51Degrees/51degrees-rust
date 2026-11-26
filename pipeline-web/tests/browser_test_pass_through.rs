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

//! The server route of the browser tests. A server runs a web pipeline with
//! a cloud request engine, serves the page the script the JavaScript builder
//! writes, and takes the browser's answers back on the JSON endpoint. Each
//! iteration must reach the cloud with the session id, the sequence and the
//! browser's answers, and a server must be able to give the visitor's IP
//! address, JA4 and HTTP/2 settings as query parameters.
//!
//! The browser test answers arrive under a `51D_` name that changes with
//! each script, so the service cannot list it among its evidence keys. These
//! tests answer the discovery request with the list the service gives today,
//! which names none of them.

use std::sync::{Arc, Mutex};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_pipeline_core::FlowData;
use fiftyone_pipeline_web::{
    build_evidence, serve_javascript, RequestData, WebIntegrationOptions, WebPipeline,
};

/// The evidence keys the service lists today that these tests rely on. The
/// session id, the sequence and the browser test answers are not among them.
const EVIDENCE_KEYS: &str = r#"["header.user-agent","query.user-agent","server.client-ip",
    "query.client-ip","header.client-ip","query.ja4","header.ja4","server.ja4",
    "query.h2","header.h2","server.h2","query.humanconfidence","query.transaction"]"#;

/// A scrambled script's answer name, as a browser sends it back.
const ANSWER_NAME: &str = "51D_q7Hx2b";

/// The answers a browser sends, in the compressed form.
const ANSWERS: &str = "3z|eJzLSM3JyQcABiwCFQ";

/// A transport that records each data request and answers with an empty
/// device.
#[derive(Default)]
struct Cloud {
    sent: Mutex<Vec<CloudHttpRequest>>,
}

impl Cloud {
    /// The form of the last data request.
    fn last_form(&self) -> Vec<(String, String)> {
        self.sent
            .lock()
            .unwrap()
            .last()
            .expect("a data request was sent")
            .form
            .clone()
    }
}

impl CloudHttpClient for Cloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let body = if request.url.contains("evidencekeys") {
            EVIDENCE_KEYS
        } else if request.url.contains("accessibleproperties") {
            r#"{"Products":{}}"#
        } else {
            self.sent.lock().unwrap().push(request.clone());
            r#"{"device":{}}"#
        };
        Ok(CloudHttpResponse {
            status: 200,
            body: body.to_owned(),
            retry_after: None,
        })
    }
}

/// A web request as an adapter gives it.
#[derive(Default)]
struct Request {
    cookies: Vec<(String, String)>,
    query: Vec<(String, String)>,
    form: Vec<(String, String)>,
}

impl RequestData for Request {
    fn headers(&self) -> Vec<(String, String)> {
        vec![
            ("User-Agent".to_owned(), "Mozilla/5.0".to_owned()),
            ("Host".to_owned(), "publisher.example.test".to_owned()),
        ]
    }
    fn cookies(&self) -> Vec<(String, String)> {
        self.cookies.clone()
    }
    fn query_params(&self) -> Vec<(String, String)> {
        self.query.clone()
    }
    fn form_params(&self) -> Vec<(String, String)> {
        self.form.clone()
    }
    fn client_ip(&self) -> Option<String> {
        // The connection the publisher's server terminates.
        Some("10.0.0.9".to_owned())
    }
    fn is_https(&self) -> bool {
        true
    }
}

fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
    values
        .iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect()
}

/// The value of a form field, or `None` when the form has none.
fn field<'a>(form: &'a [(String, String)], name: &str) -> Option<&'a str> {
    form.iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn web_pipeline(cloud: Arc<Cloud>) -> WebPipeline {
    let engine = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .license_key("test-license-key")
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(cloud)
        .build()
        .expect("engine builds");
    WebPipeline::build(vec![Arc::new(engine)], WebIntegrationOptions::default())
        .expect("web pipeline builds")
}

/// Process one request through the web pipeline as an adapter does.
fn process(web: &WebPipeline, request: &Request) -> FlowData {
    let evidence = build_evidence(request, web.pipeline().evidence_key_filter());
    let mut data = web.pipeline().create_flow_data_with(evidence);
    data.process().expect("process errors are suppressed");
    data
}

#[test]
fn every_iteration_reaches_the_cloud_with_the_session_sequence_and_answers() {
    let cloud = Arc::new(Cloud::default());
    let web = web_pipeline(cloud.clone());

    // The page. The server supplies the visitor's address, JA4 and HTTP/2
    // settings as query parameters.
    let page = Request {
        query: pairs(&[
            ("client-ip", "203.0.113.7"),
            ("ja4", "t13d1516h2_8daaf6152771_02713d6af862"),
            ("h2", "1:65536;2:0;4:6291456;6:262144|15663105|0|m,a,s,p"),
        ]),
        ..Request::default()
    };
    let data = process(&web, &page);
    let first = cloud.last_form();
    let session_id = field(&first, "session-id")
        .expect("the first iteration sends a session id")
        .to_owned();
    assert!(!session_id.is_empty());
    assert_eq!(field(&first, "sequence"), Some("1"));
    assert_eq!(field(&first, "client-ip"), Some("203.0.113.7"));
    assert_eq!(
        field(&first, "ja4"),
        Some("t13d1516h2_8daaf6152771_02713d6af862")
    );
    assert_eq!(
        field(&first, "h2"),
        Some("1:65536;2:0;4:6291456;6:262144|15663105|0|m,a,s,p")
    );
    assert_eq!(field(&first, "license"), Some("test-license-key"));

    // The script the page is given carries the session id and the sequence,
    // and calls back to this server.
    let script = serve_javascript(&data, &page, &web.endpoint_options());
    let script = script.body_str().expect("a script body");
    // The script is minified, so only the values are looked for.
    assert!(
        script.contains(&session_id),
        "the script carries the session id"
    );
    assert!(
        script.contains("sequence:1"),
        "the script carries the sequence"
    );

    // The browser's answers come back on the JSON endpoint, in the form body
    // the script posts and in the cookie the browser tests write, with the
    // session id and sequence the script was given.
    let callback = Request {
        cookies: pairs(&[(ANSWER_NAME, ANSWERS), ("51D_BrowserTests", ANSWERS)]),
        query: pairs(&[
            ("client-ip", "203.0.113.7"),
            ("ja4", "t13d1516h2_8daaf6152771_02713d6af862"),
            ("h2", "1:65536;2:0;4:6291456;6:262144|15663105|0|m,a,s,p"),
        ]),
        form: pairs(&[
            (ANSWER_NAME, ANSWERS),
            ("session-id", &session_id),
            ("sequence", "1"),
        ]),
    };
    process(&web, &callback);
    let second = cloud.last_form();
    assert_eq!(
        field(&second, "session-id"),
        Some(session_id.as_str()),
        "the same session id on the next iteration"
    );
    assert_eq!(field(&second, "sequence"), Some("2"));
    assert_eq!(
        field(&second, ANSWER_NAME),
        Some(ANSWERS),
        "the browser's answers reach the cloud: {second:?}"
    );
    assert_eq!(
        field(&second, "51D_BrowserTests"),
        Some(ANSWERS),
        "the answers cookie reaches the cloud: {second:?}"
    );
    assert_eq!(field(&second, "client-ip"), Some("203.0.113.7"));
    assert_eq!(
        field(&second, "ja4"),
        Some("t13d1516h2_8daaf6152771_02713d6af862")
    );
}

#[test]
fn a_name_that_only_starts_like_an_answer_is_not_sent() {
    let cloud = Arc::new(Cloud::default());
    let web = web_pipeline(cloud.clone());
    let request = Request {
        cookies: pairs(&[("tracking", "1")]),
        form: pairs(&[("other", "value")]),
        ..Request::default()
    };
    process(&web, &request);
    let form = cloud.last_form();
    assert_eq!(field(&form, "tracking"), None, "{form:?}");
    assert_eq!(field(&form, "other"), None, "{form:?}");
}
