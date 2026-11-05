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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-javascript-builder-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-javascript-builder-lib.rs&utm_term=logo)
//!
//! # 51Degrees JavaScript builder
//!
//! The JavaScript builder element renders a client-side JavaScript include from
//! the JSON payload produced by the [`fiftyone_json_builder`] crate. The
//! generated script creates a global manager object on the client device, runs
//! any JavaScript-typed properties the JSON carries, and (when a callback URL is
//! configured) posts the evidence it gathers back to the server for a refreshed
//! result. It implements the
//! [javascript-builder specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/javascript-builder.md).
//!
//! ## The template
//!
//! The script is rendered from a single Mustache template,
//! `JavaScriptResource.mustache`, embedded at compile time with `include_str!`.
//! Rendering uses a small,
//! pure-safe-Rust Mustache renderer (in this crate's `mustache` module) with
//! HTML escaping disabled, so the JSON payload, the callback URL and the object
//! name reach the client verbatim.
//!
//! ## Configuration and per-request overrides
//!
//! [`JavaScriptBuilderElementBuilder`] sets the host, endpoint, protocol, object
//! name, cookie behavior and minification. Several of those can be overridden
//! per request through evidence (`query.fod-js-object-name`,
//! `query.fod-js-enable-cookies`, `header.host` and `header.protocol`). See
//! [`JavaScriptBuilderElement`] for the full derivation rules.
//!
//! ## The user prompt section
//!
//! The template has a section that gathers the visitor's answer from the
//! 51Degrees Preference Management Platform (PMP) or a consent platform on the
//! page, which is what a 51Did is created from. It is rendered only when the
//! builder was given a cloud request engine whose resource key is licensed for
//! 51Did, through
//! [`JavaScriptBuilderElementBuilder::set_shared_cloud_request_engine`], which
//! waits for the engine's discovery to complete, or
//! [`JavaScriptBuilderElementBuilder::set_cloud_request_engine`], which decides
//! when it is called.
//!
//! ## Minification
//!
//! With the default-on `minify` feature the rendered script is minified by the
//! oxc toolchain (see the `minify` module). A minification failure falls back to
//! the unminified script, so valid JavaScript is always served. The `set_minify`
//! builder flag turns minification off for an element, and building the crate
//! with `default-features = false` drops the oxc dependency entirely.
//!
//! ## A worked pipeline
//!
//! ```
//! use std::sync::Arc;
//! use fiftyone_pipeline_core::{Evidence, Pipeline};
//! use fiftyone_json_builder::JsonBuilderElement;
//! use fiftyone_javascript_builder::{JavaScriptBuilderElement, JAVASCRIPT_BUILDER_DATA_KEY};
//! use fiftyone_pipeline_engines_fiftyone::SequenceElement;
//!
//! // The JSON builder needs the sequence number the Sequence element gives.
//! let pipeline = Pipeline::builder()
//!     .add_element(Arc::new(SequenceElement::new()))
//!     .add_element(Arc::new(JsonBuilderElement::new()))
//!     .add_element(Arc::new(JavaScriptBuilderElement::new()))
//!     .build()
//!     .unwrap();
//!
//! let mut data = pipeline.create_flow_data_with(
//!     Evidence::builder().add("header.host", "localhost").build(),
//! );
//! data.process().unwrap();
//!
//! let js = data.get(JAVASCRIPT_BUILDER_DATA_KEY).unwrap().javascript().to_owned();
//! assert!(js.contains("fiftyoneDegreesManager"));
//! ```

#![warn(missing_docs)]

mod builder;
mod callback_url;
mod constants;
mod data;
mod element;
mod minify;
mod mustache;
mod template_data;

pub use builder::JavaScriptBuilderElementBuilder;
pub use constants::{
    BUILDER_DEFAULT_ENABLE_COOKIES, BUILDER_DEFAULT_HOST, BUILDER_DEFAULT_MINIFY,
    BUILDER_DEFAULT_OBJECT_NAME, BUILDER_DEFAULT_PROTOCOL, EVIDENCE_ENABLE_COOKIES,
    EVIDENCE_ENABLE_COOKIES_SUFFIX, EVIDENCE_HOST_KEY, EVIDENCE_OBJECT_NAME,
    EVIDENCE_OBJECT_NAME_SUFFIX, FALLBACK_PROTOCOL, FODID_PRODUCT_NAME,
    JAVASCRIPT_BUILDER_ELEMENT_DATA_KEY, JAVASCRIPT_PROPERTY_KEY,
};
pub use data::{JavaScriptBuilderElementData, JAVASCRIPT_BUILDER_DATA_KEY};
pub use element::JavaScriptBuilderElement;
pub use template_data::JavaScriptResource;
