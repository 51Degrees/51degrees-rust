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

//! The stored string of a property, offered beside the parsed value where
//! the device data holds its values as strings, as the
//! [access-to-results specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/access-to-results.md#accessing-results)
//! and its
//! [values stored as strings](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/value-types.md#values-stored-as-strings)
//! describe.

use fiftyone_device_detection_shared::{DeviceData, DeviceDataBase, DevicePropertyReader};
use fiftyone_pipeline_core::PropertyValue;

/// The reason the reader gives for the property it holds with no value.
const NO_PROFILE: &str = "No matching profiles could be found for the supplied evidence.";

/// A reader holding each value as the string a data file stores, and
/// parsing it as the C library does, so a Boolean is true only for `True`.
struct StoredReader {
    names: Vec<String>,
    stored: Vec<Option<&'static str>>,
    holds_strings: bool,
}

impl StoredReader {
    fn new(holds_strings: bool) -> Self {
        StoredReader {
            names: ["ismobile", "platformname", "browsername"]
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
            stored: vec![Some("Unknown"), Some("Android"), None],
            holds_strings,
        }
    }

    fn parsed(&self, position: usize, text: &str) -> PropertyValue {
        if self.names[position] == "ismobile" {
            PropertyValue::Bool(text == "True")
        } else {
            PropertyValue::from(text)
        }
    }
}

impl DevicePropertyReader for StoredReader {
    fn property_names(&self) -> &[String] {
        &self.names
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|known| known == name)
    }

    fn read_property(&self, position: usize) -> Result<Option<PropertyValue>, String> {
        Ok(self.stored[position].map(|text| self.parsed(position, text)))
    }

    fn read_property_with_reason(
        &self,
        position: usize,
    ) -> Result<Result<PropertyValue, Option<String>>, String> {
        Ok(match self.stored[position] {
            Some(text) => Ok(self.parsed(position, text)),
            None => Err(Some(NO_PROFILE.to_owned())),
        })
    }

    fn holds_values_as_strings(&self) -> bool {
        self.holds_strings
    }

    fn read_stored_string(
        &self,
        position: usize,
    ) -> Option<Result<Result<String, Option<String>>, String>> {
        if !self.holds_strings {
            return None;
        }
        Some(Ok(match self.stored[position] {
            Some(text) => Ok(text.to_owned()),
            None => Err(Some(NO_PROFILE.to_owned())),
        }))
    }
}

#[test]
fn the_stored_string_is_given_beside_the_parsed_value() {
    let device = DeviceDataBase::new().with_reader(StoredReader::new(true));
    let stored = device
        .values_as_string()
        .expect("data whose reader holds strings offers them");

    // "Unknown" in a Boolean property parses as false, and the stored
    // string tells it apart from a stored "False".
    assert_eq!(
        stored.value_as_string("IsMobile").value().unwrap(),
        "Unknown"
    );
    assert!(!*device.is_mobile().value().unwrap());
    assert_eq!(
        stored.value_as_string("platformname").value().unwrap(),
        "Android"
    );
}

#[test]
fn reading_the_stored_string_leaves_the_typed_accessor_unchanged() {
    let device = DeviceDataBase::new().with_reader(StoredReader::new(true));
    let stored = device.values_as_string().expect("strings are offered");

    stored.value_as_string("IsMobile");
    assert_eq!(device.is_mobile().value().ok(), Some(&false));
    assert_eq!(
        device.platform_name().value().ok().map(String::as_str),
        Some("Android")
    );
}

#[test]
fn a_property_with_no_value_is_reported_as_every_accessor_reports_it() {
    let device = DeviceDataBase::new().with_reader(StoredReader::new(true));
    let stored = device.values_as_string().expect("strings are offered");

    let browser = stored.value_as_string("BrowserName");
    assert!(!browser.has_value());
    assert_eq!(browser.no_value_message(), Some(NO_PROFILE));
    assert_eq!(
        browser.no_value_message(),
        device.browser_name().no_value_message()
    );

    let unknown = stored.value_as_string("NotAProperty");
    assert!(!unknown.has_value());
    assert_eq!(
        unknown.no_value_message(),
        device.string_property("NotAProperty").no_value_message()
    );
}

#[test]
fn data_that_holds_no_strings_does_not_offer_them() {
    let cloud = DeviceDataBase::new().set("IsMobile", true);
    assert!(cloud.values_as_string().is_none());

    let parsed_only = DeviceDataBase::new().with_reader(StoredReader::new(false));
    assert!(parsed_only.values_as_string().is_none());
}
