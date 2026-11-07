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

//! The profiles a Hash data file holds, read in the order the C++
//! `ProfileMetaDataCollectionHash` in 51degrees-cxx reads them.

use fiftyone_device_detection_sys as sys;
use fiftyone_pipeline_core::Result;

use super::{metadata_read_failed, Manager};

/// One profile a data file holds, which is one set of values for the
/// properties of one component, such as a single crawler or a single model of
/// hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct DataFileProfile {
    /// The profile's id, which stays the same from one data file to the next.
    /// [`super::Results::process_device_id`] takes it to give the profile's
    /// values.
    pub profile_id: u32,
    /// The id of the component the profile belongs to, as
    /// [`super::DataFileComponent::component_id`] gives it.
    pub component_id: u8,
}

impl Manager {
    /// Every profile the data file holds, in the data file's order, which is
    /// the order of profile id.
    ///
    /// A profile that cannot be read is an error rather than left out, as the
    /// C++ code the other SDKs call raises one.
    pub fn data_file_profiles(&self) -> Result<Vec<DataFileProfile>> {
        let _file_read = self.file_read();
        // Safety: the manager is initialized for the lifetime of `self`.
        let count = unsafe { sys::fiftyoneDegreesShimHashGetProfileCount(self.as_ptr()) };
        let mut profiles = Vec::with_capacity(count as usize);
        for index in 0..count {
            let profile = self
                .read_profile(index)
                .ok_or_else(|| metadata_read_failed(&format!("profile {index}")))?;
            profiles.push(profile);
        }
        Ok(profiles)
    }

    /// The profile at `index` among the data file's profiles, or [`None`]
    /// when the index is out of range or the profile cannot be read. The
    /// caller holds the file read permit.
    fn read_profile(&self, index: u32) -> Option<DataFileProfile> {
        let mut profile_id = 0u32;
        let mut component_id = 0u8;
        // Safety: both output pointers are writable locals.
        let read = unsafe {
            sys::fiftyoneDegreesShimHashGetProfile(
                self.as_ptr(),
                index,
                &mut profile_id,
                &mut component_id,
            )
        };
        (read != 0).then_some(DataFileProfile {
            profile_id,
            component_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::super::tests::{data_file, METADATA_PROFILES};
    use super::*;

    #[test]
    fn every_profile_is_listed_in_order_of_id_with_its_component() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping profiles");
            return;
        };
        for performance in METADATA_PROFILES {
            let name = performance.as_str();
            let manager = Manager::open(&path, performance).expect("data file loads");
            let profiles = manager.data_file_profiles().expect("the profiles are read");
            // Safety: the manager is initialized for the lifetime of `manager`.
            let count = unsafe { sys::fiftyoneDegreesShimHashGetProfileCount(manager.as_ptr()) };
            assert_eq!(profiles.len(), count as usize, "{name}");
            assert!(!profiles.is_empty(), "{name}");
            assert!(
                profiles
                    .windows(2)
                    .all(|pair| pair[0].profile_id < pair[1].profile_id),
                "{name}: the profiles are in order of id"
            );

            let components = manager
                .data_file_components()
                .expect("the components are read");
            let component_ids: HashSet<u8> = components.iter().map(|c| c.component_id).collect();
            assert!(
                profiles
                    .iter()
                    .all(|p| component_ids.contains(&p.component_id)),
                "{name}: every profile belongs to a component the file defines"
            );
            for component in &components {
                let default = DataFileProfile {
                    profile_id: component.default_profile_id,
                    component_id: component.component_id,
                };
                assert!(
                    profiles.contains(&default),
                    "{name}: the default profile of {} is listed with it",
                    component.name
                );
            }
        }
    }

    #[test]
    fn a_listed_profile_gives_its_values() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping profile values");
            return;
        };
        let manager =
            Manager::open(&path, crate::PerformanceProfile::InMemory).expect("data file loads");
        let profiles = manager.data_file_profiles().expect("the profiles are read");
        let hardware = profiles
            .iter()
            .find(|p| p.component_id == 1)
            .expect("the data file holds a hardware profile");
        let mut results = manager.create_results().expect("results allocate");
        let found = results
            .process_device_id(&hardware.profile_id.to_string())
            .expect("the profile is read");
        assert_eq!(found, 1);
        assert!(results.has_values("IsMobile").expect("IsMobile is read"));
    }

    #[test]
    fn the_shim_reads_no_profile_past_the_end() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the profile shim");
            return;
        };
        for performance in METADATA_PROFILES {
            let name = performance.as_str();
            let manager = Manager::open(&path, performance).expect("data file loads");
            let _file_read = manager.file_read();
            // Safety: the manager is initialized for the lifetime of `manager`.
            let count = unsafe { sys::fiftyoneDegreesShimHashGetProfileCount(manager.as_ptr()) };
            assert!(manager.read_profile(count - 1).is_some(), "{name}");
            for index in [count, u32::MAX] {
                assert_eq!(manager.read_profile(index), None, "{name} {index}");
            }
        }
    }
}
