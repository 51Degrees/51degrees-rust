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

//! The strongly-typed 51Did element-data model.
//!
//! [`FodIdData`] is the typed view of a 51Did result. [`FodIdDataBase`] is
//! the concrete type an engine producing 51Dids populates, stored under
//! [`FODID_DATA_KEY`].
//!
//! # Two layers: the raw envelope and the parsed identifier
//!
//! Each identifier is a base64-encoded
//! [OWID](https://github.com/SWAN-community/owid) envelope, the signed data
//! model that carries the value two recipients compare to decide whether they
//! observed the same browser instance under the same usage purpose. This model
//! exposes both layers:
//!
//! - the raw base64 string (for example through [`FodIdData::id_prob_global`]),
//!   suitable for storing in a cookie or forwarding to another party unchanged,
//!   and
//! - the parsed [`FodId`] (for example through
//!   [`FodIdData::id_prob_global_fod_id`]), which unpacks the envelope's payload
//!   (the usage, the identifier type, the license id and the match key) and
//!   gives access to the OWID domain, date and signature for verification.
//!
//! # Three identifier kinds, global and license scoped
//!
//! A result holds up to six identifiers (see [`IDENTIFIER_PROPERTIES`]), each
//! with a raw and a parsed accessor:
//!
//! - **probabilistic** ([`IdProbGlobal`](ID_PROB_GLOBAL_PROPERTY) /
//!   [`IdProbLic`](ID_PROB_LIC_PROPERTY)), derived from the device and network,
//! - **random** ([`IdRandGlobal`](ID_RAND_GLOBAL_PROPERTY) /
//!   [`IdRandLic`](ID_RAND_LIC_PROPERTY)), a server-generated GUID, and
//! - **hashed-email** ([`IdHemGlobal`](ID_HEM_GLOBAL_PROPERTY) /
//!   [`IdHemLic`](ID_HEM_LIC_PROPERTY)), derived from a supplied email and salt.
//!
//! Each *global* variant is unique across all callers; each *license-scoped*
//! variant is unique only within the caller's own license key.
//!
//! Parsing is done lazily by the accessor (not eagerly when the data is built),
//! so a caller that only needs the raw string never pays for parsing, and a
//! malformed envelope surfaces as an [`AspectPropertyValue::NoValue`] carrying
//! the parse error rather than failing the whole result.
//!
//! # The dynamic bag still works
//!
//! As with every element-data type, the raw values are also reachable by name
//! through the inherited [`ElementData::get`] (for example
//! `fodid.get("IdProbGlobal")`). The typed accessors read from that same bag, so
//! the two views never disagree.

use std::any::Any;

use fiftyone_pipeline_core::{ElementData, NoValueError, PropertyValue, TypedKey};
use fiftyone_pipeline_engines::{AspectData, AspectDataBase, AspectPropertyValue};
use fodid::FodId;

/// The string data key the 51Degrees identifier engine stores its element data
/// under.
///
/// The element data key is `"fodid"`, matching the top-level member the cloud
/// service returns the identifier values under.
pub const FODID_ELEMENT_DATA_KEY: &str = "fodid";

/// The typed handle used to store and retrieve [`FodIdDataBase`] in a flow data.
///
/// Pass this to [`FlowData::get`](fiftyone_pipeline_core::FlowData::get) to read
/// a result already downcast to `&FodIdDataBase`, or to
/// [`FlowData::get_or_add`](fiftyone_pipeline_core::FlowData::get_or_add) from an
/// engine. The key string is [`FODID_ELEMENT_DATA_KEY`].
pub const FODID_DATA_KEY: TypedKey<FodIdDataBase> = TypedKey::new(FODID_ELEMENT_DATA_KEY);

/// The property name for the globally-scoped probabilistic identifier, unique
/// across all callers from the same device and network.
pub const ID_PROB_GLOBAL_PROPERTY: &str = "IdProbGlobal";

