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

//! Assembling a web-ready pipeline.
//!
//! [`WebPipeline`] is the framework-neutral web pipeline helper. Given the
//! application's own elements (device detection, IP intelligence, a cloud
//! request engine, and so on) and a set of
//! [`WebIntegrationOptions`], it inserts the elements the client-side endpoints
//! need and orders them as the
//! [web-integration specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/web-integration.md)
//! requires.
//!
//! # Element ordering
//!
//! The built pipeline runs:
//!
//! 1. [`fiftyone_pipeline_engines_fiftyone::SequenceElement`] first, so the
//!    session id and sequence number exist before anything else reads them.
//!    It is only added when the application's elements do not already hold
//!    one, so the elements of a pipeline builder with usage sharing on keep
//!    their own.
//! 2. the application's own elements, in the order they were added.
//! 3. [`fiftyone_pipeline_engines_fiftyone::SetHeadersElement`] (only when
//!    [`WebIntegrationOptions::use_set_header_properties`] is set), after the
//!    core elements so it can see every `SetHeader*` property they produced.
//! 4. [`fiftyone_json_builder::JsonBuilderElement`] as the penultimate element,
//!    so it serializes the results of everything before it.
//! 5. [`fiftyone_javascript_builder::JavaScriptBuilderElement`] last, because it
//!    wraps the JSON the JSON builder produced.
//!
//! When [`WebIntegrationOptions::client_side_evidence_enabled`] is `false` the
//! JSON and JavaScript builders are not added, because the client-side
//! endpoints are not in use.
//!
//! # Builders the application gives
//!
//! A JSON builder or JavaScript builder among the application's elements,
//! found by its data key ignoring case, is the one the pipeline uses, so its
//! exclusion lists, template or object name apply. It runs where the
//! application put it, and the builder of the same kind is not added. A JSON
//! builder added for a JavaScript builder the application gives goes
//! immediately before it.
//!
//! The pipeline holds each builder it runs as a resource, so an endpoint can
//! build a response again when a builder left no data for a request. A builder
//! the application gives is held when [`FlowElement::into_any`] gives back its
//! own type, as the JSON and JavaScript builders do. One that does not, such
//! as an element wrapping a builder, still runs, but an endpoint cannot build
//! its response again.
//!
//! # Endpoint matching
//!
//! [`WebPipeline::endpoint_for`] and [`endpoint_matches`] decide which endpoint
//! (if any) a request path targets. Matching is a case-insensitive `ends_with`
//! against the configured paths, so an application mounted under a sub-path
//! (for example `/app/51Degrees.core.js`) still routes correctly.

use std::any::Any;
use std::fmt;
use std::sync::{Arc, OnceLock};

use fiftyone_javascript_builder::{JavaScriptBuilderElement, JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY};
use fiftyone_json_builder::{JsonBuilderElement, JSON_BUILDER_ELEMENT_DATA_KEY};
use fiftyone_pipeline_core::constants::{
    DEFAULT_CORE_JS_ENDPOINT, DEFAULT_JSON_ENDPOINT, EVIDENCE_HTTP_HEADER_PREFIX,
};
use fiftyone_pipeline_core::{EvidenceKeyFilterWhitelist, FlowElement, Pipeline, Result};
use fiftyone_pipeline_engines_fiftyone::{SequenceElement, SetHeadersElement};

use crate::clientside::EndpointOptions;

/// The client-side endpoint a request path targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebEndpoint {
    /// The JavaScript include endpoint (default `/51Degrees.core.js`).
    JavaScript,
    /// The JSON callback endpoint (default `/51dpipeline/json`).
    Json,
}

/// Options that control how [`WebPipeline`] assembles a web pipeline.
///
/// The defaults follow the web-integration specification. Client-side evidence
/// is enabled (so the JSON and JavaScript builders are added) and the
/// set-headers element is enabled (so elements can request extra evidence
/// through response headers). The endpoint paths default to the
/// [`fiftyone_pipeline_core::constants`] values.
#[derive(Debug, Clone)]
pub struct WebIntegrationOptions {
    /// When `true` (the default) a JSON builder and a JavaScript builder are
    /// added, each only when the application's elements hold none, so the
    /// client-side endpoints have content to serve. Set it to `false` for a
    /// server-only pipeline with no client-side JavaScript. A builder among
    /// the application's elements runs either way.
    pub client_side_evidence_enabled: bool,

