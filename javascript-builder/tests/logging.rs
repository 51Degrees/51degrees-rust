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

//! The records the JavaScript builder logs, at warning level and above, in
//! the cases where the .NET JavaScript builder logs.
//!
//! Each record is kept with the thread that logged it, because the tests in
//! this binary run in parallel and each counts only its own records.

use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, ThreadId};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_javascript_builder::{JavaScriptBuilderElement, JAVASCRIPT_BUILDER_DATA_KEY};
use fiftyone_json_builder::JsonBuilderElement;
use fiftyone_pipeline_core::{Evidence, Pipeline};

struct CapturingLogger {
    entries: Mutex<Vec<(ThreadId, log::Level, String)>>,
}

impl log::Log for CapturingLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.entries.lock().unwrap().push((
                thread::current().id(),
                record.level(),
                record.args().to_string(),
            ));
        }
    }

    fn flush(&self) {}
}

fn logger() -> &'static CapturingLogger {
    static LOGGER: OnceLock<&'static CapturingLogger> = OnceLock::new();
    LOGGER.get_or_init(|| {
        let logger: &'static CapturingLogger = Box::leak(Box::new(CapturingLogger {
            entries: Mutex::new(Vec::new()),
        }));
        log::set_logger(logger).expect("no other logger is installed in this binary");
        log::set_max_level(log::LevelFilter::Warn);
        logger
    })
}

/// The records the current thread has logged, from the `from`th on.
fn records_since(from: usize) -> Vec<(log::Level, String)> {
    let me = thread::current().id();
    logger()
        .entries
        .lock()
        .unwrap()
        .iter()
        .filter(|(id, _, _)| *id == me)
        .skip(from)
        .map(|(_, level, message)| (*level, message.clone()))
        .collect()
}

/// How many records the current thread has logged so far.
fn mark() -> usize {
    records_since(0).len()
}

/// Process one request and return the script and the records it logged.
fn request(
    pipeline: &Arc<Pipeline>,
    evidence: &[(&str, &str)],
) -> (String, Vec<(log::Level, String)>) {
    let from = mark();
    let mut builder = Evidence::builder();
    for (key, value) in evidence {
        builder = builder.add(*key, *value);
    }
    let mut data = pipeline.create_flow_data_with(builder.build());
    data.process().expect("processing succeeds");
    let js = data
        .get(JAVASCRIPT_BUILDER_DATA_KEY)
        .expect("javascript builder data present")
        .javascript()
        .to_owned();
    (js, records_since(from))
}

/// The Sequence element, the JSON builder and `element`, as the web
/// integration orders them.
fn pipeline(element: JavaScriptBuilderElement) -> Arc<Pipeline> {
    Pipeline::builder()
        .add_element(Arc::new(
            fiftyone_pipeline_engines_fiftyone::SequenceElement::new(),
        ))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::new(element))
        .build()
        .expect("pipeline builds")
}

/// .NET logs an invalid object name in the builder at critical level and
/// throws. The `log` facade's highest level is error.
#[test]
fn an_invalid_object_name_in_the_builder_is_logged() {
    let from = mark();
    let result = JavaScriptBuilderElement::builder().set_object_name("1bad");
    assert!(result.is_err());
    let records = records_since(from);
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].0, log::Level::Error);
    assert!(records[0].1.contains("'1bad'"), "{records:?}");
}

#[test]
fn a_valid_object_name_in_the_builder_logs_nothing() {
    let from = mark();
    let result = JavaScriptBuilderElement::builder().set_object_name("ok");
    assert!(result.is_ok());
    assert!(records_since(from).is_empty());
}

