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

//! The builder for [`JavaScriptBuilderElement`].

use std::fmt;
use std::ops::Deref;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::sync::Arc;

use fiftyone_cloud_request_engine::{CloudRequestEngine, LicensedProducts};
use fiftyone_pipeline_core::constants::DEFAULT_JSON_ENDPOINT;
use fiftyone_pipeline_core::{Error, Pipeline, Result};

use crate::constants::{
    BUILDER_DEFAULT_ENABLE_COOKIES, BUILDER_DEFAULT_HOST, BUILDER_DEFAULT_MINIFY,
    BUILDER_DEFAULT_OBJECT_NAME, BUILDER_DEFAULT_PROTOCOL, FODID_PRODUCT_NAME,
};
use crate::element::JavaScriptBuilderElement;
use crate::mustache::Template;
use crate::steps::{DefaultJavaScriptBuilderSteps, JavaScriptBuilderSteps, SharedSteps};

/// Where an element's user prompt decision comes from.
#[derive(Clone)]
pub(crate) enum UserPromptSource {
    /// Decided from the pipeline each script is built for.
    Pipeline,
    /// Decided when the builder was configured.
    Decided(bool),
    /// Decided from this request engine's licensed products once its
    /// discovery has completed.
    Engine(SharedEngine),
}

/// A shared cloud request engine that keeps the builder and the element
/// `UnwindSafe` and `RefUnwindSafe`.
///
/// The JavaScript builder only reads the engine's licensed products, which
/// are set once, so a panic elsewhere cannot leave a value half changed for
/// it to read.
#[derive(Clone)]
pub(crate) struct SharedEngine(Arc<CloudRequestEngine>);

impl UnwindSafe for SharedEngine {}
impl RefUnwindSafe for SharedEngine {}

impl Deref for SharedEngine {
    type Target = CloudRequestEngine;

    fn deref(&self) -> &CloudRequestEngine {
        &self.0
    }
}

impl fmt::Debug for UserPromptSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UserPromptSource::Pipeline => f.write_str("Pipeline"),
            UserPromptSource::Decided(render) => f.debug_tuple("Decided").field(render).finish(),
            UserPromptSource::Engine(_) => f.write_str("Engine"),
        }
    }
}

/// Configures and constructs a [`JavaScriptBuilderElement`].
///
/// The defaults are minification
/// on, cookies enabled, object name `fod`, the default JSON endpoint, and an
/// empty host and protocol so the values from request evidence are used.
///
/// # Example
///
/// ```
/// use fiftyone_javascript_builder::JavaScriptBuilderElement;
///
/// let element = JavaScriptBuilderElement::builder()
///     .set_object_name("myObj").unwrap()
///     .set_protocol("https").unwrap()
///     .set_minify(false)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct JavaScriptBuilderElementBuilder {
    pub(crate) host: String,
    pub(crate) endpoint: String,
    pub(crate) protocol: String,
    pub(crate) object_name: String,
    pub(crate) enable_cookies: bool,
    pub(crate) minify: bool,
    pub(crate) user_prompt: UserPromptSource,
    pub(crate) steps: SharedSteps,
    /// The template given by the caller, or `None` for the bundled one.
    pub(crate) template: Option<Template>,
}

impl JavaScriptBuilderElementBuilder {
    /// Start a builder pre-populated with the defaults.
    pub fn new() -> Self {
        JavaScriptBuilderElementBuilder {
            host: BUILDER_DEFAULT_HOST.to_owned(),
            endpoint: DEFAULT_JSON_ENDPOINT.to_owned(),
            protocol: BUILDER_DEFAULT_PROTOCOL.to_owned(),
            object_name: BUILDER_DEFAULT_OBJECT_NAME.to_owned(),
            enable_cookies: BUILDER_DEFAULT_ENABLE_COOKIES,
            minify: BUILDER_DEFAULT_MINIFY,
            user_prompt: UserPromptSource::Pipeline,
            steps: SharedSteps::new(DefaultJavaScriptBuilderSteps),
            template: None,
        }
    }

