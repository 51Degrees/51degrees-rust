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

use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use fiftyone_json_builder::JSON_BUILDER_DATA_KEY;
use fiftyone_pipeline_core::constants::EVIDENCE_PROTOCOL_KEY;
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
    PropertyValue, PropertyValueType, Result,
};

use crate::builder::{
    grants_51did, is_valid_object_name, offers_51did, SharedEngine, UserPromptSource,
};
use crate::callback_url::{callback_url, CallbackUrl};
use crate::constants::{
    DELAY_EXECUTION_MARKER, EVIDENCE_ENABLE_COOKIES, EVIDENCE_HOST_KEY, EVIDENCE_OBJECT_NAME,
    FALLBACK_PROTOCOL, FETCH_PROPERTY, JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY,
    JAVASCRIPT_PROPERTY_KEY, PROMISE_FULL_VALUE, PROMISE_PROPERTY,
};
use crate::data::{JavaScriptBuilderElementData, JAVASCRIPT_BUILDER_DATA_KEY};
use crate::minify::minify;
use crate::mustache::Template;
use crate::steps::{JavaScriptBuilderContext, SharedSteps};
use crate::template_data::JavaScriptResource;
use crate::JavaScriptBuilderElementBuilder;

/// The Mustache template, embedded at compile time.
const TEMPLATE_SOURCE: &str = include_str!("../assets/JavaScriptResource.mustache");

/// How an element decides whether the script carries the user prompt
/// section.
enum UserPrompt {
    /// From the pipeline each script is built for, read for each script.
    Pipeline,
    /// The same answer for every script.
    Decided(bool),
    /// From the request engine's licensed products, kept once they are known.
    Engine {
        engine: SharedEngine,
        settled: OnceLock<bool>,
    },
}

/// The request parameters as the JSON object the script is given, each name
/// and value written as a JSON string in the order given.
fn parameters_json(parameters: &[(String, String)]) -> String {
    let members: Vec<String> = parameters
        .iter()
        .map(|(name, value)| format!("{}:{}", json_string(name), json_string(value)))
        .collect();
    format!("{{{}}}", members.join(","))
}

