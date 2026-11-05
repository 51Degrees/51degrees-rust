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

//! What building on a data file that does not exist logs.
//!
//! The .NET engine builders and facades throw for a missing data file and log
//! nothing at warning level or above, so the builders here return the error
//! and log nothing at those levels either. Each error is the native library's
//! file-not-found status, so a build that fails for any other reason fails the
//! test. Kept in its own test binary because a logger is installed once for the
//! whole process.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fiftyone_device_detection::{
    DeviceDetectionOnPremiseEngineBuilder, DeviceDetectionPipelineBuilder,
};
use fiftyone_ip_intelligence::{
    IpIntelligenceOnPremiseEngineBuilder, IpIntelligencePipelineBuilder,
};
use fiftyone_pipeline_core::Error;

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

/// A path in the temporary folder that holds no file.
fn missing(extension: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fiftyone-missing-data-file-{}.{extension}",
        std::process::id()
    ));
    assert!(!path.exists(), "{} exists", path.display());
    path
}

/// Checks that `build` failed because the data file at `path` does not exist,
/// being the native status `FileNotFound` with a message that names the file.
fn assert_file_not_found<T>(build: &str, result: Result<T, Error>, path: &Path) {
    let name = path
        .file_name()
        .expect("the path names a file")
        .to_string_lossy()
        .into_owned();
    match result {
        Err(Error::Native { status, message }) => {
            assert_eq!(status, "FileNotFound", "{build}: {message}");
            assert!(
                message.contains(&name),
                "{build}: {message:?} does not name {name}"
            );
        }
        Err(other) => panic!("{build} failed for another reason: {other}"),
        Ok(_) => panic!("{build} built with no data file"),
    }
}

/// Each build fails with the native file-not-found status, and none logs at
/// warning level or above. One test, so the records cannot come from another
/// test running at the same time.
#[test]
fn a_missing_data_file_is_returned_and_not_logged() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Trace);
    log::warn!("logger check");
    assert_eq!(
        std::mem::take(&mut *LOGGED.lock().unwrap()),
        vec!["WARN logger check"]
    );

    let hash = missing("hash");
    let ipi = missing("ipi");
    assert_file_not_found(
        "the Device Detection engine",
        DeviceDetectionOnPremiseEngineBuilder::new(hash.clone()).build(),
        &hash,
    );
    assert_file_not_found(
        "the Device Detection pipeline",
        DeviceDetectionPipelineBuilder::on_premise(hash.clone())
            .auto_update(false)
            .share_usage(false)
            .build(),
        &hash,
    );
    assert_file_not_found(
        "the IP Intelligence engine",
        IpIntelligenceOnPremiseEngineBuilder::new(ipi.clone()).build(),
        &ipi,
    );
    assert_file_not_found(
        "the IP Intelligence pipeline",
        IpIntelligencePipelineBuilder::on_premise(ipi.clone())
            .auto_update(false)
            .share_usage(false)
            .build(),
        &ipi,
    );

    assert_eq!(*LOGGED.lock().unwrap(), Vec::<String>::new());
}
