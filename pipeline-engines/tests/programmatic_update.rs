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

//! A programmatic update, as the data-updates specification's "Programmatic
//! update" section says. With automatic updates on it polls the update URL.
//! With them off and a data file on disk it refreshes the engine from that
//! file when the file is newer, and sends no request, even with an update URL
//! set. With them off a memory data source, which has no file, needs no
//! update and sends no request. The requests are seen by a local stand-in for
//! the Distributor.
#![cfg(feature = "data-update")]

// This binary uses only part of the stand-in.
#[allow(dead_code)]
#[path = "support/distributor.rs"]
mod distributor;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{TimeZone, Utc};
use distributor::StandIn;
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData, Result,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, AutoUpdateStatus,
    DataFileConfiguration, DataFileConfigurationBuilder, DataUpdateService, OnPremiseAspectEngine,
};

/// The product of the data file, and the one the stand-in offers.
const TAC: &str = "V4TAC";

/// The data file the engine was built from.
const OLD_DATA: &str = "old data";

/// A newer data file put in place of the old one.
const FILE_DATA: &str = "newer data from the file";

/// The data the stand-in serves.
const SERVED_DATA: &str = "newer data from the update URL";

/// How much later than the old file a newer file is.
const NEWER: Duration = Duration::from_secs(10);

/// An engine that keeps the data it was last refreshed with.
struct TestEngine {
    filter: EvidenceKeyFilterWhitelist,
    data_files: Vec<Arc<AspectEngineDataFile>>,
    refreshes: AtomicUsize,
    data: Mutex<Vec<u8>>,
}

impl TestEngine {
    fn new(data_file: Arc<AspectEngineDataFile>) -> TestEngine {
        TestEngine {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            data_files: vec![data_file],
            refreshes: AtomicUsize::new(0),
            data: Mutex::new(OLD_DATA.as_bytes().to_vec()),
        }
    }
}

impl FlowElement for TestEngine {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "programmatic"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

impl AspectEngine for TestEngine {
    fn data_source_tier(&self) -> &str {
        "TAC"
    }
    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &[]
    }
}

impl OnPremiseAspectEngine for TestEngine {
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
        &self.data_files
    }
    fn refresh(&self, _identifier: Option<&str>) -> Result<()> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        if let Some(path) = self.data_files[0].data_file_path() {
            *self.data.lock().unwrap() = std::fs::read(path).unwrap();
        }
        Ok(())
    }
    fn refresh_from_memory(&self, _identifier: Option<&str>, data: &[u8]) -> Result<()> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        *self.data.lock().unwrap() = data.to_vec();
        Ok(())
    }
}

/// An engine, registered with its service, and its data file in a folder
/// removed when dropped.
struct Fixture {
    folder: Option<PathBuf>,
    engine: Arc<TestEngine>,
    shared: Arc<dyn OnPremiseAspectEngine>,
    service: DataUpdateService,
}

