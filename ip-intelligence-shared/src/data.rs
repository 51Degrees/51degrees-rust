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

//! The IP-intelligence element-data backing both engines populate.
//!
//! This is the IP Intelligence counterpart of the device-detection
//! `device-detection-shared::data` module. It defines the concrete backing
//! ([`IpIntelligenceDataBase`]) that both engines populate, the [`TypedKey`]
//! ([`IP_DATA_KEY`]) the result is stored under, and the value stores and
//! metadata helpers around them. The strongly-typed read trait
//! ([`IpIntelligenceData`](crate::IpIntelligenceData)) and its accessor set live
//! in the generated [`crate::ip_intelligence_data`] module; this module supplies
//! the by-name typed stores those accessors delegate to. The crate root
//! re-exports every public item.
//!
//! # Plain and weighted properties
//!
//! Most IP Intelligence properties are *plain*: a single IP resolves to one
//! definite value of the property's declared type, for example the country name
//! ([`String`]), an `IsVPN` flag ([`bool`]), the latitude ([`f32`]) or the UTC
//! offset ([`i32`]). The generated accessors return those directly, wrapped in
//! an [`AspectPropertyValue`] so an absent value carries the engine's no-value
//! reason.
//!
//! A small number of properties are *weighted*: the engine returns several
//! candidate values for them, each with a `0.0..=1.0` confidence weighting. In
//! the data file these are the `weightedstring` properties
//! (`CountryCodesGeographical`, `CountryCodesPopulation` and `Mcc`). Their
//! accessors return an ordered `Vec<`[`WeightedValue`]`<String>>`, and they also
//! appear in the dynamic [`ElementData::get`] bag as a
//! [`PropertyValue::KeyValueList`] of `value`/`weight` records.
//!
//! Both engines populate the same backing the same way: a plain value is stored
//! through one of the `set_*` builders (for example
//! [`IpIntelligenceDataBase::set_string`]); a weighted property is stored
//! through [`IpIntelligenceDataBase::set_weighted_string`], which sorts the
//! candidates high weighting first and writes the flattened dynamic-bag mirror.

use std::any::Any;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use fiftyone_pipeline_core::{
    ElementData, NoValueError, PropertyMetaData, PropertyValue, PropertyValueType, TypedKey,
    WeightedValue,
};
use fiftyone_pipeline_engines::{
    AspectData, AspectDataBase, AspectPropertyMetaData, AspectPropertyValue,
};

use crate::ip_intelligence_data::GENERATED_PROPERTY_TYPES;

/// The string element-data key under which IP-intelligence data is stored in a
/// flow data.
///
/// Both the cloud and on-premise engines store the data under the key `"ip"`.
pub const IP_DATA_KEY_NAME: &str = "ip";

/// The typed handle used to store and retrieve [`IpIntelligenceDataBase`] in a
/// [`fiftyone_pipeline_core::FlowData`].
///
/// Both the cloud and on-premise engines write their result under this single
/// key, so an application can call `flow_data.get(IP_DATA_KEY)` and read the
/// result without caring which engine produced it.
pub const IP_DATA_KEY: TypedKey<IpIntelligenceDataBase> = TypedKey::new(IP_DATA_KEY_NAME);

/// The key under which each flattened weighted record stores its candidate
/// value in the dynamic property bag mirror.
pub const WEIGHTED_RECORD_VALUE_KEY: &str = "value";

/// The key under which each flattened weighted record stores its `0.0..=1.0`
/// weighting multiplier in the dynamic property bag mirror.
pub const WEIGHTED_RECORD_WEIGHT_KEY: &str = "weight";

/// The type the warning for a read of a missing property names.
const DATA_TYPE_NAME: &str = "IpIntelligenceDataBase";

// ---------------------------------------------------------------------------
// Value stores
// ---------------------------------------------------------------------------

/// A single plain value for one property, or a no-value with the reason.
///
/// This is the unit kept in [`IpIntelligenceDataBase`]'s plain typed stores. It
/// mirrors the two states of an [`AspectPropertyValue`]: a present value, or a
/// no-value message explaining why the engine determined nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueStore<T> {
    /// A present value.
    Value(T),
    /// No value could be determined. The message explains why.
    NoValue {
        /// The explanation surfaced through the accessor when read.
        message: String,
    },
}

/// A weighted value store for a single weighted property.
///
/// Held only for the genuinely weighted properties (`CountryCodesGeographical`,
/// `CountryCodesPopulation`, `Mcc`). It mirrors the two states of an
/// [`AspectPropertyValue`]: an ordered list of weighted candidates, or a
/// no-value message. The list is held high weighting first; the constructor
/// sorts it for you.
#[derive(Debug, Clone, PartialEq)]
pub enum WeightedStore<T> {
    /// One or more weighted candidate values, ordered high weighting first.
    Values(Vec<WeightedValue<T>>),
    /// No value could be determined. The message explains why.
    NoValue {
        /// The explanation surfaced through the accessor when read.
        message: String,
    },
}

impl<T> WeightedStore<T> {
    /// Build a store from a list of weighted candidates, sorting it high
    /// weighting first.
    ///
    /// An empty list is stored as the [`WeightedStore::Values`] variant (an
    /// empty distribution), not as a no-value. Use [`WeightedStore::no_value`]
    /// when the engine could determine nothing.
    pub fn values(mut list: Vec<WeightedValue<T>>) -> Self {
        // Sort descending by raw weighting so the most probable candidate is
        // first. A stable sort keeps the engine's original order for ties.
        list.sort_by_key(|item| std::cmp::Reverse(item.raw_weighting));
        WeightedStore::Values(list)
    }

    /// Build a no-value store carrying the supplied explanatory message.
    pub fn no_value(message: impl Into<String>) -> Self {
        WeightedStore::NoValue {
            message: message.into(),
        }
    }

    /// Borrow the weighted list, or `Err(NoValueError)` carrying the stored
    /// message if this is a no-value.
    pub fn list(&self) -> Result<&[WeightedValue<T>], NoValueError> {
        match self {
            WeightedStore::Values(list) => Ok(list),
            WeightedStore::NoValue { message } => Err(NoValueError::new(message.clone())),
        }
    }
}

