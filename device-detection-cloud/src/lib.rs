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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-cloud-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-cloud-lib.rs&utm_term=logo)
//!
//! # 51Degrees cloud device detection
//!
//! The cloud variant of device detection. It implements the
//! [device-detection-cloud specification](https://github.com/51Degrees/specifications/blob/main/device-detection-specification/pipeline-elements/device-detection-cloud.md).
//!
//! ## What it does
//!
//! A [`fiftyone_cloud_request_engine::CloudRequestEngine`] makes a single HTTP
//! call to the 51Degrees cloud per flow data and stores the raw JSON response
//! under the `cloud` data key. [`DeviceDetectionCloudEngine`] then reads that
//! JSON, slices out the `device` member, and turns it into a
//! [`DeviceDataBase`], which it stores under [`DEVICE_DATA_KEY`].
//! It consumes no evidence of its own, so it must sit *after* a cloud request
//! engine in the pipeline.
//!
//! ## Hardware-profile lookup (TAC and native model)
//!
//! [`HardwareProfileCloudEngine`] is the second cloud engine in this crate. It
//! covers the lookups where one input parameter matches *several* device
//! profiles: a Type Allocation Code (`query.tac`) or a native model name
//! (`query.nativemodel`). It slices the response's `hardware.profiles` array and
//! turns each entry into a [`DeviceDataBase`], collecting them into a
//! [`MultiDeviceData`] stored under [`HARDWARE_DATA_KEY`]. Each profile is read
//! through the same [`DeviceData`] accessors as a single-device result. It
//! needs a resource key with the
//! hardware-profile-lookup product.
//!
//! ## Interface compatibility with the on-premise engine
//!
//! The engine produces the same concrete element-data type, under the same
//! [`TypedKey`](fiftyone_pipeline_core::TypedKey), as the on-premise Hash engine:
//! a [`DeviceDataBase`] under [`DEVICE_DATA_KEY`] (data
//! key string `"device"`). A consuming application reads the result through that
//! key, or through the shared [`DeviceData`] typed accessors,
//! and can swap a cloud engine for an on-premise one (or the reverse) without
//! touching its result-reading code. The shared model lives in the
//! `fiftyone-device-detection-shared` crate precisely so both engines depend on
//! one definition.
//!
//! ## No-value handling
//!
//! When the cloud determines no value for a property it sends JSON `null` with a
//! sibling `<property>nullreason` entry explaining why. The mapping (in the
//! private `dto` module) holds the property with no value and that entry's
//! text as its message, or `Unknown` when the response gives none, so a typed
//! accessor returns
//! [`AspectPropertyValue::NoValue`](fiftyone_pipeline_engines::AspectPropertyValue::NoValue)
//! carrying the cloud's explanation. The `nullreason` entries themselves are
//! not properties and are left out of the data. Read the message from the
//! property, for example `device.hardware_vendor().no_value_message()`, rather
//! than from a `hardwarevendornullreason` entry. This realises the
//! [null-values specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#null-values).
//!
//! ## A property the response did not return
//!
//! Data from a successful response, and each hardware profile, is given the
//! properties the resource key grants, so reading a property the response did
//! not return gives the reason the
//! [missing properties specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#missing-properties)
//! names. A read of the device data also logs a warning, as .NET's cloud data
//! does, unless the resource key lacks the device product. The engine logs a
//! missing product once, as a warning, when it reads the key's products, as
//! the
//! [cloud aspect engine specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/cloud-aspect-engine.md#start-up-activity)
//! says, so a read logs nothing more. A read of a hardware profile logs
//! nothing, because .NET builds each profile with no logger.
//! [`ElementData::has_property`](fiftyone_pipeline_core::ElementData::has_property)
//! answers `false` for such a property, so an element that asks first, such
//! as Set Headers, can tell it from one the response gave as null. With a
//! license key alone the granted properties are not listed, so every
//! property counts as held.
//!
//! ## Property metadata
//!
//! The set of device properties a resource key grants comes from the cloud
//! request engine's
//! [`public_properties`](fiftyone_cloud_request_engine::CloudRequestEngine::public_properties),
//! which it normally fetches when it is built, so the engine reads its property
//! metadata when it is constructed. After a transient failure at that point the
//! engine reads it on the first process call after the request engine's
//! discovery completes, rather than keeping empty metadata.
//!
//! ## Configuration
//!
//! [`configuration::add_builders`] adds both engines' builders to a
//! [`fiftyone_pipeline_core::configuration::BuilderRegistry`] under the names
//! .NET gives them, with .NET's settings, so a configuration file written for
//! .NET makes the same engines here.
//!
//! ## Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use fiftyone_pipeline_core::{Evidence, Pipeline};
//! use fiftyone_cloud_request_engine::CloudRequestEngine;
//! use fiftyone_device_detection_cloud::DeviceDetectionCloudEngine;
//! use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY};
//!
//! let request_engine = Arc::new(
//!     CloudRequestEngine::builder()
//!         .resource_key("my-resource-key")
//!         .build()
//!         .unwrap(),
//! );
//! let device_engine = DeviceDetectionCloudEngine::builder()
//!     .cloud_request_engine(request_engine.clone())
//!     .build();
//!
//! let pipeline = Pipeline::builder()
//!     .add_element(request_engine)
//!     .add_element(Arc::new(device_engine))
//!     .build()
//!     .unwrap();
//!
//! let mut data = pipeline.create_flow_data_with(
//!     Evidence::builder().add("header.user-agent", "Mozilla/5.0").build(),
//! );
//! data.process().unwrap();
//! if let Some(device) = data.get(DEVICE_DATA_KEY) {
//!     if let Ok(mobile) = device.is_mobile().value() {
//!         println!("IsMobile: {mobile}");
//!     }
//! }
//! ```

#![warn(missing_docs)]

pub mod configuration;
mod dto;
mod engine;
mod hardware_engine;
mod meta;

pub use engine::{DeviceDetectionCloudEngine, DeviceDetectionCloudEngineBuilder};
pub use hardware_engine::{HardwareProfileCloudEngine, HardwareProfileCloudEngineBuilder};

// Re-export the shared device data model so consumers of the cloud engine have
// the result types and key to hand without a separate dependency line. These are
// the same items the on-premise engine surfaces, which is what makes the engines
// swappable.
pub use fiftyone_device_detection_shared::{DeviceData, DeviceDataBase, DEVICE_DATA_KEY};

// The hardware-profile lookup's result type and key live in the shared model
// too, so the on-premise TAC and Native engines add to the same data.
pub use fiftyone_device_detection_shared::{
    MultiDeviceData, HARDWARE_DATA_KEY, HARDWARE_ELEMENT_DATA_KEY,
};
