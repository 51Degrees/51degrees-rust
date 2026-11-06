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

    /// Return the value as an `f64` if it is a [`PropertyValue::Double`], a
    /// [`PropertyValue::Float`] or a [`PropertyValue::Integer`].
    ///
    /// A `Float` gives the `f64` nearest its shortest decimal text, so a
    /// latitude of 18.543 reads as 18.543 rather than as the widened
    /// 18.542999267578125.
    pub fn as_double(&self) -> Option<f64> {
        match self {
            PropertyValue::Double(d) => Some(*d),
            PropertyValue::Float(f) => Some(f.to_string().parse().unwrap_or(f64::from(*f))),
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

/// The number of decimal places a value converted from an Azimuth or
/// Declination, such as a latitude or a longitude, is given to.
const ANGLE_DECIMAL_PLACES: usize = 3;

// The quarter place `round_angle` moves a half by must be far wider than the
// gap between doubles near 180, which is about 3e-14.
const _: () = assert!(ANGLE_DECIMAL_PLACES <= 9);

/// Round a value converted from an Azimuth or Declination, such as a latitude
/// or a longitude, to 3 decimal places, giving the `f32` nearest the rounded
/// decimal, as the
/// [value types specification](https://github.com/51Degrees/specifications/blob/0e86a4642eea1585f2d7e3ad8eef6af7c68e9aaa/pipeline-specification/features/value-types.md#rounding-of-azimuth-and-declination-values)
/// requires.
///
/// The value is rounded before it is narrowed, because the `f32` nearest a
/// value can round to a different last place. A value exactly half way
/// between two places rounds away from zero, as the 51Degrees C library
/// writes it, and minus zero is given as zero.
///
/// ```
/// use fiftyone_pipeline_core::round_angle;
///
/// // A stored Declination of 6751 converts to 18.542741172521133.
/// assert_eq!(round_angle(6751.0 * 90.0 / 32767.0), 18.543);
/// ```
pub fn round_angle(value: f64) -> f32 {
    if !value.is_finite() {
        return value as f32;
    }
    // A double exactly half way between two places is an odd number of half
    // places. A double holds only binary fractions, so such a value is an odd
    // multiple of 1 / 2^(places + 1), which is a sixteenth for 3 places, such
    // as 0.0625. Formatting rounds a half to even, so it is moved a quarter of
    // a place away from zero first.
    let halves = value * (1_u64 << (ANGLE_DECIMAL_PLACES + 1)) as f64;
    let value = if halves.fract() == 0.0 && halves % 2.0 != 0.0 {
        let quarter_place = 0.25 / 10_f64.powi(ANGLE_DECIMAL_PLACES as i32);
        value + value.signum() * quarter_place
    } else {
        value
    };
    let rounded = format!("{value:.ANGLE_DECIMAL_PLACES$}")
        .parse::<f32>()
        .unwrap_or(value as f32);
    if rounded == 0.0 {
        0.0
    } else {
        rounded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stored Declination or Azimuth converted as the C library converts
    /// it, multiplying before dividing.
    fn converted(stored: i16, max_angle: f64) -> f64 {
        f64::from(stored) * max_angle / 32767.0
    }

    #[test]
    fn an_angle_is_the_f32_nearest_its_value_to_three_places() {
        // 18.542741172521133, the specification's example.
        assert_eq!(round_angle(converted(6751, 90.0)), 18.543);
        assert_eq!(round_angle(converted(-6751, 90.0)), -18.543);
        assert_eq!(round_angle(converted(7948, 90.0)), 21.831);
        assert_eq!(round_angle(converted(1, 90.0)), 0.003);
        assert_eq!(round_angle(0.0), 0.0);
        assert_eq!(round_angle(90.0), 90.0);
        assert_eq!(round_angle(-180.0), -180.0);
        assert_eq!(round_angle(converted(6751, 90.0)).to_string(), "18.543");
    }

    /// Rounding the `f32` nearest these values would give 21.212 and
    /// 23.479, so they show the value is rounded before it is narrowed.
    #[test]
    fn an_angle_is_rounded_before_it_is_narrowed() {
        // 21.21250038148137, whose nearest f32 is 21.212499618530273.
        assert_eq!(round_angle(converted(7723, 90.0)), 21.213);
        // 23.47849971007416, whose nearest f32 is 23.478500366210938.
        assert_eq!(round_angle(converted(4274, 180.0)), 23.478);
    }

    #[test]
    fn a_half_rounds_away_from_zero() {
        assert_eq!(round_angle(0.0625), 0.063);
        assert_eq!(round_angle(-0.0625), -0.063);
        assert_eq!(round_angle(51.3125), 51.313);
        assert_eq!(round_angle(0.1875), 0.188);
        // An eighth has three places, so it is not a half.
        assert_eq!(round_angle(0.125), 0.125);
    }

    /// Every value half way between two places from -180 to 180 rounds away
    /// from zero. These are the odd sixteenths.
    #[test]
    fn every_half_rounds_away_from_zero() {
        let mut checked = 0;
        for sixteenths in (-2879_i32..=2879).step_by(2) {
            let value = f64::from(sixteenths) / 16.0;
            // In thousandths the value is an odd number of halves, so its
            // size rounded away from zero is one more half.
            let size = (sixteenths.unsigned_abs() * 125).div_ceil(2);
            let sign = if sixteenths < 0 { "-" } else { "" };
            let text = format!("{sign}{}.{:03}", size / 1000, size % 1000);
            assert_eq!(round_angle(value), text.parse::<f32>().unwrap(), "{value}");
            checked += 1;
        }
        assert_eq!(checked, 2880);
    }

    #[test]
    fn minus_zero_is_given_as_zero() {
        let rounded = round_angle(-0.0001);
        assert_eq!(rounded, 0.0);
        assert!(rounded.is_sign_positive());
    }

    #[test]
    fn a_float_reads_as_the_double_nearest_its_decimal() {
        assert_eq!(PropertyValue::Float(18.543).as_double(), Some(18.543));
        assert_eq!(PropertyValue::Float(-0.063).as_double(), Some(-0.063));
        assert_eq!(PropertyValue::Float(0.1).as_double(), Some(0.1));
        assert_eq!(PropertyValue::Float(1e9).as_double(), Some(1e9));
        assert_eq!(
            PropertyValue::Float(f32::INFINITY).as_double(),
            Some(f64::INFINITY)
        );
        assert!(PropertyValue::Float(f32::NAN).as_double().unwrap().is_nan());
    }

    #[test]
    fn a_value_that_is_not_finite_is_kept() {
        assert!(round_angle(f64::NAN).is_nan());
        assert_eq!(round_angle(f64::INFINITY), f32::INFINITY);
        assert_eq!(round_angle(f64::NEG_INFINITY), f32::NEG_INFINITY);
    }
}
