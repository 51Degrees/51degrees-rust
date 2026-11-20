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

//! Usage sharing through the axum integration. The usage sharing element
//! accepts no evidence and shares what it finds in the web request, so a
//! request to a page the middleware serves must reach it with the whole
//! request, headers no element reads included. A form parameter in the body
//! of a POST request is shared on the same terms as one in the query string.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData, Result,
};
use fiftyone_pipeline_engines_fiftyone::{
    ShareUsageConfig, ShareUsageElement, UsageBatch, UsageSink,
};
use fiftyone_pipeline_web::{WebIntegrationOptions, WebPipeline};
use fiftyone_pipeline_web_axum::{register, FiftyOneState};
use tower::ServiceExt;

/// The session id the request carries, which the shared document records.
const SESSION_ID: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

/// Keeps each document the usage sharing element delivers.
#[derive(Default)]
struct Keep(Mutex<Vec<String>>);

impl UsageSink for Keep {
    fn receive(
        &self,
        batch: UsageBatch,
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.0.lock().unwrap().push(batch.document().to_owned());
        Ok(())
    }
}

#[tokio::test]
async fn a_page_request_shares_the_headers_no_element_reads() {
    let sink = Arc::new(Keep::default());
    let share = Arc::new(ShareUsageElement::with_sink(
        ShareUsageConfig::builder().build(),
        Arc::clone(&sink),
    ));
    let web = WebPipeline::build(
        vec![Arc::clone(&share) as Arc<dyn FlowElement>],
        WebIntegrationOptions::default(),
    )
    .expect("web pipeline builds");
    let app = register(
        Router::new().route("/", get(|| async { "home" })),
        FiftyOneState::from_web_pipeline(&web),
    );

    let request = Request::builder()
        .method("GET")
        .uri(format!("/?session-id={SESSION_ID}"))
        .header("host", "localhost")
        .header("User-Agent", "Mozilla/5.0")
        .header("X-Test", "test")
        .header("Cookie", "51D_Test=kept; other=dropped")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    drop(response);

    // The element delivers what it holds when the last reference to it goes.
    drop(web);
    drop(share);
    let documents = sink.0.lock().unwrap();
    assert_eq!(documents.len(), 1, "{documents:?}");
    // The http crate gives every header name in lower case, so the document
    // is read without case.
    let document = documents[0].to_lowercase();
    let session = format!("<sessionid>{SESSION_ID}</sessionid>");
    for shared in [
        "<header name=\"user-agent\">mozilla/5.0</header>",
        "<header name=\"x-test\">test</header>",
        "<cookie name=\"51d_test\">kept</cookie>",
        session.as_str(),
    ] {
        assert!(document.contains(shared), "{shared} in {document}");
    }
    for left_out in ["name=\"cookie\"", "name=\"other\""] {
        assert!(!document.contains(left_out), "{left_out} in {document}");
    }
}

/// A Flow Element that accepts the campaign parameter and adds nothing.
struct CampaignReader {
    filter: EvidenceKeyFilterWhitelist,
}

impl FlowElement for CampaignReader {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "campaign"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

/// automated-testing.md lines 205 to 209: a parameter in the form body of a
/// POST request is shared when a Flow Element in the Pipeline accepts it and
/// not when none does, and a `51D_` parameter is always shared.
#[tokio::test]
async fn a_form_parameter_in_a_post_body_is_shared_only_when_the_pipeline_uses_it() {
    for used in [false, true] {
        let sink = Arc::new(Keep::default());
        let share = Arc::new(ShareUsageElement::with_sink(
            ShareUsageConfig::builder().build(),
            Arc::clone(&sink),
        ));
        let mut elements: Vec<Arc<dyn FlowElement>> = Vec::new();
        if used {
            elements.push(Arc::new(CampaignReader {
                filter: EvidenceKeyFilterWhitelist::new(["query.campaign"]),
            }));
        }
        elements.push(Arc::clone(&share) as Arc<dyn FlowElement>);
        let web = WebPipeline::build(elements, WebIntegrationOptions::default())
            .expect("web pipeline builds");
        let app = register(Router::new(), FiftyOneState::from_web_pipeline(&web));

        let request = Request::builder()
            .method("POST")
            .uri("/51dpipeline/json")
            .header("host", "localhost")
            .header("User-Agent", "form body agent")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(
                "51D_ScreenPixelsWidth=1080&campaign=spring&password=secret",
            ))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "campaign used {used}");
        drop(response);

        // The element delivers what it holds when the last reference to it
        // goes.
        drop(web);
        drop(share);
        let documents = sink.0.lock().unwrap();
        assert_eq!(documents.len(), 1, "campaign used {used}: {documents:?}");
        let document = &documents[0];
        assert!(
            document.contains("form body agent"),
            "campaign used {used}: {document}"
        );
        assert!(
            document.contains("Name=\"51D_ScreenPixelsWidth\">1080</query>"),
            "campaign used {used}: {document}"
        );
        assert_eq!(
            document.contains("Name=\"campaign\">spring</query>"),
            used,
            "campaign used {used}: {document}"
        );
        assert!(
            !document.contains("secret"),
            "campaign used {used}: {document}"
        );
    }
}
