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
//! Element data is `Send + Sync`, so the elements of a parallel group can read
//! what earlier elements added while they run at the same time.

use std::any::Any;
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
/// It is bound on [`Sync`] because the elements of a parallel group (see
/// [`crate::PipelineBuilder::add_parallel_elements`]) read the element data
/// added before the group from several threads at once. Plain data is `Sync`
/// with no extra code. A value worked out on first read needs a [`std::sync`]
/// type such as `Mutex` or `OnceLock`, not a `Cell` or `RefCell`.
///
/// Element data is still written from one thread only, by the element that
/// adds it. See the
/// [thread-safety specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/thread-safety.md#element-data).
pub trait ElementData: Any + Send + Sync {
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

    /// List the property names available in this data. The order is
    /// unspecified.
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
#[derive(Debug, Clone, Default)]
pub struct MapElementData {
    values: HashMap<String, PropertyValue, RandomState>,
}

impl MapElementData {
    /// Create an empty bag.
    pub fn new() -> Self {
        MapElementData {
            values: HashMap::default(),
        }
    }

    /// Set a property value, overwriting any existing value for that name. The
    /// name is lowercased. Returns `self` for chaining during construction.
    pub fn set(mut self, name: impl AsRef<str>, value: impl Into<PropertyValue>) -> Self {
        self.values
            .insert(name.as_ref().to_lowercase(), value.into());
        self
    }

    /// Insert a property value by mutable reference (for use after the bag has
    /// been created), overwriting any existing value for that name.
    pub fn insert(&mut self, name: impl AsRef<str>, value: impl Into<PropertyValue>) {
        self.values
            .insert(name.as_ref().to_lowercase(), value.into());
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
        self.values.keys().cloned().collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
