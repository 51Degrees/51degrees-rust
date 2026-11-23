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

//! The strongly-typed no-value wrapper used by every engine accessor.
//!
//! Engines expose individual property accessors that return an
//! [`AspectPropertyValue<T>`] rather than a bare `T`, so a caller can tell apart
//! "the engine determined no value for this request" from "the value is
//! present". This is the
//! [null-values rule](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#null-values)
//! expressed in the type system.
//!
//! It is distinct from the related "missing property" concept (a property that
//! is not present in the result set at all, for example because the data file
//! or resource key does not include it). That case is handled by
//! [`crate::MissingPropertyReason`] resolution on the engine, not by this type.
//!
//! It can hold a value or "no value". Being a Rust `enum`, the two states are
//! exhaustive and the compiler enforces handling both. The
//! [`AspectPropertyValue::value`] accessor returns a
//! `Result<&T, NoValueError>` rather than panicking, so "no value" never
//! escapes as an unexpected error.

use fiftyone_pipeline_core::NoValueError;

/// The default message reported when a no-value instance is unwrapped.
///
/// The message is `"This instance does not have a set value"`.
pub const DEFAULT_NO_VALUE_MESSAGE: &str = "This instance does not have a set value";

/// A property value that the engine may or may not have determined.
///
/// `T` is the underlying value type, for example [`String`], [`bool`] or a
/// `Vec<String>`. The two variants are:
///
/// - [`AspectPropertyValue::Value`] carries the determined value.
/// - [`AspectPropertyValue::NoValue`] carries the explanation of why no value
///   could be determined, for example because required evidence was missing or
///   no match was found.
///
/// # Example
///
/// ```
/// use fiftyone_pipeline_engines::AspectPropertyValue;
///
/// let present: AspectPropertyValue<String> = "Chrome".to_owned().into();
/// assert!(present.has_value());
/// assert_eq!(present.value().unwrap(), "Chrome");
///
/// let absent = AspectPropertyValue::<String>::no_value(
///     "No User-Agent was supplied.",
/// );
/// assert!(!absent.has_value());
/// assert!(absent.value().is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AspectPropertyValue<T> {
    /// A value was determined.
    Value(T),
    /// No value could be determined. The message explains why.
    NoValue {
        /// The explanation of why no value is available, surfaced through
        /// [`NoValueError`] when the value is accessed.
        message: String,
    },
}

impl<T> AspectPropertyValue<T> {
    /// Create an instance that holds a determined value.
    pub fn new(value: T) -> Self {
        AspectPropertyValue::Value(value)
    }

    /// Create a no-value instance with the supplied explanatory message.
    pub fn no_value(message: impl Into<String>) -> Self {
        AspectPropertyValue::NoValue {
            message: message.into(),
        }
    }

    /// Create a no-value instance with the default explanatory message.
    pub fn empty() -> Self {
        AspectPropertyValue::no_value(DEFAULT_NO_VALUE_MESSAGE)
    }

    /// True if this instance holds a value, false if it is a no-value.
    pub fn has_value(&self) -> bool {
        matches!(self, AspectPropertyValue::Value(_))
    }

    /// Borrow the underlying value.
    ///
    /// Returns `Err(NoValueError)` carrying the stored message if this is a
    /// no-value instance, rather than panicking.
    pub fn value(&self) -> Result<&T, NoValueError> {
        match self {
            AspectPropertyValue::Value(value) => Ok(value),
            AspectPropertyValue::NoValue { message } => Err(NoValueError::new(message.clone())),
        }
    }

    /// Take the underlying value by ownership, consuming this instance.
    ///
    /// Returns `Err(NoValueError)` if this is a no-value instance.
    pub fn into_value(self) -> Result<T, NoValueError> {
        match self {
            AspectPropertyValue::Value(value) => Ok(value),
            AspectPropertyValue::NoValue { message } => Err(NoValueError::new(message)),
        }
    }

    /// The no-value message, if this is a no-value instance.
    ///
    /// Returns `None` when a value is present.
    pub fn no_value_message(&self) -> Option<&str> {
        match self {
            AspectPropertyValue::Value(_) => None,
            AspectPropertyValue::NoValue { message } => Some(message),
        }
    }

    /// Borrow the value if present, like [`Option::as_ref`] but discarding the
    /// no-value message.
    pub fn as_option(&self) -> Option<&T> {
        match self {
            AspectPropertyValue::Value(value) => Some(value),
            AspectPropertyValue::NoValue { .. } => None,
        }
    }

