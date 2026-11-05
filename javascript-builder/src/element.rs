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

//! The JavaScript builder flow element.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use fiftyone_json_builder::JSON_BUILDER_DATA_KEY;
use fiftyone_pipeline_core::constants::{
    EVIDENCE_PROTOCOL_KEY, EVIDENCE_QUERY_PREFIX, EVIDENCE_SEPARATOR,
};
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
    PropertyValue, PropertyValueType, Result,
};
use fiftyone_pipeline_engines_fiftyone::constants::{EVIDENCE_SEQUENCE, EVIDENCE_SESSIONID};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

use crate::builder::{grants_51did, is_valid_object_name, SharedEngine, UserPromptSource};
use crate::constants::{
    DELAY_EXECUTION_MARKER, EVIDENCE_ENABLE_COOKIES, EVIDENCE_HOST_KEY, EVIDENCE_OBJECT_NAME,
    FALLBACK_PROTOCOL, FETCH_PROPERTY, JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY,
    JAVASCRIPT_PROPERTY_KEY, PROMISE_FULL_VALUE, PROMISE_PROPERTY,
};
use crate::data::{JavaScriptBuilderElementData, JAVASCRIPT_BUILDER_DATA_KEY};
use crate::minify::minify;
use crate::mustache::Template;
use crate::template_data::JavaScriptResource;
use crate::JavaScriptBuilderElementBuilder;

/// The Mustache template, embedded at compile time.
const TEMPLATE_SOURCE: &str = include_str!("../assets/JavaScriptResource.mustache");

/// The `application/x-www-form-urlencoded` percent-encoding set used for the
/// request parameter keys and values.
///
/// The set percent-encodes everything except the
/// unreserved characters `A-Z a-z 0-9 - _ . *` (and encodes a space as `%20`,
/// not `+`, because each key and value is encoded individually before being
/// placed into the JSON object). It starts from
/// "encode all non-alphanumerics" and adds back the four exceptions.
const URL_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'*');

/// The longest session id the script is given, in bytes.
const MAX_SESSION_ID_LENGTH: usize = 64;

/// The session id to render, which is the one given when it is safe and an
/// empty string when it is not.
///
/// The template writes the session id inside quotes without any escaping, so
/// a value holding a quote, a backslash or a line break would end the string
/// early and break the script or change what it does. The
/// [javascript-builder specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/javascript-builder.md)
/// says a session id that is not 1 to 64 ASCII letters, digits and hyphens is
/// rendered as an empty string.
fn safe_session_id(session_id: String) -> String {
    let safe = !session_id.is_empty()
        && session_id.len() <= MAX_SESSION_ID_LENGTH
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    if safe {
        session_id
    } else {
        String::new()
    }
}

/// The sequence to render, which is the one given when it is positive and `1`
/// when it is not.
///
/// The template writes the sequence as bare code, and the script counts its
/// own requests up from it, so a value of zero or less is not a sequence the
/// script can use. The specification says such a value is rendered as `1`.
fn safe_sequence(sequence: i32) -> i32 {
    if sequence > 0 {
        sequence
    } else {
        1
    }
}

