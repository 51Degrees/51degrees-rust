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

//! This crate's element builders, found by the names .NET gives them, so
//! that a configuration file written for .NET makes the same elements here.
//! See [`fiftyone_pipeline_core::configuration`].
//!
//! [`add_builders`] adds `SequenceElementBuilder`, `SetHeadersElementBuilder`
//! and `ShareUsageBuilder`. The first two have no settings. The usage
//! sharing builder takes .NET's settings, read and checked as .NET reads
//! and checks them:
//!
//! - `SetIncludedQueryStringParameters` and `SetIncludedQueryStringParameter`
//!   add to the query string parameters that are shared, and
//!   `SetShareAllQueryStringParameters` shares them all.
//! - `SetBlockedHttpHeaders` replaces the headers that are not shared, and
//!   `SetBlockedHttpHeader` adds one.
//! - `SetIgnoreFlowDataEvidenceFilter` adds `key:value` pairs separated by
//!   commas, logging a warning for an entry without a colon.
//! - `SetShareAllEvidence`, `SetSharePercentage`,
//!   `SetMinimumEntriesPerMessage`, `SetShareUsageUrl` and
//!   `SetRepeatEvidenceIntervalMinutes`.
//! - `SetMaximumQueueSize`, where 0 or no value gives 1000 or ten times the
//!   minimum entries per message, whichever is larger.
//! - `SetAddTimeout` and `SetTakeTimeout` in milliseconds, where -1 waits
//!   with no limit.
//! - `SetTrackSession` compares the session cookie that
//!   `SetAspSessionCookieName` names, which has no effect unless track
//!   session is on. Track session with no name is refused unless all
//!   evidence is shared, as there is no session cookie to default to.
//! - `SetFailuresToEnterRecovery`, `SetFailuresWindowSeconds`,
//!   `SetInitialRecoveryDelaySeconds`, `SetMaxRecoveryDelaySeconds` and
//!   `SetRecoveryMultiplier` are read and have no effect, with a warning
//!   logged, as the element here has no recovery settings.
//!
//! The pipeline holds the usage sharing element as a resource, so what a
//! configuration set can be read back with [`ShareUsageElement::config`].
//!
//! [`use_fiftyone_pipeline_builder`] makes pipelines as .NET's
//! `FiftyOnePipelineBuilder` does, adding the Sequence element and usage
//! sharing that a configuration leaves out, with the pipeline setting
//! `SetShareUsage` to turn usage sharing off.
//!
//! ```
//! use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
//! use fiftyone_pipeline_core::PipelineBuilder;
//! use fiftyone_pipeline_engines_fiftyone::{configuration, ShareUsageElement};
//!
//! let mut registry = BuilderRegistry::new();
//! configuration::add_builders(&mut registry);
//! let options = PipelineOptions::from_json(
//!     r#"{
//!         "PipelineOptions": {
//!             "Elements": [
//!                 { "BuilderName": "SequenceElementBuilder" },
//!                 {
//!                     "BuilderName": "ShareUsageBuilder",
//!                     "BuildParameters": {
//!                         "SharePercentage": 0.25,
//!                         "IgnoreFlowDataEvidenceFilter": "header.user-agent:Bot"
//!                     }
//!                 }
//!             ]
//!         }
//!     }"#,
//! )?;
//! let pipeline = PipelineBuilder::new().build_from_configuration(&registry, &options)?;
//!
//! let element = pipeline.resource::<ShareUsageElement>().expect("usage sharing");
//! let config = element.config();
//! assert_eq!(config.share_percentage(), 0.25);
//! assert_eq!(
//!     config.ignore_flow_data_evidence_filter(),
//!     [("header.user-agent".to_string(), "Bot".to_string())]
//! );
//! # Ok::<(), fiftyone_pipeline_core::Error>(())
//! ```

use std::sync::Arc;
use std::time::Duration;

use fiftyone_pipeline_core::configuration::{
    BuildContext, BuilderDefinition, BuilderRegistry, PipelineDefinition, PipelineElements,
};
use fiftyone_pipeline_core::{Error, FlowElement, Result};

