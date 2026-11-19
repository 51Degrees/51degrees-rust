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

//! The agent signature element made from a configuration, compared with
//! what .NET's FiftyOne.Pipeline.AgentSignature 4.5.129 made from the same
//! configuration. Each expected setting and message is the one .NET held
//! or gave, apart from the differences the tests name.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{process_signed, test_key_directory, FakeClient, FakeResponse, Signer, DIRECTORY_URL};
use fiftyone_agent_signature::constants::{REASON_VERIFIED, STATUS_VERIFIED};
use fiftyone_agent_signature::{configuration, AgentSignatureElement, HttpClient};
use fiftyone_pipeline_core::configuration::{BuilderRegistry, ElementOptions, PipelineOptions};
use fiftyone_pipeline_core::{Error, Pipeline, PipelineBuilder, Result};

const BUILDER: &str = "AgentSignatureElementBuilder";
const BUILDER_TYPE: &str = "fiftyone_agent_signature::builder::AgentSignatureElementBuilder";
const NO_SUB_ELEMENTS: &str = "No SubElements";
const NO_INNER: &str = "No Inner Exception.";
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

fn registry() -> BuilderRegistry {
    let mut registry = BuilderRegistry::new();
    configuration::add_builders(&mut registry);
    registry
}

/// The pipeline `options` describe, given `client` to fetch with. A client
/// that reaches nothing stands in for the network unless a test needs one.
fn build_with(options: &PipelineOptions, client: Arc<dyn HttpClient>) -> Result<Arc<Pipeline>> {
    PipelineBuilder::new()
        .add_resource(Arc::new(client))
        .build_from_configuration(&registry(), options)
}

fn build(options: &PipelineOptions) -> Result<Arc<Pipeline>> {
    build_with(options, Arc::new(FakeClient::default()))
}

/// The element `builder` with `parameters`, alone in a pipeline.
fn with(parameters: &[(&str, &str)]) -> Result<Arc<Pipeline>> {
    let element = parameters
        .iter()
        .fold(ElementOptions::new(BUILDER), |element, (n, v)| {
            element.parameter(*n, *v)
        });
    build(&PipelineOptions::new().element(element))
}