/// Generates a JavaScript include to be run on the client device.
///
/// The element renders the bundled Mustache template with the JSON payload
/// produced by the JSON builder, the session id and sequence number (from the
/// sequence element's data, or from evidence when no sequence element ran),
/// a callback URL and the request parameters, then optionally minifies the
/// result. The generated JavaScript is stored on the flow data under the
/// [`crate::JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY`] element data key. It implements
/// the
/// [javascript-builder specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/javascript-builder.md).
///
/// # Derivation rules
///
/// - **protocol**: the configured protocol if set, else the `header.protocol`
///   evidence, else `https`.
/// - **host**: the configured host if set, else the `header.host` evidence.
/// - **object name**: the `query.fod-js-object-name` evidence if present and a
///   valid JavaScript identifier that is not a reserved word, `Infinity`,
///   `NaN`, `undefined` or `fiftyoneDegreesManager`, else the configured
///   object name (default `fod`). An invalid requested name, the empty string
///   included, is ignored with a warning logged through the `log` crate.
/// - **enable cookies**: the `query.fod-js-enable-cookies` evidence parsed as a
///   boolean if present, else the configured default (true).
/// - **callback URL**: built only when protocol, host and endpoint are all
///   present, normalizing the single slash between host and endpoint. When a URL
///   is built the background-update mechanism is enabled.
/// - **parameters**: every `query.*` evidence entry except the session id and
///   sequence, with the prefix stripped and the key and value URL-encoded,
///   serialized as a JSON object.
/// - **session id**: the session id from the flow data when it is 1 to 64
///   ASCII letters, digits and hyphens, else an empty string, because the
///   template writes it inside quotes without any escaping.
/// - **sequence**: the sequence from the flow data when it is a positive 32
///   bit integer, else `1`, because the template writes it as bare code.
/// - **has delayed properties**: true when the JSON payload contains the
///   `delayexecution` marker.
/// - **supports promises**: true when this request's device-detection
///   `Promise` property is `Full`.
/// - **supports fetch**: true when this request's device-detection `Fetch`
///   property is true.
/// - **user prompt**: true when the builder was given a cloud request engine
///   whose licensed products include 51Did, through
///   [`JavaScriptBuilderElementBuilder::set_shared_cloud_request_engine`] or
///   [`JavaScriptBuilderElementBuilder::set_cloud_request_engine`], and false
///   while the shared engine's discovery has not completed. The section it
///   controls sits inside the update section, so it is only rendered when a
///   callback URL was built as well.
///
/// # Example
///
/// ```
/// use std::sync::Arc;
/// use fiftyone_pipeline_core::{Evidence, Pipeline};
/// use fiftyone_json_builder::JsonBuilderElement;
/// use fiftyone_javascript_builder::{JavaScriptBuilderElement, JAVASCRIPT_BUILDER_DATA_KEY};
///
/// let pipeline = Pipeline::builder()
///     .add_element(Arc::new(JsonBuilderElement::new()))
///     .add_element(Arc::new(JavaScriptBuilderElement::new()))
///     .build()
///     .unwrap();
///
/// let mut data = pipeline.create_flow_data_with(
///     Evidence::builder()
///         .add("header.host", "localhost")
///         .add("query.sequence", "1")
///         .build(),
/// );
/// data.process().unwrap();
///
/// let js = data.get(JAVASCRIPT_BUILDER_DATA_KEY).unwrap().javascript().to_owned();
/// assert!(js.contains("fiftyoneDegreesManager"));
/// ```
pub struct JavaScriptBuilderElement {
    evidence_key_filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    template: Template,

    host: String,
    endpoint: String,
    protocol: String,
    object_name: String,
    enable_cookies: bool,
    minify: bool,
    /// Whether the script carries the user prompt section, once that is
    /// settled from the cloud request engine's licensed products.
    user_prompt_settled: OnceLock<bool>,
    /// The request engine whose licensed products settle
    /// `user_prompt_settled`, when the builder was given a shared one.
    user_prompt_engine: Option<SharedEngine>,
}

impl JavaScriptBuilderElement {
    /// Create a JavaScript builder with the default configuration.
    pub fn new() -> Self {
        JavaScriptBuilderElementBuilder::new().build()
    }

    /// Start configuring a JavaScript builder.
    pub fn builder() -> JavaScriptBuilderElementBuilder {
        JavaScriptBuilderElementBuilder::new()
    }

    /// Internal constructor used by the builder.
    pub(crate) fn from_parts(
        host: String,
        endpoint: String,
        protocol: String,
        object_name: String,
        enable_cookies: bool,
        minify: bool,
        user_prompt: UserPromptSource,
    ) -> Self {
        // The template is parsed once at construction. The embedded source is a
        // valid Mustache template, so parsing it cannot fail in practice. The
        // expect documents that invariant.
        let template = Template::parse(TEMPLATE_SOURCE)
            .expect("the embedded JavaScriptResource template is valid Mustache");
        let (user_prompt_settled, user_prompt_engine) = match user_prompt {
            UserPromptSource::Decided(render) => (OnceLock::from(render), None),
            UserPromptSource::Engine(engine) => (OnceLock::new(), Some(engine)),
        };

        JavaScriptBuilderElement {
            evidence_key_filter: EvidenceKeyFilterWhitelist::new([
                EVIDENCE_HOST_KEY,
                EVIDENCE_PROTOCOL_KEY,
                EVIDENCE_OBJECT_NAME,
                EVIDENCE_ENABLE_COOKIES,
            ]),
            properties: vec![PropertyMetaData::new(
                JAVASCRIPT_PROPERTY_KEY,
                JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY,
                PropertyValueType::String,
            )],
            template,
            host,
            endpoint,
            protocol,
            object_name,
            enable_cookies,
            minify,
            user_prompt_settled,
            user_prompt_engine,
        }
    }