    /// Use these steps to give the parameters, session id, sequence and user
    /// prompt decision in place of the builder's own. See
    /// [`JavaScriptBuilderSteps`].
    pub fn set_steps(mut self, steps: impl JavaScriptBuilderSteps + 'static) -> Self {
        self.steps = SharedSteps::new(steps);
        self
    }

    /// Render the script from this Mustache template in place of the bundled
    /// one.
    ///
    /// The template is given the values the bundled one is given, named in
    /// the
    /// [javascript-builder specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/javascript-builder.md),
    /// with `_supportsFetch` and `_userPrompt` as well. Variables
    /// (`{{name}}`, `{{&name}}` and `{{{name}}}`) are written without HTML
    /// escaping, and sections (`{{#name}}` and `{{^name}}`) are rendered from
    /// the boolean values. Lists, partials, lambdas, dotted names and
    /// comments are not supported. The specification allows a template other
    /// than the bundled one as long as the script still does everything the
    /// bundled one does.
    ///
    /// A template with a tag or a section left open, or with a section closed
    /// under another name, is a configuration error.
    pub fn set_template(mut self, source: &str) -> Result<Self> {
        match Template::parse(source) {
            Ok(template) => {
                self.template = Some(template);
                Ok(self)
            }
            Err(error) => Err(Error::configuration(format!(
                "The JavaScript template is not valid Mustache ({error})."
            ))),
        }
    }

    /// Decide whether the script carries the user prompt section from the
    /// licensed products of the cloud request engine that serves this
    /// pipeline.
    ///
    /// The section gathers the visitor's answer that a 51Did is created from,
    /// so it is rendered only when the engine's resource key is licensed for
    /// 51Did, meaning its licensed products include the 51Did product with at
    /// least one property. The decision is taken from the licensed products and
    /// not from the 51Did cloud engine's property list, because that engine
    /// reports its default properties when the key grants no 51Did product, so
    /// its list is populated either way.
    ///
    /// The element keeps the engine and settles the decision at the first
    /// render after the engine's discovery has completed. Until then each
    /// script leaves the section out, so a transient failure when the engine
    /// was built leaves it out only until a request completes discovery.
    ///
    /// The last call to this method,
    /// [`set_cloud_request_engine`](Self::set_cloud_request_engine) or
    /// [`set_user_prompt`](Self::set_user_prompt) decides. Without any of the
    /// three, the pipeline decides, as [`set_user_prompt`](Self::set_user_prompt)
    /// describes.
    pub fn set_shared_cloud_request_engine(mut self, engine: Arc<CloudRequestEngine>) -> Self {
        self.user_prompt = UserPromptSource::Engine(SharedEngine(engine));
        self
    }

    /// As
    /// [`set_shared_cloud_request_engine`](Self::set_shared_cloud_request_engine)
    /// for a caller holding only a reference, so the decision is taken here,
    /// once. If the engine's discovery has not completed, which follows a
    /// transient failure when it was built, the section is left out for the
    /// life of the element and a warning is logged. Prefer the shared form.
    pub fn set_cloud_request_engine(mut self, engine: &CloudRequestEngine) -> Self {
        let render = match engine.public_properties() {
            Ok(products) => grants_51did(products),
            Err(_) => {
                log::warn!(
                    "The cloud request engine's licensed products are not known \
                     yet, so the JavaScript builder leaves the user prompt \
                     section out of every script. Pass the engine to \
                     set_shared_cloud_request_engine to decide once they are known."
                );
                false
            }
        };
        self.user_prompt = UserPromptSource::Decided(render);
        self
    }

