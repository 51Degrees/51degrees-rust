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

//! The usage-sharing element.
//!
//! The usage-sharing element collects a filtered subset of the evidence from a
//! request, batches it into a GZip-compressed XML document and POSTs it to a
//! configurable 51Degrees endpoint on a background thread. It implements the
//! [usage-sharing-element specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/usage-sharing-element.md).
//!
//! # Architecture
//!
//! Processing follows the recommended producer/consumer design. The `process`
//! function does the minimum work on the request thread: apply the ignore
//! filter, roll the share dice, consult the repeat-evidence tracker, extract
//! the wanted evidence into a self-contained [`UsageRecord`], then hand it
//! to a bounded channel. A single background thread consumes the channel.
//! Once it holds the configured minimum entries it takes more until none
//! arrives within the take timeout or it holds twice the minimum, as the .NET
//! and Java pipelines do, then builds the XML and sends it.
//!
//! Usage sharing is expendable. A failure (a full queue, a send error, an
//! offline endpoint) is logged through the [`log`] facade and never disrupts
//! the rest of the pipeline, as the
//! [error-handling section](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/usage-sharing-element.md#error-handling)
//! requires. Repeated failures are logged at most once a minute, with a count
//! of the failures since the last message.
//!
//! # Transport and the `share-usage-transport` feature
//!
//! Actually sending anything over the network is gated behind the crate's
//! `share-usage-transport` Cargo feature, which is **off by default**. The
//! feature exists because the built-in transport uses a blocking `reqwest`
//! client, and `reqwest` does not build for `wasm32-wasip1`.
//!
//! - With the feature **off** (the default), the element is constructed with a
//!   no-op transport. It still samples, filters, batches and deduplicates
//!   evidence like any pipeline element, but each batch is dropped rather than
//!   transmitted, so **no connection to 51Degrees is ever opened**. This is the
//!   state of a plain `cargo build` of any crate that does not turn the feature
//!   on.
//! - With the feature **on**, [`ShareUsageElement::new`] and
//!   [`ShareUsageElement::with_defaults`] wire up the blocking `reqwest`
//!   transport that POSTs to the configured endpoint
//!   ([`SHARE_USAGE_DEFAULT_URL`](crate::constants::SHARE_USAGE_DEFAULT_URL) by
//!   default).
//!
//! Sending usage data therefore requires two opt-ins: the element must be
//! added to the pipeline, *and* `share-usage-transport` must be enabled. The
//! Device Detection and IP Intelligence facade crates make both by default,
//! through their `share-usage` feature. On a target where the built-in client
//! cannot run (such as `wasm32-wasip1`) the feature is left off and the
//! element runs with the no-op transport.
//!
//! # Receiving the records
//!
//! [`ShareUsageElement::with_sink`] hands each batch to a caller's
//! [`UsageSink`] in place of the built-in transport, whether or not the
//! feature is on. The sink gets the records and the document the built-in
//! transport would send, and can add its own elements to each record.

use std::net::UdpSocket;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
#[cfg(feature = "share-usage-transport")]
use std::sync::OnceLock;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use fiftyone_pipeline_core::constants::EVIDENCE_CLIENT_IP_KEY;
use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, FlowData, FlowElement, Pipeline, PropertyMetaData, Result,
};

use crate::constants::{
    EVIDENCE_SEQUENCE, EVIDENCE_SESSIONID, SHARE_USAGE_DEFAULT_ADD_TIMEOUT_MS,
    SHARE_USAGE_DEFAULT_ELEMENT_DATA_KEY, SHARE_USAGE_DEFAULT_MAX_QUEUE_SIZE,
    SHARE_USAGE_DEFAULT_MIN_ENTRIES_PER_MESSAGE,
    SHARE_USAGE_DEFAULT_REPEAT_EVIDENCE_INTERVAL_MINUTES, SHARE_USAGE_DEFAULT_SHARE_PERCENTAGE,
    SHARE_USAGE_DEFAULT_TAKE_TIMEOUT_MS, SHARE_USAGE_DEFAULT_URL,
};
use crate::evidence_filter::{EvidenceKeyFilterShareUsage, EvidenceKeyFilterShareUsageTracker};
use crate::sequence_element::{SequenceData, SequenceElement};
use crate::share_usage_tracker::ShareUsageTracker;
use crate::usage_record::{parse_sequence, write_document, StaticInfo, UsageRecord};
use crate::usage_sink::{UsageBatch, UsageSink};

/// The smallest value accepted for the minimum entries per message. A message
/// needs at least one record.
pub const MIN_ENTRIES_PER_MESSAGE_FLOOR: usize = 1;

/// A message holds at most this many times the minimum entries per message.
const MAXIMUM_ENTRIES_MULTIPLE: usize = 2;

/// How long the built-in transport waits for the endpoint to answer.
#[cfg(feature = "share-usage-transport")]
const HTTP_TIMEOUT_SECONDS: u64 = 30;

/// The most characters of an endpoint's error response that are logged.
#[cfg(feature = "share-usage-transport")]
const MAX_LOGGED_RESPONSE_CHARACTERS: usize = 1000;

/// How long a request waits between attempts to add to a full queue, while
/// the add timeout has not run out.
const ADD_RETRY_INTERVAL_MILLISECONDS: u64 = 1;

/// The shortest gap between two log messages about the same kind of failure.
const LOG_INTERVAL_MILLISECONDS: u64 = 60_000;

/// A public address used to ask the operating system which local address
/// routes to the internet. Connecting a UDP socket sends no packets.
const ROUTE_PROBE_V4: &str = "8.8.8.8:10002";

/// The IPv6 equivalent of [`ROUTE_PROBE_V4`], for hosts without IPv4.
const ROUTE_PROBE_V6: &str = "[2001:4860:4860::8888]:10002";

/// Configuration for the usage-sharing element.
///
/// Build one with [`ShareUsageConfig::builder`]. Every field has a default that
/// matches the specification, so the common case is
/// `ShareUsageConfig::builder().build()`.
#[derive(Debug, Clone)]
pub struct ShareUsageConfig {
    share_percentage: f64,
    minimum_entries_per_message: usize,
    maximum_queue_size: usize,
    add_timeout: Duration,
    take_timeout: Duration,
    repeat_evidence_interval: Duration,
    share_usage_url: String,
    blocked_http_headers: Vec<String>,
    included_query_string_parameters: Option<Vec<String>>,
    share_all_evidence: bool,
    ignore_flow_data_evidence_filter: Vec<(String, String)>,
}

impl ShareUsageConfig {
    /// Start building a configuration from the specification defaults.
    pub fn builder() -> ShareUsageConfigBuilder {
        ShareUsageConfigBuilder::new()
    }

    /// The approximate proportion of requests to share, `0.0..=1.0`.
    pub fn share_percentage(&self) -> f64 {
        self.share_percentage
    }

    /// The number of entries accumulated before a message is sent. A message
    /// holds up to twice this number.
    pub fn minimum_entries_per_message(&self) -> usize {
        self.minimum_entries_per_message
    }

    /// The maximum number of queued entries.
    pub fn maximum_queue_size(&self) -> usize {
        self.maximum_queue_size
    }

    /// The endpoint usage data is sent to.
    pub fn share_usage_url(&self) -> &str {
        &self.share_usage_url
    }

    /// The repeat-evidence interval (the dedup sliding-window length).
    pub fn repeat_evidence_interval(&self) -> Duration {
        self.repeat_evidence_interval
    }

    /// The evidence key and value pairs that stop a request being shared. Keys
    /// are lowercase.
    pub fn ignore_flow_data_evidence_filter(&self) -> &[(String, String)] {
        &self.ignore_flow_data_evidence_filter
    }
}

