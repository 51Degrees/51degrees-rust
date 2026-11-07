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

//! The agent signature element, found by the name .NET gives its builder,
//! so that a configuration file written for .NET makes the same element
//! here. See [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_builders`] adds `AgentSignatureElementBuilder`, which a
//! configuration can also name `AgentSignatureElement`, with the 14
//! settings of .NET's builder, read and checked as .NET reads and checks
//! them:
//!
//! - `SetCacheSize` and `SetMaxResponseBytes`, each at least 1.
//! - `SetCacheLifetime`, `SetNegativeCacheLifetime`, `SetWaitBudget`,
//!   `SetFetchTimeout`, `SetClockSkew` and `SetMaxLifetime`, each a period
//!   written as .NET writes a `TimeSpan`, such as `00:05:00` or
//!   `00:00:00.350`, from zero to 365 days.
//! - `SetRegistry`, which adds one registry of agent cards, and
//!   `SetReachabilityCheckUrl`, each an address.
//! - `SetTrustForwardedEvidence`, `SetAllowLegacySignatureAgent` and
//!   `SetAllowInlineDirectory`, each `true` or `false`.
//! - `SetHttpClient` takes its value in code only, so naming it fails as it
//!   does in .NET.
//!
//! The element fetches through the `Arc<dyn HttpClient>` the
//! [`PipelineBuilder`](fiftyone_pipeline_core::PipelineBuilder) was given as
//! a resource, or through the built-in client of the `reqwest-client`
//! feature when it was given none. The pipeline holds the element, so what a
//! configuration set can be read back.
//!
//! ```
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! use fiftyone_agent_signature::{
//!     configuration, AgentSignatureElement, HttpClient, HttpRequest, HttpResponse,
//! };
//! use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
//! use fiftyone_pipeline_core::PipelineBuilder;
//!
//! // A client that reaches nothing, which is enough to build the element.
//! struct NoNetwork;
//!
//! impl HttpClient for NoNetwork {
//!     fn get(&self, _request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
//!         Err("no network here".to_owned())
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
//!                     "BuilderName": "AgentSignatureElement",
//!                     "BuildParameters": { "WaitBudget": "00:00:00.5", "CacheSize": 50 }
//!                 }
//!             ]
//!         }
//!     }"#,
//! )?;
//! let client: Arc<dyn HttpClient> = Arc::new(NoNetwork);
//! let pipeline = PipelineBuilder::new()
//!     .add_resource(Arc::new(client))
//!     .build_from_configuration(&registry, &options)?;
//!
//! let element = pipeline
//!     .resource::<AgentSignatureElement>()
//!     .expect("the agent signature element");
//! assert_eq!(element.wait_budget(), Duration::from_millis(500));
//! assert_eq!(element.cache_size(), 50);
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

use std::sync::Arc;
use std::time::Duration;

use fiftyone_pipeline_core::configuration::{
    BuildContext, BuilderDefinition, BuilderRegistry, TimeSpan,
};
use fiftyone_pipeline_core::{Error, FlowElement, Result};

use crate::constants::MAXIMUM_PERIOD;
use crate::{AgentSignatureElement, AgentSignatureElementBuilder, HttpClient};

/// .NET's message for a cache size below one.
const CACHE_SIZE_TOO_SMALL: &str = "The cache must be allowed to hold at least one key directory.";

/// .NET's message for a response limit below one byte.
const MAX_RESPONSE_BYTES_TOO_SMALL: &str =
    "The response limit must allow at least one byte to be read.";

/// Add the agent signature element's builder to `registry`, as
/// `AgentSignatureElementBuilder`. Returns `registry` for chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry.add(agent_signature_element_builder())
}

