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

//! End-to-end tests for the three 51Degrees flow elements.
//!
//! The usage-sharing test spins up a local [`tiny_http`] server to capture the
//! POSTed payload rather than reaching the real 51Degrees endpoint.

use std::any::Any;
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, PropertyValueType,
    Result, TypedKey,
};
use fiftyone_pipeline_engines_fiftyone::{
    SequenceElement, SetHeadersElement, ShareUsageConfig, ShareUsageElement,
};

// --------------------------------------------------------------------------
// Sequence element
// --------------------------------------------------------------------------

/// Run a pipeline of the Sequence element alone over `evidence`, with errors
/// suppressed or not, and give the flow data and what processing returned.
fn sequence_run(evidence: &[(&str, &str)], suppress: bool) -> (FlowData, Result<()>) {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .suppress_process_exceptions(suppress)
        .build()
        .unwrap();
    let mut builder = Evidence::builder();
    for (key, value) in evidence {
        builder = builder.add(*key, *value);
    }
    let mut data = pipeline.create_flow_data_with(builder.build());
    let result = data.process();
    (data, result)
}

/// True for a GUID in the 36 character lower case form with hyphens that
/// .NET's `Guid.ToString()` gives.
fn is_dotnet_guid(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(index, c)| match index {
            8 | 13 | 18 | 23 => c == '-',
            _ => matches!(c, '0'..='9' | 'a'..='f'),
        })
}

#[test]
fn sequence_generates_session_id_and_starts_at_one() {
    let (data, result) = sequence_run(&[], false);
    result.unwrap();

    let session_id = data
        .evidence()
        .get("query.session-id")
        .expect("a session id is added to the evidence");
    assert!(is_dotnet_guid(session_id), "{session_id}");
    assert_eq!(data.evidence().get("query.sequence"), Some("1"));
    let sequence = data.get(SequenceElement::KEY).unwrap();
    assert_eq!(sequence.session_id(), Some(session_id));
    assert_eq!(sequence.sequence(), Some(1));
    assert!(data.errors().is_empty());
}

#[test]
fn sequence_preserves_supplied_session_and_increments() {
    let (data, result) = sequence_run(
        &[("query.session-id", "abc-123"), ("query.sequence", "4")],
        false,
    );
    result.unwrap();

    assert_eq!(data.evidence().get("query.session-id"), Some("abc-123"));
    assert_eq!(data.evidence().get("query.sequence"), Some("5"));
    let sequence = data.get(SequenceElement::KEY).unwrap();
    assert_eq!(sequence.session_id(), Some("abc-123"));
    assert_eq!(sequence.sequence(), Some(5));
}

#[test]
fn an_empty_session_id_counts_as_absent() {
    // The specification's Process section says a session id that is not a
    // non-empty string is not present, so a new one is added.
    let (data, result) = sequence_run(&[("query.session-id", "")], false);
    result.unwrap();

    let session_id = data
        .evidence()
        .get("query.session-id")
        .expect("a session id is in the evidence");
    assert!(is_dotnet_guid(session_id), "{session_id:?}");
    let sequence = data.get(SequenceElement::KEY).unwrap();
    assert_eq!(sequence.session_id(), Some(session_id));
    assert!(data.errors().is_empty());
}

#[test]
fn the_values_keep_the_callers_spelling_and_place() {
    // A key the caller gave keeps its spelling and place and takes the new
    // value, and a key the element adds goes last.
    let entries = |data: &FlowData| -> Vec<(String, String)> {
        data.evidence()
            .entries()
            .map(|entry| (entry.key_as_given().to_owned(), entry.value().to_owned()))
            .collect()
    };

    let (data, result) = sequence_run(
        &[
            ("Query.Sequence", "4"),
            ("header.user-agent", "agent"),
            ("Query.Session-Id", "abc"),
        ],
        false,
    );
    result.unwrap();
    assert_eq!(
        entries(&data),
        [
            ("Query.Sequence".to_owned(), "5".to_owned()),
            ("header.user-agent".to_owned(), "agent".to_owned()),
            ("Query.Session-Id".to_owned(), "abc".to_owned()),
        ]
    );

    let (data, result) = sequence_run(&[("Query.Sequence", "4")], false);
    result.unwrap();
    let entries = entries(&data);
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(entries[0], ("Query.Sequence".to_owned(), "5".to_owned()));
    assert_eq!(entries[1].0, "query.session-id");
    assert!(is_dotnet_guid(&entries[1].1), "{entries:?}");
}

