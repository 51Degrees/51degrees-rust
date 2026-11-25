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

//! The data update service loads a watched or downloaded data file only when
//! the date its header gives for its publication is later than that of the
//! data the engine has loaded. A file that is not newer is not loaded, and
//! the check is reported as not needed, whatever the file's modification
//! time says.
//!
//! The engine here reads the date from a data file that holds only the date
//! as text, as a real engine reads it from the header of its data file.
#![cfg(feature = "data-update")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, NaiveDate, Utc};
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData, Result,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, AutoUpdateStatus,
    DataFileConfiguration, DataUpdateService, OnPremiseAspectEngine,
};

/// The date the data the engine starts with was published.
const LOADED: &str = "2026-11-10";

/// A date before [`LOADED`].
const EARLIER: &str = "2026-11-09";

/// A date after [`LOADED`].
const LATER: &str = "2026-11-11";

/// How long a test waits for a watched file's check to end.
const CHECK_WAIT: Duration = Duration::from_secs(10);

/// How long the watcher is given to start before the file changes.
const WATCHER_START: Duration = Duration::from_millis(300);

/// The date a data file here was published, being the text it holds.
fn published(path: &Path) -> Option<DateTime<Utc>> {
    let text = std::fs::read_to_string(path).ok()?;
    let date = NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok()?;
    Some(date.and_hms_opt(0, 0, 0)?.and_utc())
}

/// Midnight UTC on `date`.
fn midnight(date: &str) -> DateTime<Utc> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
}

/// An engine that counts its refreshes and records the date of the data it
/// loads, as a real engine records the date in its data file's header.
struct DatedEngine {
    filter: EvidenceKeyFilterWhitelist,
    data_files: Vec<Arc<AspectEngineDataFile>>,
    refreshes: Arc<AtomicUsize>,
}

impl DatedEngine {
    /// Load the data file, recording the date it was published.
    fn load(&self) {
        let file = &self.data_files[0];
        match file.data_file_path().and_then(published) {
            Some(date) => file.set_data_published(date),
            None => file.clear_data_published(),
        }
    }
}

impl FlowElement for DatedEngine {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "dated"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

impl AspectEngine for DatedEngine {
    fn data_source_tier(&self) -> &str {
        "Lite"
    }
    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &[]
    }
}

impl OnPremiseAspectEngine for DatedEngine {
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
        &self.data_files
    }
    fn refresh(&self, _identifier: Option<&str>) -> Result<()> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        self.load();
        Ok(())
    }
    fn published_from_file(
        &self,
        _data_file_identifier: Option<&str>,
        path: &Path,
    ) -> Option<DateTime<Utc>> {
        published(path)
    }
}