impl Default for ShareUsageConfig {
    fn default() -> Self {
        ShareUsageConfigBuilder::new().build()
    }
}

/// Builder for [`ShareUsageConfig`].
#[derive(Debug, Clone)]
pub struct ShareUsageConfigBuilder {
    config: ShareUsageConfig,
}

impl ShareUsageConfigBuilder {
    /// Create a builder primed with the specification defaults.
    pub fn new() -> Self {
        ShareUsageConfigBuilder {
            config: ShareUsageConfig {
                share_percentage: SHARE_USAGE_DEFAULT_SHARE_PERCENTAGE,
                minimum_entries_per_message: SHARE_USAGE_DEFAULT_MIN_ENTRIES_PER_MESSAGE,
                maximum_queue_size: SHARE_USAGE_DEFAULT_MAX_QUEUE_SIZE,
                add_timeout: Duration::from_millis(SHARE_USAGE_DEFAULT_ADD_TIMEOUT_MS),
                take_timeout: Duration::from_millis(SHARE_USAGE_DEFAULT_TAKE_TIMEOUT_MS),
                repeat_evidence_interval: Duration::from_secs(
                    SHARE_USAGE_DEFAULT_REPEAT_EVIDENCE_INTERVAL_MINUTES * 60,
                ),
                share_usage_url: SHARE_USAGE_DEFAULT_URL.to_owned(),
                blocked_http_headers: Vec::new(),
                included_query_string_parameters: Some(Vec::new()),
                share_all_evidence: false,
                ignore_flow_data_evidence_filter: Vec::new(),
            },
        }
    }

    /// Set the share percentage (`0.0..=1.0`). Values outside the range are
    /// clamped. Each request is in the sample with this probability. Returns
    /// `self` for chaining.
    pub fn share_percentage(mut self, share_percentage: f64) -> Self {
        self.config.share_percentage = share_percentage.clamp(0.0, 1.0);
        self
    }

    /// Set the minimum entries per message. Values below
    /// [`MIN_ENTRIES_PER_MESSAGE_FLOOR`] are raised to it. A message is sent
    /// once it holds this many entries and no more arrive within the take
    /// timeout, or once it holds twice this many. Returns `self` for chaining.
    pub fn minimum_entries_per_message(mut self, minimum_entries_per_message: usize) -> Self {
        self.config.minimum_entries_per_message =
            minimum_entries_per_message.max(MIN_ENTRIES_PER_MESSAGE_FLOOR);
        self
    }

    /// Set the maximum queue size. Returns `self` for chaining.
    pub fn maximum_queue_size(mut self, maximum_queue_size: usize) -> Self {
        self.config.maximum_queue_size = maximum_queue_size.max(1);
        self
    }

    /// Set how long a request waits for room when the queue is full. The
    /// request's data is discarded if no room appears in that time. A timeout
    /// too long to add to the clock, such as `Duration::MAX`, waits until there
    /// is room. Returns `self` for chaining.
    pub fn add_timeout(mut self, add_timeout: Duration) -> Self {
        self.config.add_timeout = add_timeout;
        self
    }

    /// Set how long the background thread waits for another entry once a
    /// message holds the minimum entries per message. The message is sent when
    /// none arrives in that time, or when it holds twice the minimum, as the
    /// .NET and Java pipelines do. Below the minimum the thread waits with no
    /// time limit, so an idle element uses no processor time. Returns `self`
    /// for chaining.
    pub fn take_timeout(mut self, take_timeout: Duration) -> Self {
        self.config.take_timeout = take_timeout;
        self
    }

    /// Set the repeat-evidence interval (the dedup window). Returns `self` for
    /// chaining.
    pub fn repeat_evidence_interval(mut self, interval: Duration) -> Self {
        self.config.repeat_evidence_interval = interval;
        self
    }

    /// Set the endpoint to send usage data to. Returns `self` for chaining.
    pub fn share_usage_url(mut self, url: impl Into<String>) -> Self {
        self.config.share_usage_url = url.into();
        self
    }

    /// Set the HTTP headers that must not be shared. The `cookie` header is
    /// always blocked regardless of this list. Returns `self` for chaining.
    pub fn blocked_http_headers<I, S>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.config.blocked_http_headers = headers.into_iter().map(Into::into).collect();
        self
    }

    /// Set the query string parameters to share (in addition to any starting
    /// with `51d_`). Passing `None` shares every query parameter. Returns
    /// `self` for chaining.
    pub fn included_query_string_parameters<I, S>(mut self, params: Option<I>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.config.included_query_string_parameters =
            params.map(|p| p.into_iter().map(Into::into).collect());
        self
    }

    /// Share every evidence value, ignoring the blocked-header and query
    /// filters. Returns `self` for chaining.
    pub fn share_all_evidence(mut self, share_all: bool) -> Self {
        self.config.share_all_evidence = share_all;
        self
    }

    /// Set evidence key and value pairs that stop a request being shared. A
    /// request whose evidence holds any of the keys with exactly that value is
    /// not shared. Keys match in any case and values match exactly. Returns
    /// `self` for chaining.
    pub fn ignore_flow_data_evidence_filter<I, K, V>(mut self, entries: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: Into<String>,
    {
        self.config.ignore_flow_data_evidence_filter = entries
            .into_iter()
            .map(|(key, value)| (key.as_ref().to_lowercase(), value.into()))
            .collect();
        self
    }

    /// Produce the configuration, raising the queue size so it can hold at
    /// least one message.
    pub fn build(mut self) -> ShareUsageConfig {
        self.config.minimum_entries_per_message = self
            .config
            .minimum_entries_per_message
            .max(MIN_ENTRIES_PER_MESSAGE_FLOOR);
        if self.config.maximum_queue_size < self.config.minimum_entries_per_message {
            self.config.maximum_queue_size = self.config.minimum_entries_per_message;
        }
        self.config
    }
}

impl Default for ShareUsageConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl StaticInfo {
    /// The values for this machine and build, gathered when the element is
    /// created.
    fn gather() -> Self {
        StaticInfo {
            core_version: env!("CARGO_PKG_VERSION").to_owned(),
            language_version: env!("FIFTYONE_RUSTC_VERSION").to_owned(),
            server_ip: server_ip(),
            platform: platform(),
        }
    }
}

/// The error a sink's `receive` gives.
type SinkError = Box<dyn std::error::Error + Send + Sync>;

/// The built-in transport, a blocking `reqwest` client posting the gzip
/// compressed document, which counts only a 200 answer as success. Compiled
/// only with the `share-usage-transport` feature, because reqwest does not
/// build for wasm32-wasip1.
///
/// The client is built by the first send, which runs on the background thread,
/// because building a blocking client inside an async runtime panics and an
/// element is often created inside one.
#[cfg(feature = "share-usage-transport")]
struct HttpUsageSink {
    client: OnceLock<std::result::Result<reqwest::blocking::Client, String>>,
    url: String,
}

#[cfg(feature = "share-usage-transport")]
impl HttpUsageSink {
    fn new(url: String) -> Self {
        HttpUsageSink {
            client: OnceLock::new(),
            url,
        }
    }

    /// The client, built on first use. A client that cannot be built makes
    /// every send fail with the reason, which is logged like any failed send.
    fn client(&self) -> std::result::Result<&reqwest::blocking::Client, String> {
        self.client
            .get_or_init(|| {
                reqwest::blocking::Client::builder()
                    .timeout(Duration::from_secs(HTTP_TIMEOUT_SECONDS))
                    .build()
                    .map_err(|e| format!("The HTTP client could not be built: {e}"))
            })
            .as_ref()
            .map_err(Clone::clone)
    }
}

