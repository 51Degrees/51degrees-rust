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

//! The values the JSON builder collects before it writes them as text.

use indexmap::IndexMap;

/// One value in the JSON the builder writes.
///
/// The builder's steps collect the output as these values, and the last step
/// writes them as text. A step that changes the output adds, changes or
/// removes them.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum JsonValue {
    /// JSON `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A whole number.
    Integer(i64),
    /// A 64-bit floating point number. It is written in .NET's shortest
    /// round trip form, and NaN and the infinities as the strings `"NaN"`,
    /// `"Infinity"` and `"-Infinity"`, as .NET's builder writes them.
    Double(f64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<JsonValue>),
    /// An object, with its keys in the order they were added.
    Object(JsonObject),
}

impl From<bool> for JsonValue {
    fn from(value: bool) -> Self {
        JsonValue::Bool(value)
    }
}

impl From<i64> for JsonValue {
    fn from(value: i64) -> Self {
        JsonValue::Integer(value)
    }
}

impl From<f64> for JsonValue {
    fn from(value: f64) -> Self {
        JsonValue::Double(value)
    }
}

impl From<String> for JsonValue {
    fn from(value: String) -> Self {
        JsonValue::String(value)
    }
}

impl From<&str> for JsonValue {
    fn from(value: &str) -> Self {
        JsonValue::String(value.to_owned())
    }
}

impl From<Vec<JsonValue>> for JsonValue {
    fn from(value: Vec<JsonValue>) -> Self {
        JsonValue::Array(value)
    }
}

impl From<JsonObject> for JsonValue {
    fn from(value: JsonObject) -> Self {
        JsonValue::Object(value)
    }
}

/// A JSON object whose keys keep the order they were added in, which is the
/// order they are written in.
///
/// Inserting a key that is already present replaces its value and keeps its
/// place. Two objects are equal only when they hold the same keys in the same
/// order with equal values.
#[derive(Debug, Clone, Default)]
pub struct JsonObject(IndexMap<String, JsonValue>);

impl JsonObject {
    /// Create an empty object.
    pub fn new() -> Self {
        JsonObject(IndexMap::new())
    }

    /// The number of keys.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True if the object has no keys.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Set the value for a key and return the value it replaced. A new key
    /// goes last, and a key already present keeps its place.
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: impl Into<JsonValue>,
    ) -> Option<JsonValue> {
        self.0.insert(key.into(), value.into())
    }

    /// The value for a key.
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        self.0.get(key)
    }

    /// The value for a key, to change in place.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut JsonValue> {
        self.0.get_mut(key)
    }

    /// True if the object has the key.
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// Remove a key and return its value. The other keys keep their order.
    pub fn remove(&mut self, key: &str) -> Option<JsonValue> {
        self.0.shift_remove(key)
    }

    /// The keys and values in order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &JsonValue)> + '_ {
        self.0.iter().map(|(key, value)| (key.as_str(), value))
    }

    /// The keys in order.
    pub fn keys(&self) -> impl Iterator<Item = &str> + '_ {
        self.0.keys().map(String::as_str)
    }
}

impl PartialEq for JsonObject {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len() && self.0.iter().eq(other.0.iter())
    }
}

impl FromIterator<(String, JsonValue)> for JsonObject {
    fn from_iter<I: IntoIterator<Item = (String, JsonValue)>>(iter: I) -> Self {
        JsonObject(iter.into_iter().collect())
    }
}