/// .NET logs a callback URL its `Uri` refuses at error level, once for the
/// life of the element, and the script has no update section.
#[test]
fn an_invalid_callback_url_is_logged_once_and_left_out() {
    let pipeline = pipeline(
        JavaScriptBuilderElement::builder()
            .set_minify(false)
            .build(),
    );

    let (js, records) = request(&pipeline, &[("header.host", "exa mple.com")]);
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].0, log::Level::Error);
    assert!(
        records[0]
            .1
            .contains("\"https://exa mple.com/51dpipeline/json\""),
        "{records:?}"
    );
    assert!(!js.contains("processRequest"));

    let (js, records) = request(&pipeline, &[("header.host", "exa mple.com")]);
    assert!(records.is_empty(), "{records:?}");
    assert!(!js.contains("processRequest"));

    let (js, records) = request(&pipeline, &[("header.host", "example.com")]);
    assert!(records.is_empty(), "{records:?}");
    assert!(js.contains("'https://example.com/51dpipeline/json'"));

    let (_, records) = request(&pipeline, &[("header.host", "exa mple.com")]);
    assert!(records.is_empty(), "{records:?}");
}

#[test]
fn an_invalid_protocol_in_the_evidence_is_logged() {
    let pipeline = pipeline(
        JavaScriptBuilderElement::builder()
            .set_minify(false)
            .build(),
    );
    let (js, records) = request(
        &pipeline,
        &[("header.host", "example.com"), ("header.protocol", "1http")],
    );
    assert_eq!(records.len(), 1, "{records:?}");
    assert!(!js.contains("processRequest"));
}

/// The record writes the URL escaped, so a line break in the host cannot
/// start a new log line.
#[test]
fn an_invalid_callback_url_is_logged_on_one_line() {
    let pipeline = pipeline(
        JavaScriptBuilderElement::builder()
            .set_minify(false)
            .build(),
    );
    let (js, records) = request(&pipeline, &[("header.host", "a\nb.com\r\u{2028}")]);
    assert_eq!(records.len(), 1, "{records:?}");
    for c in ['\n', '\r', '\u{2028}'] {
        assert!(!records[0].1.contains(c), "{c:?} in {records:?}");
    }
    assert!(!js.contains("processRequest"));
}

/// A host with user information is refused, and the record replaces the
/// user information with `***`, so a user name or password never reaches
/// the log.
#[test]
fn a_refused_host_does_not_log_its_user_information() {
    let pipeline = pipeline(
        JavaScriptBuilderElement::builder()
            .set_minify(false)
            .build(),
    );
    let (js, records) = request(&pipeline, &[("header.host", "user:secret@example.com")]);
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].0, log::Level::Error);
    assert!(!records[0].1.contains("secret"), "{records:?}");
    assert!(!records[0].1.contains("user"), "{records:?}");
    assert!(
        records[0]
            .1
            .contains("\"https://***@example.com/51dpipeline/json\""),
        "{records:?}"
    );
    assert!(!js.contains("processRequest"));
}

/// Everything in the host value before its last `@` is replaced by `***` in
/// the record, after the host's first `/` too, so a password holding `/` is
/// not logged and the part of a refused host before that `@` is lost.
#[test]
fn a_refused_host_logs_only_what_follows_its_user_information() {
    let cases = [
        (
            "a@b@example.com",
            "\"https://***@example.com/51dpipeline/json\"",
        ),
        ("u@[::1]:80x", "\"https://***@[::1]:80x/51dpipeline/json\""),
        (
            "user:pa/ss@example.com",
            "\"https://***@example.com/51dpipeline/json\"",
        ),
        (
            "user:pa?ss@example.com",
            "\"https://***@example.com/51dpipeline/json\"",
        ),
        ("u@exa mple.com/a@b", "\"https://***@b/51dpipeline/json\""),
        ("exa mple.com/a@b", "\"https://***@b/51dpipeline/json\""),
    ];
    for (host, expected) in cases {
        let pipeline = pipeline(
            JavaScriptBuilderElement::builder()
                .set_minify(false)
                .build(),
        );
        let (_, records) = request(&pipeline, &[("header.host", host)]);
        assert_eq!(records.len(), 1, "host {host:?}, {records:?}");
        assert!(
            records[0].1.contains(expected),
            "host {host:?}, {records:?}"
        );
    }
}