#[cfg(feature = "share-usage-transport")]
impl UsageSink for HttpUsageSink {
    fn receive(&self, batch: UsageBatch) -> std::result::Result<(), SinkError> {
        let body = batch
            .gzip()
            .map_err(|e| format!("The message could not be compressed: {e}"))?;
        let response = self
            .client()?
            .post(&self.url)
            .header("content-encoding", "gzip")
            .header("content-type", "text/xml")
            .body(body)
            .send()
            .map_err(|e| format!("The request to {} failed: {e}", self.url))?;
        let status = response.status();
        if status == reqwest::StatusCode::OK {
            return Ok(());
        }
        let content = response.text().unwrap_or_default();
        let content: String = content
            .chars()
            .take(MAX_LOGGED_RESPONSE_CHARACTERS)
            .collect();
        Err(format!("{} answered {status} with {content:?}", self.url).into())
    }
}

/// The sink used when the built-in HTTP transport is not compiled in, which is
/// the default and the only option on `wasm32-wasip1`. The usage-sharing
/// element still runs and collects evidence, but each batch is dropped, so
/// nothing is sent to 51Degrees. Enable the `share-usage-transport` feature to
/// transmit over the built-in reqwest client.
#[cfg(not(feature = "share-usage-transport"))]
struct DiscardingSink;

#[cfg(not(feature = "share-usage-transport"))]
impl UsageSink for DiscardingSink {
    fn receive(&self, _batch: UsageBatch) -> std::result::Result<(), SinkError> {
        Ok(())
    }
}

/// Lets a repeated failure be logged at most once a minute.
///
/// Every call to [`LogThrottle::record`] counts one failure. When a message is
/// due, it returns the number of failures since the last message.
struct LogThrottle {
    start: Instant,
    /// Milliseconds after `start` of the last message plus one, or zero when
    /// nothing has been logged yet.
    last_logged: AtomicU64,
    failures: AtomicU64,
}

impl LogThrottle {
    fn new() -> Self {
        LogThrottle {
            start: Instant::now(),
            last_logged: AtomicU64::new(0),
            failures: AtomicU64::new(0),
        }
    }

    /// Count a failure. Returns the failures since the last message when a
    /// message is due now.
    fn record(&self) -> Option<u64> {
        self.failures.fetch_add(1, Ordering::Relaxed);
        let now = u64::try_from(self.start.elapsed().as_millis())
            .unwrap_or(u64::MAX - 1)
            .saturating_add(1);
        let last = self.last_logged.load(Ordering::Relaxed);
        if last != 0 && now.saturating_sub(last) < LOG_INTERVAL_MILLISECONDS {
            return None;
        }
        self.last_logged
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .ok()
            .map(|_| self.failures.swap(0, Ordering::Relaxed))
    }
}

/// Why a request's data did not reach the queue.
enum Discarded {
    /// The queue stayed full for the whole add timeout.
    QueueFull,
    /// The background thread is not running.
    Stopped,
}

/// Sends usage data to 51Degrees for analysis.
///
/// Construct one with [`ShareUsageElement::new`]. The background sending thread
/// is started on construction and stopped, after a final flush, when the
/// element is dropped.
///
/// One element can be in only one pipeline at a time. Building a second
/// pipeline with it fails while the first is still alive.
pub struct ShareUsageElement {
    filter: Arc<EvidenceKeyFilterShareUsage>,
    properties: Vec<PropertyMetaData>,
    tracker: ShareUsageTracker,
    config: ShareUsageConfig,
    sender: SyncSender<UsageRecord>,
    worker: Mutex<Option<JoinHandle<()>>>,
    /// The pipeline the element is in, so a second can be refused.
    pipeline: Mutex<Weak<Pipeline>>,
    success_count: Arc<AtomicU64>,
    fail_count: Arc<AtomicU64>,
    discarded: LogThrottle,
}

impl ShareUsageElement {
    /// The default element data key, `"shareusage"`.
    pub const DEFAULT_ELEMENT_DATA_KEY: &'static str = SHARE_USAGE_DEFAULT_ELEMENT_DATA_KEY;

    /// Create a usage-sharing element with the given configuration.
    ///
    /// Whether the element transmits depends on the `share-usage-transport`
    /// feature (off by default). With the feature on, this wires the built-in
    /// blocking `reqwest` transport posting to `config.share_usage_url`. With it
    /// off, the element is constructed with a no-op transport: it collects and
    /// batches evidence but sends nothing to 51Degrees. See the module-level
    /// documentation for the full rationale. To receive each batch instead,
    /// use [`with_sink`](Self::with_sink).
    ///
    /// The element can be created inside an async runtime, because the HTTP
    /// client is built on the element's own background thread.
    pub fn new(config: ShareUsageConfig) -> Self {
        // With the built-in transport, post over a blocking reqwest client.
        // Without it (for example on wasm32-wasip1) drop each batch so the
        // element still constructs and runs.
        #[cfg(feature = "share-usage-transport")]
        let sink: Box<dyn UsageSink> = Box::new(HttpUsageSink::new(config.share_usage_url.clone()));
        #[cfg(not(feature = "share-usage-transport"))]
        let sink: Box<dyn UsageSink> = Box::new(DiscardingSink);

        Self::with_boxed_sink(config, sink)
    }

    /// Create a usage-sharing element that hands each batch to `sink` in place
    /// of the built-in transport, with or without the `share-usage-transport`
    /// feature. The configuration's endpoint is not used.
    ///
    /// The sink's `receive` runs on the element's background thread, and a
    /// batch it accepts counts towards [`success_count`](Self::success_count)
    /// and one it refuses towards [`fail_count`](Self::fail_count).
    pub fn with_sink(config: ShareUsageConfig, sink: impl UsageSink + 'static) -> Self {
        Self::with_boxed_sink(config, Box::new(sink))
    }

    /// Create a usage-sharing element with the default configuration.
    ///
    /// As with [`new`](Self::new), whether it transmits depends on the
    /// `share-usage-transport` feature (off by default); without it the element
    /// collects evidence but sends nothing.
    pub fn with_defaults() -> Self {
        Self::new(ShareUsageConfig::default())
    }

    fn with_boxed_sink(config: ShareUsageConfig, sink: Box<dyn UsageSink>) -> Self {
        // Build the evidence filter from the configured settings. Used for both
        // the element filter and the tracker filter so the two cannot diverge.
        let make_filter = || {
            if config.share_all_evidence {
                EvidenceKeyFilterShareUsage::share_all()
            } else {
                EvidenceKeyFilterShareUsage::new(
                    config.blocked_http_headers.clone(),
                    config.included_query_string_parameters.clone(),
                )
            }
        };

        let filter = Arc::new(make_filter());

        // The tracker filter is built from the same settings but excludes the
        // session id and sequence so they cannot defeat deduplication.
        let tracker = ShareUsageTracker::new(
            config.repeat_evidence_interval,
            config
                .maximum_queue_size
                .max(SHARE_USAGE_DEFAULT_MAX_QUEUE_SIZE),
            Box::new(EvidenceKeyFilterShareUsageTracker::new(make_filter())),
        );

        let (tx, rx) = sync_channel::<UsageRecord>(config.maximum_queue_size);

        let success_count = Arc::new(AtomicU64::new(0));
        let fail_count = Arc::new(AtomicU64::new(0));

        let worker = Worker {
            receiver: rx,
            sink,
            config: config.clone(),
            static_info: StaticInfo::gather(),
            success_count: Arc::clone(&success_count),
            fail_count: Arc::clone(&fail_count),
            send_failures: LogThrottle::new(),
        };
        // Without a background thread nothing can be sent. The receiver is
        // dropped with the closure, so every later add is discarded as stopped
        // and logged, and the pipeline carries on.
        let worker = match std::thread::Builder::new()
            .name("fiftyone-share-usage".to_owned())
            .spawn(move || worker.run())
        {
            Ok(handle) => Some(handle),
            Err(e) => {
                log::error!(
                    "Usage sharing cannot send anything because its background \
                     thread did not start: {e}"
                );
                None
            }
        };

        ShareUsageElement {
            filter,
            properties: Vec::new(),
            tracker,
            config,
            sender: tx,
            worker: Mutex::new(worker),
            pipeline: Mutex::new(Weak::new()),
            success_count,
            fail_count,
            discarded: LogThrottle::new(),
        }
    }