    /// Render the user prompt section in every script, or leave it out of
    /// every script, whatever the pipeline holds. This needs no cloud request
    /// engine.
    ///
    /// The last call to this method,
    /// [`set_shared_cloud_request_engine`](Self::set_shared_cloud_request_engine)
    /// or [`set_cloud_request_engine`](Self::set_cloud_request_engine)
    /// decides. Without any of the three, the section is rendered when the
    /// pipeline the script is built for has an element whose data key is
    /// `fodid`, ignoring case, offering at least one available property, as
    /// .NET's builder decides. An element whose properties are not loaded yet
    /// offers nothing
    /// ([`FlowElement::properties_loaded`](fiftyone_pipeline_core::FlowElement::properties_loaded)),
    /// so a script built before the 51Did cloud engine's discovery completes
    /// leaves the section out. The pipeline is read for each script and the
    /// answer is not kept, so a script built once discovery completes has the
    /// section.
    ///
    /// A cloud request engine with only a license key has no accessible
    /// properties to read, so the 51Did cloud engine offers no available
    /// property and the section is left out, whatever the license grants. A
    /// pipeline on a license key that needs the section calls this method with
    /// `true`. .NET has no answer to follow here, because its cloud request
    /// engine refuses to build without a resource key.
    pub fn set_user_prompt(mut self, render: bool) -> Self {
        self.user_prompt = UserPromptSource::Decided(render);
        self
    }

    /// Set whether client-side processing stores results in cookies.
    ///
    /// This can also be set per request through the
    /// `query.fod-js-enable-cookies` evidence key.
    pub fn set_enable_cookies(mut self, enable_cookies: bool) -> Self {
        self.enable_cookies = enable_cookies;
        self
    }

    /// Set the host the client JavaScript should query for updates, without
    /// the scheme, for example `example.com` or `example.com:8080`. By default
    /// the host from the request evidence is used.
    pub fn set_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Set the endpoint queried on the host, for example `/api/v4/json`.
    pub fn set_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// Set the protocol the client JavaScript uses when querying for updates.
    ///
    /// Only `http` or `https` (case-insensitive) are accepted. Any other value
    /// is a configuration error.
    pub fn set_protocol(mut self, protocol: impl Into<String>) -> Result<Self> {
        let protocol = protocol.into();
        if protocol.eq_ignore_ascii_case("http") || protocol.eq_ignore_ascii_case("https") {
            self.protocol = protocol;
            Ok(self)
        } else {
            Err(Error::configuration(format!(
                "Invalid protocol in configuration ({protocol}), must be 'http' or 'https'"
            )))
        }
    }

    /// Set the default name of the object instantiated by the client
    /// JavaScript.
    ///
    /// The name must be a valid JavaScript identifier (it must match
    /// `[a-zA-Z_$][0-9a-zA-Z_$]*` in full) and must not be a name the
    /// [object name rule](https://github.com/51Degrees/specifications/blob/88dda87fbd5a5842c664991cdeaed970193a90f6/pipeline-specification/pipeline-elements/javascript-builder.md#L144-L182)
    /// refuses. Those are the reserved words, `Infinity`, `NaN`, `undefined`,
    /// `fiftyoneDegreesManager`, and global names a page cannot use for the
    /// object, such as `eval`, `window`, `location` and `localStorage`. An
    /// invalid name is a configuration error. The same rule applies to a name
    /// requested through the `query.fod-js-object-name` evidence, where an
    /// invalid name is ignored with a warning logged.
    pub fn set_object_name(mut self, object_name: impl Into<String>) -> Result<Self> {
        let object_name = object_name.into();
        if is_valid_object_name(&object_name) {
            self.object_name = object_name;
            Ok(self)
        } else {
            let message = format!(
                "The JavaScript object name '{object_name}' is not valid. It must \
                 be a valid JavaScript identifier that is not a reserved word."
            );
            log::error!("{message}");
            Err(Error::configuration(message))
        }
    }

    /// Enable or disable minification of the generated JavaScript.
    ///
    /// Minification only takes effect when the crate's `minify` feature is
    /// enabled (it is on by default). With the feature disabled this flag is
    /// retained but has no effect.
    pub fn set_minify(mut self, minify: bool) -> Self {
        self.minify = minify;
        self
    }

    /// Build the configured [`JavaScriptBuilderElement`].
    pub fn build(self) -> JavaScriptBuilderElement {
        JavaScriptBuilderElement::from_builder(self)
    }
}

