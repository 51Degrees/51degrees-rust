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

//! The stored string of a property, got as well as its parsed value, as the
//! [access-to-results specification](https://github.com/51Degrees/specifications/blob/706de1f0cb3edd560eb7fd7ef3bb19b93a4883dc/pipeline-specification/features/access-to-results.md#L66)
//! says: "Where an Element Data instance holds Property values as strings
//! and parses them into the type each Property declares, it SHOULD be
//! possible to get the stored string for a Property as well as the parsed
//! value."

use fiftyone_device_detection_shared::{DeviceData, DeviceDataBase, DevicePropertyReader};
use fiftyone_pipeline_core::PropertyValue;
use fiftyone_pipeline_engines::AspectData;

/// A reader holding each property as the strings a data file stores, and
/// parsing them into the declared type as the on-premise engine does: a
/// boolean is true only for `True`, and a profile with more than one value
/// for a boolean gives no value.
struct StoredReader {
    names: Vec<String>,
    stored: Vec<Vec<&'static str>>,
}

impl StoredReader {
    fn new() -> Self {
        StoredReader {
            names: vec![
                "ismobile".to_string(),
                "istv".to_string(),
                "platformname".to_string(),
                "browsername".to_string(),
            ],
            stored: vec![
                vec!["Unknown"],
                vec!["True", "False"],
                vec!["Android"],
                vec![],
            ],
        }
    }
}

impl DevicePropertyReader for StoredReader {
    fn property_names(&self) -> &[String] {
        &self.names
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|held| held == name)
    }

    fn read_property(&self, position: usize) -> Result<Option<PropertyValue>, String> {
        let stored = &self.stored[position];
        Ok(match (self.names[position].as_str(), stored.as_slice()) {
            (_, []) => None,
            ("ismobile" | "istv", [one]) => Some(PropertyValue::Bool(*one == "True")),
            ("ismobile" | "istv", _) => None,
            (_, [one]) => Some(PropertyValue::String(one.to_string())),
            (_, _) => None,
        })
    }

    fn stores_strings(&self) -> bool {
        true
    }

    fn read_stored_string(
        &self,
        position: usize,
    ) -> Option<Result<Result<String, Option<String>>, String>> {
        let stored = &self.stored[position];
        Some(Ok(if stored.is_empty() {
            Err(Some(format!("no {}", self.names[position])))
        } else {
            Ok(stored.join("|"))
        }))
    }
}

#[test]
fn the_stored_string_is_got_as_well_as_the_parsed_value() {
    let device = DeviceDataBase::new().with_reader(StoredReader::new());

    // The parsed value cannot tell "Unknown" from a real false.
    assert!(!*device.is_mobile().value().unwrap());

    let stored = device
        .values_as_string()
        .expect("data holding values as strings offers them");
    assert_eq!(
        stored.value_as_string("IsMobile").value().unwrap(),
        "Unknown"
    );
    assert_eq!(
        stored.value_as_string("ismobile").value().unwrap(),
        "Unknown"
    );

    // The parsed value is still there, unchanged by reading the string.
    assert!(!*device.is_mobile().value().unwrap());
}

#[test]
fn several_stored_values_are_given_where_the_parsed_value_has_none() {
    let device = DeviceDataBase::new().with_reader(StoredReader::new());
    assert!(!device.is_tv().has_value());
    let stored = device.values_as_string().unwrap();
    assert_eq!(
        stored.value_as_string("IsTv").value().unwrap(),
        "True|False"
    );
}

#[test]
fn a_string_property_gives_the_same_stored_and_parsed_value() {
    let device = DeviceDataBase::new().with_reader(StoredReader::new());
    let stored = device.values_as_string().unwrap();
    assert_eq!(
        stored.value_as_string("PlatformName").value().unwrap(),
        device.platform_name().value().unwrap()
    );
}

#[test]
fn a_property_with_nothing_stored_has_no_stored_string() {
    let device = DeviceDataBase::new().with_reader(StoredReader::new());
    let stored = device.values_as_string().unwrap();
    let browser = stored.value_as_string("BrowserName");
    assert!(!browser.has_value());
    assert_eq!(browser.no_value_message(), Some("no browsername"));
    assert!(!stored.value_as_string("NotAProperty").has_value());
}

#[test]
fn data_that_does_not_hold_strings_does_not_offer_them() {
    // Values set directly, as a cloud engine sets them, are already typed.
    let device = DeviceDataBase::new().set("IsMobile", true);
    assert!(device.values_as_string().is_none());
}
