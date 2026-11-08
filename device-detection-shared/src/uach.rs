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

//! The high entropy values evidence keys, and the deprecated User-Agent
//! Client Hints high-entropy decoder element.
//!
//! [`UachJsConversionElement`] implements the
//! [UA-CH high-entropy decoder specification](https://github.com/51Degrees/specifications/blob/main/device-detection-specification/pipeline-elements/uach-high-entropy-decoder.md).
//! The UA-CH JavaScript API (`navigator.userAgentData.getHighEntropyValues`)
//! returns client-hint data in a different shape from the UA-CH HTTP headers,
//! base-64 encoded. The element decodes that evidence and writes the
//! equivalent `sec-ch-ua*` header values into its own element data.
//!
//! No engine reads that data. The on-premise Hash engine passes the high
//! entropy values to the C library, which turns them into the `sec-ch-ua*`
//! headers before it matches, so a pipeline needs no decoder element.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde_json::Value;

use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, PropertyMetaData, PropertyValueType, Result, TypedKey,
};

/// The evidence-key suffix (without prefix) the high-entropy blob is supplied
/// under, `51d_gethighentropyvalues`.
pub const UACH_HIGH_ENTROPY_EVIDENCE_SUFFIX: &str = "51d_gethighentropyvalues";

/// The query-prefixed evidence key, `query.51d_gethighentropyvalues`. The
/// specification gives it precedence over the cookie key when both are
/// present. The on-premise engine uses whichever of the two comes first in
/// the evidence.
pub const UACH_EVIDENCE_QUERY_KEY: &str = "query.51d_gethighentropyvalues";

/// The cookie-prefixed evidence key, `cookie.51d_gethighentropyvalues`.
pub const UACH_EVIDENCE_COOKIE_KEY: &str = "cookie.51d_gethighentropyvalues";

/// The data key under which this element stores its converted `sec-ch-ua*`
/// values.
pub const UACH_ELEMENT_DATA_KEY: &str = "uach";

/// The six `sec-ch-ua*` header names this element can emit, in the order the
/// specification lists them.
const SEC_CH_UA: &str = "sec-ch-ua";
const SEC_CH_UA_FULL_VERSION_LIST: &str = "sec-ch-ua-full-version-list";
const SEC_CH_UA_MODEL: &str = "sec-ch-ua-model";
const SEC_CH_UA_MOBILE: &str = "sec-ch-ua-mobile";
const SEC_CH_UA_PLATFORM: &str = "sec-ch-ua-platform";
const SEC_CH_UA_PLATFORM_VERSION: &str = "sec-ch-ua-platform-version";

/// The element data produced by [`UachJsConversionElement`].
///
/// A flat property bag holding whichever of the six `sec-ch-ua*` values the
/// decoder could derive from the high-entropy blob. Read a value through the
/// inherited [`ElementData::get`] by header name, for example
/// `uach.get("sec-ch-ua-mobile")`. No engine reads them.
#[derive(Debug, Clone, Default)]
pub struct UachData {
    values: MapElementData,
}

impl UachData {
    /// Create an empty UACH data bag.
    pub fn new() -> Self {
        UachData {
            values: MapElementData::new(),
        }
    }

    /// Borrow the underlying property bag.
    pub fn values(&self) -> &MapElementData {
        &self.values
    }
}

impl ElementData for UachData {
    fn get(
        &self,
        name: &str,
    ) -> std::result::Result<
        fiftyone_pipeline_core::PropertyValue,
        fiftyone_pipeline_core::NoValueError,
    > {
        self.values.get(name)
    }

