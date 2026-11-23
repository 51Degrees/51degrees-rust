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

//! Where the values a script reads come from, and how they are read.
//!
//! A script only ever runs on a request where every source property it names
//! is available, so the whole of the reading happens here, before any check or
//! rule is evaluated.

use std::collections::HashMap;

use crate::model::{format_number, ValueType, INT_MAX, INT_MIN};

/// A value as the element that produced it supplies it.
///
/// A source value arrives either as its native type or as text, and both are
/// accepted for every type a script infers.
#[derive(Debug, Clone, PartialEq)]
pub enum SourceValue {
    /// True or false.
    Bool(bool),
    /// A whole number.
    Int(i64),
    /// A number that may carry a fractional part.
    Double(f64),
    /// Text, which is how a value read from a data file or a cloud response
    /// usually arrives.
    Text(String),
}

/// One candidate of a weighted value, being a value and how strongly the data
/// supports it.
#[derive(Debug, Clone, PartialEq)]
pub struct WeightedSourceValue {
    /// How strongly the data supports the value.
    pub weight: f64,
    /// The value.
    pub value: SourceValue,
}

/// What asking a source for one property answers.
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup {
    /// The element is not in the flow data, or the property is not on it, or
    /// its value is null.
    Absent,
    /// The source has the property but supplies its own no value message
    /// instead of a value, which is repeated in the message the derived
    /// property carries.
    NoValue(String),
    /// One value.
    Value(SourceValue),
    /// A list of weighted values, of which the one with the highest weight is
    /// read. The first of two equal weights wins.
    Weighted(Vec<WeightedSourceValue>),
    /// A list where a single value is needed, which no script can read.
    List,
}

/// Where a script reads the properties other elements have already produced.
///
/// Implement this to run a script somewhere other than a 51Degrees pipeline,
/// for example in a host that already holds the values. Both halves of a name
/// are matched without regard to letter case, as the Pipeline matches property
/// names elsewhere.
pub trait PropertySource {
    /// Read one property, named by the element that supplies it and by the
    /// property name on that element.
    fn lookup(&self, element_key: &str, property_name: &str) -> Lookup;
}

/// A source built from values held in a map, which is the simplest way to run
/// a script over values a caller already has.
#[derive(Debug, Clone, Default)]
pub struct MapSource {
    /// Keyed by the full name, `elementkey.propertyname`, folded to lower case.
    entries: HashMap<String, Lookup>,
}

impl MapSource {
    /// An empty source, on which every property is absent.
    pub fn new() -> Self {
        MapSource::default()
    }

    /// Add one property, named as `elementKey.PropertyName`.
    pub fn with(mut self, name: &str, lookup: Lookup) -> Self {
        self.insert(name, lookup);
        self
    }

    /// Add one property, named as `elementKey.PropertyName`.
    pub fn insert(&mut self, name: &str, lookup: Lookup) {
        self.entries.insert(name.to_lowercase(), lookup);
    }
}

impl PropertySource for MapSource {
    fn lookup(&self, element_key: &str, property_name: &str) -> Lookup {
        let key = format!("{element_key}.{property_name}").to_lowercase();
        self.entries.get(&key).cloned().unwrap_or(Lookup::Absent)
    }
}

/// A source value read as the type a script inferred for it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Reading {
    Bool(bool),
    Int(i32),
    Double(f64),
    Text(String),
}

/// Read a source value as the type given, or answer that it cannot be read.
///
/// Values are never coerced loosely, so the text `N/A`, `Unknown` and the
/// empty string never become false or zero, they make the property absent.
pub(crate) fn convert(value: &SourceValue, value_type: ValueType) -> Option<Reading> {
    match value {
        SourceValue::Text(text) => convert_text(text, value_type),
        SourceValue::Bool(flag) => match value_type {
            ValueType::Bool => Some(Reading::Bool(*flag)),
            // A native boolean read as text becomes True or False with a
            // capital first letter, which is the form the 51Degrees data
            // carries.
            ValueType::String => Some(Reading::Text(
                if *flag { "True" } else { "False" }.to_owned(),
            )),
            _ => None,
        },
        SourceValue::Int(number) => convert_number(*number as f64, value_type),
        SourceValue::Double(number) => convert_number(*number, value_type),
    }
}