    /// When `true` (the default) the set-headers element is added so elements
    /// can request extra evidence (such as User-Agent Client Hints) via HTTP
    /// response headers.
    pub use_set_header_properties: bool,

    /// When `true` (the default) processing exceptions are recorded on the flow
    /// data rather than propagated, so a single failing element does not break
    /// the request. The web integration suppresses by default even though the
    /// bare pipeline default is not to.
    pub suppress_process_exceptions: bool,

    /// The path the JavaScript endpoint is served from. Defaults to
    /// [`fiftyone_pipeline_core::constants::DEFAULT_CORE_JS_ENDPOINT`].
    pub javascript_endpoint: String,

    /// The path the JSON endpoint is served from. Defaults to
    /// [`fiftyone_pipeline_core::constants::DEFAULT_JSON_ENDPOINT`].
    pub json_endpoint: String,
}

impl Default for WebIntegrationOptions {
    fn default() -> Self {
        WebIntegrationOptions {
            client_side_evidence_enabled: true,
            use_set_header_properties: true,
            suppress_process_exceptions: true,
            javascript_endpoint: DEFAULT_CORE_JS_ENDPOINT.to_owned(),
            json_endpoint: DEFAULT_JSON_ENDPOINT.to_owned(),
        }
    }
}

/// A built, web-ready pipeline together with the configuration the endpoints
/// need.
///
/// Construct one with [`WebPipeline::build`]. It owns the shared
/// [`Pipeline`], the resolved [`WebIntegrationOptions`] and the evidence-key
/// whitelist used to derive the `Vary` header. An adapter clones the `Arc` to
/// create flow data per request, reads the endpoint paths, and serves the
/// client-side endpoints with [`WebPipeline::endpoint_options`].
pub struct WebPipeline {
    pipeline: Arc<Pipeline>,
    options: WebIntegrationOptions,
    vary: Arc<VaryList>,
}

impl WebPipeline {
    /// Assemble a web pipeline from the application's elements and options.
    ///
    /// The supplied `elements` are the application's own elements in execution
    /// order (for example a cloud request engine then a device-detection cloud
    /// engine, or a single on-premise engine). The sequence, set-headers, JSON
    /// and JavaScript elements are inserted around them as described in the
    /// module documentation, and a JSON or JavaScript builder among them is
    /// used in place of the one that would be added. The returned pipeline uses
    /// [`WebIntegrationOptions::suppress_process_exceptions`].
    ///
    /// Returns the same errors as [`fiftyone_pipeline_core::PipelineBuilder::build`]
    /// (notably an empty pipeline).
    pub fn build(
        elements: Vec<Arc<dyn FlowElement>>,
        options: WebIntegrationOptions,
    ) -> Result<WebPipeline> {
        let (ordered, builders) = order_elements(elements, &options);

        // The Vary whitelist is the union of the header keys every element in
        // the pipeline names, shared with the endpoints.
        let vary = Arc::new(VaryList::new(ordered.clone()));

        let mut builder =
            Pipeline::builder().suppress_process_exceptions(options.suppress_process_exceptions);
        for element in ordered {
            builder = builder.add_element(element);
        }
        for resource in builders {
            builder = builder.add_resource(resource);
        }
        let pipeline = builder.build()?;

        Ok(WebPipeline {
            pipeline,
            options,
            vary,
        })
    }

    /// The shared, immutable pipeline. Clone the `Arc` to create flow data.
    pub fn pipeline(&self) -> &Arc<Pipeline> {
        &self.pipeline
    }

    /// The resolved web-integration options.
    pub fn options(&self) -> &WebIntegrationOptions {
        &self.options
    }

    /// The evidence-key whitelist used to derive the `Vary` header for the
    /// client-side endpoints.
    ///
    /// While an element is still learning the evidence it accepts, as the cloud
    /// request engine is after a transient discovery failure, this is the list
    /// collected when the pipeline was built. Once every element's filter is
    /// complete the list is collected again and kept. A copy taken earlier does
    /// not change, so serve the endpoints with
    /// [`WebPipeline::endpoint_options`] rather than a copy.
    pub fn vary_whitelist(&self) -> &EvidenceKeyFilterWhitelist {
        self.vary.whitelist()
    }

