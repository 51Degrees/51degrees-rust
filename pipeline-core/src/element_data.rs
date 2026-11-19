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

//! Element data: the per-element container of property values.
//!
//! Element data is the output a flow element writes into the flow data, holding
//! the values of the properties it populates. See the
//! [conceptual overview](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/conceptual-overview.md#element-data).
//!
//! Two access mechanisms are provided, exactly as the
//! [access-to-results specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/access-to-results.md)
//! describes:
//!
//! 1. A dynamic property bag, [`ElementData::get`], that returns a
//!    [`PropertyValue`] by string name. This is a specification MUST.
//! 2. Strongly-typed access through [`crate::TypedKey`] and the [`Any`]
//!    downcast hooks ([`ElementData::as_any`] / [`ElementData::as_any_mut`]).
//!
//! [`MapElementData`] is a ready-made backing that concrete element data can
//! embed to get the dynamic bag for free.
//!
//! Element data is `Send`, so a flow data can move between threads, and need
//! not be `Sync`, because it is not safe to use from several threads at once.

use std::any::Any;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use ahash::RandomState;

use crate::ci_map::ci_get;
use crate::error::NoValueError;
use crate::value::PropertyValue;

/// The behavior every element-data type must provide.
///
/// The trait is bound on [`Any`] so [`crate::FlowData`] can downcast a stored
/// `dyn ElementData` back to its concrete type given a [`crate::TypedKey`]. It
/// is bound on [`Send`] because a flow data may be moved between threads, for
/// example handed from a request-handling thread to a worker.
///
/// It is not bound on [`Sync`], because element data is not safe to use from
/// several threads at once, so neither is a flow data. Element data that
/// reads its values from an on-premise engine's results instance, such as the
/// Device Detection and IP Intelligence data, takes no lock and is not
/// `Sync`, as the
/// [thread-safety specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/thread-safety.md#results-instances)
/// requires, so the compiler refuses code that shares it across threads. A
/// caller that must share it wraps it in a lock of its own, such as a
/// `Mutex`. A member of a parallel group that reads element data added
/// before the group is responsible for the thread safety of those reads.
///
/// Element data is written only by the flow element that provides it, or the
/// flow elements that share it, and only while they process, as the
/// [thread-safety specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/thread-safety.md#element-data)
/// requires. [`crate::FlowData::get_or_add`] refuses a write once that
/// element has finished.
pub trait ElementData: Any + Send {
    /// Get a property value by name.
    ///
    /// Returns `Err(NoValueError)` when the property is present in this data but
    /// has no value (the
    /// [null-values rule](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#null-values)),
    /// or when the property name is unknown to this data. Property names are
    /// matched case-insensitively.
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError>;

    /// Whether this data holds the property, with or without a value.
    ///
    /// [`ElementData::get`] fails in both cases, so this tells a property the
    /// data does not hold apart from one it holds with no value. The default
    /// answers `true`, for data that cannot tell the two apart.
    fn has_property(&self, _name: &str) -> bool {
        true
    }

    /// List the property names available in this data, in the data's own
    /// order. The JSON builder writes the properties in this order.
    fn keys(&self) -> Vec<String>;

    /// Borrow this value as `&dyn Any` for downcasting to the concrete type.
    fn as_any(&self) -> &dyn Any;

    /// Borrow this value as `&mut dyn Any` for downcasting to the concrete type.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// A reusable, case-insensitive property bag that concrete element data can
/// embed.
///
/// An element-data struct holds a `MapElementData` and forwards [`ElementData`]
/// to it, or simply uses `MapElementData` directly for elements whose output is
/// a flat set of values.
///
/// Property names are stored lowercased so lookups are case-insensitive, as the
/// [access-to-results specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/access-to-results.md#general-guidance)
/// requires.
///
/// [`ElementData::keys`] lists the names in the order they were first set,
/// and setting a name again keeps its place.
#[derive(Debug, Clone, Default)]
pub struct MapElementData {
    values: HashMap<String, PropertyValue, RandomState>,
    /// The keys of `values` in the order they were first set.
    order: Vec<String>,
}

impl MapElementData {
    /// Create an empty bag.
    pub fn new() -> Self {
        MapElementData {
            values: HashMap::default(),
            order: Vec::new(),
        }
    }

