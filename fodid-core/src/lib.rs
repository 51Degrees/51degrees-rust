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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-fodid-core-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-fodid-core-lib.rs&utm_term=logo)
//!
//! The 51Did element data, property names and metadata that any engine
//! producing 51Dids is built on.
//!
//! # What this crate holds
//!
//! - [`FodIdData`], typed access to the six identifiers a 51Did result holds,
//!   each as the raw base64 envelope and as a parsed [`FodId`].
//! - [`FodIdDataBase`], the concrete data an engine stores under
//!   [`FODID_DATA_KEY`].
//! - The six property names ([`IDENTIFIER_PROPERTIES`]).
//! - Their metadata as 51Degrees describes them
//!   ([`default_fiftyone_property_metadata`]), in the category and component
//!   [`CATEGORY_NAME`] and [`COMPONENT_NAME`], with the component itself
//!   ([`default_component_metadata`]) and the aspect and core views
//!   ([`default_aspect_property_metadata`], [`default_property_metadata`]).
//!
//! # No cloud request engine
//!
//! This crate depends on the pipeline core, the aspect engine layer, the
//! 51Degrees metadata model and the [`fodid`] reader only. An engine
//! producing 51Dids can be built on it without taking the cloud request
//! engine and its HTTP stack.
//!
//! The cloud engine in `fiftyone-fodid-cloud` populates the same
//! [`FodIdDataBase`], and that crate re-exports every item here under its
//! own name, so code written against either crate reads one type.

#![warn(missing_docs)]

mod data;
mod metadata;

pub use data::{
    FodIdData, FodIdDataBase, FODID_DATA_KEY, FODID_ELEMENT_DATA_KEY, IDENTIFIER_PROPERTIES,
    ID_HEM_GLOBAL_PROPERTY, ID_HEM_LIC_PROPERTY, ID_PROB_GLOBAL_PROPERTY, ID_PROB_LIC_PROPERTY,
    ID_RAND_GLOBAL_PROPERTY, ID_RAND_LIC_PROPERTY,
};
pub use metadata::{
    default_aspect_property_metadata, default_component_metadata,
    default_fiftyone_property_metadata, default_property_metadata, CATEGORY_NAME, COMPONENT_NAME,
};

// Re-export the parsed-identifier type so a consumer can work with the decoded
// envelope without adding a direct dependency on the `fodid` reader crate.
pub use fodid::FodId;

// Re-export the aspect-value wrapper used across this crate's public surface,
// for the same reason.
pub use fiftyone_pipeline_engines::AspectPropertyValue;
