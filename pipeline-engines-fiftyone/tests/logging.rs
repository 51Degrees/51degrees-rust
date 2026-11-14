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

//! How many records the elements log at warning level or above, and at which
//! levels, for the cases where .NET's elements log. Kept in its own test
//! binary because a logger is installed once for the whole process, and each
//! test holds [`SERIAL`] so the records it reads are its own.

use std::any::Any;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, PropertyValueType,
    Result, TypedKey,
};
use fiftyone_pipeline_engines::{AspectDataBase, EngineDeployment, MissingPropertySource};
use fiftyone_pipeline_engines_fiftyone::{
    SequenceElement, SetHeadersElement, ShareUsageConfig, ShareUsageElement, UsageBatch, UsageSink,
};

/// Log records at warning level or above, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

/// Keeps every record at warning level or above in [`LOGGED`].
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

/// Install the logger, wait for any other test to finish and clear the
/// records it left.
fn start() -> MutexGuard<'static, ()> {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    LOGGED.lock().unwrap().clear();
    guard
}

/// The records logged since [`start`].
fn logged() -> Vec<String> {
    LOGGED.lock().unwrap().clone()
}

/// Process `raw` as the request's sequence, with errors suppressed or not.
fn process_sequence(raw: &str, suppress: bool) -> Result<()> {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .suppress_process_exceptions(suppress)
        .build()
        .unwrap();
    let evidence = Evidence::builder().add("query.sequence", raw).build();
    pipeline.create_flow_data_with(evidence).process()
}

#[test]
fn a_sequence_that_cannot_be_used_logs_nothing() {
    // It counts as absent and gives 1, with no error to record or log.
    for suppress in [false, true] {
        let _serial = start();
        process_sequence("abc", suppress).unwrap();
        assert_eq!(logged(), Vec::<String>::new(), "suppress {suppress}");
    }
}

#[test]
fn a_sequence_that_parses_logs_nothing() {
    let _serial = start();
    process_sequence("5", false).unwrap();
    assert_eq!(logged(), Vec::<String>::new());
}

/// Data that holds `held` with no value, and nothing else.
struct NoValueData {
    held: &'static str,
}