/// The settings in the order .NET's `GetMethods` gives the builder's
/// methods.
fn agent_signature_element_builder() -> BuilderDefinition<AgentSignatureElementBuilder> {
    BuilderDefinition::new(
        "AgentSignatureElementBuilder",
        |builder: AgentSignatureElementBuilder, context: &mut BuildContext| {
            let element = Arc::new(builder.build()?);
            context.add_resource(Arc::clone(&element) as _);
            Ok(element as Arc<dyn FlowElement>)
        },
    )
    .code_only_setting("SetHttpClient", "HttpClient")
    .try_setting(
        "SetCacheSize",
        |builder: AgentSignatureElementBuilder, size| {
            at_least_one(size, "size", CACHE_SIZE_TOO_SMALL)
                .map(|size| builder.set_cache_size(size))
        },
    )
    .try_setting(
        "SetCacheLifetime",
        |builder: AgentSignatureElementBuilder, lifetime| {
            period(lifetime, "lifetime").map(|lifetime| builder.set_cache_lifetime(lifetime))
        },
    )
    .try_setting(
        "SetNegativeCacheLifetime",
        |builder: AgentSignatureElementBuilder, lifetime| {
            period(lifetime, "lifetime")
                .map(|lifetime| builder.set_negative_cache_lifetime(lifetime))
        },
    )
    .try_setting(
        "SetWaitBudget",
        |builder: AgentSignatureElementBuilder, budget| {
            period(budget, "waitBudget").map(|budget| builder.set_wait_budget(budget))
        },
    )
    .try_setting(
        "SetFetchTimeout",
        |builder: AgentSignatureElementBuilder, timeout| {
            period(timeout, "fetchTimeout").map(|timeout| builder.set_fetch_timeout(timeout))
        },
    )
    .try_setting(
        "SetClockSkew",
        |builder: AgentSignatureElementBuilder, skew| {
            period(skew, "clockSkew").map(|skew| builder.set_clock_skew(skew))
        },
    )
    .try_setting(
        "SetMaxLifetime",
        |builder: AgentSignatureElementBuilder, lifetime| {
            period(lifetime, "maxLifetime").map(|lifetime| builder.set_max_lifetime(lifetime))
        },
    )
    .setting(
        "SetRegistry",
        |builder: AgentSignatureElementBuilder, url: String| builder.set_registry(url),
    )
    .setting(
        "SetTrustForwardedEvidence",
        |builder: AgentSignatureElementBuilder, trust: bool| {
            builder.set_trust_forwarded_evidence(trust)
        },
    )
    .setting(
        "SetReachabilityCheckUrl",
        |builder: AgentSignatureElementBuilder, url: String| {
            builder.set_reachability_check_url(url)
        },
    )
    .setting(
        "SetAllowLegacySignatureAgent",
        |builder: AgentSignatureElementBuilder, allow: bool| {
            builder.set_allow_legacy_signature_agent(allow)
        },
    )
    .setting(
        "SetAllowInlineDirectory",
        |builder: AgentSignatureElementBuilder, allow: bool| {
            builder.set_allow_inline_directory(allow)
        },
    )
    .try_setting(
        "SetMaxResponseBytes",
        |builder: AgentSignatureElementBuilder, bytes| {
            at_least_one(bytes, "bytes", MAX_RESPONSE_BYTES_TOO_SMALL)
                .map(|bytes| builder.set_max_response_bytes(bytes))
        },
    )
    .build_method(Vec::new(), |_, context| {
        let builder = AgentSignatureElement::builder();
        Ok(match context.latest_resource::<Arc<dyn HttpClient>>() {
            Some(client) => builder.set_http_client(Arc::clone(client.as_ref())),
            None => builder,
        })
    })
}

/// A count of at least one, refused below that with `message` as .NET's
/// `ArgumentOutOfRangeException` gives it for the parameter `parameter`.
fn at_least_one(value: i32, parameter: &str, message: &str) -> Result<usize> {
    match usize::try_from(value) {
        Ok(count) if count >= 1 => Ok(count),
        _ => Err(out_of_range(message, parameter, value)),
    }
}

/// A period from zero to 365 days, refused outside that as .NET's
/// `CheckPeriod` refuses it for the parameter `parameter`.
fn period(value: TimeSpan, parameter: &str) -> Result<Duration> {
    match value.to_duration() {
        Some(duration) if duration <= MAXIMUM_PERIOD => Ok(duration),
        _ => {
            let maximum =
                TimeSpan::from_duration(MAXIMUM_PERIOD).expect("365 days fits in a TimeSpan");
            let message =
                format!("The period must not be negative and must not be longer than {maximum}.");
            Err(out_of_range(&message, parameter, value))
        }
    }
}

/// The message of .NET's `ArgumentOutOfRangeException`, which names the
/// parameter and the value refused.
fn out_of_range(message: &str, parameter: &str, value: impl std::fmt::Display) -> Error {
    Error::configuration(format!(
        "{message} (Parameter '{parameter}')\nActual value was {value}."
    ))
}