#[test]
fn sequences_follow_the_specification() {
    // The sequence sent and the sequence after. Only a string of the digits
    // 0 to 9 from 0 up to 2,147,483,646 is read, and anything else counts as
    // absent, giving 1 with no error.
    let cases: [(Option<&str>, i64); 20] = [
        (None, 1),
        (Some("5"), 6),
        (Some("0"), 1),
        (Some("0005"), 6),
        (Some("2147483646"), 2147483647),
        (Some("2147483647"), 1),
        (Some("3000000000"), 1),
        (Some("99999999999999999999999"), 1),
        (Some("abc"), 1),
        (Some(" 5"), 1),
        (Some("5 "), 1),
        (Some("+5"), 1),
        (Some("-2"), 1),
        (Some("5\0"), 1),
        (Some("-2147483648"), 1),
        (Some(""), 1),
        (Some("1.5"), 1),
        (Some("\u{663}"), 1),
        (Some("\u{a0}5"), 1),
        (Some("5e2"), 1),
    ];
    for suppress in [false, true] {
        for (sent, sequence) in cases {
            let evidence: Vec<(&str, &str)> = sent
                .map(|sent| ("query.sequence", sent))
                .into_iter()
                .collect();
            let (data, result) = sequence_run(&evidence, suppress);
            let case = format!("{sent:?} with suppress {suppress}");

            result.unwrap_or_else(|e| panic!("{case}: {e}"));
            assert!(data.errors().is_empty(), "{case}");
            let session_id = data.evidence().get("query.session-id").unwrap_or_default();
            assert!(is_dotnet_guid(session_id), "{case}: {session_id}");
            let after = sequence.to_string();
            assert_eq!(
                data.evidence().get("query.sequence"),
                Some(after.as_str()),
                "{case}"
            );
            let held = data.get(SequenceElement::KEY).expect("sequence data");
            assert_eq!(held.session_id(), Some(session_id), "{case}");
            assert_eq!(held.sequence(), Some(sequence), "{case}");
        }
    }
}

/// Records the session id and sequence the evidence holds when it runs.
struct EvidenceReader {
    filter: EvidenceKeyFilterWhitelist,
    seen: Mutex<Vec<(Option<String>, Option<String>)>>,
}

impl EvidenceReader {
    fn new() -> Self {
        EvidenceReader {
            filter: EvidenceKeyFilterWhitelist::new(["query.session-id", "query.sequence"]),
            seen: Mutex::new(Vec::new()),
        }
    }
}

impl FlowElement for EvidenceReader {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let read = |key: &str| data.evidence().get(key).map(str::to_owned);
        let seen = (read("query.session-id"), read("query.sequence"));
        self.seen.lock().unwrap().push(seen);
        Ok(())
    }
    fn data_key(&self) -> &str {
        "reader"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

#[test]
fn a_later_element_reads_the_values_from_the_evidence() {
    let reader = Arc::new(EvidenceReader::new());
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::clone(&reader) as Arc<dyn FlowElement>)
        .build()
        .unwrap();

    for (sent, expected) in [(None, "1"), (Some("4"), "5")] {
        let mut evidence = Evidence::builder();
        if let Some(sent) = sent {
            evidence = evidence.add("query.sequence", sent);
        }
        let mut data = pipeline.create_flow_data_with(evidence.build());
        data.process().unwrap();

        let (session_id, sequence) = reader.seen.lock().unwrap().pop().expect("the reader ran");
        let created = data
            .get(SequenceElement::KEY)
            .and_then(|sequence| sequence.session_id());
        assert_eq!(session_id.as_deref(), created, "{sent:?}");
        assert_eq!(sequence.as_deref(), Some(expected), "{sent:?}");
    }
}

