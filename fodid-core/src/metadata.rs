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

//! The metadata of the six 51Did properties.
//!
//! Every engine producing 51Dids describes its properties the same way, so
//! the metadata is built here once. The 51Degrees form
//! ([`default_fiftyone_property_metadata`]) is the source, and the aspect and
//! core forms are views of it, so the three never disagree.

use fiftyone_pipeline_core::{PropertyMetaData, PropertyValueType};
use fiftyone_pipeline_engines::AspectPropertyMetaData;
use fiftyone_pipeline_engines_fiftyone::{
    ComponentMetaData, FiftyOneAspectPropertyMetaData, ValueMetaData,
};

use crate::data::{
    FODID_ELEMENT_DATA_KEY, ID_HEM_GLOBAL_PROPERTY, ID_HEM_LIC_PROPERTY, ID_PROB_GLOBAL_PROPERTY,
    ID_PROB_LIC_PROPERTY, ID_RAND_GLOBAL_PROPERTY, ID_RAND_LIC_PROPERTY,
};

/// The category every 51Did property belongs to.
pub const CATEGORY_NAME: &str = "FODid";

/// The name of the component every 51Did property belongs to.
pub const COMPONENT_NAME: &str = "FODid";

/// The 51Did component's id, which no data file component has.
const COMPONENT_ID: u8 = 255;

/// The display order of every 51Did property, the last place.
const DISPLAY_ORDER: u8 = 255;

/// The value every 51Did property has when nothing else is known.
const DEFAULT_VALUE: &str = "N/A";

/// Each property's name and description, in the order of
/// [`IDENTIFIER_PROPERTIES`](crate::IDENTIFIER_PROPERTIES).
const DESCRIPTIONS: [(&str, &str); 6] = [
    (
        ID_PROB_GLOBAL_PROPERTY,
        "Probabilistic 51Did, unique across all callers from the same device and network.",
    ),
    (
        ID_PROB_LIC_PROPERTY,
        "Probabilistic 51Did, unique only across the caller’s license key.",
    ),
    (
        ID_RAND_GLOBAL_PROPERTY,
        "Random 51Did carrying a server-generated GUID, unique across all callers.",
    ),
    (
        ID_RAND_LIC_PROPERTY,
        "Random 51Did carrying a server-generated GUID, scoped to the caller’s license key.",
    ),
    (
        ID_HEM_GLOBAL_PROPERTY,
        "Hashed-email 51Did derived from the supplied email and salt, unique across all callers.",
    ),
    (
        ID_HEM_LIC_PROPERTY,
        "Hashed-email 51Did derived from the supplied email and salt, scoped to the caller’s license key.",
    ),
];

/// The six 51Did properties as 51Degrees describes them, in the order of
/// [`IDENTIFIER_PROPERTIES`](crate::IDENTIFIER_PROPERTIES).
///
/// Each is a string in the category [`CATEGORY_NAME`] and the component
/// [`COMPONENT_NAME`], available, in no data tier, with the default value
/// `N/A`, no listed values and a description.
pub fn default_fiftyone_property_metadata() -> Vec<FiftyOneAspectPropertyMetaData> {
    DESCRIPTIONS
        .into_iter()
        .map(|(name, description)| {
            let aspect = AspectPropertyMetaData::from_core(
                PropertyMetaData::new(name, FODID_ELEMENT_DATA_KEY, PropertyValueType::String)
                    .with_category(CATEGORY_NAME),
            )
            .with_description(description);
            FiftyOneAspectPropertyMetaData::from_aspect(aspect)
                .with_component_name(COMPONENT_NAME)
                .with_component_id(COMPONENT_ID)
                .with_display_order(DISPLAY_ORDER)
                .with_default_value(ValueMetaData::new(DEFAULT_VALUE))
        })
        .collect()
}

/// The 51Did component, named [`COMPONENT_NAME`] and holding
/// [`default_fiftyone_property_metadata`].
pub fn default_component_metadata() -> ComponentMetaData {
    ComponentMetaData::new(COMPONENT_ID, COMPONENT_NAME)
        .with_properties(default_fiftyone_property_metadata())
}