    /// The [`EndpointOptions`] for the client-side endpoints, taking the `Vary`
    /// header from [`WebPipeline::vary_whitelist`] at each response.
    pub fn endpoint_options(&self) -> EndpointOptions {
        EndpointOptions::from_vary_list(Arc::clone(&self.vary))
    }

    /// Decide which client-side endpoint, if any, a request path targets.
    ///
    /// The JavaScript path is checked first, then the JSON path, each by a
    /// case-insensitive suffix match against the configured endpoints. Returns
    /// `None` for any other path.
    pub fn endpoint_for(&self, path: &str) -> Option<WebEndpoint> {
        if endpoint_matches(path, &self.options.javascript_endpoint) {
            Some(WebEndpoint::JavaScript)
        } else if endpoint_matches(path, &self.options.json_endpoint) {
            Some(WebEndpoint::Json)
        } else {
            None
        }
    }
}

/// True if a request path targets the given endpoint path.
///
/// The match is a case-insensitive `ends_with`. This lets an application be
/// mounted under a prefix and still have the 51Degrees endpoints resolve, while
/// remaining exact at the suffix so unrelated paths do not match.
pub fn endpoint_matches(path: &str, endpoint: &str) -> bool {
    if endpoint.is_empty() {
        return false;
    }
    path.to_lowercase().ends_with(&endpoint.to_lowercase())
}

/// A resource for the pipeline to hold, which it finds again by type.
type Resource = Arc<dyn Any + Send + Sync>;

/// Insert the web elements around the application elements and return the full
/// ordered list, with the client-side builders it runs as resources for the
/// pipeline.
fn order_elements(
    elements: Vec<Arc<dyn FlowElement>>,
    options: &WebIntegrationOptions,
) -> (Vec<Arc<dyn FlowElement>>, Vec<Resource>) {
    let mut ordered: Vec<Arc<dyn FlowElement>> = Vec::with_capacity(elements.len() + 4);
    let mut resources: Vec<Resource> = Vec::new();

    // 1. Sequence element first, unless the application's elements already
    // hold one, as a pipeline builder's elements do with usage sharing on.
    let has_sequence = elements
        .iter()
        .any(|element| element.data_key() == SequenceElement::DEFAULT_ELEMENT_DATA_KEY);
    if !has_sequence {
        ordered.push(Arc::new(SequenceElement::new()));
    }

    // The builders the application gives run where it put them, and are held
    // when they give back their own type.
    let given_json = position(&elements, JSON_BUILDER_ELEMENT_DATA_KEY);
    let given_javascript = position(&elements, JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY);
    if let Some(json) =
        given_json.and_then(|index| given_builder::<JsonBuilderElement>(&elements[index]))
    {
        resources.push(json);
    }
    if let Some(javascript) = given_javascript
        .and_then(|index| given_builder::<JavaScriptBuilderElement>(&elements[index]))
    {
        resources.push(javascript);
    }

    // A builder is added only when the client-side endpoints are in use and
    // the application gives none of its kind.
    let client_side = options.client_side_evidence_enabled;
    let mut json =
        (client_side && given_json.is_none()).then(|| Arc::new(JsonBuilderElement::new()));
    let javascript = (client_side && given_javascript.is_none())
        .then(|| Arc::new(JavaScriptBuilderElement::new()));

    // 2. The application's own elements, in their given order, with an added
    // JSON builder immediately before a JavaScript builder the application
    // gives.
    for (index, element) in elements.into_iter().enumerate() {
        if Some(index) == given_javascript {
            if let Some(json) = json.take() {
                ordered.push(json.clone());
                resources.push(json);
            }
        }
        ordered.push(element);
    }

    // 3. Set-headers after the core elements (so it sees their properties).
    if options.use_set_header_properties {
        ordered.push(Arc::new(SetHeadersElement::new()));
    }

    // 4 and 5. An added JSON builder penultimate and an added JavaScript
    // builder last.
    if let Some(json) = json {
        ordered.push(json.clone());
        resources.push(json);
    }
    if let Some(javascript) = javascript {
        ordered.push(javascript.clone());
        resources.push(javascript);
    }

    (ordered, resources)
}