impl ElementData for NoValueData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        Err(NoValueError::new(format!("No value for '{name}'.")))
    }
    fn has_property(&self, name: &str) -> bool {
        name.eq_ignore_ascii_case(self.held)
    }
    fn keys(&self) -> Vec<String> {
        Vec::new()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// An element declaring one `SetHeader` property, whose data either lacks it
/// or holds it with no value.
struct HeaderElement {
    holds_property: bool,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl HeaderElement {
    const PROPERTY: &'static str = "SetHeaderBrowserAccept-CH";
    const MAP_KEY: TypedKey<MapElementData> = TypedKey::new("header");
    const NO_VALUE_KEY: TypedKey<NoValueData> = TypedKey::new("header");

    fn new(holds_property: bool) -> Self {
        HeaderElement {
            holds_property,
            filter: EvidenceKeyFilterWhitelist::default(),
            properties: vec![PropertyMetaData::new(
                Self::PROPERTY,
                "header",
                PropertyValueType::String,
            )],
        }
    }
}

impl FlowElement for HeaderElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        if self.holds_property {
            data.get_or_add(Self::NO_VALUE_KEY, || NoValueData {
                held: Self::PROPERTY,
            })?;
        } else {
            data.get_or_add(Self::MAP_KEY, MapElementData::new)?;
        }
        Ok(())
    }
    fn data_key(&self) -> &str {
        "header"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

fn set_headers_pipeline(holds_property: bool) -> Arc<Pipeline> {
    Pipeline::builder()
        .add_element(Arc::new(HeaderElement::new(holds_property)))
        .add_element(Arc::new(SetHeadersElement::new()))
        .build()
        .unwrap()
}

#[test]
fn set_headers_warns_on_each_request_for_a_missing_property() {
    let _serial = start();
    let pipeline = set_headers_pipeline(false);
    for _ in 0..2 {
        pipeline.create_flow_data().process().unwrap();
    }
    let warning = "WARN Property 'SetHeaderBrowserAccept-CH' is missing from \
                   'header', so it adds no value to the Accept-CH header.";
    assert_eq!(logged(), vec![warning, warning]);
}

/// An element declaring one `SetHeader` property, whose aspect data lacks it
/// and can say so, as an on-premise engine's data can.
struct AspectHeaderElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl AspectHeaderElement {
    const KEY: TypedKey<AspectDataBase> = TypedKey::new("header");

    fn new() -> Self {
        AspectHeaderElement {
            filter: EvidenceKeyFilterWhitelist::default(),
            properties: vec![PropertyMetaData::new(
                HeaderElement::PROPERTY,
                "header",
                PropertyValueType::String,
            )],
        }
    }
}

impl FlowElement for AspectHeaderElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.get_or_add(Self::KEY, || {
            let mut aspect = AspectDataBase::new("header");
            aspect.add_missing_property_source(Arc::new(MissingPropertySource::new(
                "header",
                "HeaderEngine",
                EngineDeployment::OnPremise,
                "Lite",
                Vec::new(),
            )));
            aspect
        })?;
        Ok(())
    }
    fn data_key(&self) -> &str {
        "header"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// Set Headers asks whether the data holds the property before reading it,
/// so the aspect data's own warning for a read of a missing property is not
/// logged as well, as .NET's `TryGet` logs nothing.
#[test]
fn set_headers_gives_the_only_warning_for_a_property_aspect_data_lacks() {
    let _serial = start();
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(AspectHeaderElement::new()))
        .add_element(Arc::new(SetHeadersElement::new()))
        .build()
        .unwrap();
    for _ in 0..2 {
        pipeline.create_flow_data().process().unwrap();
    }
    let warning = "WARN Property 'SetHeaderBrowserAccept-CH' is missing from \
                   'header', so it adds no value to the Accept-CH header.";
    assert_eq!(logged(), vec![warning, warning]);
}

#[test]
fn set_headers_says_nothing_for_a_property_with_no_value() {
    let _serial = start();
    set_headers_pipeline(true)
        .create_flow_data()
        .process()
        .unwrap();
    assert_eq!(logged(), Vec::<String>::new());
}

/// Start a local HTTP server that refuses every request with a 403, and count
/// the requests.
fn start_refusing_server() -> (String, Arc<AtomicUsize>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}/new.ashx", server.server_addr());
    let requests = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&requests);
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            counted.fetch_add(1, Ordering::SeqCst);
            let response = tiny_http::Response::from_string("refused").with_status_code(403);
            let _ = request.respond(response);
        }
    });
    (url, requests)
}

#[test]
fn a_refused_send_logs_one_warning() {
    let _serial = start();
    let (url, requests) = start_refusing_server();
    let share = Arc::new(ShareUsageElement::new(
        ShareUsageConfig::builder()
            .share_usage_url(url.clone())
            .minimum_entries_per_message(1)
            .build(),
    ));
    let pipeline = Pipeline::builder()
        .add_element(Arc::clone(&share) as Arc<dyn FlowElement>)
        .build()
        .unwrap();
    let evidence = Evidence::builder()
        .add("header.user-agent", "refused-agent")
        .build();
    pipeline.create_flow_data_with(evidence).process().unwrap();

    const SEND_DEADLINE_SECONDS: u64 = 10;
    const POLL_INTERVAL_MILLISECONDS: u64 = 25;
    let deadline = Instant::now() + Duration::from_secs(SEND_DEADLINE_SECONDS);
    while share.fail_count() == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MILLISECONDS));
    }
    drop(pipeline);
    drop(share);

    assert_eq!(requests.load(Ordering::SeqCst), 1);
    let records = logged();
    assert_eq!(records.len(), 1, "{records:?}");
    assert!(records[0].starts_with("WARN "), "{records:?}");
    assert!(records[0].contains(&url), "{records:?}");
    assert!(records[0].contains("403"), "{records:?}");
}

/// A sink that refuses every message and counts them.
struct Refusing(Arc<AtomicUsize>);