    /// The number of payloads successfully sent. Intended for diagnostics and
    /// tests.
    pub fn success_count(&self) -> u64 {
        self.success_count.load(Ordering::Relaxed)
    }

    /// The number of payloads that failed to send. Intended for diagnostics and
    /// tests.
    pub fn fail_count(&self) -> u64 {
        self.fail_count.load(Ordering::Relaxed)
    }

    /// True if the evidence matches a pair in the ignore filter, so the
    /// request is not shared.
    fn is_ignored(&self, data: &FlowData) -> bool {
        self.config
            .ignore_flow_data_evidence_filter
            .iter()
            .any(|(key, value)| data.evidence().get(key) == Some(value.as_str()))
    }

    /// Decide whether this request is in the shared sample. Each request is
    /// included with the share percentage as its probability, using the
    /// calling thread's own random number generator.
    fn in_sample(&self) -> bool {
        let p = self.config.share_percentage;
        p >= 1.0 || (p > 0.0 && fastrand::f64() < p)
    }

    /// Copy what the record needs out of a request's flow data. The evidence
    /// is read in the order it was given, with each key spelled as given.
    fn extract(&self, data: &FlowData) -> UsageRecord {
        let mut record = UsageRecord::new(
            data.pipeline()
                .flow_elements()
                .iter()
                .map(|element| element.type_name())
                .collect(),
        );
        // The session id, sequence and client IP have elements of their own,
        // so they are not written again as evidence entries.
        let mut evidence_session_id = None;
        let mut evidence_sequence = None;
        for entry in data.evidence().entries() {
            let key = entry.key();
            if key.eq_ignore_ascii_case(EVIDENCE_CLIENT_IP_KEY) {
                record.client_ip = entry.value().to_owned();
            } else if key.eq_ignore_ascii_case(EVIDENCE_SESSIONID) {
                evidence_session_id = Some(entry.value());
            } else if key.eq_ignore_ascii_case(EVIDENCE_SEQUENCE) {
                evidence_sequence = Some(entry.value());
            } else if self.filter.include(key) {
                record.add_evidence(entry.key_as_given(), entry.value());
            }
        }

        // The Sequence element's values are the ones this request carries,
        // because it increments the sequence the client sent. Without one the
        // request's own evidence gives them. A sequence that is not a 32-bit
        // integer is written as 0.
        let sequence = data.get(SequenceElement::KEY);
        record.session_id = sequence
            .and_then(SequenceData::session_id)
            .or(evidence_session_id)
            .unwrap_or_default()
            .to_owned();
        record.sequence = match sequence.and_then(SequenceData::sequence) {
            Some(sequence) => i32::try_from(sequence).unwrap_or(0),
            None => evidence_sequence.and_then(parse_sequence).unwrap_or(0),
        };
        record
    }

    /// Log a discarded record, at most once a minute.
    fn log_discarded(&self, reason: Discarded) {
        let Some(count) = self.discarded.record() else {
            return;
        };
        match reason {
            Discarded::QueueFull => log::warn!(
                "Usage sharing discarded a record because its queue of {} was \
                 full, which means records arrive faster than they can be sent. \
                 Records discarded since the last report: {count}.",
                self.config.maximum_queue_size
            ),
            Discarded::Stopped => log::warn!(
                "Usage sharing discarded a record because its background thread \
                 is not running. Records discarded since the last report: {count}."
            ),
        }
    }
}

impl Default for ShareUsageElement {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl FlowElement for ShareUsageElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        if self.is_ignored(data) {
            return Ok(());
        }

        // Sampling first: a request not in the sample is never shared.
        if !self.in_sample() {
            return Ok(());
        }

        // Repeat-evidence deduplication: the tracker decides whether this
        // request's evidence is new enough to share.
        if !self.tracker.track(data) {
            return Ok(());
        }

        let usage = self.extract(data);

        // Hand the data to the background sender, waiting up to the add
        // timeout for room. Data that does not fit is discarded, which the
        // specification permits, and logged. Sharing must never fail the
        // pipeline.
        if let Err(reason) = add_to_queue(&self.sender, usage, self.config.add_timeout) {
            self.log_discarded(reason);
        }

        Ok(())
    }

    fn data_key(&self) -> &str {
        SHARE_USAGE_DEFAULT_ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.filter.as_ref()
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }

    fn is_concurrent(&self) -> bool {
        // This element runs a background sending thread.
        true
    }

    /// Refuses a second pipeline while the first is alive, matching the .NET
    /// element, which refuses a second at any time.
    fn add_pipeline(&self, pipeline: &Arc<Pipeline>) -> Result<()> {
        let mut current = self.pipeline.lock().unwrap_or_else(PoisonError::into_inner);
        if current.strong_count() > 0 {
            return Err(Error::configuration(
                "A ShareUsageElement can be in only one pipeline at a time, and \
                 this one is in a pipeline that is still alive. Give each \
                 pipeline its own ShareUsageElement. To build a pipeline from a \
                 facade's elements, build the facade with share_usage(false), \
                 keep the facade's pipeline, which for on-premise data runs the \
                 data file updates, and add a ShareUsageElement to the new \
                 pipeline.",
            ));
        }
        *current = Arc::downgrade(pipeline);
        Ok(())
    }
}

impl Drop for ShareUsageElement {
    fn drop(&mut self) {
        // Dropping the sender closes the channel, which the worker treats as a
        // shutdown signal: it sends any remaining queued data (even below the
        // minimum batch size) and then exits. We replace the held sender with a
        // disconnected one so the original is dropped here, then join the
        // worker so the final flush completes before the element goes away.
        let (dead_tx, _dead_rx) = sync_channel::<UsageRecord>(0);
        let live_tx = std::mem::replace(&mut self.sender, dead_tx);
        drop(live_tx);

        let handle = match self.worker.lock() {
            Ok(mut guard) => guard.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(handle) = handle {
            let _ = handle.join();
        }
    }
}

/// Add a record to the queue, waiting up to `timeout` for room while it is
/// full.
fn add_to_queue(
    sender: &SyncSender<UsageRecord>,
    mut usage: UsageRecord,
    timeout: Duration,
) -> std::result::Result<(), Discarded> {
    // A timeout too long to add to the clock, such as Duration::MAX, has no
    // deadline, as with the standard library's own timeouts.
    let deadline = Instant::now().checked_add(timeout);
    loop {
        match sender.try_send(usage) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Disconnected(_)) => return Err(Discarded::Stopped),
            Err(TrySendError::Full(returned)) => {
                let mut wait = Duration::from_millis(ADD_RETRY_INTERVAL_MILLISECONDS);
                if let Some(deadline) = deadline {
                    let now = Instant::now();
                    if now >= deadline {
                        return Err(Discarded::QueueFull);
                    }
                    wait = wait.min(deadline - now);
                }
                usage = returned;
                std::thread::sleep(wait);
            }
        }
    }
}

/// The background thread: what it reads from and where it delivers to.
struct Worker {
    receiver: Receiver<UsageRecord>,
    sink: Box<dyn UsageSink>,
    config: ShareUsageConfig,
    static_info: StaticInfo,
    success_count: Arc<AtomicU64>,
    fail_count: Arc<AtomicU64>,
    send_failures: LogThrottle,
}

