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

//! The values element, whose data holds one of each kind of value the JSON
//! builder writes and which adds two errors and a warning, so the JSON and
//! the JavaScript the two languages write for the same flow data can be
//! compared byte for byte. The .NET producer's `ValuesElement` holds the
//! same values in the same order.
//!
//! Only the properties something reads by their metadata are declared,
//! being a delayed JavaScript property and one that names it as evidence,
//! which the JSON builder reads, and `Promise`, which the JavaScript builder
//! reads. The JSON builder writes the rest from the data alone.

use std::any::Any;
use std::collections::BTreeMap;

use fiftyone_pipeline_core::{
    ElementData, Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    FlowError, NoValueError, PropertyMetaData, PropertyValue, PropertyValueType, Result, TypedKey,
};

/// The element's data key.
const VALUES_DATA_KEY: &str = "values";

const VALUES_KEY: TypedKey<ValuesData> = TypedKey::new(VALUES_DATA_KEY);

/// The errors the element adds, in order.
const ERRORS: [&str; 2] = ["The first error.", "The second error."];

/// The warning the element adds after its errors.
const WARNING: &str = "The warning.";

/// The reason the property with no value gives.
const NO_VALUE_REASON: &str = "There is no value, on purpose.";

/// The evidence that gives the `Promise` property its value. Without it,
/// `Promise` has no value.
const PROMISE_EVIDENCE_KEY: &str = "header.values-promise";

/// The reason `Promise` gives when the evidence has no value for it.
const NO_PROMISE_REASON: &str = "The evidence gave no promise.";

/// A property's value, or the reason it has none.
type Entry = std::result::Result<PropertyValue, &'static str>;

/// The values element's data, in the order the properties were set.
struct ValuesData {
    entries: Vec<(&'static str, Entry)>,
}

impl ValuesData {
    fn new(promise: Option<String>) -> Self {
        ValuesData {
            entries: vec![
                (
                    "Text",
                    Ok(PropertyValue::String(
                        "a \"quote\", a backslash \\, U+0001 \u{1}, U+2028 \u{2028}, <, & and \u{e9}"
                            .to_owned(),
                    )),
                ),
                (
                    "Whitespace",
                    Ok(PropertyValue::String(
                        "a tab \t, a new line \n, a return \r, U+0085 \u{85} and U+2029 \u{2029}"
                            .to_owned(),
                    )),
                ),
                ("One", Ok(PropertyValue::Double(1.0))),
                ("Tenth", Ok(PropertyValue::Double(0.1))),
                ("Small", Ok(PropertyValue::Double(1e-7))),
                ("Large", Ok(PropertyValue::Double(1e21))),
                ("Single", Ok(PropertyValue::Float(0.1))),
                ("SingleLarge", Ok(PropertyValue::Float(1e9))),
                ("Count", Ok(PropertyValue::Integer(42))),
                ("Flag", Ok(PropertyValue::Bool(true))),
                (
                    "Names",
                    Ok(PropertyValue::StringList(vec![
                        "first".to_owned(),
                        "second".to_owned(),
                    ])),
                ),
                ("Missing", Err(NO_VALUE_REASON)),
                (
                    "Script",
                    Ok(PropertyValue::JavaScript(
                        "document.cookie = \"51D_Values=1\";".to_owned(),
                    )),
                ),
                (
                    "FromScript",
                    Ok(PropertyValue::String("Read from the script.".to_owned())),
                ),
                (
                    "Weights",
                    Ok(PropertyValue::KeyValueList(vec![
                        weighted(40000, "first"),
                        weighted(25535, "second"),
                    ])),
                ),
                (
                    "Promise",
                    promise
                        .map(PropertyValue::String)
                        .ok_or(NO_PROMISE_REASON),
                ),
            ],
        }
    }

    fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, entry)| entry)
    }
}

/// One item of a weighted list, as .NET's JSON builder writes it.
fn weighted(raw_weighting: i64, value: &str) -> BTreeMap<String, PropertyValue> {
    BTreeMap::from([
        (
            "rawweighting".to_owned(),
            PropertyValue::Integer(raw_weighting),
        ),
        ("value".to_owned(), PropertyValue::String(value.to_owned())),
    ])
}

impl ElementData for ValuesData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        match self.entry(name) {
            Some(Ok(value)) => Ok(value.clone()),
            Some(Err(reason)) => Err(NoValueError::new(*reason)),
            None => Err(NoValueError::new(format!(
                "No value for property '{name}'."
            ))),
        }
    }

    fn has_property(&self, name: &str) -> bool {
        self.entry(name).is_some()
    }

    fn keys(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Adds the values element's data, its two errors and its warning.
pub struct ValuesElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl ValuesElement {
    pub fn new() -> Self {
        ValuesElement {
            filter: EvidenceKeyFilterWhitelist::new([PROMISE_EVIDENCE_KEY]),
            properties: vec![
                PropertyMetaData::new("Script", VALUES_DATA_KEY, PropertyValueType::JavaScript)
                    .with_delay_execution(true),
                PropertyMetaData::new("FromScript", VALUES_DATA_KEY, PropertyValueType::String)
                    .with_evidence_properties(["Script"]),
                PropertyMetaData::new("Promise", VALUES_DATA_KEY, PropertyValueType::String),
            ],
        }
    }
}

impl Default for ValuesElement {
    fn default() -> Self {
        Self::new()
    }
}

impl FlowElement for ValuesElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let promise = data.evidence().get(PROMISE_EVIDENCE_KEY).map(str::to_owned);
        data.get_or_add(VALUES_KEY, || ValuesData::new(promise))?;
        for message in ERRORS {
            let error = Error::Pipeline {
                message: message.to_owned(),
            };
            data.add_error(FlowError::new(VALUES_DATA_KEY, error), true);
        }
        data.add_warning(WARNING, Some(self as &dyn FlowElement));
        Ok(())
    }

    fn data_key(&self) -> &str {
        VALUES_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}