use crate::constants::{
    SHARE_USAGE_DEFAULT_ADD_TIMEOUT_MS, SHARE_USAGE_DEFAULT_MAX_QUEUE_SIZE,
    SHARE_USAGE_DEFAULT_MIN_ENTRIES_PER_MESSAGE,
    SHARE_USAGE_DEFAULT_REPEAT_EVIDENCE_INTERVAL_MINUTES, SHARE_USAGE_DEFAULT_SHARE_PERCENTAGE,
    SHARE_USAGE_DEFAULT_TAKE_TIMEOUT_MS, SHARE_USAGE_DEFAULT_URL,
};
use crate::{SequenceElement, SetHeadersElement, ShareUsageConfig, ShareUsageElement};

/// The timeout in milliseconds that waits with no limit, as .NET's
/// `Timeout.Infinite` does.
const WAIT_WITH_NO_LIMIT_MILLISECONDS: i32 = -1;

/// A maximum queue size of 0 means no size was set, as in .NET.
const QUEUE_SIZE_NOT_SET: i32 = 0;

/// With no queue size set, the queue holds this many times the minimum
/// entries per message, if that is more than the default.
const QUEUE_SIZE_ENTRIES_MULTIPLE: i32 = 10;

/// The range .NET's `SetFailuresToEnterRecovery` accepts.
const FAILURES_TO_ENTER_RECOVERY_MIN: i32 = 1;
const FAILURES_TO_ENTER_RECOVERY_MAX: i32 = 100;

const SECONDS_PER_MINUTE: u64 = 60;

/// The setting whose entries are checked as they are read, as .NET names it
/// in its warning.
const IGNORE_FILTER_SETTING: &str = "IgnoreFlowDataEvidenceFilter";

/// Usage sharing is on unless a configuration turns it off, as in .NET.
const SHARE_USAGE_ENABLED_BY_DEFAULT: bool = true;

/// Add this crate's element builders to `registry`, being
/// `SequenceElementBuilder`, `SetHeadersElementBuilder` and
/// `ShareUsageBuilder`. Returns `registry` for chaining.
pub fn add_builders(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    registry
        .add(sequence_element_builder())
        .add(set_headers_element_builder())
        .add(share_usage_builder())
}

/// Make pipelines from `registry` as .NET's `FiftyOnePipelineBuilder` makes
/// them, so usage sharing is on unless the configuration turns it off.
/// Returns `registry` for chaining.
///
/// The pipeline setting `SetShareUsage` (a boolean, true by default) is
/// added to the pipeline's own. Before the pipeline is built, a
/// [`SequenceElement`] is added to run first unless one is in the
/// configuration on its own, and while usage sharing is on, a
/// [`ShareUsageElement`] with the default settings is added to run last
/// unless one is in the configuration on its own. An element in a parallel
/// group of two or more is not counted, as .NET does not count one. The
/// pipeline holds an element added this way as a resource, as it does one
/// the configuration names.
///
/// The element sends usage only when this crate's `share-usage-transport`
/// feature is on, which it is not by default (see [`ShareUsageElement`]).
///
/// ```
/// use fiftyone_pipeline_core::configuration::{BuilderRegistry, PipelineOptions};
/// use fiftyone_pipeline_core::PipelineBuilder;
/// use fiftyone_pipeline_engines_fiftyone::configuration;
///
/// let mut registry = BuilderRegistry::new();
/// configuration::add_builders(&mut registry);
/// configuration::use_fiftyone_pipeline_builder(&mut registry);
/// let options = PipelineOptions::from_json(
///     r#"{
///         "PipelineOptions": {
///             "Elements": [ { "BuilderName": "SetHeadersElementBuilder" } ],
///             "BuildParameters": { "SetShareUsage": false }
///         }
///     }"#,
/// )?;
/// let pipeline = PipelineBuilder::new().build_from_configuration(&registry, &options)?;
///
/// let keys: Vec<&str> = pipeline.flow_elements().iter().map(|e| e.data_key()).collect();
/// assert_eq!(keys, ["sequence", "set-headers"]);
/// # Ok::<(), fiftyone_pipeline_core::Error>(())
/// ```
pub fn use_fiftyone_pipeline_builder(registry: &mut BuilderRegistry) -> &mut BuilderRegistry {
    use_fiftyone_pipeline_builder_with_share_usage(registry, SHARE_USAGE_ENABLED_BY_DEFAULT)
}

