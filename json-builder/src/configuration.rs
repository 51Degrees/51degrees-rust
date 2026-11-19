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

//! The JSON builder, found by the name .NET gives it, so that a
//! configuration file written for .NET makes the same element here. See
//! [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_builders`] adds `JsonBuilderElementBuilder`, which a configuration
//! can also name `JsonBuilderElement`. As in .NET, it takes no setting from
//! a configuration. Naming `SetJsonConverters` fails with .NET's message,
//! because converters are given in code, with
//! [`JsonBuilderElementBuilder::set_json_converters`]. The steps and the
//! exclusion lists are set in code only too, and .NET's builder has
//! neither.
//!
//! ```
//! use fiftyone_json_builder::configuration;
//! use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
//! use fiftyone_pipeline_core::PipelineBuilder;
//!
//! let mut registry = BuilderRegistry::new();
//! fiftyone_pipeline_engines_fiftyone::configuration::add_builders(&mut registry);
//! configuration::add_builders(&mut registry);
//! let options = PipelineOptions::from_json(
//!     r#"{
//!         "PipelineOptions": {
//!             "Elements": [
//!                 { "BuilderName": "SequenceElementBuilder" },
//!                 { "BuilderName": "JsonBuilderElementBuilder" }
//!             ]
//!         }
//!     }"#,
//! )?;
//! let pipeline = PipelineBuilder::new().build_from_configuration(&registry, &options)?;
//!
//! let keys: Vec<&str> = pipeline.flow_elements().iter().map(|e| e.data_key()).collect();
//! assert_eq!(keys, ["sequence", "json-builder"]);
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

use std::sync::Arc;

use fiftyone_pipeline_core::configuration::{BuildContext, BuilderDefinition, BuilderRegistry};
use fiftyone_pipeline_core::FlowElement;

use crate::JsonBuilderElementBuilder;

/// The name of .NET's `IEnumerable<JsonConverter>`, the type
/// `SetJsonConverters` takes, as configuration errors give it.
const JSON_CONVERTERS_TYPE_NAME: &str = "IEnumerable`1";

/// Add the JSON builder to `registry`, as `JsonBuilderElementBuilder`.
/// Returns `registry` for chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry.add(json_builder_element_builder())
}

fn json_builder_element_builder() -> BuilderDefinition<JsonBuilderElementBuilder> {
    BuilderDefinition::new(
        "JsonBuilderElementBuilder",
        |builder: JsonBuilderElementBuilder, _: &mut BuildContext| {
            Ok(Arc::new(builder.build()) as Arc<dyn FlowElement>)
        },
    )
    .code_only_setting("SetJsonConverters", JSON_CONVERTERS_TYPE_NAME)
    .build_method(Vec::new(), |_, _| Ok(JsonBuilderElementBuilder::new()))
}
