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

//! The cloud request engine, found by the name .NET gives its builder, so
//! that a configuration file written for .NET makes the same engine here.
//! See [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_builders`] adds `CloudRequestEngineBuilder`, which a configuration
//! can also name `CloudRequestEngine`. It takes the 22 settings of .NET's
//! builder, read and checked as .NET reads and checks them:
//!
//! - `SetEndPoint` sets the data, properties and evidence keys endpoints
//!   from a base URL, adding a `/` when it has none, and
//!   `SetDataEndpoint`, `SetPropertiesEndpoint` and `SetEvidenceKeysEndpoint`
//!   set one each, so the one given last wins. An endpoint left empty takes
//!   the default when the engine is built.
//! - `SetResourceKey` and `SetLicenseKey`, and `SetProperties` and
//!   `SetProperty`, which add to the properties the engine asks for, being
//!   [`CloudRequestEngineBuilder::values`]. The combinations refused in code
//!   are refused here too. `SetProperties` can also be named
//!   `RequestedProperties`, the cloud request engine specification's name,
//!   which .NET refuses.
//! - `SetTimeOutSeconds`, where 0 or less means no limit.
//! - `SetFailuresToEnterRecovery` from 1 to 1000, held at the engine's
//!   largest value of 100 with a warning logged, `SetFailuresWindowSeconds`
//!   from 1, and `SetRecoverySeconds`.
//! - `SetCloudRequestOrigin`.
//! - `SetCacheSize`, which gives the engine an [`LruCache`] of that many
//!   responses.
//! - `SetUseExponentialBackoff` set to true, `SetExponentialBackoffInitialDelay`,
//!   `SetExponentialBackoffMaxDelay`, `SetExponentialBackoffMultiplier`,
//!   `SetLazyLoadingTimeout` and `SetCacheHitOrMiss` set to false are read
//!   and have no effect, with a warning logged. The engine here has one
//!   fixed recovery period and no lazy loading, and always records a cache
//!   hit.
//! - `SetLazyLoading` and `SetCache` take their values in code only, so
//!   naming one fails as it does in .NET.
//!
//! The engine sends its requests through the `Arc<dyn CloudHttpClient>` the
//! [`PipelineBuilder`](fiftyone_pipeline_core::PipelineBuilder) was given as
//! a resource, wrapped in a further `Arc` as `add_resource(Arc::new(client))`
//! below does, or through the built-in client of the `reqwest-client`
//! feature, which is off by default, when it was given none. A client given
//! in any other form, such as `add_resource(Arc::new(MyClient))`, is not
//! found. The pipeline holds the engine, so the cloud engines after it find
//! it, and what a configuration set can be read back.
//!
//! The builder of each cloud aspect engine finds that engine with
//! [`request_engine`], and takes the settings .NET's cloud aspect engine
//! builders share through [`add_aspect_engine_settings`].
//!
//! ```
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! use fiftyone_cloud_request_engine::{
//!     configuration, CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
//! };
//! use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
//! use fiftyone_pipeline_core::PipelineBuilder;
//!
//! // Answers the two requests the engine makes when it is built, in place
//! // of the cloud service.
//! struct StandIn;
//!
//! impl CloudHttpClient for StandIn {
//!     fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
//!         let body = if request.url.contains("evidencekeys") {
//!             r#"["header.user-agent"]"#
//!         } else {
//!             r#"{"Products":{}}"#
//!         };
//!         Ok(CloudHttpResponse { status: 200, body: body.to_string(), retry_after: None })
//!     }
//! }
//!
//! let mut registry = BuilderRegistry::new();
//! configuration::add_builders(&mut registry);
//! let options = PipelineOptions::from_json(
//!     r#"{
//!         "PipelineOptions": {
//!             "Elements": [
//!                 {
//!                     "BuilderName": "CloudRequestEngine",
//!                     "BuildParameters": { "ResourceKey": "my-resource-key", "TimeOutSeconds": 5 }
//!                 }
//!             ]
//!         }
//!     }"#,
//! )?;
//! let client: Arc<dyn CloudHttpClient> = Arc::new(StandIn);
//! let pipeline = PipelineBuilder::new()
//!     .add_resource(Arc::new(client))
//!     .build_from_configuration(&registry, &options)?;
//!
//! let engine = pipeline
//!     .resource::<CloudRequestEngine>()
//!     .expect("the cloud request engine");
//! assert_eq!(engine.resource_key(), Some("my-resource-key"));
//! assert_eq!(engine.timeout(), Duration::from_secs(5));
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

use std::sync::Arc;

use fiftyone_caching::{CacheBuilder, LruCache};
use fiftyone_pipeline_core::configuration::{BuildContext, BuilderDefinition, BuilderRegistry};
use fiftyone_pipeline_core::{Error, FlowElement, Result};

use crate::constants::{
    DATA_FILENAME, EVIDENCE_KEYS_FILENAME, FAILURES_TO_ENTER_RECOVERY_MAX,
    FAILURES_TO_ENTER_RECOVERY_MIN, PROPERTIES_FILENAME,
};
use crate::{CloudHttpClient, CloudRequestEngine, CloudRequestEngineBuilder};

/// The most failures to enter recovery that .NET's builder accepts.
const CONFIGURED_FAILURES_TO_ENTER_RECOVERY_MAX: u32 = 1000;

/// What the build says with no client and no built-in one. A client of
/// any other type, such as one given as `Arc::new(MyClient)`, is not found.
const NO_CLIENT_RESOURCE: &str =
    "no CloudHttpClient was given to PipelineBuilder::add_resource and the \
     `reqwest-client` feature is not enabled, so the CloudRequestEngine has no \
     HTTP transport. Give one with add_resource(Arc::new(client)), where client is \
     an Arc<dyn CloudHttpClient>, or enable the `reqwest-client` feature to use \
     the built-in reqwest client";

/// The timeout in seconds that means no limit, as the engine's builder
/// takes it.
const NO_LIMIT_SECONDS: u64 = 0;

/// The name of .NET's `LazyLoadingConfiguration`, the type `SetLazyLoading`
/// takes, as configuration errors give it.
const LAZY_LOADING_TYPE_NAME: &str = "LazyLoadingConfiguration";

/// The name of .NET's `CacheConfiguration`, the type `SetCache` takes, as
/// configuration errors give it.
const CACHE_TYPE_NAME: &str = "CacheConfiguration";

/// Add the cloud request engine's builder to `registry`, as
/// `CloudRequestEngineBuilder`. Returns `registry` for chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry.add(cloud_request_engine_builder())
}

