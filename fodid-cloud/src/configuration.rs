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

//! The 51Did cloud engine, found by the name .NET gives its builder, so
//! that a configuration file written for .NET makes the same engine here.
//! See [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_builders`] adds `DidCloudEngineBuilder`, which a configuration can
//! also name `DidCloudEngine`. The engine reads the response of the cloud
//! request engine made before it, so a configuration names one such engine
//! first. It takes the seven settings of .NET's builder, none of which has
//! an effect here, as [`add_aspect_engine_settings`] describes.
//!
//! The pipeline holds the engine made, so it can be read back.
//!
//! ```
//! use std::sync::Arc;
//!
//! use fiftyone_cloud_request_engine::{CloudHttpClient, CloudHttpRequest, CloudHttpResponse};
//! use fiftyone_fodid_cloud::{configuration, FodIdCloudEngine};
//! use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
//! use fiftyone_pipeline_core::{FlowElement, PipelineBuilder};
//! use fiftyone_pipeline_engines::AspectEngine;
//!
//! // Answers the two requests the request engine makes when it is built, in
//! // place of the cloud service.
//! struct StandIn;
//!
//! impl CloudHttpClient for StandIn {
//!     fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
//!         let body = if request.url.contains("evidencekeys") {
//!             r#"["query.client-ip","query.id.usage"]"#
//!         } else {
//!             r#"{"Products":{"FODid":{"DataTier":"CloudV5Bespoke","Properties":[
//!                 {"Name":"IdProbGlobal","Type":"String","Category":"FODid"}]}}}"#
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
//!                 { "BuilderName": "DidCloudEngine" }
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
//! assert_eq!(keys, ["cloud-response", "fodid"]);
//! let engine = pipeline
//!     .resource::<FodIdCloudEngine>()
//!     .expect("the 51Did cloud engine");
//! assert!(engine.properties_loaded());
//! let names: Vec<&str> = engine.aspect_properties().iter().map(|p| p.name()).collect();
//! assert_eq!(names, ["IdProbGlobal"]);
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

use std::sync::Arc;

use fiftyone_cloud_request_engine::configuration::{add_aspect_engine_settings, request_engine};
use fiftyone_pipeline_core::configuration::{BuildContext, BuilderDefinition, BuilderRegistry};
use fiftyone_pipeline_core::FlowElement;

use crate::{FodIdCloudEngine, FodIdCloudEngineBuilder};

/// The name of the engine in the messages .NET gives.
const ENGINE_NAME: &str = "DidCloudEngine";

/// Add the 51Did cloud engine's builder to `registry`, as
/// `DidCloudEngineBuilder`. Returns `registry` for chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry.add(did_cloud_engine_builder())
}

/// The 51Did cloud engine's builder, finding the cloud request engine made
/// before it.
fn did_cloud_engine_builder() -> BuilderDefinition<FodIdCloudEngineBuilder> {
    let definition = BuilderDefinition::new(
        "DidCloudEngineBuilder",
        |builder: FodIdCloudEngineBuilder, context: &mut BuildContext| {
            let request_engine = request_engine(context, ENGINE_NAME)?;
            let engine = Arc::new(builder.cloud_request_engine(request_engine).build()?);
            context.add_resource(Arc::clone(&engine) as _);
            Ok(engine as Arc<dyn FlowElement>)
        },
    );
    add_aspect_engine_settings(definition, ENGINE_NAME)
        .build_method(Vec::new(), |_, _| Ok(FodIdCloudEngine::builder()))
}