impl Fixture {
    /// A data file holding [`OLD_DATA`] at a path, or a memory data source
    /// when `test` is `None`, updated from the stand-in with the
    /// configuration `configure` gives.
    fn new(
        test: Option<&str>,
        stand_in: &StandIn,
        configure: impl FnOnce(DataFileConfigurationBuilder) -> DataFileConfigurationBuilder,
    ) -> Fixture {
        let folder = test.map(|test| {
            let folder = std::env::temp_dir().join(format!(
                "fiftyone-programmatic-update-{test}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("data.hash"), OLD_DATA).unwrap();
            folder
        });
        let builder = match &folder {
            Some(folder) => DataFileConfiguration::builder(folder.join("data.hash")),
            None => DataFileConfiguration::memory_builder(),
        }
        .data_update_url(stand_in.url())
        .file_system_watcher_enabled(false);
        let data_file = Arc::new(AspectEngineDataFile::new(configure(builder).build()));
        data_file.record_product(Some(TAC.to_owned()));
        let engine = Arc::new(TestEngine::new(Arc::clone(&data_file)));
        let shared: Arc<dyn OnPremiseAspectEngine> = engine.clone();
        let service = DataUpdateService::new();
        service
            .register(Arc::clone(&shared), Arc::clone(&data_file))
            .unwrap();
        Fixture {
            folder,
            engine,
            shared,
            service,
        }
    }

    fn path(&self) -> PathBuf {
        self.folder.as_ref().unwrap().join("data.hash")
    }

    /// Put a newer data file holding [`FILE_DATA`] in place of the old one.
    fn put_newer_file(&self) {
        let path = self.path();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, FILE_DATA).unwrap();
        set_modified(&path, modified + NEWER);
    }

    /// The data the engine was last refreshed with.
    fn data(&self) -> String {
        String::from_utf8(self.engine.data.lock().unwrap().clone()).unwrap()
    }

    /// The data file on disk.
    fn file(&self) -> String {
        std::fs::read_to_string(self.path()).unwrap()
    }

    fn refreshes(&self) -> usize {
        self.engine.refreshes.load(Ordering::SeqCst)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(folder) = &self.folder {
            let _ = std::fs::remove_dir_all(folder);
        }
    }
}

fn set_modified(path: &Path, modified: std::time::SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
}

/// A stand-in for licence keys holding [`TAC`], serving [`SERVED_DATA`].
fn serving() -> StandIn {
    let published = Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap();
    StandIn::new(&[TAC], Some((SERVED_DATA.as_bytes(), published)))
}

/// The product and status of each request the stand-in saw.
fn requests(stand_in: &StandIn) -> Vec<(Option<String>, u16)> {
    stand_in
        .requests()
        .into_iter()
        .map(|request| (request.product, request.status))
        .collect()
}

#[test]
fn with_automatic_updates_off_a_newer_file_is_applied_and_no_request_is_sent() {
    let stand_in = serving();
    let fixture = Fixture::new(Some("newer-file"), &stand_in, |builder| {
        builder.automatic_updates_enabled(false)
    });
    fixture.put_newer_file();

    let status = fixture.service.check_for_update(&fixture.shared, None);

    assert_eq!(status.unwrap(), AutoUpdateStatus::Success);
    assert_eq!(fixture.data(), FILE_DATA);
    assert_eq!(fixture.file(), FILE_DATA);
    assert_eq!(requests(&stand_in), []);
}

#[test]
fn with_automatic_updates_off_an_unchanged_file_needs_no_update_and_no_request_is_sent() {
    let stand_in = serving();
    let fixture = Fixture::new(Some("unchanged-file"), &stand_in, |builder| {
        builder.automatic_updates_enabled(false)
    });

    let status = fixture.service.check_for_update(&fixture.shared, None);

    assert_eq!(status.unwrap(), AutoUpdateStatus::NotNeeded);
    assert_eq!(fixture.refreshes(), 0);
    assert_eq!(fixture.data(), OLD_DATA);
    assert_eq!(fixture.file(), OLD_DATA);
    assert_eq!(requests(&stand_in), []);
}

#[test]
fn with_automatic_updates_on_the_update_url_is_polled() {
    let stand_in = serving();
    let fixture = Fixture::new(Some("polled"), &stand_in, |builder| {
        builder.automatic_updates_enabled(true)
    });

    let status = fixture.service.check_for_update(&fixture.shared, None);

    assert_eq!(status.unwrap(), AutoUpdateStatus::Success);
    assert_eq!(fixture.data(), SERVED_DATA);
    assert_eq!(fixture.file(), SERVED_DATA);
    assert_eq!(
        requests(&stand_in),
        [(None, 300), (Some(TAC.to_owned()), 200)]
    );
}

#[test]
fn with_automatic_updates_on_a_memory_data_source_is_polled() {
    let stand_in = serving();
    let fixture = Fixture::new(None, &stand_in, |builder| {
        builder.automatic_updates_enabled(true)
    });

    let status = fixture.service.check_for_update(&fixture.shared, None);

    assert_eq!(status.unwrap(), AutoUpdateStatus::Success);
    assert_eq!(fixture.data(), SERVED_DATA);
    assert_eq!(
        requests(&stand_in),
        [(None, 300), (Some(TAC.to_owned()), 200)]
    );
}

#[test]
fn with_automatic_updates_off_a_memory_data_source_needs_no_update_and_no_request_is_sent() {
    let stand_in = serving();
    let fixture = Fixture::new(None, &stand_in, |builder| {
        builder.automatic_updates_enabled(false)
    });

    let status = fixture.service.check_for_update(&fixture.shared, None);

    assert_eq!(status.unwrap(), AutoUpdateStatus::NotNeeded);
    assert_eq!(fixture.refreshes(), 0);
    assert_eq!(fixture.data(), OLD_DATA);
    assert_eq!(requests(&stand_in), []);
}