/// Build the flattened key-value mirror of a weighted list for the dynamic
/// property bag.
///
/// Each weighted candidate becomes a small ordered record with a `value` entry
/// (`to_value(&candidate)`) and a `weight` entry (the `0.0..=1.0` multiplier).
/// The result is wrapped as a [`PropertyValue::KeyValueList`] so it can be
/// stored in the embedded [`AspectDataBase`] and read through
/// [`ElementData::get`].
fn weighted_mirror<T, F>(list: &[WeightedValue<T>], to_value: F) -> PropertyValue
where
    F: Fn(&T) -> PropertyValue,
{
    let records = list
        .iter()
        .map(|weighted| {
            let mut record = BTreeMap::new();
            record.insert(
                WEIGHTED_RECORD_VALUE_KEY.to_owned(),
                to_value(&weighted.value),
            );
            record.insert(
                WEIGHTED_RECORD_WEIGHT_KEY.to_owned(),
                PropertyValue::Double(f64::from(weighted.weighting())),
            );
            record
        })
        .collect();
    PropertyValue::KeyValueList(records)
}

// ---------------------------------------------------------------------------
// Values read on request
// ---------------------------------------------------------------------------

/// Reads the properties of one lookup from its native results when they are
/// requested.
///
/// An on-premise engine gives its [`IpIntelligenceDataBase`] a reader rather
/// than a copy of every value, so a caller that reads one property does not pay
/// to read the others, some of which are costly, such as a polygon written out
/// as text.
pub trait IpIntelligencePropertyReader: Send + Sync {
    /// The lowercase names of the properties this reader can read. A property
    /// is identified by its position in this list.
    fn property_names(&self) -> &[String];

    /// The position of the property with the given lowercase name, or `None`
    /// when this reader cannot read it.
    fn position(&self, name: &str) -> Option<usize>;

    /// Read the property at `position` into `data` through its `set_*` methods.
    /// For a property with no value, a `set_*_no_value` method records the
    /// reason and lists the property among the keys, so the JSON builder
    /// writes it as null with the reason, and
    /// [`IpIntelligenceDataBase::set_string_no_value_unlisted`] records the
    /// reason alone. Leaving `data` unchanged gives the default reason. `Err`
    /// carries the message of a read that failed.
    fn read_property(
        &self,
        position: usize,
        data: &mut IpIntelligenceDataBase,
    ) -> Result<(), String>;
}

/// The values an [`IpIntelligencePropertyReader`] has read, so each property is
/// read at most once.
struct DeferredValues {
    reader: Box<dyn IpIntelligencePropertyReader>,
    state: Mutex<DeferredState>,
}

struct DeferredState {
    /// The values read so far.
    values: IpIntelligenceDataBase,
    /// Whether each property has been read, by position in the reader's names,
    /// with the message of the read if it failed. Empty until the first read.
    read: Vec<Option<Option<String>>>,
}

impl DeferredValues {
    /// The position of a property in the reader's names, or `None` when the
    /// reader cannot read it.
    fn position(&self, name: &str) -> Option<usize> {
        let position = match self.reader.position(name) {
            Some(position) => Some(position),
            None if needs_lowercase(name) => self.reader.position(&key(name)),
            None => None,
        }?;
        (position < self.reader.property_names().len()).then_some(position)
    }

    /// Read the property at `position` if it has not been read, returning the
    /// message of the read if it failed.
    fn read(&self, state: &mut DeferredState, position: usize) -> Option<String> {
        if state.read.is_empty() {
            state.read.resize(self.reader.property_names().len(), None);
        }
        if let Some(failure) = &state.read[position] {
            return failure.clone();
        }
        let failure = self.reader.read_property(position, &mut state.values).err();
        state.read[position] = Some(failure.clone());
        failure
    }

    /// Look at the values read so far, after reading the property at `position`
    /// if it has not been read. `Err` carries the message of a read that
    /// failed.
    fn with<R>(
        &self,
        position: usize,
        look: impl FnOnce(&IpIntelligenceDataBase) -> R,
    ) -> Result<R, String> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let state = &mut *state;
        if let Some(message) = self.read(state, position) {
            return Err(message);
        }
        Ok(look(&state.values))
    }

    /// The lowercase names of the properties read with a value or with a
    /// listed reason for none, reading any not yet read.
    fn keys(&self) -> Vec<String> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let state = &mut *state;
        for position in 0..self.reader.property_names().len() {
            self.read(state, position);
        }
        state.values.keys()
    }
}

/// Whether a property name has letters that lowercasing would change.
fn needs_lowercase(name: &str) -> bool {
    !name.is_ascii() || name.bytes().any(|byte| byte.is_ascii_uppercase())
}

impl fmt::Debug for DeferredValues {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        f.debug_struct("DeferredValues")
            .field("read", &state.read)
            .field("values", &state.values)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// Concrete data
// ---------------------------------------------------------------------------

/// The concrete IP-intelligence element data both engines produce.
///
/// Embeds an [`AspectDataBase`] for the standard aspect plumbing (the dynamic
/// property bag, the engine keys and the cache-hit flag) and keeps the typed
/// values in dedicated stores next to it: one plain store per value type, plus a
/// weighted store for the genuinely weighted properties. The generated
/// [`IpIntelligenceData`](crate::IpIntelligenceData) accessors read these stores
/// by name; the dynamic [`ElementData`] bag holds a mirror so string-keyed
/// lookups also work.
///
/// An on-premise engine also attaches an [`IpIntelligencePropertyReader`]
/// through [`IpIntelligenceDataBase::with_reader`], and a property not set on
/// the data directly is then read from the native results the first time it is
/// requested.
///
/// A cloud engine sets a property the response gives as null through the
/// `set_*_no_value` setters, so the data holds the property with no value and
/// the response's message, and lists it among its keys.
///
/// A property neither set directly nor one the reader can read is missing.
/// When the engine added a
/// [`MissingPropertySource`](fiftyone_pipeline_engines::MissingPropertySource)
/// to the embedded [`AspectDataBase`], its no-value carries the reason, as
/// .NET's `PropertyMissingException` does, and each read of the property logs
/// a warning, as .NET's aspect data does.
#[derive(Debug, Clone)]
pub struct IpIntelligenceDataBase {
    base: AspectDataBase,
    strings: BTreeMap<String, ValueStore<String>>,
    floats: BTreeMap<String, ValueStore<f32>>,
    integers: BTreeMap<String, ValueStore<i32>>,
    booleans: BTreeMap<String, ValueStore<bool>>,
    weighted_strings: BTreeMap<String, WeightedStore<String>>,
    /// The lowercase name of each property set with no value, in the order
    /// first set, with how many names the bag held then, so that `keys`
    /// lists it among the bag's names where it was set.
    no_value_places: Vec<(String, usize)>,
    /// The values read on request, when an engine attached a reader.
    deferred: Option<Arc<DeferredValues>>,
}

impl IpIntelligenceDataBase {
    /// Create empty IP-intelligence data attributed to the engine with the given
    /// data key (typically [`IP_DATA_KEY_NAME`]).
    pub fn new(engine_key: impl Into<String>) -> Self {
        Self::from_base(AspectDataBase::new(engine_key))
    }

