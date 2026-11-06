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

//! Making a pipeline from a configuration file, as the
//! [pipeline configuration specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/pipeline-configuration.md)
//! requires, so that a file written for the .NET pipeline makes the same
//! pipeline here.
//!
//! [`PipelineOptions`] describes the pipeline in the shape of .NET's
//! options, and [`PipelineOptions::from_json`] reads it from a JSON file.
//! A [`BuilderRegistry`] holds a [`BuilderDefinition`] for each element
//! builder a configuration can name, and
//! [`crate::PipelineBuilder::build_from_configuration`] makes the pipeline,
//! finding builders and their settings by name and reading each value from
//! text as .NET does.
//!
//! ```
//! use std::sync::Arc;
//! use fiftyone_pipeline_core::configuration::{
//!     BuilderDefinition, BuilderRegistry, PipelineOptions,
//! };
//! use fiftyone_pipeline_core::{
//!     EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
//!     PipelineBuilder, PropertyMetaData, Result,
//! };
//!
//! struct Greeter {
//!     greeting: String,
//!     filter: EvidenceKeyFilterWhitelist,
//! }
//!
//! impl FlowElement for Greeter {
//!     fn process(&self, _data: &mut FlowData) -> Result<()> {
//!         Ok(())
//!     }
//!     fn data_key(&self) -> &str {
//!         "greeter"
//!     }
//!     fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
//!         &self.filter
//!     }
//!     fn properties(&self) -> &[PropertyMetaData] {
//!         &[]
//!     }
//! }
//!
//! let registry = BuilderRegistry::new().with(
//!     BuilderDefinition::new("GreeterBuilder", |greeting: String, _| {
//!         let filter = EvidenceKeyFilterWhitelist::new(Vec::<String>::new());
//!         Ok(Arc::new(Greeter { greeting, filter }) as Arc<dyn FlowElement>)
//!     })
//!     .setting("SetGreeting", |_, greeting: String| greeting)
//!     .build_method(vec![], |_, _| Ok("Hello".to_string())),
//! );
//!
//! let options = PipelineOptions::from_json(
//!     r#"{
//!         // Comments are allowed, as .NET allows them.
//!         "PipelineOptions": {
//!             "Elements": [
//!                 { "BuilderName": "Greeter", "BuildParameters": { "Greeting": "Hi" } }
//!             ]
//!         }
//!     }"#,
//! )?;
//! let pipeline = PipelineBuilder::new().build_from_configuration(&registry, &options)?;
//! assert_eq!(pipeline.flow_elements()[0].data_key(), "greeter");
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

mod json;
mod options;
mod registry;
mod text;
mod time_span;

pub use options::{ConfigValue, ElementOptions, PipelineOptions};
pub use registry::{BuildArguments, BuildContext, BuilderDefinition, BuilderRegistry, Parameter};
pub use text::{parse_csv, parse_enum, ConfigType, ParseFailure};
pub use time_span::TimeSpan;
