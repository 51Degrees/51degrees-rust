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

use fiftyone_pipeline_core::configuration::{BuilderRegistry, ElementOptions, PipelineOptions};
use fiftyone_pipeline_core::{Error, Pipeline, PipelineBuilder, Result};
use fiftyone_pipeline_engines_fiftyone::constants::SHARE_USAGE_DEFAULT_URL;
use fiftyone_pipeline_engines_fiftyone::{configuration, ShareUsageConfig};

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
    let pipeline = result.expect("the pipeline is built");
    let config = pipeline
        .resource::<ShareUsageConfig>()
        .expect("the usage sharing configuration");
    (*config).clone()
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

    let config = pipeline.resource::<ShareUsageConfig>().unwrap();
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
    let filter = pipeline.flow_elements()[0].evidence_key_filter();
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
fn sharing_all_evidence_shares_every_key() {
    let _serial = start();
    let pipeline = share_usage(&[("ShareAllEvidence", "true")]).unwrap();
    assert!(pipeline
        .resource::<ShareUsageConfig>()
        .unwrap()
        .share_all_evidence());
    let filter = pipeline.flow_elements()[0].evidence_key_filter();
    for key in ["header.cookie", "cookie.session", "query.anything"] {
        assert!(filter.include(key), "{key}");
    }
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
    // .NET fails here with a NullReferenceException. A configuration file
    // cannot give this order, as its names are applied sorted.
    assert_eq!(
        included(&[
            ("ShareAllQueryStringParameters", "true"),
            ("IncludedQueryStringParameter", "x"),
        ]),
        None
    );
    assert_eq!(
        included(&[("SetIncludedQueryStringParameters", "")]),
        Some(Vec::new())
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
    let config = pipeline.resource::<ShareUsageConfig>().unwrap();
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
