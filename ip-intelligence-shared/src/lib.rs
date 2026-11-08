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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-ip-intelligence-shared-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-ip-intelligence-shared-lib.rs&utm_term=logo)
//!
//! # 51Degrees IP-intelligence shared model
//!
//! The IP-intelligence element data that the on-premise and cloud engines both
//! build on, following the
//! [ip-intelligence specification](https://github.com/51Degrees/specifications/tree/main/ip-intelligence-specification).
//! Keeping these here, rather than in either engine crate, is what makes the two
//! engines interface-compatible: both populate the same [`IpIntelligenceDataBase`]
//! under the same [`IP_DATA_KEY`], so a consuming application can swap an
//! on-premise engine for a cloud one (or the reverse) without touching its
//! result-reading code. This mirrors the device-detection shared model in
//! `fiftyone-device-detection-shared`.
//!
//! ## What the crate provides
//!
//! - [`IpIntelligenceData`], the read trait extending
//!   [`fiftyone_pipeline_engines::AspectData`] with one strongly-typed accessor
//!   per documented network and location property. Most properties are *plain*
//!   (a single typed value, for example a [`String`] country name, a [`bool`]
//!   `IsVPN` flag, an [`f32`] latitude or an [`i32`] offset); the few weighted
//!   properties (the country-code distributions, `Mcc` and
//!   `StateGeoNameIdsGeographical`) return an ordered
//!   `Vec<`[`WeightedValue`](fiftyone_pipeline_core::WeightedValue)`<String>>`.
//!   Every accessor wraps its value in an
//!   [`AspectPropertyValue`](fiftyone_pipeline_engines::AspectPropertyValue) so
//!   an absent value carries the engine's no-value reason.
//! - [`IpIntelligenceDataBase`], the concrete backing both engines populate. It
//!   embeds an [`AspectDataBase`](fiftyone_pipeline_engines::AspectDataBase) and
//!   keeps the typed values in dedicated stores beside it.
//! - [`IP_DATA_KEY`], the [`TypedKey`](fiftyone_pipeline_core::TypedKey) used to
//!   store and retrieve the data, with the data key string `"ip"`.
//! - [`GENERATED_PROPERTY_TYPES`] (and the [`generated_property_names`] and
//!   [`default_property_metadata`] / [`default_aspect_property_metadata`]
//!   helpers derived from it) a minimal wrapper can publish.
//!
//! The [`IpIntelligenceData`] trait and its accessor set are generated from the
//! common metadata by the 51Degrees PropertyGenerator tool into an internal
//! `ip_intelligence_data` module; the hand-written `data` module supplies the
//! backing store the accessors delegate to. Both are re-exported here, so the
//! crate root is the single import surface.

#![warn(missing_docs)]

mod data;
mod ip_intelligence_data;

pub use data::{
    declared_property_value_type, default_aspect_property_metadata, default_property_metadata,
    generated_property_names, IpIntelligenceDataBase, IpIntelligencePropertyReader, ValueStore,
    WeightedStore, IP_DATA_KEY, IP_DATA_KEY_NAME, WEIGHTED_PROPERTY_VALUE_TYPE,
    WEIGHTED_RECORD_VALUE_KEY, WEIGHTED_RECORD_WEIGHT_KEY,
};
pub use ip_intelligence_data::{IpIntelligenceData, GENERATED_PROPERTY_TYPES};
