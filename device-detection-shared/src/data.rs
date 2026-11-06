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

//! The shared device-data interface and its concrete backing.
//!
//! [`DeviceData`](crate::DeviceData) is the typed view of a device-detection
//! result. [`DeviceDataBase`]
//! is the single concrete type both engines populate, so callers always retrieve
//! the same type under [`DEVICE_DATA_KEY`] regardless of which engine ran. See
//! the [crate documentation](crate) for how this keeps the engines
//! interface-compatible.

use std::any::Any;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use fiftyone_pipeline_core::{
    ElementData, MapElementData, NoValueError, PropertyMetaData, PropertyValue, PropertyValueType,
    TypedKey,
};
use fiftyone_pipeline_engines::{
    AspectData, AspectDataBase, AspectPropertyMetaData, AspectPropertyValue,
};

use crate::device_data::GENERATED_PROPERTY_TYPES;

/// The string data key device detection stores its element data under.
///
/// The element data key is `"device"`. The on-premise and cloud Rust
/// engines MUST use this same key so their results land in the same slot.
pub const DEVICE_ELEMENT_DATA_KEY: &str = "device";

/// The typed handle used to store and retrieve [`DeviceDataBase`] in a flow
/// data.
///
/// Pass this to
/// [`FlowData::get`](fiftyone_pipeline_core::FlowData::get) to read a device
/// result already downcast to `&DeviceDataBase`, or to
/// [`FlowData::get_or_add`](fiftyone_pipeline_core::FlowData::get_or_add) from an
/// engine to create the data exactly once. The key string is
/// [`DEVICE_ELEMENT_DATA_KEY`].
pub const DEVICE_DATA_KEY: TypedKey<DeviceDataBase> = TypedKey::new(DEVICE_ELEMENT_DATA_KEY);

/// The explanation a typed accessor gives for a property with no value, when
/// the engine gave no reason it is missing.
///
/// A property that
/// the engine did not populate surfaces as an
/// [`AspectPropertyValue::NoValue`](fiftyone_pipeline_engines::AspectPropertyValue::NoValue)
/// carrying a message rather than a hard error.
const ABSENT_MESSAGE: &str = "The property was not present in the device data.";

/// The explanation used when a property is present but stored as a value of an
/// unexpected type for the requested accessor.
const WRONG_TYPE_MESSAGE: &str =
    "The property was present but its stored value did not match the requested type.";

/// The type the warning for a read of a missing property names.
const DATA_TYPE_NAME: &str = "DeviceDataBase";

/// Reads the properties of one detection from its native results when they are
/// requested.
///
/// An on-premise engine gives its [`DeviceDataBase`] a reader rather than a copy
/// of every value, so a caller that reads one property does not pay to read the
/// others.
pub trait DevicePropertyReader: Send + Sync {
    /// The lowercase names of the properties this reader can read. A property
    /// is identified by its position in this list.
    fn property_names(&self) -> &[String];

    /// The position of the property with the given lowercase name, or `None`
    /// when this reader cannot read it.
    fn position(&self, name: &str) -> Option<usize>;

    /// Read the property at `position`. `Ok(None)` when it has no value, and
    /// `Err` with a message when the read fails.
    fn read_property(&self, position: usize) -> Result<Option<PropertyValue>, String>;
}

/// The outcome of reading one property.
type ReadOutcome = Result<Option<PropertyValue>, String>;

/// The values a [`DevicePropertyReader`] has read, so each property is read at
/// most once.
struct DeferredValues {
    reader: Box<dyn DevicePropertyReader>,
    /// The outcome of each read so far, by position in the reader's names.
    /// Empty until the first read.
    read: Mutex<Vec<Option<ReadOutcome>>>,
}

impl DeferredValues {
    /// The position of a property in the reader's names, or `None` when the
    /// reader cannot read it.
    fn position(&self, name: &str) -> Option<usize> {
        match self.reader.position(name) {
            Some(position) => Some(position),
            None if needs_lowercase(name) => self.reader.position(&name.to_lowercase()),
            None => None,
        }
    }

    /// The outcome of reading the property at `position`, read on its first
    /// request.
    fn outcome<'a>(
        &self,
        read: &'a mut Vec<Option<ReadOutcome>>,
        position: usize,
    ) -> &'a ReadOutcome {
        if read.is_empty() {
            read.resize(self.reader.property_names().len(), None);
        }
        read[position].get_or_insert_with(|| self.reader.read_property(position))
    }

    /// The position of a property the reader can read, or `None` when it
    /// cannot.
    fn readable(&self, name: &str) -> Option<usize> {
        self.position(name)
            .filter(|position| *position < self.reader.property_names().len())
    }

    /// The value of a property, read on its first request, or `None` when the
    /// reader cannot read it.
    fn get(&self, name: &str) -> Option<ReadOutcome> {
        let position = self.readable(name)?;
        let mut read = self.read.lock().unwrap_or_else(PoisonError::into_inner);
        Some(self.outcome(&mut read, position).clone())
    }

    /// The lowercase names of every property the reader holds, with or
    /// without a value. Nothing is read.
    fn keys(&self) -> Vec<String> {
        self.reader.property_names().to_vec()
    }
}

