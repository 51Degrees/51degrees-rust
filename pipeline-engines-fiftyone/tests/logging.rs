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
use fiftyone_pipeline_engines_fiftyone::{
    SequenceElement, SetHeadersElement, ShareUsageConfig, ShareUsageElement,
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

fn process_sequence(raw: &str, suppress: bool) {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .suppress_process_exceptions(suppress)
        .build()
        .unwrap();
    let evidence = Evidence::builder().add("query.sequence", raw).build();
    pipeline.create_flow_data_with(evidence).process().unwrap();
}

#[test]
fn a_sequence_that_does_not_parse_logs_one_error() {
    for suppress in [false, true] {
        let _serial = start();
        process_sequence("abc", suppress);
        assert_eq!(
            logged(),
            vec![
                "ERROR Failed to increment the usage sequence number, because \
                 the sequence is not an integer."
            ],
            "suppress {suppress}"
        );
    }
}

#[test]
fn a_sequence_that_parses_logs_nothing() {
    let _serial = start();
    process_sequence("5", false);
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

    let deadline = Instant::now() + Duration::from_secs(10);
    while share.fail_count() == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
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
