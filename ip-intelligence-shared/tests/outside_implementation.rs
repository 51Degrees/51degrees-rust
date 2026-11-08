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

//! An implementation of [`IpIntelligenceData`] written outside these crates.
//! Every accessor has a default body, so such an implementation compiles
//! without writing any accessor, and keeps compiling when a property is added.

use std::any::Any;

use fiftyone_ip_intelligence_shared::{
    IpIntelligenceData, IpIntelligenceDataBase, GENERATED_PROPERTY_TYPES, IP_DATA_KEY_NAME,
};
use fiftyone_pipeline_core::{
    ElementData, NoValueError, PropertyValue, PropertyValueType, WeightedValue, WktString,
};
use fiftyone_pipeline_engines::{AspectData, AspectDataBase, AspectPropertyValue};

/// Implements the supertraits for a struct holding an [`AspectDataBase`] in
/// `base`, as code outside these crates usually does.
macro_rules! aspect_data {
    ($name:ident) => {
        impl ElementData for $name {
            fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
                self.base.get(name)
            }

            fn keys(&self) -> Vec<String> {
                self.base.keys()
            }

            fn as_any(&self) -> &dyn Any {
                self
            }

            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
        }

        impl AspectData for $name {
            fn engine_keys(&self) -> &[String] {
                self.base.engine_keys()
            }

            fn cache_hit(&self) -> bool {
                self.base.cache_hit()
            }
        }
    };
}

/// Writes no accessor.
struct NoAccessors {
    base: AspectDataBase,
}

aspect_data!(NoAccessors);

impl IpIntelligenceData for NoAccessors {}

/// Writes `country` only.
struct CountryOnly {
    base: AspectDataBase,
}

aspect_data!(CountryOnly);

impl IpIntelligenceData for CountryOnly {
    fn country(&self) -> AspectPropertyValue<String> {
        AspectPropertyValue::new("United Kingdom".to_owned())
    }
}

/// The reason a default accessor gives.
fn not_provided(name: &str) -> String {
    format!("Property '{name}' is not provided by this implementation of IpIntelligenceData.")
}

/// The no-value reason of every accessor, or `None` where the accessor has a
/// value, in the order of [`GENERATED_PROPERTY_TYPES`].
fn reasons(data: &dyn IpIntelligenceData) -> Vec<Option<String>> {
    macro_rules! reasons {
        ($data:expr, $($accessor:ident),* $(,)?) => {
            vec![$($data.$accessor().no_value_message().map(str::to_owned)),*]
        };
    }
    reasons!(
        data,
        accuracy_radius_max,
        accuracy_radius_min,
        areas,
        asn,
        asn_name,
        browser_diversity,
        connection_type,
        continent_code2,
        continent_geo_name_id,
        continent_name,
        country,
        country_code,
        country_code3,
        country_codes_geographical,
        country_codes_population,
        country_geo_name_id,
        county,
        county_geo_name_id,
        currency_code,
        dial_code,
        hardware_diversity,
        human_probability,
        ip,
        ip_range_end,
        ip_range_start,
        ip_v6,
        is_broadband,
        is_cellular,
        is_eu,
        is_hosted,
        iso31662_lvl4,
        iso31662_lvl4_subdivision_only,
        iso31662_lvl8,
        iso31662_lvl8_subdivision_only,
        is_proxy,
        is_public_router,
        is_tor,
        is_vpn,
        language_code,
        latitude,
        location_confidence,
        longitude,
        mcc,
        platform_diversity,
        region,
        registered_country,
        registered_country_geo_name_id,
        registered_name,
        registered_owner,
        state,
        state_geo_name_id,
        state_geo_name_ids_geographical,
        suburb,
        time_zone_iana,
        time_zone_offset,
        town,
        town_geo_name_id,
        zip_code,
    )
}

#[test]
fn an_implementation_writing_no_accessor_gives_a_reason_for_each() {
    let data = NoAccessors {
        base: AspectDataBase::new(IP_DATA_KEY_NAME),
    };
    let reasons = reasons(&data);
    assert_eq!(reasons.len(), GENERATED_PROPERTY_TYPES.len());
    for (reason, (name, _)) in reasons.iter().zip(GENERATED_PROPERTY_TYPES) {
        assert_eq!(reason.as_deref(), Some(not_provided(name).as_str()));
    }
    assert_eq!(
        data.state_geo_name_ids_geographical().no_value_message(),
        Some(not_provided("StateGeoNameIdsGeographical").as_str())
    );
}

#[test]
fn an_accessor_the_implementation_writes_is_the_one_used() {
    let data = CountryOnly {
        base: AspectDataBase::new(IP_DATA_KEY_NAME),
    };
    assert_eq!(
        data.country().value().map(String::as_str),
        Ok("United Kingdom")
    );
    assert_eq!(
        data.continent_geo_name_id().no_value_message(),
        Some(not_provided("ContinentGeoNameId").as_str())
    );
}

/// The data base reads every accessor from its stores, so none falls back to
/// the default body.
#[test]
fn the_data_base_reads_every_accessor_from_its_stores() {
    let mut data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
    for (name, value_type) in GENERATED_PROPERTY_TYPES {
        match value_type {
            PropertyValueType::Bool => data.set_boolean(name, true),
            PropertyValueType::Integer => data.set_integer(name, 1),
            PropertyValueType::Float => data.set_float(name, 1.0),
            PropertyValueType::String => data.set_string(name, "text"),
            PropertyValueType::WktString => {
                data.set_wkt_string(name, WktString::new("POINT (0 0)"))
            }
            PropertyValueType::KeyValueList => {
                data.set_weighted_string(name, vec![WeightedValue::new(65535, "text".to_owned())])
            }
            other => panic!("{name} has the type {other:?}, which this test does not set"),
        }
    }
    let reasons = reasons(&data);
    assert_eq!(reasons.len(), GENERATED_PROPERTY_TYPES.len());
    for (reason, (name, _)) in reasons.iter().zip(GENERATED_PROPERTY_TYPES) {
        assert!(reason.is_none(), "{name} gave {reason:?}");
    }
}
