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
use std::fmt;
use std::sync::{Arc, OnceLock};

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

/// Gives the device data of a profile from its id, for the profiles a
/// [`MultiDeviceData`] holds by id until they are first read.
pub trait ProfileReader: Send + Sync {
    /// The device data of the profile with id `id`. A profile that cannot be
    /// read gives device data whose properties report why.
    fn read_profile(&self, id: u32) -> DeviceDataBase;
}

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
/// A profile is added with its values by
/// [`push_profile`](MultiDeviceData::push_profile), as a cloud response gives
/// them, or by id with
/// [`push_profile_ids`](MultiDeviceData::push_profile_ids), as an on-premise
/// engine finds them. Profiles added by id are read through their
/// [`ProfileReader`] on the first call to
/// [`profiles`](MultiDeviceData::profiles), once, and are read again only if
/// more profiles are added after that.
///
/// Its dynamic [`ElementData`] view lists one property,
/// [`PROFILES_PROPERTY_NAME`], holding a copy of each profile added with
/// its values as a [`PropertyValue::ElementDataList`], so the JSON
/// builder writes `hardware.profiles` with each profile as an object, and
/// a property a profile holds with no value as null with its reason.
/// Profiles added by id are never read for it, as .NET's `MultiDeviceData`
/// keeps them out of the list its JSON builder reads, so on premise
/// `hardware.profiles` is written empty. Any other name is a no-value.
/// Read typed values, without copying, through
/// [`MultiDeviceData::profiles`].
#[derive(Debug, Clone, Default)]
pub struct MultiDeviceData {
    /// The profiles added with their values, in the order added.
    profiles: Vec<DeviceDataBase>,
    /// The profiles added by id, in the order added.
    found: Vec<FoundIds>,
    /// Every profile in the order added, once read. Unset until the first
    /// call to `profiles` when any were added by id.
    read: OnceLock<Vec<DeviceDataBase>>,
}

/// Profiles added by id and the reader that gives their device data.
#[derive(Clone)]
struct FoundIds {
    /// How many profiles had been added with their values before these ids,
    /// which places the ids among them.
    at: usize,
    reader: Arc<dyn ProfileReader>,
    ids: Vec<u32>,
}

impl fmt::Debug for FoundIds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FoundIds")
            .field("at", &self.at)
            .field("ids", &self.ids)
            .finish_non_exhaustive()
    }
}

impl MultiDeviceData {
    /// Create an empty result with no matching profiles.
    pub fn new() -> Self {
        MultiDeviceData::default()
    }

    /// The device profiles that matched the lookup, in the order they were
    /// added. Empty when nothing matched (or, in the cloud, when the resource
    /// key does not grant the hardware-profile-lookup product). Profiles
    /// added by id are read on the first call.
    pub fn profiles(&self) -> &[DeviceDataBase] {
        if self.found.is_empty() {
            return &self.profiles;
        }
        self.read.get_or_init(|| self.read_all())
    }

    /// Consume the result and return the owned list of matching profiles,
    /// reading any added by id that have not been read.
    pub fn into_profiles(mut self) -> Vec<DeviceDataBase> {
        if self.found.is_empty() {
            return self.profiles;
        }
        match self.read.take() {
            Some(read) => read,
            None => self.read_all(),
        }
    }

    /// Append a matched profile, with its values, to the result. It is
    /// written under [`PROFILES_PROPERTY_NAME`] with every property the
    /// profile lists.
    pub fn push_profile(&mut self, profile: DeviceDataBase) {
        self.profiles.push(profile);
        self.read = OnceLock::new();
    }

    /// Append the profiles with ids `ids`, in order, to the result. Each is
    /// read through `reader` on the next call to
    /// [`profiles`](Self::profiles), and none is written under
    /// [`PROFILES_PROPERTY_NAME`].
    pub fn push_profile_ids(&mut self, reader: Arc<dyn ProfileReader>, ids: Vec<u32>) {
        if ids.is_empty() {
            return;
        }
        self.found.push(FoundIds {
            at: self.profiles.len(),
            reader,
            ids,
        });
        self.read = OnceLock::new();
    }

    /// The number of matching profiles, counting those added by id without
    /// reading them.
    pub fn len(&self) -> usize {
        let found: usize = self.found.iter().map(|found| found.ids.len()).sum();
        self.profiles.len() + found
    }

    /// True when no profiles matched.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every profile in the order added, reading those added by id.
    fn read_all(&self) -> Vec<DeviceDataBase> {
        let mut all = Vec::with_capacity(self.len());
        let mut given = self.profiles.iter();
        let mut placed = 0;
        for found in &self.found {
            all.extend(given.by_ref().take(found.at - placed).cloned());
            placed = found.at;
            all.extend(found.ids.iter().map(|&id| found.reader.read_profile(id)));
        }
        all.extend(given.cloned());
        all
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
    use std::sync::atomic::{AtomicUsize, Ordering};

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

    /// Gives a profile holding its id as `ProfileId`, counting each read.
    #[derive(Default)]
    struct CountingReader {
        reads: AtomicUsize,
    }

    impl CountingReader {
        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
        }
    }