/// As [`use_fiftyone_pipeline_builder`], with `share_usage` as the value
/// of `SetShareUsage` when the configuration does not give it. A crate
/// whose own pipeline builders leave usage sharing off by default uses
/// this so that pipelines made from a configuration do the same. Returns
/// `registry` for chaining.
pub fn use_fiftyone_pipeline_builder_with_share_usage(
    registry: &mut BuilderRegistry,
    share_usage: bool,
) -> &mut BuilderRegistry {
    registry.set_pipeline(fiftyone_pipeline_builder(share_usage))
}

/// The setting .NET's `FiftyOnePipelineBuilder` adds to the pipeline's own.
struct FiftyOnePipelineBuilder {
    share_usage: bool,
}

fn fiftyone_pipeline_builder(share_usage: bool) -> PipelineDefinition<FiftyOnePipelineBuilder> {
    PipelineDefinition::new(
        move || FiftyOnePipelineBuilder { share_usage },
        |builder: FiftyOnePipelineBuilder,
         elements: &mut PipelineElements,
         context: &mut BuildContext| {
            if !elements.contains::<SequenceElement>() {
                elements.insert_first(Arc::new(SequenceElement::new()));
            }
            if builder.share_usage && !elements.contains::<ShareUsageElement>() {
                elements.push(share_usage_element(ShareUsageBuilder::new(), context)?);
            }
            Ok(())
        },
    )
    .setting(
        "SetShareUsage",
        |mut builder: FiftyOnePipelineBuilder, enabled: bool| {
            builder.share_usage = enabled;
            builder
        },
    )
}

/// Makes the [`SequenceElement`]. It has no settings.
struct SequenceElementBuilder;

fn sequence_element_builder() -> BuilderDefinition<SequenceElementBuilder> {
    BuilderDefinition::new(
        "SequenceElementBuilder",
        |_: SequenceElementBuilder, _: &mut BuildContext| {
            Ok(Arc::new(SequenceElement::new()) as Arc<dyn FlowElement>)
        },
    )
    .build_method(Vec::new(), |_, _| Ok(SequenceElementBuilder))
}

/// Makes the [`SetHeadersElement`]. It has no settings.
struct SetHeadersElementBuilder;

fn set_headers_element_builder() -> BuilderDefinition<SetHeadersElementBuilder> {
    BuilderDefinition::new(
        "SetHeadersElementBuilder",
        |_: SetHeadersElementBuilder, _: &mut BuildContext| {
            Ok(Arc::new(SetHeadersElement::new()) as Arc<dyn FlowElement>)
        },
    )
    .build_method(Vec::new(), |_, _| Ok(SetHeadersElementBuilder))
}

/// The usage sharing settings, held as .NET's `ShareUsageBuilder` holds
/// them until the element is made, with its defaults and in its types.
struct ShareUsageBuilder {
    included_query_string_parameters: Option<Vec<String>>,
    share_all_evidence: bool,
    blocked_http_headers: Vec<String>,
    ignore_flow_data_evidence_filter: Vec<(String, String)>,
    share_percentage: f64,
    minimum_entries_per_message: i32,
    maximum_queue_size: i32,
    add_timeout: Duration,
    take_timeout: Duration,
    share_usage_url: String,
    repeat_evidence_interval: Duration,
    track_session: bool,
    session_cookie_name: String,
}

impl ShareUsageBuilder {
    fn new() -> Self {
        ShareUsageBuilder {
            included_query_string_parameters: Some(Vec::new()),
            share_all_evidence: false,
            blocked_http_headers: Vec::new(),
            ignore_flow_data_evidence_filter: Vec::new(),
            share_percentage: SHARE_USAGE_DEFAULT_SHARE_PERCENTAGE,
            minimum_entries_per_message: SHARE_USAGE_DEFAULT_MIN_ENTRIES_PER_MESSAGE as i32,
            maximum_queue_size: QUEUE_SIZE_NOT_SET,
            add_timeout: Duration::from_millis(SHARE_USAGE_DEFAULT_ADD_TIMEOUT_MS),
            take_timeout: Duration::from_millis(SHARE_USAGE_DEFAULT_TAKE_TIMEOUT_MS),
            share_usage_url: SHARE_USAGE_DEFAULT_URL.to_string(),
            repeat_evidence_interval: Duration::from_secs(
                SHARE_USAGE_DEFAULT_REPEAT_EVIDENCE_INTERVAL_MINUTES * SECONDS_PER_MINUTE,
            ),
            track_session: false,
            session_cookie_name: String::new(),
        }
    }