/// The property name for the license-scoped probabilistic identifier, unique
/// only across the caller's own license key.
pub const ID_PROB_LIC_PROPERTY: &str = "IdProbLic";

/// The property name for the globally-scoped random identifier, a
/// server-generated GUID unique across all callers.
pub const ID_RAND_GLOBAL_PROPERTY: &str = "IdRandGlobal";

/// The property name for the license-scoped random identifier, a
/// server-generated GUID scoped to the caller's own license key.
pub const ID_RAND_LIC_PROPERTY: &str = "IdRandLic";

/// The property name for the globally-scoped hashed-email identifier, derived
/// from the supplied email and salt and unique across all callers.
pub const ID_HEM_GLOBAL_PROPERTY: &str = "IdHemGlobal";

/// The property name for the license-scoped hashed-email identifier, derived
/// from the supplied email and salt and scoped to the caller's license key.
pub const ID_HEM_LIC_PROPERTY: &str = "IdHemLic";

/// The six identifier property names this engine surfaces, in declaration
/// order: the probabilistic, random and hashed-email identifiers, each in a
/// global and a license-scoped variant.
pub const IDENTIFIER_PROPERTIES: [&str; 6] = [
    ID_PROB_GLOBAL_PROPERTY,
    ID_PROB_LIC_PROPERTY,
    ID_RAND_GLOBAL_PROPERTY,
    ID_RAND_LIC_PROPERTY,
    ID_HEM_GLOBAL_PROPERTY,
    ID_HEM_LIC_PROPERTY,
];

/// The standard explanation used when a typed accessor finds the property absent
/// from the underlying bag.
const ABSENT_MESSAGE: &str = "The property was not present in the 51Degrees identifier data.";

/// The explanation used when a property is present but stored as a value of an
/// unexpected type for the requested accessor.
const WRONG_TYPE_MESSAGE: &str =
    "The property was present but its stored value was not a 51Did string.";

/// Strongly-typed accessors for a 51Degrees-identifier (51Did / FODid) result.
///
/// This trait extends [`AspectData`] (and so [`ElementData`]) with named
/// accessors for the six identifier properties ([`IDENTIFIER_PROPERTIES`]), each
/// available both as the raw base64 envelope and as a parsed [`FodId`]. Each
/// accessor returns an
/// [`AspectPropertyValue<T>`] so the caller can tell "the cloud issued an
/// identifier" from "no identifier was available" (and, for the parsed form,
/// from "the value could not be read as a 51Did") without losing the
/// explanation.
pub trait FodIdData: AspectData {
    /// The globally-scoped 51Did as the raw base64 OWID envelope string, as the
    /// cloud sent it. Property name [`ID_PROB_GLOBAL_PROPERTY`].
    ///
    /// This value is unique across all callers observing the same device and
    /// network. Store or forward it unchanged; compare two identifiers by their
    /// parsed [`FodId::match_key`], not by the envelope string, which is reissued
    /// (with a fresh date and signature) on every call.
    fn id_prob_global(&self) -> AspectPropertyValue<String>;

    /// The license-scoped 51Did as the raw base64 OWID envelope string. Property
    /// name [`ID_PROB_LIC_PROPERTY`].
    ///
    /// This value is unique only across the caller's own license key, so it
    /// cannot be correlated with identifiers issued to other license holders.
    fn id_prob_lic(&self) -> AspectPropertyValue<String>;

    /// The globally-scoped 51Did parsed into a [`FodId`], unpacking the OWID
    /// envelope and its payload (the usage, the identifier type, the license
    /// id and the match key).
    ///
    /// Returns a no-value if the identifier was absent, or if the value could
    /// not be read as a 51Did (the no-value message then carries the reason
    /// the `fodid` reader gave). Reading does not verify the signature.
    fn id_prob_global_fod_id(&self) -> AspectPropertyValue<FodId>;

    /// The license-scoped 51Did parsed into a [`FodId`]. Behaves like
    /// [`FodIdData::id_prob_global_fod_id`] for the license-scoped value.
    fn id_prob_lic_fod_id(&self) -> AspectPropertyValue<FodId>;

