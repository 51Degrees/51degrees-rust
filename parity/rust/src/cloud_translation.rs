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

//! The cloud translation pipeline: the Sequence element, the cloud request
//! engine, the cloud countries translation engine and the JSON builder, as
//! the .NET producer builds it.
//!
//! The cloud is a stand-in inside this program, the same as the .NET
//! producer's. It grants the six translated properties, takes the two
//! parity evidence keys, and answers each data request with the body and
//! status those keys carry, so each case gives both languages the same
//! cloud response.

use std::sync::Arc;

use anyhow::Result;
use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_ip_intelligence_translation::CloudCountriesTranslationEngine;
use fiftyone_json_builder::JsonBuilderElementBuilder;
use fiftyone_pipeline_core::Pipeline;
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

/// The `pipeline` of a case that runs through this pipeline.
pub const PIPELINE: &str = "cloud-translation";

/// Never contacted. The stand-in answers every request itself.
const ENDPOINT: &str = "https://cloud.parity.invalid/api/v4/";

/// Not a real key. The stand-in grants it the six properties.
const RESOURCE_KEY: &str = "parity-resource-key";

/// The form fields that carry the body and the status of the response, as
/// the evidence keys `query.parity-cloud-response` and
/// `query.parity-cloud-status` reach the cloud. With no status the response
/// is 200, and with no body it is empty.
const RESPONSE_FIELD: &str = "parity-cloud-response";
const STATUS_FIELD: &str = "parity-cloud-status";
const STATUS_DEFAULT: u16 = 200;

/// The six properties .NET's engine tests list, with the types the cloud
/// gives them. The same text as the .NET producer's.
const ACCESSIBLE_PROPERTIES: &str = r#"{"Products":{"countrynamestranslated":{"DataTier":"CloudV4","Properties":[{"Name":"CountryNamesGeographicalTranslated","Type":"WeightedString"},{"Name":"CountryNamesPopulationTranslated","Type":"WeightedString"},{"Name":"CountryNamesGeographicalAllTranslated","Type":"Array"},{"Name":"CountryNamesPopulationAllTranslated","Type":"Array"},{"Name":"CountryCodesGeographicalAll","Type":"Array"},{"Name":"CountryCodesPopulationAll","Type":"Array"}]}}}"#;

/// The evidence the stand-in takes. The same text as the .NET producer's.
const EVIDENCE_KEYS: &str = r#"["query.parity-cloud-response","query.parity-cloud-status"]"#;

/// Answers the discovery requests with the constants above, and a data
/// request with the response its form carries.
struct StandInCloud;

impl CloudHttpClient for StandInCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let field = |name: &str| {
            request
                .form
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        let (status, body) = if request.url.contains("accessibleproperties") {
            (STATUS_DEFAULT, ACCESSIBLE_PROPERTIES.to_owned())
        } else if request.url.contains("evidencekeys") {
            (STATUS_DEFAULT, EVIDENCE_KEYS.to_owned())
        } else {
            let status = match field(STATUS_FIELD) {
                Some(text) => text
                    .parse()
                    .map_err(|_| format!("{STATUS_FIELD} {text} is not a status"))?,
                None => STATUS_DEFAULT,
            };
            (status, field(RESPONSE_FIELD).unwrap_or_default().to_owned())
        };
        Ok(CloudHttpResponse {
            status,
            body,
            retry_after: None,
        })
    }
}

/// Sequence, the cloud request engine on the stand-in, the cloud countries
/// translation engine and the JSON builder, with process errors collected
/// rather than returned.
pub fn build_pipeline() -> Result<Arc<Pipeline>> {
    let request = Arc::new(
        CloudRequestEngine::builder()
            .resource_key(RESOURCE_KEY)
            .endpoint(ENDPOINT)
            .http_client(Arc::new(StandInCloud))
            .build()?,
    );
    let countries = CloudCountriesTranslationEngine::builder()
        .cloud_request_engine(Arc::clone(&request))
        .build()?;
    let json = JsonBuilderElementBuilder::new().build();
    Ok(Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(request)
        .add_element(Arc::new(countries))
        .add_element(Arc::new(json))
        .suppress_process_exceptions(true)
        .build()?)
}