/// Whether a property name has letters that lowercasing would change.
fn needs_lowercase(name: &str) -> bool {
    !name.is_ascii() || name.bytes().any(|byte| byte.is_ascii_uppercase())
}

impl fmt::Debug for DeferredValues {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let read = self.read.lock().unwrap_or_else(PoisonError::into_inner);
        let read: Vec<(&String, &ReadOutcome)> = self
            .reader
            .property_names()
            .iter()
            .zip(read.iter())
            .filter_map(|(name, outcome)| outcome.as_ref().map(|outcome| (name, outcome)))
            .collect();
        f.debug_struct("DeferredValues")
            .field("read", &read)
            .finish_non_exhaustive()
    }
}

/// Where a property's value came from when it was looked up.
enum Lookup {
    /// The value, from the bag or the reader.
    Found(PropertyValue),
    /// No value, with the bag's explanation.
    Absent(NoValueError),
    /// Held with no value, with the message set for it.
    NoValue(String),
    /// The data does not hold the property, with the reason it is missing.
    Missing(String),
    /// The reader failed to read the value, with its message.
    Failed(String),
}

// The strongly-typed `DeviceData` read trait and its `impl` for
// `DeviceDataBase` are generated from the common metadata by the 51Degrees
// PropertyGenerator tool and live in the `crate::device_data` module. Each
// generated accessor delegates to one of the by-name dynamic-bag getters on
// `DeviceDataBase` below (`string_property`, `bool_property`,
// `integer_property`, `double_property`, `string_list_property`).

/// The concrete device-data type both the on-premise and cloud engines
/// populate.
///
/// `DeviceDataBase` embeds an [`AspectDataBase`] for the property bag, engine
/// keys and cache-hit flag, and implements [`DeviceData`](crate::DeviceData) by reading each typed
/// property out of that bag. Because it is the single shared type, it is what
/// makes the two engines interface-compatible: an engine builds one of these,
/// fills the bag through [`DeviceDataBase::set`] / [`DeviceDataBase::insert`],
/// and the caller reads it back through [`DEVICE_DATA_KEY`].
///
/// The bag keys are the property names (for example `IsMobile`,
/// `HardwareVendor`). Lookups are case-insensitive, so an engine that populates
/// `ismobile` and a caller that reads `IsMobile` agree.
///
/// An on-premise engine also attaches a [`DevicePropertyReader`] through
/// [`DeviceDataBase::with_reader`], and a property not in the bag is then read
/// from the native results the first time it is requested.
///
/// A cloud engine holds a property the response gives as null through
/// [`DeviceDataBase::insert_no_value`], so the property has no value and
/// carries the response's message.
///
/// A property the data does not hold in any of these ways is missing. When
/// the engine added a
/// [`MissingPropertySource`](fiftyone_pipeline_engines::MissingPropertySource)
/// to the wrapped [`AspectDataBase`], its no-value carries the reason, as
/// .NET's `PropertyMissingException` does, and each read of the property logs
/// a warning, as .NET's aspect data does.
///
/// # Example
///
/// ```
/// use fiftyone_device_detection_shared::{DeviceData, DeviceDataBase};
///
/// let device = DeviceDataBase::new()
///     .set("IsMobile", true)
///     .set("PlatformName", "Android");
///
/// assert_eq!(*device.is_mobile().value().unwrap(), true);
/// assert_eq!(device.platform_name().value().unwrap(), "Android");
///
/// // A property the engine did not populate is a no-value, not an error.
/// assert!(!device.browser_name().has_value());
///
/// // Anything reachable through the dynamic bag, including properties without
/// // a typed accessor, is still available by name.
/// use fiftyone_pipeline_core::ElementData;
/// assert_eq!(device.get("PlatformName").unwrap().as_str(), Some("Android"));
/// ```
#[derive(Debug, Clone)]
pub struct DeviceDataBase {
    base: AspectDataBase,
    /// The message of each property held with no value, as a string under
    /// the property's name, in the order the properties were added.
    no_values: MapElementData,
    /// For each property in `no_values`, in the same order, how many names
    /// the bag held when the property was added, so that `keys` lists it
    /// among the bag's names where it was added.
    no_value_places: Vec<usize>,
    /// The values read on request, when an engine attached a reader.
    deferred: Option<Arc<DeferredValues>>,
}

impl DeviceDataBase {
    /// Create an empty device data attributed to the device-detection engine
    /// (data key [`DEVICE_ELEMENT_DATA_KEY`]).
    pub fn new() -> Self {
        DeviceDataBase::from_base(AspectDataBase::new(DEVICE_ELEMENT_DATA_KEY))
    }

