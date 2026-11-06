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

//! The data model this crate re-exports is the one in `fiftyone-fodid-core`,
//! so data either crate names is one type and reads through either crate.

use fiftyone_fodid_cloud::FodIdData;
use fiftyone_pipeline_core::TypedKey;

#[test]
fn the_data_is_the_core_crate_type() {
    // Each binding compiles only because the two names are one type.
    let data: fiftyone_fodid_cloud::FodIdDataBase =
        fiftyone_fodid_core::FodIdDataBase::new().set("IdProbGlobal", "pg");
    assert_eq!(data.id_prob_global().value().unwrap(), "pg");
    let key: TypedKey<fiftyone_fodid_core::FodIdDataBase> = fiftyone_fodid_cloud::FODID_DATA_KEY;
    assert_eq!(key.name(), fiftyone_fodid_core::FODID_ELEMENT_DATA_KEY);
}

#[test]
fn the_engine_trait_is_the_core_crate_one() {
    // Each call compiles only because the cloud engine implements the core
    // crate's trait, which this crate re-exports.
    fn core_engine<E: fiftyone_fodid_core::FodIdEngine>() {}
    fn reexported_engine<E: fiftyone_fodid_cloud::FodIdEngine>() {}
    core_engine::<fiftyone_fodid_cloud::FodIdCloudEngine>();
    reexported_engine::<fiftyone_fodid_cloud::FodIdCloudEngine>();
}

#[test]
fn the_names_and_metadata_are_the_core_crate_ones() {
    assert_eq!(
        fiftyone_fodid_cloud::IDENTIFIER_PROPERTIES,
        fiftyone_fodid_core::IDENTIFIER_PROPERTIES
    );
    assert_eq!(
        fiftyone_fodid_cloud::default_property_metadata(),
        fiftyone_fodid_core::default_property_metadata()
    );
    assert_eq!(
        fiftyone_fodid_cloud::default_aspect_property_metadata(),
        fiftyone_fodid_core::default_aspect_property_metadata()
    );
    assert_eq!(
        fiftyone_fodid_cloud::default_fiftyone_property_metadata(),
        fiftyone_fodid_core::default_fiftyone_property_metadata()
    );
    assert_eq!(
        fiftyone_fodid_cloud::default_component_metadata(),
        fiftyone_fodid_core::default_component_metadata()
    );
    assert_eq!(
        fiftyone_fodid_cloud::CATEGORY_NAME,
        fiftyone_fodid_core::CATEGORY_NAME
    );
    assert_eq!(
        fiftyone_fodid_cloud::COMPONENT_NAME,
        fiftyone_fodid_core::COMPONENT_NAME
    );
}
