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

//! The Device Detection and hardware profile cloud engines, found by the
//! names .NET gives their builders, so that a configuration file written for
//! .NET makes the same engines here. See
//! [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_builders`] adds `DeviceDetectionCloudEngineBuilder` and
//! `HardwareProfileCloudEngineBuilder`, which a configuration can also name
//! `DeviceDetectionCloudEngine` and `HardwareProfileCloudEngine`. Each engine
//! reads the response of the cloud request engine made before it, so a
//! configuration names one such engine first, and each takes the seven
//! settings of .NET's builder:
//!
//! - `SetLazyLoadingTimeout`, `SetCacheSize` and `SetCacheHitOrMiss` set to
//!   true are read and have no effect, with a warning logged, because the
//!   engines here have no lazy loading and no cache. A negative cache size
//!   is refused, as .NET refuses it.
//! - `SetProperties` and `SetProperty` are read and have no effect, with a
//!   warning logged, as they have no effect in .NET. The properties the
//!   cloud returns are those the cloud request engine asks for.
//! - `SetLazyLoading` and `SetCache` take their values in code only, so
//!   naming one fails as it does in .NET.
//!
//! The pipeline holds each engine made, so it can be read back.
//!
//! ```
//! use std::sync::Arc;
//!
//! use fiftyone_cloud_request_engine::{CloudHttpClient, CloudHttpRequest, CloudHttpResponse};
//! use fiftyone_device_detection_cloud::{configuration, DeviceDetectionCloudEngine};
//! use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
//! use fiftyone_pipeline_core::PipelineBuilder;
//! use fiftyone_pipeline_engines::AspectEngine;
//!
//! // Answers the two requests the request engine makes when it is built, in
//! // place of the cloud service.
//! struct StandIn;
//!
//! impl CloudHttpClient for StandIn {
//!     fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
//!         let body = if request.url.contains("evidencekeys") {
//!             r#"["header.user-agent"]"#
//!         } else {
//!             r#"{"Products":{"device":{"DataTier":"CloudV4Free","Properties":[
//!                 {"Name":"IsMobile","Type":"Boolean","Category":"Device"}]}}}"#
//!         };
//!         Ok(CloudHttpResponse { status: 200, body: body.to_string(), retry_after: None })
//!     }
//! }
//!
//! let mut registry = BuilderRegistry::new();
//! fiftyone_cloud_request_engine::configuration::add_builders(&mut registry);
//! configuration::add_builders(&mut registry);
//! let options = PipelineOptions::from_json(
//!     r#"{
//!         "PipelineOptions": {
//!             "Elements": [
//!                 {
//!                     "BuilderName": "CloudRequestEngine",
//!                     "BuildParameters": { "ResourceKey": "my-resource-key" }
//!                 },
//!                 { "BuilderName": "DeviceDetectionCloudEngine" }
//!             ]
//!         }
//!     }"#,
//! )?;
//! let client: Arc<dyn CloudHttpClient> = Arc::new(StandIn);
//! let pipeline = PipelineBuilder::new()
//!     .add_resource(Arc::new(client))
//!     .build_from_configuration(&registry, &options)?;
//!
//! let keys: Vec<&str> = pipeline.flow_elements().iter().map(|e| e.data_key()).collect();
//! assert_eq!(keys, ["cloud-response", "device"]);
//! let engine = pipeline
//!     .resource::<DeviceDetectionCloudEngine>()
//!     .expect("the Device Detection cloud engine");
//! assert_eq!(engine.data_source_tier(), "CloudV4Free");
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

use std::sync::Arc;

use fiftyone_cloud_request_engine::configuration::{add_aspect_engine_settings, request_engine};
use fiftyone_pipeline_core::configuration::{BuildContext, BuilderDefinition, BuilderRegistry};
use fiftyone_pipeline_core::FlowElement;

use crate::{
    DeviceDetectionCloudEngine, DeviceDetectionCloudEngineBuilder, HardwareProfileCloudEngine,
    HardwareProfileCloudEngineBuilder,
};

/// Add the Device Detection and hardware profile cloud engines' builders to
/// `registry`, as `DeviceDetectionCloudEngineBuilder` and
/// `HardwareProfileCloudEngineBuilder`. Returns `registry` for chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry
        .add(device_detection_cloud_engine_builder())
        .add(hardware_profile_cloud_engine_builder())
}

/// The Device Detection cloud engine's builder, finding the cloud request
/// engine made before it.
fn device_detection_cloud_engine_builder() -> BuilderDefinition<DeviceDetectionCloudEngineBuilder> {
    const NAME: &str = "DeviceDetectionCloudEngine";
    let definition = BuilderDefinition::new(
        "DeviceDetectionCloudEngineBuilder",
        |builder: DeviceDetectionCloudEngineBuilder, context: &mut BuildContext| {
            let request_engine = request_engine(context, NAME)?;
            let engine = Arc::new(builder.cloud_request_engine(request_engine).try_build()?);
            context.add_resource(Arc::clone(&engine) as _);
            Ok(engine as Arc<dyn FlowElement>)
        },
    );
    add_aspect_engine_settings(definition, NAME)
        .build_method(Vec::new(), |_, _| Ok(DeviceDetectionCloudEngine::builder()))
}

/// The hardware profile cloud engine's builder, finding the cloud request
/// engine made before it.
fn hardware_profile_cloud_engine_builder() -> BuilderDefinition<HardwareProfileCloudEngineBuilder> {
    const NAME: &str = "HardwareProfileCloudEngine";
    let definition = BuilderDefinition::new(
        "HardwareProfileCloudEngineBuilder",
        |builder: HardwareProfileCloudEngineBuilder, context: &mut BuildContext| {
            let request_engine = request_engine(context, NAME)?;
            let engine = Arc::new(builder.cloud_request_engine(request_engine).try_build()?);
            context.add_resource(Arc::clone(&engine) as _);
            Ok(engine as Arc<dyn FlowElement>)
        },
    );
    add_aspect_engine_settings(definition, NAME)
        .build_method(Vec::new(), |_, _| Ok(HardwareProfileCloudEngine::builder()))
}
