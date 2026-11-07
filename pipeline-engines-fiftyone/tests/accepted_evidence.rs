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

//! The usage sharing element accepts no evidence, so the evidence a pipeline
//! accepts, and the cache keys, ETag and Vary header worked out from it, come
//! from the other elements alone. It still shares the evidence it finds.

use std::sync::{Arc, Mutex};

use fiftyone_pipeline_core::{
    Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData, Result,
};
use fiftyone_pipeline_engines_fiftyone::{
    SequenceElement, ShareUsageConfig, ShareUsageElement, UsageBatch, UsageSink,
};

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

/// An engine that accepts the User-Agent header and nothing else.
struct Engine {
    filter: EvidenceKeyFilterWhitelist,
}

impl Engine {
    fn new() -> Self {
        Engine {
            filter: EvidenceKeyFilterWhitelist::new(["header.user-agent"]),
        }
    }
}

impl FlowElement for Engine {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "engine"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

/// Keys that usage sharing shares under its default settings and that the
/// engine does not accept. `If-None-Match` is the header a browser sends to
/// revalidate, so accepting it would change the ETag of every revalidation.
const NOT_ACCEPTED: [&str; 5] = [
    "header.if-none-match",
    "header.x-test",
    "header.accept-language",
    "cookie.51d_test",
    "query.51d_test",
];

#[test]
fn the_usage_sharing_element_accepts_no_evidence() {
    let element =
        ShareUsageElement::with_sink(ShareUsageConfig::builder().build(), Keep::default());
    let filter = element.evidence_key_filter();
    for key in std::iter::once("header.user-agent").chain(NOT_ACCEPTED) {
        assert!(!filter.include(key), "{key}");
    }
}

#[test]
fn a_pipeline_with_usage_sharing_accepts_only_its_engines_evidence() {
    let share = ShareUsageElement::with_sink(ShareUsageConfig::builder().build(), Keep::default());
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(Engine::new()))
        .add_element(Arc::new(share))
        .build()
        .unwrap();
    let filter = pipeline.evidence_key_filter();
    assert!(filter.include("header.user-agent"));
    for key in NOT_ACCEPTED {
        assert!(!filter.include(key), "{key}");
    }
}

/// The Sequence element accepts the session id and the sequence itself, so a
/// pipeline with usage sharing, which accepts nothing, still takes both.
#[test]
fn a_pipeline_with_usage_sharing_still_accepts_the_session_id_and_sequence() {
    let share = ShareUsageElement::with_sink(ShareUsageConfig::builder().build(), Keep::default());
    let session_keys = ["query.session-id", "query.sequence"];
    for key in session_keys {
        assert!(!share.evidence_key_filter().include(key), "{key}");
    }
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(Engine::new()))
        .add_element(Arc::new(share))
        .build()
        .unwrap();
    let filter = pipeline.evidence_key_filter();
    for key in session_keys {
        assert!(filter.include(key), "{key}");
    }
}

#[test]
fn evidence_given_by_hand_is_shared_as_before() {
    let sink = Arc::new(Keep::default());
    let share = Arc::new(ShareUsageElement::with_sink(
        ShareUsageConfig::builder()
            .blocked_http_headers(["x-blocked"])
            .build(),
        Arc::clone(&sink),
    ));
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(Engine::new()))
        .add_element(Arc::clone(&share) as Arc<dyn FlowElement>)
        .build()
        .unwrap();
    let evidence = Evidence::builder()
        .add("header.User-Agent", "Mozilla/5.0")
        .add("header.X-Test", "test")
        .add("header.X-Blocked", "blocked")
        .add("header.Cookie", "51D_Test=kept; other=dropped")
        .add("cookie.51D_Test", "kept")
        .add("cookie.other", "dropped")
        .build();
    pipeline.create_flow_data_with(evidence).process().unwrap();

    // The element delivers what it holds when it is dropped.
    drop(pipeline);
    drop(share);
    let documents = sink.0.lock().unwrap();
    assert_eq!(documents.len(), 1, "{documents:?}");
    let document = &documents[0];
    for shared in [
        "<header Name=\"User-Agent\"><![CDATA[Mozilla/5.0]]></header>",
        "<header Name=\"X-Test\"><![CDATA[test]]></header>",
        "<cookie Name=\"51D_Test\"><![CDATA[kept]]></cookie>",
    ] {
        assert!(document.contains(shared), "{shared} in {document}");
    }
    for left_out in ["Name=\"X-Blocked\"", "Name=\"Cookie\"", "Name=\"other\""] {
        assert!(!document.contains(left_out), "{left_out} in {document}");
    }
}