// --------------------------------------------------------------------------
// Set headers element
// --------------------------------------------------------------------------

/// Element data for the fake device-detection element below.
struct FakeDeviceData(MapElementData);

impl ElementData for FakeDeviceData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        self.0.get(name)
    }
    fn keys(&self) -> Vec<String> {
        self.0.keys()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A stand-in engine that publishes two `SetHeader*` properties and populates
/// them, so the set-headers element has something to scan and read.
struct FakeDeviceElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl FakeDeviceElement {
    const KEY: TypedKey<FakeDeviceData> = TypedKey::new("device");

    fn new() -> Self {
        FakeDeviceElement {
            filter: EvidenceKeyFilterWhitelist::new(["header.user-agent"]),
            properties: vec![
                PropertyMetaData::new(
                    "SetHeaderBrowserAccept-CH",
                    "device",
                    PropertyValueType::String,
                ),
                PropertyMetaData::new(
                    "SetHeaderHardwareAccept-CH",
                    "device",
                    PropertyValueType::String,
                ),
                PropertyMetaData::new("IsMobile", "device", PropertyValueType::Bool),
            ],
        }
    }
}

impl FlowElement for FakeDeviceElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.get_or_add(Self::KEY, || {
            FakeDeviceData(
                MapElementData::new()
                    .set(
                        "SetHeaderBrowserAccept-CH",
                        "SEC-CH-UA,SEC-CH-UA-Full-Version",
                    )
                    .set("SetHeaderHardwareAccept-CH", "SEC-CH-UA, SEC-CH-UA-Mobile")
                    .set("IsMobile", true),
            )
        })?;
        Ok(())
    }
    fn data_key(&self) -> &str {
        "device"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

#[test]
fn set_headers_merges_comma_separated_values_per_header() {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(FakeDeviceElement::new()))
        .add_element(Arc::new(SetHeadersElement::new()))
        .build()
        .unwrap();

    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    let headers = data.get(SetHeadersElement::KEY).unwrap().response_headers();

    // Both SetHeader properties target Accept-CH, so their distinct,
    // comma-separated values are merged with no duplicates.
    let accept_ch = headers.get("Accept-CH").expect("Accept-CH should be set");
    let values: Vec<&str> = accept_ch.split(',').collect();
    assert!(values.contains(&"SEC-CH-UA"));
    assert!(values.contains(&"SEC-CH-UA-Full-Version"));
    assert!(values.contains(&"SEC-CH-UA-Mobile"));
    // SEC-CH-UA appears in both properties but only once in the merged value.
    assert_eq!(values.iter().filter(|v| **v == "SEC-CH-UA").count(), 1);
}

#[test]
fn set_headers_leaves_out_an_unavailable_set_header_property() {
    // An engine lists the properties its data set was opened without as
    // unavailable, and .NET scans only the pipeline's available properties, so
    // an unavailable SetHeader property sets nothing even when the data holds
    // a value for it.
    let mut device = FakeDeviceElement::new();
    device.properties[1] = PropertyMetaData::new(
        "SetHeaderHardwareAccept-CH",
        "device",
        PropertyValueType::String,
    )
    .with_available(false);
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(device))
        .add_element(Arc::new(SetHeadersElement::new()))
        .build()
        .unwrap();

    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    let headers = data.get(SetHeadersElement::KEY).unwrap().response_headers();
    assert_eq!(
        headers.get("Accept-CH").map(String::as_str),
        Some("SEC-CH-UA,SEC-CH-UA-Full-Version")
    );
}

#[test]
fn set_headers_is_empty_when_no_set_header_properties_present() {
    // A pipeline with only the sequence element has no SetHeader* properties.
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(SetHeadersElement::new()))
        .build()
        .unwrap();

    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    let headers = data.get(SetHeadersElement::KEY).unwrap().response_headers();
    assert!(headers.is_empty());
}

/// A stand-in engine whose `SetHeader*` property metadata is not visible until
/// `reveal` is called, modelling a cloud engine that discovers its properties
/// lazily on its first process. Used to prove the set-headers element does not
/// freeze an empty scan.
struct LazyDeviceElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: std::sync::OnceLock<Vec<PropertyMetaData>>,
}