    /// Create a device data wrapping an existing [`AspectDataBase`].
    ///
    /// Used by an engine that has built its property bag separately, for
    /// example when restoring a cached result, so the engine keys and cache-hit
    /// flag carried by the `AspectDataBase` are preserved.
    pub fn from_base(base: AspectDataBase) -> Self {
        DeviceDataBase {
            base,
            no_values: MapElementData::new(),
            no_value_places: Vec::new(),
            deferred: None,
        }
    }

    /// Read every property not in the bag from `reader` when it is first
    /// requested, keeping each value once read.
    ///
    /// A value in the bag takes precedence over the reader's. Reads go through
    /// a lock, so the data can be shared between threads.
    pub fn with_reader(mut self, reader: impl DevicePropertyReader + 'static) -> Self {
        self.deferred = Some(Arc::new(DeferredValues {
            reader: Box::new(reader),
            read: Mutex::new(Vec::new()),
        }));
        self
    }

    /// Look a property up in the bag, then among the properties held with no
    /// value, then through the reader if there is one.
    fn lookup(&self, name: &str) -> Lookup {
        if let Some(value) = self.base.values().get_value(name) {
            return Lookup::Found(value.clone());
        }
        if let Some(message) = self.no_value_message(name) {
            return Lookup::NoValue(message.to_owned());
        }
        match self
            .deferred
            .as_ref()
            .and_then(|deferred| deferred.get(name))
        {
            Some(Ok(Some(value))) => Lookup::Found(value),
            Some(Err(message)) => Lookup::Failed(message),
            // The reader holds the property, with no value for this detection.
            Some(Ok(None)) => self.absent(name),
            // Neither the bag nor the reader holds the property.
            None => match self.base.missing_property(name, DATA_TYPE_NAME) {
                Some(missing) => Lookup::Missing(missing.description),
                None => self.absent(name),
            },
        }
    }

    /// The message of a property held with no value.
    fn no_value_message(&self, name: &str) -> Option<&str> {
        self.no_values
            .get_value(name)
            .and_then(PropertyValue::as_str)
    }

    /// Whether the bag, the properties held with no value or the reader hold
    /// a property.
    fn holds(&self, name: &str) -> bool {
        self.base.values().get_value(name).is_some()
            || self.no_values.get_value(name).is_some()
            || self
                .deferred
                .as_ref()
                .is_some_and(|deferred| deferred.readable(name).is_some())
    }

    /// The bag's own explanation of a property it has no value for.
    fn absent(&self, name: &str) -> Lookup {
        match self.base.values().get(name) {
            Ok(value) => Lookup::Found(value),
            Err(absent) => Lookup::Absent(absent),
        }
    }

    /// Convert a looked-up value to a typed value, explaining a no-value.
    fn typed<T>(
        &self,
        name: &str,
        convert: impl FnOnce(PropertyValue) -> Option<T>,
    ) -> AspectPropertyValue<T> {
        match self.lookup(name) {
            Lookup::Found(value) => match convert(value) {
                Some(typed) => AspectPropertyValue::new(typed),
                None => AspectPropertyValue::no_value(WRONG_TYPE_MESSAGE),
            },
            Lookup::Absent(_) => AspectPropertyValue::no_value(ABSENT_MESSAGE),
            Lookup::NoValue(message) | Lookup::Missing(message) | Lookup::Failed(message) => {
                AspectPropertyValue::no_value(message)
            }
        }
    }

    /// Set a property value, overwriting any existing value for that name, and
    /// return `self` for chaining during construction. The name is matched
    /// case-insensitively. Delegates to [`AspectDataBase::set`].
    pub fn set(mut self, name: impl AsRef<str>, value: impl Into<PropertyValue>) -> Self {
        self.base = self.base.set(name, value);
        self
    }

    /// Insert a property value by mutable reference (for use after the data has
    /// been created), overwriting any existing value for that name. Delegates
    /// to [`AspectDataBase::insert`].
    pub fn insert(&mut self, name: impl AsRef<str>, value: impl Into<PropertyValue>) {
        self.base.insert(name, value);
    }

    /// Hold a property with no value, with the message that says why, as a
    /// cloud engine does for a property the response gives as null.
    ///
    /// The typed accessor then gives a no-value carrying `message`, and
    /// [`ElementData::get`] fails with it. The data holds the property, so
    /// nothing is logged and no missing property reason is given. A value
    /// set for the same name takes precedence. The name is matched
    /// case-insensitively.
    pub fn insert_no_value(&mut self, name: impl AsRef<str>, message: impl Into<String>) {
        if self.no_values.get_value(name.as_ref()).is_none() {
            self.no_value_places.push(self.base.values().len());
        }
        self.no_values
            .insert(name, PropertyValue::String(message.into()));
    }