fn message(result: Result<Arc<Pipeline>>) -> String {
    match result {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

/// The message .NET gives for an element that cannot be made.
fn failed(message: &str) -> String {
    format!(
        "Configuration failed to build.\nBuilderName: {BUILDER}\nSubElement Builder Names: \
         {NO_SUB_ELEMENTS}\n{message}\n{NO_INNER}\n"
    )
}

/// .NET's message for a value its `TryParse` for `type_name` refuses.
fn not_read(setting: &str, type_name: &str, value: &str) -> String {
    failed(&format!(
        "Method '{setting}' on builder '{BUILDER_TYPE}' for element 0 expects a parameter of \
         type '{type_name}'. Failed to parse value '{value}' using the static 'TryParse' method."
    ))
}

/// .NET's message for a period its builder refuses, which it gives as the
/// inner message of "Exception has been thrown by the target of an
/// invocation.". The crates give it as the message.
fn period_refused(parameter: &str, value: &str) -> String {
    failed(&format!(
        "The period must not be negative and must not be longer than 365.00:00:00. \
         (Parameter '{parameter}')\nActual value was {value}."
    ))
}

fn element(result: Result<Arc<Pipeline>>) -> Arc<AgentSignatureElement> {
    result
        .expect("the pipeline")
        .resource::<AgentSignatureElement>()
        .expect("the agent signature element")
}

/// The settings .NET held for the element named by its builder with no
/// parameters. .NET also held a null `HttpClient` and a `Concurrency` of
/// the processor count, which the crates do not have.
fn assert_defaults(element: &AgentSignatureElement) {
    assert_eq!(element.cache_size(), 1000);
    assert_eq!(element.cache_lifetime(), DAY);
    assert_eq!(element.negative_cache_lifetime(), Duration::from_secs(300));
    assert_eq!(element.wait_budget(), Duration::from_millis(350));
    assert_eq!(element.fetch_timeout(), Duration::from_secs(5));
    assert_eq!(element.clock_skew(), Duration::from_secs(60));
    assert_eq!(element.max_lifetime(), Duration::ZERO);
    assert!(element.allow_legacy_signature_agent());
    assert!(!element.trust_forwarded_evidence());
    assert_eq!(element.reachability_check_url(), None);
    assert!(!element.allow_inline_directory());
    assert_eq!(element.max_response_bytes(), 262_144);
    assert!(element.registries().is_empty());
}

#[test]
fn the_builder_is_found_by_dotnets_names() {
    for name in [
        BUILDER,
        "AgentSignatureElement",
        "agentsignatureelementbuilder",
    ] {
        let pipeline = build(&PipelineOptions::new().element(ElementOptions::new(name)))
            .expect("the pipeline");
        let keys: Vec<&str> = pipeline
            .flow_elements()
            .iter()
            .map(|e| e.data_key())
            .collect();
        assert_eq!(keys, ["agent-signature"], "{name}");
        assert_defaults(&element(Ok(pipeline)));
    }
}

#[test]
fn every_setting_is_read_as_dotnet_reads_it() {
    let reachability = "https://127.0.0.1:1/.well-known/http-message-signatures-directory";
    let element = element(with(&[
        ("CacheSize", "5"),
        ("CacheLifetime", "02:00:00"),
        ("NegativeCacheLifetime", "00:00:30"),
        ("WaitBudget", "00:00:01"),
        ("FetchTimeout", "00:00:10"),
        ("ClockSkew", "00:00:05"),
        ("MaxLifetime", "01:00:00"),
        ("Registry", "https://example.com/registry.txt"),
        ("TrustForwardedEvidence", "true"),
        ("ReachabilityCheckUrl", reachability),
        ("AllowLegacySignatureAgent", "false"),
        ("AllowInlineDirectory", "True"),
        ("MaxResponseBytes", "1000"),
    ]));
    assert_eq!(element.cache_size(), 5);
    assert_eq!(element.cache_lifetime(), Duration::from_secs(2 * 60 * 60));
    assert_eq!(element.negative_cache_lifetime(), Duration::from_secs(30));
    assert_eq!(element.wait_budget(), Duration::from_secs(1));
    assert_eq!(element.fetch_timeout(), Duration::from_secs(10));
    assert_eq!(element.clock_skew(), Duration::from_secs(5));
    assert_eq!(element.max_lifetime(), Duration::from_secs(60 * 60));
    assert_eq!(element.registries(), ["https://example.com/registry.txt"]);
    assert!(element.trust_forwarded_evidence());
    assert_eq!(element.reachability_check_url(), Some(reachability));
    assert!(!element.allow_legacy_signature_agent());
    assert!(element.allow_inline_directory());
    assert_eq!(element.max_response_bytes(), 1000);
}

#[test]
fn a_setting_is_found_with_or_without_set_in_any_case() {
    let element = element(with(&[
        ("SetCacheSize", "7"),
        ("setwaitbudget", "00:00:02"),
        ("SETREGISTRY", "https://example.com/r.txt"),
    ]));
    assert_eq!(element.cache_size(), 7);
    assert_eq!(element.wait_budget(), Duration::from_secs(2));
    assert_eq!(element.registries(), ["https://example.com/r.txt"]);
}

/// .NET gives these as the inner message of "Exception has been thrown by
/// the target of an invocation.", and the crates as the message.
#[test]
fn a_count_below_one_is_refused_with_dotnets_message() {
    for (setting, value, parameter, text) in [
        (
            "CacheSize",
            "0",
            "size",
            "The cache must be allowed to hold at least one key directory.",
        ),
        (
            "CacheSize",
            "-1",
            "size",
            "The cache must be allowed to hold at least one key directory.",
        ),
        (
            "MaxResponseBytes",
            "0",
            "bytes",
            "The response limit must allow at least one byte to be read.",
        ),
        (
            "MaxResponseBytes",
            "-5",
            "bytes",
            "The response limit must allow at least one byte to be read.",
        ),
    ] {
        assert_eq!(
            message(with(&[(setting, value)])),
            failed(&format!(
                "{text} (Parameter '{parameter}')\nActual value was {value}."
            )),
            "{setting} {value}"
        );
    }
    assert_eq!(element(with(&[("CacheSize", "1")])).cache_size(), 1);
    // The largest size takes memory only as signatures arrive.
    assert_eq!(
        element(with(&[("CacheSize", "2147483647")])).cache_size(),
        2_147_483_647
    );
    assert_eq!(
        element(with(&[("MaxResponseBytes", "2147483647")])).max_response_bytes(),
        2_147_483_647
    );
}

#[test]
fn a_count_dotnet_cannot_read_is_refused() {
    for value in ["x", "2147483648"] {
        assert_eq!(
            message(with(&[("CacheSize", value)])),
            not_read("SetCacheSize", "Int32", value)
        );
    }
    let options = PipelineOptions::new()
        .element(ElementOptions::new(BUILDER).parameter("CacheSize", vec!["5"]));
    assert_eq!(
        message(build(&options)),
        failed(&format!(
            "Method 'SetCacheSize' on builder '{BUILDER_TYPE}' for element 0 expects a \
             parameter of type 'Int32' but supplied object is of type 'String[]'."
        ))
    );
}

#[test]
fn a_period_is_read_as_dotnet_reads_a_timespan() {
    assert_eq!(
        element(with(&[("CacheLifetime", "365")])).cache_lifetime(),
        365 * DAY
    );
    assert_eq!(
        element(with(&[("CacheLifetime", "5")])).cache_lifetime(),
        5 * DAY
    );
    assert_eq!(
        element(with(&[("MaxLifetime", "0")])).max_lifetime(),
        Duration::ZERO
    );
    assert_eq!(
        element(with(&[("MaxLifetime", "00:00:00.0000001")])).max_lifetime(),
        Duration::from_nanos(100)
    );
}

#[test]
fn a_period_out_of_range_is_refused_with_dotnets_message() {
    for (setting, value, parameter, written) in [
        ("CacheLifetime", "-00:00:01", "lifetime", "-00:00:01"),
        ("CacheLifetime", "366", "lifetime", "366.00:00:00"),
        (
            "CacheLifetime",
            "365.00:00:00.0000001",
            "lifetime",
            "365.00:00:00.0000001",
        ),
        ("NegativeCacheLifetime", "-1", "lifetime", "-1.00:00:00"),
        ("NegativeCacheLifetime", "366", "lifetime", "366.00:00:00"),
        (
            "WaitBudget",
            "-1.02:03:04.5",
            "waitBudget",
            "-1.02:03:04.5000000",
        ),
        (
            "FetchTimeout",
            "10675199.02:48:05.4775807",
            "fetchTimeout",
            "10675199.02:48:05.4775807",
        ),
        ("ClockSkew", "-00:00:01", "clockSkew", "-00:00:01"),
        ("MaxLifetime", "366", "maxLifetime", "366.00:00:00"),
    ] {
        assert_eq!(
            message(with(&[(setting, value)])),
            period_refused(parameter, written),
            "{setting} {value}"
        );
    }
}

#[test]
fn a_period_dotnet_cannot_read_is_refused() {
    for (setting, value) in [
        ("CacheLifetime", "x"),
        ("CacheLifetime", ""),
        ("ClockSkew", "00:60:00"),
    ] {
        assert_eq!(
            message(with(&[(setting, value)])),
            not_read(&format!("Set{setting}"), "TimeSpan", value),
            "{setting} {value}"
        );
    }
    let options = PipelineOptions::new()
        .element(ElementOptions::new(BUILDER).parameter("CacheLifetime", vec!["00:01:00"]));
    assert_eq!(
        message(build(&options)),
        failed(&format!(
            "Method 'SetCacheLifetime' on builder '{BUILDER_TYPE}' for element 0 expects a \
             parameter of type 'TimeSpan' but supplied object is of type 'String[]'."
        ))
    );
}

#[test]
fn a_registry_is_one_address() {
    assert!(element(with(&[("Registry", "")])).registries().is_empty());
    assert_eq!(
        element(with(&[(
            "Registry",
            "https://a.example/r.txt,https://b.example/r.txt"
        )]))
        .registries(),
        ["https://a.example/r.txt,https://b.example/r.txt"]
    );
    let options = PipelineOptions::new().element(
        ElementOptions::new(BUILDER).parameter("Registry", vec!["https://example.com/r.txt"]),
    );
    assert_eq!(
        message(build(&options)),
        failed("Object of type 'System.String[]' cannot be converted to type 'System.String'.")
    );
}

/// .NET's builder takes these and its element reads the registry from the
/// address as given. The crates refuse them when the element is built, as
/// they do in code.
#[test]
fn a_registry_that_is_not_https_is_refused() {
    for url in ["http://example.com/r.txt", "https://u:p@example.com/r.txt"] {
        assert_eq!(
            message(with(&[("Registry", url)])),
            failed(&format!(
                "The registry address '{url}' must be HTTPS and carry no user information."
            ))
        );
    }
}

#[test]
fn the_flags_are_read_as_dotnet_reads_booleans() {
    assert!(element(with(&[("TrustForwardedEvidence", " TRUE ")])).trust_forwarded_evidence());
    assert_eq!(
        message(with(&[("TrustForwardedEvidence", "yes")])),
        not_read("SetTrustForwardedEvidence", "Boolean", "yes")
    );
    assert_eq!(
        message(with(&[("AllowInlineDirectory", "1")])),
        not_read("SetAllowInlineDirectory", "Boolean", "1")
    );
}

/// .NET holds an empty address and makes no check, as the crates do.
#[test]
fn an_empty_reachability_address_makes_no_check() {
    assert_eq!(
        element(with(&[("ReachabilityCheckUrl", "")])).reachability_check_url(),
        None
    );
}

#[test]
fn the_client_is_given_in_code_only() {
    assert_eq!(
        message(with(&[("HttpClient", "x")])),
        failed(&format!(
            "Method 'SetHttpClient' on builder '{BUILDER_TYPE}' for element 0 expects a \
             parameter of type 'HttpClient' but this type does not have the expected 'TryParse' \
             method (or has multiples that cannot be resolved)."
        ))
    );
}

/// .NET lists the same 14 settings in the same order, giving
/// `RuntimeParameterInfo` as each type. `Clock` is not one of them, as
/// .NET's `SetClock` is internal, and nor is `Concurrency`. .NET matches a
/// parameter to any public method of the builder by its name, so for
/// `Build` it finds `Build()` and refuses it with "Method 'Build' on
/// builder '...' for element 0 takes no parameters This is not
/// supported.". Here only the settings are matched, so `Build` is a
/// parameter no setting takes.
#[test]
fn an_unknown_setting_lists_dotnets_settings() {
    let listed = [
        "SetHttpClient (HttpClient)",
        "SetCacheSize (Int32)",
        "SetCacheLifetime (TimeSpan)",
        "SetNegativeCacheLifetime (TimeSpan)",
        "SetWaitBudget (TimeSpan)",
        "SetFetchTimeout (TimeSpan)",
        "SetClockSkew (TimeSpan)",
        "SetMaxLifetime (TimeSpan)",
        "SetRegistry (String)",
        "SetTrustForwardedEvidence (Boolean)",
        "SetReachabilityCheckUrl (String)",
        "SetAllowLegacySignatureAgent (Boolean)",
        "SetAllowInlineDirectory (Boolean)",
        "SetMaxResponseBytes (Int32)",
    ]
    .iter()
    .map(|s| format!("{s}\n"))
    .collect::<String>();
    for (name, upper) in [
        ("Registries", "REGISTRIES"),
        ("Clock", "CLOCK"),
        ("Concurrency", "CONCURRENCY"),
        ("Build", "BUILD"),
    ] {
        assert_eq!(
            message(with(&[(name, "x")])),
            failed(&format!(
                "Builder '{BUILDER_TYPE}' for element 0 has no 'Set' methods or 'Build' methods \
                 that match (case-insensitively) the following parameters: {upper}. The \
                 available configuration methods on this builder are: \n{listed}The available \
                 'Build' methods have the following signatures: \n\n"
            )),
            "{name}"
        );
    }
}

/// .NET, given the element twice with a cache size of 3 on the second,
/// made both, each with its own settings. The pipeline holds the first, so
/// here the second trusts forwarded evidence, which its evidence filter
/// shows.
#[test]
fn the_element_named_twice_is_made_twice() {
    let pipeline = build(
        &PipelineOptions::new()
            .element(ElementOptions::new(BUILDER))
            .element(ElementOptions::new(BUILDER).parameter("TrustForwardedEvidence", "true")),
    )
    .expect("the pipeline");
    let elements = pipeline.flow_elements();
    let keys: Vec<&str> = elements.iter().map(|e| e.data_key()).collect();
    assert_eq!(keys, ["agent-signature", "agent-signature"]);
    let trusted: Vec<bool> = elements
        .iter()
        .map(|e| e.evidence_key_filter().include("query.signature"))
        .collect();
    assert_eq!(trusted, [false, true]);
    assert_defaults(&element(Ok(pipeline)));
}

/// The values .NET's JSON reader gives as numbers and booleans are read as
/// their text, as .NET read them from this configuration in a file.
#[test]
fn a_json_file_is_read_as_dotnet_reads_it() {
    let options = PipelineOptions::from_json(
        r#"{
            "PipelineOptions": {
                "Elements": [
                    {
                        "BuilderName": "AgentSignatureElement",
                        "BuildParameters": {
                            "CacheSize": 12,
                            "WaitBudget": "00:00:00.5",
                            "TrustForwardedEvidence": true,
                            "Registry": "https://example.com/registry.txt",
                            "MaxResponseBytes": 4096
                        }
                    }
                ]
            }
        }"#,
    )
    .expect("the options");
    let element = element(build(&options));
    assert_eq!(element.cache_size(), 12);
    assert_eq!(element.wait_budget(), Duration::from_millis(500));
    assert!(element.trust_forwarded_evidence());
    assert_eq!(element.registries(), ["https://example.com/registry.txt"]);
    assert_eq!(element.max_response_bytes(), 4096);
}

/// The client given to the pipeline builder fetches the keys, so a signed
/// request is verified, and the reachability check uses it too.
#[test]
fn the_element_fetches_with_the_client_given() {
    let client = Arc::new(FakeClient::default().with(
        DIRECTORY_URL,
        FakeResponse::directory(test_key_directory(None)),
    ));
    let options = PipelineOptions::new().element(
        ElementOptions::new(BUILDER)
            .parameter("ReachabilityCheckUrl", DIRECTORY_URL)
            .parameter("WaitBudget", "00:00:20"),
    );
    let pipeline =
        build_with(&options, Arc::clone(&client) as Arc<dyn HttpClient>).expect("the pipeline");
    let data = process_signed(&pipeline, &Signer::default().sign(), &[]);
    assert_eq!(
        data.agent_signature().value().map(String::as_str),
        Ok(STATUS_VERIFIED)
    );
    assert_eq!(
        data.agent_signature_reason().value().map(String::as_str),
        Ok(REASON_VERIFIED)
    );
    // One fetch for the request and one for the check, which runs on a
    // thread of its own.
    let deadline = Instant::now() + Duration::from_secs(20);
    while client.calls(DIRECTORY_URL) < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(client.calls(DIRECTORY_URL), 2);
}
