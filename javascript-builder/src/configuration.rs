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

//! The JavaScript builder, found by the name .NET gives it, so that a
//! configuration file written for .NET makes the same element here. See
//! [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_builders`] adds `JavaScriptBuilderElementBuilder`, which a
//! configuration can also name `JavaScriptBuilderElement`. It takes .NET's
//! six settings, `SetEnableCookies`, `SetHost`, `SetEndpoint`,
//! `SetProtocol`, `SetObjectName` and `SetMinify`, each given to the
//! [`JavaScriptBuilderElementBuilder`] method of the same name. So a
//! protocol other than `http` or `https`, or an object name that is not a
//! valid identifier, is refused as it is in code. The steps, the template
//! and the user prompt decision are set in code only, and .NET's builder
//! has none of them.
//!
//! The pipeline holds the element made, so what a configuration set can be
//! read back.
//!
//! ```
//! use fiftyone_javascript_builder::{configuration, JavaScriptBuilderElement};
//! use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
//! use fiftyone_pipeline_core::PipelineBuilder;
//!
//! let mut registry = BuilderRegistry::new();
//! fiftyone_pipeline_engines_fiftyone::configuration::add_builders(&mut registry);
//! fiftyone_json_builder::configuration::add_builders(&mut registry);
//! configuration::add_builders(&mut registry);
//! let options = PipelineOptions::from_json(
//!     r#"{
//!         "PipelineOptions": {
//!             "Elements": [
//!                 { "BuilderName": "SequenceElementBuilder" },
//!                 { "BuilderName": "JsonBuilderElementBuilder" },
//!                 {
//!                     "BuilderName": "JavaScriptBuilderElement",
//!                     "BuildParameters": { "ObjectName": "myObj", "Minify": false }
//!                 }
//!             ]
//!         }
//!     }"#,
//! )?;
//! let pipeline = PipelineBuilder::new().build_from_configuration(&registry, &options)?;
//!
//! let element = pipeline
//!     .resource::<JavaScriptBuilderElement>()
//!     .expect("the JavaScript builder");
//! assert_eq!(element.object_name(), "myObj");
//! assert!(!element.minify());
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

use std::sync::Arc;

use fiftyone_pipeline_core::configuration::{BuildContext, BuilderDefinition, BuilderRegistry};
use fiftyone_pipeline_core::FlowElement;

use crate::{JavaScriptBuilderElement, JavaScriptBuilderElementBuilder};

/// Add the JavaScript builder to `registry`, as
/// `JavaScriptBuilderElementBuilder`. Returns `registry` for chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry.add(javascript_builder_element_builder())
}

/// The builder, with its settings in the order .NET's
/// `JavaScriptBuilderElementBuilder` declares them.
fn javascript_builder_element_builder() -> BuilderDefinition<JavaScriptBuilderElementBuilder> {
    BuilderDefinition::new(
        "JavaScriptBuilderElementBuilder",
        |builder: JavaScriptBuilderElementBuilder, context: &mut BuildContext| {
            let element = Arc::new(builder.build());
            context.add_resource(Arc::clone(&element) as _);
            Ok(element as Arc<dyn FlowElement>)
        },
    )
    .setting(
        "SetEnableCookies",
        |builder: JavaScriptBuilderElementBuilder, enable: bool| builder.set_enable_cookies(enable),
    )
    .setting(
        "SetHost",
        |builder: JavaScriptBuilderElementBuilder, host: String| builder.set_host(host),
    )
    .setting(
        "SetEndpoint",
        |builder: JavaScriptBuilderElementBuilder, endpoint: String| builder.set_endpoint(endpoint),
    )
    .try_setting(
        "SetProtocol",
        |builder: JavaScriptBuilderElementBuilder, protocol: String| builder.set_protocol(protocol),
    )
    .try_setting(
        "SetObjectName",
        |builder: JavaScriptBuilderElementBuilder, name: String| builder.set_object_name(name),
    )
    .setting(
        "SetMinify",
        |builder: JavaScriptBuilderElementBuilder, minify: bool| builder.set_minify(minify),
    )
    .build_method(Vec::new(), |_, _| Ok(JavaScriptBuilderElement::builder()))
}
