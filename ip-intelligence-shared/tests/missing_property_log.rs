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

//! What a read of IP Intelligence data logs. A property recorded with no
//! value is held, so its read gives the stored message and logs nothing. Kept
//! in its own test binary because a logger is installed once for the whole
//! process, and in one test so the records it reads are its own.

use std::sync::{Arc, Mutex};

use fiftyone_ip_intelligence_shared::{IpIntelligenceDataBase, IP_DATA_KEY_NAME};
use fiftyone_pipeline_core::{ElementData, PropertyValueType};
use fiftyone_pipeline_engines::{AspectPropertyMetaData, EngineDeployment, MissingPropertySource};

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

/// An engine whose metadata lists `AccuracyRadiusMin` in the tier it uses.
fn source() -> Arc<MissingPropertySource> {
    Arc::new(MissingPropertySource::new(
        IP_DATA_KEY_NAME,
        "TestEngine",
        EngineDeployment::OnPremise,
        "Lite",
        vec![AspectPropertyMetaData::new(
            "AccuracyRadiusMin",
            IP_DATA_KEY_NAME,
            PropertyValueType::Integer,
        )
        .with_data_tiers(["Lite"])],
    ))
}

#[test]
fn a_property_recorded_with_no_value_gives_its_message_and_logs_nothing() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    take();

    let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
    data.set_integer_no_value("AccuracyRadiusMin", "no radius for this range");
    data.base_mut().add_missing_property_source(source());

    assert!(data.has_property("AccuracyRadiusMin"));
    for _ in 0..2 {
        assert_eq!(
            data.get("AccuracyRadiusMin").unwrap_err().message,
            "no radius for this range"
        );
        assert_eq!(
            data.integer("accuracyradiusmin").no_value_message(),
            Some("no radius for this range")
        );
    }
    assert_eq!(take(), Vec::<String>::new());

    // A property the data does not hold still logs, so the logger is
    // listening.
    assert!(!data.has_property("NotAProperty"));
    assert!(data.get("NotAProperty").is_err());
    assert_eq!(
        take(),
        vec![
            "WARN Property 'NotAProperty' missing from aspect data \
             'IpIntelligenceDataBase'. Unknown"
        ]
    );
}