    /// Create IP-intelligence data wrapping an existing [`AspectDataBase`],
    /// preserving its bag, engine keys and cache-hit flag.
    pub fn from_base(base: AspectDataBase) -> Self {
        IpIntelligenceDataBase {
            base,
            strings: BTreeMap::new(),
            floats: BTreeMap::new(),
            integers: BTreeMap::new(),
            booleans: BTreeMap::new(),
            weighted_strings: BTreeMap::new(),
            no_value_places: Vec::new(),
            deferred: None,
        }
    }

    /// Read every property not set on this data directly from `reader` when it
    /// is first requested, keeping each value once read.
    ///
    /// A value set directly takes precedence over the reader's. Reads go
    /// through a lock, so the data can be shared between threads.
    pub fn with_reader(mut self, reader: impl IpIntelligencePropertyReader + 'static) -> Self {
        self.deferred = Some(Arc::new(DeferredValues {
            reader: Box::new(reader),
            state: Mutex::new(DeferredState {
                values: IpIntelligenceDataBase::new(IP_DATA_KEY_NAME),
                read: Vec::new(),
            }),
        }));
        self
    }

    /// The reader, and the position of `name` in its names, when there is a
    /// reader that can read the property.
    fn readable(&self, name: &str) -> Option<(&DeferredValues, usize)> {
        let deferred = self.deferred.as_deref()?;
        Some((deferred, deferred.position(name)?))
    }

    /// A typed value read through the reader, or the no-value for a missing
    /// property when the reader cannot read it.
    fn deferred_value<T>(
        &self,
        name: &str,
        look: impl FnOnce(&IpIntelligenceDataBase) -> AspectPropertyValue<T>,
    ) -> AspectPropertyValue<T> {
        match self.readable(name) {
            Some((deferred, position)) => deferred
                .with(position, look)
                .unwrap_or_else(|message| AspectPropertyValue::no_value(message)),
            None => self.missing(name),
        }
    }

    /// The no-value for a property this data does not hold, carrying the
    /// reason it is missing when the engine gave a source for one, and the
    /// default no-value otherwise. A reason also logs a warning.
    fn missing<T>(&self, name: &str) -> AspectPropertyValue<T> {
        match self.base.missing_property(name, DATA_TYPE_NAME) {
            Some(missing) => AspectPropertyValue::no_value(missing.description),
            None => AspectPropertyValue::empty(),
        }
    }

    /// Whether the property is set on this data directly, with or without a
    /// value, or the reader can read it.
    fn holds(&self, name: &str) -> bool {
        let key = key(name);
        self.base.values().get_value(name).is_some()
            || self.strings.contains_key(&key)
            || self.floats.contains_key(&key)
            || self.integers.contains_key(&key)
            || self.booleans.contains_key(&key)
            || self.weighted_strings.contains_key(&key)
            || self.readable(name).is_some()
    }

    /// The message of a property recorded through one of the
    /// `set_*_no_value` setters.
    fn stored_no_value(&self, name: &str) -> Option<&str> {
        let key = if needs_lowercase(name) {
            Cow::Owned(key(name))
        } else {
            Cow::Borrowed(name)
        };
        let key = key.as_ref();
        no_value_message(self.strings.get(key))
            .or_else(|| no_value_message(self.floats.get(key)))
            .or_else(|| no_value_message(self.integers.get(key)))
            .or_else(|| no_value_message(self.booleans.get(key)))
            .or_else(|| match self.weighted_strings.get(key) {
                Some(WeightedStore::NoValue { message }) => Some(message.as_str()),
                _ => None,
            })
    }

    /// Borrow the embedded [`AspectDataBase`], which holds the values set on
    /// this data directly. Values a reader supplies are not in it, so read
    /// through [`ElementData`] or the typed getters to see every value.
    pub fn base(&self) -> &AspectDataBase {
        &self.base
    }

    /// Mutably borrow the embedded [`AspectDataBase`].
    pub fn base_mut(&mut self) -> &mut AspectDataBase {
        &mut self.base
    }

    /// Mark this data as having been served from a cache hit.
    pub fn set_cache_hit(&mut self) {
        self.base.set_cache_hit();
    }

    /// Record that another engine (by data key) also contributed to this data.
    pub fn add_engine_key(&mut self, engine_key: impl Into<String>) {
        self.base.add_engine_key(engine_key);
    }

    // -- plain setters ------------------------------------------------------

    /// Store a plain string-valued property and its dynamic-bag mirror.
    ///
    /// This is the entry point a wrapper calls for the registered-range and
    /// textual location properties (including the IP range bounds). The property
    /// name is matched case-insensitively by the accessors and the dynamic bag.
    pub fn set_string(&mut self, name: impl AsRef<str>, value: impl Into<String>) {
        let value = value.into();
        self.base
            .insert(name.as_ref(), PropertyValue::String(value.clone()));
        self.strings
            .insert(key(name.as_ref()), ValueStore::Value(value));
    }

    /// Store a plain single-precision float property (`Latitude`, `Longitude`)
    /// and its dynamic-bag mirror, which is a [`PropertyValue::Float`] so the
    /// value is written with a 32-bit float's own digits.
    pub fn set_float(&mut self, name: impl AsRef<str>, value: f32) {
        self.base.insert(name.as_ref(), PropertyValue::Float(value));
        self.floats
            .insert(key(name.as_ref()), ValueStore::Value(value));
    }

