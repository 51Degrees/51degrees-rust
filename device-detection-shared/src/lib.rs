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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-shared-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-shared-lib.rs&utm_term=logo)
//!
//! # 51Degrees device-detection shared model
//!
//! The pieces of device detection that the on-premise and cloud engines both
//! build on, implementing the
//! [device-detection specification](https://github.com/51Degrees/specifications/tree/main/device-detection-specification).
//! Keeping these here, rather than in either engine crate, is what makes the two
//! engines interface-compatible: both populate the same [`DeviceDataBase`] under
//! the same [`DEVICE_DATA_KEY`], so a consuming application can swap an
//! on-premise engine for a cloud one (or the reverse) without touching its
//! result-reading code.
//!
//! ## What the crate provides
//!
//! - [`DeviceData`], a trait extending
//!   [`fiftyone_pipeline_engines::AspectData`] with one strongly-typed
//!   [`AspectPropertyValue`](fiftyone_pipeline_engines::AspectPropertyValue)
//!   accessor per documented Device Detection property (hardware, operating
//!   system, browser, crawler and the on-premise match metrics). The trait and
//!   its implementation are generated from the common metadata by the
//!   51Degrees PropertyGenerator tool into an internal `device_data` module.
//! - [`DeviceDataBase`], a concrete backing that embeds an
//!   [`AspectDataBase`](fiftyone_pipeline_engines::AspectDataBase) and reads
//!   each typed property out of the dynamic property bag, wrapping it in an
//!   `AspectPropertyValue`. Both engines write into one of these.
//! - [`DEVICE_DATA_KEY`], the [`TypedKey`](fiftyone_pipeline_core::TypedKey)
//!   used to store and retrieve the device data, with the data key string
//!   `"device"`.
//! - [`GENERATED_PROPERTY_TYPES`] (and the [`declared_property_value_type`] /
//!   [`default_property_metadata`] helpers derived from it) which a wrapper can
//!   use to type and publish the generated property set.
//! - [`UACH_EVIDENCE_QUERY_KEY`] and [`UACH_EVIDENCE_COOKIE_KEY`], the keys
//!   the high entropy values are supplied under.
//! - [`MultiDeviceData`], the list of device profiles a hardware-profile
//!   lookup by TAC or native model gives, stored under [`HARDWARE_DATA_KEY`]
//!   (data key string `"hardware"`) by the cloud and on-premise lookups alike.
//! - [`UachJsConversionElement`], deprecated, the User-Agent Client Hints
//!   high-entropy decoder element described in the
//!   [UA-CH high-entropy decoder specification](https://github.com/51Degrees/specifications/blob/main/device-detection-specification/pipeline-elements/uach-high-entropy-decoder.md).
//!   No engine reads its data, as the on-premise Hash engine passes the high
//!   entropy values to the C library, which converts them itself.
//!
//! ## The dynamic property bag is always available
//!
//! Beyond the generated typed accessors, every property a 51Degrees data file or
//! resource key returns is reachable through the dynamic bag inherited from
//! [`ElementData::get`](fiftyone_pipeline_core::ElementData::get) by its string
//! name (for example `device.get("ScreenMMWidth")`). The typed accessors are a
//! convenience over that bag, not a replacement for it, and they read from
//! exactly the same store.

#![warn(missing_docs)]

mod data;
mod device_data;
mod multi_device;
mod uach;

pub use data::{
    declared_property_value_type, default_aspect_property_metadata, default_property_metadata,
    generated_property_names, DeviceDataBase, DevicePropertyReader, DEVICE_DATA_KEY,
    DEVICE_ELEMENT_DATA_KEY,
};
pub use device_data::{DeviceData, GENERATED_PROPERTY_TYPES, TYPED_PROPERTY_NAMES};
pub use multi_device::{
    MultiDeviceData, ProfileReader, HARDWARE_DATA_KEY, HARDWARE_ELEMENT_DATA_KEY,
    PROFILES_PROPERTY_NAME,
};
#[allow(deprecated)]
pub use uach::UachJsConversionElement;
pub use uach::{
    UACH_EVIDENCE_COOKIE_KEY, UACH_EVIDENCE_QUERY_KEY, UACH_HIGH_ENTROPY_EVIDENCE_SUFFIX,
};
