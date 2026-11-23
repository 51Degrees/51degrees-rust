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

//! The TAC, Native and property keyed engines, found by the names .NET
//! gives their builders, so that a configuration file written for .NET makes
//! the same engines here. See [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_builders`] adds `TacEngineBuilder`, `NativeEngineBuilder` and
//! `PropertyKeyedDeviceEngineBuilder`, which a configuration can also name
//! without `Builder`. Each takes no build parameters, and .NET's settings:
//!
//! - `SetKeyProperty`, the property profiles are looked up by. The TAC and
//!   Native builders start with `TAC` and `NativeModel`. The property keyed
//!   builder has none, so a configuration of it must name one.
//! - `SetElementDataKey`, the key the engine writes its data under.
//! - `SetCacheSize` and `SetCacheHitOrMiss` are read and have no effect,
//!   with .NET's warning that the engines have no cache logged, as .NET
//!   logs it. `SetLazyLoadingTimeout` is read and has no effect, with a
//!   warning logged, as the engines have no lazy loading.
//! - `SetProperties` and `SetProperty` are read and have no effect, as in
//!   .NET, because each engine gives only `Profiles`. A warning is logged.
//! - `SetCache` and `SetLazyLoading` take their values in code only, so
//!   naming one fails as it does in .NET.
//!
//! The engines need an on-premise Device Detection engine earlier in the
//! pipeline, so a configuration names one first, with the key properties in
//! its `IndexedProperties` setting so each lookup reads the data file's
//! index. The pipeline holds each engine made, and [`Pipeline::resource`]
//! gives the first.
//!
//! [`Pipeline::resource`]: fiftyone_pipeline_core::Pipeline::resource
//!
//! ```no_run
//! use fiftyone_device_detection_property_keyed::configuration;
//! use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
//! use fiftyone_pipeline_core::PipelineBuilder;
//!
//! let mut registry = BuilderRegistry::new();
//! fiftyone_device_detection_onpremise::configuration::add_builders(&mut registry);
//! configuration::add_builders(&mut registry);
//! let options = PipelineOptions::from_json(
//!     r#"{
//!         "PipelineOptions": {
//!             "Elements": [
//!                 {
//!                     "BuilderName": "DeviceDetectionHashEngine",
//!                     "BuildParameters": {
//!                         "DataFile": "TAC-HashV41.hash",
//!                         "CreateTempDataCopy": "false",
//!                         "AutoUpdate": "false",
//!                         "DataFileSystemWatcher": "false",
//!                         "IndexedProperties": "TAC,NativeModel"
//!                     }
//!                 },
//!                 { "BuilderName": "TacEngine" },
//!                 { "BuilderName": "NativeEngine" }
//!             ]
//!         }
//!     }"#,
//! )?;
//! let pipeline = PipelineBuilder::new().build_from_configuration(&registry, &options)?;
//! let keys: Vec<&str> = pipeline
//!     .flow_elements()
//!     .iter()
//!     .map(|element| element.data_key())
//!     .collect();
//! assert_eq!(keys, ["device", "hardware", "hardware"]);
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

use std::sync::Arc;

use fiftyone_pipeline_core::configuration::{BuildContext, BuilderDefinition, BuilderRegistry};
use fiftyone_pipeline_core::{FlowElement, Result};

use crate::{
    NativeEngineBuilder, PropertyKeyedDeviceEngine, PropertyKeyedDeviceEngineBuilder,
    TacEngineBuilder,
};

/// .NET's warning for a cache setting, which a property keyed engine reads
/// and ignores.
const NO_CACHE: &str = "Caching is not supported by property keyed engines.";

/// Why the property settings have no effect.
const PROFILES_ONLY: &str = "as the engine gives only Profiles, as .NET's does";