    /// Mark this data as having been served from a cache hit. Delegates to
    /// [`AspectDataBase::set_cache_hit`].
    pub fn set_cache_hit(&mut self) {
        self.base.set_cache_hit();
    }

    /// Borrow the wrapped [`AspectDataBase`], which holds the values set on
    /// this data directly. Values a reader supplies and properties held with
    /// no value are not in it, so read through [`ElementData`] or the typed
    /// getters to see every property.
    pub fn base(&self) -> &AspectDataBase {
        &self.base
    }

    /// Mutably borrow the wrapped [`AspectDataBase`].
    pub fn base_mut(&mut self) -> &mut AspectDataBase {
        &mut self.base
    }

    /// Read a string-typed property, wrapping the result in an
    /// [`AspectPropertyValue`]. Absent or wrong-typed values become a no-value
    /// with an explanatory message.
    ///
    /// The generated [`DeviceData`](crate::DeviceData) accessors delegate here
    /// (and to the sibling typed getters) by property name.
    pub fn string_property(&self, name: &str) -> AspectPropertyValue<String> {
        self.typed(name, |value| value.as_str().map(str::to_owned))
    }

    /// Read a boolean-typed property, wrapping the result in an
    /// [`AspectPropertyValue`].
    pub fn bool_property(&self, name: &str) -> AspectPropertyValue<bool> {
        self.typed(name, |value| value.as_bool())
    }

    /// Read an integer-typed property, wrapping the result in an
    /// [`AspectPropertyValue`].
    pub fn integer_property(&self, name: &str) -> AspectPropertyValue<i64> {
        self.typed(name, |value| value.as_integer())
    }

    /// Read a double-typed property, wrapping the result in an
    /// [`AspectPropertyValue`]. Single-precision properties are surfaced as
    /// doubles too, so this serves every floating-point Device Detection
    /// property.
    pub fn double_property(&self, name: &str) -> AspectPropertyValue<f64> {
        self.typed(name, |value| value.as_double())
    }

    /// Read a string-list-typed property, wrapping the result in an
    /// [`AspectPropertyValue`]. A single string value is accepted and returned
    /// as a one-element list, since some sources return a scalar where the
    /// property is declared as a list.
    pub fn string_list_property(&self, name: &str) -> AspectPropertyValue<Vec<String>> {
        self.typed(name, |value| match value {
            PropertyValue::StringList(list) => Some(list),
            PropertyValue::String(s) => Some(vec![s]),
            _ => None,
        })
    }
}

impl Default for DeviceDataBase {
    fn default() -> Self {
        DeviceDataBase::new()
    }
}

impl ElementData for DeviceDataBase {
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
        match self.lookup(name) {
            Lookup::Found(value) => Ok(value),
            Lookup::Absent(absent) => Err(absent),
            Lookup::NoValue(message) | Lookup::Missing(message) | Lookup::Failed(message) => {
                Err(NoValueError::new(message))
            }
        }
    }

    /// Whether the bag, the properties held with no value or the reader hold
    /// the property. Unless the wrapped
    /// [`AspectDataBase`] knows its missing properties, through a
    /// [`MissingPropertySource`](fiftyone_pipeline_engines::MissingPropertySource)
    /// or a failed cloud request, it answers `true`, because without one the
    /// data cannot say which properties should be there.
    fn has_property(&self, name: &str) -> bool {
        !self.base.knows_missing_properties() || self.holds(name)
    }

    /// The names of the properties in the bag and those held with no value,
    /// in the order they were added, then those the reader holds, with or
    /// without a value, as .NET's data lists every property it holds. A
    /// property with no value is listed so the JSON builder writes it as
    /// null with the reason, in the place the cloud response gave it.
    /// Listing reads nothing.
    fn keys(&self) -> Vec<String> {
        let bag = self.base.keys();
        let mut keys = Vec::with_capacity(bag.len() + self.no_value_places.len());
        let mut listed = 0;
        for (name, &place) in self.no_values.keys().into_iter().zip(&self.no_value_places) {
            // The bag only grows, so each place is at least the one before
            // and at most the bag's length.
            let place = place.clamp(listed, bag.len());
            keys.extend_from_slice(&bag[listed..place]);
            listed = place;
            if self.base.values().get_value(&name).is_none() {
                keys.push(name);
            }
        }
        keys.extend_from_slice(&bag[listed..]);
        if let Some(deferred) = &self.deferred {
            // The reader's names are distinct, so only the few names set
            // directly can repeat one of them.
            let set_directly = keys.len();
            for name in deferred.keys() {
                if !keys[..set_directly].contains(&name) {
                    keys.push(name);
                }
            }
        }
        keys
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl AspectData for DeviceDataBase {
    fn engine_keys(&self) -> &[String] {
        self.base.engine_keys()
    }

    fn cache_hit(&self) -> bool {
        self.base.cache_hit()
    }
}

// ---------------------------------------------------------------------------
// Property metadata
// ---------------------------------------------------------------------------

/// The declared core value type for a property, looked up in the generated
/// table case-insensitively.
///
/// Returns [`None`] when the property is not one of the generated set, so a
/// caller can decide its own default (the on-premise engine treats an unknown
/// data-file property as a plain string, the lossless default for the native
/// string reader).
pub fn declared_property_value_type(name: &str) -> Option<PropertyValueType> {
    GENERATED_PROPERTY_TYPES
        .iter()
        .find(|(generated, _)| generated.eq_ignore_ascii_case(name))
        .map(|(_, value_type)| *value_type)
}

/// Every property name the generated model surfaces, in name order.
pub fn generated_property_names() -> Vec<&'static str> {
    GENERATED_PROPERTY_TYPES
        .iter()
        .map(|(name, _)| *name)
        .collect()
}

