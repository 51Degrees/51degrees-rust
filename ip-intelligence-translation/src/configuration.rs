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

//! The translation engines, found by the names .NET gives their builders,
//! so that a configuration file written for .NET makes the same engines
//! here. See [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_builders`] adds these builders, in the order .NET listed them. A
//! configuration can also name each without `Builder` at the end.
//!
//! - `CountriesTranslationEngineBuilder` and
//!   `CountryCodeTranslationEngineBuilder`, which take no settings, as their
//!   sources and translations are compiled in.
//! - `TranslationEngineBuilder`, the builder of the translation engine both
//!   are made from (see [`fiftyone_pipeline_translation::configuration`]).
//! - `CloudCountriesTranslationEngineBuilder`. The engine reads the response
//!   of the cloud request engine made before it, so a configuration names
//!   one such engine first. It takes the seven settings of .NET's builder,
//!   none of which has an effect here, as [`add_aspect_engine_settings`]
//!   describes.
//!
//! The pipeline holds the engine made, so it can be read back.
//!
//! ```
//! use std::sync::Arc;
//!
//! use fiftyone_cloud_request_engine::{CloudHttpClient, CloudHttpRequest, CloudHttpResponse};
//! use fiftyone_ip_intelligence_translation::{configuration, CloudCountriesTranslationEngine};
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
//!             r#"["query.client-ip","header.accept-language"]"#
//!         } else {
//!             r#"{"Products":{"countrynamestranslated":{"DataTier":"CloudV4","Properties":[
//!                 {"Name":"CountryNamesGeographicalTranslated","Type":"WeightedString"},
//!                 {"Name":"CountryCodesGeographicalAll","Type":"Array"}]}}}"#
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
//!                 { "BuilderName": "CloudCountriesTranslationEngine" }
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
//! assert_eq!(keys, ["cloud-response", "countrynamestranslated"]);
//! let engine = pipeline
//!     .resource::<CloudCountriesTranslationEngine>()
//!     .expect("the cloud countries translation engine");
//! let names: Vec<&str> = engine.aspect_properties().iter().map(|p| p.name()).collect();
//! assert_eq!(names, ["CountryNamesGeographicalTranslated", "CountryCodesGeographicalAll"]);
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

use std::sync::Arc;

use fiftyone_cloud_request_engine::configuration::{add_aspect_engine_settings, request_engine};
use fiftyone_pipeline_core::configuration::{BuildContext, BuilderDefinition, BuilderRegistry};
use fiftyone_pipeline_core::FlowElement;

use crate::{
    CloudCountriesTranslationEngine, CloudCountriesTranslationEngineBuilder,
    CountriesTranslationEngine, CountriesTranslationEngineBuilder, CountryCodeTranslationEngine,
    CountryCodeTranslationEngineBuilder,
};

/// The name of the engine in the messages .NET gives.
const CLOUD_COUNTRIES_ENGINE_NAME: &str = "CloudCountriesTranslationEngine";

/// Add the translation engines' builders to `registry`. Returns `registry`
/// for chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry
        .add(countries_translation_engine_builder())
        .add(country_code_translation_engine_builder());
    fiftyone_pipeline_translation::configuration::add_builders(registry);
    registry.add(cloud_countries_translation_engine_builder())
}

/// The builder of the engine that translates country names into the
/// request's language, which takes no settings.
fn countries_translation_engine_builder() -> BuilderDefinition<CountriesTranslationEngineBuilder> {
    BuilderDefinition::new(
        "CountriesTranslationEngineBuilder",
        |builder: CountriesTranslationEngineBuilder, context: &mut BuildContext| {
            let engine = Arc::new(builder.build()?);
            context.add_resource(Arc::clone(&engine) as _);
            Ok(engine as Arc<dyn FlowElement>)
        },
    )
    .build_method(Vec::new(), |_, _| Ok(CountriesTranslationEngine::builder()))
}

/// The builder of the engine that gives each country code's English name,
/// which takes no settings.
fn country_code_translation_engine_builder(
) -> BuilderDefinition<CountryCodeTranslationEngineBuilder> {
    BuilderDefinition::new(
        "CountryCodeTranslationEngineBuilder",
        |builder: CountryCodeTranslationEngineBuilder, context: &mut BuildContext| {
            let engine = Arc::new(builder.build()?);
            context.add_resource(Arc::clone(&engine) as _);
            Ok(engine as Arc<dyn FlowElement>)
        },
    )
    .build_method(Vec::new(), |_, _| {
        Ok(CountryCodeTranslationEngine::builder())
    })
}

/// The cloud countries translation engine's builder, finding the cloud
/// request engine made before it.
fn cloud_countries_translation_engine_builder(
) -> BuilderDefinition<CloudCountriesTranslationEngineBuilder> {
    let definition = BuilderDefinition::new(
        "CloudCountriesTranslationEngineBuilder",
        |builder: CloudCountriesTranslationEngineBuilder, context: &mut BuildContext| {
            let request_engine = request_engine(context, CLOUD_COUNTRIES_ENGINE_NAME)?;
            let engine = Arc::new(builder.cloud_request_engine(request_engine).build()?);
            context.add_resource(Arc::clone(&engine) as _);
            Ok(engine as Arc<dyn FlowElement>)
        },
    );
    add_aspect_engine_settings(definition, CLOUD_COUNTRIES_ENGINE_NAME)
        .build_method(Vec::new(), |_, _| {
            Ok(CloudCountriesTranslationEngine::builder())
        })
}