impl LazyDeviceElement {
    const KEY: TypedKey<FakeDeviceData> = TypedKey::new("device");

    fn new() -> Self {
        LazyDeviceElement {
            filter: EvidenceKeyFilterWhitelist::new(["header.user-agent"]),
            properties: std::sync::OnceLock::new(),
        }
    }

    /// Make the SetHeader property metadata visible, as a cloud engine would once
    /// its accessible-properties discovery completes.
    fn reveal(&self) {
        let _ = self.properties.set(vec![PropertyMetaData::new(
            "SetHeaderBrowserAccept-CH",
            "device",
            PropertyValueType::String,
        )]);
    }
}

impl FlowElement for LazyDeviceElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        // The header value is always stored; only the metadata is revealed late.
        data.get_or_add(Self::KEY, || {
            FakeDeviceData(MapElementData::new().set("SetHeaderBrowserAccept-CH", "SEC-CH-UA"))
        })?;
        Ok(())
    }
    fn data_key(&self) -> &str {
        "device"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        self.properties.get().map_or(&[], |p| p.as_slice())
    }
}

#[test]
fn set_headers_does_not_cache_an_empty_scan() {
    // Model the cloud ordering: on the first request the device element has not
    // yet revealed its SetHeader* property metadata, so the scan is empty. The
    // set-headers element must not cache that empty result. Once the metadata is
    // revealed, a later request produces the Accept-CH header.
    let device = Arc::new(LazyDeviceElement::new());
    let pipeline = Pipeline::builder()
        .add_element(device.clone() as Arc<dyn FlowElement>)
        .add_element(Arc::new(SetHeadersElement::new()))
        .build()
        .unwrap();

    // First request: no SetHeader metadata visible yet, so no header is set.
    let mut first = pipeline.create_flow_data();
    first.process().unwrap();
    assert!(
        first
            .get(SetHeadersElement::KEY)
            .unwrap()
            .response_headers()
            .is_empty(),
        "no Accept-CH before the device metadata is discovered"
    );

    // The device engine discovers its properties.
    device.reveal();

    // Second request: the scan now finds the SetHeader property and sets the
    // header, proving the empty first scan was not cached.
    let mut second = pipeline.create_flow_data();
    second.process().unwrap();
    let headers = second
        .get(SetHeadersElement::KEY)
        .unwrap()
        .response_headers();
    assert_eq!(
        headers.get("Accept-CH").map(String::as_str),
        Some("SEC-CH-UA"),
        "Accept-CH is set once the metadata is revealed"
    );
}

/// An element that publishes `SetHeaderBrowserAccept-CH` with a fixed value
/// under its own data key.
struct HeaderElement {
    key: &'static str,
    value: &'static str,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl HeaderElement {
    fn new(key: &'static str, value: &'static str) -> Self {
        HeaderElement {
            key,
            value,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: vec![PropertyMetaData::new(
                "SetHeaderBrowserAccept-CH",
                key,
                PropertyValueType::String,
            )],
        }
    }
}

impl FlowElement for HeaderElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let value = self.value;
        data.get_or_add(TypedKey::<FakeDeviceData>::new(self.key), || {
            FakeDeviceData(MapElementData::new().set("SetHeaderBrowserAccept-CH", value))
        })?;
        Ok(())
    }
    fn data_key(&self) -> &str {
        self.key
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// An element that wraps another and passes every call to it.
struct Wrapped(Arc<dyn FlowElement>);

impl FlowElement for Wrapped {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        self.0.process(data)
    }
    fn data_key(&self) -> &str {
        self.0.data_key()
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.0.evidence_key_filter()
    }
    fn properties(&self) -> &[PropertyMetaData] {
        self.0.properties()
    }
    fn is_concurrent(&self) -> bool {
        self.0.is_concurrent()
    }
    fn type_name(&self) -> &'static str {
        self.0.type_name()
    }
    fn add_pipeline(&self, pipeline: &Arc<Pipeline>) -> Result<()> {
        self.0.add_pipeline(pipeline)
    }
}

