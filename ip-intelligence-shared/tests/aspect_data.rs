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

//! The data model of the IP Intelligence specification, data-model.md:
//!
//! - line 10: "All Engines MUST populate data objects that implement Aspect
//!   Data as defined in the Pipeline specification."

use fiftyone_ip_intelligence_shared::{IpIntelligenceDataBase, IP_DATA_KEY_NAME};
use fiftyone_pipeline_core::ElementData;
use fiftyone_pipeline_engines::AspectData;

/// Takes the data an engine populates only as Aspect Data, so the test reads
/// it through that interface alone.
fn as_aspect_data(data: &dyn AspectData) -> &dyn AspectData {
    data
}

/// The data object both IP Intelligence engines populate is Aspect Data, and
/// reports through that interface the engine that populated it and whether
/// it came from a cache.
#[test]
fn engine_data_is_aspect_data() {
    let data = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
    let aspect = as_aspect_data(&data);
    assert_eq!(aspect.engine_keys(), [IP_DATA_KEY_NAME.to_string()]);
    assert!(!aspect.cache_hit());
    // Aspect Data is Element Data, so the values are read by name from it.
    let element: &dyn ElementData = &data;
    assert!(element.get("NoSuchProperty").is_err());
}
