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

//! The IP Intelligence specification, data-model.md line 37: "Each property
//! MUST also be exposed as a weighted value". A property read as a single
//! value is also read as a weighted list.

use fiftyone_ip_intelligence_shared::{IpIntelligenceDataBase, IP_DATA_KEY_NAME};

/// Reads the weighted list of a property as `(value, raw_weighting)` pairs,
/// or `None` when the property has no weighted value.
fn weighted(data: &IpIntelligenceDataBase, name: &str) -> Option<Vec<(String, u16)>> {
    let value = data.weighted_string(name);
    if !value.has_value() {
        return None;
    }
    Some(
        value
            .value()
            .ok()?
            .iter()
            .map(|item| (item.value.clone(), item.raw_weighting))
            .collect(),
    )
}

/// Every plain property, of each type, is also read as a weighted value
/// holding the value it has, with the full weighting.
#[test]
fn plain_properties_are_also_weighted_values() {
    let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
    data.set_string("RegisteredName", "APNIC");
    data.set_wkt_string("Areas", "POINT(1 2)");
    data.set_float("Latitude", 51.5);
    data.set_integer("AccuracyRadiusMin", 20);
    data.set_boolean("IsBroadband", true);

    for (name, expected) in [
        ("RegisteredName", "APNIC"),
        ("Areas", "POINT(1 2)"),
        ("Latitude", "51.5"),
        ("AccuracyRadiusMin", "20"),
        ("IsBroadband", "True"),
    ] {
        assert_eq!(
            weighted(&data, name),
            Some(vec![(expected.to_string(), u16::MAX)]),
            "{name} is not exposed as a weighted value"
        );
    }
}

/// A plain property with no value has no weighted value either, and gives
/// the same reason.
#[test]
fn plain_property_with_no_value_has_no_weighted_value() {
    let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
    data.set_string_no_value("RegisteredName", "needs a lookup");
    let value = data.weighted_string("RegisteredName");
    assert!(!value.has_value());
    assert_eq!(value.no_value_message(), Some("needs a lookup"));
}
