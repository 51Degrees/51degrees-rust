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

//! The data updates specification says "These messages MUST also be
//! consistent across languages to make life easier for 51Degrees support.",
//! and of the errors "A programmatic check, the update on start-up and an
//! update from memory pass the error to their caller instead of logging it."
//! So a refresh of the engine that fails gives its error to the caller and
//! logs nothing of its own, whether from the file or from memory. The
//! service logs a failed check it made itself. Kept in its own test binary
//! because a logger is installed once for the whole process. The test needs
//! the Lite file, from `_51DEGREES_DD_PATH` or the data repository in the
//! `cxx` submodule, and skips with a note without it.

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::path::PathBuf;
use std::sync::Mutex;

use fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngineBuilder;
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_engines::{DataFileConfiguration, OnPremiseAspectEngine};

/// Every record at Warning or above, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

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

/// The Lite file `_51DEGREES_DD_PATH` names, or the one in the data
/// repository inside the `cxx` submodule.
fn lite_data_file() -> Option<PathBuf> {
    if let Ok(path) = crate::deprecated_names::var("_51DEGREES_DD_PATH") {
        let path = PathBuf::from(path.trim());
        if path.is_file() {
            return Some(path);
        }
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .to_path_buf();
    Some(workspace.join("cxx/data/device-detection/51Degrees-LiteV4.1.hash"))
        .filter(|path| path.is_file())
}

/// A refresh from a data file that is gone, and one from memory holding
/// half a data file, each give the error to the caller and log nothing.
#[test]
fn a_failed_refresh_gives_its_error_to_the_caller_and_logs_nothing() {
    let Some(lite) = lite_data_file() else {
        eprintln!("skipping the failed refresh: set _51DEGREES_DD_PATH to the Lite file");
        return;
    };
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    let dir = std::env::temp_dir().join(format!("fiftyone-dd-not-logged-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("51Degrees.hash");
    std::fs::copy(&lite, &path).unwrap();
    let bytes = std::fs::read(&lite).unwrap();
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .performance_profile(PerformanceProfile::InMemory)
        .auto_update(
            DataFileConfiguration::builder(&path)
                .automatic_updates_enabled(false)
                .file_system_watcher_enabled(false)
                .license_key_required_for_updates(false)
                .build(),
            None,
        )
        .build()
        .expect("the data file loads");
    LOGGED.lock().unwrap().clear();

    std::fs::remove_file(&path).unwrap();
    engine.refresh(None).expect_err("the data file is gone");
    engine
        .refresh_from_memory(None, &bytes[..bytes.len() / 2])
        .expect_err("half a data file does not load");

    let logged = std::mem::take(&mut *LOGGED.lock().unwrap());
    assert_eq!(logged, Vec::<String>::new());
    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
}