/// Build the core [`PropertyMetaData`] for every generated property, owned by
/// the [`DEVICE_ELEMENT_DATA_KEY`] element. A wrapper with no richer metadata
/// source can publish this set directly.
pub fn default_property_metadata() -> Vec<PropertyMetaData> {
    GENERATED_PROPERTY_TYPES
        .iter()
        .map(|(name, value_type)| {
            PropertyMetaData::new(*name, DEVICE_ELEMENT_DATA_KEY, *value_type)
        })
        .collect()
}

/// Build the aspect [`AspectPropertyMetaData`] for every generated property.
pub fn default_aspect_property_metadata() -> Vec<AspectPropertyMetaData> {
    default_property_metadata()
        .into_iter()
        .map(AspectPropertyMetaData::from_core)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceData;
    use fiftyone_pipeline_engines::{
        EngineDeployment, MissingPropertyReason, MissingPropertySource,
    };
    use std::collections::HashMap;

    #[test]
    fn data_key_string_is_device() {
        assert_eq!(DEVICE_ELEMENT_DATA_KEY, "device");
        assert_eq!(DEVICE_DATA_KEY.name(), "device");
    }

    #[test]
    fn present_values_are_returned_typed() {
        let device = DeviceDataBase::new()
            .set("IsMobile", true)
            .set("HardwareVendor", "Apple")
            .set("DeviceType", "SmartPhone")
            .set("PlatformName", "iOS")
            .set("PlatformVersion", "17.0")
            .set("BrowserName", "Mobile Safari")
            .set("BrowserVersion", "17.0")
            .set("ScreenPixelsWidth", 1170i64)
            .set("ScreenPixelsHeight", 2532i64)
            .set("IsCrawler", false);

        assert!(*device.is_mobile().value().unwrap());
        assert_eq!(device.hardware_vendor().value().unwrap(), "Apple");
        assert_eq!(device.device_type().value().unwrap(), "SmartPhone");
        assert_eq!(device.platform_name().value().unwrap(), "iOS");
        assert_eq!(device.platform_version().value().unwrap(), "17.0");
        assert_eq!(device.browser_name().value().unwrap(), "Mobile Safari");
        assert_eq!(device.browser_version().value().unwrap(), "17.0");
        assert_eq!(*device.screen_pixels_width().value().unwrap(), 1170);
        assert_eq!(*device.screen_pixels_height().value().unwrap(), 2532);
        assert!(!*device.is_crawler().value().unwrap());
    }

    #[test]
    fn case_insensitive_property_names() {
        // An engine that writes lowercase keys is read by the canonical
        // accessor, because the bag folds case.
        let device = DeviceDataBase::new().set("ismobile", true);
        assert!(*device.is_mobile().value().unwrap());
    }

    #[test]
    fn absent_values_are_no_value_with_message() {
        let device = DeviceDataBase::new();
        let mobile = device.is_mobile();
        assert!(!mobile.has_value());
        assert_eq!(mobile.no_value_message(), Some(ABSENT_MESSAGE));

        let vendor = device.hardware_vendor();
        assert!(!vendor.has_value());
        assert_eq!(vendor.no_value_message(), Some(ABSENT_MESSAGE));
    }

    #[test]
    fn match_metrics_round_trip() {
        let device = DeviceDataBase::new()
            .set("DeviceId", "12345-67890-11111-22222")
            .set("MatchedNodes", 3i64)
            .set("Difference", 0i64)
            .set("Drift", 1i64)
            .set("Iterations", 42i64)
            .set("Method", "Exact");

        assert_eq!(
            device.device_id().value().unwrap(),
            "12345-67890-11111-22222"
        );
        assert_eq!(*device.matched_nodes().value().unwrap(), 3);
        assert_eq!(*device.difference().value().unwrap(), 0);
        assert_eq!(*device.drift().value().unwrap(), 1);
        assert_eq!(*device.iterations().value().unwrap(), 42);
        assert_eq!(device.method().value().unwrap(), "Exact");
    }

    #[test]
    fn list_property_accepts_list_and_scalar() {
        let from_list = DeviceDataBase::new().set(
            "HardwareName",
            vec!["iPhone".to_owned(), "iPhone 15".to_owned()],
        );
        assert_eq!(
            from_list.hardware_name().value().unwrap(),
            &["iPhone".to_owned(), "iPhone 15".to_owned()]
        );

        let from_scalar = DeviceDataBase::new().set("HardwareName", "iPhone");
        assert_eq!(
            from_scalar.hardware_name().value().unwrap(),
            &["iPhone".to_owned()]
        );
    }

    #[test]
    fn wrong_type_is_reported() {
        // IsMobile is stored as a string here, so the bool accessor cannot
        // honour it and reports the wrong-type message rather than a panic.
        let device = DeviceDataBase::new().set("IsMobile", "true");
        let mobile = device.is_mobile();
        assert!(!mobile.has_value());
        assert_eq!(mobile.no_value_message(), Some(WRONG_TYPE_MESSAGE));
    }

    #[test]
    fn aspect_and_element_data_are_inherited() {
        let mut device = DeviceDataBase::new().set("IsMobile", true);
        assert_eq!(device.engine_keys(), ["device"]);
        assert!(!device.cache_hit());
        device.set_cache_hit();
        assert!(device.cache_hit());

        // Reachable as the dynamic bag and through the aspect trait object.
        let as_element: &dyn ElementData = &device;
        assert_eq!(as_element.get("IsMobile").unwrap().as_bool(), Some(true));
        assert!(as_element.get("DeviceType").is_err());
    }

    #[test]
    fn dynamic_bag_reaches_untyped_properties() {
        // A property without a typed accessor is still readable by name,
        // demonstrating the documented fallback.
        let device = DeviceDataBase::new().set("ScreenMMWidth", 71.0f64);
        assert_eq!(device.get("ScreenMMWidth").unwrap().as_double(), Some(71.0));
        assert!(device.keys().iter().any(|k| k == "screenmmwidth"));
    }

    /// A reader over fixed outcomes that counts how often each is read.
    struct CountingReader {
        names: Vec<String>,
        outcomes: Vec<Result<Option<PropertyValue>, String>>,
        reads: Arc<Mutex<HashMap<String, usize>>>,
    }

    impl CountingReader {
        fn new() -> (Self, Arc<Mutex<HashMap<String, usize>>>) {
            let reads = Arc::new(Mutex::new(HashMap::new()));
            let reader = CountingReader {
                names: ["ismobile", "platformname", "browsername", "hardwarevendor"]
                    .iter()
                    .map(|name| (*name).to_owned())
                    .collect(),
                outcomes: vec![
                    Ok(Some(PropertyValue::Bool(true))),
                    Ok(Some(PropertyValue::from("Android"))),
                    Ok(None),
                    Err("the native read failed".to_owned()),
                ],
                reads: Arc::clone(&reads),
            };
            (reader, reads)
        }
    }

    impl DevicePropertyReader for CountingReader {
        fn property_names(&self) -> &[String] {
            &self.names
        }

        fn position(&self, name: &str) -> Option<usize> {
            self.names.iter().position(|known| known == name)
        }

        fn read_property(&self, position: usize) -> Result<Option<PropertyValue>, String> {
            *self
                .reads
                .lock()
                .unwrap()
                .entry(self.names[position].clone())
                .or_insert(0) += 1;
            self.outcomes[position].clone()
        }
    }

    fn reads_of(reads: &Mutex<HashMap<String, usize>>, name: &str) -> usize {
        reads.lock().unwrap().get(name).copied().unwrap_or(0)
    }

    #[test]
    fn reader_reads_only_the_properties_requested() {
        let (reader, reads) = CountingReader::new();
        let device = DeviceDataBase::new().with_reader(reader);
        assert!(*device.is_mobile().value().unwrap());
        assert_eq!(reads_of(&reads, "ismobile"), 1);
        assert_eq!(reads_of(&reads, "platformname"), 0);
        assert_eq!(reads_of(&reads, "hardwarevendor"), 0);
    }

    #[test]
    fn reader_reads_each_property_once() {
        let (reader, reads) = CountingReader::new();
        let device = DeviceDataBase::new().with_reader(reader);
        assert!(*device.is_mobile().value().unwrap());
        assert_eq!(device.get("ISMOBILE").unwrap().as_bool(), Some(true));
        assert!(*device.is_mobile().value().unwrap());
        assert_eq!(reads_of(&reads, "ismobile"), 1);
    }

    #[test]
    fn keys_list_every_property_with_or_without_a_value() {
        let (reader, reads) = CountingReader::new();
        let device = DeviceDataBase::new()
            .set("DeviceId", "1-2-3-4")
            .with_reader(reader);
        // The bag first, then the reader's names in its order, including
        // browsername, which has no value, and hardwarevendor, whose read
        // fails.
        assert_eq!(
            device.keys(),
            [
                "deviceid",
                "ismobile",
                "platformname",
                "browsername",
                "hardwarevendor"
            ]
        );
        // Listing the names reads nothing.
        for name in ["ismobile", "platformname", "browsername", "hardwarevendor"] {
            assert_eq!(reads_of(&reads, name), 0, "{name}");
        }
        // Each listed property without a value gives its reason.
        assert_eq!(
            device.get("browsername").unwrap_err().message,
            "No value for property 'browsername'."
        );
        assert_eq!(
            device.get("hardwarevendor").unwrap_err().message,
            "the native read failed"
        );
    }

    #[test]
    fn a_value_in_the_bag_wins_over_the_reader() {
        let (reader, reads) = CountingReader::new();
        let mut device = DeviceDataBase::new().with_reader(reader);
        device.insert("IsMobile", false);
        assert!(!*device.is_mobile().value().unwrap());
        assert_eq!(reads_of(&reads, "ismobile"), 0);
    }

    #[test]
    fn a_property_without_a_value_reads_as_absent() {
        let (reader, _) = CountingReader::new();
        let device = DeviceDataBase::new().with_reader(reader);
        let browser = device.browser_name();
        assert!(!browser.has_value());
        assert_eq!(browser.no_value_message(), Some(ABSENT_MESSAGE));
        assert!(device.get("BrowserName").is_err());
        assert!(device.get("NotAProperty").is_err());
    }

    #[test]
    fn a_failed_read_is_a_no_value_carrying_its_message() {
        let (reader, _) = CountingReader::new();
        let device = DeviceDataBase::new().with_reader(reader);
        let vendor = device.hardware_vendor();
        assert_eq!(vendor.no_value_message(), Some("the native read failed"));
        assert_eq!(
            device.get("HardwareVendor").unwrap_err().message,
            "the native read failed"
        );
    }

    /// The details of an engine whose metadata lists the reader's properties.
    fn missing_property_source() -> Arc<MissingPropertySource> {
        let properties = ["IsMobile", "PlatformName", "BrowserName", "HardwareVendor"]
            .iter()
            .map(|name| {
                AspectPropertyMetaData::new(
                    *name,
                    DEVICE_ELEMENT_DATA_KEY,
                    PropertyValueType::String,
                )
                .with_data_tiers(["Lite"])
            })
            .collect();
        Arc::new(
            MissingPropertySource::new(
                DEVICE_ELEMENT_DATA_KEY,
                "TestEngine",
                EngineDeployment::OnPremise,
                "Lite",
                properties,
            )
            .with_typed_properties(|name: &str| declared_property_value_type(name).is_some()),
        )
    }

    #[test]
    fn a_property_the_reader_cannot_read_gives_the_missing_reason() {
        let (reader, _) = CountingReader::new();
        let source = missing_property_source();
        let mut device = DeviceDataBase::new().with_reader(reader);
        device
            .base_mut()
            .add_missing_property_source(Arc::clone(&source));

        // DeviceType has an accessor, and the metadata does not list it.
        let device_type = source.reason("DeviceType");
        assert_eq!(
            device_type.reason,
            MissingPropertyReason::DataFileUpgradeRequired
        );
        assert_eq!(
            device.device_type().no_value_message(),
            Some(device_type.description.as_str())
        );
        assert_eq!(
            device.get("DeviceType").unwrap_err().message,
            device_type.description
        );

        let unknown = source.reason("NotAProperty");
        assert_eq!(unknown.reason, MissingPropertyReason::Unknown);
        assert_eq!(
            device.get("NotAProperty").unwrap_err().message,
            unknown.description
        );
        assert_eq!(
            device.string_property("NotAProperty").no_value_message(),
            Some(unknown.description.as_str())
        );
    }

    #[test]
    fn a_property_the_reader_reads_without_a_value_is_not_missing() {
        let (reader, _) = CountingReader::new();
        let mut device = DeviceDataBase::new().with_reader(reader);
        device
            .base_mut()
            .add_missing_property_source(missing_property_source());
        assert_eq!(
            device.browser_name().no_value_message(),
            Some(ABSENT_MESSAGE)
        );
        assert_eq!(
            device.get("BrowserName").unwrap_err().message,
            "No value for property 'BrowserName'."
        );
        assert!(*device.is_mobile().value().unwrap());
        assert_eq!(
            device.hardware_vendor().no_value_message(),
            Some("the native read failed")
        );
    }

    #[test]
    fn without_a_source_a_missing_property_keeps_the_bag_explanation() {
        let (reader, _) = CountingReader::new();
        let device = DeviceDataBase::new().with_reader(reader);
        assert_eq!(
            device.device_type().no_value_message(),
            Some(ABSENT_MESSAGE)
        );
        assert_eq!(
            device.get("DeviceType").unwrap_err().message,
            "No value for property 'DeviceType'."
        );
    }

    #[test]
    fn a_source_explains_a_property_missing_from_data_with_no_reader() {
        let mut device = DeviceDataBase::new().set("IsMobile", true);
        let source = missing_property_source();
        device
            .base_mut()
            .add_missing_property_source(Arc::clone(&source));
        assert_eq!(
            device.device_type().no_value_message(),
            Some(source.reason("DeviceType").description.as_str())
        );
        assert!(*device.is_mobile().value().unwrap());
    }

    #[test]
    fn a_source_lets_the_data_say_what_it_holds() {
        let (reader, reads) = CountingReader::new();
        let mut device = DeviceDataBase::new()
            .set("DeviceId", "1-2-3-4")
            .with_reader(reader);
        assert!(device.has_property("DeviceType"));

        device
            .base_mut()
            .add_missing_property_source(missing_property_source());
        // In the bag, then read with a value, with none, and failing.
        for name in ["DeviceId", "IsMobile", "browsername", "HardwareVendor"] {
            assert!(device.has_property(name), "{name}");
        }
        assert!(!device.has_property("DeviceType"));
        assert!(!device.has_property("NotAProperty"));
        // Asking reads nothing.
        assert_eq!(reads_of(&reads, "ismobile"), 0);
    }

    /// A message a cloud response gives for a property it returns as null.
    const NO_MATCH: &str = "No matching profiles were found.";

    #[test]
    fn a_property_held_with_no_value_gives_its_message() {
        let mut device = DeviceDataBase::new().set("IsMobile", true);
        device.insert_no_value("HardwareVendor", NO_MATCH);
        let vendor = device.hardware_vendor();
        assert!(!vendor.has_value());
        assert_eq!(vendor.no_value_message(), Some(NO_MATCH));
        assert_eq!(device.get("hardwarevendor").unwrap_err().message, NO_MATCH);
        assert!(device.has_property("HARDWAREVENDOR"));
        assert_eq!(device.keys(), ["ismobile", "hardwarevendor"]);
        assert!(device.base().values().get_value("hardwarevendor").is_none());
    }

    #[test]
    fn a_property_held_with_no_value_is_not_missing() {
        let mut device = DeviceDataBase::new();
        device.insert_no_value("HardwareVendor", NO_MATCH);
        device
            .base_mut()
            .add_missing_property_source(missing_property_source());
        assert!(device.has_property("HardwareVendor"));
        // The source would give "The reason for this is unknown...".
        assert_eq!(device.hardware_vendor().no_value_message(), Some(NO_MATCH));
        assert_eq!(device.get("HardwareVendor").unwrap_err().message, NO_MATCH);
        assert!(!device.has_property("DeviceType"));
    }

    #[test]
    fn keys_keep_the_order_properties_were_added_in() {
        let mut device = DeviceDataBase::new();
        device.insert_no_value("HardwareVendor", NO_MATCH);
        device.insert("IsMobile", true);
        device.insert("BrowserName", "Chrome");
        device.insert_no_value("PlatformName", NO_MATCH);
        device.insert("DeviceType", "Mobile");
        // A name given again keeps its first place.
        device.insert_no_value("hardwarevendor", NO_MATCH);
        device.insert("ismobile", false);
        assert_eq!(
            device.keys(),
            [
                "hardwarevendor",
                "ismobile",
                "browsername",
                "platformname",
                "devicetype"
            ]
        );
    }

    #[test]
    fn a_value_for_the_same_name_wins_over_no_value() {
        let mut device = DeviceDataBase::new();
        device.insert_no_value("HardwareVendor", NO_MATCH);
        device.insert("hardwarevendor", "Apple");
        assert_eq!(device.hardware_vendor().value().unwrap(), "Apple");
        assert_eq!(device.keys(), ["hardwarevendor"]);
    }

    #[test]
    fn no_value_wins_over_the_reader_and_is_listed_once() {
        let (reader, reads) = CountingReader::new();
        let mut device = DeviceDataBase::new()
            .set("DeviceId", "1-2-3-4")
            .with_reader(reader);
        device.insert_no_value("PlatformName", NO_MATCH);
        assert_eq!(device.platform_name().no_value_message(), Some(NO_MATCH));
        assert_eq!(reads_of(&reads, "platformname"), 0);
        assert_eq!(
            device.keys(),
            [
                "deviceid",
                "platformname",
                "ismobile",
                "browsername",
                "hardwarevendor"
            ]
        );
    }

    // WebAssembly has no threads to spawn.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn reads_from_several_threads_agree_and_read_once() {
        let (reader, reads) = CountingReader::new();
        let device = Arc::new(DeviceDataBase::new().with_reader(reader));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let device = Arc::clone(&device);
                std::thread::spawn(move || device.platform_name().value().unwrap().clone())
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().unwrap(), "Android");
        }
        assert_eq!(reads_of(&reads, "platformname"), 1);
    }
}