impl Worker {
    /// The consumer loop. Below the minimum entries it waits for each record
    /// with no time limit, so an idle element uses no processor time. Once it
    /// holds the minimum it takes more until none arrives within the take
    /// timeout or it holds twice the minimum, then sends the message. Once the
    /// element is dropped and every queued record has been received, it sends
    /// what remains, even below the minimum, and exits.
    fn run(self) {
        let min_entries = self.config.minimum_entries_per_message;
        let max_entries = min_entries.saturating_mul(MAXIMUM_ENTRIES_MULTIPLE);
        let take_timeout = self.config.take_timeout;
        let mut batch: Vec<UsageRecord> = Vec::with_capacity(max_entries);

        // `recv` fails only when the channel is closed and empty.
        while let Ok(item) = self.receiver.recv() {
            batch.push(item);
            if batch.len() < min_entries {
                continue;
            }
            while batch.len() < max_entries {
                match self.receiver.recv_timeout(take_timeout) {
                    Ok(item) => batch.push(item),
                    Err(_) => break,
                }
            }
            self.send_batch(std::mem::take(&mut batch));
        }
        if !batch.is_empty() {
            self.send_batch(batch);
        }
    }

    /// Write the document and hand the batch to the sink, recording success
    /// or failure.
    fn send_batch(&self, batch: Vec<UsageRecord>) {
        if batch.is_empty() {
            return;
        }
        match self.deliver(batch) {
            Ok(()) => {
                self.success_count.fetch_add(1, Ordering::Relaxed);
            }
            Err(detail) => {
                // Expendable: a failure is counted and logged, never
                // propagated.
                self.fail_count.fetch_add(1, Ordering::Relaxed);
                if let Some(failures) = self.send_failures.record() {
                    log::warn!(
                        "Usage sharing failed to deliver a message. {detail}. \
                         Messages failed since the last report: {failures}."
                    );
                }
            }
        }
    }

    /// Write the `<Devices>` document for the batch and hand both to the sink.
    fn deliver(&self, records: Vec<UsageRecord>) -> std::result::Result<(), String> {
        let document = write_document(&records, &self.static_info, &*self.sink, utc_now_iso8601)
            .map_err(|e| format!("A record could not be written, because {e}"))?;
        self.sink
            .receive(UsageBatch::new(records, document))
            .map_err(|e| e.to_string())
    }
}

/// The address this host uses to reach the internet. The operating system
/// picks it when a UDP socket is connected to a public address, which sends
/// no packets. Empty when the host has no route out.
fn server_ip() -> String {
    for (local, probe) in [("0.0.0.0:0", ROUTE_PROBE_V4), ("[::]:0", ROUTE_PROBE_V6)] {
        let address = UdpSocket::bind(local)
            .and_then(|socket| socket.connect(probe).map(|()| socket))
            .and_then(|socket| socket.local_addr());
        match address {
            Ok(address) if !address.ip().is_unspecified() => return address.ip().to_string(),
            Ok(_) => {}
            Err(e) => log::debug!("Usage sharing found no local address routing to {probe}: {e}"),
        }
    }
    String::new()
}

/// The operating system's name and version, with the service pack where the
/// system reports one.
fn platform() -> String {
    match os_version() {
        Some(version) => format!("{} {version}", std::env::consts::OS),
        None => std::env::consts::OS.to_owned(),
    }
}

/// The Windows version and service pack, from `RtlGetVersion`, which reports
/// the real version whatever the program's manifest says.
#[cfg(windows)]
fn os_version() -> Option<String> {
    /// The `OSVERSIONINFOW` structure.
    #[repr(C)]
    struct OsVersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform_id: u32,
        service_pack: [u16; 128],
    }

    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(info: *mut OsVersionInfo) -> i32;
    }

    let mut info = OsVersionInfo {
        size: u32::try_from(std::mem::size_of::<OsVersionInfo>()).ok()?,
        major: 0,
        minor: 0,
        build: 0,
        platform_id: 0,
        service_pack: [0; 128],
    };
    // SAFETY: `info` is a writable OSVERSIONINFOW whose size field is set,
    // which is all RtlGetVersion requires. It always succeeds.
    if unsafe { RtlGetVersion(&mut info) } != 0 {
        return None;
    }
    let length = info
        .service_pack
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(info.service_pack.len());
    let service_pack = String::from_utf16_lossy(&info.service_pack[..length]);
    let version = format!("{}.{}.{}", info.major, info.minor, info.build);
    if service_pack.trim().is_empty() {
        Some(version)
    } else {
        Some(format!("{version} {}", service_pack.trim()))
    }
}

/// The kernel release, with the distribution's name from `os-release` where
/// there is one, for example `6.8.0-1015-azure (Ubuntu 22.04.4 LTS)`.
#[cfg(target_os = "linux")]
fn os_version() -> Option<String> {
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    let distribution = ["/etc/os-release", "/usr/lib/os-release"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| os_release_name(&text));
    match (kernel, distribution) {
        (Some(kernel), Some(distribution)) => Some(format!("{kernel} ({distribution})")),
        (kernel, distribution) => kernel.or(distribution),
    }
}

/// The macOS version, for example `14.5`.
#[cfg(target_os = "macos")]
fn os_version() -> Option<String> {
    let plist = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
    plist_string(&plist, "ProductVersion")
}

/// No version is known for other systems.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn os_version() -> Option<String> {
    None
}

/// The `PRETTY_NAME` value from an `os-release` file, without its quotes.
#[cfg(any(target_os = "linux", test))]
fn os_release_name(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("PRETTY_NAME="))
        .map(|value| value.trim().trim_matches('"').trim_matches('\'').to_owned())
        .filter(|value| !value.is_empty())
}

/// The `<string>` that follows `<key>key</key>` in a property list.
#[cfg(any(target_os = "macos", test))]
fn plist_string(plist: &str, key: &str) -> Option<String> {
    let after = plist.split(&format!("<key>{key}</key>")).nth(1)?;
    let start = after.find("<string>")? + "<string>".len();
    let end = start + after[start..].find("</string>")?;
    Some(after[start..end].trim().to_owned()).filter(|value| !value.is_empty())
}

/// The current UTC time formatted as `yyyy-MM-ddTHH:mm:ss`, as the payload
/// requires. Computed from the Unix epoch without pulling in a date library,
/// keeping the crate's dependency surface small.
fn utc_now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (year, month, day, hour, minute, second) = civil_from_unix(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}")
}