    /// Whether this script carries the user prompt section. The answer is
    /// kept once the request engine's licensed products are known. Until then
    /// the section is left out without keeping that answer, as a result
    /// obtained before discovery completes must not be kept.
    fn user_prompt(&self) -> bool {
        if let Some(&render) = self.user_prompt_settled.get() {
            return render;
        }
        match self
            .user_prompt_engine
            .as_ref()
            .map(|engine| engine.public_properties())
        {
            Some(Ok(products)) => *self
                .user_prompt_settled
                .get_or_init(|| grants_51did(products)),
            _ => false,
        }
    }

    /// Resolve the protocol: configured value, else `header.protocol` evidence,
    /// else the `https` fallback.
    fn resolve_protocol(&self, data: &FlowData) -> String {
        if !self.protocol.is_empty() {
            return self.protocol.clone();
        }
        if let Some(protocol) = data.evidence().get(EVIDENCE_PROTOCOL_KEY) {
            if !protocol.is_empty() {
                return protocol.to_owned();
            }
        }
        FALLBACK_PROTOCOL.to_owned()
    }

    /// Resolve the host: configured value, else `header.host` evidence, else an
    /// empty string.
    fn resolve_host(&self, data: &FlowData) -> String {
        if !self.host.is_empty() {
            return self.host.clone();
        }
        data.evidence()
            .get(EVIDENCE_HOST_KEY)
            .unwrap_or("")
            .to_owned()
    }

    /// Resolve the object name: `query.fod-js-object-name` evidence if present
    /// and a valid JavaScript identifier, else the configured object name.
    ///
    /// A requested name that is not valid is ignored with a warning logged,
    /// because it would be written into the script as given. The requested
    /// text is left out of the warning so that it cannot reach the log either.
    fn resolve_object_name(&self, data: &FlowData) -> String {
        match data.evidence().get(EVIDENCE_OBJECT_NAME) {
            Some(name) if is_valid_object_name(name) => name.to_owned(),
            Some(_) => {
                log::warn!(
                    "The requested JavaScript object name ({}) is not a valid JavaScript \
                     identifier, so the configured name '{}' is used.",
                    EVIDENCE_OBJECT_NAME,
                    self.object_name
                );
                self.object_name.clone()
            }
            None => self.object_name.clone(),
        }
    }

    /// Resolve the enable-cookies flag: `query.fod-js-enable-cookies` evidence
    /// parsed as a boolean if present and parseable, else the configured
    /// default.
    fn resolve_enable_cookies(&self, data: &FlowData) -> bool {
        match data.evidence().get(EVIDENCE_ENABLE_COOKIES) {
            Some(value) => value.trim().parse::<bool>().unwrap_or(self.enable_cookies),
            None => self.enable_cookies,
        }
    }

    /// Build the callback URL, or `None` if protocol, host or endpoint is
    /// missing.
    ///
    /// The single slash between host and endpoint is normalized: a slash is
    /// added when neither side has one, and a duplicate is removed when both
    /// sides have one.
    fn build_url(protocol: &str, host: &str, endpoint: &str) -> Option<String> {
        if protocol.trim().is_empty() || host.trim().is_empty() || endpoint.trim().is_empty() {
            return None;
        }

        let endpoint_has_slash = endpoint.starts_with('/');
        let host_has_slash = host.ends_with('/');

        let normalised_endpoint = if !endpoint_has_slash && !host_has_slash {
            // No slash on either side: add one.
            format!("/{endpoint}")
        } else if endpoint_has_slash && host_has_slash {
            // A slash on both sides: drop the leading one from the endpoint.
            endpoint[1..].to_owned()
        } else {
            endpoint.to_owned()
        };

        Some(format!("{protocol}://{host}{normalised_endpoint}"))
    }

    /// The session id the script is given, which is the value read from the
    /// flow data when it is safe to render and an empty string otherwise.
    ///
    /// See [`safe_session_id`] for what safe means here.
    fn rendered_session_id(data: &FlowData) -> String {
        safe_session_id(Self::session_id(data))
    }

    /// The sequence the script is given, which is the value read from the flow
    /// data when it is a positive number and `1` otherwise.
    fn rendered_sequence(data: &FlowData) -> i32 {
        safe_sequence(Self::sequence(data))
    }

