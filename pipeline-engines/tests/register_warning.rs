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

//! What the data update service logs when it is given the only reference to
//! an engine, which it holds weakly and so cannot keep. Kept in its own test
//! binary because a logger is installed once for the whole process, and in
//! one test so the records it reads are its own.
#![cfg(feature = "data-update")]

use std::sync::{Arc, Mutex};

use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData, Result,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, DataFileConfiguration,
    DataUpdateService, OnPremiseAspectEngine,
};

/// Log records at warning level or above, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

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

/// The records logged since the last call.
fn take() -> Vec<String> {
    std::mem::take(&mut *LOGGED.lock().unwrap())
}

/// An engine with one data file held in memory, so registering it installs
/// no watcher and polls nothing.
struct QuietEngine {
    filter: EvidenceKeyFilterWhitelist,
    data_files: Vec<Arc<AspectEngineDataFile>>,
}

impl FlowElement for QuietEngine {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "quiet"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

impl AspectEngine for QuietEngine {
    fn data_source_tier(&self) -> &str {
        "Lite"
    }
    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &[]
    }
}

impl OnPremiseAspectEngine for QuietEngine {
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
        &self.data_files
    }
    fn refresh(&self, _identifier: Option<&str>) -> Result<()> {
        Ok(())
    }
}

fn quiet_engine() -> (Arc<dyn OnPremiseAspectEngine>, Arc<AspectEngineDataFile>) {
    let data_file = Arc::new(AspectEngineDataFile::new(
        DataFileConfiguration::memory_builder().build(),
    ));
    let engine = Arc::new(QuietEngine {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        data_files: vec![Arc::clone(&data_file)],
    });
    (engine, data_file)
}

/// A caller that hands the service its only reference to the engine gets no
/// registration, as the engine is dropped when `register` returns, so the
/// service warns. A caller that keeps a reference gets no warning and a
/// registration that lasts.
#[test]
fn registering_the_only_reference_to_an_engine_is_logged() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    let service = DataUpdateService::new();

    let (engine, data_file) = quiet_engine();
    service.register(engine, data_file).unwrap();
    assert_eq!(service.registered_count(), 0);
    assert_eq!(
        take(),
        [
            "WARN The data file for quiet will not be updated, because the data update \
          service was given the only reference to its engine, which is dropped when \
          registration ends. Keep a reference to the engine for as long as it is to \
          be updated."
        ]
    );

    let (engine, data_file) = quiet_engine();
    service.register(Arc::clone(&engine), data_file).unwrap();
    assert_eq!(service.registered_count(), 1);
    assert_eq!(take(), Vec::<String>::new());
    drop(engine);
}