    impl ProfileReader for CountingReader {
        fn read_profile(&self, id: u32) -> DeviceDataBase {
            self.reads.fetch_add(1, Ordering::SeqCst);
            DeviceDataBase::new().set("ProfileId", id.to_string())
        }
    }

    fn profile_id(profile: &DeviceDataBase) -> String {
        profile
            .get("ProfileId")
            .expect("each profile holds its id")
            .as_str()
            .expect("the id is text")
            .to_owned()
    }

    fn profile_ids(data: &MultiDeviceData) -> Vec<String> {
        data.profiles().iter().map(profile_id).collect()
    }

    #[test]
    fn profiles_found_by_id_are_read_when_first_read() {
        let reader = Arc::new(CountingReader::default());
        let mut data = MultiDeviceData::new();
        data.push_profile_ids(reader.clone(), Vec::new());
        assert!(data.is_empty());
        data.push_profile_ids(reader.clone(), vec![12280, 17779]);
        // Counting the ids reads none of them.
        assert_eq!(data.len(), 2);
        assert!(!data.is_empty());
        assert_eq!(reader.reads(), 0);

        assert_eq!(profile_ids(&data), ["12280", "17779"]);
        assert_eq!(reader.reads(), 2);
        assert_eq!(profile_ids(&data), ["12280", "17779"]);
        assert_eq!(reader.reads(), 2);
    }

    #[test]
    fn profiles_keep_the_order_they_were_added() {
        let reader: Arc<dyn ProfileReader> = Arc::new(CountingReader::default());
        let mut data = MultiDeviceData::new();
        data.push_profile(DeviceDataBase::new().set("ProfileId", "1"));
        data.push_profile_ids(Arc::clone(&reader), vec![2, 3]);
        data.push_profile(DeviceDataBase::new().set("ProfileId", "4"));
        data.push_profile_ids(Arc::clone(&reader), vec![5]);
        data.push_profile(DeviceDataBase::new().set("ProfileId", "6"));

        assert_eq!(data.len(), 6);
        assert_eq!(profile_ids(&data), ["1", "2", "3", "4", "5", "6"]);
        let owned: Vec<String> = data.into_profiles().iter().map(profile_id).collect();
        assert_eq!(owned, ["1", "2", "3", "4", "5", "6"]);
    }

    #[test]
    fn adding_after_a_read_reads_again() {
        let reader = Arc::new(CountingReader::default());
        let mut data = MultiDeviceData::new();
        data.push_profile_ids(reader.clone(), vec![2]);
        assert_eq!(profile_ids(&data), ["2"]);
        data.push_profile(DeviceDataBase::new().set("ProfileId", "3"));
        assert_eq!(profile_ids(&data), ["2", "3"]);
        data.push_profile_ids(reader.clone(), vec![4]);
        assert_eq!(profile_ids(&data), ["2", "3", "4"]);
        // Each read after an addition reads every id again, as .NET's
        // `AddProfileId` clears the profiles it has read.
        assert_eq!(reader.reads(), 1 + 1 + 2);
    }

    #[test]
    fn into_profiles_reads_ids_not_yet_read() {
        let reader = Arc::new(CountingReader::default());
        let mut data = MultiDeviceData::new();
        data.push_profile_ids(reader.clone(), vec![7, 8]);
        let owned: Vec<String> = data.into_profiles().iter().map(profile_id).collect();
        assert_eq!(owned, ["7", "8"]);
        assert_eq!(reader.reads(), 2);
    }

    #[test]
    fn only_profiles_added_with_their_values_are_written_under_profiles() {
        let reader = Arc::new(CountingReader::default());
        let mut data = MultiDeviceData::new();
        data.push_profile_ids(reader.clone(), vec![12280]);
        assert_eq!(
            data.get("profiles").unwrap(),
            PropertyValue::ElementDataList(ElementDataList::default())
        );

        let apple = DeviceDataBase::new().set("HardwareVendor", "Apple");
        data.push_profile(apple.clone());
        assert_eq!(
            data.get("Profiles").unwrap(),
            PropertyValue::ElementDataList(ElementDataList::new(vec![
                Arc::new(apple) as Arc<dyn ElementData>
            ]))
        );
        // Writing the data reads no profile added by id.
        assert_eq!(reader.reads(), 0);
        assert_eq!(data.len(), 2);
        assert_eq!(profile_id(&data.profiles()[0]), "12280");
    }
}