/// The cloud request engine made before the cloud aspect engine called
/// `engine_name`, for that engine's builder to read from. .NET's cloud
/// aspect engine finds its request engine in the pipeline by type, so with
/// none made before it, or more than one, this gives .NET's message.
pub fn request_engine(
    context: &BuildContext,
    engine_name: &str,
) -> Result<Arc<CloudRequestEngine>> {
    match context.resources::<CloudRequestEngine>().as_slice() {
        [one] => Ok(Arc::clone(one)),
        _ => Err(Error::configuration(format!(
            "The '{engine_name}' requires a 'CloudRequestEngine' before it in the Pipeline. \
             This engine will be unable to produce results until this is corrected."
        ))),
    }
}

/// Add the seven settings of .NET's `AspectEngineBuilderBase` to the
/// builder of the cloud aspect engine called `engine_name`, in the order
/// .NET declares them. None changes the builder, as the cloud engines here
/// have no lazy loading, no cache and no list of properties of their own.
///
/// - `SetLazyLoadingTimeout`, `SetCacheSize` and `SetCacheHitOrMiss` set to
///   true are read and have no effect, with a warning logged. A negative
///   cache size is refused, as .NET refuses it.
/// - `SetProperties` and `SetProperty` are read and have no effect, with a
///   warning logged, as they have no effect in .NET. The properties the
///   cloud returns are those the cloud request engine asks for.
/// - `SetLazyLoading` and `SetCache` take their values in code only, so
///   naming one fails as it does in .NET.
pub fn add_aspect_engine_settings<B: Send + 'static>(
    definition: BuilderDefinition<B>,
    engine_name: &'static str,
) -> BuilderDefinition<B> {
    definition
        .code_only_setting("SetLazyLoading", LAZY_LOADING_TYPE_NAME)
        .setting("SetLazyLoadingTimeout", move |builder: B, _: i32| {
            aspect_setting_ignored(engine_name, "SetLazyLoadingTimeout");
            builder
        })
        .code_only_setting("SetCache", CACHE_TYPE_NAME)
        .try_setting("SetCacheSize", move |builder: B, size: i32| {
            if size < 0 {
                return Err(negative_cache_size(size));
            }
            aspect_setting_ignored(engine_name, "SetCacheSize");
            Ok(builder)
        })
        .setting("SetCacheHitOrMiss", move |builder: B, record: bool| {
            if record {
                aspect_setting_ignored(engine_name, "SetCacheHitOrMiss");
            }
            builder
        })
        .setting("SetProperties", move |builder: B, _: Vec<String>| {
            aspect_properties_ignored(engine_name, "SetProperties");
            builder
        })
        .setting("SetProperty", move |builder: B, _: String| {
            aspect_properties_ignored(engine_name, "SetProperty");
            builder
        })
}