/// A new, empty folder for one test, holding a data file published on
/// [`LOADED`].
fn folder(test: &str) -> (PathBuf, PathBuf) {
    let dir =
        std::env::temp_dir().join(format!("fiftyone-published-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("device.dat");
    std::fs::write(&path, LOADED).unwrap();
    (dir, path)
}

/// An engine built from the data file `config` names.
fn engine(config: DataFileConfiguration) -> Arc<DatedEngine> {
    let engine = Arc::new(DatedEngine {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        data_files: vec![Arc::new(AspectEngineDataFile::new(config))],
        refreshes: Arc::new(AtomicUsize::new(0)),
    });
    engine.load();
    engine
}

/// The names of the files in `dir`.
fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Serve `body` once, uncompressed, then close, and give the URL.
fn serve_once(body: &'static str) -> (String, std::thread::JoinHandle<()>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let url = format!("http://{}/datafile", server.server_addr());
    let handle = std::thread::spawn(move || {
        if let Ok(request) = server.recv() {
            let _ = request.respond(tiny_http::Response::from_data(body.as_bytes()));
        }
    });
    (url, handle)
}

/// Download a data file published on `date` into a data file published on
/// [`LOADED`], and give the status, the refresh count, the folder's files
/// and the date the engine then has.
fn download(
    test: &str,
    date: &'static str,
) -> (
    AutoUpdateStatus,
    usize,
    PathBuf,
    Vec<String>,
    Option<DateTime<Utc>>,
) {
    let (dir, path) = folder(test);
    let (url, server) = serve_once(date);
    let config = DataFileConfiguration::builder(&path)
        .data_update_url(url)
        .decompress_content(false)
        .verify_md5(false)
        .verify_modified_since(false)
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .build();
    let engine = engine(config);
    let service = DataUpdateService::new();
    let engine_dyn: Arc<dyn OnPremiseAspectEngine> = engine.clone();
    service
        .register(Arc::clone(&engine_dyn), Arc::clone(&engine.data_files[0]))
        .unwrap();

    let status = service.check_for_update(&engine_dyn, None).unwrap();
    server.join().unwrap();
    let names = file_names(&dir);
    (
        status,
        engine.refreshes.load(Ordering::SeqCst),
        path,
        names,
        engine.data_files[0].data_published(),
    )
}

/// A downloaded file published later than the loaded data is loaded.
#[test]
fn a_downloaded_file_published_later_is_loaded() {
    let (status, refreshes, path, names, date) = download("download-later", LATER);
    assert_eq!(status, AutoUpdateStatus::Success);
    assert_eq!(refreshes, 1);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), LATER);
    assert_eq!(names, ["device.dat"]);
    assert_eq!(date, Some(midnight(LATER)));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// A downloaded file published on the same day as the loaded data, or
/// earlier, is not loaded and is reported as not needed. The data file is
/// left as it was and the download is removed.
#[test]
fn a_downloaded_file_published_on_the_same_day_or_earlier_is_not_loaded() {
    for (test, date) in [("download-same", LOADED), ("download-earlier", EARLIER)] {
        let (status, refreshes, path, names, loaded) = download(test, date);
        assert_eq!(status, AutoUpdateStatus::NotNeeded, "{date}");
        assert_eq!(refreshes, 0, "{date}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), LOADED, "{date}");
        assert_eq!(names, ["device.dat"], "{date}");
        assert_eq!(loaded, Some(midnight(LOADED)), "{date}");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}

/// A watched data file and the statuses its checks end with.
struct Watched {
    dir: PathBuf,
    path: PathBuf,
    engine: Arc<DatedEngine>,
    statuses: Arc<Mutex<Vec<AutoUpdateStatus>>>,
    _service: DataUpdateService,
}

impl Watched {
    fn new(test: &str) -> Watched {
        let (dir, path) = folder(test);
        let config = DataFileConfiguration::builder(&path)
            .automatic_updates_enabled(false)
            .file_system_watcher_enabled(true)
            .build();
        let engine = engine(config);
        let service = DataUpdateService::new();
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let told = Arc::clone(&statuses);
        service.on_update_complete(move |complete| {
            told.lock().unwrap().push(complete.status);
        });
        let engine_dyn: Arc<dyn OnPremiseAspectEngine> = engine.clone();
        service
            .register(engine_dyn, Arc::clone(&engine.data_files[0]))
            .unwrap();
        std::thread::sleep(WATCHER_START);
        Watched {
            dir,
            path,
            engine,
            statuses,
            _service: service,
        }
    }

    /// Replace the data file with one published on `date`, written later
    /// than the one it replaces, and give the status the check ends with.
    /// A further event for the same write is in progress, so the first
    /// other status is the one the write gave.
    fn replace(&self, date: &str) -> Option<AutoUpdateStatus> {
        let seen = self.statuses.lock().unwrap().len();
        std::thread::sleep(Duration::from_millis(50));
        let new = self.dir.join("device.new");
        std::fs::write(&new, date).unwrap();
        std::fs::rename(&new, &self.path).unwrap();
        let start = Instant::now();
        while start.elapsed() < CHECK_WAIT {
            let found = self.statuses.lock().unwrap()[seen..]
                .iter()
                .copied()
                .find(|status| *status != AutoUpdateStatus::InProgress);
            if found.is_some() {
                return found;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    fn refreshes(&self) -> usize {
        self.engine.refreshes.load(Ordering::SeqCst)
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A watched file published later than the loaded data is loaded.
#[test]
fn a_watched_file_published_later_is_loaded() {
    let watched = Watched::new("watch-later");
    assert_eq!(watched.replace(LATER), Some(AutoUpdateStatus::Success));
    assert_eq!(watched.refreshes(), 1);
    assert_eq!(
        watched.engine.data_files[0].data_published(),
        Some(midnight(LATER))
    );
}

/// A watched file published on the same day as the loaded data, or earlier,
/// is not loaded although it was written later, and is reported as not
/// needed. A file published later is still loaded after them.
#[test]
fn a_watched_file_published_on_the_same_day_or_earlier_is_not_loaded() {
    let watched = Watched::new("watch-same");
    for date in [LOADED, EARLIER] {
        assert_eq!(
            watched.replace(date),
            Some(AutoUpdateStatus::NotNeeded),
            "{date}"
        );
        assert_eq!(watched.refreshes(), 0, "{date}");
        assert_eq!(
            watched.engine.data_files[0].data_published(),
            Some(midnight(LOADED)),
            "{date}"
        );
    }
    assert_eq!(watched.replace(LATER), Some(AutoUpdateStatus::Success));
    assert_eq!(watched.refreshes(), 1);
}

/// A programmatic check of a data file with no update URL loads the file
/// only when it was published later, as a watched file is.
#[test]
fn a_checked_file_is_loaded_only_when_published_later() {
    let (dir, path) = folder("check");
    let config = DataFileConfiguration::builder(&path)
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .build();
    let engine = engine(config);
    let service = DataUpdateService::new();
    let engine_dyn: Arc<dyn OnPremiseAspectEngine> = engine.clone();
    service
        .register(Arc::clone(&engine_dyn), Arc::clone(&engine.data_files[0]))
        .unwrap();

    for (date, expected, refreshes) in [
        (LOADED, AutoUpdateStatus::NotNeeded, 0),
        (EARLIER, AutoUpdateStatus::NotNeeded, 0),
        (LATER, AutoUpdateStatus::Success, 1),
    ] {
        std::thread::sleep(Duration::from_millis(50));
        let new = dir.join("device.new");
        std::fs::write(&new, date).unwrap();
        std::fs::rename(&new, &path).unwrap();
        let status = service.check_for_update(&engine_dyn, None).unwrap();
        assert_eq!(status, expected, "{date}");
        assert_eq!(engine.refreshes.load(Ordering::SeqCst), refreshes, "{date}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
