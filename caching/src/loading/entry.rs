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

//! Values with the times that decide how long they may be used.

use std::time::SystemTime;

/// A value as a [`crate::Store`] holds it, with its times.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry<V> {
    /// The value.
    pub value: V,
    /// When the source produced the value. Every copy keeps this time.
    pub written: SystemTime,
    /// When this copy stops being used, or `None` for no fixed end.
    pub expires: Option<SystemTime>,
    /// When this copy's idle period last started, at its write or its last
    /// renewal.
    pub renewed: SystemTime,
}

/// A value as a [`crate::ValueLoader`] returns it.
///
/// A source usually returns just the value, through `From<V>`. A cache
/// acting as the loader of another cache returns the value with the time it
/// was written and the time its copy stops being usable, so the cache above
/// never keeps a copy longer than the one it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded<V> {
    /// The value.
    pub value: V,
    /// When the source produced the value, or `None` for now.
    pub written: Option<SystemTime>,
    /// When the value stops being usable, or `None` for no limit beyond the
    /// cache's own lifetimes.
    pub expires: Option<SystemTime>,
}

impl<V> Loaded<V> {
    /// A value written now, with no limit of its own.
    pub fn new(value: V) -> Self {
        Loaded {
            value,
            written: None,
            expires: None,
        }
    }

    /// Sets when the value stops being usable, for example from a source's
    /// own freshness rules.
    pub fn expires_at(mut self, expires: SystemTime) -> Self {
        self.expires = Some(expires);
        self
    }
}

impl<V> From<V> for Loaded<V> {
    fn from(value: V) -> Self {
        Loaded::new(value)
    }
}

/// The earlier of two optional times, where `None` means no end.
pub(crate) fn earliest(a: Option<SystemTime>, b: Option<SystemTime>) -> Option<SystemTime> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}
