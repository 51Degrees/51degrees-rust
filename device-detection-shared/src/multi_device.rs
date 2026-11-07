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
/// Because a multi-profile container has no scalar properties of its own, its
/// dynamic [`ElementData::get`] reports every name as a no-value and
/// [`ElementData::keys`] is empty. Read the matches through
/// [`MultiDeviceData::profiles`] instead.
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
        // A multi-profile result exposes no scalar properties of its own; each
        // matching device is read through `profiles()`.
        Err(NoValueError::new(format!(
            "No value for property '{name}'. A hardware-profile lookup returns a list of \
             device profiles; read them with MultiDeviceData::profiles()."
        )))
    }

    fn keys(&self) -> Vec<String> {
        Vec::new()
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
    fn dynamic_access_is_empty() {
        let data = MultiDeviceData::new();
        assert!(data.keys().is_empty());
        assert!(data.get("HardwareVendor").is_err());
    }
}
