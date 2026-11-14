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

//! The pipeline that runs Device Detection and then the Set Headers
//! element, so the response headers Device Detection asks for, such as
//! `Accept-CH`, are compared with .NET's. The .NET producer builds the same
//! pipeline.

use std::sync::Arc;

use anyhow::Result;
use fiftyone_pipeline_core::{FlowData, FlowElement, Pipeline};
use fiftyone_pipeline_engines_fiftyone::SetHeadersElement;
use serde_json::{Map, Value};

/// The `pipeline` of a case that runs through this pipeline.
pub const PIPELINE: &str = "set-headers";

/// `device` and then the Set Headers element, with process errors
/// collected.
pub fn build_pipeline(device: &Arc<dyn FlowElement>) -> Result<Arc<Pipeline>> {
    Ok(Pipeline::builder()
        .add_element(Arc::clone(device))
        .add_element(Arc::new(SetHeadersElement::new()))
        .suppress_process_exceptions(true)
        .build()?)
}

/// Each response header the Set Headers element gave with its value, or
/// null when the flow data holds no Set Headers data.
pub fn response_headers(data: &FlowData) -> Value {
    match data.get(SetHeadersElement::KEY) {
        Some(set_headers) => Value::Object(
            set_headers
                .response_headers()
                .iter()
                .filter(|(name, _)| !name.eq_ignore_ascii_case("accept-ch"))
                .map(|(name, value)| (name.clone(), Value::String(value.clone())))
                .collect::<Map<String, Value>>(),
        ),
        None => Value::Null,
    }
}
