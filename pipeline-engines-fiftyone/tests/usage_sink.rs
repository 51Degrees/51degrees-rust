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

//! A caller's own sink replaces the built-in transport and writes its own
//! elements around each record and its own product, through the public
//! items alone, as a service that stores the records it bills from does.

use std::io::Read;
use std::sync::{Arc, Mutex};

use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines_fiftyone::{
    ShareUsageConfig, ShareUsageElement, UsageBatch, UsageRecord, UsageSink, UsageWriteError,
    UsageWriter,
};

/// When the sink says each record was stored.
const DATE_ADDED: &str = "2026-11-10T01:00:00";

/// The address the sink gives as this server's public one.
const PUBLIC_SERVER_IP: &str = "203.0.113.7";

/// A sink that keeps each batch in place of sending it, writes `DateAdded`
/// before each record and `PublicServerIP` after it, and names its own
/// product with no Flow Elements.
#[derive(Default)]
struct Store(Mutex<Vec<UsageBatch>>);

impl UsageSink for Store {
    fn receive(&self, batch: UsageBatch) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.0.lock().unwrap().push(batch);
        Ok(())
    }

    fn write_before_record(
        &self,
        writer: &mut UsageWriter<'_>,
        _record: &UsageRecord,
    ) -> Result<(), UsageWriteError> {
        writer.element("DateAdded", DATE_ADDED)
    }

    fn write_pipeline_info(
        &self,
        writer: &mut UsageWriter<'_>,
        _flow_elements: &[&str],
    ) -> Result<(), UsageWriteError> {
        writer.element("Product", "Pipeline.Cloud")
    }

    fn write_after_record(
        &self,
        writer: &mut UsageWriter<'_>,
        _record: &UsageRecord,
    ) -> Result<(), UsageWriteError> {
        writer.element("PublicServerIP", PUBLIC_SERVER_IP)
    }
}

#[test]
fn a_callers_writer_and_sink_replace_the_defaults() {
    let store = Arc::new(Store::default());
    let element = Arc::new(ShareUsageElement::with_sink(
        ShareUsageConfig::builder().build(),
        Arc::clone(&store),
    ));
    let pipeline = Pipeline::builder()
        .add_element(Arc::clone(&element) as Arc<dyn FlowElement>)
        .build()
        .unwrap();
    for agent in ["one", "two\u{1}"] {
        let evidence = Evidence::builder()
            .add("header.User-Agent", agent)
            .add("server.client-ip", "192.0.2.7")
            .build();
        pipeline.create_flow_data_with(evidence).process().unwrap();
    }
    // The element delivers what it holds when it is dropped.
    drop(pipeline);
    drop(element);

    let batches = store.0.lock().unwrap();
    assert_eq!(batches.len(), 1, "both records are in one batch");
    let batch = &batches[0];
    assert_eq!(batch.records().len(), 2);
    assert_eq!(batch.records()[0].client_ip(), "192.0.2.7");

    let document = batch.document();
    assert!(!document.contains("<FlowElement"), "{document}");
    assert!(!document.contains("<Product>Pipeline<"), "{document}");
    let devices: Vec<&str> = document.split("<Device>").skip(1).collect();
    assert_eq!(devices.len(), 2, "{document}");
    for (device, entry) in devices.iter().zip([
        "<header Name=\"User-Agent\">one</header>",
        "<header escaped=\"true\" Name=\"User-Agent\">two\\x0001</header>",
    ]) {
        assert!(
            device.starts_with(&format!(
                "<DateAdded>{DATE_ADDED}</DateAdded><SessionId />\
                 <Sequence>0</Sequence><DateSent>"
            )),
            "{device}"
        );
        assert!(
            device.contains("<ClientIP>192.0.2.7</ClientIP><Version>"),
            "{device}"
        );
        assert!(
            device.contains("</Version><Product>Pipeline.Cloud</Product><Language>rust</Language>"),
            "{device}"
        );
        assert!(
            device.contains(&format!(
                "</Platform>{entry}<PublicServerIP>{PUBLIC_SERVER_IP}</PublicServerIP></Device>"
            )),
            "{device}"
        );
    }

    let mut unzipped = String::new();
    flate2::read::GzDecoder::new(&batch.gzip().unwrap()[..])
        .read_to_string(&mut unzipped)
        .unwrap();
    assert_eq!(unzipped, document);
}