/// A host value holding `@` after its first `/` is refused, whether it is
/// configured or comes from the evidence, so a password holding `/` reaches
/// neither the script nor the log. Read up to its first `/` alone,
/// `user:12/ss@example.com` is the host `user` with the port `12`, and the
/// script would carry `ss@example.com` in its path.
#[test]
fn a_host_with_an_at_after_its_first_slash_is_refused() {
    let cases = [
        (
            "user:12/ss@example.com",
            "12/ss",
            "\"https://***@example.com/51dpipeline/json\"",
        ),
        (
            "example.com/a@b",
            "a@b",
            "\"https://***@b/51dpipeline/json\"",
        ),
    ];
    for (host, hidden, logged) in cases {
        for configured in [false, true] {
            let builder = JavaScriptBuilderElement::builder().set_minify(false);
            let (builder, evidence) = if configured {
                (builder.set_host(host), Vec::new())
            } else {
                (builder, vec![("header.host", host)])
            };
            let pipeline = pipeline(builder.build());
            let (js, records) = request(&pipeline, &evidence);
            let case = format!("host {host:?}, configured {configured}");
            assert!(!js.contains("processRequest"), "{case}");
            assert!(!js.contains(hidden), "{case}");
            assert_eq!(records.len(), 1, "{case}, {records:?}");
            assert!(records[0].1.contains(logged), "{case}, {records:?}");
            assert!(!records[0].1.contains(hidden), "{case}, {records:?}");
        }
    }
}

/// A scheme other than `http` or `https` in the evidence is refused and
/// logged like any other URL that is not valid.
#[test]
fn a_scheme_other_than_http_in_the_evidence_is_logged() {
    let pipeline = pipeline(
        JavaScriptBuilderElement::builder()
            .set_minify(false)
            .build(),
    );
    let (js, records) = request(
        &pipeline,
        &[("header.host", "example.com"), ("header.protocol", "ftp")],
    );
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].0, log::Level::Error);
    assert!(
        records[0]
            .1
            .contains("\"ftp://example.com/51dpipeline/json\""),
        "{records:?}"
    );
    assert!(!js.contains("processRequest"));
}

#[test]
fn a_host_that_would_end_the_string_does_not_reach_the_script() {
    let pipeline = pipeline(
        JavaScriptBuilderElement::builder()
            .set_minify(false)
            .build(),
    );

    let (js, records) = request(&pipeline, &[("header.host", "example.com');alert(1);//")]);
    assert_eq!(records.len(), 1, "{records:?}");
    assert!(!js.contains("alert(1)"));

    let (js, records) = request(&pipeline, &[("header.host", "example.com/');alert(1);//")]);
    assert!(records.is_empty(), "{records:?}");
    assert!(js.contains("'https://example.com/%27);alert(1);//51dpipeline/json'"));
    assert!(!js.contains("');alert(1)"));
}

/// .NET reads Promise and Fetch on the normal path without logging when the
/// pipeline has no device detection.
#[test]
fn no_promise_or_fetch_property_logs_nothing() {
    let pipeline = pipeline(JavaScriptBuilderElement::builder().build());
    for _ in 0..2 {
        let (_, records) = request(&pipeline, &[("header.host", "example.com")]);
        assert!(records.is_empty(), "{records:?}");
    }
}

/// .NET logs a minification failure at error level once for the life of the
/// element and serves the script unminified.
#[cfg(all(feature = "minify", not(target_family = "wasm")))]
#[test]
fn a_minify_failure_is_logged_once_and_served_unminified() {
    use fiftyone_json_builder::{JsonBuilderData, JSON_BUILDER_DATA_KEY};
    use fiftyone_pipeline_core::{
        EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
        Result,
    };

    /// An element in place of the JSON builder that gives the JavaScript
    /// builder a JSON payload that is not JavaScript.
    struct BrokenJson {
        filter: EvidenceKeyFilterWhitelist,
    }

    impl FlowElement for BrokenJson {
        fn process(&self, data: &mut FlowData) -> Result<()> {
            data.get_or_add(JSON_BUILDER_DATA_KEY, JsonBuilderData::new)?
                .set_json("{\"device\":");
            Ok(())
        }

        fn data_key(&self) -> &str {
            "json-builder"
        }

        fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
            &self.filter
        }

        fn properties(&self) -> &[PropertyMetaData] {
            &[]
        }
    }

    let pipeline = Pipeline::builder()
        .add_element(Arc::new(BrokenJson {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<&str>::new()),
        }))
        .add_element(Arc::new(JavaScriptBuilderElement::new()))
        .build()
        .expect("pipeline builds");

    let (js, records) = request(&pipeline, &[("header.host", "example.com")]);
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].0, log::Level::Error);
    assert!(records[0].1.contains("minifying"), "{records:?}");
    assert!(js.contains("\n    "), "the script is served unminified");

    for _ in 0..2 {
        let (js, records) = request(&pipeline, &[("header.host", "example.com")]);
        assert!(records.is_empty(), "{records:?}");
        assert!(js.contains("\n    "), "the script is served unminified");
    }
}

