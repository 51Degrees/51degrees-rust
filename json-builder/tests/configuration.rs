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

//! The JSON builder made from a configuration, compared with what the .NET
//! pipeline 4.5.129 made from the same configuration. Each expected message
//! is the one .NET gave, apart from the type .NET gives every setting in the
//! list of a builder's settings, which `fiftyone_pipeline_core`'s
//! configuration records as a difference.

use std::any::type_name;
use std::sync::Arc;

use fiftyone_json_builder::{
    configuration, JsonBuilderElement, JsonBuilderElementBuilder, JSON_BUILDER_DATA_KEY,
};
use fiftyone_pipeline_core::configuration::{BuilderRegistry, ElementOptions, PipelineOptions};
use fiftyone_pipeline_core::{Error, Evidence, Pipeline, PipelineBuilder, Result};
use fiftyone_pipeline_engines_fiftyone::constants::{EVIDENCE_SEQUENCE, EVIDENCE_SESSIONID};
use fiftyone_pipeline_engines_fiftyone::{SequenceElement, SetHeadersElement};

const BUILDER_NAME: &str = "JsonBuilderElementBuilder";
const NO_SUB_ELEMENTS: &str = "No SubElements";
const NO_INNER: &str = "No Inner Exception.";
const NO_TRY_PARSE: &str = "this type does not have the expected 'TryParse' method (or has \
                            multiples that cannot be resolved).";

fn registry() -> BuilderRegistry {
    let mut registry = BuilderRegistry::new();
    fiftyone_pipeline_engines_fiftyone::configuration::add_builders(&mut registry);
    configuration::add_builders(&mut registry);
    registry
}

fn build(options: &PipelineOptions) -> Result<Arc<Pipeline>> {
    PipelineBuilder::new().build_from_configuration(&registry(), options)
}

fn one(element: ElementOptions) -> Result<Arc<Pipeline>> {
    build(&PipelineOptions::new().element(element))
}

fn message(result: Result<Arc<Pipeline>>) -> String {
    match result {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

/// The message .NET gives for an element that cannot be made.
fn failed(builder: &str, message: &str) -> String {
    format!(
        "Configuration failed to build.\nBuilderName: {builder}\nSubElement Builder Names: \
         {NO_SUB_ELEMENTS}\n{message}\n{NO_INNER}\n"
    )
}

/// The message for a value `SetJsonConverters` refuses, which .NET gave
/// with `but` saying why.
fn converters_refused(but: &str) -> String {
    let builder = type_name::<JsonBuilderElementBuilder>();
    failed(
        BUILDER_NAME,
        &format!(
            "Method 'SetJsonConverters' on builder '{builder}' for element 0 expects a \
             parameter of type 'IEnumerable`1' but {but}"
        ),
    )
}

fn keys(pipeline: &Pipeline) -> Vec<&str> {
    pipeline
        .flow_elements()
        .iter()
        .map(|e| e.data_key())
        .collect()
}

/// The JSON `pipeline` writes for the second request of one session.
fn json(pipeline: &Arc<Pipeline>) -> String {
    let evidence = Evidence::builder()
        .add(EVIDENCE_SESSIONID, "session")
        .add(EVIDENCE_SEQUENCE, "1")
        .build();
    let mut data = pipeline.create_flow_data_with(evidence);
    data.process().expect("processing succeeds");
    data.get(JSON_BUILDER_DATA_KEY)
        .expect("the JSON builder's data")
        .json()
        .to_owned()
}

#[test]
fn the_json_builder_is_found_by_dotnets_names() {
    for name in [
        "JsonBuilderElementBuilder",
        "JsonBuilderElement",
        "jsonbuilderelementbuilder",
    ] {
        let pipeline = one(ElementOptions::new(name)).unwrap();
        assert_eq!(keys(&pipeline), ["json-builder"], "{name}");
    }

    // .NET only adds "Builder" to the name, and lists every builder it
    // loaded, in which these four come in this order.
    assert_eq!(
        message(one(ElementOptions::new("JsonBuilder"))),
        failed(
            "JsonBuilder",
            "Could not find builder matching 'JsonBuilder' for element 0. Available \
             builders are: SequenceElementBuilder,SetHeadersElementBuilder,\
             ShareUsageBuilder,JsonBuilderElementBuilder"
        )
    );
}

/// The element made from a file writes what `JsonBuilderElement::new()`
/// writes, so it has the same defaults, and it leaves out the Set Headers
/// element's data as .NET's does.
#[test]
fn a_json_file_written_for_dotnet_makes_the_same_element_as_code() {
    let options = PipelineOptions::from_json(
        r#"{
          "PipelineOptions": {
            "Elements": [
              { "BuilderName": "SequenceElementBuilder" },
              { "BuilderName": "SetHeadersElementBuilder" },
              { "BuilderName": "JsonBuilderElementBuilder", "BuildParameters": {} }
            ]
          }
        }"#,
    )
    .unwrap();
    let configured = build(&options).unwrap();
    assert_eq!(
        keys(&configured),
        ["sequence", "set-headers", "json-builder"]
    );

    let coded = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(SetHeadersElement::new()))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .build()
        .unwrap();
    let written = json(&configured);
    assert_eq!(written, json(&coded));
    assert!(written.contains("\"sequence\""), "{written}");
    assert!(!written.contains("set-headers"), "{written}");
}

#[test]
fn json_converters_cannot_be_given_in_a_configuration() {
    for (name, value) in [
        ("JsonConverters", "x"),
        ("SetJsonConverters", "x"),
        ("JsonConverters", ""),
    ] {
        assert_eq!(
            message(one(ElementOptions::new(BUILDER_NAME).parameter(name, value))),
            converters_refused(NO_TRY_PARSE),
            "{name} '{value}'"
        );
    }

    // .NET names a list given in code `List`1` and an array `String[]`,
    // and the crates have one list form.
    assert_eq!(
        message(one(
            ElementOptions::new(BUILDER_NAME).parameter("JsonConverters", vec!["a"])
        )),
        converters_refused("supplied object is of type 'String[]'.")
    );

    let options = PipelineOptions::from_json(
        r#"{
          "PipelineOptions": {
            "Elements": [
              {
                "BuilderName": "JsonBuilderElementBuilder",
                "BuildParameters": { "JsonConverters": "x" }
              }
            ]
          }
        }"#,
    )
    .unwrap();
    assert_eq!(message(build(&options)), converters_refused(NO_TRY_PARSE));
}

/// The crates' own exclusion settings are set in code only, so a
/// configuration naming one fails as it does in .NET, whose builder has
/// none.
#[test]
fn an_unknown_setting_lists_set_json_converters() {
    let builder = type_name::<JsonBuilderElementBuilder>();
    for (name, value, upper) in [
        ("Unknown", "1", "UNKNOWN"),
        ("PropertyExclusionList", "a", "PROPERTYEXCLUSIONLIST"),
    ] {
        assert_eq!(
            message(one(ElementOptions::new(BUILDER_NAME).parameter(name, value))),
            failed(
                BUILDER_NAME,
                &format!(
                    "Builder '{builder}' for element 0 has no 'Set' methods or 'Build' \
                     methods that match (case-insensitively) the following parameters: \
                     {upper}. The available configuration methods on this builder are: \n\
                     SetJsonConverters (IEnumerable`1)\nThe available 'Build' methods have \
                     the following signatures: \n\n"
                )
            ),
            "{name}"
        );
    }
}
