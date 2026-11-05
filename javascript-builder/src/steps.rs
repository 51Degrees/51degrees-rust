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

//! The steps the JavaScript builder takes to gather values for the script,
//! which a caller can replace one at a time.

use std::fmt;
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::sync::Arc;

use fiftyone_pipeline_core::FlowData;

use crate::default_steps;
use crate::element::JavaScriptBuilderElement;

/// What every step is given: the element and the flow data the script is
/// being built for.
#[derive(Clone, Copy)]
pub struct JavaScriptBuilderContext<'a> {
    element: &'a JavaScriptBuilderElement,
    data: &'a FlowData,
}

impl<'a> JavaScriptBuilderContext<'a> {
    /// Bundle the element and the flow data.
    pub fn new(element: &'a JavaScriptBuilderElement, data: &'a FlowData) -> Self {
        JavaScriptBuilderContext { element, data }
    }

    /// The JavaScript builder doing the building, for its configured host,
    /// endpoint, protocol, object name and cookie setting.
    pub fn element(&self) -> &'a JavaScriptBuilderElement {
        self.element
    }

    /// The flow data the script is built for.
    pub fn data(&self) -> &'a FlowData {
        self.data
    }
}

/// The steps the JavaScript builder takes to gather values for the script,
/// each of which a caller can replace.
///
/// Every method has a default, so an implementation replaces only the steps
/// it changes. To keep the default for part of a step, call the function of
/// the same name in [`default_steps`](crate::default_steps). Pass the
/// implementation to
/// [`JavaScriptBuilderElementBuilder::set_steps`](crate::JavaScriptBuilderElementBuilder::set_steps).
///
/// The steps match the protected virtual methods of .NET's
/// `JavaScriptBuilderElement`, as follows.
///
/// | Method                                           | .NET               |
/// |--------------------------------------------------|--------------------|
/// | [`get_parameters`](Self::get_parameters)         | `GetParameters`    |
/// | [`get_sequence`](Self::get_sequence)             | `GetSequence`      |
/// | [`get_session_id`](Self::get_session_id)         | `GetSessionId`     |
/// | [`render_user_prompt`](Self::render_user_prompt) | `RenderUserPrompt` |
///
/// Whatever a step gives, the script is given a session id of 1 to 64 ASCII
/// letters, digits and hyphens or an empty string, and a sequence of at least
/// 1, because [`JavaScriptResource::new`](crate::JavaScriptResource::new)
/// replaces any other value as the
/// [javascript-builder specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/javascript-builder.md)
/// says.
///
/// # Example
///
/// Adding a fixed parameter to every callback the script makes, after the
/// parameters the builder takes from the evidence.
///
/// ```
/// use fiftyone_javascript_builder::{
///     default_steps, JavaScriptBuilderContext, JavaScriptBuilderElement,
///     JavaScriptBuilderSteps,
/// };
///
/// struct WithRegion;
///
/// impl JavaScriptBuilderSteps for WithRegion {
///     fn get_parameters(
///         &self,
///         context: &JavaScriptBuilderContext<'_>,
///     ) -> Vec<(String, String)> {
///         let mut parameters = default_steps::get_parameters(context);
///         parameters.push(("region".to_owned(), "eu".to_owned()));
///         parameters
///     }
/// }
///
/// let element = JavaScriptBuilderElement::builder()
///     .set_steps(WithRegion)
///     .build();
/// ```
pub trait JavaScriptBuilderSteps: Send + Sync {
    /// The query parameters the script adds to each callback, as names and
    /// values in the order they are written.
    ///
    /// They are written into the script as one JSON object, each name and
    /// value as it is given, so a name should be given once and both should
    /// already be encoded for a URL. The default gives every `query.`
    /// evidence value apart from the session id and sequence, named by the
    /// text after `query.`, with the name and value percent-encoded, in name
    /// order.
    fn get_parameters(&self, context: &JavaScriptBuilderContext<'_>) -> Vec<(String, String)> {
        default_steps::get_parameters(context)
    }

    /// The sequence number of this request in its session.
    ///
    /// The default reads the Sequence element's data, then the
    /// `query.sequence` evidence when no Sequence element ran, and gives `1`
    /// when neither holds a positive 32-bit integer.
    fn get_sequence(&self, context: &JavaScriptBuilderContext<'_>) -> i32 {
        default_steps::get_sequence(context)
    }

    /// The session id of this request.
    ///
    /// The default reads the Sequence element's data, then the
    /// `query.session-id` evidence when no Sequence element ran, and gives
    /// an empty string when neither holds 1 to 64 ASCII letters, digits and
    /// hyphens.
    fn get_session_id(&self, context: &JavaScriptBuilderContext<'_>) -> String {
        default_steps::get_session_id(context)
    }

    /// Whether the script carries the user prompt section, which gathers the
    /// visitor's answer that a 51Did is created from.
    ///
    /// The section sits inside the update section, so it is only rendered
    /// when the script has a callback URL as well. The default renders it
    /// when the builder was given a cloud request engine whose licensed
    /// products include 51Did. A server running the whole pipeline for many
    /// callers can replace this step to decide for each request.
    fn render_user_prompt(&self, context: &JavaScriptBuilderContext<'_>) -> bool {
        default_steps::render_user_prompt(context)
    }
}

/// The builder's own steps, each one the default.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultJavaScriptBuilderSteps;

impl JavaScriptBuilderSteps for DefaultJavaScriptBuilderSteps {}

/// The steps an element uses, shared between a builder and the elements it
/// builds.
///
/// The element only calls the steps, and keeps nothing that a step which
/// panics could leave half changed, so the builder and the element stay
/// `UnwindSafe` and `RefUnwindSafe` whatever the steps hold.
#[derive(Clone)]
pub(crate) struct SharedSteps(Arc<dyn JavaScriptBuilderSteps>);

impl SharedSteps {
    /// Share these steps.
    pub(crate) fn new(steps: impl JavaScriptBuilderSteps + 'static) -> Self {
        SharedSteps(Arc::new(steps))
    }

    /// The steps.
    pub(crate) fn get(&self) -> &dyn JavaScriptBuilderSteps {
        self.0.as_ref()
    }
}

impl UnwindSafe for SharedSteps {}
impl RefUnwindSafe for SharedSteps {}

impl fmt::Debug for SharedSteps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SharedSteps")
    }
}
