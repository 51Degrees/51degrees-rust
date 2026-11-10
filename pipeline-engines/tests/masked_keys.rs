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

//! A data update that fails while its URL carries a licence key must leave
//! only the key's masked form, being its last 4 characters with every
//! character before them replaced, in the error it returns and in every log
//! record. Kept in its own test binary because a logger is installed once
//! for the whole process.
#![cfg(feature = "data-update")]

use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData, Result,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, DataFileConfiguration,
    DataUpdateService, OnPremiseAspectEngine,
};

/// The licence key in the update URL. It is not a real key.
const KEY: &str = "ABCDEFGHIJKLMNOP";

/// [`KEY`] as a message may show it.
const MASKED: &str = "************MNOP";

/// How long the test waits for the first scheduled poll, which is due one
/// second after registration.
const POLL_WAIT_SECONDS: u64 = 30;

/// Every log record, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Keeps every record in [`LOGGED`].
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

/// An engine with data loaded, which a refresh leaves as it is.
struct LoadedEngine {
    filter: EvidenceKeyFilterWhitelist,
    data_files: Vec<Arc<AspectEngineDataFile>>,
}

impl FlowElement for LoadedEngine {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "loaded"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

impl AspectEngine for LoadedEngine {
    fn data_source_tier(&self) -> &str {
        "Lite"
    }
    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &[]
    }
}

impl OnPremiseAspectEngine for LoadedEngine {
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
        &self.data_files
    }
    fn refresh(&self, _identifier: Option<&str>) -> Result<()> {
        Ok(())
    }
}

/// The URL is used as configured, with the key in its query, so the update
/// on start-up and the scheduled poll both fail with that URL to hand. The
/// error, the start-up warning and the scheduled poll's warning each name it
/// with the key masked.
#[test]
fn a_failed_update_from_a_url_carrying_a_licence_key_shows_only_its_masked_form() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Trace);

    // Nothing listens on a port once its listener is dropped.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/datafile?LicenseKeys={KEY}&Download=True",
        listener.local_addr().unwrap()
    );
    drop(listener);
    let data_file = Arc::new(AspectEngineDataFile::new(
        DataFileConfiguration::memory_builder()
            .data_update_url(url)
            .update_on_startup(true)
            .automatic_updates_enabled(true)
            .polling_interval_seconds(1)
            .max_randomisation_seconds(0)
            .build(),
    ));
    let engine: Arc<dyn OnPremiseAspectEngine> = Arc::new(LoadedEngine {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        data_files: vec![Arc::clone(&data_file)],
    });
    let service = DataUpdateService::new();

    let error = service
        .register(Arc::clone(&engine), data_file)
        .expect_err("nothing is listening");
    for shown in [error.to_string(), format!("{error:?}")] {
        assert!(!shown.contains(KEY), "the error shows the key: {shown}");
        assert!(
            shown.contains(&format!("LicenseKeys={MASKED}")),
            "the error lost the masked key: {shown}"
        );
    }

    let deadline = Instant::now() + Duration::from_secs(POLL_WAIT_SECONDS);
    let polled = |logged: &[String]| {
        logged
            .iter()
            .any(|record| record.contains("Update will be attempted again later"))
    };
    while !polled(&LOGGED.lock().unwrap()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(service);
    let logged = std::mem::take(&mut *LOGGED.lock().unwrap());

    assert!(polled(&logged), "no scheduled poll was logged: {logged:?}");
    for record in &logged {
        assert!(
            !record.contains(KEY),
            "a log record shows the key: {record}"
        );
    }
    for wanted in ["on start-up", "Update will be attempted again later"] {
        assert!(
            logged.iter().any(|record| record.contains(wanted)
                && record.contains(&format!("LicenseKeys={MASKED}"))),
            "no record with '{wanted}' names the masked key: {logged:?}"
        );
    }
}