/// Log that the setting `setting` of the cloud aspect engine called
/// `engine_name` has no effect.
fn aspect_setting_ignored(engine_name: &str, setting: &str) {
    log::warn!("The {engine_name} setting '{setting}' has no effect here and is ignored.");
}

/// Log that the properties setting `setting` of the cloud aspect engine
/// called `engine_name` has no effect.
fn aspect_properties_ignored(engine_name: &str, setting: &str) {
    log::warn!(
        "The {engine_name} setting '{setting}' has no effect, because the properties the cloud \
         returns are those the cloud request engine asks for."
    );
}

/// .NET's message for a negative cache size, which its cache refuses.
fn negative_cache_size(size: i32) -> Error {
    Error::configuration(format!(
        "capacity ('{size}') must be a non-negative value. (Parameter 'capacity')\nActual value \
         was {size}."
    ))
}

/// The settings read from a configuration, held as .NET's
/// `CloudRequestEngineBuilder` holds them until the engine is made. A
/// setting that is not given keeps the engine builder's default.
#[derive(Default)]
struct CloudRequestEngineSettings {
    data_endpoint: Option<String>,
    properties_endpoint: Option<String>,
    evidence_keys_endpoint: Option<String>,
    resource_key: Option<String>,
    license_key: Option<String>,
    timeout_seconds: Option<i32>,
    failures_to_enter_recovery: Option<u32>,
    failures_window_seconds: Option<u64>,
    recovery_seconds: Option<f64>,
    cloud_request_origin: Option<String>,
    cache_size: Option<usize>,
    properties: Vec<String>,
}

impl CloudRequestEngineSettings {
    /// Set the three endpoints from the base URL `uri`, as .NET's
    /// `SetEndPoint` does.
    fn set_end_point(&mut self, uri: &str) {
        let base = if uri.ends_with('/') {
            uri.to_string()
        } else {
            format!("{uri}/")
        };
        self.data_endpoint = Some(format!("{base}{DATA_FILENAME}"));
        self.properties_endpoint = Some(format!("{base}{PROPERTIES_FILENAME}"));
        self.evidence_keys_endpoint = Some(format!("{base}{EVIDENCE_KEYS_FILENAME}"));
    }

