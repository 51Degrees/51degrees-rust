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

//! The default of each step in [`JsonBuilderSteps`].
//!
//! Each function takes the steps in use as its first argument and calls the
//! other steps through it, so a caller's step can call the function of the
//! same name here to keep the default for part of its work.

use std::collections::BTreeMap;

use indexmap::IndexSet;

use fiftyone_pipeline_core::{
    ElementData, MapElementData, NoValueError, PropertyValue, PropertyValueType,
};

use crate::constants::{
    DELAY_EXECUTION_SUFFIX, ERRORS_KEY, EVIDENCE_PROPERTIES_SUFFIX, JAVASCRIPT_PROPERTIES_KEY,
    MAX_JAVASCRIPT_ITERATIONS, NULL_REASON_SUFFIX, WARNINGS_KEY,
};
use crate::element::JsonBuilderElement;
use crate::steps::{JsonBuilderContext, JsonBuilderSteps};
use crate::text::{write_indented, NEW_LINE};
use crate::value::{JsonObject, JsonValue};

/// The default of [`JsonBuilderSteps::build_json`].
///
/// A flow data with no sequence number is treated as the first request, so
/// the `javascriptProperties` list is added.
pub fn build_json<S: JsonBuilderSteps + ?Sized>(
    steps: &S,
    context: &JsonBuilderContext<'_>,
) -> String {
    let mut all_properties = steps.get_all_properties(context);
    let sequence = JsonBuilderElement::sequence_number(context.data()).unwrap_or(0);
    if sequence < MAX_JAVASCRIPT_ITERATIONS {
        add_javascript_properties(steps, context, &mut all_properties);
    }
    steps.add_errors(context, &mut all_properties);
    steps.add_warnings(context, &mut all_properties);
    steps.write_json(context, &all_properties)
}

/// The default of [`JsonBuilderSteps::write_json`], being indented text in
/// the form .NET's builder writes, with the platform's line ending. See
/// [`write_indented`] for the form.
pub fn write_json<S: JsonBuilderSteps + ?Sized>(
    _steps: &S,
    _context: &JsonBuilderContext<'_>,
    all_properties: &JsonObject,
) -> String {
    write_indented(all_properties, NEW_LINE)
}

/// The default of [`JsonBuilderSteps::get_all_properties`].
///
/// Element data is written in the order it was added to the flow data, and
/// a key already written is skipped.
pub fn get_all_properties<S: JsonBuilderSteps + ?Sized>(
    steps: &S,
    context: &JsonBuilderContext<'_>,
) -> JsonObject {
    let element = context.element();
    let data = context.data();
    let mut all_properties = JsonObject::new();
    for key in data.data_keys() {
        let element_key = key.to_lowercase();
        if element.is_element_excluded(&element_key) || all_properties.contains_key(&element_key) {
            continue;
        }
        if let Some(element_data) = data.get_str(&element_key) {
            let values = steps.get_values(context, &element_key, element_data);
            all_properties.insert(element_key, JsonValue::Object(values));
        }
    }
    all_properties
}

/// The default of [`JsonBuilderSteps::get_values`].
///
/// Properties are written in the order the element data lists them, and a
/// name that only differs in case from one already written is skipped.
pub fn get_values<S: JsonBuilderSteps + ?Sized>(
    steps: &S,
    context: &JsonBuilderContext<'_>,
    data_path: &str,
    source: &dyn ElementData,
) -> JsonObject {
    let element = context.element();
    let names: IndexSet<String> = source
        .keys()
        .into_iter()
        .map(|name| name.to_lowercase())
        .filter(|name| !element.is_property_excluded(name))
        .collect();

    let mut values = JsonObject::new();
    for name in &names {
        steps.add_json_values_for_property(
            context,
            &mut values,
            data_path,
            name,
            source.get(name),
            false,
        );
    }
    values
}

