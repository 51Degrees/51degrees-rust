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

//! The `Vary` header of a web pipeline whose cloud request engine could not
//! reach the cloud service when it was built.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_pipeline_core::{Evidence, EvidenceKeyFilter, FlowElement};
use fiftyone_pipeline_web::{serve_json, RequestData, WebIntegrationOptions, WebPipeline};

/// A transport whose evidence keys request fails until `reachable` is set.
#[derive(Default)]
struct Cloud {
    reachable: AtomicBool,
}

impl CloudHttpClient for Cloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let ok = |body: &str| {
            Ok(CloudHttpResponse {
                status: 200,
                body: body.to_owned(),
                retry_after: None,
            })
        };
        if request.url.contains("evidencekeys") {
            if self.reachable.load(Ordering::SeqCst) {
                ok(r#"["header.user-agent","header.sec-ch-ua"]"#)
            } else {
                Err("connection refused".to_owned())
            }
        } else if request.url.contains("accessibleproperties") {
            ok(r#"{"Products":{}}"#)
        } else {
            ok(r#"{"device":{"ismobile":true}}"#)
        }
    }
}

struct Request;

impl RequestData for Request {
    fn headers(&self) -> Vec<(String, String)> {
        Vec::new()
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
        true
    }
}

fn names(vary: Option<&str>) -> Vec<String> {
    vary.map(|vary| vary.split(',').map(str::to_owned).collect())
        .unwrap_or_default()
}

#[test]
fn vary_gains_the_cloud_headers_once_discovery_recovers() {
    let cloud = Arc::new(Cloud::default());
    let engine = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .endpoint("https://cloud.example.test/api/v4/")
        .http_client(cloud.clone())
        .build()
        .expect("an unreachable cloud does not fail the build");
    assert!(!engine.is_evidence_key_filter_complete());
    let web = WebPipeline::build(vec![Arc::new(engine)], WebIntegrationOptions::default())
        .expect("web pipeline builds");
    let options = web.endpoint_options();
    let serve = || {
        let evidence = Evidence::builder().add("header.user-agent", "UA").build();
        let mut data = web.pipeline().create_flow_data_with(evidence);
        data.process().expect("process errors are suppressed");
        serve_json(&data, &Request, &options)
    };

    let failed = serve();
    assert_eq!(failed.header("Cache-Control"), Some("no-cache"));
    let before = names(failed.header("Vary"));
    assert!(before.iter().any(|h| h == "host"), "{before:?}");
    assert!(!before.iter().any(|h| h == "user-agent"), "{before:?}");

    cloud.reachable.store(true, Ordering::SeqCst);
    let answered = serve();
    assert_eq!(
        answered.header("Cache-Control"),
        Some("private, max-age=1800")
    );
    let after = names(answered.header("Vary"));
    assert!(after.iter().any(|h| h == "user-agent"), "{after:?}");
    assert!(after.iter().any(|h| h == "sec-ch-ua"), "{after:?}");
    assert!(after.iter().any(|h| h == "host"), "{after:?}");
    assert!(web.vary_whitelist().include("header.sec-ch-ua"));
}
