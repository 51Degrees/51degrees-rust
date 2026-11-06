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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-derived-properties-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-derived-properties-lib.rs&utm_term=logo)
//!
//! # Derived properties
//!
//! [`DerivedPropertyElement`] turns property values that are already in the
//! flow data into one more property value. That is the whole of its scope. It
//! holds no data file, sends no request over the network, keeps nothing between
//! one request and the next, and reads no evidence from the request.
//!
//! How a property is computed is written in a **script**, which is one YAML or
//! JSON file describing one output property. The script format is shared
//! between every 51Degrees language implementation, at
//! [51Degrees/derived-properties](https://github.com/51Degrees/derived-properties),
//! so one script gives the same answer in every language. This crate reads
//! format 1 and is tested against the same conformance cases as the other
//! languages.
//!
//! ## The one rule
//!
//! A script names source properties inside its conditions. Where every one of
//! those properties is available, the checks and the rules run and a value is
//! chosen. Where any one of them is not available, the script produces no
//! value, and the message names every property that was missing along with what
//! its source element said about each one. There is no third state.
//!
//! ## The scripts are compiled in
//!
//! The scripts 51Degrees ships are embedded in this crate at build time rather
//! than read from a file, so an element carrying one runs where there is no
//! file system and where fetching a file over the network would defeat the
//! purpose, such as in a WebAssembly host or at an edge runtime. A caller may
//! also supply the text of a script of its own, and
//! [`DerivedPropertyElementBuilder::add_script_file`] reads one from a file
//! where there is a file system to read it from.
//!
//! ## In a pipeline
//!
//! ```no_run
//! use fiftyone_derived_properties::{BuiltInScript, DerivedPropertyElement};
//!
//! let element = DerivedPropertyElement::builder()
//!     .add_built_in(BuiltInScript::HumanConfidence)
//!     .build()
//!     .expect("the shipped script is known to be sound");
//! ```
//!
//! Add the element to a pipeline **after** the elements that supply the
//! properties its scripts name, then read the result from the flow data under
//! the key `derived`.
//!
//! ## Without a pipeline
//!
//! A script runs over any [`PropertySource`], so a host that already holds the
//! values can run one without building a pipeline at all.
//!
//! ```
//! use fiftyone_derived_properties::{
//!     BuiltInScript, Lookup, MapSource, Outcome, SourceValue,
//! };
//!
//! let script = BuiltInScript::HumanConfidence.compile().unwrap();
//! let source = MapSource::new()
//!     .with("device.IsCrawler", Lookup::Value(SourceValue::Bool(false)))
//!     .with("device.IsHeadless", Lookup::Value(SourceValue::Bool(false)))
//!     // Read as text, not as a boolean, because the data carries the string
//!     // Unknown until the client side JavaScript has run.
//!     .with(
//!         "device.HasWebDriver",
//!         Lookup::Value(SourceValue::Text("False".to_owned())),
//!     )
//!     .with(
//!         "device.IsVisible",
//!         Lookup::Value(SourceValue::Text("True".to_owned())),
//!     )
//!     .with("device.BrowserReleaseYear", Lookup::Value(SourceValue::Int(2026)))
//!     .with("device.BrowserReleaseAge", Lookup::Value(SourceValue::Int(1)))
//!     .with("ip.HumanProbability", Lookup::Value(SourceValue::Int(9)))
//!     .with(
//!         "ip.ConnectionType",
//!         Lookup::Value(SourceValue::Text("Broadband".to_owned())),
//!     );
//!
//! match script.evaluate(&source) {
//!     Outcome::Value(value) => assert_eq!(value.to_text(), "High"),
//!     Outcome::NoValue { message, .. } => panic!("{message}"),
//! }
//! ```
//!
//! Leave one of those properties out and the script produces no value, naming
//! the property that was missing, which is the other half of what a script can
//! do.

#![warn(missing_docs)]

mod canonical;
mod element;
mod evaluate;
mod fault;
mod model;
mod parse;
mod scripts;
mod source;
mod validate;

pub use element::{
    DerivedData, DerivedPropertyElement, DerivedPropertyElementBuilder,
    DERIVED_DEFAULT_ELEMENT_DATA_KEY,
};
pub use evaluate::{Evaluation, Outcome, USUAL_CAUSES};
pub use fault::{Fault, Faults};
pub use model::{
    Aggregate, Check, CompareOp, Condition, Literal, Operand, Output, OutputValue, Rule, Script,
    SourceProperty, ValueType, INT_MAX, INT_MIN,
};
pub use scripts::BuiltInScript;
pub use source::{Lookup, MapSource, PropertySource, SourceValue, WeightedSourceValue};

impl Script {
    /// Read the text of a script, as YAML or as JSON.
    ///
    /// The script's own `Name` is taken as read, because no file name was
    /// given to check it against.
    pub fn compile(text: &str) -> Result<Script, Faults> {
        Script::compile_named(text, None, "code")
    }

    /// Read the text of a script that came from somewhere with a name.
    ///
    /// `name` is the file name without its extension, which the script's `Name`
    /// must equal, and `source` says where the script came from so that a fault
    /// can name it.
    pub fn compile_named(text: &str, name: Option<&str>, source: &str) -> Result<Script, Faults> {
        validate::validate_text(text, name, source)
    }
}