impl UsageSink for Refusing {
    fn receive(
        &self,
        _batch: UsageBatch,
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err("the endpoint answered 503".into())
    }
}

/// usage-sharing-element.md, "Back-off after failed sends", item 6: when
/// sending pauses, a warning gives the delay. With the defaults, ten failed
/// sends within 100 seconds pause sending for 2 seconds. Each failed send
/// gives a warning of its own before it.
#[test]
fn a_pause_after_failed_sends_logs_one_warning() {
    let _serial = start();
    let refused = Arc::new(AtomicUsize::new(0));
    let share = Arc::new(ShareUsageElement::with_sink(
        ShareUsageConfig::builder()
            .minimum_entries_per_message(1)
            .take_timeout(Duration::ZERO)
            .build(),
        Refusing(Arc::clone(&refused)),
    ));
    let pipeline = Pipeline::builder()
        .add_element(Arc::clone(&share) as Arc<dyn FlowElement>)
        .build()
        .unwrap();

    const SEND_DEADLINE_SECONDS: u64 = 10;
    const POLL_INTERVAL_MILLISECONDS: u64 = 25;
    for n in 0..10 {
        let evidence = Evidence::builder()
            .add("header.user-agent", format!("refused-{n}"))
            .build();
        pipeline.create_flow_data_with(evidence).process().unwrap();
        let deadline = Instant::now() + Duration::from_secs(SEND_DEADLINE_SECONDS);
        while refused.load(Ordering::SeqCst) <= n && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MILLISECONDS));
        }
    }
    // The pause starts once the tenth send has failed, so wait for the
    // element to count it.
    let deadline = Instant::now() + Duration::from_secs(SEND_DEADLINE_SECONDS);
    while share.fail_count() < 10 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MILLISECONDS));
    }
    drop(pipeline);
    drop(share);

    assert_eq!(refused.load(Ordering::SeqCst), 10);
    let records = logged();
    assert_eq!(records.len(), 11, "{records:?}");
    for record in &records[..10] {
        assert!(
            record.starts_with("WARN Usage sharing failed"),
            "{records:?}"
        );
    }
    assert_eq!(
        records[10],
        "WARN Usage sharing paused for 2 seconds after failed sends. Usage data is \
         still queued, up to the maximum queue size, and sending starts again when the \
         pause ends."
    );
}

/// usage-sharing-element.md, "Request detail": "When failures occur, the
/// status code and content of the response MUST be logged to aid
/// troubleshooting." So each refused send gives a warning with the status
/// and the content, not only the first in a minute. Three failures are fewer
/// than the ten that pause sending by default.
#[test]
fn every_refused_send_logs_its_status_and_content() {
    let _serial = start();
    let (url, requests) = start_refusing_server();
    let share = Arc::new(ShareUsageElement::new(
        ShareUsageConfig::builder()
            .share_usage_url(url.clone())
            .minimum_entries_per_message(1)
            .take_timeout(Duration::ZERO)
            .build(),
    ));
    let pipeline = Pipeline::builder()
        .add_element(Arc::clone(&share) as Arc<dyn FlowElement>)
        .build()
        .unwrap();

    const SEND_DEADLINE_SECONDS: u64 = 10;
    const POLL_INTERVAL_MILLISECONDS: u64 = 25;
    for n in 1..=3 {
        let evidence = Evidence::builder()
            .add("header.user-agent", format!("refused-agent-{n}"))
            .build();
        pipeline.create_flow_data_with(evidence).process().unwrap();
        let deadline = Instant::now() + Duration::from_secs(SEND_DEADLINE_SECONDS);
        while share.fail_count() < n && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MILLISECONDS));
        }
    }
    drop(pipeline);
    drop(share);

    assert_eq!(requests.load(Ordering::SeqCst), 3);
    let records = logged();
    assert_eq!(records.len(), 3, "{records:?}");
    for record in &records {
        assert!(record.starts_with("WARN "), "{records:?}");
        assert!(record.contains(&url), "{records:?}");
        assert!(record.contains("403"), "{records:?}");
        assert!(record.contains("refused"), "{records:?}");
    }
}