    fn keys(&self) -> Vec<String> {
        self.values.keys()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// The UA-CH high-entropy decoder flow element.
///
/// Reads the base-64 `getHighEntropyValues` blob from evidence (preferring the
/// query key over the cookie key), decodes and parses it, and writes the
/// equivalent `sec-ch-ua*` values into a `UachData` under
/// `UACH_ELEMENT_DATA_KEY`.
///
/// The element has no configuration. No engine reads its data, so a pipeline
/// does not need it. The on-premise Hash engine passes the high entropy
/// values to the C library, which makes the same conversion before it
/// matches.
///
/// # Example
///
/// ```
/// # #![allow(deprecated)]
/// use base64::{engine::general_purpose::STANDARD, Engine as _};
/// use fiftyone_pipeline_core::{ElementData, Evidence, Pipeline};
/// use fiftyone_device_detection_shared::UachJsConversionElement;
/// use std::sync::Arc;
///
/// let blob = STANDARD.encode(
///     r#"{"brands":[{"brand":"Chromium","version":"124"}],"mobile":false,"platform":"macOS"}"#,
/// );
///
/// let pipeline = Pipeline::builder()
///     .add_element(Arc::new(UachJsConversionElement::new()))
///     .build()
///     .unwrap();
/// let mut data = pipeline.create_flow_data_with(
///     Evidence::builder()
///         .add("query.51d_gethighentropyvalues", blob)
///         .build(),
/// );
/// data.process().unwrap();
///
/// let uach = data.get(UachJsConversionElement::DATA_KEY).unwrap();
/// assert_eq!(uach.get("sec-ch-ua-mobile").unwrap().as_str(), Some("?0"));
/// assert_eq!(uach.get("sec-ch-ua-platform").unwrap().as_str(), Some("\"macOS\""));
/// ```
#[deprecated(
    note = "no engine reads its data, as the Hash engine reads high entropy values itself"
)]
pub struct UachJsConversionElement {
    evidence_key_filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

#[allow(deprecated)]
impl UachJsConversionElement {
    /// The typed key for this element's `UachData`.
    pub const DATA_KEY: TypedKey<UachData> = TypedKey::new(UACH_ELEMENT_DATA_KEY);

    /// Create a UA-CH high-entropy decoder.
    pub fn new() -> Self {
        // The element accepts the high-entropy blob under either prefix, with
        // the query key ranked above the cookie key so it wins when both are
        // present (a lower order means higher precedence).
        let evidence_key_filter = EvidenceKeyFilterWhitelist::with_orders([
            (UACH_EVIDENCE_QUERY_KEY, 0),
            (UACH_EVIDENCE_COOKIE_KEY, 1),
        ]);

        // Publish the six headers this element can populate. They are plain
        // string values (sec-ch-ua-mobile is a string of "?0"/"?1", per the
        // specification note).
        let properties = [
            SEC_CH_UA,
            SEC_CH_UA_FULL_VERSION_LIST,
            SEC_CH_UA_MODEL,
            SEC_CH_UA_MOBILE,
            SEC_CH_UA_PLATFORM,
            SEC_CH_UA_PLATFORM_VERSION,
        ]
        .into_iter()
        .map(|name| PropertyMetaData::new(name, UACH_ELEMENT_DATA_KEY, PropertyValueType::String))
        .collect();

        UachJsConversionElement {
            evidence_key_filter,
            properties,
        }
    }

    /// Pick the encoded blob from evidence, preferring the query key over the
    /// cookie key as the specification requires.
    fn select_evidence<'a>(&self, data: &'a FlowData) -> Option<&'a str> {
        data.evidence()
            .get(UACH_EVIDENCE_QUERY_KEY)
            .or_else(|| data.evidence().get(UACH_EVIDENCE_COOKIE_KEY))
    }
}

#[allow(deprecated)]
impl Default for UachJsConversionElement {
    fn default() -> Self {
        UachJsConversionElement::new()
    }
}