    /// Store a plain integer property (`TimeZoneOffset`, `AccuracyRadiusMin`,
    /// the diversity scores) and its dynamic-bag mirror.
    pub fn set_integer(&mut self, name: impl AsRef<str>, value: i32) {
        self.base
            .insert(name.as_ref(), PropertyValue::Integer(i64::from(value)));
        self.integers
            .insert(key(name.as_ref()), ValueStore::Value(value));
    }

    /// Store a plain boolean property (`IsBroadband`, `IsVPN`, `IsEu`, and the
    /// rest of the network flags) and its dynamic-bag mirror.
    pub fn set_boolean(&mut self, name: impl AsRef<str>, value: bool) {
        self.base.insert(name.as_ref(), PropertyValue::Bool(value));
        self.booleans
            .insert(key(name.as_ref()), ValueStore::Value(value));
    }

    /// Store a weighted string property (`CountryCodesGeographical`,
    /// `CountryCodesPopulation`, `Mcc`) and its dynamic-bag mirror.
    ///
    /// The list is sorted high weighting first and mirrored into the dynamic bag
    /// as a [`PropertyValue::KeyValueList`] of `value`/`weight` records.
    pub fn set_weighted_string(
        &mut self,
        name: impl AsRef<str>,
        values: Vec<WeightedValue<String>>,
    ) {
        let store = WeightedStore::values(values);
        let WeightedStore::Values(list) = &store else {
            unreachable!("WeightedStore::values always returns Values");
        };
        self.base.insert(
            name.as_ref(),
            weighted_mirror(list, |s| PropertyValue::String(s.clone())),
        );
        self.weighted_strings.insert(key(name.as_ref()), store);
    }

    /// Record that a string-valued property had no value, with an explanatory
    /// message. The matching accessor then returns a no-value carrying it,
    /// and [`ElementData::keys`] lists the property.
    pub fn set_string_no_value(&mut self, name: impl AsRef<str>, message: impl Into<String>) {
        let key = self.no_value_key(name.as_ref());
        self.strings.insert(
            key,
            ValueStore::NoValue {
                message: message.into(),
            },
        );
    }

    /// Record that a string-valued property had no value, with an explanatory
    /// message, as [`Self::set_string_no_value`] does, but leave the property
    /// out of [`ElementData::keys`], so the JSON builder writes nothing for
    /// it. The on-premise engine records `Ip` and `IpV6` this way when the
    /// evidence holds no address of their kind, as .NET's data leaves them
    /// out of its dictionary.
    pub fn set_string_no_value_unlisted(
        &mut self,
        name: impl AsRef<str>,
        message: impl Into<String>,
    ) {
        self.strings.insert(
            key(name.as_ref()),
            ValueStore::NoValue {
                message: message.into(),
            },
        );
    }

    /// Record that a float-valued property had no value, with a message.
    pub fn set_float_no_value(&mut self, name: impl AsRef<str>, message: impl Into<String>) {
        let key = self.no_value_key(name.as_ref());
        self.floats.insert(
            key,
            ValueStore::NoValue {
                message: message.into(),
            },
        );
    }

    /// Record that an integer property had no value, with a message.
    pub fn set_integer_no_value(&mut self, name: impl AsRef<str>, message: impl Into<String>) {
        let key = self.no_value_key(name.as_ref());
        self.integers.insert(
            key,
            ValueStore::NoValue {
                message: message.into(),
            },
        );
    }

    /// Record that a boolean property had no value, with a message.
    pub fn set_boolean_no_value(&mut self, name: impl AsRef<str>, message: impl Into<String>) {
        let key = self.no_value_key(name.as_ref());
        self.booleans.insert(
            key,
            ValueStore::NoValue {
                message: message.into(),
            },
        );
    }

    /// Record that a weighted string property had no value, with a message.
    pub fn set_weighted_string_no_value(
        &mut self,
        name: impl AsRef<str>,
        message: impl Into<String>,
    ) {
        let key = self.no_value_key(name.as_ref());
        self.weighted_strings
            .insert(key, WeightedStore::no_value(message));
    }

    /// The store key for a property being set with no value, noting where
    /// [`ElementData::keys`] lists the property the first time it is set.
    fn no_value_key(&mut self, name: &str) -> String {
        let key = key(name);
        if !self
            .no_value_places
            .iter()
            .any(|(placed, _)| *placed == key)
        {
            self.no_value_places
                .push((key.clone(), self.base.values().len()));
        }
        key
    }

    // -- plain getters ------------------------------------------------------

    /// Read a plain string property by name, as the generated string accessors
    /// do.
    pub fn string(&self, name: &str) -> AspectPropertyValue<String> {
        match self.strings.get(&key(name)) {
            Some(store) => value_store_to_aspect(Some(store)),
            None => self.deferred_value(name, |values| values.string(name)),
        }
    }

    /// Read a plain float property by name.
    pub fn float(&self, name: &str) -> AspectPropertyValue<f32> {
        match self.floats.get(&key(name)) {
            Some(store) => value_store_to_aspect(Some(store)),
            None => self.deferred_value(name, |values| values.float(name)),
        }
    }

    /// Read a plain integer property by name.
    pub fn integer(&self, name: &str) -> AspectPropertyValue<i32> {
        match self.integers.get(&key(name)) {
            Some(store) => value_store_to_aspect(Some(store)),
            None => self.deferred_value(name, |values| values.integer(name)),
        }
    }

    /// Read a plain boolean property by name.
    pub fn boolean(&self, name: &str) -> AspectPropertyValue<bool> {
        match self.booleans.get(&key(name)) {
            Some(store) => value_store_to_aspect(Some(store)),
            None => self.deferred_value(name, |values| values.boolean(name)),
        }
    }

