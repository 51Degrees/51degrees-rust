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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-ip-intelligence-cloud-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-ip-intelligence-cloud-lib.rs&utm_term=logo)
//!
//! Cloud IP-intelligence engine that maps the cloud JSON response to
//! [`IpIntelligenceDataBase`].
//!
//! This crate realises the
//! [ip-intelligence-cloud specification](https://github.com/51Degrees/specifications/blob/main/ip-intelligence-specification/pipeline-elements/ip-intelligence-cloud.md).
//!
//! # Where it sits
//!
//! A cloud IP-intelligence pipeline has a
//! [`fiftyone_cloud_request_engine::CloudRequestEngine`] followed by an
//! [`IpIntelligenceCloudEngine`]. The request engine makes the single HTTP call
//! and stores the raw JSON response under its `cloud` data key. This engine then
//! reads that JSON, slices out the part it owns (the `ip` member) and
//! deserializes it into the shared
//! [`IpIntelligenceDataBase`], stored under [`IP_DATA_KEY`]. Because both the
//! cloud and on-premise engines produce that same type under that same key, a
//! consuming application can swap one engine for the other without changing how
//! it reads results.
//!
//! # Weighted values
//!
//! IP Intelligence properties are probabilistic, so the cloud encodes each one
//! as a JSON array of `{ "rawweighting": <u16>, "value": <candidate> }` objects.
//! This engine parses every array into a `Vec<`[`WeightedValue`]`<T>>` and
//! inserts it through the shared `set_weighted_*` methods, which sort the list
//! high weighting first and write the dynamic-bag mirror. The candidate type is
//! chosen from the cloud property metadata `Type` field (for example
//! `WeightedString`, `WeightedInteger`, `WeightedDouble`) when it is available,
//! and otherwise inferred from the JSON value itself.
//!
//! # No values
//!
//! When the cloud determines nothing for a property it sends `null` (or omits
//! it) and supplies a reason. Two reason encodings are handled, matching the two
//! shapes the cloud service has used:
//!
//! - a sibling `<propertyname>nullreason` entry next to the property, and
//! - a single top-level `nullValueReasons` object keyed by property name.
//!
//! Either way the reason is recorded through the shared `set_*_no_value`
//! methods, so the typed accessor returns an [`AspectPropertyValue::NoValue`]
//! carrying the message.

#![warn(missing_docs)]

mod engine;
mod parse;

pub use engine::{IpIntelligenceCloudEngine, IpIntelligenceCloudEngineBuilder};

// Re-export the shared element-data type and key so a consumer of the cloud
// engine can read results without depending on the shared crate directly.
pub use fiftyone_ip_intelligence_shared::{
    IpIntelligenceData, IpIntelligenceDataBase, IP_DATA_KEY, IP_DATA_KEY_NAME,
};

// Re-export the weighted-value and aspect-value types used in this engine's
// public surface, for the same reason.
pub use fiftyone_pipeline_core::WeightedValue;
pub use fiftyone_pipeline_engines::AspectPropertyValue;
