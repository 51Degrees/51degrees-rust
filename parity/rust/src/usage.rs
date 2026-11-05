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

//! Usage sharing in the engines' pipeline, with the settings the .NET
//! producer gives it, and two sinks that keep each document in memory in
//! place of the network. The second also writes elements of its own, as the
//! .NET producer's `CustomShareUsage` does.

use std::collections::HashMap;
use std::error::Error;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use fiftyone_pipeline_engines_fiftyone::{
    ShareUsageConfig, UsageBatch, UsageRecord, UsageSink, UsageWriteError, UsageWriter,
};

/// More entries than any corpus holds, so every record waits in the queue
/// and goes in one batch when usage sharing is dropped.
const ENTRIES_PER_MESSAGE: usize = 10_000;

/// A header that is not shared.
const BLOCKED_HEADER: &str = "Referer";

/// A query string parameter that is shared, beside those whose names start
/// with `51D_`.
const INCLUDED_QUERY_PARAMETER: &str = "utm_source";

/// An evidence key and value that stop a request being shared, as a health
/// check's would.
const IGNORED_KEY: &str = "header.x-health-check";
const IGNORED_VALUE: &str = "true";

/// The custom sink's elements before and after each record.
const BEFORE_NAME: &str = "Before";
const BEFORE_TEXT: &str = "a & <b> \"c\" é中";
const AFTER_NAME: &str = "After";
const AFTER_PREFIX: &str = "Sequence ";

/// The product and Flow Element names the custom sink writes in place of the
/// pipeline's.
const PRODUCT: &str = "Parity";
const FLOW_ELEMENTS: &[&str] = &["Custom element 1", "Custom element 2"];

/// What starts and ends each record in a document.
const RECORD_START: &str = "<Device>";
const RECORD_END: &str = "</Device>";

/// A record's session id, written with text or empty.
const SESSION_ID_START: &str = "<SessionId>";
const SESSION_ID_END: &str = "</SessionId>";
const SESSION_ID_EMPTY: &str = "<SessionId />";

/// Usage sharing's settings, the same as the .NET producer's.
pub fn config() -> ShareUsageConfig {
    ShareUsageConfig::builder()
        .share_percentage(1.0)
        .minimum_entries_per_message(ENTRIES_PER_MESSAGE)
        .maximum_queue_size(ENTRIES_PER_MESSAGE)
        .repeat_evidence_interval(Duration::ZERO)
        .blocked_http_headers([BLOCKED_HEADER])
        .included_query_string_parameters(Some([INCLUDED_QUERY_PARAMETER]))
        .ignore_flow_data_evidence_filter([(IGNORED_KEY, IGNORED_VALUE)])
        .build()
}

/// Keeps each document usage sharing delivers.
#[derive(Default)]
pub struct UsageCapture {
    documents: Mutex<Vec<String>>,
}

impl UsageCapture {
    /// Each record's text as usage sharing wrote it, by its session id. The
    /// records are cut from the document text, as the .NET producer cuts
    /// them, so nothing is written again.
    pub fn records_by_session(&self) -> HashMap<String, String> {
        let documents = self
            .documents
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut records = HashMap::new();
        for document in documents.iter() {
            let mut rest = document.as_str();
            while let Some(start) = rest.find(RECORD_START) {
                let Some(length) = rest[start..].find(RECORD_END) else {
                    break;
                };
                let end = start + length + RECORD_END.len();
                let record = &rest[start..end];
                if let Some(session_id) = session_id(record) {
                    records.insert(session_id, record.to_owned());
                }
                rest = &rest[end..];
            }
        }
        records
    }
}

impl UsageSink for UsageCapture {
    fn receive(&self, batch: UsageBatch) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.documents
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(batch.document().to_owned());
        Ok(())
    }
}

/// Keeps each document as [`UsageCapture`] does, and writes an element before
/// and after each record and its own product and Flow Element names.
#[derive(Default)]
pub struct CustomUsageCapture {
    capture: UsageCapture,
}

impl CustomUsageCapture {
    /// As [`UsageCapture::records_by_session`].
    pub fn records_by_session(&self) -> HashMap<String, String> {
        self.capture.records_by_session()
    }
}

impl UsageSink for CustomUsageCapture {
    fn receive(&self, batch: UsageBatch) -> Result<(), Box<dyn Error + Send + Sync>> {
        self.capture.receive(batch)
    }

    fn write_before_record(
        &self,
        writer: &mut UsageWriter<'_>,
        _record: &UsageRecord,
    ) -> Result<(), UsageWriteError> {
        writer.element(BEFORE_NAME, BEFORE_TEXT)
    }

    fn write_pipeline_info(
        &self,
        writer: &mut UsageWriter<'_>,
        _flow_elements: &[&str],
    ) -> Result<(), UsageWriteError> {
        writer.element("Product", PRODUCT)?;
        for name in FLOW_ELEMENTS {
            writer.element("FlowElement", name)?;
        }
        Ok(())
    }

    fn write_after_record(
        &self,
        writer: &mut UsageWriter<'_>,
        record: &UsageRecord,
    ) -> Result<(), UsageWriteError> {
        writer.element(AFTER_NAME, &format!("{AFTER_PREFIX}{}", record.sequence()))
    }
}

/// The session id a record holds, with the escapes of its text undone, or
/// `None` when it has no `SessionId` element.
fn session_id(record: &str) -> Option<String> {
    if let Some(start) = record.find(SESSION_ID_START) {
        let text = &record[start + SESSION_ID_START.len()..];
        let end = text.find(SESSION_ID_END)?;
        Some(
            text[..end]
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&"),
        )
    } else if record.contains(SESSION_ID_EMPTY) {
        Some(String::new())
    } else {
        None
    }
}
