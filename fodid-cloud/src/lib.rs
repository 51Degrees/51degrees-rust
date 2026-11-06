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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-fodid-cloud-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-fodid-cloud-lib.rs&utm_term=logo)
//!
//! Cloud engine that unpacks the 51Degrees identifier (51Did / FODid) block from
//! the cloud JSON response into typed data.
//!
//! # What this crate does
//!
//! A 51Degrees cloud response carries each requested product in its own
//! top-level JSON member: `device` for device detection, `ip` for IP
//! intelligence, and `fodid` for the 51Degrees identifier. This crate provides
//! the pipeline element that owns the `fodid` member.
//!
//! [`FodIdCloudEngine`] sits after a
//! [`CloudRequestEngine`](fiftyone_cloud_request_engine::CloudRequestEngine) in a
//! pipeline. The request engine makes the single HTTP call and stores the raw
//! JSON; this engine slices out the `fodid` member and maps it into a
//! [`FodIdDataBase`], stored under [`FODID_DATA_KEY`].
//!
//! # The identifier values
//!
//! The block holds two probabilistic identifiers, each a base64-encoded
//! [OWID](https://github.com/SWAN-community/owid) envelope:
//!
//! - [`IdProbGlobal`](ID_PROB_GLOBAL_PROPERTY), unique across all callers
//!   observing the same device and network, and
//! - [`IdProbLic`](ID_PROB_LIC_PROPERTY), unique only across the caller's own
//!   license key.
//!
//! Both are exposed in two forms: the raw base64 string
//! ([`FodIdData::id_prob_global`]), and the parsed
//! [`FodId`] ([`FodIdData::id_prob_global_fod_id`]) which unpacks
//! the envelope's payload (the usage, the identifier type, the license id and
//! the match key) and the OWID
//! domain, date and signature. The parsing reuses the [`fodid`] reader, so the
//! envelope handling is identical whether a 51Did arrives from the cloud or is
//! read from a stored cookie.
//!
//! The data model is in [`fiftyone_fodid_core`], which does not depend on the
//! cloud request engine. This crate re-exports every item of it under the same
//! name, so [`FodIdDataBase`] here and there is one type.
//!
//! # Cloud only
//!
//! A 51Did is issued by the cloud, which alone holds the signing key, so there
//! is no on-premise engine that produces the same data (unlike device detection
//! and IP intelligence, which have interchangeable cloud and on-premise
//! engines). [`FodIdCloudEngine`] is the only producer of [`FODID_DATA_KEY`].

#![warn(missing_docs)]

mod dto;
mod engine;

pub use engine::{FodIdCloudEngine, FodIdCloudEngineBuilder};
pub use fiftyone_fodid_core::{
    default_aspect_property_metadata, default_component_metadata,
    default_fiftyone_property_metadata, default_property_metadata, FodIdData, FodIdDataBase,
    CATEGORY_NAME, COMPONENT_NAME, FODID_DATA_KEY, FODID_ELEMENT_DATA_KEY, IDENTIFIER_PROPERTIES,
    ID_HEM_GLOBAL_PROPERTY, ID_HEM_LIC_PROPERTY, ID_PROB_GLOBAL_PROPERTY, ID_PROB_LIC_PROPERTY,
    ID_RAND_GLOBAL_PROPERTY, ID_RAND_LIC_PROPERTY,
};

// Re-export the parsed-identifier type so a consumer can work with the decoded
// envelope without adding a direct dependency on the `fodid` reader crate.
pub use fodid::FodId;

// Re-export the aspect-value wrapper used across this engine's public surface,
// for the same reason.
pub use fiftyone_pipeline_engines::AspectPropertyValue;
