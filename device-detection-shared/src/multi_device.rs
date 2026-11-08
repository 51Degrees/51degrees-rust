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

//! The multi-profile result type produced by the hardware-profile lookup,
//! in the cloud and on premise.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::DeviceDataBase;
use fiftyone_pipeline_core::{ElementData, NoValueError, PropertyValue, TypedKey};

/// The element-data key string the hardware-profile lookup stores its result
/// under, which is `"hardware"`.
pub const HARDWARE_ELEMENT_DATA_KEY: &str = "hardware";

/// The typed key a caller reads the hardware-profile lookup result back through.
///
/// ```no_run
/// # use fiftyone_pipeline_core::FlowData;
/// # use fiftyone_device_detection_shared::{DeviceData, HARDWARE_DATA_KEY};
/// # fn demo(data: &FlowData) {
/// if let Some(hardware) = data.get(HARDWARE_DATA_KEY) {
///     for profile in hardware.profiles() {
///         println!("{:?}", profile.hardware_vendor().value());
///     }
/// }
/// # }
/// ```
pub const HARDWARE_DATA_KEY: TypedKey<MultiDeviceData> = TypedKey::new(HARDWARE_ELEMENT_DATA_KEY);

/// The one property a [`MultiDeviceData`] lists, `Profiles`, holding every
/// matched profile, as .NET's `MultiDeviceData` names it.
pub const PROFILES_PROPERTY_NAME: &str = "Profiles";

/// The result of a hardware-profile lookup: the list of device profiles that
/// matched the supplied parameter (a TAC or a native model name).
///
/// A single TAC or native model name can match several device profiles, so the
/// lookup gives a list rather than a single device. This type carries a list of
/// [`DeviceDataBase`] profiles, each interface-compatible with the result of a
/// normal single-device detection, so the same typed accessors
/// ([`DeviceData`](crate::DeviceData)) read each one. The cloud and on-premise
/// lookups both store this type under [`HARDWARE_DATA_KEY`], so one lookup can
/// add its profiles to another's.
///
/// Its dynamic [`ElementData`] view lists one property,
/// [`PROFILES_PROPERTY_NAME`], holding each profile's values as a
/// [`PropertyValue::KeyValueList`], so the JSON builder writes
/// `hardware.profiles`. The record of a profile added with
/// [`push_profile_with_properties`](MultiDeviceData::push_profile_with_properties)
/// holds the properties named there, which an on-premise engine gives as
/// the item properties it lists for `Profiles`. The record of a profile
/// added with [`push_profile`](MultiDeviceData::push_profile) holds every
/// property the profile lists, as a cloud response gives them. Each record
/// is keyed by the lower case property name and leaves out a property the
/// profile holds with no value. Any other name is a no-value. Read typed
/// values through [`MultiDeviceData::profiles`].
#[derive(Debug, Clone, Default)]
pub struct MultiDeviceData {
    profiles: Vec<DeviceDataBase>,
    /// For each profile, in the same order, the properties its record
    /// holds, or [`None`] for every property the profile lists.
    record_properties: Vec<Option<Arc<[String]>>>,
}

impl MultiDeviceData {
    /// Create an empty result with no matching profiles.
    pub fn new() -> Self {
        MultiDeviceData::default()
    }

    /// The device profiles that matched the lookup, in the order they were
    /// added. Empty when nothing matched (or, in the cloud, when the resource
    /// key does not grant the hardware-profile-lookup product).
    pub fn profiles(&self) -> &[DeviceDataBase] {
        &self.profiles
    }

    /// Consume the result and return the owned list of matching profiles.
    pub fn into_profiles(self) -> Vec<DeviceDataBase> {
        self.profiles
    }

    /// Append a matched profile to the result. Its record under
    /// [`PROFILES_PROPERTY_NAME`] holds every property the profile lists.
    pub fn push_profile(&mut self, profile: DeviceDataBase) {
        self.profiles.push(profile);
        self.record_properties.push(None);
    }

    /// Append a matched profile whose record under
    /// [`PROFILES_PROPERTY_NAME`] holds only the properties named in
    /// `properties`, in any case. One list can be shared by every profile an
    /// engine adds.
    pub fn push_profile_with_properties(
        &mut self,
        profile: DeviceDataBase,
        properties: Arc<[String]>,
    ) {
        self.profiles.push(profile);
        self.record_properties.push(Some(properties));
    }

    /// The number of matching profiles.
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// True when no profiles matched.
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}

