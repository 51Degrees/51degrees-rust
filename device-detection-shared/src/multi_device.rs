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
use std::sync::Arc;

use crate::DeviceDataBase;
use fiftyone_pipeline_core::{ElementData, ElementDataList, NoValueError, PropertyValue, TypedKey};

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
/// [`PROFILES_PROPERTY_NAME`], holding a copy of the profiles as a
/// [`PropertyValue::ElementDataList`], so the JSON builder writes
/// `hardware.profiles` with each profile as an object, and a property a
/// profile holds with no value as null with its reason. Any other name is
/// a no-value. Read typed values, without copying, through
/// [`MultiDeviceData::profiles`].
#[derive(Debug, Clone, Default)]
pub struct MultiDeviceData {
    profiles: Vec<DeviceDataBase>,
}

impl MultiDeviceData {
    /// Create an empty result with no matching profiles.
    pub fn new() -> Self {
        MultiDeviceData {
            profiles: Vec::new(),
        }
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

    /// Append a matched profile to the result.
    pub fn push_profile(&mut self, profile: DeviceDataBase) {
        self.profiles.push(profile);
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
            let profiles = self
                .profiles
                .iter()
                .map(|profile| Arc::new(profile.clone()) as Arc<dyn ElementData>)
                .collect::<ElementDataList>();
            return Ok(PropertyValue::ElementDataList(profiles));
        }
        Err(NoValueError::new(format!(
            "No value for property '{name}'. A hardware-profile lookup lists \
             its device profiles under '{PROFILES_PROPERTY_NAME}'."
        )))
    }

    fn has_property(&self, name: &str) -> bool {
        name.eq_ignore_ascii_case(PROFILES_PROPERTY_NAME)
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
            PropertyValue::ElementDataList(ElementDataList::default())
        );
        assert!(data.get("HardwareVendor").is_err());
        assert!(data.has_property("profiles"));
        assert!(!data.has_property("HardwareVendor"));
    }

    #[test]
    fn the_profiles_property_holds_each_profile_in_order() {
        let apple = DeviceDataBase::new()
            .set("ProfileId", "12280")
            .set("HardwareVendor", "Apple");
        let mut unknown = DeviceDataBase::new().set("ProfileId", "12281");
        unknown.insert_no_value("HardwareVendor", "No vendor for this TAC.");
        let mut data = MultiDeviceData::new();
        data.push_profile(apple.clone());
        data.push_profile(unknown.clone());

        let expected = PropertyValue::ElementDataList(ElementDataList::new(vec![
            Arc::new(apple) as Arc<dyn ElementData>,
            Arc::new(unknown) as Arc<dyn ElementData>,
        ]));
        // The JSON builder asks in lower case.
        assert_eq!(data.get("profiles").unwrap(), expected);
        assert_eq!(data.get("Profiles").unwrap(), expected);

        // A property a profile holds with no value keeps its reason.
        let Ok(PropertyValue::ElementDataList(profiles)) = data.get("profiles") else {
            panic!("the profiles are a list of element data");
        };
        let second = profiles.iter().nth(1).expect("two profiles");
        assert_eq!(
            second.get("HardwareVendor").unwrap_err().message,
            "No vendor for this TAC."
        );
    }
}
