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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-json-builder-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-json-builder-lib.rs&utm_term=logo)
//!
//! # 51Degrees JSON builder
//!
//! The JSON builder element serializes every piece of element data in a
//! [`fiftyone_pipeline_core::FlowData`] into a single JSON object and stores it
//! on the flow data under the [`JSON_PROPERTY_KEY`] property of its own element
//! data. It implements the
//! [json-builder specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/json-builder.md).
//!
//! The JSON it produces is what the client-side `51Degrees.core.js` and the
//! callback endpoint consume, so the JavaScript builder and the web layers
//! depend on its exact shape. See [`JsonBuilderElement`] for the field
//! conventions and a worked example.
//!
//! ## What it emits
//!
//! Each remaining element's data becomes a nested object keyed by that element's
//! lowercased data key. Within it, every property is emitted by its lowercased
//! name, plus the sibling keys described on [`JsonBuilderElement`]
//! (`nullreason`, `delayexecution`, `evidenceproperties`). The list of
//! JavaScript-typed property paths is collected into a top-level
//! [`JAVASCRIPT_PROPERTIES_KEY`] array, and any flow errors and warnings are
//! appended under [`ERRORS_KEY`] and [`WARNINGS_KEY`].
//!
//! ## Changing the output
//!
//! Each step the builder takes is a method of [`JsonBuilderSteps`] with a
//! default, matching the protected virtual methods of .NET's
//! `JsonBuilderElement`, so a caller replaces only the steps it changes and
//! calls the functions in [`default_steps`] for the rest. A
//! [`JsonValueConverter`] changes how a property value is written. Both are
//! given to the [`JsonBuilderElementBuilder`].
//!
//! ## Text form
//!
//! The JSON is written as .NET's builder writes it with Newtonsoft.Json,
//! indented by two spaces, with keys in lower case apart from
//! `javascriptProperties`, doubles in .NET's shortest round trip form and
//! .NET's string escapes. Lines end with the platform's line ending, as
//! .NET's do. [`write_indented`] describes the form, and a step that wants
//! the same text on every platform calls it with `"\n"`.
//!
//! ## Determinism
//!
//! Keys are written in the order they were added. Elements are added in the
//! order their data was added to the flow data, and properties in the order
//! each element data lists them, as .NET writes them, so the same flow data
//! always yields byte-identical JSON on one platform. That stability is what
//! lets downstream code compute a reliable ETag from the JSON.

#![warn(missing_docs)]

mod builder;
mod constants;
mod converter;
mod data;
pub mod default_steps;
mod element;
mod steps;
mod text;
mod value;

pub use builder::JsonBuilderElementBuilder;
pub use constants::{DEFAULT_ELEMENT_EXCLUSION_LIST, DEFAULT_PROPERTY_EXCLUSION_LIST};
pub use constants::{
    DELAY_EXECUTION_SUFFIX, ERRORS_KEY, EVIDENCE_PROPERTIES_SUFFIX, JAVASCRIPT_PROPERTIES_KEY,
    JSON_BUILDER_ELEMENT_DATA_KEY, JSON_PROPERTY_KEY, MAX_JAVASCRIPT_ITERATIONS,
    NULL_REASON_SUFFIX, SEQUENCE_EVIDENCE_KEY, WARNINGS_KEY,
};
pub use converter::JsonValueConverter;
pub use data::{JsonBuilderData, JSON_BUILDER_DATA_KEY};
pub use element::JsonBuilderElement;
pub use steps::{DefaultJsonBuilderSteps, JsonBuilderContext, JsonBuilderSteps, PipelineConfig};
pub use text::{write_indented, NEW_LINE};
pub use value::{JsonObject, JsonValue};