    /// Set a property value, overwriting any existing value for that name. The
    /// name is lowercased. Returns `self` for chaining during construction.
    pub fn set(mut self, name: impl AsRef<str>, value: impl Into<PropertyValue>) -> Self {
        self.insert(name, value);
        self
    }

    /// Insert a property value by mutable reference (for use after the bag has
    /// been created), overwriting any existing value for that name.
    pub fn insert(&mut self, name: impl AsRef<str>, value: impl Into<PropertyValue>) {
        match self.values.entry(name.as_ref().to_lowercase()) {
            Entry::Occupied(mut entry) => {
                entry.insert(value.into());
            }
            Entry::Vacant(entry) => {
                self.order.push(entry.key().clone());
                entry.insert(value.into());
            }
        }
    }

    /// The number of property values held.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// True if no property values are held.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Look up a property value by name without the [`ElementData`] error
    /// semantics. Returns `None` if the name is not present.
    pub fn get_value(&self, name: &str) -> Option<&PropertyValue> {
        ci_get(&self.values, name)
    }
}

impl ElementData for MapElementData {
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
        match self.get_value(name) {
            Some(value) => Ok(value.clone()),
            None => Err(NoValueError::new(format!(
                "No value for property '{name}'."
            ))),
        }
    }

    fn has_property(&self, name: &str) -> bool {
        self.get_value(name).is_some()
    }

    fn keys(&self) -> Vec<String> {
        self.order.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// The values of element data, read once, so they can be listed and shared.
///
/// Element data that reads a results instance is not `Sync`, so it cannot be
/// held in an [`crate::ElementDataList`]. A snapshot reads, through
/// [`ElementData::get`], every property the data lists in
/// [`ElementData::keys`], and then answers from what it read, the value or
/// the reason for no value, in the data's order.
#[derive(Debug, Clone)]
pub struct ElementDataSnapshot {
    /// The names the data listed, in its order.
    names: Vec<String>,
    /// What reading each name gave, in the same order.
    values: Vec<Result<PropertyValue, NoValueError>>,
}

impl ElementDataSnapshot {
    /// Read every property `data` lists.
    pub fn new(data: &dyn ElementData) -> Self {
        let names = data.keys();
        let values = names.iter().map(|name| data.get(name)).collect();
        ElementDataSnapshot { names, values }
    }

    /// The position of a name, ignoring case.
    fn position(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|known| {
            known.eq_ignore_ascii_case(name)
                || (!(known.is_ascii() && name.is_ascii())
                    && known.to_lowercase() == name.to_lowercase())
        })
    }
}

impl ElementData for ElementDataSnapshot {
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
        match self.position(name) {
            Some(position) => self.values[position].clone(),
            None => Err(NoValueError::new(format!(
                "No value for property '{name}'."
            ))),
        }
    }

    fn has_property(&self, name: &str) -> bool {
        self.position(name).is_some()
    }

    fn keys(&self) -> Vec<String> {
        self.names.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Element data that counts its reads, gives one value and one reason,
    /// and is not `Sync`.
    struct Counted {
        reads: Cell<usize>,
    }

    impl ElementData for Counted {
        fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
            self.reads.set(self.reads.get() + 1);
            match name {
                "Vendor" => Ok(PropertyValue::String("Apple".to_owned())),
                _ => Err(NoValueError::new("No model for this profile.")),
            }
        }

        fn keys(&self) -> Vec<String> {
            vec!["Vendor".to_owned(), "Model".to_owned()]
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    fn assert_sync<T: Sync>() {}

    /// A snapshot reads each property once, keeps its value or reason in the
    /// data's order, matches names ignoring case, and is `Sync`.
    #[test]
    fn a_snapshot_reads_each_listed_property_once() {
        let data = Counted {
            reads: Cell::new(0),
        };
        let snapshot = ElementDataSnapshot::new(&data);
        assert_eq!(data.reads.get(), 2);
        assert_eq!(snapshot.keys(), ["Vendor", "Model"]);
        assert_eq!(
            snapshot.get("vendor").unwrap(),
            PropertyValue::String("Apple".to_owned())
        );
        assert_eq!(
            snapshot.get("MODEL").unwrap_err().message,
            "No model for this profile."
        );
        assert!(snapshot.has_property("model"));
        assert!(!snapshot.has_property("Colour"));
        assert!(snapshot.get("Colour").is_err());
        assert_eq!(data.reads.get(), 2);
        assert_sync::<ElementDataSnapshot>();
    }
}
