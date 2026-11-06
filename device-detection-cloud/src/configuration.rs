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

use fiftyone_cloud_request_engine::CloudRequestEngine;
use fiftyone_pipeline_core::configuration::{BuildContext, BuilderDefinition, BuilderRegistry};
use fiftyone_pipeline_core::{Error, FlowElement, Result};

use crate::{
    DeviceDetectionCloudEngine, DeviceDetectionCloudEngineBuilder, HardwareProfileCloudEngine,
    HardwareProfileCloudEngineBuilder,
};

/// The name of .NET's `LazyLoadingConfiguration`, the type `SetLazyLoading`
/// takes, as configuration errors give it.
const LAZY_LOADING_TYPE_NAME: &str = "LazyLoadingConfiguration";

/// The name of .NET's `CacheConfiguration`, the type `SetCache` takes, as
/// configuration errors give it.
const CACHE_TYPE_NAME: &str = "CacheConfiguration";

/// Add the Device Detection and hardware profile cloud engines' builders to
/// `registry`, as `DeviceDetectionCloudEngineBuilder` and
/// `HardwareProfileCloudEngineBuilder`. Returns `registry` for chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry
        .add(device_detection_cloud_engine_builder())
        .add(hardware_profile_cloud_engine_builder())
}

/// The cloud request engine made before the engine called `engine_name`,
/// or .NET's message when there is none or more than one, because .NET's
/// engine finds its request engine in the pipeline by type.
fn request_engine(context: &BuildContext, engine_name: &str) -> Result<Arc<CloudRequestEngine>> {
    match context.resources::<CloudRequestEngine>().as_slice() {
        [one] => Ok(Arc::clone(one)),
        _ => Err(Error::configuration(format!(
            "The '{engine_name}' requires a 'CloudRequestEngine' before it in the Pipeline. \
             This engine will be unable to produce results until this is corrected."
        ))),
    }
}

/// Log that the setting `setting` of the engine called `engine_name` has
/// no effect.
fn ignored(engine_name: &str, setting: &str) {
    log::warn!("The {engine_name} setting '{setting}' has no effect here and is ignored.");
}

/// Log that the properties setting `setting` of the engine called
/// `engine_name` has no effect.
fn properties_ignored(engine_name: &str, setting: &str) {
    log::warn!(
        "The {engine_name} setting '{setting}' has no effect, because the properties the cloud \
         returns are those the cloud request engine asks for."
    );
}

/// Refuse a negative cache size with the message .NET's cache gives.
fn check_cache_size(size: i32) -> Result<()> {
    if size < 0 {
        return Err(Error::configuration(format!(
            "capacity ('{size}') must be a non-negative value. (Parameter 'capacity')\nActual \
             value was {size}."
        )));
    }
    Ok(())
}

/// Add .NET's seven aspect engine settings to `definition`, in the order
/// .NET's `AspectEngineBuilderBase` declares them. None changes the
/// builder, as the engines here have no lazy loading, no cache and no list
/// of properties of their own.
fn aspect_engine_settings<B: Send + 'static>(
    definition: BuilderDefinition<B>,
    engine_name: &'static str,
) -> BuilderDefinition<B> {
    definition
        .code_only_setting("SetLazyLoading", LAZY_LOADING_TYPE_NAME)
        .setting("SetLazyLoadingTimeout", move |builder: B, _: i32| {
            ignored(engine_name, "SetLazyLoadingTimeout");
            builder
        })
        .code_only_setting("SetCache", CACHE_TYPE_NAME)
        .try_setting("SetCacheSize", move |builder: B, size: i32| {
            check_cache_size(size)?;
            ignored(engine_name, "SetCacheSize");
            Ok(builder)
        })
        .setting("SetCacheHitOrMiss", move |builder: B, record: bool| {
            if record {
                ignored(engine_name, "SetCacheHitOrMiss");
            }
            builder
        })
        .setting("SetProperties", move |builder: B, _: Vec<String>| {
            properties_ignored(engine_name, "SetProperties");
            builder
        })
        .setting("SetProperty", move |builder: B, _: String| {
            properties_ignored(engine_name, "SetProperty");
            builder
        })
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
    aspect_engine_settings(definition, NAME)
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
    aspect_engine_settings(definition, NAME)
        .build_method(Vec::new(), |_, _| Ok(HardwareProfileCloudEngine::builder()))
}