/// Add the three engines' builders to `registry`, as `NativeEngineBuilder`,
/// `PropertyKeyedDeviceEngineBuilder` and `TacEngineBuilder`, the order .NET
/// lists them in among the available builders. Returns `registry` for
/// chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry
        .add(definition::<NativeEngineBuilder>("NativeEngine"))
        .add(definition::<PropertyKeyedDeviceEngineBuilder>(
            "PropertyKeyedDeviceEngine",
        ))
        .add(definition::<TacEngineBuilder>("TacEngine"))
}

/// The settings the three builders share, as .NET's
/// `PropertyKeyedDeviceEngineBaseBuilder` declares them.
trait KeyedBuilder: Sized + Send + 'static {
    fn create() -> Self;
    fn key_property(self, key_property: String) -> Self;
    fn data_key(self, data_key: &'static str) -> Self;
    fn finish(self) -> Result<Arc<PropertyKeyedDeviceEngine>>;
}

macro_rules! keyed_builder {
    ($builder:ty) => {
        impl KeyedBuilder for $builder {
            fn create() -> Self {
                <$builder>::new()
            }

            fn key_property(self, key_property: String) -> Self {
                self.set_key_property(key_property)
            }

            fn data_key(self, data_key: &'static str) -> Self {
                self.set_element_data_key(data_key)
            }

            fn finish(self) -> Result<Arc<PropertyKeyedDeviceEngine>> {
                self.build()
            }
        }
    };
}

keyed_builder!(TacEngineBuilder);
keyed_builder!(NativeEngineBuilder);
keyed_builder!(PropertyKeyedDeviceEngineBuilder);

/// Log that the setting `setting` of the engine `engine` has no effect, and
/// why.
fn ignored(engine: &str, setting: &str, why: &str) {
    log::warn!("The {engine} setting '{setting}' has no effect here and is ignored, {why}.");
}

/// The builder of the engine named `engine`, found as `engine` and
/// `Builder`, with its settings in the order .NET's reflection lists them,
/// being those of `PropertyKeyedDeviceEngineBaseBuilder`, then
/// `PropertyKeyedEngineBuilderBase`, then those of `AspectEngineBuilderBase`
/// it does not override.
fn definition<B: KeyedBuilder>(engine: &'static str) -> BuilderDefinition<B> {
    BuilderDefinition::new(
        format!("{engine}Builder"),
        |builder: B, context: &mut BuildContext| {
            let made = builder.finish()?;
            context.add_resource(Arc::clone(&made) as _);
            Ok(made as Arc<dyn FlowElement>)
        },
    )
    .setting("SetKeyProperty", |builder: B, key_property: String| {
        builder.key_property(key_property)
    })
    // The pipeline's typed keys hold their name for the life of the
    // process, so a configured data key is kept for as long, as the
    // builder's default key is. A configuration is read once for a
    // pipeline, so this is one short string each time.
    .setting("SetElementDataKey", |builder: B, data_key: String| {
        builder.data_key(Box::leak(data_key.into_boxed_str()))
    })
    .setting("SetCacheSize", |builder: B, _: i32| {
        log::warn!("{NO_CACHE}");
        builder
    })
    .setting("SetCacheHitOrMiss", |builder: B, _: bool| {
        log::warn!("{NO_CACHE}");
        builder
    })
    .code_only_setting("SetCache", "CacheConfiguration")
    .code_only_setting("SetLazyLoading", "LazyLoadingConfiguration")
    .setting("SetLazyLoadingTimeout", move |builder: B, _: i32| {
        ignored(
            engine,
            "SetLazyLoadingTimeout",
            "as the engine has no lazy loading",
        );
        builder
    })
    .setting("SetProperties", move |builder: B, _: Vec<String>| {
        ignored(engine, "SetProperties", PROFILES_ONLY);
        builder
    })
    .setting("SetProperty", move |builder: B, _: String| {
        ignored(engine, "SetProperty", PROFILES_ONLY);
        builder
    })
    .build_method(Vec::new(), |_, _| Ok(B::create()))
}