    /// The engine builder with the settings given.
    fn builder(self) -> CloudRequestEngineBuilder {
        // An endpoint that is empty or only white space takes the default.
        let given = |url: Option<String>| url.filter(|url| !url.trim().is_empty());
        let mut builder = CloudRequestEngine::builder();
        if let Some(url) = given(self.data_endpoint) {
            builder = builder.data_endpoint(url);
        }
        if let Some(url) = given(self.properties_endpoint) {
            builder = builder.properties_endpoint(url);
        }
        if let Some(url) = given(self.evidence_keys_endpoint) {
            builder = builder.evidence_keys_endpoint(url);
        }
        if let Some(key) = self.resource_key {
            builder = builder.resource_key(key);
        }
        if let Some(key) = self.license_key {
            builder = builder.license_key(key);
        }
        if let Some(seconds) = self.timeout_seconds {
            builder = builder.timeout_seconds(u64::try_from(seconds).unwrap_or(NO_LIMIT_SECONDS));
        }
        if let Some(failures) = self.failures_to_enter_recovery {
            builder = builder.failures_to_enter_recovery(failures);
        }
        if let Some(seconds) = self.failures_window_seconds {
            builder = builder.failures_window_seconds(seconds);
        }
        if let Some(seconds) = self.recovery_seconds {
            builder = builder.recovery_seconds(seconds);
        }
        if let Some(origin) = self.cloud_request_origin {
            builder = builder.cloud_request_origin(origin);
        }
        if let Some(size) = self.cache_size {
            let cache: LruCache<String, String> = CacheBuilder::new().size(size).build();
            builder = builder.cache(Arc::new(cache));
        }
        if !self.properties.is_empty() {
            builder = builder.values(self.properties);
        }
        builder
    }
}

fn ignored(setting: &str) {
    log::warn!("The cloud request engine setting '{setting}' has no effect here and is ignored.");
}

