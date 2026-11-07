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

//! What a refresh that fails logs. .NET's Hash engine logs one error and keeps
//! the data it had, so this engine logs one error, keeps its data and returns
//! the error. Kept in its own test binary because a logger is installed once
//! for the whole process.
//!
//! The test needs the Lite file `51DEGREES_DD_PATH` names and skips with a
//! note when it is missing.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};
use fiftyone_device_detection_shared::DEVICE_DATA_KEY;
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{ElementData, Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::OnPremiseAspectEngine;

/// A mobile Safari on iPhone User-Agent.
const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

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

/// The records logged since the last call, which clears them.
fn take() -> Vec<String> {
    std::mem::take(&mut *LOGGED.lock().unwrap())
}

/// Whether the engine still answers `IsMobile` for a mobile User-Agent.
fn is_mobile(engine: &Arc<DeviceDetectionOnPremiseEngine>) -> bool {
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder().add_element(element).build().unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", MOBILE_USER_AGENT)
            .build(),
    );
    data.process().unwrap();
    let device = data.get(DEVICE_DATA_KEY).unwrap();
    format!("{:?}", device.get("IsMobile").unwrap()) == "Bool(true)"
}

/// Refresh, and check the refresh fails, logs exactly one error naming the
/// file, and leaves the engine answering.
fn refresh_fails(engine: &Arc<DeviceDetectionOnPremiseEngine>, path: &Path, case: &str) {
    let result = engine.refresh(None);
    let records = take();
    let error = result.expect_err(case);
    assert_eq!(records.len(), 1, "{case}: {records:?}");
    let expected = format!(
        "ERROR Failed to refresh data from '{}'. The engine keeps the data it had. If issue \
         persists, please contact support@51degrees.com. {error}",
        path.display()
    );
    assert_eq!(records[0], expected, "{case}");
    assert!(is_mobile(engine), "{case}: the engine kept its data");
    assert_eq!(take(), Vec::<String>::new(), "{case}");
}

/// One test, so the records cannot come from another test running at the
/// same time.
#[test]
fn a_refresh_that_fails_logs_one_error_and_keeps_the_data() {
    let Some(lite) = std::env::var("51DEGREES_DD_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    else {
        eprintln!("skipping the refresh logging test: set 51DEGREES_DD_PATH to the Lite file");
        return;
    };
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    log::warn!("logger check");
    assert_eq!(take(), vec!["WARN logger check"]);

    let dir = std::env::temp_dir().join(format!(
        "fiftyone-dd-refresh-logging-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("51Degrees.hash");
    std::fs::copy(&lite, &path).unwrap();

    // In memory, so the data file can be replaced on every platform.
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .performance_profile(PerformanceProfile::InMemory)
        .build()
        .expect("the data file loads");
    assert_eq!(take(), Vec::<String>::new());
    assert!(is_mobile(&engine));

    std::fs::write(&path, vec![0x5a_u8; 4096]).unwrap();
    refresh_fails(&engine, &path, "a file that is not a data file");

    let bytes = std::fs::read(&lite).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
    refresh_fails(&engine, &path, "half a data file");

    std::fs::remove_file(&path).unwrap();
    refresh_fails(&engine, &path, "no file");

    // A refresh from memory that fails logs as one from a file does, naming
    // no file.
    for (case, data) in [
        ("no data", Vec::new()),
        (
            "half a data file in memory",
            bytes[..bytes.len() / 2].to_vec(),
        ),
    ] {
        let result = engine.refresh_from_memory(None, &data);
        let records = take();
        let error = result.expect_err(case);
        assert_eq!(
            records,
            vec![format!(
                "ERROR Failed to refresh data from memory. The engine keeps the data it had. \
                 If issue persists, please contact support@51degrees.com. {error}"
            )],
            "{case}"
        );
        assert!(is_mobile(&engine), "{case}: the engine kept its data");
    }
    engine
        .refresh_from_memory(None, &bytes)
        .expect("the Lite data loads from memory");
    assert_eq!(take(), Vec::<String>::new());
    assert!(is_mobile(&engine));

    // A refresh that works logs nothing at warning level or above.
    std::fs::copy(&lite, &path).unwrap();
    engine.refresh(None).expect("the Lite file loads again");
    assert_eq!(take(), Vec::<String>::new());
    assert!(is_mobile(&engine));

    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
}