    /// Add query string parameters to share. Once every one is shared,
    /// adding names changes nothing.
    fn include(mut self, names: impl IntoIterator<Item = String>) -> Self {
        if let Some(included) = self.included_query_string_parameters.as_mut() {
            included.extend(names);
        }
        self
    }

    /// The configuration for the element, with the queue size .NET gives
    /// and refusing the sizes .NET's element refuses. Track session with no
    /// session cookie named is refused too, unless all evidence is shared,
    /// as there is then no session to compare.
    fn config(self) -> Result<ShareUsageConfig> {
        let session_cookie_name = self.session_cookie_name.trim();
        if self.track_session && session_cookie_name.is_empty() && !self.share_all_evidence {
            return Err(Error::configuration(
                "Usage sharing's 'SetTrackSession' is on but 'SetAspSessionCookieName' names no \
                 session cookie, so there is no session to compare. Set \
                 'SetAspSessionCookieName' to the name of the cookie that holds the session id, \
                 or turn 'SetTrackSession' off.",
            ));
        }
        let session_cookie_name = if self.track_session {
            session_cookie_name.to_owned()
        } else {
            String::new()
        };
        let minimum = self.minimum_entries_per_message;
        let maximum = if self.maximum_queue_size == QUEUE_SIZE_NOT_SET {
            // .NET multiplies in 32 bits without checking, so a large
            // minimum wraps and is then refused below.
            (SHARE_USAGE_DEFAULT_MAX_QUEUE_SIZE as i32)
                .max(minimum.wrapping_mul(QUEUE_SIZE_ENTRIES_MULTIPLE))
        } else {
            self.maximum_queue_size
        };
        if minimum > maximum {
            return Err(Error::configuration(
                "The minimum entries per message cannot be larger than the maximum size of the \
                 queue",
            ));
        }
        let Ok(maximum @ 1..) = usize::try_from(maximum) else {
            return Err(Error::configuration(format!(
                "The maximum queue size must be at least 1. Received: {maximum}."
            )));
        };
        Ok(ShareUsageConfig::builder()
            .included_query_string_parameters(self.included_query_string_parameters)
            .share_all_evidence(self.share_all_evidence)
            .blocked_http_headers(self.blocked_http_headers)
            .ignore_flow_data_evidence_filter(self.ignore_flow_data_evidence_filter)
            .share_percentage(self.share_percentage)
            .minimum_entries_per_message(usize::try_from(minimum).unwrap_or(0))
            .maximum_queue_size(maximum)
            .add_timeout(self.add_timeout)
            .take_timeout(self.take_timeout)
            .share_usage_url(self.share_usage_url)
            .repeat_evidence_interval(self.repeat_evidence_interval)
            .track_session(session_cookie_name)
            .build())
    }
}

/// A timeout in milliseconds, as .NET's queue takes one.
fn timeout(setting: &str, milliseconds: i32) -> Result<Duration> {
    match u64::try_from(milliseconds) {
        Ok(milliseconds) => Ok(Duration::from_millis(milliseconds)),
        Err(_) if milliseconds == WAIT_WITH_NO_LIMIT_MILLISECONDS => Ok(Duration::MAX),
        Err(_) => Err(Error::configuration(format!(
            "The value of '{setting}' must be {WAIT_WITH_NO_LIMIT_MILLISECONDS}, to wait with \
             no limit, or a number of milliseconds from 0. Received: {milliseconds}."
        ))),
    }
}

/// `url` without the white space around it, if it starts with a scheme and
/// a colon as an absolute URI does
/// ([RFC 3986](https://www.rfc-editor.org/rfc/rfc3986) sections 3.1 and
/// 4.3). Otherwise the message .NET's `Uri` gives for the same text.
fn absolute_uri(url: &str) -> std::result::Result<&str, &'static str> {
    let url = url.trim();
    match url.split_once(':') {
        Some((scheme, _)) if !scheme.is_empty() => {
            let mut characters = scheme.chars();
            let valid = characters.next().is_some_and(|c| c.is_ascii_alphabetic())
                && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
            if valid {
                Ok(url)
            } else {
                Err("Invalid URI: The URI scheme is not valid.")
            }
        }
        _ => Err("Invalid URI: The format of the URI could not be determined."),
    }
}