/// True when the licensed products include the 51Did product with at least one
/// property. The product name is compared ignoring case, because the cloud
/// service reports it as `FODid`.
pub(crate) fn grants_51did(products: &LicensedProducts) -> bool {
    products.products.iter().any(|(name, product)| {
        name.eq_ignore_ascii_case(FODID_PRODUCT_NAME) && !product.properties.is_empty()
    })
}

/// True when the pipeline has an element whose data key is the 51Did key,
/// ignoring case, whose properties are loaded and include at least one
/// available property. The elements of a parallel group are included.
pub(crate) fn offers_51did(pipeline: &Pipeline) -> bool {
    pipeline.flow_elements().iter().any(|element| {
        element.data_key().eq_ignore_ascii_case(FODID_PRODUCT_NAME)
            && element.properties_loaded()
            && element
                .properties()
                .iter()
                .any(|property| property.available)
    })
}

impl Default for JavaScriptBuilderElementBuilder {
    fn default() -> Self {
        JavaScriptBuilderElementBuilder::new()
    }
}

/// Names the object cannot have. These are the reserved words of the language,
/// including those reserved only in strict mode and the literals `null`,
/// `true` and `false`, plus the three global values a top level `var` cannot
/// replace, so the object would silently never be created, and the name of
/// the constructor the script itself defines.
#[rustfmt::skip]
const RESERVED_WORDS: &[&str] = &[
    "await", "break", "case", "catch", "class", "const", "continue", "debugger",
    "default", "delete", "do", "else", "enum", "export", "extends", "false",
    "finally", "for", "function", "if", "implements", "import", "in",
    "instanceof", "interface", "let", "new", "null", "package", "private",
    "protected", "public", "return", "static", "super", "switch", "this",
    "throw", "true", "try", "typeof", "var", "void", "while", "with", "yield",
    "Infinity", "NaN", "undefined", "fiftyoneDegreesManager",
];

/// Global names the object cannot have, beyond [`RESERVED_WORDS`]. Strict
/// mode code cannot declare `eval` or `arguments`, and the script's top level
/// `var eval` would replace the page's own `eval`. `var location` sets
/// `location.href` to the object's text, which navigates the page away. The
/// rest are properties of the browser's window that a top level `var` cannot
/// replace, so the object would never be created. These are the names the
/// [object name rule](https://github.com/51Degrees/specifications/blob/88dda87fbd5a5842c664991cdeaed970193a90f6/pipeline-specification/pipeline-elements/javascript-builder.md#L144-L182)
/// adds to the reserved words.
#[rustfmt::skip]
const UNSAFE_GLOBAL_NAMES: &[&str] = &[
    "eval", "arguments", "location",
    "window", "document", "top", "name", "status", "history", "navigator",
    "closed", "frameElement", "isSecureContext", "crossOriginIsolated",
    "trustedTypes", "crypto", "indexedDB", "localStorage", "sessionStorage",
    "caches", "cookieStore", "speechSynthesis", "styleMedia",
];

/// True if the string is a valid JavaScript identifier per the
/// `[a-zA-Z_$][0-9a-zA-Z_$]*` rule and is not one of [`RESERVED_WORDS`] or
/// [`UNSAFE_GLOBAL_NAMES`].
///
/// The first character must be a letter, underscore or dollar sign, and the
/// rest may also be digits. An empty string is invalid. The name is written
/// into the script as a variable name, a session storage key and a property
/// name without any escaping, so a name outside this rule would break the
/// script or change what it does.
pub(crate) fn is_valid_object_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if is_identifier_start(first) => {}
        _ => return false,
    }
    chars.all(is_identifier_part)
        && !RESERVED_WORDS.contains(&name)
        && !UNSAFE_GLOBAL_NAMES.contains(&name)
}

/// True if the character may start a JavaScript identifier (letter, `_` or `$`).
fn is_identifier_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '$'
}

/// True if the character may continue a JavaScript identifier (an identifier
/// start character or an ASCII digit).
fn is_identifier_part(c: char) -> bool {
    is_identifier_start(c) || c.is_ascii_digit()
}