/// An element that wraps another and passes on every call but
/// `add_pipeline`, so the wrapped element's own `add_pipeline` is never
/// called.
struct WrappedWithoutAddPipeline(Arc<dyn FlowElement>);

impl FlowElement for WrappedWithoutAddPipeline {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        self.0.process(data)
    }
    fn data_key(&self) -> &str {
        self.0.data_key()
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.0.evidence_key_filter()
    }
    fn properties(&self) -> &[PropertyMetaData] {
        self.0.properties()
    }
}

/// Set Headers on its own, wrapped by an element that passes every call on,
/// and wrapped by one that does not pass `add_pipeline` on.
fn set_headers_three_ways() -> [(&'static str, Arc<dyn FlowElement>); 3] {
    [
        ("plain", Arc::new(SetHeadersElement::new())),
        (
            "wrapped",
            Arc::new(Wrapped(Arc::new(SetHeadersElement::new()))),
        ),
        (
            "wrapped without add_pipeline",
            Arc::new(WrappedWithoutAddPipeline(
                Arc::new(SetHeadersElement::new()),
            )),
        ),
    ]
}

/// The Accept-CH value of one request through `pipeline`.
fn accept_ch(pipeline: &Arc<Pipeline>) -> Option<String> {
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    data.get(SetHeadersElement::KEY)
        .unwrap()
        .response_headers()
        .get("Accept-CH")
        .cloned()
}

#[test]
fn set_headers_keeps_each_pipelines_headers_apart() {
    for (how, shared) in set_headers_three_ways() {
        let a = Pipeline::builder()
            .add_element(Arc::new(FakeDeviceElement::new()))
            .add_element(Arc::clone(&shared))
            .build()
            .unwrap();
        let b = Pipeline::builder()
            .add_element(Arc::new(HeaderElement::new("test", "X-Test")))
            .add_element(Arc::clone(&shared))
            .build()
            .unwrap();
        let alone = Pipeline::builder()
            .add_element(Arc::new(FakeDeviceElement::new()))
            .add_element(Arc::new(SetHeadersElement::new()))
            .build()
            .unwrap();

        // B first, so a single kept scan would be B's.
        // Twice each, because a wrapper that does not pass add_pipeline on
        // has the pipeline's entry added by its first request.
        for _ in 0..2 {
            assert_eq!(accept_ch(&b).as_deref(), Some("X-Test"), "{how}");
        }
        for _ in 0..2 {
            let from_a = accept_ch(&a);
            assert!(from_a.is_some(), "{how}");
            assert!(!from_a.as_deref().unwrap().contains("X-Test"), "{how}");
            assert_eq!(from_a, accept_ch(&alone), "{how}");
        }
        assert_eq!(accept_ch(&b).as_deref(), Some("X-Test"), "{how}");
    }
}

/// A [`HeaderElement`] that counts the calls to `properties`, which Set
/// Headers makes when it scans a pipeline.
struct CountedHeaderElement {
    inner: HeaderElement,
    scans: AtomicUsize,
}

impl FlowElement for CountedHeaderElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        self.inner.process(data)
    }
    fn data_key(&self) -> &str {
        self.inner.data_key()
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.inner.evidence_key_filter()
    }
    fn properties(&self) -> &[PropertyMetaData] {
        self.scans.fetch_add(1, Ordering::Relaxed);
        self.inner.properties()
    }
}

#[test]
fn set_headers_stops_scanning_a_pipeline_once_its_scan_is_kept() {
    for (how, set_headers) in set_headers_three_ways() {
        let counted = Arc::new(CountedHeaderElement {
            inner: HeaderElement::new("test", "X-Test"),
            scans: AtomicUsize::new(0),
        });
        let pipeline = Pipeline::builder()
            .add_element(Arc::clone(&counted) as Arc<dyn FlowElement>)
            .add_element(set_headers)
            .build()
            .unwrap();
        // add_pipeline adds the pipeline's entry, so the first request keeps
        // its scan. Through a wrapper that does not pass add_pipeline on, the
        // first request adds the entry and the second keeps its scan.
        let first_kept = if how == "wrapped without add_pipeline" {
            2
        } else {
            1
        };
        for _ in 0..first_kept {
            assert_eq!(accept_ch(&pipeline).as_deref(), Some("X-Test"), "{how}");
        }
        let scans = counted.scans.load(Ordering::Relaxed);
        assert!(scans > 0, "{how}");
        for _ in 0..10 {
            assert_eq!(accept_ch(&pipeline).as_deref(), Some("X-Test"), "{how}");
        }
        assert_eq!(counted.scans.load(Ordering::Relaxed), scans, "{how}");
    }
}