impl ElementData for MultiDeviceData {
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
        if name.eq_ignore_ascii_case(PROFILES_PROPERTY_NAME) {
            return Ok(PropertyValue::KeyValueList(
                self.profiles
                    .iter()
                    .zip(&self.record_properties)
                    .map(|(profile, properties)| profile_record(profile, properties.as_deref()))
                    .collect(),
            ));
        }
        Err(NoValueError::new(format!(
            "No value for property '{name}'. A hardware-profile lookup lists \
             its device profiles under '{PROFILES_PROPERTY_NAME}'."
        )))
    }

    fn keys(&self) -> Vec<String> {
        vec![PROFILES_PROPERTY_NAME.to_owned()]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// One profile's values of `properties`, or of every property it lists when
/// there is no list, keyed by the lower case property name and leaving out
/// each property the profile holds with no value.
fn profile_record(
    profile: &DeviceDataBase,
    properties: Option<&[String]>,
) -> BTreeMap<String, PropertyValue> {
    let listed;
    let names = match properties {
        Some(properties) => properties,
        None => {
            listed = profile.keys();
            listed.as_slice()
        }
    };
    names
        .iter()
        .filter_map(|name| {
            profile
                .get(name)
                .ok()
                .map(|value| (name.to_lowercase(), value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceData;

    #[test]
    fn key_string_is_hardware() {
        assert_eq!(HARDWARE_ELEMENT_DATA_KEY, "hardware");
        assert_eq!(HARDWARE_DATA_KEY.name(), "hardware");
    }

    #[test]
    fn collects_and_reads_profiles() {
        let mut data = MultiDeviceData::new();
        assert!(data.is_empty());

        data.push_profile(DeviceDataBase::new().set("HardwareVendor", "Apple"));
        data.push_profile(DeviceDataBase::new().set("HardwareVendor", "Samsung"));

        assert_eq!(data.len(), 2);
        assert_eq!(
            data.profiles()[0].hardware_vendor().value().unwrap(),
            "Apple"
        );
        assert_eq!(
            data.profiles()[1].hardware_vendor().value().unwrap(),
            "Samsung"
        );
    }

    #[test]
    fn no_profiles_list_an_empty_profiles_property() {
        let data = MultiDeviceData::new();
        assert_eq!(data.keys(), ["Profiles"]);
        assert_eq!(
            data.get("Profiles").unwrap(),
            PropertyValue::KeyValueList(Vec::new())
        );
        assert!(data.get("HardwareVendor").is_err());
    }

    #[test]
    fn the_profiles_property_holds_each_profiles_values_in_order() {
        let mut data = MultiDeviceData::new();
        data.push_profile(
            DeviceDataBase::new()
                .set("ProfileId", "12280")
                .set("HardwareVendor", "Apple"),
        );
        data.push_profile(DeviceDataBase::new().set("HardwareVendor", "Samsung"));

        let record = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), PropertyValue::from(*value)))
                .collect::<BTreeMap<_, _>>()
        };
        // The data lists its property names in lower case, as the other
        // records in the crates are keyed.
        let expected = PropertyValue::KeyValueList(vec![
            record(&[("hardwarevendor", "Apple"), ("profileid", "12280")]),
            record(&[("hardwarevendor", "Samsung")]),
        ]);
        // The JSON builder asks in lower case.
        assert_eq!(data.get("profiles").unwrap(), expected);
        assert_eq!(data.get("Profiles").unwrap(), expected);
    }

    #[test]
    fn a_profile_added_with_its_properties_records_only_those() {
        let properties: Arc<[String]> = ["ProfileId", "HardwareVendor", "HardwareModel"]
            .map(str::to_owned)
            .into();
        let mut data = MultiDeviceData::new();
        data.push_profile_with_properties(
            DeviceDataBase::new()
                .set("ProfileId", "12280")
                .set("HardwareVendor", "Apple")
                .set("BrowserName", "Safari"),
            properties,
        );
        // A profile added without a list, as the cloud adds them, keeps
        // every property it lists.
        data.push_profile(
            DeviceDataBase::new()
                .set("HardwareVendor", "Samsung")
                .set("BrowserName", "Chrome"),
        );

        let record = |pairs: &[(&str, &str)]| {
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_owned(), PropertyValue::from(*value)))
                .collect::<BTreeMap<_, _>>()
        };
        // `BrowserName` is not listed and `HardwareModel` has no value, so
        // neither is in the first record.
        assert_eq!(
            data.get("profiles").unwrap(),
            PropertyValue::KeyValueList(vec![
                record(&[("hardwarevendor", "Apple"), ("profileid", "12280")]),
                record(&[("browsername", "Chrome"), ("hardwarevendor", "Samsung")]),
            ])
        );
        assert_eq!(data.len(), 2);
        assert_eq!(data.profiles()[0].browser_name().value().unwrap(), "Safari");
    }
}
