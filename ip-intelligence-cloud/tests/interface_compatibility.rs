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

//! The IP Intelligence cloud engine's element data is the on-premise
//! engine's.
//!
//! cloud-aspect-engine.md line 24: "The key requirement is that the Element
//! Data for a Cloud Aspect Engine MUST be interface compatible with the
//! Element Data for the equivalent on-premise Engine."
//!
//! The on-premise engine stores an `IpIntelligenceDataBase` under the shared
//! crate's `IP_DATA_KEY`. The checks below are made by the compiler: the key
//! the cloud engine exports is that same key, and its data type reads through
//! the `IpIntelligenceData` trait that code written against the on-premise
//! engine uses.

use fiftyone_ip_intelligence_cloud::{IpIntelligenceData, IP_DATA_KEY};
use fiftyone_ip_intelligence_shared::IpIntelligenceDataBase;
use fiftyone_pipeline_core::TypedKey;

/// Accepts only element data that offers the IP Intelligence interface.
fn reads_through_the_interface<D: IpIntelligenceData + ?Sized>(_data: &D) {}

#[test]
fn the_cloud_key_is_the_on_premise_key_and_its_data_offers_the_interface() {
    let cloud: TypedKey<IpIntelligenceDataBase> = IP_DATA_KEY;
    let on_premise: TypedKey<IpIntelligenceDataBase> = fiftyone_ip_intelligence_shared::IP_DATA_KEY;
    assert_eq!(cloud.name(), on_premise.name());
    reads_through_the_interface(&IpIntelligenceDataBase::new(cloud.name()));
}
