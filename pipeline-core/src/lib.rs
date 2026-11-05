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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-pipeline-core-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-pipeline-core-lib.rs&utm_term=logo)
//!
//! # 51Degrees pipeline core
//!
//! The reflection-free Rust core of the 51Degrees pipeline. Every other crate
//! in the workspace builds on the trait surface defined here, so this crate is
//! the keystone and is kept deliberately small, ergonomic and stable.
//!
//! It implements the language-agnostic
//! [pipeline specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/conceptual-overview.md).
//!
//! ## The pieces
//!
//! - [`Pipeline`] and [`PipelineBuilder`] group ordered [`FlowElement`]s and run
//!   them in order, with the elements of a parallel group at the same time. A
//!   built pipeline is immutable and shared via [`std::sync::Arc`].
//! - [`FlowData`] is the unit of work. It carries the immutable [`Evidence`]
//!   input, the [`ElementData`] outputs, and any errors, and it knows how to
//!   process itself via its pipeline back-reference.
//! - [`Evidence`] is an immutable, case-insensitive `prefix.field` store with a
//!   defined precedence order ([`EvidencePrefix`]). [`EvidenceKeyFilter`] and
//!   [`EvidenceKeyFilterWhitelist`] advertise the evidence an element accepts
//!   and drive deterministic [`DataKey`] generation for caching and ETags.
//! - [`TypedKey<T>`] gives strongly-typed, reflection-free access to element
//!   data, replacing the C# `IsAssignableFrom` pattern with a compile-time type
//!   carried in the key plus an [`std::any::Any`] downcast.
//! - [`PropertyMetaData`] describes the properties an element populates.
//! - [`Error`], [`FlowError`], [`NoValueError`] and [`MissingPropertyReason`]
//!   form the error model.
//!
//! ## A minimal pipeline
//!
//! ```
//! use std::sync::Arc;
//! use fiftyone_pipeline_core::{
//!     ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist,
//!     FlowData, FlowElement, MapElementData, Pipeline, PropertyMetaData,
//!     PropertyValueType, Result, TypedKey,
//! };
//!
//! // Element data: here we just reuse the built-in map-backed bag.
//! struct GreetingData(MapElementData);
//! impl ElementData for GreetingData {
//!     fn get(&self, name: &str) -> std::result::Result<
//!         fiftyone_pipeline_core::PropertyValue,
//!         fiftyone_pipeline_core::NoValueError,
//!     > {
//!         self.0.get(name)
//!     }
//!     fn keys(&self) -> Vec<String> { self.0.keys() }
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//!
//! struct GreetingElement {
//!     filter: EvidenceKeyFilterWhitelist,
//!     properties: Vec<PropertyMetaData>,
//! }
//! impl GreetingElement {
//!     const KEY: TypedKey<GreetingData> = TypedKey::new("greeting");
//!     fn new() -> Self {
//!         GreetingElement {
//!             filter: EvidenceKeyFilterWhitelist::new(["query.name"]),
//!             properties: vec![PropertyMetaData::new(
//!                 "message", "greeting", PropertyValueType::String,
//!             )],
//!         }
//!     }
//! }
//! impl FlowElement for GreetingElement {
//!     fn process(&self, data: &mut FlowData) -> Result<()> {
//!         let name = data.evidence().get("query.name").unwrap_or("world").to_owned();
//!         data.get_or_add(Self::KEY, || {
//!             GreetingData(MapElementData::new().set("message", format!("Hello, {name}!")))
//!         })?;
//!         Ok(())
//!     }
//!     fn data_key(&self) -> &str { "greeting" }
//!     fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter { &self.filter }
//!     fn properties(&self) -> &[PropertyMetaData] { &self.properties }
//! }
//!
//! let pipeline = Pipeline::builder()
//!     .add_element(Arc::new(GreetingElement::new()))
//!     .build()
//!     .unwrap();
//!
//! let mut data = pipeline.create_flow_data_with(
//!     Evidence::builder().add("query.name", "Ada").build(),
//! );
//! data.process().unwrap();
//!
//! let greeting = data.get(GreetingElement::KEY).unwrap();
//! assert_eq!(greeting.get("message").unwrap().as_str(), Some("Hello, Ada!"));
//! ```

#![warn(missing_docs)]

mod ci_map;
pub mod configuration;
pub mod constants;
mod element_data;
mod error;
mod evidence;
mod flow_data;
mod flow_element;
mod group_threads;
mod pipeline;
mod property;
pub mod redact;
mod typed_key;
mod value;
mod warning;

pub use element_data::{ElementData, MapElementData};
pub use error::{Error, FlowError, MissingPropertyReason, NoValueError, Result};
pub use evidence::{
    compare_keys, DataKey, Evidence, EvidenceBuilder, EvidenceEntry, EvidenceKeyFilter,
    EvidenceKeyFilterAggregator, EvidenceKeyFilterWhitelist, EvidencePrefix,
};
pub use flow_data::FlowData;
pub use flow_element::FlowElement;
pub use pipeline::{Pipeline, PipelineBuilder};
pub use property::PropertyMetaData;
pub use typed_key::TypedKey;
pub use value::{PropertyValue, PropertyValueType, WeightedValue};
pub use warning::FlowWarning;