    /// The session id for the template.
    ///
    /// The specification takes it from the sequence element's data, because
    /// on a first request there is no `query.session-id` evidence and the
    /// sequence element is what creates the id. Evidence is immutable here, so
    /// the id the sequence element creates is only in its element data.
    /// Reading evidence alone left `fod.sessionId` empty on every first page.
    /// Evidence is still read when no sequence element ran, and an empty
    /// string is used when neither has a value.
    fn session_id(data: &FlowData) -> String {
        if let Some(id) = data
            .get(SequenceElement::KEY)
            .and_then(|sequence| sequence.session_id())
        {
            return id.to_owned();
        }
        data.evidence()
            .get(EVIDENCE_SESSIONID)
            .unwrap_or("")
            .to_owned()
    }

    /// The sequence number for the template.
    ///
    /// Taken from the sequence element's data, which has already added one to
    /// any `query.sequence` evidence, as the specification says. Without a
    /// sequence element the evidence is read instead, and `1` is used when
    /// that is absent or does not parse.
    fn sequence(data: &FlowData) -> i32 {
        if let Some(sequence) = data
            .get(SequenceElement::KEY)
            .and_then(|sequence| sequence.sequence())
            .and_then(|sequence| i32::try_from(sequence).ok())
        {
            return sequence;
        }
        data.evidence()
            .get(EVIDENCE_SEQUENCE)
            .and_then(|s| s.trim().parse::<i32>().ok())
            .unwrap_or(1)
    }

    /// Build the request-parameters JSON object.
    ///
    /// Every `query.*` evidence entry except the session id and sequence is
    /// included. The `query.` prefix is stripped, then the key and value are
    /// URL-encoded. The result is serialized as a JSON object. A `BTreeMap` is
    /// used so the key order, and therefore the serialized JSON, is
    /// deterministic.
    fn build_parameters(data: &FlowData) -> String {
        let query_prefix = format!("{EVIDENCE_QUERY_PREFIX}{EVIDENCE_SEPARATOR}");
        let mut parameters: BTreeMap<String, String> = BTreeMap::new();

        for (key, value) in data.evidence().iter() {
            if !key.starts_with(&query_prefix) {
                continue;
            }
            // Evidence keys are lowercased; the excluded keys are too.
            if key.eq_ignore_ascii_case(EVIDENCE_SESSIONID)
                || key.eq_ignore_ascii_case(EVIDENCE_SEQUENCE)
            {
                continue;
            }
            let field = &key[query_prefix.len()..];
            let encoded_key = utf8_percent_encode(field, URL_ENCODE_SET).to_string();
            let encoded_value = utf8_percent_encode(value, URL_ENCODE_SET).to_string();
            parameters.insert(encoded_key, encoded_value);
        }

        serde_json::to_string(&parameters).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Whether this request's value of a device-detection property satisfies a
    /// predicate.
    ///
    /// The value describes the visitor's browser, so it is read from each
    /// request's flow data. A request with no value, because no element
    /// produces the property or because this request has no answer for it,
    /// does not satisfy the predicate, and the next request is read afresh.
    fn property_satisfies(
        data: &FlowData,
        property: &str,
        predicate: impl Fn(&PropertyValue) -> bool,
    ) -> bool {
        data.get_evidence_or_property(property)
            .is_ok_and(|value| predicate(&value))
    }

    /// Whether the client supports promises, being true only when this
    /// request's device-detection `Promise` property is `Full`.
    fn supports_promises(data: &FlowData) -> bool {
        Self::property_satisfies(data, PROMISE_PROPERTY, |value| {
            value.as_str() == Some(PROMISE_FULL_VALUE)
        })
    }

    /// Whether the client supports the fetch API, being true only when this
    /// request's device-detection `Fetch` property is true.
    fn supports_fetch(data: &FlowData) -> bool {
        Self::property_satisfies(data, FETCH_PROPERTY, |value| value.as_bool() == Some(true))
    }

    /// Read the JSON payload from the JSON builder's element data, or an empty
    /// string if the JSON builder did not run.
    fn json_object(data: &FlowData) -> String {
        data.get(JSON_BUILDER_DATA_KEY)
            .map(|json| json.json().to_owned())
            .unwrap_or_default()
    }

    /// Render and (optionally) minify the JavaScript for this request, returning
    /// the content to store and whether minification flagged an error.
    fn build_javascript(&self, data: &FlowData) -> (String, bool) {
        let protocol = self.resolve_protocol(data);
        let host = self.resolve_host(data);
        let object_name = self.resolve_object_name(data);
        let enable_cookies = self.resolve_enable_cookies(data);

        let supports_promises = Self::supports_promises(data);
        let supports_fetch = Self::supports_fetch(data);

        let json_object = Self::json_object(data);
        let parameters = Self::build_parameters(data);
        let session_id = Self::rendered_session_id(data);
        let sequence = Self::rendered_sequence(data);

        let url = Self::build_url(&protocol, &host, &self.endpoint);
        let update_enabled = url.as_ref().is_some_and(|u| !u.is_empty());

        let has_delayed_properties = json_object.contains(DELAY_EXECUTION_MARKER);

        let resource = JavaScriptResource::new(
            object_name,
            json_object,
            session_id,
            sequence,
            supports_promises,
            supports_fetch,
            url.unwrap_or_default(),
            parameters,
            enable_cookies,
            update_enabled,
            has_delayed_properties,
            self.user_prompt(),
        );

        let content = resource.render(&self.template);

        let outcome = if self.minify {
            minify(content)
        } else {
            crate::minify::MinifyOutcome {
                content,
                had_error: false,
            }
        };

        (outcome.content, outcome.had_error)
    }
}

impl Default for JavaScriptBuilderElement {
    fn default() -> Self {
        JavaScriptBuilderElement::new()
    }
}

impl FlowElement for JavaScriptBuilderElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        // build_javascript already substitutes the unminified script when
        // minification fails, so the had_error flag needs no further handling
        // here. The content is the correct script to serve either way.
        let (content, _had_error) = self.build_javascript(data);