/// The builder, with its settings in the order .NET's
/// `CloudRequestEngineBuilder` and its base class declare them.
fn cloud_request_engine_builder() -> BuilderDefinition<CloudRequestEngineSettings> {
    BuilderDefinition::new(
        "CloudRequestEngineBuilder",
        |settings: CloudRequestEngineSettings, context: &mut BuildContext| {
            let mut builder = settings.builder();
            builder = match context.latest_resource::<Arc<dyn CloudHttpClient>>() {
                Some(client) => builder.http_client(Arc::clone(client.as_ref())),
                None => builder.no_client_message(NO_CLIENT_RESOURCE),
            };
            let engine = Arc::new(builder.build()?);
            context.add_resource(Arc::clone(&engine) as _);
            Ok(engine as Arc<dyn FlowElement>)
        },
    )
    .setting(
        "SetEndPoint",
        |mut settings: CloudRequestEngineSettings, uri: String| {
            settings.set_end_point(&uri);
            settings
        },
    )
    .setting(
        "SetDataEndpoint",
        |mut settings: CloudRequestEngineSettings, url: String| {
            settings.data_endpoint = Some(url);
            settings
        },
    )
    .setting(
        "SetPropertiesEndpoint",
        |mut settings: CloudRequestEngineSettings, url: String| {
            settings.properties_endpoint = Some(url);
            settings
        },
    )
    .setting(
        "SetEvidenceKeysEndpoint",
        |mut settings: CloudRequestEngineSettings, url: String| {
            settings.evidence_keys_endpoint = Some(url);
            settings
        },
    )
    .setting(
        "SetResourceKey",
        |mut settings: CloudRequestEngineSettings, key: String| {
            settings.resource_key = Some(key);
            settings
        },
    )
    .setting(
        "SetLicenseKey",
        |mut settings: CloudRequestEngineSettings, key: String| {
            settings.license_key = Some(key);
            settings
        },
    )
    .setting(
        "SetTimeOutSeconds",
        |mut settings: CloudRequestEngineSettings, seconds: i32| {
            settings.timeout_seconds = Some(seconds);
            settings
        },
    )
    .try_setting(
        "SetFailuresToEnterRecovery",
        |mut settings: CloudRequestEngineSettings, failures: i32| {
            let range = FAILURES_TO_ENTER_RECOVERY_MIN..=CONFIGURED_FAILURES_TO_ENTER_RECOVERY_MAX;
            let Some(failures) = u32::try_from(failures).ok().filter(|f| range.contains(f)) else {
                return Err(Error::configuration(format!(
                    "failuresToEnterRecovery must be within {} and {} (both inclusive). \
                     Received: {failures}. (Parameter 'failuresToEnterRecovery')",
                    range.start(),
                    range.end()
                )));
            };
            if failures > FAILURES_TO_ENTER_RECOVERY_MAX {
                log::warn!(
                    "The cloud request engine takes at most {FAILURES_TO_ENTER_RECOVERY_MAX} \
                     failures to enter recovery, so it uses {FAILURES_TO_ENTER_RECOVERY_MAX} \
                     where {failures} was given."
                );
            }
            settings.failures_to_enter_recovery = Some(failures);
            Ok(settings)
        },
    )
    .try_setting(
        "SetFailuresWindowSeconds",
        |mut settings: CloudRequestEngineSettings, seconds: i32| {
            let Ok(window @ 1..) = u64::try_from(seconds) else {
                return Err(Error::configuration(format!(
                    "failuresWindowSeconds must be positive, Received: {seconds}. (Parameter \
                     'failuresWindowSeconds')"
                )));
            };
            settings.failures_window_seconds = Some(window);
            Ok(settings)
        },
    )
    .setting(
        "SetRecoverySeconds",
        |mut settings: CloudRequestEngineSettings, seconds: f64| {
            settings.recovery_seconds = Some(seconds);
            settings
        },
    )
    .setting(
        "SetUseExponentialBackoff",
        |settings: CloudRequestEngineSettings, use_backoff: bool| {
            if use_backoff {
                ignored("SetUseExponentialBackoff");
            }
            settings
        },
    )
    .setting(
        "SetExponentialBackoffInitialDelay",
        |settings: CloudRequestEngineSettings, _: f64| {
            ignored("SetExponentialBackoffInitialDelay");
            settings
        },
    )
    .setting(
        "SetExponentialBackoffMaxDelay",
        |settings: CloudRequestEngineSettings, _: f64| {
            ignored("SetExponentialBackoffMaxDelay");
            settings
        },
    )
    .setting(
        "SetExponentialBackoffMultiplier",
        |settings: CloudRequestEngineSettings, _: f64| {
            ignored("SetExponentialBackoffMultiplier");
            settings
        },
    )
    .setting(
        "SetCloudRequestOrigin",
        |mut settings: CloudRequestEngineSettings, origin: String| {
            settings.cloud_request_origin = Some(origin);
            settings
        },
    )
    .code_only_setting("SetLazyLoading", LAZY_LOADING_TYPE_NAME)
    .setting(
        "SetLazyLoadingTimeout",
        |settings: CloudRequestEngineSettings, _: i32| {
            ignored("SetLazyLoadingTimeout");
            settings
        },
    )
    .code_only_setting("SetCache", CACHE_TYPE_NAME)
    .try_setting(
        "SetCacheSize",
        |mut settings: CloudRequestEngineSettings, size: i32| {
            let Ok(capacity) = usize::try_from(size) else {
                return Err(negative_cache_size(size));
            };
            settings.cache_size = Some(capacity);
            Ok(settings)
        },
    )
    .setting(
        "SetCacheHitOrMiss",
        |settings: CloudRequestEngineSettings, record: bool| {
            if !record {
                ignored("SetCacheHitOrMiss");
            }
            settings
        },
    )
    .setting(
        "SetProperties",
        |mut settings: CloudRequestEngineSettings, properties: Vec<String>| {
            settings.properties.extend(properties);
            settings
        },
    )
    .setting(
        "SetProperty",
        |mut settings: CloudRequestEngineSettings, property: String| {
            settings.properties.push(property);
            settings
        },
    )
    .setting_alternate_name("SetProperties", "RequestedProperties")
    .build_method(Vec::new(), |_, _| Ok(CloudRequestEngineSettings::default()))
}