/// The `key:value` pairs of `text`, separated by commas, as .NET's
/// `SetIgnoreFlowDataEvidenceFilter` reads them. Each pair and its key
/// are trimmed, and an entry without a colon is logged and left out.
fn ignore_filter_entries(text: &str) -> Vec<(String, String)> {
    if text.trim().is_empty() {
        log::warn!("Configuration for '{IGNORE_FILTER_SETTING}' is invalid.");
        return Vec::new();
    }
    text.split(',')
        .filter_map(|entry| {
            let entry = entry.trim();
            match entry.split_once(':') {
                Some((key, value)) => Some((key.trim().to_string(), value.to_string())),
                None => {
                    log::warn!("Configuration for '{IGNORE_FILTER_SETTING}' is invalid.{entry}");
                    None
                }
            }
        })
        .collect()
}

fn ignored(setting: &str) {
    log::warn!("The usage sharing setting '{setting}' has no effect here and is ignored.");
}

/// The usage sharing element `builder` describes, given to the pipeline as
/// a resource.
fn share_usage_element(
    builder: ShareUsageBuilder,
    context: &mut BuildContext,
) -> Result<Arc<dyn FlowElement>> {
    let element = Arc::new(ShareUsageElement::new(builder.config()?));
    context.add_resource(Arc::clone(&element) as _);
    Ok(element)
}