#[test]
fn set_headers_forgets_a_pipeline_that_has_gone() {
    let shared = Arc::new(SetHeadersElement::new());
    for (key, value) in [("first", "X-First"), ("second", "X-Second")] {
        // Each pipeline is dropped at the end of its turn, and the next may be
        // given the same address, which must not bring back the old headers.
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(HeaderElement::new(key, value)))
            .add_element(Arc::clone(&shared) as Arc<dyn FlowElement>)
            .build()
            .unwrap();
        assert_eq!(accept_ch(&pipeline).as_deref(), Some(value));
    }
}

// --------------------------------------------------------------------------
// Share usage element
// --------------------------------------------------------------------------

/// Start a local HTTP server that records the decompressed body of every POST
/// it receives, replying 200. Returns the bound URL and the shared sink.
fn start_capture_server() -> (String, Arc<Mutex<Vec<String>>>, Arc<AtomicUsize>) {
    start_capture_server_answering(200, "")
}

/// Start a local HTTP server that records the decompressed body of every POST
/// it receives and answers each with `status` and `content`.
fn start_capture_server_answering(
    status: u16,
    content: &'static str,
) -> (String, Arc<Mutex<Vec<String>>>, Arc<AtomicUsize>) {
    let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
    let url = format!("http://{}/new.ashx", server.server_addr());
    let bodies: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let request_count = Arc::new(AtomicUsize::new(0));

    let server_thread = Arc::clone(&server);
    let bodies_thread = Arc::clone(&bodies);
    let count_thread = Arc::clone(&request_count);
    std::thread::spawn(move || {
        for mut request in server_thread.incoming_requests() {
            let mut raw = Vec::new();
            let _ = request.as_reader().read_to_end(&mut raw);

            // The body is gzip-compressed XML.
            let mut decoder = flate2::read::GzDecoder::new(&raw[..]);
            let mut xml = String::new();
            if decoder.read_to_string(&mut xml).is_ok() {
                bodies_thread.lock().unwrap().push(xml);
            }
            count_thread.fetch_add(1, Ordering::SeqCst);

            let response = tiny_http::Response::from_string(content).with_status_code(status);
            let _ = request.respond(response);
        }
    });

    (url, bodies, request_count)
}

#[test]
fn a_share_usage_element_gives_back_its_configuration_from_the_pipeline() {
    let config = ShareUsageConfig::builder()
        .share_percentage(0.25)
        .minimum_entries_per_message(7)
        .build();
    let share = Arc::new(ShareUsageElement::new(config.clone()));
    assert_eq!(format!("{:?}", share.config()), format!("{config:?}"));

    // A pipeline made in code holds no resource for the element, which is
    // reached through `into_any` instead.
    let pipeline = Pipeline::builder()
        .add_element(Arc::clone(&share) as Arc<dyn FlowElement>)
        .build()
        .unwrap();
    assert!(pipeline.resource::<ShareUsageElement>().is_none());
    let found = Arc::clone(&pipeline.flow_elements()[0])
        .into_any()
        .and_then(|element| element.downcast::<ShareUsageElement>().ok())
        .expect("the usage sharing element");
    assert!(Arc::ptr_eq(&found, &share));
    assert_eq!(found.config().share_percentage(), 0.25);
    assert_eq!(found.config().minimum_entries_per_message(), 7);
}