    /// The globally-scoped random 51Did as the raw base64 OWID envelope string.
    /// Property name [`ID_RAND_GLOBAL_PROPERTY`].
    ///
    /// A random identifier carries a server-generated GUID rather than a value
    /// derived from the device, so it is unique across all callers.
    fn id_rand_global(&self) -> AspectPropertyValue<String>;

    /// The license-scoped random 51Did as the raw base64 OWID envelope string.
    /// Property name [`ID_RAND_LIC_PROPERTY`].
    fn id_rand_lic(&self) -> AspectPropertyValue<String>;

    /// The globally-scoped random 51Did parsed into a [`FodId`]. The parsed
    /// value carries a 16-byte GUID match key rather than a 32-byte SHA-256.
    fn id_rand_global_fod_id(&self) -> AspectPropertyValue<FodId>;

    /// The license-scoped random 51Did parsed into a [`FodId`].
    fn id_rand_lic_fod_id(&self) -> AspectPropertyValue<FodId>;

    /// The globally-scoped hashed-email 51Did as the raw base64 OWID envelope
    /// string. Property name [`ID_HEM_GLOBAL_PROPERTY`].
    ///
    /// A hashed-email identifier is derived from the caller-supplied email and a
    /// salt, so the same email yields the same identifier across callers.
    fn id_hem_global(&self) -> AspectPropertyValue<String>;

    /// The license-scoped hashed-email 51Did as the raw base64 OWID envelope
    /// string. Property name [`ID_HEM_LIC_PROPERTY`].
    fn id_hem_lic(&self) -> AspectPropertyValue<String>;

    /// The globally-scoped hashed-email 51Did parsed into a [`FodId`].
    fn id_hem_global_fod_id(&self) -> AspectPropertyValue<FodId>;

    /// The license-scoped hashed-email 51Did parsed into a [`FodId`].
    fn id_hem_lic_fod_id(&self) -> AspectPropertyValue<FodId>;
}

/// The concrete 51Did element data an engine producing 51Dids populates.
///
/// It wraps an [`AspectDataBase`] property bag holding the raw base64 strings
/// keyed by their property names, and implements [`FodIdData`] on top, parsing
/// each envelope on demand.
///
/// # Example
///
/// ```
/// use fiftyone_fodid_core::{FodIdData, FodIdDataBase};
///
/// // Built by an engine, constructed directly here.
/// let data = FodIdDataBase::new().set("IdProbGlobal", "not-a-valid-envelope");
///
/// // The raw value is always available.
/// assert_eq!(data.id_prob_global().value().unwrap(), "not-a-valid-envelope");
///
/// // Parsing an invalid envelope is a no-value, not a panic; the license-scoped
/// // value was never set, so it is absent.
/// assert!(!data.id_prob_global_fod_id().has_value());
/// assert!(!data.id_prob_lic().has_value());
/// ```
#[derive(Debug, Clone)]
pub struct FodIdDataBase {
    base: AspectDataBase,
}

impl FodIdDataBase {
    /// Create an empty identifier data attributed to the engine (data key
    /// [`FODID_ELEMENT_DATA_KEY`]).
    pub fn new() -> Self {
        FodIdDataBase {
            base: AspectDataBase::new(FODID_ELEMENT_DATA_KEY),
        }
    }

    /// Create an identifier data wrapping an existing [`AspectDataBase`],
    /// preserving its engine keys and cache-hit flag.
    pub fn from_base(base: AspectDataBase) -> Self {
        FodIdDataBase { base }
    }

    /// Set a property value, overwriting any existing value for that name, and
    /// return `self` for chaining. The name is matched case-insensitively.
    pub fn set(mut self, name: impl AsRef<str>, value: impl Into<PropertyValue>) -> Self {
        self.base = self.base.set(name, value);
        self
    }