#[cfg(all(feature = "minify", not(target_family = "wasm")))]
#[test]
fn a_script_that_minifies_logs_nothing() {
    let pipeline = pipeline(JavaScriptBuilderElement::new());
    let (js, records) = request(&pipeline, &[("header.host", "example.com")]);
    assert!(records.is_empty(), "{records:?}");
    assert!(!js.contains("\n    "), "the script is minified");
}

/// The JavaScript builder reads `Promise` and `Fetch` from the flow data on
/// each request. Aspect data with a missing property source, as an on-premise
/// engine writes, knows it lacks them, so it is not asked and logs nothing.
#[test]
fn reading_promise_and_fetch_logs_nothing_for_aspect_data_that_lacks_them() {
    use fiftyone_pipeline_core::{
        EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
        Result, TypedKey,
    };
    use fiftyone_pipeline_engines::{AspectDataBase, EngineDeployment, MissingPropertySource};

    /// Writes aspect data under `ip` with a missing property source.
    struct SourcedData {
        filter: EvidenceKeyFilterWhitelist,
    }

    impl FlowElement for SourcedData {
        fn process(&self, data: &mut FlowData) -> Result<()> {
            let mut aspect = AspectDataBase::new("ip");
            aspect.add_missing_property_source(Arc::new(MissingPropertySource::new(
                "ip",
                "TestEngine",
                EngineDeployment::OnPremise,
                "Lite",
                Vec::new(),
            )));
            data.get_or_add(TypedKey::<AspectDataBase>::new("ip"), || aspect)?;
            Ok(())
        }
        fn data_key(&self) -> &str {
            "ip"
        }
        fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
            &self.filter
        }
        fn properties(&self) -> &[PropertyMetaData] {
            &[]
        }
    }

    let pipeline = Pipeline::builder()
        .add_element(Arc::new(
            fiftyone_pipeline_engines_fiftyone::SequenceElement::new(),
        ))
        .add_element(Arc::new(SourcedData {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        }))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::new(
            JavaScriptBuilderElement::builder()
                .set_minify(false)
                .build(),
        ))
        .build()
        .expect("pipeline builds");
    for _ in 0..2 {
        let (_, records) = request(&pipeline, &[("header.host", "example.com")]);
        assert!(records.is_empty(), "{records:?}");
    }
}

/// A cloud that cannot be reached, so a request engine built on it has not
/// completed discovery.
struct UnreachableCloud;

impl CloudHttpClient for UnreachableCloud {
    fn send(&self, _request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        Err("connection refused".to_owned())
    }
}

/// The reference form decides at build whether to render the user prompt
/// section, so with the licensed products not yet known it leaves the section
/// out and logs one warning saying so. The shared form decides once the
/// products are known, so it logs nothing.
#[test]
fn the_reference_form_logs_a_warning_when_the_products_are_not_known() {
    let engine = CloudRequestEngine::builder()
        .resource_key("test-resource-key")
        .http_client(Arc::new(UnreachableCloud))
        .recovery_seconds(0.0)
        .build()
        .expect("a transient failure does not fail the build");
    assert!(!engine.has_loaded_metadata());

    let from = mark();
    let _ = JavaScriptBuilderElement::builder().set_cloud_request_engine(&engine);
    let records = records_since(from);
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].0, log::Level::Warn);
    assert!(
        records[0]
            .1
            .starts_with("The cloud request engine's licensed products are not known yet"),
        "{records:?}"
    );

    let from = mark();
    let _ = JavaScriptBuilderElement::builder().set_shared_cloud_request_engine(Arc::new(engine));
    assert_eq!(records_since(from), Vec::new());
}
