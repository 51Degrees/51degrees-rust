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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-property-keyed-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-property-keyed-lib.rs&utm_term=logo)
//!
//! # Hardware profiles by TAC and native model name
//!
//! The engines here give every hardware profile in a 51Degrees Device
//! Detection data file that holds the value a request gives of one property,
//! as .NET's `FiftyOne.DeviceDetection.PropertyKeyed` does.
//!
//! - [`TacEngineBuilder`] builds the engine for `TAC`, the Type Allocation
//!   Code, which is the first eight digits of a mobile device's IMEI.
//! - [`NativeEngineBuilder`] builds the engine for `NativeModel`, the model
//!   name a device's own operating system reports, such as `SM-G960F`.
//! - [`PropertyKeyedDeviceEngineBuilder`] builds one for any other property.
//!
//! One TAC or model name can belong to several devices, so the result is a
//! [`MultiDeviceData`](fiftyone_device_detection_shared::MultiDeviceData)
//! holding a list of profiles, each read with the same
//! [`DeviceData`](fiftyone_device_detection_shared::DeviceData) accessors as
//! a detection. The TAC and Native engines both write it under
//! [`HARDWARE_DATA_KEY`](fiftyone_device_detection_shared::HARDWARE_DATA_KEY),
//! as the cloud's hardware lookup does, so the same code reads either.
//!
//! ## Use
//!
//! Add the engines after an on-premise Device Detection engine whose data
//! file holds `TAC` and `NativeModel`, and ask that engine to index them so
//! each lookup is fast, with `indexed_property` in code or its
//! `IndexedProperties` setting in a configuration. `NativeModel` is in the
//! Enterprise and TAC data files, and `TAC` only in the TAC file. With the free Lite file, which
//! holds neither, the engines accept no evidence and write nothing. Then give
//! the evidence `query.tac` or `query.nativemodel`.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngineBuilder;
//! use fiftyone_device_detection_property_keyed::{NativeEngineBuilder, TacEngineBuilder};
//! use fiftyone_device_detection_shared::{DeviceData, HARDWARE_DATA_KEY};
//! use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
//!
//! let device = DeviceDetectionOnPremiseEngineBuilder::new("TAC-HashV41.hash")
//!     .indexed_property("TAC")
//!     .indexed_property("NativeModel")
//!     .build()?;
//! let tac = TacEngineBuilder::new().build()?;
//! let native = NativeEngineBuilder::new().build()?;
//! let pipeline = Pipeline::builder()
//!     .add_element(device as Arc<dyn FlowElement>)
//!     .add_element(tac as Arc<dyn FlowElement>)
//!     .add_element(native as Arc<dyn FlowElement>)
//!     .build()?;
//! let mut data = pipeline.create_flow_data_with(
//!     Evidence::builder().add("query.tac", "35925406").build(),
//! );
//! data.process()?;
//! if let Some(hardware) = data.get(HARDWARE_DATA_KEY) {
//!     for profile in hardware.profiles() {
//!         println!("{:?}", profile.hardware_vendor().value());
//!     }
//! }
//! for warning in data.warnings() {
//!     println!("{}", warning.message);
//! }
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```
//!
//! A request carrying the evidence always gets the data, holding no profiles
//! when its value matches none. A TAC that is not exactly eight digits, or a
//! native model name shorter than two characters, gives a warning in the
//! flow data and no profiles, never an error. A value is matched exactly,
//! including case, as the data file's index matches it. .NET's engines
//! ignore the case of a value.
//!
//! When both engines are in one pipeline and the request gives both values,
//! the profiles of the engine added first come first, and a profile both
//! engines find is listed twice, as in .NET.
//!
//! A pipeline made from a configuration file written for .NET finds the
//! engines under .NET's builder names once [`configuration::add_builders`]
//! has added them to the registry.

#![warn(missing_docs)]

pub mod configuration;
pub mod constants;
mod engine;
mod number;

pub use engine::{
    NativeEngineBuilder, PropertyKeyedDeviceEngine, PropertyKeyedDeviceEngineBuilder,
    TacEngineBuilder,
};
