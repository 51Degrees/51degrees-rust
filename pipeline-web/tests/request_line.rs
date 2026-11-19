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

//! The request line evidence, which the agent signature element rebuilds a
//! signed request from.

use fiftyone_pipeline_core::constants::{
    EVIDENCE_REQUEST_METHOD_KEY, EVIDENCE_REQUEST_PATH_KEY, EVIDENCE_REQUEST_QUERY_KEY,
};
use fiftyone_pipeline_core::{Evidence, EvidenceKeyFilterWhitelist};
use fiftyone_pipeline_web::{build_evidence, RequestData};

/// A request that supplies its request line where it is given one.
struct LineRequest {
    method: Option<&'static str>,
    target: Option<&'static str>,
}

impl RequestData for LineRequest {
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

    fn method(&self) -> Option<String> {
        self.method.map(str::to_owned)
    }

    fn request_target(&self) -> Option<String> {
        self.target.map(str::to_owned)
    }
}

/// The evidence for a request, with a filter asking for the request line.
fn evidence(method: Option<&'static str>, target: Option<&'static str>) -> Evidence {
    let filter = EvidenceKeyFilterWhitelist::new([
        EVIDENCE_REQUEST_METHOD_KEY,
        EVIDENCE_REQUEST_PATH_KEY,
        EVIDENCE_REQUEST_QUERY_KEY,
    ]);
    build_evidence(&LineRequest { method, target }, &filter)
}

#[test]
fn the_request_line_is_evidence_exactly_as_the_request_carried_it() {
    let evidence = evidence(Some("get"), Some("/a%2Fb/c?x=1&y=%20&x=2"));
    assert_eq!(evidence.get(EVIDENCE_REQUEST_METHOD_KEY), Some("get"));
    assert_eq!(evidence.get(EVIDENCE_REQUEST_PATH_KEY), Some("/a%2Fb/c"));
    assert_eq!(
        evidence.get(EVIDENCE_REQUEST_QUERY_KEY),
        Some("x=1&y=%20&x=2")
    );
}

#[test]
fn a_request_with_no_query_string_gives_an_empty_query() {
    let evidence = evidence(Some("GET"), Some("/"));
    assert_eq!(evidence.get(EVIDENCE_REQUEST_PATH_KEY), Some("/"));
    assert_eq!(evidence.get(EVIDENCE_REQUEST_QUERY_KEY), Some(""));
}

#[test]
fn the_request_line_is_given_whole_or_not_at_all() {
    for evidence in [
        evidence(None, Some("/")),
        evidence(Some("GET"), None),
        evidence(None, None),
    ] {
        assert!(evidence.is_empty());
    }
}

#[test]
fn the_request_line_is_left_out_where_no_element_asks_for_it() {
    let filter = EvidenceKeyFilterWhitelist::new(["header.user-agent"]);
    let request = LineRequest {
        method: Some("GET"),
        target: Some("/?a=1"),
    };
    assert!(build_evidence(&request, &filter).is_empty());
}