/// A JSON string as Newtonsoft.Json writes it with its default settings,
/// which is serde_json's form with U+0085, U+2028 and U+2029 also written as
/// `\u` escapes. Those three can only appear inside a string, so replacing
/// them after serde_json has written the string changes nothing else.
fn json_string(text: &str) -> String {
    serde_json::Value::from(text)
        .to_string()
        .replace('\u{85}', "\\u0085")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Read a boolean as .NET's `bool.TryParse` does. `true` or `false` in any
/// case, once white space and NUL characters are trimmed from both ends.
/// White space is the Unicode White_Space property in both languages.
fn parse_boolean(text: &str) -> Option<bool> {
    let text = text.trim_matches(|c: char| c.is_whitespace() || c == '\0');
    if text.eq_ignore_ascii_case("true") {
        Some(true)
    } else if text.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

/// Generates a JavaScript include to be run on the client device.
///
/// The element renders the bundled Mustache template with the JSON payload
/// produced by the JSON builder, the session id and sequence number (from the
/// Sequence element's data, or an empty session id and sequence `1` when no
/// Sequence element ran), a callback URL and the request parameters, then
/// optionally minifies the result. The generated JavaScript is stored on the
/// flow data under the
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
/// - **enable cookies**: the `query.fod-js-enable-cookies` evidence when it
///   is `true` or `false` in any case, with white space and NUL characters
///   around it ignored, as .NET's `bool.TryParse` reads it, else the
///   configured default (true).
/// - **callback URL**: built only when protocol, host and endpoint are all
///   present, normalizing the single slash between host and endpoint. A URL
///   that is not valid is left out and logged at error level the first time,
///   and characters after the host and in the endpoint that could end the
///   string the script holds the URL in are percent-encoded. The protocol
///   must be `http` or `https`, ignoring case. A host holding `@` anywhere,
///   a zone identifier, `?` or `#` is not valid, and nor is one with a `:`
///   and no port after it, or one whose last label is a number unless it is
///   an IPv4 address. The log shows `***` in place of anything in a refused
///   host value before its last `@`, even after the host's first `/`, so a
///   user name or password is never logged. When a URL is built the
///   background-update mechanism is enabled.
/// - **parameters**: every `query.*` evidence entry except the session id and
///   sequence, with the prefix stripped and the key and value encoded as
///   .NET's `WebUtility.UrlEncode` encodes them, in name order, serialized as
///   a JSON object as Newtonsoft.Json writes one.
/// - **session id**: the session id from the Sequence element's data when it
///   is 1 to 64 ASCII letters, digits and hyphens, else an empty string,
///   because the template writes it inside quotes without any escaping. An
///   empty string when no Sequence element ran.
/// - **sequence**: the sequence from the Sequence element's data when it is a
///   positive 32 bit integer, else `1`, because the template writes it as
///   bare code. `1` when no Sequence element ran.
/// - **has delayed properties**: true when the JSON payload contains the
///   `delayexecution` marker.
/// - **supports promises**: true when this request's device-detection
///   `Promise` property is `Full`.
/// - **supports fetch**: true when this request's device-detection `Fetch`
///   property is true.
/// - **user prompt**: by default, true when the pipeline has an element whose
///   data key is `fodid`, ignoring case, whose properties are loaded and
///   include at least one available property, as .NET decides. A builder given
///   [`JavaScriptBuilderElementBuilder::set_user_prompt`] uses that answer
///   instead. A builder given a cloud request engine, through
///   [`JavaScriptBuilderElementBuilder::set_shared_cloud_request_engine`] or
///   [`JavaScriptBuilderElementBuilder::set_cloud_request_engine`], renders
///   the section when the engine's licensed products include 51Did, and
///   leaves it out while the shared engine's discovery has not completed.
///   The section sits inside the update section, so it is only rendered when
///   a callback URL was built as well.
///
/// The parameters, session id, sequence and user prompt rules are the
/// defaults of [`JavaScriptBuilderSteps`](crate::JavaScriptBuilderSteps),
/// which a caller can replace through
/// [`JavaScriptBuilderElementBuilder::set_steps`]. A session id or sequence
/// that a replaced step gives is made safe in the same way. A caller can also
/// give the template the script is rendered from, through
/// [`JavaScriptBuilderElementBuilder::set_template`].
///
/// # Example
///
/// ```
/// use std::sync::Arc;
/// use fiftyone_pipeline_core::{Evidence, Pipeline};
/// use fiftyone_json_builder::JsonBuilderElement;
/// use fiftyone_javascript_builder::{JavaScriptBuilderElement, JAVASCRIPT_BUILDER_DATA_KEY};
/// use fiftyone_pipeline_engines_fiftyone::SequenceElement;
///
/// // The JSON builder needs the sequence number the Sequence element gives.
/// let pipeline = Pipeline::builder()
///     .add_element(Arc::new(SequenceElement::new()))
///     .add_element(Arc::new(JsonBuilderElement::new()))
///     .add_element(Arc::new(JavaScriptBuilderElement::new()))
///     .build()
///     .unwrap();
///
/// let mut data = pipeline.create_flow_data_with(
///     Evidence::builder().add("header.host", "localhost").build(),
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
    /// How the element decides whether the script carries the user prompt
    /// section.
    user_prompt: UserPrompt,
    /// Set once a callback URL that is not valid has been logged, because a
    /// misconfigured element would otherwise log on every request.
    invalid_url_logged: AtomicBool,
    /// Set once a minification failure has been logged, for the same reason.
    minify_error_logged: AtomicBool,
    /// The steps that give the parameters, session id, sequence and user
    /// prompt decision.
    steps: SharedSteps,
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

    /// The JavaScript this element produces for the flow data around the given
    /// JSON, without adding it to the flow data.
    ///
    /// The client-side JavaScript endpoint serves this when the element left
    /// no data for a request, with `json` being the JSON builder's output for
    /// the same request.
    pub fn javascript_for(&self, data: &FlowData, json: &str) -> String {
        self.build_javascript(data, json.to_owned())
    }

    /// The host the script calls back to, as configured. Empty means the
    /// `header.host` evidence of each request is used.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The endpoint the script calls back to on the host, as configured.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// The protocol the script calls back with, as configured. Empty means
    /// the `header.protocol` evidence of each request is used, else `https`.
    pub fn protocol(&self) -> &str {
        &self.protocol
    }

    /// The name of the object the script creates, as configured. A request
    /// can ask for another valid name through the `query.fod-js-object-name`
    /// evidence.
    pub fn object_name(&self) -> &str {
        &self.object_name
    }

    /// Whether the script stores results in cookies, as configured. A request
    /// can ask otherwise through the `query.fod-js-enable-cookies` evidence.
    pub fn enable_cookies(&self) -> bool {
        self.enable_cookies
    }

    /// Whether the script is minified, as configured. It only is when the
    /// crate's `minify` feature is enabled.
    pub fn minify(&self) -> bool {
        self.minify
    }

    /// Internal constructor used by the builder.
    pub(crate) fn from_builder(builder: JavaScriptBuilderElementBuilder) -> Self {
        // The template is parsed once at construction. The embedded source is a
        // valid Mustache template, so parsing it cannot fail in practice. The
        // expect documents that invariant. A template given to the builder was
        // parsed when it was given.
        let template = builder.template.unwrap_or_else(|| {
            Template::parse(TEMPLATE_SOURCE)
                .expect("the embedded JavaScriptResource template is valid Mustache")
        });
        let user_prompt = match builder.user_prompt {
            UserPromptSource::Pipeline => UserPrompt::Pipeline,
            UserPromptSource::Decided(render) => UserPrompt::Decided(render),
            UserPromptSource::Engine(engine) => UserPrompt::Engine {
                engine,
                settled: OnceLock::new(),
            },
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
            host: builder.host,
            endpoint: builder.endpoint,
            protocol: builder.protocol,
            object_name: builder.object_name,
            enable_cookies: builder.enable_cookies,
            minify: builder.minify,
            user_prompt,
            invalid_url_logged: AtomicBool::new(false),
            minify_error_logged: AtomicBool::new(false),
            steps: builder.steps,
        }
    }

    /// Whether the script for this flow data carries the user prompt section,
    /// by the default rule. With a shared request engine the answer is kept
    /// once the engine's licensed products are known, and until then the
    /// section is left out without keeping that answer.
    pub(crate) fn user_prompt(&self, data: &FlowData) -> bool {
        match &self.user_prompt {
            UserPrompt::Pipeline => offers_51did(data.pipeline()),
            UserPrompt::Decided(render) => *render,
            UserPrompt::Engine { engine, settled } => match settled.get() {
                Some(&render) => render,
                None => match engine.public_properties() {
                    Ok(products) => *settled.get_or_init(|| grants_51did(products)),
                    Err(_) => false,
                },
            },
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
    /// read as .NET's `bool.TryParse` reads it, else the configured default.
    /// Blank or other text gives the configured default, as the specification
    /// and .NET do.
    fn resolve_enable_cookies(&self, data: &FlowData) -> bool {
        data.evidence()
            .get(EVIDENCE_ENABLE_COOKIES)
            .and_then(parse_boolean)
            .unwrap_or(self.enable_cookies)
    }

    /// The callback URL, or `None` when a part is missing or the URL is not
    /// valid. A URL that is not valid is logged at error level the first time
    /// only, with anything in the host value before its last `@` replaced by
    /// `***`.
    fn build_url(&self, protocol: &str, host: &str) -> Option<String> {
        match callback_url(protocol, host, &self.endpoint) {
            CallbackUrl::Valid(url) => Some(url),
            CallbackUrl::Missing => None,
            CallbackUrl::Invalid(url) => {
                if !self.invalid_url_logged.swap(true, Ordering::Relaxed) {
                    log::error!(
                        "The generated URL {url:?} is not valid, so the JavaScript has no \
                         callback URL. Check the JavaScript builder's protocol, host and \
                         endpoint, and the protocol and host evidence. This is reported once."
                    );
                }
                None
            }
        }
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

    /// Render and (optionally) minify the JavaScript for this request around
    /// the given JSON. When minification fails the unminified script is
    /// returned.
    fn build_javascript(&self, data: &FlowData, json_object: String) -> String {
        let protocol = self.resolve_protocol(data);
        let host = self.resolve_host(data);
        let object_name = self.resolve_object_name(data);
        let enable_cookies = self.resolve_enable_cookies(data);

        let supports_promises = Self::supports_promises(data);
        let supports_fetch = Self::supports_fetch(data);

        // The steps are called in the order .NET's builder calls the methods
        // they match.
        let context = JavaScriptBuilderContext::new(self, data);
        let steps = self.steps.get();
        let parameters = parameters_json(&steps.get_parameters(&context));
        let session_id = steps.get_session_id(&context);
        let sequence = steps.get_sequence(&context);

        let url = self.build_url(&protocol, &host);
        let update_enabled = url.as_ref().is_some_and(|u| !u.is_empty());

        let has_delayed_properties = json_object.contains(DELAY_EXECUTION_MARKER);
        let user_prompt = steps.render_user_prompt(&context);

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
            user_prompt,
        );

        let content = resource.render(&self.template);

        let outcome = if self.minify {
            minify(content)
        } else {
            crate::minify::MinifyOutcome {
                content,
                errors: Vec::new(),
            }
        };
        if !outcome.errors.is_empty() && !self.minify_error_logged.swap(true, Ordering::Relaxed) {
            log::error!(
                "Errors occurred when minifying the JavaScript, so it is served unminified. \
                 This is reported once. {}",
                outcome.errors.join(" ")
            );
        }

        outcome.content
    }
}

impl Default for JavaScriptBuilderElement {
    fn default() -> Self {
        JavaScriptBuilderElement::new()
    }
}

impl FlowElement for JavaScriptBuilderElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let content = self.build_javascript(data, Self::json_object(data));

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

    /// The builder itself, so the web integration can hold one the
    /// application gives and build a response again with it.
    fn into_any(self: Arc<Self>) -> Option<Arc<dyn Any + Send + Sync>> {
        Some(self)
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

    /// A name and value pair, as a step gives it.
    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    #[test]
    fn parameters_are_written_as_a_json_object_in_the_order_given() {
        assert_eq!(parameters_json(&[]), "{}");
        assert_eq!(
            parameters_json(&[pair("b", "2"), pair("a", "1")]),
            r#"{"b":"2","a":"1"}"#
        );
    }

    #[test]
    fn parameters_are_written_as_json_strings() {
        let json = parameters_json(&[pair("a\"b", "c\\d\n")]);
        assert_eq!(json, r#"{"a\"b":"c\\d\n"}"#);
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["a\"b"], "c\\d\n");
    }

    #[test]
    fn parameters_are_written_as_newtonsoft_writes_them() {
        // The text .NET's JsonConvert.SerializeObject gives for a dictionary
        // holding the same names and values, in the same order.
        let json = parameters_json(&[
            pair("nel", "\u{85}"),
            pair("ls", "\u{2028}"),
            pair("ps", "\u{2029}"),
            pair("quote", "a\"b\\c"),
            pair("control", "\u{8}\u{c}\n\r\t\u{1}\u{1f}\u{7f}"),
            pair("html", "<&'>/"),
            pair("high", "caf\u{e9}\u{20ac}"),
        ]);
        assert_eq!(
            json,
            "{\"nel\":\"\\u0085\",\"ls\":\"\\u2028\",\"ps\":\"\\u2029\",\
             \"quote\":\"a\\\"b\\\\c\",\
             \"control\":\"\\b\\f\\n\\r\\t\\u0001\\u001f\u{7f}\",\
             \"html\":\"<&'>/\",\"high\":\"caf\u{e9}\u{20ac}\"}"
        );
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["ls"], "\u{2028}");
    }

    #[test]
    fn the_default_parameters_are_encoded_as_dotnet_encodes_them() {
        let element = JavaScriptBuilderElement::new();
        let data = flow_data_with(&[
            ("query.b", "2 3"),
            ("query.a", "1"),
            ("query.sequence", "3"),
        ]);
        let context = JavaScriptBuilderContext::new(&element, &data);
        let parameters = crate::default_steps::get_parameters(&context);
        assert_eq!(parameters_json(&parameters), r#"{"a":"1","b":"2+3"}"#);
    }

    #[test]
    fn the_configured_values_can_be_read() {
        let element = JavaScriptBuilderElement::builder()
            .set_host("example.com")
            .set_endpoint("json")
            .set_protocol("http")
            .unwrap()
            .set_object_name("named")
            .unwrap()
            .set_enable_cookies(false)
            .build();
        assert_eq!(element.host(), "example.com");
        assert_eq!(element.endpoint(), "json");
        assert_eq!(element.protocol(), "http");
        assert_eq!(element.object_name(), "named");
        assert!(!element.enable_cookies());
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

    /// Global names a top level `var` cannot safely use as the object are
    /// refused in code and ignored from the evidence.
    #[test]
    fn unsafe_global_names_are_refused() {
        let element = JavaScriptBuilderElement::builder()
            .set_object_name("configured")
            .unwrap()
            .build();
        for name in [
            "eval",
            "arguments",
            "window",
            "document",
            "top",
            "name",
            "status",
            "history",
            "navigator",
            "closed",
        ] {
            assert!(
                JavaScriptBuilderElement::builder()
                    .set_object_name(name)
                    .is_err(),
                "'{name}' was accepted in code"
            );
            let data = flow_data_with(&[("query.fod-js-object-name", name)]);
            assert_eq!(
                element.resolve_object_name(&data),
                "configured",
                "'{name}' was taken from the evidence"
            );
        }
    }

    /// Names close to the unsafe global names, or differing only in case,
    /// are still accepted.
    #[test]
    fn names_near_the_unsafe_global_names_are_accepted() {
        for name in [
            "windowed",
            "names",
            "Window",
            "Eval",
            "evals",
            "argumentsList",
            "documents",
            "topmost",
            "myStatus",
            "_name",
            "$window",
            "history2",
            "Navigator",
            "closedAt",
        ] {
            assert!(is_valid_object_name(name), "'{name}' was refused");
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
    fn enable_cookies_evidence_is_read_as_dotnet_reads_it() {
        // Each value with what .NET's bool.TryParse gives for it, None being
        // a value it does not take, which leaves the configured default.
        let cases: [(&str, Option<bool>); 29] = [
            ("true", Some(true)),
            ("TRUE", Some(true)),
            ("True", Some(true)),
            ("tRuE", Some(true)),
            ("false", Some(false)),
            ("FALSE", Some(false)),
            ("False", Some(false)),
            (" false ", Some(false)),
            ("\tfalse\0", Some(false)),
            ("false\0", Some(false)),
            ("\0true", Some(true)),
            ("\u{3000}false", Some(false)),
            ("\u{85}true", Some(true)),
            ("\u{a0}true", Some(true)),
            ("\u{200b}false", None),
            ("\u{feff}false", None),
            ("", None),
            (" ", None),
            ("yes", None),
            ("no", None),
            ("1", None),
            ("0", None),
            ("on", None),
            ("t", None),
            ("f", None),
            ("falsey", None),
            ("true1", None),
            (" tru", None),
            ("tr ue", None),
        ];
        for configured in [true, false] {
            let element = JavaScriptBuilderElement::builder()
                .set_enable_cookies(configured)
                .build();
            for (value, parsed) in cases {
                let data = flow_data_with(&[("query.fod-js-enable-cookies", value)]);
                assert_eq!(
                    element.resolve_enable_cookies(&data),
                    parsed.unwrap_or(configured),
                    "{value:?} with {configured} configured"
                );
            }
        }
    }

    #[test]
    fn promise_and_fetch_are_unsupported_without_a_device_answer() {
        // No device element is present, so neither property resolves.
        let data = flow_data_with(&[]);
        assert!(!JavaScriptBuilderElement::supports_promises(&data));
        assert!(!JavaScriptBuilderElement::supports_fetch(&data));
    }
}