/// The aspect view of [`default_fiftyone_property_metadata`].
pub fn default_aspect_property_metadata() -> Vec<AspectPropertyMetaData> {
    default_fiftyone_property_metadata()
        .iter()
        .map(|property| property.aspect().clone())
        .collect()
}

/// The core view of [`default_fiftyone_property_metadata`].
///
/// An engine that learns its property list at run time, such as the cloud
/// engine, reports this set until it has that list.
pub fn default_property_metadata() -> Vec<PropertyMetaData> {
    default_fiftyone_property_metadata()
        .iter()
        .map(|property| property.core().clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IDENTIFIER_PROPERTIES;

    /// The names and descriptions, written out here so a change to either
    /// fails a test. The apostrophes are U+2019.
    const EXPECTED: [(&str, &str); 6] = [
        (
            "IdProbGlobal",
            "Probabilistic 51Did, unique across all callers from the same device and network.",
        ),
        (
            "IdProbLic",
            "Probabilistic 51Did, unique only across the caller’s license key.",
        ),
        (
            "IdRandGlobal",
            "Random 51Did carrying a server-generated GUID, unique across all callers.",
        ),
        (
            "IdRandLic",
            "Random 51Did carrying a server-generated GUID, scoped to the caller’s license key.",
        ),
        (
            "IdHemGlobal",
            "Hashed-email 51Did derived from the supplied email and salt, unique across all callers.",
        ),
        (
            "IdHemLic",
            "Hashed-email 51Did derived from the supplied email and salt, scoped to the caller’s license key.",
        ),
    ];

    #[test]
    fn the_names_and_descriptions_are_in_order() {
        let properties = default_fiftyone_property_metadata();
        let got: Vec<(&str, &str)> = properties
            .iter()
            .map(|p| (p.name(), p.aspect().description()))
            .collect();
        assert_eq!(got, EXPECTED);
        let names: Vec<&str> = got.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, IDENTIFIER_PROPERTIES);
    }

    #[test]
    fn each_property_has_the_51did_fields() {
        for property in default_fiftyone_property_metadata() {
            let name = property.name().to_owned();
            let core = property.core();
            assert_eq!(core.element_data_key, "fodid", "{name}");
            assert_eq!(core.category, "FODid", "{name}");
            assert_eq!(core.value_type, PropertyValueType::String, "{name}");
            assert!(core.available, "{name}");
            assert!(!core.delay_execution, "{name}");
            assert!(core.evidence_properties.is_empty(), "{name}");
            assert!(core.item_properties.is_empty(), "{name}");
            assert!(
                property.aspect().data_tiers_where_present().is_empty(),
                "{name}"
            );
            assert_eq!(property.component_name(), "FODid", "{name}");
            assert_eq!(property.component_id(), Some(255), "{name}");
            assert_eq!(property.display_order(), 255, "{name}");
            assert!(property.url().is_empty(), "{name}");
            assert!(!property.mandatory(), "{name}");
            assert!(!property.list(), "{name}");
            assert!(!property.obsolete(), "{name}");
            assert!(property.show(), "{name}");
            assert!(property.show_values(), "{name}");
            assert!(property.values().is_empty(), "{name}");
            let default = property.default_value().expect("a default value");
            assert_eq!(default.name, "N/A", "{name}");
            assert!(default.description.is_empty(), "{name}");
            assert!(default.url.is_empty(), "{name}");
        }
    }

    #[test]
    fn the_component_holds_the_six_properties() {
        let component = default_component_metadata();
        assert_eq!(component.name, "FODid");
        assert_eq!(component.component_id, 255);
        assert!(component.default_profile_id.is_none());
        assert_eq!(component.properties(), default_fiftyone_property_metadata());
        assert!(component.property("idhemlic").is_some());
    }

    #[test]
    fn the_aspect_and_core_views_are_the_51degrees_ones() {
        let fiftyone = default_fiftyone_property_metadata();
        let aspect = default_aspect_property_metadata();
        let core = default_property_metadata();
        assert_eq!(aspect.len(), 6);
        assert_eq!(core.len(), 6);
        for ((f, a), c) in fiftyone.iter().zip(&aspect).zip(&core) {
            assert_eq!(f.aspect(), a);
            assert_eq!(f.core(), c);
        }
    }
}