#[test]
fn share_usage_posts_gzipped_xml_on_flush() {
    let (url, bodies, request_count) = start_capture_server();

    let config = ShareUsageConfig::builder()
        .share_usage_url(url)
        .share_percentage(1.0)
        .build();
    let share = Arc::new(ShareUsageElement::new(config));

    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::clone(&share) as Arc<dyn FlowElement>)
        .build()
        .unwrap();

    // Process a few distinct requests so they are not deduplicated.
    for i in 0..3 {
        let evidence = Evidence::builder()
            .add("header.user-agent", format!("agent-{i}"))
            .add("server.client-ip", "198.51.100.7")
            .build();
        let mut data = pipeline.create_flow_data_with(evidence);
        data.process().unwrap();
    }

    // Dropping the element flushes the queue (even below the minimum batch) and
    // joins the background thread, so by the time both Arcs are gone the POST
    // has been sent.
    drop(pipeline);
    drop(share);

    // Allow the server thread to record the request.
    wait_until(Duration::from_secs(5), || {
        request_count.load(Ordering::SeqCst) >= 1
    });

    let captured = bodies.lock().unwrap();
    assert!(!captured.is_empty(), "expected at least one POST");
    let xml = &captured[0];
    assert!(xml.contains("<Devices>"), "missing Devices root: {xml}");
    assert!(xml.contains("<Device>"), "missing Device element: {xml}");
    assert!(
        xml.contains("agent-0") || xml.contains("agent-1") || xml.contains("agent-2"),
        "user-agent evidence missing: {xml}"
    );
    assert!(
        xml.contains("<ClientIP>198.51.100.7</ClientIP>"),
        "client ip missing: {xml}"
    );
    assert!(xml.contains("<SessionId>"), "session id missing: {xml}");
}

#[test]
fn share_usage_deduplicates_identical_requests() {
    let (url, bodies, request_count) = start_capture_server();

    let config = ShareUsageConfig::builder()
        .share_usage_url(url)
        .share_percentage(1.0)
        .build();
    let share = Arc::new(ShareUsageElement::new(config));

    let pipeline = Pipeline::builder()
        // No sequence element here, so the evidence is genuinely identical
        // across requests and the tracker can deduplicate it.
        .add_element(Arc::clone(&share) as Arc<dyn FlowElement>)
        .build()
        .unwrap();

    for _ in 0..5 {
        let evidence = Evidence::builder()
            .add("header.user-agent", "same-agent")
            .add("server.client-ip", "203.0.113.9")
            .build();
        let mut data = pipeline.create_flow_data_with(evidence);
        data.process().unwrap();
    }

    drop(pipeline);
    drop(share);

    wait_until(Duration::from_secs(5), || {
        request_count.load(Ordering::SeqCst) >= 1
    });

    let captured = bodies.lock().unwrap();
    // Identical evidence collapses to a single shared <Device>.
    let total_devices: usize = captured
        .iter()
        .map(|xml| xml.matches("<Device>").count())
        .sum();
    assert_eq!(
        total_devices, 1,
        "expected exactly one shared device, got {total_devices}"
    );
}

#[test]
fn share_usage_is_created_and_used_inside_an_async_runtime() {
    // A web server usually builds its pipeline inside its async runtime, where
    // building a blocking HTTP client panics, so the element must not build
    // one as it is created.
    let (url, bodies, request_count) = start_capture_server();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let share = Arc::new(ShareUsageElement::new(
            ShareUsageConfig::builder()
                .share_usage_url(url)
                .minimum_entries_per_message(1)
                .build(),
        ));
        let pipeline = Pipeline::builder()
            .add_element(Arc::clone(&share) as Arc<dyn FlowElement>)
            .build()
            .unwrap();
        pipeline
            .create_flow_data_with(
                Evidence::builder()
                    .add("header.user-agent", "async-agent")
                    .build(),
            )
            .process()
            .unwrap();
        wait_until(Duration::from_secs(5), || share.success_count() == 1);
        assert_eq!(share.success_count(), 1, "the message was sent");
        drop(pipeline);
        drop(share);
    });

    assert_eq!(request_count.load(Ordering::SeqCst), 1);
    assert!(bodies.lock().unwrap()[0].contains("async-agent"));
}