    /// Read a weighted string property by name.
    ///
    /// Returns [`AspectPropertyValue::NoValue`] when the property is absent or
    /// was recorded as a no-value. The list is ordered high weighting first.
    /// This is how the genuinely weighted properties (`CountryCodesGeographical`,
    /// `CountryCodesPopulation`, `Mcc`) are read.
    pub fn weighted_string(&self, name: &str) -> AspectPropertyValue<Vec<WeightedValue<String>>> {
        match self.weighted_strings.get(&key(name)) {
            Some(WeightedStore::Values(list)) => AspectPropertyValue::new(list.clone()),
            Some(WeightedStore::NoValue { message }) => {
                AspectPropertyValue::no_value(message.clone())
            }
            None => self.deferred_value(name, |values| values.weighted_string(name)),
        }
    }
}

/// Normalize a property name to the lowercase form used as the store key, so
/// lookups are case-insensitive like the core property bag.
fn key(name: &str) -> String {
    name.to_lowercase()
}

/// Convert an optional [`ValueStore`] into an [`AspectPropertyValue`].
///
/// A present [`ValueStore::Value`] becomes a value; a present
/// [`ValueStore::NoValue`] carries its message; an absent store (the property
/// was never set) becomes the default no-value.
fn value_store_to_aspect<T: Clone>(store: Option<&ValueStore<T>>) -> AspectPropertyValue<T> {
    match store {
        Some(ValueStore::Value(value)) => AspectPropertyValue::new(value.clone()),
        Some(ValueStore::NoValue { message }) => AspectPropertyValue::no_value(message.clone()),
        None => AspectPropertyValue::empty(),
    }
}

/// The message of a [`ValueStore::NoValue`].
fn no_value_message<T>(store: Option<&ValueStore<T>>) -> Option<&str> {
    match store {
        Some(ValueStore::NoValue { message }) => Some(message),
        _ => None,
    }
}

impl ElementData for IpIntelligenceDataBase {
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
        if let Some(value) = self.base.values().get_value(name) {
            return Ok(value.clone());
        }
        // A no-value set directly is held, so it is not missing, and it takes
        // precedence over the reader, as it does in the typed getters.
        if let Some(message) = self.stored_no_value(name) {
            return Err(NoValueError::new(message));
        }
        match self.readable(name) {
            // The values read hold the reason a property read with no value
            // has none.
            Some((deferred, position)) => deferred
                .with(position, |values| values.get(name))
                .unwrap_or_else(|message| Err(NoValueError::new(message))),
            // The property is missing, and the embedded base gives the reason.
            None => self.base.values().get(name).map_err(|absent| {
                self.base
                    .missing_property(name, DATA_TYPE_NAME)
                    .map_or(absent, |missing| NoValueError::new(missing.description))
            }),
        }
    }

    /// Whether the property is set on this data or the reader can read it.
    /// Unless the embedded [`AspectDataBase`] knows its missing properties,
    /// through a
    /// [`MissingPropertySource`](fiftyone_pipeline_engines::MissingPropertySource)
    /// or a failed cloud request, it answers `true`, as the [`AspectDataBase`]
    /// does.
    fn has_property(&self, name: &str) -> bool {
        !self.base.knows_missing_properties() || self.holds(name)
    }

