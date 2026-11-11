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

//! What reading a property that real Device Detection and IP Intelligence
//! data do not hold logs. .NET's aspect data logs one warning for each such
//! read, naming the reason, and nothing for a property it holds (measured
//! with the Hash engine restricted to IsMobile). Kept in its own test binary
//! because a logger is installed once for the whole process, and in one test
//! so the records it reads are its own.
//!
//! Each half skips when its data file is absent.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use fiftyone_device_detection::{
    DeviceData, DeviceDetectionOnPremiseEngineBuilder, Evidence, FlowElement, PerformanceProfile,
    Pipeline, DEVICE_DATA_KEY,
};
use fiftyone_ip_intelligence::{
    IpIntelligenceData, IpIntelligenceOnPremiseEngineBuilder, IP_DATA_KEY,
};
use fiftyone_pipeline_core::ElementData;
use fiftyone_pipeline_engines::AspectEngine;

/// A mobile Safari on iPhone User-Agent.
const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

/// Cloudflare's public DNS resolver, in every IP Intelligence tier.
const CLOUDFLARE_IPV4: &str = "1.1.1.1";

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

/// A data file named by `variable`, or else the first file at `relative`
/// found from this crate's directory upwards.
fn data_file(variable: &str, relative: &str) -> Option<PathBuf> {
    if let Some(path) = std::env::var(variable)
        .ok()
        .map(|path| PathBuf::from(path.trim()))
        .filter(|path| path.is_file())
    {
        return Some(path);
    }
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join(relative);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// The warning for a read of `name` from `data_type`, with `reason`.
fn warning(name: &str, data_type: &str, reason: impl std::fmt::Debug) -> String {
    format!("WARN Property '{name}' missing from aspect data '{data_type}'. {reason:?}")
}

#[test]
fn each_read_of_a_missing_property_logs_one_warning() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    device_detection();
    ip_intelligence();
}

/// The Hash engine restricted to IsMobile.
fn device_detection() {
    let Some(file) = data_file(
        "_51DEGREES_DD_PATH",
        "cxx/data/device-detection/51Degrees-LiteV4.1.hash",
    ) else {
        eprintln!("no Lite Hash data file found; skipping Device Detection");
        return;
    };
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(file)
        .performance_profile(PerformanceProfile::HighPerformance)
        .property("IsMobile")
        .build()
        .expect("the Lite Hash data set loads");
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder().add_element(element).build().unwrap();
    let mut flow = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", MOBILE_USER_AGENT)
            .build(),
    );
    flow.process().unwrap();
    let device = flow.get(DEVICE_DATA_KEY).unwrap();
    let expected = vec![
        warning(
            "BrowserName",
            "DeviceDataBase",
            engine.missing_property_reason("BrowserName").reason,
        ),
        warning(
            "NotAProperty",
            "DeviceDataBase",
            engine.missing_property_reason("NotAProperty").reason,
        ),
        warning(
            "BrowserName",
            "DeviceDataBase",
            engine.missing_property_reason("BrowserName").reason,
        ),
    ];
    take();

    assert!(*device.is_mobile().value().unwrap());
    assert!(device.get("IsMobile").is_ok());
    assert!(!device.browser_name().has_value());
    assert!(device.get("NotAProperty").is_err());
    assert!(device.get("BrowserName").is_err());
    assert!(!device.has_property("BrowserName"));
    assert!(device.has_property("IsMobile"));

    assert_eq!(take(), expected);
}

/// The IP Intelligence engine restricted to the ASN properties.
fn ip_intelligence() {
    let Some(file) = data_file(
        "_51DEGREES_IPI_PATH",
        "cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi",
    ) else {
        eprintln!("no ASN data file found; skipping IP Intelligence");
        return;
    };
    let engine = Arc::new(
        IpIntelligenceOnPremiseEngineBuilder::new(file)
            .performance_profile(PerformanceProfile::HighPerformance)
            .properties(["Asn", "AsnName"])
            .auto_update(false)
            .file_system_watcher(false)
            .build()
            .expect("the ASN data set loads"),
    );
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder().add_element(element).build().unwrap();
    let mut flow = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("query.client-ip", CLOUDFLARE_IPV4)
            .build(),
    );
    flow.process().unwrap();
    let ip = flow.get(IP_DATA_KEY).unwrap();
    let expected = vec![
        warning(
            "CountryCode",
            "IpIntelligenceDataBase",
            engine.missing_property_reason("CountryCode").reason,
        ),
        warning(
            "NotAProperty",
            "IpIntelligenceDataBase",
            engine.missing_property_reason("NotAProperty").reason,
        ),
    ];
    take();

    assert!(ip.asn().has_value());
    assert!(ip.get("Asn").is_ok());
    assert!(!ip.country_code().has_value());
    assert!(ip.get("NotAProperty").is_err());
    assert!(!ip.has_property("CountryCode"));
    assert!(ip.has_property("AsnName"));

    assert_eq!(take(), expected);
}
