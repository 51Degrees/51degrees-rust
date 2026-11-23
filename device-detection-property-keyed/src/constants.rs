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

//! The names the engines use, as .NET's
//! `FiftyOne.DeviceDetection.PropertyKeyed` gives them.

/// The property the TAC engine looks profiles up by.
pub const TAC_PROPERTY_NAME: &str = "TAC";

/// The property the Native engine looks profiles up by.
pub const NATIVE_MODEL_PROPERTY_NAME: &str = "NativeModel";

/// What a property keyed engine's evidence key starts with. The rest is the
/// key property's name in lower case, so the TAC engine accepts `query.tac`.
pub const EVIDENCE_KEY_PREFIX: &str = "query.";

/// What the data key of a
/// [`PropertyKeyedDeviceEngineBuilder`](crate::PropertyKeyedDeviceEngineBuilder)'s
/// engine starts with when none is set. The rest is the key property's name
/// in lower case.
pub const DEFAULT_DATA_KEY_PREFIX: &str = "profiles-";