    /// The names of the properties set on this data, with a value or with
    /// none, in the order they were first set, then those the reader reads
    /// in the same way. A property set with no value is listed so the JSON
    /// builder writes it as null with the reason, among the values where the
    /// engine set it, as .NET's data lists it, unless it was set through
    /// [`IpIntelligenceDataBase::set_string_no_value_unlisted`]. With a
    /// reader attached this reads every property not yet read, because
    /// whether a property has a value is only known once it is read.
    fn keys(&self) -> Vec<String> {
        let bag = self.base.keys();
        let mut keys = Vec::with_capacity(bag.len() + self.no_value_places.len());
        let mut listed = 0;
        for (name, place) in &self.no_value_places {
            // The bag only grows, so each place is at least the one before
            // and at most the bag's length.
            let place = (*place).clamp(listed, bag.len());
            keys.extend_from_slice(&bag[listed..place]);
            listed = place;
            // A value set for the same name is listed in the bag's place.
            if self.base.values().get_value(name).is_none() {
                keys.push(name.clone());
            }
        }
        keys.extend_from_slice(&bag[listed..]);
        if let Some(deferred) = &self.deferred {
            // The reader's names are distinct, so only the names set directly
            // can repeat one of them.
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

impl AspectData for IpIntelligenceDataBase {
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

/// The value type a weighted property is reported as in metadata.
///
/// Weighted lists are surfaced through the dynamic bag as
/// [`PropertyValueType::KeyValueList`] (the flattened `value`/`weight` records),
/// so that is the type published for a weighted property, regardless of the
/// underlying candidate type.
pub const WEIGHTED_PROPERTY_VALUE_TYPE: PropertyValueType = PropertyValueType::KeyValueList;

/// Every property name the generated model surfaces, in name order.
///
/// Derived from the generated [`GENERATED_PROPERTY_TYPES`] table. A wrapper that
/// wants to populate or iterate the full documented set (such as the on-premise
/// engine) uses this rather than hard-coding names.
pub fn generated_property_names() -> Vec<&'static str> {
    GENERATED_PROPERTY_TYPES
        .iter()
        .map(|(name, _)| *name)
        .collect()
}

/// The declared core value type for a property, looked up in the generated
/// table case-insensitively.
///
/// Returns [`None`] when the property is not one of the generated set, so a
/// caller can decide its own default (the on-premise engine treats an unknown
/// requested property as the weighted key-value-list type).
pub fn declared_property_value_type(name: &str) -> Option<PropertyValueType> {
    GENERATED_PROPERTY_TYPES
        .iter()
        .find(|(generated, _)| generated.eq_ignore_ascii_case(name))
        .map(|(_, value_type)| *value_type)
}

/// Build the core [`PropertyMetaData`] for every generated property, owned by
/// the [`IP_DATA_KEY_NAME`] element.
///
/// A wrapper that has no richer source of metadata (for example a minimal
/// on-premise configuration) can publish this set directly. Each property is
/// typed with its declared value type from the generated table.
pub fn default_property_metadata() -> Vec<PropertyMetaData> {
    GENERATED_PROPERTY_TYPES
        .iter()
        .map(|(name, value_type)| PropertyMetaData::new(*name, IP_DATA_KEY_NAME, *value_type))
        .collect()
}

/// Build the aspect [`AspectPropertyMetaData`] for every generated property,
/// wrapping [`default_property_metadata`] with empty descriptions and tiers.
pub fn default_aspect_property_metadata() -> Vec<AspectPropertyMetaData> {
    default_property_metadata()
        .into_iter()
        .map(AspectPropertyMetaData::from_core)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IpIntelligenceData;
    use fiftyone_pipeline_engines::{
        EngineDeployment, MissingPropertyReason, MissingPropertySource, DEFAULT_NO_VALUE_MESSAGE,
    };

    /// A small weighted string list, deliberately out of weighting order so the
    /// setter's sort is exercised.
    fn sample_countries() -> Vec<WeightedValue<String>> {
        vec![
            WeightedValue::new(20_000, "GB".to_owned()),
            WeightedValue::new(60_000, "FR".to_owned()),
            WeightedValue::new(40_000, "DE".to_owned()),
        ]
    }

    #[test]
    fn data_key_is_ip() {
        assert_eq!(IP_DATA_KEY.name(), "ip");
        assert_eq!(IP_DATA_KEY_NAME, "ip");
    }

    #[test]
    fn plain_string_accessor_round_trips() {
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
        data.set_string("Country", "United Kingdom");
        assert_eq!(data.country().value().unwrap().as_str(), "United Kingdom");
        // Case-insensitive read through the lowercase store key.
        assert_eq!(
            data.string("country").value().unwrap().as_str(),
            "United Kingdom"
        );
        // The dynamic bag mirrors the plain value, not a key-value list.
        assert_eq!(
            data.get("Country").unwrap(),
            PropertyValue::String("United Kingdom".to_owned())
        );
    }

    #[test]
    fn float_integer_and_boolean_accessors_round_trip() {
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
        data.set_float("Latitude", 51.5);
        data.set_integer("TimeZoneOffset", 60);
        data.set_boolean("IsVPN", true);
        assert_eq!(*data.latitude().value().unwrap(), 51.5_f32);
        assert_eq!(*data.time_zone_offset().value().unwrap(), 60_i32);
        assert!(*data.is_vpn().value().unwrap());
        // The boolean is mirrored into the dynamic bag as a bool.
        assert_eq!(data.get("IsVPN").unwrap(), PropertyValue::Bool(true));
    }

    /// A float is mirrored into the dynamic bag as a 32-bit float rather than
    /// widened to a double, which would add digits it never had, and reads as
    /// a double as the double nearest the same decimal.
    #[test]
    fn a_float_is_mirrored_as_a_32_bit_float() {
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
        data.set_float("Latitude", 18.543);
        assert_eq!(data.get("Latitude").unwrap(), PropertyValue::Float(18.543));
        assert_eq!(data.get("latitude").unwrap().as_double(), Some(18.543));
    }

    #[test]
    fn explicit_no_value_carries_message() {
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
        data.set_string_no_value("Country", "not in this tier");
        data.set_float_no_value("Latitude", "no location");
        data.set_integer_no_value("TimeZoneOffset", "no location");
        data.set_boolean_no_value("IsVPN", "not in this tier");
        assert!(data.country().value().is_err());
        assert!(data.latitude().value().is_err());
        assert!(data.time_zone_offset().value().is_err());
        assert!(data.is_vpn().value().is_err());
    }

    #[test]
    fn absent_property_is_a_no_value() {
        let data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
        assert!(data.country().value().is_err());
        assert!(data.country_codes_geographical().value().is_err());
    }

    #[test]
    fn weighted_string_accessor_orders_high_weighting_first() {
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
        data.set_weighted_string("CountryCodesGeographical", sample_countries());
        let value = data.country_codes_geographical();
        let list = value.value().expect("a weighted list");
        // Highest weighting first: FR (60k), DE (40k), GB (20k).
        assert_eq!(list[0].value, "FR");
        assert_eq!(list[1].value, "DE");
        assert_eq!(list[2].value, "GB");
    }

    #[test]
    fn weighting_multiplier_is_zero_to_one() {
        let value = WeightedValue::new(65_535, "x".to_owned());
        assert!((value.weighting() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn weighted_dynamic_bag_mirror_is_key_value_list() {
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
        data.set_weighted_string("CountryCodesPopulation", sample_countries());
        let bag = data.get("CountryCodesPopulation").expect("present in bag");
        match bag {
            PropertyValue::KeyValueList(records) => {
                assert_eq!(records.len(), 3);
                assert!(records[0].contains_key(WEIGHTED_RECORD_VALUE_KEY));
                assert!(records[0].contains_key(WEIGHTED_RECORD_WEIGHT_KEY));
            }
            other => panic!("expected a key-value list, got {other:?}"),
        }
    }

    #[test]
    fn usable_through_trait_objects() {
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
        data.set_string("CountryCode", "GB");
        let as_trait: &dyn IpIntelligenceData = &data;
        assert_eq!(as_trait.country_code().value().unwrap().as_str(), "GB");
    }

    #[test]
    fn typed_metadata_carries_declared_types() {
        let meta = default_property_metadata();
        assert_eq!(meta.len(), GENERATED_PROPERTY_TYPES.len());
        let lat = meta.iter().find(|m| m.name == "Latitude").unwrap();
        assert_eq!(lat.value_type, PropertyValueType::Float);
        let tz = meta.iter().find(|m| m.name == "TimeZoneOffset").unwrap();
        assert_eq!(tz.value_type, PropertyValueType::Integer);
        let vpn = meta.iter().find(|m| m.name == "IsVPN").unwrap();
        assert_eq!(vpn.value_type, PropertyValueType::Bool);
        let codes = meta
            .iter()
            .find(|m| m.name == "CountryCodesGeographical")
            .unwrap();
        assert_eq!(codes.value_type, PropertyValueType::KeyValueList);
    }

    /// A reader over fixed values that counts how often each property is read.
    struct CountingReader {
        names: Vec<String>,
        reads: Arc<Mutex<BTreeMap<String, usize>>>,
    }

    impl CountingReader {
        fn new() -> (Self, Arc<Mutex<BTreeMap<String, usize>>>) {
            let reads = Arc::new(Mutex::new(BTreeMap::new()));
            let reader = CountingReader {
                names: [
                    "country",
                    "isvpn",
                    "countrycodesgeographical",
                    "region",
                    "areas",
                ]
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
                reads: Arc::clone(&reads),
            };
            (reader, reads)
        }
    }

    impl IpIntelligencePropertyReader for CountingReader {
        fn property_names(&self) -> &[String] {
            &self.names
        }

        fn position(&self, name: &str) -> Option<usize> {
            self.names.iter().position(|known| known == name)
        }

        fn read_property(
            &self,
            position: usize,
            data: &mut IpIntelligenceDataBase,
        ) -> Result<(), String> {
            let name = self.names[position].as_str();
            *self
                .reads
                .lock()
                .unwrap()
                .entry(name.to_owned())
                .or_insert(0) += 1;
            match name {
                "country" => data.set_string("Country", "United Kingdom"),
                "isvpn" => data.set_boolean("IsVPN", false),
                "countrycodesgeographical" => {
                    data.set_weighted_string("CountryCodesGeographical", sample_countries())
                }
                "areas" => return Err("the native read failed".to_owned()),
                _ => {}
            }
            Ok(())
        }
    }

    fn reads_of(reads: &Mutex<BTreeMap<String, usize>>, name: &str) -> usize {
        reads.lock().unwrap().get(name).copied().unwrap_or(0)
    }

    #[test]
    fn reader_reads_only_the_properties_requested() {
        let (reader, reads) = CountingReader::new();
        let data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(reader);
        assert_eq!(data.country().value().unwrap().as_str(), "United Kingdom");
        assert_eq!(reads_of(&reads, "country"), 1);
        assert_eq!(reads_of(&reads, "areas"), 0);
        assert_eq!(reads_of(&reads, "isvpn"), 0);
    }

    #[test]
    fn reader_reads_each_property_once() {
        let (reader, reads) = CountingReader::new();
        let data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(reader);
        assert!(!*data.is_vpn().value().unwrap());
        assert_eq!(data.get("ISVPN").unwrap(), PropertyValue::Bool(false));
        assert!(!*data.is_vpn().value().unwrap());
        assert_eq!(reads_of(&reads, "isvpn"), 1);
    }

    #[test]
    fn weighted_values_read_on_request_keep_their_order() {
        let (reader, _) = CountingReader::new();
        let data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(reader);
        let value = data.country_codes_geographical();
        let list = value.value().expect("a weighted list");
        assert_eq!(list[0].value, "FR");
        assert!(matches!(
            data.get("CountryCodesGeographical"),
            Ok(PropertyValue::KeyValueList(_))
        ));
    }

    #[test]
    fn keys_list_only_the_properties_with_a_value() {
        let (reader, reads) = CountingReader::new();
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(reader);
        data.set_string("IpRangeStart", "192.0.2.0");
        let mut keys = data.keys();
        keys.sort();
        assert_eq!(
            keys,
            [
                "country",
                "countrycodesgeographical",
                "iprangestart",
                "isvpn"
            ]
        );
        for name in [
            "country",
            "isvpn",
            "countrycodesgeographical",
            "region",
            "areas",
        ] {
            assert_eq!(reads_of(&reads, name), 1, "{name}");
        }
    }

    /// A reader that records a reason for each property it gives no value,
    /// as the on-premise engine does, with `IpV6` recorded unlisted.
    struct NoValueReader {
        names: Vec<String>,
    }

    impl NoValueReader {
        fn new() -> Self {
            NoValueReader {
                names: ["asn", "latitude", "countrycodespopulation", "ip", "ipv6"]
                    .iter()
                    .map(|name| (*name).to_owned())
                    .collect(),
            }
        }
    }

    impl IpIntelligencePropertyReader for NoValueReader {
        fn property_names(&self) -> &[String] {
            &self.names
        }

        fn position(&self, name: &str) -> Option<usize> {
            self.names.iter().position(|known| known == name)
        }

        fn read_property(
            &self,
            position: usize,
            data: &mut IpIntelligenceDataBase,
        ) -> Result<(), String> {
            match self.names[position].as_str() {
                "asn" => data.set_string_no_value("Asn", "no asn"),
                "latitude" => data.set_float_no_value("Latitude", "no latitude"),
                "countrycodespopulation" => {
                    data.set_weighted_string_no_value("CountryCodesPopulation", "no codes")
                }
                "ip" => data.set_string("Ip", "192.0.2.1"),
                _ => data.set_string_no_value_unlisted("IpV6", "no IPv6"),
            }
            Ok(())
        }
    }

    #[test]
    fn a_property_read_with_no_value_gives_its_reason_and_is_listed_unless_unlisted() {
        let data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(NoValueReader::new());
        assert_eq!(data.asn().no_value_message(), Some("no asn"));
        assert_eq!(data.get("ASN").unwrap_err().message, "no asn");
        assert_eq!(data.latitude().no_value_message(), Some("no latitude"));
        assert_eq!(data.get("latitude").unwrap_err().message, "no latitude");
        assert_eq!(
            data.country_codes_population().no_value_message(),
            Some("no codes")
        );
        assert_eq!(
            data.get("CountryCodesPopulation").unwrap_err().message,
            "no codes"
        );
        assert_eq!(data.ip().value().unwrap().as_str(), "192.0.2.1");
        assert_eq!(data.ip_v6().no_value_message(), Some("no IPv6"));
        assert_eq!(data.get("IpV6").unwrap_err().message, "no IPv6");
        assert!(data.has_property("ipv6"));
        assert_eq!(
            data.keys(),
            ["asn", "latitude", "countrycodespopulation", "ip"]
        );
    }

    #[test]
    fn a_value_set_directly_wins_over_the_reader() {
        let (reader, reads) = CountingReader::new();
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(reader);
        data.set_string("Country", "France");
        assert_eq!(data.country().value().unwrap().as_str(), "France");
        assert_eq!(
            data.get("Country").unwrap(),
            PropertyValue::String("France".to_owned())
        );
        assert_eq!(reads_of(&reads, "country"), 0);
    }

    #[test]
    fn a_no_value_set_directly_wins_over_the_reader() {
        let (reader, reads) = CountingReader::new();
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(reader);
        data.set_string_no_value("Country", "not in this tier");
        data.set_weighted_string_no_value("CountryCodesGeographical", "no codes");
        assert_eq!(data.country().no_value_message(), Some("not in this tier"));
        assert_eq!(data.get("country").unwrap_err().message, "not in this tier");
        assert_eq!(
            data.get("COUNTRYCODESGEOGRAPHICAL").unwrap_err().message,
            "no codes"
        );
        assert_eq!(reads_of(&reads, "country"), 0);
        assert_eq!(reads_of(&reads, "countrycodesgeographical"), 0);
    }

    #[test]
    fn keys_list_a_property_set_with_no_value_where_it_was_set() {
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
        data.set_string_no_value("Town", "no location for this range");
        data.set_string("RegisteredName", "Example Networks");
        data.set_weighted_string_no_value("CountryCodesGeographical", "no codes");
        data.set_float_no_value("Latitude", "no location");
        data.set_integer_no_value("TimeZoneOffset", "no location");
        data.set_boolean_no_value("IsVPN", "not in this tier");
        data.set_string("Country", "United Kingdom");
        assert_eq!(
            data.keys(),
            [
                "town",
                "registeredname",
                "countrycodesgeographical",
                "latitude",
                "timezoneoffset",
                "isvpn",
                "country"
            ]
        );
        // The bag holds only the values.
        assert_eq!(data.base().keys(), ["registeredname", "country"]);
        assert_eq!(
            data.get("CountryCodesGeographical").unwrap_err().message,
            "no codes"
        );
    }

    #[test]
    fn a_property_set_with_no_value_is_listed_once_and_a_value_wins() {
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
        data.set_string_no_value("Town", "first");
        data.set_string("Country", "United Kingdom");
        data.set_string_no_value("TOWN", "second");
        data.set_string_no_value("Region", "no region");
        data.set_string("Region", "Berkshire");
        assert_eq!(data.keys(), ["town", "country", "region"]);
        assert_eq!(data.town().no_value_message(), Some("second"));
        assert_eq!(data.region().value().unwrap().as_str(), "Berkshire");
        assert_eq!(
            data.get("region").unwrap(),
            PropertyValue::String("Berkshire".to_owned())
        );
    }

    #[test]
    fn keys_list_a_no_value_set_directly_once_with_a_reader() {
        let (reader, _) = CountingReader::new();
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(reader);
        data.set_string_no_value("Country", "not in this tier");
        assert_eq!(
            data.keys(),
            ["country", "isvpn", "countrycodesgeographical"]
        );
        assert_eq!(data.get("Country").unwrap_err().message, "not in this tier");
    }

    #[test]
    fn a_property_without_a_value_reads_as_absent() {
        let (reader, _) = CountingReader::new();
        let data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(reader);
        assert!(data.region().value().is_err());
        assert!(data.get("Region").is_err());
        assert!(data.get("NotAProperty").is_err());
    }

    #[test]
    fn a_failed_read_is_a_no_value_carrying_its_message() {
        let (reader, _) = CountingReader::new();
        let data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(reader);
        assert_eq!(
            data.get("Areas").unwrap_err().message,
            "the native read failed"
        );
        assert_eq!(
            data.string("Areas").no_value_message(),
            Some("the native read failed")
        );
    }

    /// The details of an engine whose metadata lists `Country` and `Region`.
    fn missing_property_source() -> Arc<MissingPropertySource> {
        let properties = ["Country", "Region"]
            .iter()
            .map(|name| {
                AspectPropertyMetaData::new(*name, IP_DATA_KEY_NAME, PropertyValueType::String)
                    .with_data_tiers(["Lite"])
            })
            .collect();
        Arc::new(
            MissingPropertySource::new(
                IP_DATA_KEY_NAME,
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
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(reader);
        data.base_mut()
            .add_missing_property_source(Arc::clone(&source));

        // Town has an accessor, and the metadata does not list it.
        let town = source.reason("Town");
        assert_eq!(town.reason, MissingPropertyReason::DataFileUpgradeRequired);
        assert_eq!(
            data.town().no_value_message(),
            Some(town.description.as_str())
        );
        assert_eq!(data.get("Town").unwrap_err().message, town.description);

        let unknown = source.reason("NotAProperty");
        assert_eq!(unknown.reason, MissingPropertyReason::Unknown);
        assert_eq!(
            data.get("NotAProperty").unwrap_err().message,
            unknown.description
        );
        assert_eq!(
            data.string("NotAProperty").no_value_message(),
            Some(unknown.description.as_str())
        );
    }

    #[test]
    fn a_property_the_reader_reads_without_a_value_is_not_missing() {
        let (reader, _) = CountingReader::new();
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(reader);
        data.base_mut()
            .add_missing_property_source(missing_property_source());
        assert_eq!(
            data.region().no_value_message(),
            Some(DEFAULT_NO_VALUE_MESSAGE)
        );
        assert_eq!(
            data.get("Region").unwrap_err().message,
            "No value for property 'Region'."
        );
        assert_eq!(data.country().value().unwrap().as_str(), "United Kingdom");
    }

    #[test]
    fn without_a_source_a_missing_property_is_the_default_no_value() {
        let (reader, _) = CountingReader::new();
        let data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(reader);
        assert_eq!(
            data.town().no_value_message(),
            Some(DEFAULT_NO_VALUE_MESSAGE)
        );
        assert_eq!(
            data.get("Town").unwrap_err().message,
            "No value for property 'Town'."
        );
    }

    #[test]
    fn a_source_lets_the_data_say_what_it_holds() {
        let (reader, reads) = CountingReader::new();
        let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(reader);
        data.set_string("IpRangeStart", "192.0.2.0");
        data.set_float_no_value("AccuracyRadiusMin", "no radius for this range");
        assert!(data.has_property("Town"));

        data.base_mut()
            .add_missing_property_source(missing_property_source());
        // Set directly, set with no value, then read with a value, with none,
        // and failing.
        for name in [
            "IpRangeStart",
            "accuracyradiusmin",
            "Country",
            "Region",
            "Areas",
        ] {
            assert!(data.has_property(name), "{name}");
        }
        assert!(!data.has_property("Town"));
        assert!(!data.has_property("NotAProperty"));
        // Asking reads nothing.
        assert_eq!(reads_of(&reads, "country"), 0);
    }

    // WebAssembly has no threads to spawn.
    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn reads_from_several_threads_agree_and_read_once() {
        let (reader, reads) = CountingReader::new();
        let data = Arc::new(IpIntelligenceDataBase::new(IP_DATA_KEY_NAME).with_reader(reader));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let data = Arc::clone(&data);
                std::thread::spawn(move || data.country().value().unwrap().clone())
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().unwrap(), "United Kingdom");
        }
        assert_eq!(reads_of(&reads, "country"), 1);
    }
}
