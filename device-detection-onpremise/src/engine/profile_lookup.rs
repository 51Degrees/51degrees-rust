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

//! The profiles that hold a value of a property, looked up in the data set
//! the engine has loaded.

use std::sync::{Arc, Mutex};

use fiftyone_device_detection_shared::{DeviceDataBase, DevicePropertyReader, ProfileReader};
use fiftyone_pipeline_core::{Evidence, Result};

use super::{
    device_data, DetectionReader, DeviceDetectionOnPremiseEngine, LoadedDataSet, ReadOutcome,
};

/// The property each device data
/// [`profiles_for_property_value`](DeviceDetectionOnPremiseEngine::profiles_for_property_value)
/// gives holds its profile's id in, as text, because the data file has no
/// property that holds it.
pub const PROFILE_ID_PROPERTY_NAME: &str = "ProfileId";

/// The evidence key that gives a detection the profiles whose ids it holds.
const PROFILE_IDS_EVIDENCE_KEY: &str = "query.51D_ProfileIds";

impl DeviceDetectionOnPremiseEngine {
    /// The ids of the profiles in the loaded data set that hold the value
    /// `value` of the property named `property_name`, in the data file's
    /// order, which is ascending profile id. Both are matched exactly,
    /// including case, so a name or value the data file does not hold gives
    /// none. Fails when the data file cannot be read.
    ///
    /// A property named to
    /// [`indexed_property`](crate::DeviceDetectionOnPremiseEngineBuilder::indexed_property)
    /// is found from the index the data set built when it loaded, and any
    /// other by reading every profile, which gives the same ids more slowly.
    /// The property need not be one the engine is restricted to.
    ///
    /// The ids are those of the data set loaded when this is called, and a
    /// refresh can load a data file whose profiles differ. An id given to the
    /// engine as `query.51D_ProfileIds` evidence gives that profile's values,
    /// and [`Self::profiles_for_property_value`] and
    /// [`Self::profile_ids_and_reader_for_property_value`] give the ids and
    /// the values from one data set.
    pub fn profile_ids_for_property_value(
        &self,
        property_name: &str,
        value: &str,
    ) -> Result<Vec<u32>> {
        self.loaded
            .load()
            .manager
            .profile_ids_for_property_value(property_name, value)
    }

    /// The names of the properties the loaded data set indexes the profiles
    /// of by value, being those named to
    /// [`indexed_property`](crate::DeviceDetectionOnPremiseEngineBuilder::indexed_property)
    /// in the order named, including any the data file has no property of.
    /// Empty when none were named. Read from the loaded data set, so after a
    /// refresh it lists what the new data set indexes.
    pub fn indexed_properties(&self) -> Vec<String> {
        self.loaded.load().manager.indexed_properties()
    }

    /// The device data of each profile
    /// [`Self::profile_ids_for_property_value`] finds for `value` of the
    /// property named `property_name`, in the same order. Fails when the data
    /// file cannot be read.
    ///
    /// Each profile's values are those a detection gives for its id as
    /// `query.51D_ProfileIds` evidence, read when first requested, and
    /// [`PROFILE_ID_PROPERTY_NAME`] holds the id as text. The other components'
    /// properties give what such a detection gives them. The ids and every
    /// value come from the data set loaded when this is called, so a refresh
    /// cannot leave an id without its profile.
    pub fn profiles_for_property_value(
        &self,
        property_name: &str,
        value: &str,
    ) -> Result<Vec<DeviceDataBase>> {
        let loaded = self.loaded.load_full();
        let ids = loaded
            .manager
            .profile_ids_for_property_value(property_name, value)?;
        ids.into_iter().map(|id| profile(&loaded, id)).collect()
    }

    /// The ids [`Self::profile_ids_for_property_value`] finds for `value` of
    /// the property named `property_name`, with a reader giving the device
    /// data of each id as [`Self::profiles_for_property_value`] gives it,
    /// which reads nothing until asked. Fails when the data file cannot be
    /// read.
    ///
    /// The ids and the reader use the data set loaded when this is called,
    /// so a refresh cannot leave an id without its profile. Give both to
    /// [`MultiDeviceData::push_profile_ids`](fiftyone_device_detection_shared::MultiDeviceData::push_profile_ids)
    /// to read the profiles only when they are first asked for. A profile the
    /// reader cannot read gives device data holding its id, whose other
    /// properties give why as the reason they have no value.
    pub fn profile_ids_and_reader_for_property_value(
        &self,
        property_name: &str,
        value: &str,
    ) -> Result<(Vec<u32>, Arc<dyn ProfileReader>)> {
        let loaded = self.loaded.load_full();
        let ids = loaded
            .manager
            .profile_ids_for_property_value(property_name, value)?;
        Ok((ids, Arc::new(LoadedProfiles { loaded })))
    }
}

/// The device data of the profile with id `id` in `loaded`, being what a
/// detection given the id as `query.51D_ProfileIds` evidence gives, with
/// [`PROFILE_ID_PROPERTY_NAME`] holding the id as text.
fn profile(loaded: &Arc<LoadedDataSet>, id: u32) -> Result<DeviceDataBase> {
    let mut results = loaded.manager.create_results()?;
    let evidence = Evidence::builder()
        .add(PROFILE_IDS_EVIDENCE_KEY, id.to_string())
        .build();
    results.process_evidence(&evidence)?;
    let mut device = device_data(DetectionReader {
        results: Mutex::new(results),
        loaded: Arc::clone(loaded),
    });
    device.insert(PROFILE_ID_PROPERTY_NAME, id.to_string());
    Ok(device)
}

/// Reads profiles by id from the data set their ids were found in.
struct LoadedProfiles {
    loaded: Arc<LoadedDataSet>,
}

impl ProfileReader for LoadedProfiles {
    fn read_profile(&self, id: u32) -> DeviceDataBase {
        profile(&self.loaded, id).unwrap_or_else(|error| {
            let mut device = DeviceDataBase::new();
            device
                .base_mut()
                .add_missing_property_source(Arc::clone(&self.loaded.missing));
            let mut device = device.with_reader(UnreadProfile {
                loaded: Arc::clone(&self.loaded),
                reason: error.to_string(),
            });
            device.insert(PROFILE_ID_PROPERTY_NAME, id.to_string());
            device
        })
    }
}

/// Gives the reason a profile could not be read as the outcome of reading
/// any of its properties.
struct UnreadProfile {
    loaded: Arc<LoadedDataSet>,
    reason: String,
}

impl DevicePropertyReader for UnreadProfile {
    fn property_names(&self) -> &[String] {
        &self.loaded.readable.lowercase_names
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.loaded.readable.by_name.get(name).copied()
    }

    fn read_property(&self, _position: usize) -> ReadOutcome {
        Err(self.reason.clone())
    }
}
