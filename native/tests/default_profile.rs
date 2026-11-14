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

//! `PerformanceProfile::Default` selects each C library's own default
//! configuration global, and is documented as the balanced configuration
//! with no temporary copy of the data file. This checks that the Device
//! Detection default global holds the same configuration as the balanced
//! one. The IP Intelligence configuration is opaque to Rust, so its size is
//! not known here and it is not compared.

#![cfg(feature = "dd")]

use fiftyone_device_detection_sys as sys;

fn bytes_of<T>(value: *const T) -> Vec<u8> {
    // SAFETY: the configuration globals are statics of plain C structs,
    // read here without any other code in this test binary writing them.
    unsafe { std::slice::from_raw_parts(value.cast::<u8>(), std::mem::size_of::<T>()) }.to_vec()
}

#[test]
fn the_device_detection_default_configuration_is_the_balanced_one() {
    let default = bytes_of(std::ptr::addr_of!(sys::fiftyoneDegreesHashDefaultConfig));
    let balanced = bytes_of(std::ptr::addr_of!(sys::fiftyoneDegreesHashBalancedConfig));
    assert!(!default.is_empty());
    assert_eq!(default, balanced);
}