fn convert_number(number: f64, value_type: ValueType) -> Option<Reading> {
    match value_type {
        ValueType::Int => {
            if number.is_finite()
                && number.fract() == 0.0
                && number >= INT_MIN as f64
                && number <= INT_MAX as f64
            {
                Some(Reading::Int(number as i32))
            } else {
                None
            }
        }
        ValueType::Double => {
            if number.is_finite() {
                Some(Reading::Double(number))
            } else {
                None
            }
        }
        ValueType::String => Some(Reading::Text(format_number(number))),
        ValueType::Bool => None,
    }
}

/// Read the text form of a value as the type given.
pub(crate) fn convert_text(text: &str, value_type: ValueType) -> Option<Reading> {
    match value_type {
        ValueType::Bool => match text.trim().to_lowercase().as_str() {
            "true" => Some(Reading::Bool(true)),
            "false" => Some(Reading::Bool(false)),
            _ => None,
        },
        ValueType::Int => {
            let trimmed = text.trim();
            if !is_whole_number_text(trimmed) {
                return None;
            }
            // The range is checked rather than left to the parser, because the
            // format fixes int at a signed 32 bit whole number so that one
            // script gives one answer in every language.
            match trimmed.trim_start_matches('+').parse::<i64>() {
                Ok(number) if (INT_MIN..=INT_MAX).contains(&number) => {
                    Some(Reading::Int(number as i32))
                }
                _ => None,
            }
        }
        ValueType::Double => {
            let trimmed = text.trim();
            if !is_decimal_number_text(trimmed) {
                return None;
            }
            match trimmed.trim_start_matches('+').parse::<f64>() {
                Ok(number) if number.is_finite() => Some(Reading::Double(number)),
                _ => None,
            }
        }
        ValueType::String => Some(Reading::Text(text.to_owned())),
    }
}

/// An optional sign then digits, and nothing else. Rust's own parser would
/// also take text the format does not allow, so the shape is checked here.
fn is_whole_number_text(text: &str) -> bool {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit())
}

/// An optional sign, digits with a full stop as the decimal separator, and an
/// optional exponent. This is the same shape every other language accepts, and
/// it deliberately refuses the extras Rust's own parser takes, such as `inf`,
/// `NaN` and a trailing type suffix.
fn is_decimal_number_text(text: &str) -> bool {
    let body = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (mantissa, exponent) = match body.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (body, None),
    };
    if let Some(exponent) = exponent {
        let digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if digits.is_empty() || !digits.chars().all(|character| character.is_ascii_digit()) {
            return false;
        }
    }
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (mantissa, None),
    };
    let whole_ok = whole.chars().all(|character| character.is_ascii_digit());
    let fraction_ok = fraction
        .map(|fraction| fraction.chars().all(|character| character.is_ascii_digit()))
        .unwrap_or(true);
    if !whole_ok || !fraction_ok {
        return false;
    }
    // Either side of the full stop may be empty, but not both, so `.5` and `5.`
    // are numbers whilst `.` alone is not.
    let digits = whole.len() + fraction.map(str::len).unwrap_or(0);
    digits > 0
}

/// How a value is written into the message that says it could not be read.
pub(crate) fn display_source_value(value: &SourceValue) -> String {
    match value {
        SourceValue::Text(text) => text.clone(),
        SourceValue::Bool(flag) => if *flag { "True" } else { "False" }.to_owned(),
        SourceValue::Int(number) => number.to_string(),
        SourceValue::Double(number) => format_number(*number),
    }
}