/// The default of [`JsonBuilderSteps::add_json_values_for_property`].
pub fn add_json_values_for_property<S: JsonBuilderSteps + ?Sized>(
    steps: &S,
    context: &JsonBuilderContext<'_>,
    json_values: &mut JsonObject,
    data_path: &str,
    name: &str,
    value: Result<PropertyValue, NoValueError>,
    include_value_data_only: bool,
) {
    let name = name.to_lowercase();
    let complete_name = format!("{}.{name}", data_path.to_lowercase());
    let config = context.config();

    match value {
        Ok(value) => {
            let element = context.element();
            let json = match element.convert_by_caller(&value) {
                Some(json) => json,
                None => match &value {
                    PropertyValue::KeyValueList(records) => JsonValue::Array(
                        records
                            .iter()
                            .map(|record| {
                                JsonValue::Object(steps.get_values(
                                    context,
                                    &complete_name,
                                    &record_data(record),
                                ))
                            })
                            .collect(),
                    ),
                    other => element.built_in_json_value(other),
                },
            };
            json_values.insert(name.clone(), json);
            if !include_value_data_only
                && config
                    .delayed_execution_properties()
                    .contains(&complete_name)
            {
                json_values.insert(format!("{name}{DELAY_EXECUTION_SUFFIX}"), true);
            }
        }
        Err(no_value) => {
            json_values.insert(name.clone(), JsonValue::Null);
            json_values.insert(format!("{name}{NULL_REASON_SUFFIX}"), no_value.message);
        }
    }

    if !include_value_data_only {
        if let Some(evidence) = config.delayed_evidence_properties().get(&complete_name) {
            let names = evidence.iter().cloned().map(JsonValue::String).collect();
            json_values.insert(
                format!("{name}{EVIDENCE_PROPERTIES_SUFFIX}"),
                JsonValue::Array(names),
            );
        }
    }
}

/// The default of [`JsonBuilderSteps::get_javascript_properties`].
///
/// The paths are listed in the order of the pipeline's flow elements, then
/// in the order each element lists its properties, and each path once.
pub fn get_javascript_properties<S: JsonBuilderSteps + ?Sized>(
    _steps: &S,
    context: &JsonBuilderContext<'_>,
    available_properties: &[String],
) -> Vec<String> {
    let mut paths = IndexSet::new();
    for flow_element in context.data().pipeline().flow_elements() {
        let element_key = flow_element.data_key().to_lowercase();
        let javascript = flow_element
            .properties()
            .iter()
            .filter(|p| p.available && p.value_type == PropertyValueType::JavaScript);
        for property in javascript {
            let path = format!("{element_key}.{}", property.name.to_lowercase());
            if available_properties.contains(&path) {
                paths.insert(path);
            }
        }
    }
    paths.into_iter().collect()
}

/// The default of [`JsonBuilderSteps::add_errors`].
///
/// Each error is written as its message alone, without the words naming
/// its kind, as .NET and Java write it (see
/// [`fiftyone_pipeline_core::Error::message`]).
pub fn add_errors<S: JsonBuilderSteps + ?Sized>(
    _steps: &S,
    context: &JsonBuilderContext<'_>,
    all_properties: &mut JsonObject,
) {
    let errors: Vec<JsonValue> = context
        .data()
        .errors()
        .iter()
        .map(|error| JsonValue::String(error.source.message().into_owned()))
        .collect();
    if !errors.is_empty() {
        all_properties.insert(ERRORS_KEY, JsonValue::Array(errors));
    }
}

/// The default of [`JsonBuilderSteps::add_warnings`].
pub fn add_warnings<S: JsonBuilderSteps + ?Sized>(
    _steps: &S,
    context: &JsonBuilderContext<'_>,
    all_properties: &mut JsonObject,
) {
    let warnings = context.data().warnings();
    if warnings.is_empty() {
        return;
    }
    let mut messages = match all_properties.get(WARNINGS_KEY) {
        Some(JsonValue::Array(existing))
            if existing
                .iter()
                .all(|message| matches!(message, JsonValue::String(_))) =>
        {
            existing.clone()
        }
        _ => Vec::new(),
    };
    messages.extend(
        warnings
            .iter()
            .map(|warning| JsonValue::String(warning.message.clone())),
    );
    all_properties.insert(WARNINGS_KEY, JsonValue::Array(messages));
}

/// Add the `javascriptProperties` list, built by
/// [`JsonBuilderSteps::get_javascript_properties`] from the complete names
/// of the keys already collected, when it is not empty.
pub fn add_javascript_properties<S: JsonBuilderSteps + ?Sized>(
    steps: &S,
    context: &JsonBuilderContext<'_>,
    all_properties: &mut JsonObject,
) {
    let mut available = Vec::new();
    for (element_key, values) in all_properties.iter() {
        if let JsonValue::Object(values) = values {
            available.extend(values.keys().map(|name| format!("{element_key}.{name}")));
        }
    }
    let javascript = steps.get_javascript_properties(context, &available);
    if !javascript.is_empty() {
        let paths = javascript.into_iter().map(JsonValue::String).collect();
        all_properties.insert(JAVASCRIPT_PROPERTIES_KEY, JsonValue::Array(paths));
    }
}

/// One record of a list of records as element data, so its values are
/// collected by the same step as an element's.
fn record_data(record: &BTreeMap<String, PropertyValue>) -> MapElementData {
    record
        .iter()
        .fold(MapElementData::new(), |bag, (name, value)| {
            bag.set(name, value.clone())
        })
}
