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

//! The data update service logs the messages data-updates.md lists under
//! "Messages at start-up" and "Messages during operation", with the URL's
//! licence key masked. Kept in its own test binary because a logger is
//! installed once for the whole process.
#![cfg(feature = "data-update")]

use std::net::TcpListener;
use std::sync::{Arc, Mutex};

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

/// Every information record, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Keeps every information record in [`LOGGED`].
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() == log::Level::Info
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

/// Registering a data file that updates on start-up and has a file system
/// watcher logs, in order, "Updating on start-up", "Checking for update",
/// "Checking for update from '<url>' for Engine '<engine>'" with the key
/// masked, and "Creating file system watcher". Nothing listens at the URL,
/// so the check fails once its request is made.
#[test]
fn registering_logs_the_start_up_and_check_messages() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);

    let folder =
        std::env::temp_dir().join(format!("fiftyone-update-messages-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("data.hash");
    std::fs::write(&path, b"data").unwrap();

    // Nothing listens on a port once its listener is dropped.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/datafile?LicenseKeys={KEY}&Download=True",
        listener.local_addr().unwrap()
    );
    let masked_url = url.replace(KEY, MASKED);
    drop(listener);
    let data_file = Arc::new(AspectEngineDataFile::new(
        DataFileConfiguration::builder(&path)
            .data_update_url(url)
            .update_on_startup(true)
            .automatic_updates_enabled(false)
            .file_system_watcher_enabled(true)
            .build(),
    ));
    let engine: Arc<dyn OnPremiseAspectEngine> = Arc::new(LoadedEngine {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        data_files: vec![Arc::clone(&data_file)],
    });
    let service = DataUpdateService::new();

    let _ = service.register(Arc::clone(&engine), data_file);
    drop(service);
    let logged = std::mem::take(&mut *LOGGED.lock().unwrap());
    let _ = std::fs::remove_dir_all(&folder);

    let wanted = [
        "INFO Updating on start-up".to_owned(),
        "INFO Checking for update".to_owned(),
        format!("INFO Checking for update from '{masked_url}' for Engine 'loaded'"),
        "INFO Creating file system watcher".to_owned(),
    ];
    let mut from = 0;
    for message in &wanted {
        let found = logged[from..].iter().position(|record| record == message);
        let Some(found) = found else {
            panic!("'{message}' is not logged after record {from}: {logged:?}");
        };
        from += found + 1;
    }
    for record in &logged {
        assert!(!record.contains(KEY), "a record shows the key: {record}");
    }
}