/// Log records from every test in this binary, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Keeps every log record in [`LOGGED`].
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        LOGGED
            .lock()
            .unwrap()
            .push(format!("{} {}", record.level(), record.args()));
    }

    fn flush(&self) {}
}

static LOGGER: TestLogger = TestLogger;

/// Send log records to [`LOGGED`]. Safe to call from every test.
fn capture_logs() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Debug);
}

#[test]
fn share_usage_counts_only_a_200_as_success_and_logs_the_answer() {
    capture_logs();
    for (status, content) in [(201, "created"), (500, "server busy")] {
        let (url, _bodies, request_count) = start_capture_server_answering(status, content);
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
            .add("header.user-agent", format!("agent-{status}"))
            .build();
        pipeline.create_flow_data_with(evidence).process().unwrap();

        wait_until(Duration::from_secs(5), || {
            share.fail_count() + share.success_count() > 0
        });
        assert_eq!(request_count.load(Ordering::SeqCst), 1);
        assert_eq!(share.success_count(), 0, "{status} is not a success");
        assert_eq!(share.fail_count(), 1);

        let logged = LOGGED.lock().unwrap().join("\n");
        assert!(
            logged.lines().any(|line| line.starts_with("WARN")
                && line.contains(&url)
                && line.contains(&status.to_string())
                && line.contains(content)),
            "the warning names the endpoint, the status and the content:\n{logged}"
        );
    }
}

/// A pipeline holding only `element`.
fn pipeline_of(element: Arc<dyn FlowElement>) -> Result<Arc<Pipeline>> {
    Pipeline::builder().add_element(element).build()
}

/// True if `result` is usage sharing refusing a second pipeline.
fn refused(result: Result<Arc<Pipeline>>) -> bool {
    matches!(
        result,
        Err(fiftyone_pipeline_core::Error::PipelineConfiguration { message })
            if message.contains("only one pipeline at a time")
    )
}

#[test]
fn share_usage_refuses_a_second_pipeline_while_the_first_is_alive() {
    const SEND_WAIT_SECONDS: u64 = 5;
    for wrap in [false, true] {
        let (url, bodies, _) = start_capture_server();
        let config = ShareUsageConfig::builder()
            .share_usage_url(url)
            .share_percentage(1.0)
            .minimum_entries_per_message(1)
            .build();
        let share = Arc::new(ShareUsageElement::new(config));
        let element: Arc<dyn FlowElement> = if wrap {
            Arc::new(Wrapped(Arc::clone(&share) as Arc<dyn FlowElement>))
        } else {
            Arc::clone(&share) as Arc<dyn FlowElement>
        };

        let first = pipeline_of(Arc::clone(&element)).unwrap();
        assert!(refused(pipeline_of(Arc::clone(&element))), "wrapped {wrap}");
        // The refusal leaves the first pipeline processing and sharing.
        first.create_flow_data().process().unwrap();
        wait_until(Duration::from_secs(SEND_WAIT_SECONDS), || {
            !bodies.lock().unwrap().is_empty()
        });
        assert_eq!(bodies.lock().unwrap().len(), 1, "wrapped {wrap}");

        drop(first);
        assert!(pipeline_of(element).is_ok(), "wrapped {wrap}");
    }
}

#[test]
fn share_usage_refuses_two_places_in_one_pipeline_and_stays_free() {
    let share = Arc::new(ShareUsageElement::with_defaults());
    let twice = Pipeline::builder()
        .add_element(Arc::clone(&share) as Arc<dyn FlowElement>)
        .add_element(Arc::clone(&share) as Arc<dyn FlowElement>)
        .build();
    match twice {
        Err(fiftyone_pipeline_core::Error::PipelineConfiguration { message }) => {
            assert!(
                message.contains("added to the same pipeline more than once"),
                "{message}"
            );
        }
        Err(other) => panic!("unexpected error {other:?}"),
        Ok(_) => panic!("the same pipeline twice was not refused"),
    }
    // The refused pipeline was dropped, so the element can be used again.
    assert!(pipeline_of(share).is_ok());
}

/// Poll `condition` until it returns true or the timeout elapses.
fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}
