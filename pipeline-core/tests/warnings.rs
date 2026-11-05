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

//! Warnings on the flow data, being messages for the caller that never fail
//! processing, as .NET's `AddWarning` and `GetWarnings` give them. Kept in
//! its own test binary because a logger is installed once for the whole
//! process.

use std::any::Any;
use std::sync::{Arc, Mutex};

use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, FlowWarning,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, Result, TypedKey,
};

/// Log records from every test in this binary, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Keeps every log record in [`LOGGED`].
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        LOGGED
            .lock()
            .unwrap()
            .push(format!("{} {}", record.level(), record.args()));
    }

    fn flush(&self) {}
}

static LOGGER: TestLogger = TestLogger;

/// Send log records to [`LOGGED`]. Safe to call from every test.
fn capture_logs() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Trace);
}

/// The records that mention `text`, so tests running at once do not see each
/// other's.
fn logged_about(text: &str) -> Vec<String> {
    LOGGED
        .lock()
        .unwrap()
        .iter()
        .filter(|record| record.contains(text))
        .cloned()
        .collect()
}

/// Element data holding the number of warnings its element saw.
struct SeenData(MapElementData);

impl ElementData for SeenData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        self.0.get(name)
    }
    fn keys(&self) -> Vec<String> {
        self.0.keys()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// An element that records how many warnings it saw, then adds its own.
struct Warner {
    key: &'static str,
    messages: &'static [&'static str],
    filter: EvidenceKeyFilterWhitelist,
}

impl FlowElement for Warner {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let seen = data.warnings().len() as i64;
        data.get_or_add(TypedKey::<SeenData>::new(self.key), || {
            SeenData(MapElementData::new().set("seen", seen))
        })?;
        for message in self.messages {
            data.add_warning(*message, Some(self));
        }
        Ok(())
    }
    fn data_key(&self) -> &str {
        self.key
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

fn warner(key: &'static str, messages: &'static [&'static str]) -> Arc<dyn FlowElement> {
    Arc::new(Warner {
        key,
        messages,
        filter: EvidenceKeyFilterWhitelist::default(),
    })
}

/// How many warnings the element with `key` saw when it ran.
fn seen(data: &FlowData, key: &str) -> PropertyValue {
    data.get_str(key).unwrap().get("seen").unwrap()
}

fn warning(message: &str, key: &str) -> FlowWarning {
    FlowWarning::new(message, Some(key.to_owned()))
}

#[test]
fn warnings_are_kept_in_order_and_do_not_fail_processing() {
    let pipeline = Pipeline::builder()
        .add_element(warner("warner", &["first warning", "second warning"]))
        .suppress_process_exceptions(false)
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    assert!(data.errors().is_empty());
    assert_eq!(
        data.warnings(),
        [
            warning("first warning", "warner"),
            warning("second warning", "warner")
        ]
    );
}

#[test]
fn a_warning_need_not_come_from_an_element() {
    let pipeline = Pipeline::builder()
        .add_element(warner("quiet", &[]))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    assert!(data.warnings().is_empty());
    data.add_warning("from the caller", None);
    assert_eq!(data.warnings(), [FlowWarning::new("from the caller", None)]);
}

#[test]
fn a_groups_warnings_come_after_the_earlier_ones_in_element_order() {
    let pipeline = Pipeline::builder()
        .add_element(warner("before", &["before the group"]))
        .add_parallel_elements([
            warner("one", &["first in the group"]),
            warner("two", &["second in the group"]),
        ])
        .add_element(warner("after", &["after the group"]))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    assert_eq!(
        data.warnings(),
        [
            warning("before the group", "before"),
            warning("first in the group", "one"),
            warning("second in the group", "two"),
            warning("after the group", "after"),
        ]
    );
    // Each element of the group saw only the warning from before it.
    assert_eq!(seen(&data, "one"), PropertyValue::Integer(1));
    assert_eq!(seen(&data, "two"), PropertyValue::Integer(1));
    assert_eq!(seen(&data, "after"), PropertyValue::Integer(3));
}

#[test]
fn a_warning_is_logged_at_warning_level_naming_the_element_type() {
    capture_logs();
    let pipeline = Pipeline::builder()
        .add_element(warner("logger", &["an evidence value was not used"]))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    data.add_warning("an evidence value from the caller", None);
    assert_eq!(
        logged_about("an evidence value was not used"),
        ["WARN an evidence value was not used (from Warner)"]
    );
    assert_eq!(
        logged_about("an evidence value from the caller"),
        ["WARN an evidence value from the caller"]
    );
}