        let result = data.get_or_add(
            JAVASCRIPT_BUILDER_DATA_KEY,
            JavaScriptBuilderElementData::new,
        );
        match result {
            Ok(element_data) => {
                element_data.set_javascript(content);
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn data_key(&self) -> &str {
        JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.evidence_key_filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fiftyone_pipeline_core::{Evidence, Pipeline};
    use std::sync::Arc;

    /// Build a flow data carrying the supplied evidence on a pipeline whose only
    /// element is the JavaScript builder, without processing it. This lets the
    /// derivation helpers be exercised against real evidence.
    fn flow_data_with(pairs: &[(&str, &str)]) -> FlowData {
        let mut builder = Evidence::builder();
        for (key, value) in pairs {
            builder = builder.add(*key, *value);
        }
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(JavaScriptBuilderElement::new()))
            .build()
            .expect("pipeline builds");
        pipeline.create_flow_data_with(builder.build())
    }

    #[test]
    fn safe_session_id_keeps_a_safe_id() {
        for id in ["abc-123", "a", &"a".repeat(64), "0", "A-0-z"] {
            assert_eq!(safe_session_id(id.to_owned()), id, "{id:?} was emptied");
        }
    }

    #[test]
    fn safe_session_id_empties_anything_else() {
        for id in [
            "",
            "a\"b",
            "a\\b",
            "</script>",
            "a b",
            "ab\n",
            "caf\u{e9}",
            &"a".repeat(65),
        ] {
            assert_eq!(safe_session_id(id.to_owned()), "", "{id:?} was kept");
        }
    }

    #[test]
    fn safe_sequence_keeps_a_positive_number() {
        for sequence in [1, 2, i32::MAX] {
            assert_eq!(safe_sequence(sequence), sequence);
        }
    }

    #[test]
    fn safe_sequence_replaces_anything_else_with_one() {
        for sequence in [0, -1, i32::MIN] {
            assert_eq!(safe_sequence(sequence), 1);
        }
    }

    #[test]
    fn rendered_session_id_and_sequence_come_from_evidence() {
        let data = flow_data_with(&[("query.session-id", "abc-123"), ("query.sequence", "7")]);
        assert_eq!(
            JavaScriptBuilderElement::rendered_session_id(&data),
            "abc-123"
        );
        assert_eq!(JavaScriptBuilderElement::rendered_sequence(&data), 7);
    }

    #[test]
    fn rendered_session_id_and_sequence_are_safe() {
        let data = flow_data_with(&[("query.session-id", "a\"b"), ("query.sequence", "-1")]);
        assert_eq!(JavaScriptBuilderElement::rendered_session_id(&data), "");
        assert_eq!(JavaScriptBuilderElement::rendered_sequence(&data), 1);
    }

    #[test]
    fn build_url_adds_single_slash() {
        let url = JavaScriptBuilderElement::build_url("https", "example.com", "51dpipeline/json");
        assert_eq!(url.as_deref(), Some("https://example.com/51dpipeline/json"));
    }

    #[test]
    fn build_url_keeps_single_slash_on_endpoint() {
        let url = JavaScriptBuilderElement::build_url("https", "example.com", "/51dpipeline/json");
        assert_eq!(url.as_deref(), Some("https://example.com/51dpipeline/json"));
    }

    #[test]
    fn build_url_collapses_double_slash() {
        let url = JavaScriptBuilderElement::build_url("http", "example.com/", "/json");
        assert_eq!(url.as_deref(), Some("http://example.com/json"));
    }

    #[test]
    fn build_url_none_when_host_missing() {
        assert!(JavaScriptBuilderElement::build_url("https", "", "/json").is_none());
        assert!(JavaScriptBuilderElement::build_url("", "example.com", "/json").is_none());
        assert!(JavaScriptBuilderElement::build_url("https", "example.com", "").is_none());
    }

    #[test]
    fn parameters_exclude_session_and_sequence() {
        let data = flow_data_with(&[
            ("query.session-id", "abc"),
            ("query.sequence", "3"),
            ("query.user-agent", "test agent"),
            ("query.fod-js-object-name", "myObj"),
            ("header.host", "ignored"),
        ]);
        let json = JavaScriptBuilderElement::build_parameters(&data);
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let object = value.as_object().expect("an object");

        // Session id and sequence are excluded.
        assert!(!object.contains_key("session-id"));
        assert!(!object.contains_key("sequence"));
        // header.* evidence is excluded (only query.* is taken).
        assert!(!object.values().any(|v| v == "ignored"));
        // The remaining query.* keys are present with the prefix stripped, and
        // the space in the value is URL-encoded as %20 (not '+').
        assert_eq!(
            object.get("user-agent").and_then(|v| v.as_str()),
            Some("test%20agent")
        );
        assert_eq!(
            object.get("fod-js-object-name").and_then(|v| v.as_str()),
            Some("myObj")
        );
    }

    #[test]
    fn parameters_are_deterministic() {
        let data = flow_data_with(&[("query.b", "2"), ("query.a", "1"), ("query.c", "3")]);
        let first = JavaScriptBuilderElement::build_parameters(&data);
        let second = JavaScriptBuilderElement::build_parameters(&data);
        assert_eq!(first, second);
        // Keys are sorted, so 'a' precedes 'b' precedes 'c'.
        let a = first.find("\"a\"").unwrap();
        let b = first.find("\"b\"").unwrap();
        let c = first.find("\"c\"").unwrap();
        assert!(a < b && b < c);
    }

    #[test]
    fn protocol_falls_back_to_https() {
        let element = JavaScriptBuilderElement::new();
        let data = flow_data_with(&[("header.host", "example.com")]);
        assert_eq!(element.resolve_protocol(&data), "https");
    }

    #[test]
    fn protocol_taken_from_evidence_when_not_configured() {
        let element = JavaScriptBuilderElement::new();
        let data = flow_data_with(&[("header.protocol", "http")]);
        assert_eq!(element.resolve_protocol(&data), "http");
    }

    #[test]
    fn object_name_overridden_by_evidence() {
        let element = JavaScriptBuilderElement::new();
        let data = flow_data_with(&[("query.fod-js-object-name", "custom")]);
        assert_eq!(element.resolve_object_name(&data), "custom");
        let data = flow_data_with(&[]);
        assert_eq!(element.resolve_object_name(&data), "fod");
    }

    #[test]
    fn invalid_object_name_from_evidence_is_ignored() {
        let element = JavaScriptBuilderElement::builder()
            .set_object_name("configured")
            .unwrap()
            .build();
        for name in [
            "a;b",
            "9bad",
            "x\"y",
            "",
            "class",
            "Infinity",
            "NaN",
            "undefined",
            "fiftyoneDegreesManager",
        ] {
            let data = flow_data_with(&[("query.fod-js-object-name", name)]);
            assert_eq!(element.resolve_object_name(&data), "configured");
        }
    }

    #[test]
    fn enable_cookies_overridden_by_evidence() {
        let element = JavaScriptBuilderElement::new();
        let data = flow_data_with(&[("query.fod-js-enable-cookies", "false")]);
        assert!(!element.resolve_enable_cookies(&data));
        // Default is true when no evidence is supplied.
        let data = flow_data_with(&[]);
        assert!(element.resolve_enable_cookies(&data));
    }

    #[test]
    fn promise_and_fetch_are_unsupported_without_a_device_answer() {
        // No device element is present, so neither property resolves.
        let data = flow_data_with(&[]);
        assert!(!JavaScriptBuilderElement::supports_promises(&data));
        assert!(!JavaScriptBuilderElement::supports_fetch(&data));
    }
}
