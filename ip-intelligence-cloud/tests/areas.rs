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

//! What `areas()` gives for a cloud response.
//!
//! The cloud sends `Areas` as an object holding its WKT text under `value`,
//! and the engine also reads it from a bare string. Either way the data holds
//! a WKT value, and `areas()` gives its text as a `String`.

use std::sync::Arc;

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_ip_intelligence_cloud::{
    IpIntelligenceCloudEngine, IpIntelligenceData, WktString, IP_DATA_KEY,
};
use fiftyone_pipeline_core::{ElementData, Evidence, Pipeline, PropertyValue};

/// The properties the resource key offers.
const ACCESSIBLE_PROPERTIES: &str = r#"{
    "Products": {
        "ip": {
            "DataTier": "CloudV4",
            "Properties": [
                { "Name": "Areas", "Type": "WktString", "Category": "Location" }
            ]
        }
    }
}"#;

/// The evidence keys the request engine fetches.
const EVIDENCE_KEYS: &str = r#"["query.client-ip"]"#;

/// The WKT text of the area in each response.
const AREA: &str = "POLYGON((-0.976 51.459,-0.968 51.459,-0.968 51.452,-0.976 51.459))";

/// Answers each cloud endpoint, with `response` for the IP data.
struct FakeCloud {
    response: String,
}

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let body = if request.url.contains("accessibleproperties") {
            ACCESSIBLE_PROPERTIES
        } else if request.url.contains("evidencekeys") {
            EVIDENCE_KEYS
        } else {
            &self.response
        };
        Ok(CloudHttpResponse {
            status: 200,
            body: body.to_owned(),
            retry_after: None,
        })
    }
}

#[test]
fn areas_gives_the_text_of_the_wkt_value_in_a_cloud_response() {
    for response in [
        format!(r#"{{ "ip": {{ "areas": {{ "value": "{AREA}" }} }} }}"#),
        format!(r#"{{ "ip": {{ "areas": "{AREA}" }} }}"#),
    ] {
        let request_engine = Arc::new(
            CloudRequestEngine::builder()
                .resource_key("test-resource-key")
                .http_client(Arc::new(FakeCloud {
                    response: response.clone(),
                }))
                .build()
                .expect("request engine builds"),
        );
        let ip_engine = IpIntelligenceCloudEngine::builder()
            .cloud_request_engine(Arc::clone(&request_engine))
            .build()
            .expect("IP Intelligence engine builds");
        let pipeline = Pipeline::builder()
            .add_element(request_engine)
            .add_element(Arc::new(ip_engine))
            .build()
            .expect("pipeline builds");
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("query.client-ip", "192.0.2.1")
                .build(),
        );
        data.process().expect("processing succeeds");
        let ip = data.get(IP_DATA_KEY).expect("the IP data is there");

        let wkt = ip.wkt_string("Areas");
        let wkt = wkt.value().expect("the response gives an area");
        assert_eq!(wkt.as_str(), AREA, "{response}");
        assert_eq!(
            ip.areas().value().expect("areas() gives the area"),
            wkt.as_str(),
            "{response}"
        );
        assert_eq!(
            ip.get("Areas").expect("the bag holds the area"),
            PropertyValue::WktString(WktString::new(AREA)),
            "{response}"
        );
    }
}