    /// Insert a property value by mutable reference, overwriting any existing
    /// value for that name.
    pub fn insert(&mut self, name: impl AsRef<str>, value: impl Into<PropertyValue>) {
        self.base.insert(name, value);
    }

    /// Mark this data as having been served from a cache hit.
    pub fn set_cache_hit(&mut self) {
        self.base.set_cache_hit();
    }

    /// Borrow the wrapped [`AspectDataBase`].
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
    fn string_property(&self, name: &str) -> AspectPropertyValue<String> {
        match self.base.get(name) {
            Ok(value) => match value.as_str() {
                Some(s) => AspectPropertyValue::new(s.to_owned()),
                None => AspectPropertyValue::no_value(WRONG_TYPE_MESSAGE),
            },
            Err(_) => AspectPropertyValue::no_value(ABSENT_MESSAGE),
        }
    }

    /// Read a base64 identifier property and parse it into a [`FodId`].
    ///
    /// An absent property is a no-value with the standard absent message. A
    /// present value that does not read as a 51Did is a no-value whose
    /// message carries the reason the `fodid` reader gave (the OWID status,
    /// or one of the 51Did payload statuses), so a malformed identifier
    /// never panics or fails the whole result. Reading does not verify the
    /// signature, so a parsed value is not necessarily cryptographically
    /// valid.
    fn parsed_property(&self, name: &str) -> AspectPropertyValue<FodId> {
        match self.string_property(name).into_value() {
            Ok(base64) => match FodId::from_base64(&base64) {
                Ok(fod_id) => AspectPropertyValue::new(fod_id),
                Err(error) => AspectPropertyValue::no_value(format!(
                    "The 51Did value could not be read: {error}"
                )),
            },
            // The string accessor already supplied the absent/wrong-type
            // explanation; carry it through unchanged.
            Err(no_value) => AspectPropertyValue::no_value(no_value.message),
        }
    }
}

impl Default for FodIdDataBase {
    fn default() -> Self {
        FodIdDataBase::new()
    }
}