#[allow(deprecated)]
impl FlowElement for UachJsConversionElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        // Convert outside the get_or_add closure so the immutable evidence
        // borrow ends before the mutable element-data borrow begins.
        let converted = self.select_evidence(data).and_then(convert_high_entropy);

        let element_data = data.get_or_add(Self::DATA_KEY, UachData::new)?;
        if let Some(values) = converted {
            for (name, value) in values {
                element_data.values.insert(name, value);
            }
        }
        Ok(())
    }

    fn data_key(&self) -> &str {
        UACH_ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.evidence_key_filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// Decode a base-64 `getHighEntropyValues` blob and convert it to the
/// `sec-ch-ua*` header values.
///
/// Returns the `(header-name, value)` pairs that could be derived, or `None` if
/// the blob is not valid base 64 or not valid JSON. Each individual field is
/// optional: a blob that carries only `mobile` and `platform` produces only
/// those two headers, mirroring the reference conversion which emits a header
/// only when the corresponding source field is present.
fn convert_high_entropy(encoded: &str) -> Option<Vec<(String, String)>> {
    // The reference JavaScript decodes standard base 64. Reject anything that
    // is not valid rather than guessing.
    let decoded = STANDARD.decode(encoded.trim()).ok()?;
    let json: Value = serde_json::from_slice(&decoded).ok()?;
    let object = json.as_object()?;

    let mut out: Vec<(String, String)> = Vec::with_capacity(6);

    // brands -> sec-ch-ua. Each entry becomes `"Brand";v="version"`, joined by
    // ", ". A missing brand or version is treated as an empty string so the
    // structure of the header is preserved.
    if let Some(brands) = object.get("brands").and_then(Value::as_array) {
        out.push((SEC_CH_UA.to_owned(), format_brand_list(brands)));
    }

    // fullVersionList -> sec-ch-ua-full-version-list, same formatting.
    if let Some(full) = object.get("fullVersionList").and_then(Value::as_array) {
        out.push((
            SEC_CH_UA_FULL_VERSION_LIST.to_owned(),
            format_brand_list(full),
        ));
    }

    // model -> sec-ch-ua-model, a quoted string.
    if let Some(model) = object.get("model").and_then(Value::as_str) {
        out.push((SEC_CH_UA_MODEL.to_owned(), quote(model)));
    }

    // mobile -> sec-ch-ua-mobile, the boolean rendered as the "?0"/"?1" the
    // HTTP header uses (note: a string, not a JSON boolean).
    if let Some(mobile) = object.get("mobile").and_then(Value::as_bool) {
        out.push((
            SEC_CH_UA_MOBILE.to_owned(),
            if mobile { "?1" } else { "?0" }.to_owned(),
        ));
    }

    // platform -> sec-ch-ua-platform, a quoted string.
    if let Some(platform) = object.get("platform").and_then(Value::as_str) {
        out.push((SEC_CH_UA_PLATFORM.to_owned(), quote(platform)));
    }

    // platformVersion -> sec-ch-ua-platform-version, a quoted string.
    if let Some(platform_version) = object.get("platformVersion").and_then(Value::as_str) {
        out.push((
            SEC_CH_UA_PLATFORM_VERSION.to_owned(),
            quote(platform_version),
        ));
    }

    Some(out)
}

/// Format a brand list (the `brands` or `fullVersionList` array) into the
/// `sec-ch-ua` style header, `"Brand";v="version"` entries joined by `, `.
fn format_brand_list(brands: &[Value]) -> String {
    brands
        .iter()
        .map(|entry| {
            let brand = entry.get("brand").and_then(Value::as_str).unwrap_or("");
            let version = entry.get("version").and_then(Value::as_str).unwrap_or("");
            format!("\"{brand}\";v=\"{version}\"")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Wrap a string in the double quotes the structured-header string fields use.
fn quote(value: &str) -> String {
    format!("\"{value}\"")
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;
    use fiftyone_pipeline_core::{constants, Evidence, Pipeline};
    use std::sync::Arc;

    /// A representative high-entropy blob, the JSON shape
    /// `navigator.userAgentData.getHighEntropyValues` returns.
    const SAMPLE_JSON: &str = r#"{
        "architecture": "x86",
        "bitness": "64",
        "brands": [
            {"brand": "Not_A Brand", "version": "8"},
            {"brand": "Chromium", "version": "120"},
            {"brand": "Google Chrome", "version": "120"}
        ],
        "fullVersionList": [
            {"brand": "Not_A Brand", "version": "8.0.0.0"},
            {"brand": "Chromium", "version": "120.0.6099.71"},
            {"brand": "Google Chrome", "version": "120.0.6099.71"}
        ],
        "mobile": false,
        "model": "",
        "platform": "Windows",
        "platformVersion": "14.0.0"
    }"#;

    fn encode(json: &str) -> String {
        STANDARD.encode(json.as_bytes())
    }

    #[test]
    fn evidence_keys_match_core_prefixes() {
        assert!(UACH_EVIDENCE_QUERY_KEY.starts_with(constants::EVIDENCE_QUERY_PREFIX));
        assert!(UACH_EVIDENCE_COOKIE_KEY.starts_with(constants::EVIDENCE_COOKIE_PREFIX));
        assert!(UACH_EVIDENCE_QUERY_KEY.ends_with(UACH_HIGH_ENTROPY_EVIDENCE_SUFFIX));
        assert!(UACH_EVIDENCE_COOKIE_KEY.ends_with(UACH_HIGH_ENTROPY_EVIDENCE_SUFFIX));
    }

    #[test]
    fn converts_full_sample_blob() {
        let converted = convert_high_entropy(&encode(SAMPLE_JSON)).expect("valid blob");
        let map: std::collections::HashMap<_, _> = converted.into_iter().collect();

        assert_eq!(
            map.get(SEC_CH_UA).map(String::as_str),
            Some("\"Not_A Brand\";v=\"8\", \"Chromium\";v=\"120\", \"Google Chrome\";v=\"120\"")
        );
        assert_eq!(
            map.get(SEC_CH_UA_FULL_VERSION_LIST).map(String::as_str),
            Some(
                "\"Not_A Brand\";v=\"8.0.0.0\", \"Chromium\";v=\"120.0.6099.71\", \
                 \"Google Chrome\";v=\"120.0.6099.71\""
            )
        );
        assert_eq!(map.get(SEC_CH_UA_MODEL).map(String::as_str), Some("\"\""));
        assert_eq!(map.get(SEC_CH_UA_MOBILE).map(String::as_str), Some("?0"));
        assert_eq!(
            map.get(SEC_CH_UA_PLATFORM).map(String::as_str),
            Some("\"Windows\"")
        );
        assert_eq!(
            map.get(SEC_CH_UA_PLATFORM_VERSION).map(String::as_str),
            Some("\"14.0.0\"")
        );
    }

    #[test]
    fn mobile_true_renders_question_one() {
        let blob = encode(r#"{"mobile": true, "model": "Pixel 8"}"#);
        let converted = convert_high_entropy(&blob).expect("valid blob");
        let map: std::collections::HashMap<_, _> = converted.into_iter().collect();
        assert_eq!(map.get(SEC_CH_UA_MOBILE).map(String::as_str), Some("?1"));
        assert_eq!(
            map.get(SEC_CH_UA_MODEL).map(String::as_str),
            Some("\"Pixel 8\"")
        );
    }

    #[test]
    fn only_present_fields_are_emitted() {
        // A partial blob produces only the headers for the fields it carries.
        let blob = encode(r#"{"platform": "Android"}"#);
        let converted = convert_high_entropy(&blob).expect("valid blob");
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].0, SEC_CH_UA_PLATFORM);
        assert_eq!(converted[0].1, "\"Android\"");
    }

    #[test]
    fn invalid_base64_is_rejected() {
        assert!(convert_high_entropy("not valid base64 !!!").is_none());
    }

    #[test]
    fn valid_base64_but_invalid_json_is_rejected() {
        let blob = STANDARD.encode(b"this is not json");
        assert!(convert_high_entropy(&blob).is_none());
    }

    #[test]
    fn query_evidence_takes_precedence_over_cookie() {
        let query_blob = encode(r#"{"platform": "macOS"}"#);
        let cookie_blob = encode(r#"{"platform": "Windows"}"#);

        let element = UachJsConversionElement::new();
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(element))
            .build()
            .unwrap();
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add(UACH_EVIDENCE_QUERY_KEY, query_blob)
                .add(UACH_EVIDENCE_COOKIE_KEY, cookie_blob)
                .build(),
        );
        data.process().unwrap();

        let uach = data.get(UachJsConversionElement::DATA_KEY).unwrap();
        assert_eq!(
            uach.get(SEC_CH_UA_PLATFORM).unwrap().as_str(),
            Some("\"macOS\"")
        );
    }

    #[test]
    fn cookie_evidence_used_when_no_query() {
        let cookie_blob = encode(r#"{"mobile": true}"#);
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(UachJsConversionElement::new()))
            .build()
            .unwrap();
        let mut data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add(UACH_EVIDENCE_COOKIE_KEY, cookie_blob)
                .build(),
        );
        data.process().unwrap();

        let uach = data.get(UachJsConversionElement::DATA_KEY).unwrap();
        assert_eq!(uach.get(SEC_CH_UA_MOBILE).unwrap().as_str(), Some("?1"));
    }

    #[test]
    fn no_evidence_produces_empty_data_not_error() {
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(UachJsConversionElement::new()))
            .build()
            .unwrap();
        let mut data = pipeline.create_flow_data_with(Evidence::builder().build());
        data.process().unwrap();

        let uach = data.get(UachJsConversionElement::DATA_KEY).unwrap();
        assert!(uach.keys().is_empty());
    }

    #[test]
    fn element_advertises_keys_and_properties() {
        let element = UachJsConversionElement::new();
        assert_eq!(element.data_key(), UACH_ELEMENT_DATA_KEY);
        assert!(element
            .evidence_key_filter()
            .include(UACH_EVIDENCE_QUERY_KEY));
        assert!(element
            .evidence_key_filter()
            .include(UACH_EVIDENCE_COOKIE_KEY));
        // Query outranks cookie.
        assert!(
            element.evidence_key_filter().order(UACH_EVIDENCE_QUERY_KEY)
                < element
                    .evidence_key_filter()
                    .order(UACH_EVIDENCE_COOKIE_KEY)
        );
        assert_eq!(element.properties().len(), 6);
    }
}
