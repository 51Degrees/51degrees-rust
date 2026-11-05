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

//! Where the usage-sharing element delivers each batch of records.

use std::io::Write;
use std::sync::Arc;

use flate2::write::GzEncoder;
use flate2::Compression;

use crate::usage_record::{write_default_pipeline_info, UsageRecord, UsageWriteError, UsageWriter};

/// Receives each batch of usage records from a
/// [`ShareUsageElement`](crate::ShareUsageElement) made with
/// [`ShareUsageElement::with_sink`](crate::ShareUsageElement::with_sink), in
/// place of the built-in transport.
///
/// [`receive`](Self::receive) is called on the element's background thread,
/// one batch at a time. The other three methods let a sink add its own
/// elements to each record in the batch's document, and each writes nothing
/// of its own unless overridden.
///
/// A panic in any of the four methods fails that batch alone, as an error
/// from [`receive`](Self::receive) does, and the element goes on to the next
/// batch. A program built to abort on panic stops instead.
///
/// An `Arc` of a sink is a sink too, so a caller can keep a handle to it.
///
/// # Example
///
/// A sink that keeps each document and adds a `Host` element to the end of
/// each record.
///
/// ```
/// use std::sync::{Arc, Mutex};
///
/// use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
/// use fiftyone_pipeline_engines_fiftyone::{
///     ShareUsageConfig, ShareUsageElement, UsageBatch, UsageRecord, UsageSink,
///     UsageWriteError, UsageWriter,
/// };
///
/// #[derive(Default)]
/// struct Keep(Mutex<Vec<String>>);
///
/// impl UsageSink for Keep {
///     fn receive(
///         &self,
///         batch: UsageBatch,
///     ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
///         self.0.lock().unwrap().push(batch.document().to_owned());
///         Ok(())
///     }
///
///     fn write_after_record(
///         &self,
///         writer: &mut UsageWriter<'_>,
///         _record: &UsageRecord,
///     ) -> Result<(), UsageWriteError> {
///         writer.element("Host", "example")
///     }
/// }
///
/// let sink = Arc::new(Keep::default());
/// let element = Arc::new(ShareUsageElement::with_sink(
///     ShareUsageConfig::builder().build(),
///     Arc::clone(&sink),
/// ));
/// let pipeline = Pipeline::builder()
///     .add_element(Arc::clone(&element) as Arc<dyn FlowElement>)
///     .build()
///     .unwrap();
/// let evidence = Evidence::builder()
///     .add("header.User-Agent", "Mozilla/5.0")
///     .build();
/// pipeline.create_flow_data_with(evidence).process().unwrap();
///
/// // The element delivers what it holds when it is dropped.
/// drop(pipeline);
/// drop(element);
/// let documents = sink.0.lock().unwrap();
/// assert!(documents[0].ends_with(
///     "<header Name=\"User-Agent\"><![CDATA[Mozilla/5.0]]></header>\
///      <Host>example</Host></Device></Devices>"
/// ));
/// ```
pub trait UsageSink: Send + Sync {
    /// Take one batch. An error or a panic counts the batch as failed, and is
    /// logged at most once a minute with the number of failures since the
    /// last report.
    fn receive(&self, batch: UsageBatch) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Write elements at the start of each record, inside `<Device>` and
    /// before `SessionId`.
    ///
    /// # Errors
    ///
    /// Return the writer's error, which stops the batch.
    fn write_before_record(
        &self,
        writer: &mut UsageWriter<'_>,
        record: &UsageRecord,
    ) -> Result<(), UsageWriteError> {
        let _ = (writer, record);
        Ok(())
    }

    /// Write the product and the pipeline's elements, between `Version` and
    /// `Language`. Unless overridden this writes `<Product>Pipeline</Product>`
    /// and a `FlowElement` holding each of `flow_elements`, which are the
    /// type names of the elements in the pipeline that made the record.
    ///
    /// # Errors
    ///
    /// Return the writer's error, which stops the batch.
    fn write_pipeline_info(
        &self,
        writer: &mut UsageWriter<'_>,
        flow_elements: &[&str],
    ) -> Result<(), UsageWriteError> {
        write_default_pipeline_info(writer, flow_elements)
    }

    /// Write elements at the end of each record, after `BadSchema` when the
    /// record has one and before `</Device>`.
    ///
    /// # Errors
    ///
    /// Return the writer's error, which stops the batch.
    fn write_after_record(
        &self,
        writer: &mut UsageWriter<'_>,
        record: &UsageRecord,
    ) -> Result<(), UsageWriteError> {
        let _ = (writer, record);
        Ok(())
    }
}

impl<T: UsageSink + ?Sized> UsageSink for Arc<T> {
    fn receive(&self, batch: UsageBatch) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        (**self).receive(batch)
    }

    fn write_before_record(
        &self,
        writer: &mut UsageWriter<'_>,
        record: &UsageRecord,
    ) -> Result<(), UsageWriteError> {
        (**self).write_before_record(writer, record)
    }

    fn write_pipeline_info(
        &self,
        writer: &mut UsageWriter<'_>,
        flow_elements: &[&str],
    ) -> Result<(), UsageWriteError> {
        (**self).write_pipeline_info(writer, flow_elements)
    }

    fn write_after_record(
        &self,
        writer: &mut UsageWriter<'_>,
        record: &UsageRecord,
    ) -> Result<(), UsageWriteError> {
        (**self).write_after_record(writer, record)
    }
}

/// One message of usage records, as the records and as the `<Devices>`
/// document the built-in transport sends.
#[derive(Debug, Clone)]
pub struct UsageBatch {
    records: Vec<UsageRecord>,
    document: String,
}

impl UsageBatch {
    pub(crate) fn new(records: Vec<UsageRecord>, document: String) -> Self {
        UsageBatch { records, document }
    }

    /// The records, in the order they were queued.
    pub fn records(&self) -> &[UsageRecord] {
        &self.records
    }

    /// The XML document holding every record, with the sink's own elements,
    /// starting with a byte order mark and the XML declaration.
    pub fn document(&self) -> &str {
        &self.document
    }

    /// The document as UTF-8, compressed with gzip, which is the body the
    /// built-in transport posts.
    ///
    /// # Errors
    ///
    /// The compressor's error, which writing to memory does not give in
    /// practice.
    pub fn gzip(&self) -> std::io::Result<Vec<u8>> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(self.document.as_bytes())?;
        encoder.finish()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn the_gzip_body_holds_the_document_as_utf8() {
        let document = "\u{FEFF}<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                        <Devices><Device><Note>é中</Note></Device></Devices>";
        let batch = UsageBatch::new(Vec::new(), document.to_owned());
        let mut unzipped = Vec::new();
        flate2::read::GzDecoder::new(&batch.gzip().unwrap()[..])
            .read_to_end(&mut unzipped)
            .unwrap();
        assert_eq!(unzipped, document.as_bytes());
        assert!(unzipped.starts_with(b"\xEF\xBB\xBF<?xml"));
    }
}
