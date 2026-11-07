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

//! The IP Intelligence cloud engine keeps the category the cloud's property
//! list gives each property, on its property and aspect property metadata,
//! once the cloud request engine's discovery has completed.

use std::sync::Arc;

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_ip_intelligence_cloud::IpIntelligenceCloudEngine;
use fiftyone_pipeline_core::FlowElement;
use fiftyone_pipeline_engines::AspectEngine;

/// The properties the resource key offers, two with a category and one
/// without.
const ACCESSIBLE_PROPERTIES: &str = r#"{
    "Products": {
        "ip": {
            "DataTier": "CloudV4",
            "Properties": [
                { "Name": "Town", "Type": "String", "Category": "Location" },
                {
                    "Name": "CountryCodesGeographical",
                    "Type": "WeightedString",
                    "Category": "Location"
                },
                { "Name": "RegisteredName", "Type": "String", "Category": "Network" },
                { "Name": "IpRangeStart", "Type": "String" }
            ]
        }
    }
}"#;

/// The evidence keys the request engine fetches.
const EVIDENCE_KEYS: &str = r#"["query.client-ip"]"#;

/// Answers the two discovery endpoints. Nothing here processes a request.
struct FakeCloud;

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let body = if request.url.contains("accessibleproperties") {
            ACCESSIBLE_PROPERTIES
        } else if request.url.contains("evidencekeys") {
            EVIDENCE_KEYS
        } else {
            return Err("only discovery is answered".to_owned());
        };
        Ok(CloudHttpResponse {
            status: 200,
            body: body.to_owned(),
            retry_after: None,
        })
    }
}

#[test]
fn each_property_keeps_the_category_the_list_gives_it() {
    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .http_client(Arc::new(FakeCloud))
            .build()
            .expect("request engine builds"),
    );
    let engine = IpIntelligenceCloudEngine::builder()
        .cloud_request_engine(request_engine)
        .build()
        .expect("IP Intelligence engine builds");
    assert!(engine.properties_loaded(), "discovery has completed");

    let expected = [
        ("Town", "Location"),
        ("CountryCodesGeographical", "Location"),
        ("RegisteredName", "Network"),
        ("IpRangeStart", ""),
    ];
    let core: Vec<(&str, &str)> = engine
        .properties()
        .iter()
        .map(|p| (p.name.as_str(), p.category.as_str()))
        .collect();
    assert_eq!(core, expected);
    let aspect: Vec<(&str, &str)> = engine
        .aspect_properties()
        .iter()
        .map(|p| (p.name(), p.core().category.as_str()))
        .collect();
    assert_eq!(aspect, expected);
}