impl ElementData for FodIdDataBase {
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
        self.base.get(name)
    }

    fn keys(&self) -> Vec<String> {
        self.base.keys()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl AspectData for FodIdDataBase {
    fn engine_keys(&self) -> &[String] {
        self.base.engine_keys()
    }

    fn cache_hit(&self) -> bool {
        self.base.cache_hit()
    }
}

impl FodIdData for FodIdDataBase {
    fn id_prob_global(&self) -> AspectPropertyValue<String> {
        self.string_property(ID_PROB_GLOBAL_PROPERTY)
    }

    fn id_prob_lic(&self) -> AspectPropertyValue<String> {
        self.string_property(ID_PROB_LIC_PROPERTY)
    }

    fn id_prob_global_fod_id(&self) -> AspectPropertyValue<FodId> {
        self.parsed_property(ID_PROB_GLOBAL_PROPERTY)
    }

    fn id_prob_lic_fod_id(&self) -> AspectPropertyValue<FodId> {
        self.parsed_property(ID_PROB_LIC_PROPERTY)
    }

    fn id_rand_global(&self) -> AspectPropertyValue<String> {
        self.string_property(ID_RAND_GLOBAL_PROPERTY)
    }

    fn id_rand_lic(&self) -> AspectPropertyValue<String> {
        self.string_property(ID_RAND_LIC_PROPERTY)
    }

    fn id_rand_global_fod_id(&self) -> AspectPropertyValue<FodId> {
        self.parsed_property(ID_RAND_GLOBAL_PROPERTY)
    }

    fn id_rand_lic_fod_id(&self) -> AspectPropertyValue<FodId> {
        self.parsed_property(ID_RAND_LIC_PROPERTY)
    }

    fn id_hem_global(&self) -> AspectPropertyValue<String> {
        self.string_property(ID_HEM_GLOBAL_PROPERTY)
    }

    fn id_hem_lic(&self) -> AspectPropertyValue<String> {
        self.string_property(ID_HEM_LIC_PROPERTY)
    }

    fn id_hem_global_fod_id(&self) -> AspectPropertyValue<FodId> {
        self.parsed_property(ID_HEM_GLOBAL_PROPERTY)
    }

    fn id_hem_lic_fod_id(&self) -> AspectPropertyValue<FodId> {
        self.parsed_property(ID_HEM_LIC_PROPERTY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_key_string_is_fodid() {
        assert_eq!(FODID_ELEMENT_DATA_KEY, "fodid");
        assert_eq!(FODID_DATA_KEY.name(), "fodid");
    }

    #[test]
    fn raw_values_are_returned_typed() {
        let data = FodIdDataBase::new()
            .set("IdProbGlobal", "ZW52ZWxvcGU")
            .set("IdProbLic", "bGljZW5jZQ");
        assert_eq!(data.id_prob_global().value().unwrap(), "ZW52ZWxvcGU");
        assert_eq!(data.id_prob_lic().value().unwrap(), "bGljZW5jZQ");
    }

    #[test]
    fn absent_value_is_a_no_value() {
        let data = FodIdDataBase::new();
        assert!(!data.id_prob_global().has_value());
        assert!(!data.id_prob_global_fod_id().has_value());
    }

    #[test]
    fn case_insensitive_lookup_matches_cloud_casing() {
        // The cloud sends the names lower case; the canonical accessor still
        // resolves them through the case-folding bag.
        let data = FodIdDataBase::new().set("idprobglobal", "ZW52ZWxvcGU");
        assert_eq!(data.id_prob_global().value().unwrap(), "ZW52ZWxvcGU");
    }

    #[test]
    fn invalid_envelope_parses_to_a_no_value_with_a_reason() {
        let data = FodIdDataBase::new().set("IdProbGlobal", "this-is-not-base64-owid!!");
        let parsed = data.id_prob_global_fod_id();
        assert!(!parsed.has_value());
        assert!(parsed
            .no_value_message()
            .is_some_and(|m| m.contains("could not be read") && m.contains("InvalidBase64")));
    }

    #[test]
    fn short_payload_parses_to_a_no_value_naming_the_51did_status() {
        // A well formed OWID whose payload is shorter than the 51Did header
        // is not a 51Did. The no-value message names the 51Did status so the
        // reason is not lost behind a generic decode failure.
        let creator = fodid::Creator::new("51degrees.com", fodid::Crypto::new()).unwrap();
        let envelope = creator.create(vec![0u8; 3]).unwrap().as_base64().unwrap();
        let data = FodIdDataBase::new().set("IdProbGlobal", envelope.clone());
        assert_eq!(data.id_prob_global().value().unwrap(), &envelope);
        let parsed = data.id_prob_global_fod_id();
        assert!(!parsed.has_value());
        assert!(parsed
            .no_value_message()
            .is_some_and(|m| m.contains("PayloadTooShort")));
    }

    #[test]
    fn all_six_identifiers_are_exposed() {
        let data = FodIdDataBase::new()
            .set("IdProbGlobal", "pg")
            .set("IdProbLic", "pl")
            .set("IdRandGlobal", "rg")
            .set("IdRandLic", "rl")
            .set("IdHemGlobal", "hg")
            .set("IdHemLic", "hl");
        assert_eq!(data.id_prob_global().value().unwrap(), "pg");
        assert_eq!(data.id_prob_lic().value().unwrap(), "pl");
        assert_eq!(data.id_rand_global().value().unwrap(), "rg");
        assert_eq!(data.id_rand_lic().value().unwrap(), "rl");
        assert_eq!(data.id_hem_global().value().unwrap(), "hg");
        assert_eq!(data.id_hem_lic().value().unwrap(), "hl");

        // An unset identifier is a no-value, for both the raw and parsed forms.
        let empty = FodIdDataBase::new();
        assert!(!empty.id_rand_global().has_value());
        assert!(!empty.id_hem_lic_fod_id().has_value());
    }
}
