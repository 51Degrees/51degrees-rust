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

//! Typed handles for strongly-typed element-data access.
//!
//! [`TypedKey<T>`] is the no-reflection replacement for the C# pattern of
//! looking up element data by `Type` and testing it with `IsAssignableFrom`.
//! The Rust type identity travels at compile time inside the `TypedKey<T>`,
//! and [`crate::FlowData::get`] uses an `Any` downcast to recover the concrete
//! `T`. This realises mechanism 2 of the
//! [access-to-results specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/access-to-results.md#accessing-results)
//! ("get element data by supplying the type") without any runtime reflection.

use std::marker::PhantomData;

use crate::element_data::ElementData;

/// A compile-time-typed handle to a piece of element data.
///
/// A `TypedKey<T>` pairs the string data key used to store the element data in
/// [`crate::FlowData`] with the concrete element-data type `T`. Each concrete
/// flow element exposes one for its own data through its own inherent API
/// (typically an associated `const`), and callers pass it to
/// [`crate::FlowData::get`] to retrieve the data already downcast to `&T` with
/// no casting on their side.
///
/// The `name` is a `&'static str` because data keys are fixed at compile time,
/// which keeps the key cheap to copy and avoids any allocation.
#[derive(Debug)]
pub struct TypedKey<T: ElementData> {
    name: &'static str,
    _marker: PhantomData<fn() -> T>,
}

impl<T: ElementData> TypedKey<T> {
    /// Create a new typed key for the given data key string.
    ///
    /// This is `const` so a flow element can declare its key as an associated
    /// constant.
    pub const fn new(name: &'static str) -> Self {
        TypedKey {
            name,
            _marker: PhantomData,
        }
    }

    /// The string data key that this typed key refers to.
    pub const fn name(&self) -> &'static str {
        self.name
    }
}

// Derived `Clone`/`Copy` would require `T: Clone`/`T: Copy` because of the
// `PhantomData<fn() -> T>` field, even though no `T` value is ever stored.
// Implement them by hand so a `TypedKey` is always copyable regardless of `T`.
impl<T: ElementData> Clone for TypedKey<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ElementData> Copy for TypedKey<T> {}
