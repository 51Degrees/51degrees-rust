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

//! The Sequence, Set Headers and usage sharing elements made from a
//! configuration, compared with what the .NET pipeline 4.5.129 makes from
//! the same settings. Each expected value is what .NET's element held, read
//! by reflection, or the message .NET gave. A logger is installed once for
//! the whole process, so each test holds [`SERIAL`] and reads only its own
//! records.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use fiftyone_pipeline_core::configuration::{
    BuilderRegistry, ConfigValue, ElementOptions, PipelineOptions,
};
use fiftyone_pipeline_core::{Error, EvidenceKeyFilter, Pipeline, PipelineBuilder, Result};
use fiftyone_pipeline_engines_fiftyone::constants::SHARE_USAGE_DEFAULT_URL;
use fiftyone_pipeline_engines_fiftyone::{configuration, ShareUsageConfig, ShareUsageElement};

/// Log records at warning level or above, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            LOGGED
                .lock()
                .unwrap()
                .push(format!("{} {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}

static LOGGER: TestLogger = TestLogger;

fn start() -> MutexGuard<'static, ()> {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    LOGGED.lock().unwrap().clear();
    guard
}

fn logged() -> Vec<String> {
    LOGGED.lock().unwrap().clone()
}

const BUILDER: &str = "fiftyone_pipeline_engines_fiftyone::configuration::ShareUsageBuilder";
const NO_SUB_ELEMENTS: &str = "No SubElements";
const NO_INNER: &str = "No Inner Exception.";
const MINUTE_SECONDS: u64 = 60;

fn registry() -> BuilderRegistry {
    let mut registry = BuilderRegistry::new();
    configuration::add_builders(&mut registry);
    registry
}

fn build(options: &PipelineOptions) -> Result<Arc<Pipeline>> {
    PipelineBuilder::new().build_from_configuration(&registry(), options)
}

/// A pipeline of one usage sharing element with `parameters`, applied in
/// the order given.
fn share_usage(parameters: &[(&str, &str)]) -> Result<Arc<Pipeline>> {
    let element = parameters.iter().fold(
        ElementOptions::new("ShareUsage"),
        |element, (name, value)| element.parameter(*name, *value),
    );
    build(&PipelineOptions::new().element(element))
}

fn config(result: Result<Arc<Pipeline>>) -> ShareUsageConfig {
    share_usage_config(&result.expect("the pipeline is built"))
}

/// The configuration of each usage sharing element in `pipeline`, read from
/// the elements themselves in the order `flow_elements` lists them.
fn share_usage_configs(pipeline: &Pipeline) -> Vec<ShareUsageConfig> {
    pipeline
        .flow_elements()
        .iter()
        .filter_map(|element| Arc::clone(element).into_any())
        .filter_map(|element| element.downcast::<ShareUsageElement>().ok())
        .map(|element| element.config().clone())
        .collect()
}

/// The one usage sharing element in `pipeline`.
fn share_usage_element(pipeline: &Pipeline) -> Arc<ShareUsageElement> {
    let mut elements: Vec<Arc<ShareUsageElement>> = pipeline
        .flow_elements()
        .iter()
        .filter_map(|element| Arc::clone(element).into_any())
        .filter_map(|element| element.downcast::<ShareUsageElement>().ok())
        .collect();
    assert_eq!(elements.len(), 1, "one usage sharing element");
    elements.remove(0)
}

/// The configuration of the one usage sharing element in `pipeline`.
fn share_usage_config(pipeline: &Pipeline) -> ShareUsageConfig {
    let mut configs = share_usage_configs(pipeline);
    assert_eq!(configs.len(), 1, "one usage sharing element");
    configs.remove(0)
}

fn message(result: Result<Arc<Pipeline>>) -> String {
    match result {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

/// The message for an element that cannot be made. .NET gives a cause the
/// builder refuses as "Exception has been thrown by the target of an
/// invocation." with the cause as the inner message, and the crates give
/// the cause itself, as `fiftyone_pipeline_core`'s configuration tests
/// record.
fn failed(builder: &str, message: &str) -> String {
    format!(
        "Configuration failed to build.\nBuilderName: {builder}\nSubElement Builder Names: \
         {NO_SUB_ELEMENTS}\n{message}\n{NO_INNER}\n"
    )
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
    values
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn ignored(setting: &str) -> String {
    format!("WARN The usage sharing setting '{setting}' has no effect here and is ignored.")
}

#[test]
fn the_builders_are_found_by_the_names_dotnet_finds_them_by() {
    let _serial = start();
    for (name, key) in [
        ("SequenceElementBuilder", "sequence"),
        ("SequenceElement", "sequence"),
        ("sequenceelement", "sequence"),
        ("SetHeadersElementBuilder", "set-headers"),
        ("setheaderselement", "set-headers"),
        ("ShareUsageBuilder", "shareusage"),
        ("shareusage", "shareusage"),
    ] {
        let pipeline = build(&PipelineOptions::new().element(ElementOptions::new(name)))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let keys: Vec<&str> = pipeline
            .flow_elements()
            .iter()
            .map(|e| e.data_key())
            .collect();
        assert_eq!(keys, [key], "{name}");
    }

    // Neither .NET nor the crates take "Element" off a name.
    let options = PipelineOptions::new().element(ElementOptions::new("Sequence"));
    assert_eq!(
        message(build(&options)),
        failed(
            "Sequence",
            "Could not find builder matching 'Sequence' for element 0. Available builders are: \
             SequenceElementBuilder,SetHeadersElementBuilder,ShareUsageBuilder"
        )
    );
}

#[test]
fn the_defaults_are_those_of_dotnet() {
    let _serial = start();
    let config = config(share_usage(&[]));
    assert_eq!(config.share_percentage(), 1.0);
    assert_eq!(config.minimum_entries_per_message(), 50);
    assert_eq!(config.maximum_queue_size(), 1000);
    assert_eq!(config.add_timeout(), Duration::from_millis(5));
    assert_eq!(config.take_timeout(), Duration::from_millis(100));
    assert_eq!(config.share_usage_url(), SHARE_USAGE_DEFAULT_URL);
    assert_eq!(
        config.repeat_evidence_interval(),
        Duration::from_secs(20 * MINUTE_SECONDS)
    );
    assert!(config.blocked_http_headers().is_empty());
    assert_eq!(config.included_query_string_parameters(), Some(&[][..]));
    assert!(!config.share_all_evidence());
    assert!(config.ignore_flow_data_evidence_filter().is_empty());
    assert!(logged().is_empty(), "{:?}", logged());
}

#[test]
fn every_setting_is_read_as_dotnet_reads_it() {
    let _serial = start();
    let pipeline = share_usage(&[
        ("IncludedQueryStringParameters", "a,\"b,c\",,"),
        ("IncludedQueryStringParameter", "d"),
        ("ShareAllEvidence", "false"),
        ("BlockedHttpHeaders", "X-One,x-two"),
        ("BlockedHttpHeader", "X-Three"),
        (
            "IgnoreFlowDataEvidenceFilter",
            "header.User-Agent:Bot, query.x : 1 ,bad,  ",
        ),
        ("SharePercentage", "0.5"),
        ("MinimumEntriesPerMessage", "10"),
        ("MaximumQueueSize", "200"),
        ("AddTimeout", "7"),
        ("TakeTimeout", "300"),
        ("ShareUsageUrl", "HTTPS://Example.com/usage"),
        ("AspSessionCookieName", "MySession"),
        ("RepeatEvidenceIntervalMinutes", "30"),
        ("TrackSession", "true"),
        ("FailuresToEnterRecovery", "5"),
        ("FailuresWindowSeconds", "50"),
        ("InitialRecoveryDelaySeconds", "1.5"),
        ("MaxRecoveryDelaySeconds", "60"),
        ("RecoveryMultiplier", "3"),
    ])
    .expect("the pipeline is built");

    let config = share_usage_config(&pipeline);
    assert_eq!(
        config.included_query_string_parameters(),
        Some(&strings(&["a", "b,c", "", "d"])[..])
    );
    assert!(!config.share_all_evidence());
    assert_eq!(
        config.blocked_http_headers(),
        strings(&["X-One", "x-two", "X-Three"])
    );
    // .NET keeps the key's case. The crates hold keys in lower case and
    // match evidence in any case, as .NET does.
    assert_eq!(
        config.ignore_flow_data_evidence_filter(),
        pairs(&[("header.user-agent", "Bot"), ("query.x", " 1")])
    );
    assert_eq!(config.share_percentage(), 0.5);
    assert_eq!(config.minimum_entries_per_message(), 10);
    assert_eq!(config.maximum_queue_size(), 200);
    assert_eq!(config.add_timeout(), Duration::from_millis(7));
    assert_eq!(config.take_timeout(), Duration::from_millis(300));
    // .NET holds the address as a Uri, which writes the scheme and host in
    // lower case.
    assert_eq!(config.share_usage_url(), "HTTPS://Example.com/usage");
    assert_eq!(
        config.repeat_evidence_interval(),
        Duration::from_secs(30 * MINUTE_SECONDS)
    );

    // The element itself shares what the settings say.
    let element = share_usage_element(&pipeline);
    let filter = element.shared_evidence_filter();
    for key in [
        "query.a",
        "query.b,c",
        "query.d",
        "query.51d_x",
        "header.accept",
    ] {
        assert!(filter.include(key), "{key}");
    }
    for key in [
        "query.e",
        "header.x-one",
        "header.x-two",
        "header.x-three",
        "header.cookie",
    ] {
        assert!(!filter.include(key), "{key}");
    }

    assert_eq!(
        logged(),
        [
            "WARN Configuration for 'IgnoreFlowDataEvidenceFilter' is invalid.bad".to_string(),
            "WARN Configuration for 'IgnoreFlowDataEvidenceFilter' is invalid.".to_string(),
            ignored("SetAspSessionCookieName"),
            ignored("SetTrackSession"),
            ignored("SetFailuresToEnterRecovery"),
            ignored("SetFailuresWindowSeconds"),
            ignored("SetInitialRecoveryDelaySeconds"),
            ignored("SetMaxRecoveryDelaySeconds"),
            ignored("SetRecoveryMultiplier"),
        ]
    );
}

#[test]
fn the_pipeline_holds_the_usage_sharing_element_it_runs() {
    let _serial = start();
    let named = share_usage(&[("SharePercentage", "0.25")]).unwrap();
    let added = build_fiftyone(&set_headers()).unwrap();
    for (pipeline, percentage) in [(named, 0.25), (added, 1.0)] {
        let held = pipeline
            .resource::<ShareUsageElement>()
            .expect("the element is held");
        let running = pipeline
            .flow_elements()
            .iter()
            .find(|element| element.data_key() == "shareusage")
            .and_then(|element| Arc::clone(element).into_any())
            .and_then(|element| element.downcast::<ShareUsageElement>().ok())
            .expect("the element runs");
        assert!(Arc::ptr_eq(&held, &running));
        assert_eq!(held.config().share_percentage(), percentage);
        // The configuration is read from the element, not held beside it.
        assert!(pipeline.resource::<ShareUsageConfig>().is_none());
    }
}

#[test]
fn sharing_all_evidence_shares_every_key_but_those_never_shared() {
    let _serial = start();
    let pipeline = share_usage(&[("ShareAllEvidence", "true")]).unwrap();
    assert!(share_usage_config(&pipeline).share_all_evidence());
    let element = share_usage_element(&pipeline);
    let filter = element.shared_evidence_filter();
    for key in ["cookie.session", "query.anything"] {
        assert!(filter.include(key), "{key}");
    }
    assert!(!filter.include("header.cookie"));
}

#[test]
fn the_queue_size_follows_the_minimum_entries_as_in_dotnet() {
    let _serial = start();
    for (parameters, minimum, maximum) in [
        (&[("MinimumEntriesPerMessage", "200")][..], 200, 2000),
        (
            &[
                ("MaximumQueueSize", "0"),
                ("MinimumEntriesPerMessage", "200"),
            ][..],
            200,
            2000,
        ),
        (&[("MinimumEntriesPerMessage", "99")][..], 99, 1000),
        (&[("MinimumEntriesPerMessage", "2000")][..], 2000, 20000),
        (
            &[
                ("MaximumQueueSize", "50"),
                ("MinimumEntriesPerMessage", "50"),
            ][..],
            50,
            50,
        ),
    ] {
        let config = config(share_usage(parameters));
        assert_eq!(
            config.minimum_entries_per_message(),
            minimum,
            "{parameters:?}"
        );
        assert_eq!(config.maximum_queue_size(), maximum, "{parameters:?}");
    }
}

#[test]
fn a_queue_smaller_than_the_minimum_entries_fails_as_in_dotnet() {
    let _serial = start();
    let too_small = failed(
        "ShareUsage",
        "The minimum entries per message cannot be larger than the maximum size of the queue",
    );
    for parameters in [
        &[
            ("MinimumEntriesPerMessage", "100"),
            ("MaximumQueueSize", "50"),
        ][..],
        &[("MaximumQueueSize", "-5")][..],
        // Ten times the minimum wraps in .NET's 32 bit arithmetic, so the
        // default of 1000 is used and is too small.
        &[("MinimumEntriesPerMessage", "300000000")][..],
    ] {
        assert_eq!(
            message(share_usage(parameters)),
            too_small,
            "{parameters:?}"
        );
    }
    // .NET's queue refuses this size itself, with "boundedCapacity ('-5')
    // must be a non-negative and non-zero value."
    assert_eq!(
        message(share_usage(&[
            ("MinimumEntriesPerMessage", "-10"),
            ("MaximumQueueSize", "-5"),
        ])),
        failed(
            "ShareUsage",
            "The maximum queue size must be at least 1. Received: -5."
        )
    );
}

#[test]
fn a_minimum_entries_below_one_is_raised_to_one() {
    let _serial = start();
    // .NET keeps 0 and -5, which the crates' element does not take.
    for minimum in ["0", "-5"] {
        let config = config(share_usage(&[("MinimumEntriesPerMessage", minimum)]));
        assert_eq!(config.minimum_entries_per_message(), 1, "{minimum}");
        assert_eq!(config.maximum_queue_size(), 1000, "{minimum}");
    }
}

#[test]
fn failures_to_enter_recovery_is_checked_as_dotnet_checks_it() {
    let _serial = start();
    for failures in ["0", "101"] {
        assert_eq!(
            message(share_usage(&[("FailuresToEnterRecovery", failures)])),
            failed(
                "ShareUsage",
                &format!(
                    "failuresToEnterRecovery must be within 1 and 100 (both inclusive). \
                     Received: {failures}. (Parameter 'failuresToEnterRecovery')"
                )
            )
        );
    }
    for failures in ["1", "100"] {
        share_usage(&[("FailuresToEnterRecovery", failures)]).expect(failures);
    }
}

#[test]
fn settings_that_have_no_effect_still_refuse_what_dotnet_refuses() {
    let _serial = start();
    assert_eq!(
        message(share_usage(&[("TrackSession", "yes")])),
        failed(
            "ShareUsage",
            &format!(
                "Method 'SetTrackSession' on builder '{BUILDER}' for element 0 expects a \
                 parameter of type 'Boolean'. Failed to parse value 'yes' using the static \
                 'TryParse' method."
            )
        )
    );
    // Recovery settings other than the failure count take any number, as
    // in .NET.
    share_usage(&[
        ("FailuresWindowSeconds", "-1"),
        ("RecoveryMultiplier", "-1"),
    ])
    .expect("negative recovery settings");
}

#[test]
fn query_string_parameters_are_shared_as_dotnet_shares_them() {
    let _serial = start();
    let included = |parameters: &[(&str, &str)]| {
        config(share_usage(parameters))
            .included_query_string_parameters()
            .map(<[String]>::to_vec)
    };
    assert_eq!(
        included(&[
            ("IncludedQueryStringParameter", "x"),
            ("ShareAllQueryStringParameters", "true"),
        ]),
        None
    );
    assert_eq!(
        included(&[
            ("ShareAllQueryStringParameters", "true"),
            ("SetShareAllQueryStringParameters", "false"),
        ]),
        Some(Vec::new())
    );
    // .NET fails for both with a NullReferenceException. A configuration
    // file cannot give this order, as its names are applied sorted.
    for setting in [
        "IncludedQueryStringParameter",
        "IncludedQueryStringParameters",
    ] {
        assert_eq!(
            included(&[("ShareAllQueryStringParameters", "true"), (setting, "x")]),
            None,
            "{setting}"
        );
    }
    assert_eq!(
        included(&[("SetIncludedQueryStringParameters", "")]),
        Some(Vec::new())
    );
    // The list adds to what is there, as one parameter does.
    assert_eq!(
        included(&[
            ("IncludedQueryStringParameter", "x"),
            ("IncludedQueryStringParameters", "y,z"),
        ]),
        Some(strings(&["x", "y", "z"]))
    );

    // A list given in code is taken as it is.
    let options = PipelineOptions::new().element(
        ElementOptions::new("ShareUsage")
            .parameter("SetIncludedQueryStringParameters", vec!["p", "q,r"]),
    );
    assert_eq!(
        config(build(&options)).included_query_string_parameters(),
        Some(&strings(&["p", "q,r"])[..])
    );
}

#[test]
fn the_address_must_be_absolute_as_in_dotnet() {
    let _serial = start();
    for (url, cause) in [
        (
            "not a url",
            "Invalid URI: The format of the URI could not be determined.",
        ),
        (
            "/usage",
            "Invalid URI: The format of the URI could not be determined.",
        ),
        (
            "example.com/usage",
            "Invalid URI: The format of the URI could not be determined.",
        ),
        (
            ":x",
            "Invalid URI: The format of the URI could not be determined.",
        ),
        ("1http://a", "Invalid URI: The URI scheme is not valid."),
        ("ht tp://a", "Invalid URI: The URI scheme is not valid."),
    ] {
        assert_eq!(
            message(share_usage(&[("ShareUsageUrl", url)])),
            failed("ShareUsage", cause),
            "{url}"
        );
    }
    for (url, held) in [
        (" https://example.com/x ", "https://example.com/x"),
        ("\thttps://example.com/t\n", "https://example.com/t"),
        ("mailto:x", "mailto:x"),
    ] {
        assert_eq!(
            config(share_usage(&[("ShareUsageUrl", url)])).share_usage_url(),
            held
        );
    }
}

#[test]
fn the_ignore_filter_is_read_as_dotnet_reads_it() {
    let _serial = start();
    for text in ["", "   "] {
        LOGGED.lock().unwrap().clear();
        let config = config(share_usage(&[("IgnoreFlowDataEvidenceFilter", text)]));
        assert!(config.ignore_flow_data_evidence_filter().is_empty());
        assert_eq!(
            logged(),
            ["WARN Configuration for 'IgnoreFlowDataEvidenceFilter' is invalid."]
        );
    }
    assert_eq!(
        config(share_usage(&[(
            "IgnoreFlowDataEvidenceFilter",
            "a:b:c,:x,y:"
        )]))
        .ignore_flow_data_evidence_filter(),
        pairs(&[("a", "b:c"), ("", "x"), ("y", "")])
    );
    assert_eq!(
        config(share_usage(&[
            ("IgnoreFlowDataEvidenceFilter", "a:1"),
            ("SetIgnoreFlowDataEvidenceFilter", "b:2"),
        ]))
        .ignore_flow_data_evidence_filter(),
        pairs(&[("a", "1"), ("b", "2")])
    );
}

#[test]
fn blocked_headers_are_replaced_or_added_as_in_dotnet() {
    let _serial = start();
    assert_eq!(
        config(share_usage(&[
            ("BlockedHttpHeader", "a"),
            ("BlockedHttpHeaders", "b"),
        ]))
        .blocked_http_headers(),
        strings(&["b"])
    );
    assert_eq!(
        config(share_usage(&[
            ("BlockedHttpHeaders", "b,Cookie"),
            ("BlockedHttpHeader", "a"),
        ]))
        .blocked_http_headers(),
        strings(&["b", "Cookie", "a"])
    );
}

#[test]
fn the_share_percentage_is_read_as_a_double() {
    let _serial = start();
    // .NET keeps 2 and -1, which share every request and none, as the
    // crates' 1 and 0 do.
    for (percentage, held) in [("2", 1.0), ("-1", 0.0), ("0.25", 0.25)] {
        assert_eq!(
            config(share_usage(&[("SharePercentage", percentage)])).share_percentage(),
            held
        );
    }
    assert_eq!(
        message(share_usage(&[("SharePercentage", "half")])),
        failed(
            "ShareUsage",
            &format!(
                "Method 'SetSharePercentage' on builder '{BUILDER}' for element 0 expects a \
                 parameter of type 'Double'. Failed to parse value 'half' using the static \
                 'TryParse' method."
            )
        )
    );
}

#[test]
fn a_timeout_of_minus_one_waits_with_no_limit() {
    let _serial = start();
    let config = config(share_usage(&[("AddTimeout", "-1"), ("TakeTimeout", "-1")]));
    assert_eq!(config.add_timeout(), Duration::MAX);
    assert_eq!(config.take_timeout(), Duration::MAX);

    // .NET keeps these, and its queue then refuses each one when it is used.
    assert_eq!(
        message(share_usage(&[("AddTimeout", "-2")])),
        failed(
            "ShareUsage",
            "The value of 'SetAddTimeout' must be -1, to wait with no limit, or a number of \
             milliseconds from 0. Received: -2."
        )
    );
    assert_eq!(
        message(share_usage(&[("RepeatEvidenceIntervalMinutes", "-1")])),
        failed(
            "ShareUsage",
            "The value of 'SetRepeatEvidenceIntervalMinutes' must be a number of minutes from \
             0. Received: -1."
        )
    );
}

#[test]
fn an_unknown_setting_lists_the_settings_in_dotnets_order() {
    let _serial = start();
    let options = PipelineOptions::new()
        .element(ElementOptions::new("ShareUsageBuilder").parameter("Unknown", "1"));
    assert_eq!(
        message(build(&options)),
        format!(
            "Configuration failed to build.\nBuilderName: ShareUsageBuilder\nSubElement \
             Builder Names: No SubElements\nBuilder '{BUILDER}' for element 0 has no 'Set' \
             methods or 'Build' methods that match (case-insensitively) the following \
             parameters: UNKNOWN. The available configuration methods on this builder are: \n\
             SetIncludedQueryStringParameters (List`1)\n\
             SetIncludedQueryStringParameter (String)\n\
             SetShareAllQueryStringParameters (Boolean)\n\
             SetShareAllEvidence (Boolean)\n\
             SetBlockedHttpHeaders (List`1)\n\
             SetBlockedHttpHeader (String)\n\
             SetIgnoreFlowDataEvidenceFilter (String)\n\
             SetSharePercentage (Double)\n\
             SetMinimumEntriesPerMessage (Int32)\n\
             SetMaximumQueueSize (Int32)\n\
             SetAddTimeout (Int32)\n\
             SetTakeTimeout (Int32)\n\
             SetShareUsageUrl (String)\n\
             SetAspSessionCookieName (String)\n\
             SetRepeatEvidenceIntervalMinutes (Int32)\n\
             SetTrackSession (Boolean)\n\
             SetFailuresToEnterRecovery (Int32)\n\
             SetFailuresWindowSeconds (Int32)\n\
             SetInitialRecoveryDelaySeconds (Double)\n\
             SetMaxRecoveryDelaySeconds (Double)\n\
             SetRecoveryMultiplier (Double)\n\
             The available 'Build' methods have the following signatures: \n\n\n{NO_INNER}\n"
        )
    );

    for (name, builder) in [
        ("SequenceElement", "SequenceElementBuilder"),
        ("SetHeadersElementBuilder", "SetHeadersElementBuilder"),
    ] {
        let options =
            PipelineOptions::new().element(ElementOptions::new(name).parameter("Unknown", "1"));
        assert_eq!(
            message(build(&options)),
            format!(
                "Configuration failed to build.\nBuilderName: {name}\nSubElement Builder \
                 Names: No SubElements\nBuilder \
                 'fiftyone_pipeline_engines_fiftyone::configuration::{builder}' for element 0 \
                 has no 'Set' methods or 'Build' methods that match (case-insensitively) the \
                 following parameters: UNKNOWN. The available configuration methods on this \
                 builder are: \nThe available 'Build' methods have the following signatures: \
                 \n\n\n{NO_INNER}\n"
            )
        );
    }
}

#[test]
fn a_json_file_written_for_dotnet_makes_the_same_elements() {
    let _serial = start();
    let options = PipelineOptions::from_json(
        r#"{
          "PipelineOptions": {
            "Elements": [
              { "BuilderName": "SequenceElementBuilder" },
              {
                "BuilderName": "ShareUsageBuilder",
                "BuildParameters": {
                  // Written out of order, as a person might.
                  "TrackSession": true,
                  "SetShareAllQueryStringParameters": true,
                  "IncludedQueryStringParameter": "x",
                  "SharePercentage": 0.25,
                  "MinimumEntriesPerMessage": 200,
                  "IgnoreFlowDataEvidenceFilter": "header.user-agent:Bot",
                  "BlockedHttpHeaders": "a,b",
                  "BlockedHttpHeader": "c"
                }
              },
              { "BuilderName": "SetHeadersElementBuilder" }
            ]
          }
        }"#,
    )
    .unwrap();
    let pipeline = build(&options).unwrap();
    let keys: Vec<&str> = pipeline
        .flow_elements()
        .iter()
        .map(|e| e.data_key())
        .collect();
    assert_eq!(keys, ["sequence", "shareusage", "set-headers"]);

    // .NET applies the names sorted, so the header is added before the
    // list replaces it, and the parameter before every one is shared.
    let config = share_usage_config(&pipeline);
    assert_eq!(config.blocked_http_headers(), strings(&["a", "b"]));
    assert_eq!(config.included_query_string_parameters(), None);
    assert_eq!(
        config.ignore_flow_data_evidence_filter(),
        pairs(&[("header.user-agent", "Bot")])
    );
    assert_eq!(config.share_percentage(), 0.25);
    assert_eq!(config.minimum_entries_per_message(), 200);
    assert_eq!(config.maximum_queue_size(), 2000);
    assert_eq!(logged(), [ignored("SetTrackSession")]);
}

// The pipeline .NET's `FiftyOnePipelineBuilder` makes from a configuration.
// .NET 4.5.129 lists a parallel group's elements after the others in
// `FlowElements`, as `Pipeline::flow_elements` does, so each expected list
// below is in that order.

const FIFTYONE_PIPELINE: &str =
    "fiftyone_pipeline_engines_fiftyone::configuration::FiftyOnePipelineBuilder";

fn build_fiftyone(options: &PipelineOptions) -> Result<Arc<Pipeline>> {
    let mut registry = registry();
    configuration::use_fiftyone_pipeline_builder(&mut registry);
    PipelineBuilder::new().build_from_configuration(&registry, options)
}

fn data_keys(pipeline: &Pipeline) -> Vec<&str> {
    pipeline
        .flow_elements()
        .iter()
        .map(|e| e.data_key())
        .collect()
}

fn set_headers() -> PipelineOptions {
    PipelineOptions::new().element(ElementOptions::new("SetHeadersElementBuilder"))
}

#[test]
fn the_fiftyone_pipeline_adds_the_sequence_first_and_usage_sharing_last() {
    let _serial = start();
    let pipeline = build_fiftyone(&set_headers()).unwrap();
    assert_eq!(
        data_keys(&pipeline),
        ["sequence", "set-headers", "shareusage"]
    );

    // The usage sharing element added has the settings a usage sharing
    // builder given none has, which are .NET's.
    let added = share_usage_config(&pipeline);
    let configured = config(share_usage(&[]));
    assert_eq!(format!("{added:?}"), format!("{configured:?}"));
    assert_eq!(added.share_percentage(), 1.0);
    assert_eq!(added.minimum_entries_per_message(), 50);
    assert_eq!(added.share_usage_url(), SHARE_USAGE_DEFAULT_URL);
    assert!(logged().is_empty(), "{:?}", logged());

    // A registry without it makes the pipeline .NET's `PipelineBuilder`
    // makes, adding nothing.
    assert_eq!(data_keys(&build(&set_headers()).unwrap()), ["set-headers"]);
}

#[test]
fn share_usage_false_leaves_usage_sharing_out() {
    let _serial = start();
    for (name, value, expected) in [
        ("SetShareUsage", "false", &["sequence", "set-headers"][..]),
        ("ShareUsage", "False", &["sequence", "set-headers"]),
        (
            "shareusage",
            "true",
            &["sequence", "set-headers", "shareusage"],
        ),
        (
            "SetShareUsage",
            " true ",
            &["sequence", "set-headers", "shareusage"],
        ),
    ] {
        let pipeline = build_fiftyone(&set_headers().parameter(name, value)).unwrap();
        assert_eq!(data_keys(&pipeline), expected, "{name} {value}");
    }
}

#[test]
fn share_usage_has_the_default_the_registry_was_given() {
    let _serial = start();
    let build_with = |share_usage: bool, options: &PipelineOptions| {
        let mut registry = registry();
        configuration::use_fiftyone_pipeline_builder_with_share_usage(&mut registry, share_usage);
        PipelineBuilder::new()
            .build_from_configuration(&registry, options)
            .unwrap()
    };
    let on = set_headers().parameter("SetShareUsage", "true");
    let off = set_headers().parameter("SetShareUsage", "false");
    assert_eq!(
        data_keys(&build_with(false, &set_headers())),
        ["sequence", "set-headers"]
    );
    assert_eq!(
        data_keys(&build_with(false, &on)),
        ["sequence", "set-headers", "shareusage"]
    );
    assert_eq!(
        data_keys(&build_with(true, &set_headers())),
        ["sequence", "set-headers", "shareusage"]
    );
    assert_eq!(
        data_keys(&build_with(true, &off)),
        ["sequence", "set-headers"]
    );
}

#[test]
fn share_usage_that_is_not_a_boolean_fails() {
    let _serial = start();
    for (value, ending) in [
        (
            ConfigValue::from("yes"),
            ". Failed to parse value 'yes' using the static 'TryParse' method.",
        ),
        (
            ConfigValue::from(""),
            ". Failed to parse value '' using the static 'TryParse' method.",
        ),
        // A list given in code is named String[] here, where .NET names a
        // `List<string>` given in code List`1.
        (
            ConfigValue::from(vec!["true"]),
            " but supplied object is of type 'String[]'.",
        ),
    ] {
        let options = set_headers().parameter("SetShareUsage", value);
        assert_eq!(
            message(build_fiftyone(&options)),
            format!(
                "Method 'SetShareUsage' on builder '{FIFTYONE_PIPELINE}' for pipeline expects a \
                 parameter of type 'Boolean'{ending}"
            )
        );
    }
}

#[test]
fn share_usage_is_not_a_setting_of_the_core_pipeline() {
    let _serial = start();
    for value in ["false", "yes"] {
        let pipeline = build(&set_headers().parameter("SetShareUsage", value)).unwrap();
        assert_eq!(data_keys(&pipeline), ["set-headers"], "{value}");
    }
}

#[test]
fn a_sequence_or_usage_sharing_element_on_its_own_is_kept_where_it_is() {
    let _serial = start();
    let element = ElementOptions::new;
    let share_quarter = || element("ShareUsageBuilder").parameter("SharePercentage", "0.25");
    for (options, expected) in [
        (
            set_headers().element(element("SequenceElementBuilder")),
            &["set-headers", "sequence", "shareusage"][..],
        ),
        (
            set_headers().element(element("SequenceElement")),
            &["set-headers", "sequence", "shareusage"],
        ),
        (
            PipelineOptions::new().element(share_quarter()),
            &["sequence", "shareusage"],
        ),
        (
            PipelineOptions::new()
                .element(share_quarter())
                .parameter("SetShareUsage", "false"),
            &["sequence", "shareusage"],
        ),
        (
            PipelineOptions::new()
                .element(element("SequenceElementBuilder"))
                .element(element("SetHeadersElementBuilder"))
                .element(element("ShareUsageBuilder")),
            &["sequence", "set-headers", "shareusage"],
        ),
        (
            PipelineOptions::new()
                .element(element("ShareUsageBuilder"))
                .element(element("SequenceElementBuilder")),
            &["shareusage", "sequence"],
        ),
    ] {
        let pipeline = build_fiftyone(&options).unwrap();
        assert_eq!(data_keys(&pipeline), expected, "{options:?}");
    }

    // The usage sharing element the configuration names keeps its settings.
    let pipeline = build_fiftyone(&PipelineOptions::new().element(share_quarter())).unwrap();
    let config = share_usage_config(&pipeline);
    assert_eq!(config.share_percentage(), 0.25);
}

#[test]
fn an_element_in_a_parallel_group_is_not_counted() {
    let _serial = start();
    let group = |first: &str, second: &str| {
        PipelineOptions::new().element(ElementOptions::group([
            ElementOptions::new(first),
            ElementOptions::new(second),
        ]))
    };

    // .NET adds a second usage sharing element, and so do the crates.
    let pipeline = build_fiftyone(&group("ShareUsageBuilder", "SetHeadersElementBuilder")).unwrap();
    assert_eq!(
        data_keys(&pipeline),
        ["sequence", "shareusage", "shareusage", "set-headers"]
    );

    // Each of the two gives its own settings, the one added with the
    // defaults and then the group's.
    let half = ElementOptions::new("ShareUsageBuilder").parameter("SharePercentage", "0.5");
    let options = PipelineOptions::new().element(ElementOptions::group([
        half,
        ElementOptions::new("SetHeadersElementBuilder"),
    ]));
    let pipeline = build_fiftyone(&options).unwrap();
    let percentages: Vec<f64> = share_usage_configs(&pipeline)
        .iter()
        .map(ShareUsageConfig::share_percentage)
        .collect();
    assert_eq!(percentages, [1.0, 0.5]);

    // .NET adds a second Sequence element before the group and builds. The
    // crates refuse a group whose element has the data key of one before it.
    let refused = "An element of a parallel group has the data key 'sequence' of an element \
                   that runs before the group, so it could not add its data once that element \
                   had. Add it on its own.";
    for second in ["SetHeadersElementBuilder", "ShareUsageBuilder"] {
        let options = group("SequenceElementBuilder", second);
        assert_eq!(message(build_fiftyone(&options)), refused, "{second}");
    }

    // A group of one element is that element on its own here, so it is
    // counted. .NET adds a second, as its group of one is still a group.
    let sequence = ElementOptions::new("SequenceElementBuilder");
    let options = PipelineOptions::new()
        .element(ElementOptions::group([sequence]))
        .element(ElementOptions::new("SetHeadersElementBuilder"));
    assert_eq!(
        data_keys(&build_fiftyone(&options).unwrap()),
        ["sequence", "set-headers", "shareusage"]
    );
    let quarter = ElementOptions::new("ShareUsageBuilder").parameter("SharePercentage", "0.25");
    let options = set_headers().element(ElementOptions::group([quarter]));
    let pipeline = build_fiftyone(&options).unwrap();
    assert_eq!(
        data_keys(&pipeline),
        ["sequence", "set-headers", "shareusage"]
    );
    let config = share_usage_config(&pipeline);
    assert_eq!(config.share_percentage(), 0.25);
}

/// The Sequence element adds evidence, so a pipeline made from a
/// configuration refuses a parallel group holding it, wherever it is in the
/// group.
#[test]
fn a_parallel_group_holding_the_sequence_element_is_refused() {
    let _serial = start();
    let refused = "An element of a parallel group with the data key 'sequence' adds evidence, \
                   which the other elements of the group read at the same time. Add it on its \
                   own, before the elements that read what it adds.";
    for (first, second) in [
        ("SequenceElementBuilder", "SetHeadersElementBuilder"),
        ("SetHeadersElementBuilder", "SequenceElementBuilder"),
    ] {
        let options = PipelineOptions::new().element(ElementOptions::group([
            ElementOptions::new(first),
            ElementOptions::new(second),
        ]));
        assert_eq!(message(build(&options)), refused, "{first}, {second}");
    }
}

#[test]
fn with_no_elements_the_fiftyone_pipeline_adds_its_own() {
    let _serial = start();
    let pipeline = build_fiftyone(&PipelineOptions::new()).unwrap();
    assert_eq!(data_keys(&pipeline), ["sequence", "shareusage"]);
    let options = PipelineOptions::new().parameter("SetShareUsage", "false");
    assert_eq!(data_keys(&build_fiftyone(&options).unwrap()), ["sequence"]);
}

#[test]
fn an_element_that_fails_stops_the_build_before_the_pipeline_settings() {
    let _serial = start();
    let element = ElementOptions::new("ShareUsageBuilder").parameter("ShareUsageUrl", "not a url");
    let expected = failed(
        "ShareUsageBuilder",
        "Invalid URI: The format of the URI could not be determined.",
    );
    for options in [
        PipelineOptions::new().element(element.clone()),
        PipelineOptions::new()
            .element(element.clone())
            .parameter("SetShareUsage", "yes"),
    ] {
        assert_eq!(message(build_fiftyone(&options)), expected);
    }
}

#[test]
fn the_core_pipeline_settings_name_the_fiftyone_pipeline_builder() {
    let _serial = start();
    for name in [
        "SuppressProcessException",
        "SetSuppressProcessExceptions",
        "SuppressProcessExceptions",
    ] {
        let pipeline = build_fiftyone(&set_headers().parameter(name, "True")).unwrap();
        assert!(pipeline.is_suppress_process_exceptions(), "{name}");
    }
    for method in ["SetSuppressProcessException", "SetAutoDisposeElements"] {
        let options = set_headers().parameter(method, "maybe");
        assert_eq!(
            message(build_fiftyone(&options)),
            format!(
                "Method '{method}' on builder '{FIFTYONE_PIPELINE}' for pipeline expects a \
                 parameter of type 'Boolean'. Failed to parse value 'maybe' using the static \
                 'TryParse' method."
            )
        );
    }
    let options = set_headers()
        .parameter("SetShareUsage", "false")
        .parameter("SetSuppressProcessException", "true");
    let pipeline = build_fiftyone(&options).unwrap();
    assert_eq!(data_keys(&pipeline), ["sequence", "set-headers"]);
    assert!(pipeline.is_suppress_process_exceptions());

    // .NET ignores an unknown pipeline setting and logs nothing, where the
    // crates log a warning.
    assert!(logged().is_empty(), "{:?}", logged());
    let pipeline = build_fiftyone(&set_headers().parameter("SetNoSuchSetting", "1")).unwrap();
    assert_eq!(
        data_keys(&pipeline),
        ["sequence", "set-headers", "shareusage"]
    );
    assert_eq!(
        logged(),
        [
            "WARN The pipeline has no setting matching the build parameter 'SetNoSuchSetting', so \
          it is ignored."
        ]
    );
}

/// .NET looks a pipeline setting up among every public method of its
/// builder, so it refuses `Build` and `ToString` as taking no parameters
/// and `AddFlowElement` as taking an `IFlowElement`, which has no
/// `TryParse`. The crates ignore each, as any name that is not a setting.
#[test]
fn a_pipeline_setting_named_after_a_builder_method_is_ignored() {
    let _serial = start();
    for name in ["Build", "AddFlowElement", "ToString"] {
        LOGGED.lock().unwrap().clear();
        let pipeline = build_fiftyone(&set_headers().parameter(name, "1")).expect(name);
        assert_eq!(
            data_keys(&pipeline),
            ["sequence", "set-headers", "shareusage"]
        );
        assert_eq!(
            logged(),
            [format!(
                "WARN The pipeline has no setting matching the build parameter '{name}', so it \
                 is ignored."
            )]
        );
    }
}

/// .NET calls each setting as it reads it, so a value the builder refuses
/// is reported before a parameter that matches nothing. The crates check
/// every parameter's name before applying any setting, so the parameter
/// that matches nothing is reported.
#[test]
fn a_parameter_matching_nothing_is_reported_before_a_refused_value() {
    let _serial = start();
    let found = message(share_usage(&[
        ("FailuresToEnterRecovery", "0"),
        ("NoSuchSetting", "1"),
    ]));
    assert!(
        found.contains(
            "has no 'Set' methods or 'Build' methods that match (case-insensitively) the \
             following parameters: NOSUCHSETTING."
        ),
        "{found}"
    );
    assert!(!found.contains("failuresToEnterRecovery"), "{found}");
}

/// Names are compared with Unicode's full upper case mapping, so `ß`
/// matches `SS`. .NET maps each character to one character, so it ignores
/// this name as matching no setting.
#[test]
fn a_name_with_sharp_s_matches_its_double_s_form() {
    let _serial = start();
    let pipeline =
        build_fiftyone(&set_headers().parameter("SuppreßProcessExceptions", "true")).unwrap();
    assert!(pipeline.is_suppress_process_exceptions());
    assert!(logged().is_empty(), "{:?}", logged());
}

#[test]
fn a_json_file_for_dotnets_fiftyone_pipeline_builder_makes_the_same_pipeline() {
    let _serial = start();
    let options = PipelineOptions::from_json(
        r#"{
          "PipelineOptions": {
            "Elements": [ { "BuilderName": "SetHeadersElementBuilder" } ],
            "BuildParameters": {
              "SetShareUsage": false,
              "SuppressProcessExceptions": true
            }
          }
        }"#,
    )
    .unwrap();
    let pipeline = build_fiftyone(&options).unwrap();
    assert_eq!(data_keys(&pipeline), ["sequence", "set-headers"]);
    assert!(pipeline.is_suppress_process_exceptions());

    let options = PipelineOptions::from_json(
        r#"{
          "PipelineOptions": {
            "Elements": [
              { "BuilderName": "SetHeadersElementBuilder" },
              { "BuilderName": "ShareUsageBuilder", "BuildParameters": { "SharePercentage": 0.5 } }
            ],
            "BuildParameters": { "ShareUsage": "true" }
          }
        }"#,
    )
    .unwrap();
    let pipeline = build_fiftyone(&options).unwrap();
    assert_eq!(
        data_keys(&pipeline),
        ["sequence", "set-headers", "shareusage"]
    );
    assert!(!pipeline.is_suppress_process_exceptions());
    let config = share_usage_config(&pipeline);
    assert_eq!(config.share_percentage(), 0.5);
}
