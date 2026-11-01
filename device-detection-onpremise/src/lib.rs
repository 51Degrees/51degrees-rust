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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-onpremise-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-device-detection-onpremise-lib.rs&utm_term=logo)
//!
//! # 51Degrees on-premise Hash device-detection engine
//!
//! The on-premise device-detection engine, built over the safe
//! [`fiftyone_native::dd`] wrapper around the native Hash engine. It implements the
//! [device-detection specification](https://github.com/51Degrees/specifications/tree/main/device-detection-specification).
//!
//! ## What the crate provides
//!
//! - [`DeviceDetectionOnPremiseEngine`], a
//!   [`FlowElement`](fiftyone_pipeline_core::FlowElement) and
//!   [`AspectEngine`](fiftyone_pipeline_engines::AspectEngine) /
//!   [`OnPremiseAspectEngine`](fiftyone_pipeline_engines::OnPremiseAspectEngine)
//!   that loads a Hash data file, runs detection on each request, and writes a
//!   [`DeviceDataBase`](fiftyone_device_detection_shared::DeviceDataBase) into
//!   the flow data under
//!   [`DEVICE_DATA_KEY`](fiftyone_device_detection_shared::DEVICE_DATA_KEY).
//! - [`DeviceDetectionOnPremiseEngineBuilder`], a fluent builder that takes the
//!   data file path, a [`PerformanceProfile`](fiftyone_native::PerformanceProfile),
//!   an optional restricted property set and optional automatic-update settings.
//!
//! ## Interface compatibility with the cloud engine
//!
//! The engine populates the shared
//! [`DeviceDataBase`](fiftyone_device_detection_shared::DeviceDataBase) under the
//! shared
//! [`DEVICE_DATA_KEY`](fiftyone_device_detection_shared::DEVICE_DATA_KEY), the
//! same type and key the cloud engine uses. An application can swap an
//! on-premise engine for a cloud one (or the reverse) without changing how it
//! reads the result, the contract the
//! [`fiftyone_device_detection_shared`] crate exists to keep.
//!
//! ## Hot-swappable data file, no results cache
//!
//! The loaded data set lives behind an
//! [`ArcSwap`](arc_swap::ArcSwap), so
//! [`refresh`](fiftyone_pipeline_engines::OnPremiseAspectEngine::refresh) can
//! atomically replace it with a reloaded data file while in-flight detections
//! keep using the data set they snapshotted. Each result keeps its detection's
//! native results and reads a property from them the first time it is
//! requested, so a caller that reads one property does not pay to read the
//! rest. Those native results, and the data set they came from, stay alive
//! until the result is dropped, so the engine never caches results. Only the
//! cloud path may use a results cache.
//!
//! ## Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngineBuilder;
//! use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY};
//! use fiftyone_native::PerformanceProfile;
//! use fiftyone_pipeline_core::{Evidence, Pipeline};
//!
//! # fn main() -> fiftyone_pipeline_core::Result<()> {
//! let engine = DeviceDetectionOnPremiseEngineBuilder::new("51Degrees-LiteV4.1.hash")
//!     .performance_profile(PerformanceProfile::HighPerformance)
//!     .build()?;
//!
//! let pipeline = Pipeline::builder().add_element(engine).build()?;
//! let mut data = pipeline.create_flow_data_with(
//!     Evidence::builder()
//!         .add(
//!             "header.user-agent",
//!             "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
//!              AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 \
//!              Mobile/15E148 Safari/604.1",
//!         )
//!         .build(),
//! );
//! data.process()?;
//!
//! let device = data.get(DEVICE_DATA_KEY).expect("device data was produced");
//! if let Ok(is_mobile) = device.is_mobile().value() {
//!     println!("IsMobile = {is_mobile}");
//! }
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]

mod builder;
mod engine;
mod operational_copy;
mod retained;

pub use builder::{DeviceDetectionOnPremiseEngineBuilder, DATA_DOWNLOAD_TYPE};
pub use engine::DeviceDetectionOnPremiseEngine;