/// The index of the first of `elements` whose data key is `data_key`,
/// ignoring case, as the flow data keeps element data under the key in lower
/// case.
fn position(elements: &[Arc<dyn FlowElement>], data_key: &str) -> Option<usize> {
    let data_key = data_key.to_lowercase();
    elements
        .iter()
        .position(|element| element.data_key().to_lowercase() == data_key)
}

/// `element` as the builder `T`, when it gives back its own type through
/// [`FlowElement::into_any`] and that type is `T`.
fn given_builder<T: Any + Send + Sync>(element: &Arc<dyn FlowElement>) -> Option<Arc<T>> {
    Arc::clone(element).into_any()?.downcast::<T>().ok()
}

/// The `Vary` whitelist of a [`WebPipeline`], shared with the
/// [`EndpointOptions`] it gives out.
///
/// The cloud request engine specification says a consumer must not keep
/// accepted evidence obtained before discovery completed, so until every
/// element reports its filter complete the list collected at build is given
/// out. The first call after that collects the list again and keeps it.
pub(crate) struct VaryList {
    elements: Vec<Arc<dyn FlowElement>>,
    at_build: EvidenceKeyFilterWhitelist,
    complete: OnceLock<EvidenceKeyFilterWhitelist>,
}

impl VaryList {
    fn new(elements: Vec<Arc<dyn FlowElement>>) -> Self {
        let at_build = collect_vary_whitelist(&elements);
        VaryList {
            elements,
            at_build,
            complete: OnceLock::new(),
        }
    }

    /// The complete list once every element's filter is complete, otherwise
    /// the list collected at build.
    pub(crate) fn whitelist(&self) -> &EvidenceKeyFilterWhitelist {
        if let Some(complete) = self.complete.get() {
            return complete;
        }
        if self
            .elements
            .iter()
            .all(|element| element.is_evidence_key_filter_complete())
        {
            return self
                .complete
                .get_or_init(|| collect_vary_whitelist(&self.elements));
        }
        &self.at_build
    }
}

impl fmt::Debug for VaryList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VaryList")
            .field("at_build", &self.at_build)
            .field("complete", &self.complete.get())
            .finish_non_exhaustive()
    }
}

/// Build the union of the `header.*` keys the elements accept, for the `Vary`
/// header, as the specification asks: every header in the pipeline's evidence
/// key filter.
///
/// An element's filter lists the keys it names through
/// [`fiftyone_pipeline_core::EvidenceKeyFilter::named_keys`], so the engines
/// contribute the headers their data files define and the JavaScript builder
/// the host and protocol headers. A filter that decides key by key and has no
/// list is probed with the fixed set of headers the 51Degrees elements are
/// known to accept, so such an element still names the common headers.
fn collect_vary_whitelist(elements: &[Arc<dyn FlowElement>]) -> EvidenceKeyFilterWhitelist {
    let header_prefix = format!("{EVIDENCE_HTTP_HEADER_PREFIX}.");
    let mut keys: Vec<String> = Vec::new();

    for element in elements {
        let filter = element.evidence_key_filter();
        let named: Vec<String> = match filter.named_keys() {
            Some(named) => named
                .into_iter()
                .filter(|key| key.starts_with(&header_prefix))
                .collect(),
            None => CANDIDATE_HEADER_KEYS
                .iter()
                .filter(|candidate| filter.include(candidate))
                .map(|candidate| (*candidate).to_owned())
                .collect(),
        };
        for key in named {
            if !keys.iter().any(|k| k == &key) {
                keys.push(key);
            }
        }
    }

    EvidenceKeyFilterWhitelist::new(keys)
}

/// The HTTP-header evidence keys the standard 51Degrees elements may accept,
/// probed against a filter that cannot list its own keys when building the
/// `Vary` whitelist. It covers the User-Agent and the User-Agent Client Hint
/// headers device detection consumes, plus the host and protocol headers the
/// JavaScript builder reads.
const CANDIDATE_HEADER_KEYS: &[&str] = &[
    "header.user-agent",
    "header.host",
    "header.protocol",
    "header.sec-ch-ua",
    "header.sec-ch-ua-full-version",
    "header.sec-ch-ua-full-version-list",
    "header.sec-ch-ua-platform",
    "header.sec-ch-ua-platform-version",
    "header.sec-ch-ua-mobile",
    "header.sec-ch-ua-arch",
    "header.sec-ch-ua-model",
    "header.sec-ch-ua-bitness",
    "header.device-stock-ua",
];
