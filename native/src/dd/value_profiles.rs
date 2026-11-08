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

//! The profiles that hold a value of a property, found from the index of
//! profiles by value the C library builds when a data set loads naming the
//! properties to index, or by reading every profile for any other property.

use std::ffi::CString;
use std::os::raw::{c_char, c_int};

use fiftyone_device_detection_sys as sys;
use fiftyone_pipeline_core::{Error, Result};

use super::{read_metadata_string, Manager};
use crate::status::NativeException;

/// The number of profile ids a lookup first has room for. A value more
/// profiles hold is looked up again with room for all of them.
const FIRST_PROFILE_IDS_LENGTH: usize = 64;

/// The names of the properties to index, held as the C init functions take
/// them, being an array of pointers to null terminated strings.
pub(super) struct IndexedNames {
    /// The names, which the pointers point into. A `CString` keeps its bytes
    /// where they are when it moves, so the pointers stay valid.
    _names: Vec<CString>,
    pointers: Vec<*const c_char>,
    count: c_int,
}

impl IndexedNames {
    /// The names to pass to an init function, or an error when one holds a
    /// null byte, as a property name passed to the data set does.
    pub(super) fn new(names: &[String]) -> Result<Self> {
        let invalid = |message: &str| Error::Native {
            status: String::from("InvalidInput"),
            message: message.to_owned(),
        };
        let names = names
            .iter()
            .map(|name| CString::new(name.as_str()))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| invalid("an indexed property name contains an interior nul byte"))?;
        let count = c_int::try_from(names.len())
            .map_err(|_| invalid("more indexed property names than the C library takes"))?;
        let pointers = names.iter().map(|name| name.as_ptr()).collect();
        Ok(Self {
            _names: names,
            pointers,
            count,
        })
    }

    /// The array of names, or null when there are none, which indexes none
    /// as the plain init functions do.
    pub(super) fn as_ptr(&self) -> *const *const c_char {
        if self.pointers.is_empty() {
            std::ptr::null()
        } else {
            self.pointers.as_ptr()
        }
    }

    /// The number of names in the array.
    pub(super) fn count(&self) -> c_int {
        self.count
    }
}

impl Manager {
    /// The ids of the profiles that hold the value `value` of the property
    /// `property_name`, in the data file's order, which is ascending profile
    /// id. Both are matched exactly, so a name or value in other case, or one
    /// the data file does not have, gives none.
    ///
    /// A property named in
    /// [`HashOptions::indexed_properties`](super::HashOptions::indexed_properties)
    /// when the data set loaded is found from the index built then, and any
    /// other by reading every profile, which gives the same ids more slowly.
    /// [`Manager::indexed_properties`] lists the properties indexed. A name or
    /// value holding a null byte gives none, as no data file string holds one.
    pub fn profile_ids_for_property_value(
        &self,
        property_name: &str,
        value: &str,
    ) -> Result<Vec<u32>> {
        self.profile_ids_with_room(property_name, value, FIRST_PROFILE_IDS_LENGTH)
    }

    /// [`Self::profile_ids_for_property_value`], first making room for
    /// `room` ids.
    fn profile_ids_with_room(
        &self,
        property_name: &str,
        value: &str,
        room: usize,
    ) -> Result<Vec<u32>> {
        let (Ok(property_c), Ok(value_c)) = (CString::new(property_name), CString::new(value))
        else {
            return Ok(Vec::new());
        };
        let mut ids = vec![0u32; room];
        let _file_read = self.file_read();
        loop {
            let length = u32::try_from(ids.len()).unwrap_or(u32::MAX);
            let mut exception = sys::Exception::cleared();
            // Safety: the manager is initialized for the lifetime of `self`,
            // both names are null terminated, and `ids` has room for `length`
            // ids.
            let count = unsafe {
                sys::fiftyoneDegreesHashGetProfileIdsForPropertyAndValue(
                    self.as_ptr(),
                    property_c.as_ptr(),
                    value_c.as_ptr(),
                    ids.as_mut_ptr(),
                    length,
                    &mut exception,
                )
            };
            exception.check()?;
            // The count is of every profile holding the value, so when it is
            // more than there was room for, the lookup runs again with room
            // for them all.
            if count <= length {
                ids.truncate(count as usize);
                return Ok(ids);
            }
            ids.resize(count as usize, 0);
        }
    }

    /// The names of the properties the data set indexes the profiles of by
    /// value, being those in
    /// [`HashOptions::indexed_properties`](super::HashOptions::indexed_properties)
    /// in the order given, less repeats. A name the data file has no property
    /// of is kept, as the C library keeps it. Empty when none were named.
    pub fn indexed_properties(&self) -> Vec<String> {
        // Safety: the manager is initialized for the lifetime of `self`.
        let count = unsafe { sys::fiftyoneDegreesShimHashGetIndexedPropertyCount(self.as_ptr()) };
        (0..count)
            .filter_map(|index| {
                read_metadata_string(|buffer, length| {
                    // Safety: the buffer is writable for `length` bytes.
                    unsafe {
                        sys::fiftyoneDegreesShimHashGetIndexedPropertyName(
                            self.as_ptr(),
                            index,
                            buffer,
                            length,
                        )
                    }
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::data_file;
    use super::super::HashOptions;
    use super::*;
    use crate::profile::PerformanceProfile;

    const BROWSER_NAME: &str = "BrowserName";

    /// Room for one id makes a lookup run again for a value more profiles
    /// hold, and it gives the ids room for all of them gives, from the index
    /// and from reading every profile.
    #[test]
    fn a_lookup_with_room_for_one_id_runs_again() {
        let Some(path) = data_file() else {
            eprintln!("no Hash data file found; skipping the second lookup");
            return;
        };
        let indexed = HashOptions {
            indexed_properties: vec![String::from(BROWSER_NAME)],
            ..HashOptions::default()
        };
        for options in [indexed, HashOptions::default()] {
            let manager = Manager::open_with_hash_options(
                &path,
                PerformanceProfile::InMemory,
                None,
                &options,
            )
            .expect("the data file loads");
            let Some(property) = manager
                .data_file_properties()
                .expect("the properties read")
                .into_iter()
                .find(|property| property.name == BROWSER_NAME)
            else {
                eprintln!("the data file has no {BROWSER_NAME}; skipping the second lookup");
                return;
            };
            let (value, ids) = manager
                .data_file_property_values(property.index)
                .expect("the values read")
                .into_iter()
                .map(|value| {
                    let ids = manager
                        .profile_ids_for_property_value(BROWSER_NAME, &value.name)
                        .expect("the lookup succeeds");
                    (value.name, ids)
                })
                .find(|(_, ids)| ids.len() > 1)
                .expect("a value more than one profile holds");
            for room in [1, ids.len() - 1, ids.len()] {
                assert_eq!(
                    manager
                        .profile_ids_with_room(BROWSER_NAME, &value, room)
                        .expect("the lookup succeeds"),
                    ids,
                    "{value:?} with room for {room}, indexed {:?}",
                    manager.indexed_properties()
                );
            }
            assert!(manager
                .profile_ids_with_room(BROWSER_NAME, "No such browser", 1)
                .expect("the lookup succeeds")
                .is_empty());
        }
    }
}
