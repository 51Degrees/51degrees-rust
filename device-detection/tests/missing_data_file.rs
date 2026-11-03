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
//! and log nothing at those levels either. Kept in its own test binary because
//! a logger is installed once for the whole process.

use std::path::PathBuf;
use std::sync::Mutex;

use fiftyone_device_detection::{
    DeviceDetectionOnPremiseEngineBuilder, DeviceDetectionPipelineBuilder,
};
use fiftyone_ip_intelligence::{
    IpIntelligenceOnPremiseEngineBuilder, IpIntelligencePipelineBuilder,
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

/// A path in the temporary folder that holds no file.
fn missing(extension: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fiftyone-missing-data-file-{}.{extension}",
        std::process::id()
    ));
    assert!(!path.exists(), "{} exists", path.display());
    path
}

/// Each build fails, and none logs at warning level or above. One test, so
/// the records cannot come from another test running at the same time.
#[test]
fn a_missing_data_file_is_returned_and_not_logged() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Trace);
    log::warn!("logger check");
    assert_eq!(
        std::mem::take(&mut *LOGGED.lock().unwrap()),
        vec!["WARN logger check"]
    );

    assert!(DeviceDetectionOnPremiseEngineBuilder::new(missing("hash"))
        .build()
        .is_err());
    assert!(DeviceDetectionPipelineBuilder::on_premise(missing("hash"))
        .auto_update(false)
        .share_usage(false)
        .build()
        .is_err());
    assert!(IpIntelligenceOnPremiseEngineBuilder::new(missing("ipi"))
        .build()
        .is_err());
    assert!(IpIntelligencePipelineBuilder::on_premise(missing("ipi"))
        .auto_update(false)
        .share_usage(false)
        .build()
        .is_err());

    assert_eq!(*LOGGED.lock().unwrap(), Vec::<String>::new());
}