/// Convert seconds since the Unix epoch into a UTC civil date-time, using the
/// well-known Howard Hinnant days-from-civil inverse algorithm. Kept here so the
/// crate needs no date dependency for the single timestamp it formats.
fn civil_from_unix(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = (rem / 3_600) as u32;
    let minute = ((rem % 3_600) / 60) as u32;
    let second = (rem % 60) as u32;

    // days shift: algorithm counts from 0000-03-01.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };

    (year, m as u32, d as u32, hour, minute, second)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use fiftyone_pipeline_core::{Evidence, Pipeline};

    use super::*;
    use crate::constants::SHARE_USAGE_MAX_EVIDENCE_LENGTH;
    use crate::usage_record::{UsageWriteError, UsageWriter};

    /// A sink that keeps each batch, and fails with `failure` when one is
    /// set.
    #[derive(Clone, Default)]
    struct Capture {
        batches: Arc<Mutex<Vec<UsageBatch>>>,
        failure: Option<String>,
    }

    impl Capture {
        fn batches(&self) -> Vec<UsageBatch> {
            self.batches.lock().unwrap().clone()
        }

        /// Each batch's document.
        fn messages(&self) -> Vec<String> {
            self.batches()
                .iter()
                .map(|batch| batch.document().to_owned())
                .collect()
        }

        /// Every `<Device>` element across the captured messages.
        fn devices(&self) -> usize {
            self.messages()
                .iter()
                .map(|xml| xml.matches("<Device>").count())
                .sum()
        }
    }

    impl UsageSink for Capture {
        fn receive(&self, batch: UsageBatch) -> std::result::Result<(), SinkError> {
            self.batches.lock().unwrap().push(batch);
            match &self.failure {
                Some(failure) => Err(failure.clone().into()),
                None => Ok(()),
            }
        }
    }

    /// A sink that keeps each batch in a [`Capture`], writes an element
    /// before and after each record, and names its own product with no
    /// elements.
    struct Wrapping(Capture);

    impl UsageSink for Wrapping {
        fn receive(&self, batch: UsageBatch) -> std::result::Result<(), SinkError> {
            self.0.receive(batch)
        }

        fn write_before_record(
            &self,
            writer: &mut UsageWriter<'_>,
            record: &UsageRecord,
        ) -> std::result::Result<(), UsageWriteError> {
            writer.element("Before", record.session_id())
        }

        fn write_pipeline_info(
            &self,
            writer: &mut UsageWriter<'_>,
            _flow_elements: &[&str],
        ) -> std::result::Result<(), UsageWriteError> {
            writer.element("Product", "Pipeline.Custom")
        }

        fn write_after_record(
            &self,
            writer: &mut UsageWriter<'_>,
            _record: &UsageRecord,
        ) -> std::result::Result<(), UsageWriteError> {
            let server_ip = writer.server_ip();
            writer.element("After", server_ip)
        }
    }

    /// A sink that writes an element whose name cannot be an XML name, and
    /// counts the batches it is given.
    struct Misnamed(Arc<AtomicU64>);

    impl UsageSink for Misnamed {
        fn receive(&self, _batch: UsageBatch) -> std::result::Result<(), SinkError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn write_after_record(
            &self,
            writer: &mut UsageWriter<'_>,
            _record: &UsageRecord,
        ) -> std::result::Result<(), UsageWriteError> {
            writer.element("Two words", "x")
        }
    }

    /// A sink that reports when a batch arrives, then holds it until it is
    /// released, so a test can keep the queue full.
    struct Gate {
        entered: Mutex<mpsc::Sender<()>>,
        release: Mutex<mpsc::Receiver<()>>,
        sent: Arc<AtomicU64>,
    }

    impl UsageSink for Gate {
        fn receive(&self, _batch: UsageBatch) -> std::result::Result<(), SinkError> {
            let _ = self.entered.lock().unwrap().send(());
            // A time limit, so a failing test ends instead of waiting for a
            // release its panic skipped.
            let _ = self
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5));
            self.sent.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// Process each evidence set through a pipeline of `before` then a usage
    /// sharing element, then drop everything so the last message is sent.
    fn share(
        config: ShareUsageConfig,
        sink: impl UsageSink + 'static,
        before: Vec<Arc<dyn FlowElement>>,
        requests: Vec<Evidence>,
    ) {
        let element = Arc::new(ShareUsageElement::with_sink(config, sink));
        let mut builder = Pipeline::builder();
        for other in before {
            builder = builder.add_element(other);
        }
        let pipeline = builder
            .add_element(Arc::clone(&element) as Arc<dyn FlowElement>)
            .build()
            .unwrap();
        for evidence in requests {
            pipeline.create_flow_data_with(evidence).process().unwrap();
        }
        drop(pipeline);
        drop(element);
    }

    fn user_agent(value: &str) -> Evidence {
        Evidence::builder().add("header.user-agent", value).build()
    }

    /// Wait up to five seconds for `condition`.
    fn wait_for(condition: impl Fn() -> bool) {
        let start = Instant::now();
        while !condition() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn long_values_are_truncated_and_marked() {
        let capture = Capture::default();
        let long = "a".repeat(SHARE_USAGE_MAX_EVIDENCE_LENGTH * 2);
        share(
            ShareUsageConfig::default(),
            capture.clone(),
            Vec::new(),
            vec![user_agent(&long)],
        );
        let kept = "a".repeat(SHARE_USAGE_MAX_EVIDENCE_LENGTH);
        assert!(
            capture.messages()[0].contains(&format!(
                "<header Name=\"user-agent\">\
                 <![CDATA[[TRUNCATED BY USAGE SHARING] {kept}]]></header>"
            )),
            "the value is cut to the maximum length and marked"
        );
    }

    #[test]
    fn evidence_is_written_as_given_and_grouped_by_prefix() {
        let capture = Capture::default();
        share(
            ShareUsageConfig::default(),
            capture.clone(),
            Vec::new(),
            vec![Evidence::builder()
                .add("header.User-Agent", "agent")
                .add("query.51D_Test", "x")
                .add("server.client-ip", "192.0.2.7")
                .add("header.Accept-Language", "en")
                .build()],
        );
        let xml = &capture.messages()[0];
        assert!(
            xml.contains("<SessionId /><Sequence>0</Sequence>"),
            "no session id or sequence: {xml}"
        );
        assert!(xml.contains("<ClientIP>192.0.2.7</ClientIP>"), "{xml}");
        let end = "</Platform>";
        assert_eq!(
            &xml[xml.find(end).unwrap() + end.len()..],
            "<header Name=\"User-Agent\"><![CDATA[agent]]></header>\
             <header Name=\"Accept-Language\"><![CDATA[en]]></header>\
             <query Name=\"51D_Test\"><![CDATA[x]]></query></Device></Devices>"
        );
    }

    #[test]
    fn the_ignore_filter_stops_matching_requests() {
        let capture = Capture::default();
        let config = ShareUsageConfig::builder()
            .ignore_flow_data_evidence_filter([("Header.X-Test", "skip")])
            .build();
        let requests = vec![
            Evidence::builder()
                .add("header.user-agent", "one")
                .add("header.x-test", "skip")
                .build(),
            Evidence::builder()
                .add("header.user-agent", "two")
                .add("header.x-test", "Skip")
                .build(),
            user_agent("three"),
        ];
        share(config, capture.clone(), Vec::new(), requests);
        let xml = capture.messages().concat();
        assert!(
            !xml.contains("[CDATA[one]]"),
            "the exact match is not shared: {xml}"
        );
        assert!(xml.contains("[CDATA[two]]"), "values match exactly: {xml}");
        assert!(xml.contains("[CDATA[three]]"), "{xml}");
    }

    #[test]
    fn the_share_percentage_is_the_proportion_shared() {
        const SAMPLES: usize = 100_000;
        for percentage in [0.25, 0.4, 0.6, 0.75] {
            let element = ShareUsageElement::with_sink(
                ShareUsageConfig::builder()
                    .share_percentage(percentage)
                    .build(),
                Capture::default(),
            );
            fastrand::seed(7);
            let shared = (0..SAMPLES).filter(|_| element.in_sample()).count();
            let proportion = shared as f64 / SAMPLES as f64;
            assert!(
                (proportion - percentage).abs() < 0.01,
                "{percentage} shared {proportion}"
            );
        }
    }

    #[test]
    fn small_minimum_entries_are_kept() {
        for (asked, kept) in [(0, 1), (1, 1), (10, 10), (50, 50)] {
            let config = ShareUsageConfig::builder()
                .minimum_entries_per_message(asked)
                .build();
            assert_eq!(config.minimum_entries_per_message(), kept);
        }
    }

    #[test]
    fn a_message_is_sent_once_the_minimum_is_reached() {
        let capture = Capture::default();
        let element = Arc::new(ShareUsageElement::with_sink(
            ShareUsageConfig::builder()
                .minimum_entries_per_message(2)
                .build(),
            capture.clone(),
        ));
        let pipeline = Pipeline::builder()
            .add_element(Arc::clone(&element) as Arc<dyn FlowElement>)
            .build()
            .unwrap();
        for agent in ["one", "two"] {
            pipeline
                .create_flow_data_with(user_agent(agent))
                .process()
                .unwrap();
        }
        wait_for(|| element.success_count() > 0);
        assert_eq!(element.success_count(), 1, "two records make a message");
        assert_eq!(capture.devices(), 2);
    }

    #[test]
    fn a_message_takes_up_to_twice_the_minimum_within_the_take_timeout() {
        // A take timeout far longer than processing five requests takes, so
        // every record after the minimum arrives within it.
        const TAKE_TIMEOUT_SECONDS: u64 = 5;
        let capture = Capture::default();
        share(
            ShareUsageConfig::builder()
                .minimum_entries_per_message(2)
                .take_timeout(Duration::from_secs(TAKE_TIMEOUT_SECONDS))
                .build(),
            capture.clone(),
            Vec::new(),
            ["one", "two", "three", "four", "five"]
                .into_iter()
                .map(user_agent)
                .collect(),
        );
        let sizes: Vec<usize> = capture
            .messages()
            .iter()
            .map(|xml| xml.matches("<Device>").count())
            .collect();
        assert_eq!(sizes, [4, 1], "twice the minimum, then the rest");
    }

    #[test]
    fn a_full_queue_waits_for_the_add_timeout_then_discards() {
        const ADD_TIMEOUT_MILLISECONDS: u64 = 50;
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let sent = Arc::new(AtomicU64::new(0));
        let element = Arc::new(ShareUsageElement::with_sink(
            ShareUsageConfig::builder()
                .minimum_entries_per_message(1)
                .maximum_queue_size(1)
                .add_timeout(Duration::from_millis(ADD_TIMEOUT_MILLISECONDS))
                .build(),
            Gate {
                entered: Mutex::new(entered_tx),
                release: Mutex::new(release_rx),
                sent: Arc::clone(&sent),
            },
        ));
        let pipeline = Pipeline::builder()
            .add_element(Arc::clone(&element) as Arc<dyn FlowElement>)
            .build()
            .unwrap();
        let process = |agent: &str| {
            pipeline
                .create_flow_data_with(user_agent(agent))
                .process()
                .unwrap()
        };

        // The first record goes straight to the sink, which holds it. The
        // second fills the queue of one.
        process("one");
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        process("two");

        // The third waits for the add timeout, then is discarded.
        let start = Instant::now();
        process("three");
        assert!(
            start.elapsed() >= Duration::from_millis(ADD_TIMEOUT_MILLISECONDS),
            "returned after {:?}",
            start.elapsed()
        );

        for _ in 0..3 {
            let _ = release_tx.send(());
        }
        drop(pipeline);
        drop(element);
        assert_eq!(sent.load(Ordering::SeqCst), 2, "the third was discarded");
    }

    #[test]
    fn a_stopped_or_full_queue_reports_why() {
        let (tx, rx) = sync_channel::<UsageRecord>(1);
        assert!(add_to_queue(&tx, UsageRecord::new(Vec::new()), Duration::ZERO).is_ok());
        assert!(matches!(
            add_to_queue(&tx, UsageRecord::new(Vec::new()), Duration::ZERO),
            Err(Discarded::QueueFull)
        ));
        drop(rx);
        assert!(matches!(
            add_to_queue(&tx, UsageRecord::new(Vec::new()), Duration::ZERO),
            Err(Discarded::Stopped)
        ));
    }

    #[test]
    fn an_add_timeout_too_long_for_the_clock_waits_for_room() {
        let (tx, rx) = sync_channel::<UsageRecord>(1);
        assert!(add_to_queue(&tx, UsageRecord::new(Vec::new()), Duration::MAX).is_ok());

        // The queue of one is full, so the next add waits until it has room.
        let taker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            rx.recv().unwrap();
            rx
        });
        assert!(add_to_queue(&tx, UsageRecord::new(Vec::new()), Duration::MAX).is_ok());
        let rx = taker.join().unwrap();
        assert!(rx.try_recv().is_ok(), "the second record was queued");
    }

    #[test]
    fn a_request_is_shared_with_the_longest_add_timeout() {
        let capture = Capture::default();
        share(
            ShareUsageConfig::builder()
                .add_timeout(Duration::MAX)
                .build(),
            capture.clone(),
            Vec::new(),
            vec![user_agent("one")],
        );
        assert_eq!(capture.devices(), 1);
    }

    #[test]
    fn a_repeated_failure_is_logged_once_until_the_interval_passes() {
        let throttle = LogThrottle::new();
        assert_eq!(throttle.record(), Some(1));
        assert_eq!(throttle.record(), None);
        assert_eq!(throttle.record(), None);
        assert_eq!(throttle.failures.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_failed_send_is_counted() {
        let capture = Capture {
            failure: Some("the endpoint answered 500".to_owned()),
            ..Capture::default()
        };
        let element = Arc::new(ShareUsageElement::with_sink(
            ShareUsageConfig::builder()
                .minimum_entries_per_message(1)
                .build(),
            capture,
        ));
        let pipeline = Pipeline::builder()
            .add_element(Arc::clone(&element) as Arc<dyn FlowElement>)
            .build()
            .unwrap();
        pipeline
            .create_flow_data_with(user_agent("one"))
            .process()
            .unwrap();
        wait_for(|| element.fail_count() > 0);
        assert_eq!(element.fail_count(), 1);
        assert_eq!(element.success_count(), 0);
    }

    #[test]
    fn each_record_lists_the_elements_of_the_pipeline_that_made_it() {
        let capture = Capture::default();
        let element = Arc::new(ShareUsageElement::with_sink(
            ShareUsageConfig::default(),
            capture.clone(),
        ));
        // The first pipeline is dropped before the second is built, because
        // the element can be in only one pipeline at a time.
        let first = Pipeline::builder()
            .add_element(Arc::new(SequenceElement::new()))
            .add_element(Arc::clone(&element) as Arc<dyn FlowElement>)
            .build()
            .unwrap();
        first
            .create_flow_data_with(user_agent("first-agent"))
            .process()
            .unwrap();
        drop(first);
        let second = Pipeline::builder()
            .add_element(Arc::clone(&element) as Arc<dyn FlowElement>)
            .build()
            .unwrap();
        second
            .create_flow_data_with(user_agent("second-agent"))
            .process()
            .unwrap();
        drop(second);
        drop(element);

        let xml = capture.messages().concat();
        let device = |agent: &str| {
            xml.split("<Device>")
                .find(|device| device.contains(agent))
                .unwrap()
                .to_owned()
        };
        let sequence = "<FlowElement>fiftyone_pipeline_engines_fiftyone::\
                        sequence_element::SequenceElement</FlowElement>";
        let share = "<FlowElement>fiftyone_pipeline_engines_fiftyone::\
                     share_usage_element::ShareUsageElement</FlowElement>";
        let first = device("first-agent");
        assert!(first.contains(sequence) && first.contains(share), "{first}");
        let second = device("second-agent");
        assert!(
            !second.contains(sequence) && second.contains(share),
            "{second}"
        );
    }

    #[test]
    fn the_static_values_describe_this_machine_and_pipeline() {
        let capture = Capture::default();
        share(
            ShareUsageConfig::default(),
            capture.clone(),
            vec![Arc::new(SequenceElement::new())],
            vec![user_agent("one")],
        );
        let xml = &capture.messages()[0];
        let text = |name: &str| {
            if xml.contains(&format!("<{name} />")) {
                return String::new();
            }
            let start = xml.find(&format!("<{name}>")).unwrap() + name.len() + 2;
            let end = start + xml[start..].find(&format!("</{name}>")).unwrap();
            xml[start..end].to_owned()
        };

        assert_eq!(text("Language"), "rust");
        let version = text("LanguageVersion");
        assert!(
            version.split('.').count() >= 3
                && version
                    .split('.')
                    .take(2)
                    .all(|part| part.parse::<u32>().is_ok()),
            "the compiler version, not {version}"
        );
        let platform = text("Platform");
        assert!(platform.starts_with(std::env::consts::OS), "{platform}");
        let server_ip = text("ServerIP");
        assert!(
            server_ip.is_empty() || server_ip.parse::<std::net::IpAddr>().is_ok(),
            "{server_ip}"
        );
        for name in [
            "fiftyone_pipeline_engines_fiftyone::sequence_element::SequenceElement",
            "fiftyone_pipeline_engines_fiftyone::share_usage_element::ShareUsageElement",
        ] {
            assert!(
                xml.contains(&format!("<FlowElement>{name}</FlowElement>")),
                "{xml}"
            );
        }
    }

    #[test]
    fn the_sequence_elements_values_are_shared() {
        let capture = Capture::default();
        share(
            ShareUsageConfig::default(),
            capture.clone(),
            vec![Arc::new(SequenceElement::new())],
            vec![Evidence::builder()
                .add("header.user-agent", "one")
                .add(EVIDENCE_SESSIONID, "session")
                .add(EVIDENCE_SEQUENCE, "3")
                .build()],
        );
        let xml = &capture.messages()[0];
        assert!(xml.contains("<SessionId>session</SessionId>"), "{xml}");
        assert!(
            xml.contains("<Sequence>4</Sequence>"),
            "the incremented sequence: {xml}"
        );
    }

    #[test]
    fn without_a_sequence_element_the_evidence_values_are_shared() {
        let capture = Capture::default();
        share(
            ShareUsageConfig::default(),
            capture.clone(),
            Vec::new(),
            vec![Evidence::builder()
                .add("header.user-agent", "one")
                .add(EVIDENCE_SESSIONID, "session")
                .add(EVIDENCE_SEQUENCE, "3")
                .build()],
        );
        let xml = &capture.messages()[0];
        assert!(xml.contains("<SessionId>session</SessionId>"), "{xml}");
        assert!(xml.contains("<Sequence>3</Sequence>"), "{xml}");
    }

    #[test]
    fn a_sequence_that_is_not_a_32_bit_integer_is_written_as_0() {
        let capture = Capture::default();
        share(
            ShareUsageConfig::default(),
            capture.clone(),
            Vec::new(),
            ["2147483648", "five"]
                .into_iter()
                .map(|sequence| {
                    Evidence::builder()
                        .add("header.user-agent", sequence)
                        .add(EVIDENCE_SEQUENCE, sequence)
                        .build()
                })
                .collect(),
        );
        let xml = capture.messages().concat();
        assert_eq!(xml.matches("<Sequence>0</Sequence>").count(), 2, "{xml}");
    }

    #[test]
    fn a_sink_receives_the_records_and_the_document() {
        // An Arc of the sink, so the test keeps a handle to it.
        let capture = Arc::new(Capture::default());
        share(
            ShareUsageConfig::default(),
            Arc::clone(&capture),
            Vec::new(),
            vec![Evidence::builder()
                .add("header.User-Agent", "agent")
                .add(EVIDENCE_SESSIONID, "session")
                .add(EVIDENCE_SEQUENCE, "3")
                .add(EVIDENCE_CLIENT_IP_KEY, "192.0.2.7")
                .build()],
        );
        let batches = capture.batches();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].records().len(), 1);
        let record = &batches[0].records()[0];
        assert_eq!(record.session_id(), "session");
        assert_eq!(record.sequence(), 3);
        assert_eq!(record.client_ip(), "192.0.2.7");
        assert_eq!(
            record.flow_elements(),
            ["fiftyone_pipeline_engines_fiftyone::share_usage_element::ShareUsageElement"]
        );
        assert_eq!(
            record.evidence().collect::<Vec<_>>(),
            [("header", "User-Agent", "agent")]
        );
        let xml = batches[0].document();
        assert!(
            xml.starts_with("\u{FEFF}<?xml version=\"1.0\" encoding=\"utf-8\"?><Devices><Device>"),
            "{xml}"
        );
        assert!(
            xml.contains("<SessionId>session</SessionId><Sequence>3</Sequence>"),
            "{xml}"
        );
        assert!(xml.contains("<ClientIP>192.0.2.7</ClientIP>"), "{xml}");
    }

    #[test]
    fn a_sinks_elements_are_written_around_each_record() {
        let capture = Capture::default();
        share(
            ShareUsageConfig::default(),
            Arc::new(Wrapping(capture.clone())),
            Vec::new(),
            vec![Evidence::builder()
                .add("header.User-Agent", "agent")
                .add(EVIDENCE_SESSIONID, "session")
                .build()],
        );
        let xml = &capture.messages()[0];
        assert!(
            xml.contains("<Device><Before>session</Before><SessionId>session</SessionId>"),
            "{xml}"
        );
        assert!(
            xml.contains("</Version><Product>Pipeline.Custom</Product><Language>"),
            "the product is replaced and no FlowElement is written: {xml}"
        );
        let after = if xml.contains("<ServerIP />") {
            "<After />".to_owned()
        } else {
            let start = xml.find("<ServerIP>").unwrap() + "<ServerIP>".len();
            let end = xml.find("</ServerIP>").unwrap();
            format!("<After>{}</After>", &xml[start..end])
        };
        assert!(
            xml.ends_with(&format!(
                "<header Name=\"User-Agent\"><![CDATA[agent]]></header>\
                 {after}</Device></Devices>"
            )),
            "{xml}"
        );
    }

    #[test]
    fn a_sink_element_that_cannot_be_named_fails_the_batch() {
        let received = Arc::new(AtomicU64::new(0));
        let element = Arc::new(ShareUsageElement::with_sink(
            ShareUsageConfig::builder()
                .minimum_entries_per_message(1)
                .build(),
            Misnamed(Arc::clone(&received)),
        ));
        let pipeline = Pipeline::builder()
            .add_element(Arc::clone(&element) as Arc<dyn FlowElement>)
            .build()
            .unwrap();
        pipeline
            .create_flow_data_with(user_agent("one"))
            .process()
            .unwrap();
        wait_for(|| element.fail_count() > 0);
        assert_eq!(element.fail_count(), 1);
        assert_eq!(element.success_count(), 0);
        assert_eq!(
            received.load(Ordering::SeqCst),
            0,
            "the sink is not given the batch"
        );
    }

    #[test]
    fn the_distribution_name_is_read_from_os_release() {
        let text = "NAME=\"Ubuntu\"\nVERSION_ID=\"22.04\"\nPRETTY_NAME=\"Ubuntu 22.04.4 LTS\"\n";
        assert_eq!(os_release_name(text).as_deref(), Some("Ubuntu 22.04.4 LTS"));
        assert_eq!(
            os_release_name("PRETTY_NAME=Alpine").as_deref(),
            Some("Alpine")
        );
        assert_eq!(os_release_name("NAME=x"), None);
    }

    #[test]
    fn the_product_version_is_read_from_a_property_list() {
        let plist = "<dict>\n\t<key>ProductName</key>\n\t<string>macOS</string>\n\t\
                     <key>ProductVersion</key>\n\t<string>14.5</string>\n</dict>";
        assert_eq!(
            plist_string(plist, "ProductVersion").as_deref(),
            Some("14.5")
        );
        assert_eq!(plist_string(plist, "Missing"), None);
    }
}