    /// Consume this instance into an [`Option`], discarding the no-value
    /// message.
    pub fn into_option(self) -> Option<T> {
        match self {
            AspectPropertyValue::Value(value) => Some(value),
            AspectPropertyValue::NoValue { .. } => None,
        }
    }

    /// Return the contained value or the supplied default.
    pub fn unwrap_or(self, default: T) -> T {
        self.into_option().unwrap_or(default)
    }

    /// Return the contained value or compute it from the closure.
    pub fn unwrap_or_else<F: FnOnce() -> T>(self, f: F) -> T {
        self.into_option().unwrap_or_else(f)
    }

    /// Transform the contained value with `f`, preserving the no-value message
    /// unchanged. Equivalent to [`Option::map`] but it carries the message
    /// through.
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> AspectPropertyValue<U> {
        match self {
            AspectPropertyValue::Value(value) => AspectPropertyValue::Value(f(value)),
            AspectPropertyValue::NoValue { message } => AspectPropertyValue::NoValue { message },
        }
    }
}

impl<T: Default> AspectPropertyValue<T> {
    /// Return the contained value or the type's default.
    pub fn unwrap_or_default(self) -> T {
        self.into_option().unwrap_or_default()
    }
}

impl<T> From<T> for AspectPropertyValue<T> {
    fn from(value: T) -> Self {
        AspectPropertyValue::Value(value)
    }
}

impl<T> From<Option<T>> for AspectPropertyValue<T> {
    /// Build from an [`Option`], using the default no-value message for
    /// [`None`].
    fn from(value: Option<T>) -> Self {
        match value {
            Some(value) => AspectPropertyValue::Value(value),
            None => AspectPropertyValue::empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_round_trips() {
        let v = AspectPropertyValue::new(42i64);
        assert!(v.has_value());
        assert_eq!(*v.value().unwrap(), 42);
        assert_eq!(v.clone().into_value().unwrap(), 42);
        assert_eq!(v.no_value_message(), None);
    }

    #[test]
    fn no_value_reports_message() {
        let v = AspectPropertyValue::<String>::no_value("missing evidence");
        assert!(!v.has_value());
        let err = v.value().unwrap_err();
        assert_eq!(err.message, "missing evidence");
        assert_eq!(v.no_value_message(), Some("missing evidence"));
    }

    #[test]
    fn empty_uses_default_message() {
        let v = AspectPropertyValue::<bool>::empty();
        assert_eq!(v.no_value_message(), Some(DEFAULT_NO_VALUE_MESSAGE));
    }

    #[test]
    fn unwrap_helpers() {
        let present = AspectPropertyValue::new(3i64);
        assert_eq!(present.unwrap_or(99), 3);
        let absent = AspectPropertyValue::<i64>::empty();
        assert_eq!(absent.unwrap_or(99), 99);
        let absent2 = AspectPropertyValue::<i64>::empty();
        assert_eq!(absent2.unwrap_or_else(|| 7), 7);
        let absent3 = AspectPropertyValue::<i64>::empty();
        assert_eq!(absent3.unwrap_or_default(), 0);
    }

    #[test]
    fn map_preserves_no_value() {
        let present = AspectPropertyValue::new(2i64).map(|v| v * 10);
        assert_eq!(*present.value().unwrap(), 20);

        let absent = AspectPropertyValue::<i64>::no_value("x").map(|v| v * 10);
        assert!(!absent.has_value());
        assert_eq!(absent.no_value_message(), Some("x"));
    }

    #[test]
    fn from_option_and_value() {
        let some: AspectPropertyValue<i64> = Some(5).into();
        assert_eq!(*some.value().unwrap(), 5);
        let none: AspectPropertyValue<i64> = Option::<i64>::None.into();
        assert!(!none.has_value());
        let direct: AspectPropertyValue<&str> = "hi".into();
        assert_eq!(*direct.value().unwrap(), "hi");
    }

    #[test]
    fn as_and_into_option() {
        let present = AspectPropertyValue::new(1i64);
        assert_eq!(present.as_option(), Some(&1));
        assert_eq!(present.into_option(), Some(1));
        let absent = AspectPropertyValue::<i64>::empty();
        assert_eq!(absent.as_option(), None);
        assert_eq!(absent.into_option(), None);
    }
}