/// The usage sharing builder, with its settings in the order .NET's
/// `ShareUsageBuilder` declares them.
fn share_usage_builder() -> BuilderDefinition<ShareUsageBuilder> {
    BuilderDefinition::new(
        "ShareUsageBuilder",
        |builder: ShareUsageBuilder, context: &mut BuildContext| {
            share_usage_element(builder, context)
        },
    )
    .setting(
        "SetIncludedQueryStringParameters",
        |builder: ShareUsageBuilder, names: Vec<String>| builder.include(names),
    )
    .setting(
        "SetIncludedQueryStringParameter",
        |builder: ShareUsageBuilder, name: String| builder.include([name]),
    )
    .setting(
        "SetShareAllQueryStringParameters",
        |mut builder: ShareUsageBuilder, share_all: bool| {
            if share_all {
                builder.included_query_string_parameters = None;
            } else if builder.included_query_string_parameters.is_none() {
                builder.included_query_string_parameters = Some(Vec::new());
            }
            builder
        },
    )
    .setting(
        "SetShareAllEvidence",
        |mut builder: ShareUsageBuilder, share_all: bool| {
            builder.share_all_evidence = share_all;
            builder
        },
    )
    .setting(
        "SetBlockedHttpHeaders",
        |mut builder: ShareUsageBuilder, headers: Vec<String>| {
            builder.blocked_http_headers = headers;
            builder
        },
    )
    .setting(
        "SetBlockedHttpHeader",
        |mut builder: ShareUsageBuilder, header: String| {
            builder.blocked_http_headers.push(header);
            builder
        },
    )
    .setting(
        "SetIgnoreFlowDataEvidenceFilter",
        |mut builder: ShareUsageBuilder, filter: String| {
            builder
                .ignore_flow_data_evidence_filter
                .extend(ignore_filter_entries(&filter));
            builder
        },
    )
    .setting(
        "SetSharePercentage",
        |mut builder: ShareUsageBuilder, percentage: f64| {
            builder.share_percentage = percentage;
            builder
        },
    )
    .setting(
        "SetMinimumEntriesPerMessage",
        |mut builder: ShareUsageBuilder, minimum: i32| {
            builder.minimum_entries_per_message = minimum;
            builder
        },
    )
    .setting(
        "SetMaximumQueueSize",
        |mut builder: ShareUsageBuilder, size: i32| {
            builder.maximum_queue_size = size;
            builder
        },
    )
    .try_setting(
        "SetAddTimeout",
        |mut builder: ShareUsageBuilder, milliseconds: i32| {
            builder.add_timeout = timeout("SetAddTimeout", milliseconds)?;
            Ok(builder)
        },
    )
    .try_setting(
        "SetTakeTimeout",
        |mut builder: ShareUsageBuilder, milliseconds: i32| {
            builder.take_timeout = timeout("SetTakeTimeout", milliseconds)?;
            Ok(builder)
        },
    )
    .try_setting(
        "SetShareUsageUrl",
        |mut builder: ShareUsageBuilder, url: String| {
            builder.share_usage_url = absolute_uri(&url)
                .map_err(Error::configuration)?
                .to_string();
            Ok(builder)
        },
    )
    .setting(
        "SetAspSessionCookieName",
        |mut builder: ShareUsageBuilder, name: String| {
            builder.session_cookie_name = name;
            builder
        },
    )
    .try_setting(
        "SetRepeatEvidenceIntervalMinutes",
        |mut builder: ShareUsageBuilder, minutes: i32| {
            let Ok(minutes) = u64::try_from(minutes) else {
                return Err(Error::configuration(format!(
                    "The value of 'SetRepeatEvidenceIntervalMinutes' must be a number of \
                     minutes from 0. Received: {minutes}."
                )));
            };
            builder.repeat_evidence_interval = Duration::from_secs(minutes * SECONDS_PER_MINUTE);
            Ok(builder)
        },
    )
    .setting(
        "SetTrackSession",
        |mut builder: ShareUsageBuilder, track: bool| {
            builder.track_session = track;
            builder
        },
    )
    .try_setting(
        "SetFailuresToEnterRecovery",
        |builder: ShareUsageBuilder, failures: i32| {
            if !(FAILURES_TO_ENTER_RECOVERY_MIN..=FAILURES_TO_ENTER_RECOVERY_MAX)
                .contains(&failures)
            {
                return Err(Error::configuration(format!(
                    "failuresToEnterRecovery must be within {FAILURES_TO_ENTER_RECOVERY_MIN} and \
                     {FAILURES_TO_ENTER_RECOVERY_MAX} (both inclusive). Received: {failures}. \
                     (Parameter 'failuresToEnterRecovery')"
                )));
            }
            ignored("SetFailuresToEnterRecovery");
            Ok(builder)
        },
    )
    .setting(
        "SetFailuresWindowSeconds",
        |builder: ShareUsageBuilder, _: i32| {
            ignored("SetFailuresWindowSeconds");
            builder
        },
    )
    .setting(
        "SetInitialRecoveryDelaySeconds",
        |builder: ShareUsageBuilder, _: f64| {
            ignored("SetInitialRecoveryDelaySeconds");
            builder
        },
    )
    .setting(
        "SetMaxRecoveryDelaySeconds",
        |builder: ShareUsageBuilder, _: f64| {
            ignored("SetMaxRecoveryDelaySeconds");
            builder
        },
    )
    .setting(
        "SetRecoveryMultiplier",
        |builder: ShareUsageBuilder, _: f64| {
            ignored("SetRecoveryMultiplier");
            builder
        },
    )
    .build_method(Vec::new(), |_, _| Ok(ShareUsageBuilder::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absolute_uri_starts_with_a_scheme_and_a_colon() {
        const FORMAT: &str = "Invalid URI: The format of the URI could not be determined.";
        const SCHEME: &str = "Invalid URI: The URI scheme is not valid.";
        for url in [
            "https://example.com/x",
            "HTTP://a",
            "a+b-c.d:rest",
            "mailto:x",
        ] {
            assert_eq!(absolute_uri(url), Ok(url));
        }
        assert_eq!(
            absolute_uri("\t https://example.com/t\n"),
            Ok("https://example.com/t")
        );
        for url in ["not a url", "/usage", "", "  ", ":x", "example.com/usage"] {
            assert_eq!(absolute_uri(url), Err(FORMAT), "{url}");
        }
        for url in ["1http://a", "ht tp://a", "-a:b", "a_b:c"] {
            assert_eq!(absolute_uri(url), Err(SCHEME), "{url}");
        }
    }

    #[test]
    fn a_timeout_of_minus_one_waits_with_no_limit() {
        assert_eq!(timeout("SetAddTimeout", -1).unwrap(), Duration::MAX);
        assert_eq!(timeout("SetAddTimeout", 0).unwrap(), Duration::ZERO);
        assert_eq!(
            timeout("SetAddTimeout", 7).unwrap(),
            Duration::from_millis(7)
        );
        assert_eq!(
            timeout("SetTakeTimeout", -2).unwrap_err().message(),
            "The value of 'SetTakeTimeout' must be -1, to wait with no limit, or a number of \
             milliseconds from 0. Received: -2."
        );
    }
}
