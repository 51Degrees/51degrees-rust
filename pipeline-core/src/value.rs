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

//! Property value types.
//!
//! [`PropertyValue`] is a closed enum covering every value type the pipeline
//! specification expects, as listed in the
//! [properties specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#value-types).
//! Using an enum rather than a boxed `dyn Any` keeps the dynamic property bag
//! ([`crate::ElementData::get`]) self-describing and avoids reflection.

use std::collections::BTreeMap;

/// The declared type of a property's values, as carried in property metadata.
///
/// This is the metadata "Type" column from the
/// [properties specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#property-metadata).
/// It mirrors the variants of [`PropertyValue`] so that a piece of metadata can
/// describe what kind of value a property returns without an instance of it. It
/// is a small enum, which keeps the core reflection-free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PropertyValueType {
    /// A string value.
    String,
    /// A boolean value.
    Bool,
    /// An integer value.
    Integer,
    /// A floating-point value.
    Double,
    /// A 32-bit floating-point value.
    Float,
    /// A list of strings.
    StringList,
    /// A JavaScript snippet intended to run on the client device.
    JavaScript,
    /// A list of key-value-pair collections.
    KeyValueList,
}

/// A weighted property value.
///
/// IP Intelligence properties can return several candidate values, each with a
/// weighting that indicates how strongly the data supports it. The raw
/// weighting is a `u16`; [`WeightedValue::weighting`] converts it to a
/// `0.0..=1.0` multiplier by dividing by `u16::MAX`, exactly as the
/// [value-types specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/value-types.md#property-details)
/// requires.
#[derive(Debug, Clone, PartialEq)]
pub struct WeightedValue<T> {
    /// The integer weight factor as stored in the data.
    pub raw_weighting: u16,
    /// The value itself.
    pub value: T,
}

impl<T> WeightedValue<T> {
    /// Create a new weighted value.
    pub fn new(raw_weighting: u16, value: T) -> Self {
        WeightedValue {
            raw_weighting,
            value,
        }
    }

    /// The weighting recalculated as a floating-point multiplier in the range
    /// `0.0..=1.0`.
    pub fn weighting(&self) -> f32 {
        f32::from(self.raw_weighting) / f32::from(u16::MAX)
    }
}

/// A value populated by a flow element for one of its properties.
///
/// The variants map onto the
/// [core value types](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#value-types):
///
/// - [`PropertyValue::String`], [`PropertyValue::Bool`],
///   [`PropertyValue::Integer`], [`PropertyValue::Double`] and
///   [`PropertyValue::Float`] are the scalar types.
/// - [`PropertyValue::StringList`] is the "array of strings" type.
/// - [`PropertyValue::JavaScript`] is the custom JavaScript type, a string
///   carrying a snippet intended to run on the client device. It is a distinct
///   variant (rather than a plain string) so the JSON and JavaScript builder
///   elements can recognize it without separate metadata.
/// - [`PropertyValue::KeyValueList`] is the "array of key-value-pair
///   collections" type, used where an element returns several sub-records (for
///   example TAC lookup or hardware-profile lookup).
///
/// `i64` and `f64` are used for the numeric types so that the full range of
/// values seen in 51Degrees data files round-trips without loss.
/// [`PropertyValue::Float`] keeps a value that is 32-bit where it comes from
/// as 32-bit, because widening it to `f64` adds digits the value never had.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PropertyValue {
    /// A string value.
    String(String),
    /// A boolean value.
    Bool(bool),
    /// An integer value.
    Integer(i64),
    /// A floating-point value.
    Double(f64),
    /// A 32-bit floating-point value. As an `f64` the 32-bit 0.1 is
    /// 0.10000000149011612, so it is kept as it is to be written as 0.1.
    /// There is no `From<f32>`, so a number written without a suffix is
    /// always a [`PropertyValue::Double`].
    Float(f32),
    /// A list of strings (the "array of strings" type).
    StringList(Vec<String>),
    /// A JavaScript snippet intended to execute on the client device.
    JavaScript(String),
    /// A list of key-value-pair collections, used to return multiple
    /// sub-records. Each entry is keyed and ordered by property name.
    KeyValueList(Vec<BTreeMap<String, PropertyValue>>),
}

impl PropertyValue {
    /// Borrow the value as a string slice if it is a [`PropertyValue::String`]
    /// or [`PropertyValue::JavaScript`].
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PropertyValue::String(s) | PropertyValue::JavaScript(s) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Return the value as a `bool` if it is a [`PropertyValue::Bool`].
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PropertyValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Return the value as an `i64` if it is a [`PropertyValue::Integer`].
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            PropertyValue::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Return the value as an `f64` if it is a [`PropertyValue::Double`], or
    /// a widened [`PropertyValue::Float`] or [`PropertyValue::Integer`].
    pub fn as_double(&self) -> Option<f64> {
        match self {
            PropertyValue::Double(d) => Some(*d),
            PropertyValue::Float(f) => Some(f64::from(*f)),
            PropertyValue::Integer(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// Borrow the value as a slice of strings if it is a
    /// [`PropertyValue::StringList`].
    pub fn as_string_list(&self) -> Option<&[String]> {
        match self {
            PropertyValue::StringList(v) => Some(v.as_slice()),
            _ => None,
        }
    }
}

impl From<&str> for PropertyValue {
    fn from(value: &str) -> Self {
        PropertyValue::String(value.to_owned())
    }
}

impl From<String> for PropertyValue {
    fn from(value: String) -> Self {
        PropertyValue::String(value)
    }
}

impl From<bool> for PropertyValue {
    fn from(value: bool) -> Self {
        PropertyValue::Bool(value)
    }
}

impl From<i64> for PropertyValue {
    fn from(value: i64) -> Self {
        PropertyValue::Integer(value)
    }
}

impl From<f64> for PropertyValue {
    fn from(value: f64) -> Self {
        PropertyValue::Double(value)
    }
}

impl From<Vec<String>> for PropertyValue {
    fn from(value: Vec<String>) -> Self {
        PropertyValue::StringList(value)
    }
}
