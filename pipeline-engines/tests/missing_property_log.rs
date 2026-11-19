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

//! What a read of a property that aspect data does not hold logs. .NET's
//! aspect data logs one warning for each such read, naming the reason, unless
//! it was built with no logger. A read for a product the resource key lacks
//! logs nothing, because the cloud engine logs the missing product once.
//! Kept in its own test binary because a logger is installed once for the
//! whole process, and each test holds [`SERIAL`] so the records it reads are
//! its own.

use std::sync::{Arc, Mutex, MutexGuard};

use fiftyone_pipeline_core::{ElementData, PropertyValueType};
use fiftyone_pipeline_engines::{
    AspectDataBase, AspectPropertyMetaData, EngineDeployment, MissingPropertyReason,
    MissingPropertySource,
};

/// Log records at warning level or above, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

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

/// Install the logger, wait for any other test to finish and clear the
/// records it left.
fn start() -> MutexGuard<'static, ()> {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    take();
    guard
}

/// The records logged since the last call.
fn take() -> Vec<String> {
    std::mem::take(&mut *LOGGED.lock().unwrap())
}

/// A cloud engine's details, granting `properties` for the device product.
fn cloud_source(properties: Vec<AspectPropertyMetaData>) -> Arc<MissingPropertySource> {
    Arc::new(MissingPropertySource::new(
        "device",
        "TestCloudEngine",
        EngineDeployment::Cloud,
        "",
        properties,
    ))
}

/// An on-premise engine using the Lite tier, whose metadata lists IsMobile
/// in the Premium tier only.
fn source() -> Arc<MissingPropertySource> {
    Arc::new(MissingPropertySource::new(
        "device",
        "TestEngine",
        EngineDeployment::OnPremise,
        "Lite",
        vec![
            AspectPropertyMetaData::new("IsMobile", "device", PropertyValueType::Bool)
                .with_data_tiers(["Premium"]),
        ],
    ))
}

#[test]
fn each_read_of_a_missing_property_logs_one_warning() {
    let _serial = start();

    let mut data = AspectDataBase::new("device").set("PlatformName", "iOS");
    assert!(data.get("IsMobile").is_err());
    assert_eq!(take(), Vec::<String>::new(), "without a source");

    data.add_missing_property_source(source());
    assert!(data.get("IsMobile").is_err());
    assert!(data.get("NotAProperty").is_err());
    assert!(data.get("IsMobile").is_err());
    assert_eq!(data.get("PlatformName").unwrap().as_str(), Some("iOS"));
    // Asking why a property is missing, or whether the data holds it, logs
    // nothing.
    assert!(data.missing_property_reason("IsMobile").is_some());
    assert!(!data.has_property("IsMobile"));
    assert_eq!(
        take(),
        vec![
            "WARN Property 'IsMobile' missing from aspect data 'AspectDataBase'. \
             DataFileUpgradeRequired",
            "WARN Property 'NotAProperty' missing from aspect data 'AspectDataBase'. \
             Unknown",
            "WARN Property 'IsMobile' missing from aspect data 'AspectDataBase'. \
             DataFileUpgradeRequired",
        ]
    );

    // With logging stopped, as for data .NET builds with no logger, a read
    // gives the same reason and logs nothing.
    data.stop_logging_missing_properties();
    let expected = source().reason("IsMobile").description;
    assert_eq!(data.get("IsMobile").unwrap_err().message, expected);
    assert!(data.get("NotAProperty").is_err());
    assert_eq!(
        data.missing_property("IsMobile", "AspectDataBase")
            .map(|missing| missing.description),
        Some(expected)
    );
    assert_eq!(take(), Vec::<String>::new(), "with logging stopped");
}

/// A cloud engine whose resource key holds no product for its data key
/// lists no properties and logs the missing product once itself, so a read
/// of one of its properties gives the reason and logs nothing, as the cloud
/// aspect engine specification says. A property missing for any other
/// reason still logs a warning on each read.
#[test]
fn a_read_for_a_product_the_key_lacks_logs_nothing() {
    let _serial = start();

    let mut lacking = AspectDataBase::new("device");
    lacking.add_missing_property_source(cloud_source(Vec::new()));
    let expected = lacking.missing_property_reason("IsMobile").unwrap();
    assert_eq!(
        expected.reason,
        MissingPropertyReason::ProductNotAccessibleWithResourceKey
    );
    for _ in 0..3 {
        assert_eq!(
            lacking.get("IsMobile").unwrap_err().message,
            expected.description
        );
        assert_eq!(
            lacking.missing_property("IsMobile", "AspectDataBase"),
            Some(expected.clone())
        );
    }
    assert_eq!(take(), Vec::<String>::new(), "a product the key lacks");

    let mut granted = AspectDataBase::new("device");
    granted.add_missing_property_source(cloud_source(vec![AspectPropertyMetaData::new(
        "IsMobile",
        "device",
        PropertyValueType::Bool,
    )]));
    assert!(granted.get("BrowserName").is_err());
    assert!(granted.get("BrowserName").is_err());
    assert_eq!(
        take(),
        vec![
            "WARN Property 'BrowserName' missing from aspect data 'AspectDataBase'. \
             PropertyNotAccessibleWithResourceKey"
                .to_owned();
            2
        ]
    );
}
