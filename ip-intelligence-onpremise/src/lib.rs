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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-ip-intelligence-onpremise-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-ip-intelligence-onpremise-lib.rs&utm_term=logo)
//!
//! The on-premise IP-intelligence aspect engine.
//!
//! This crate wraps the safe native IP Intelligence resource manager from
//! [`fiftyone_native::ipi`] in a pipeline `FlowElement` so a `.ipi` data file
//! can be queried as part of a 51Degrees pipeline.
//!
//! # What it produces
//!
//! The engine populates an [`IpIntelligenceDataBase`] (from the shared
//! [`fiftyone_ip_intelligence_shared`] crate) under the shared
//! [`IP_DATA_KEY`], the same element-data type and key the cloud engine
//! populates. A consuming application can therefore swap the on-premise engine
//! for the cloud one (or run both) and read the result the same way, satisfying
//! the engine-compatibility requirement from the workspace coordination notes.
//!
//! Because IP Intelligence values are probabilistic, each property is read
//! through the native *weighted* getter
//! [`fiftyone_native::ipi::Results::values_weighted`], which returns every
//! candidate value paired with its raw `u16` weighting (highest first). The
//! engine parses each candidate into the property's value type (string, double
//! or integer) and stores the resulting `Vec<WeightedValue<T>>` through the
//! matching `set_weighted_*` builder on the data, so the typed accessors on
//! [`IpIntelligenceData`] return the full weighted distribution.
//!
//! # On-premise specifics
//!
//! - **Values read on request, no results cache.** Each result keeps its
//!   lookup's native results and reads a property from them the first time it
//!   is requested, so a caller that reads one property does not pay to read the
//!   rest. The native results, and the data set they came from, stay alive
//!   until the result is dropped, so this engine never attaches a results
//!   cache. It still embeds an `AspectEngineBase` with no cache so its
//!   `process` flow matches the rest of the engine layer.
//! - **Evidence keys from the data file.** The engine accepts the client IP
//!   keys its data file defines, for example `query.client-ip-51d` and
//!   `server.client-ip`, and reads them again when it refreshes
//!   ([`IpIntelligenceOnPremiseEngine::evidence_keys`]). Every query value is
//!   tried before any server value, in the data file's order, and the first
//!   that reads as an IP address is looked up through the native evidence
//!   entry point.
//! - **Refreshable.** `OnPremiseAspectEngine::refresh`, and
//!   `refresh_from_memory` with the whole of a data file, reload the data set
//!   in the engine's [`fiftyone_native::ipi::Manager`] in place, through the C
//!   library's reload, while concurrent `process` calls keep reading the data
//!   set their results came from, as the data-updates specification
//!   requires. What a lookup reads from the data set lives behind an
//!   [`arc_swap::ArcSwap`], replaced after each reload. Each load records the
//!   publish date and the next update date the data file header holds, for
//!   the data update service.
//! - **From a configuration file.** [`configuration`] makes the engine from a
//!   configuration file written for .NET's `IpiOnPremiseEngineBuilder`.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use fiftyone_pipeline_core::{Evidence, Pipeline};
//! use fiftyone_pipeline_engines::AspectData;
//! use fiftyone_ip_intelligence_shared::{IpIntelligenceData, IP_DATA_KEY};
//! use fiftyone_ip_intelligence_onpremise::IpIntelligenceOnPremiseEngine;
//! use fiftyone_native::PerformanceProfile;
//!
//! # fn main() -> fiftyone_pipeline_core::Result<()> {
//! // LowMemory reads the file from disk, so even the Enterprise file, which is
//! // over 6 GB, needs little memory. HighPerformance holds the whole file in
//! // memory for faster lookups, where there is memory for it.
//! let engine = IpIntelligenceOnPremiseEngine::builder("51Degrees-IPIV4AsnIpiV41.ipi")
//!     .performance_profile(PerformanceProfile::LowMemory)
//!     // Off because the free ASN file has no licence key for updates.
//!     .auto_update(false)
//!     .build()?;
//!
//! let pipeline = Pipeline::builder().add_element(Arc::new(engine)).build()?;
//! let mut data = pipeline.create_flow_data_with(
//!     Evidence::builder().add("server.client-ip", "185.28.167.77").build(),
//! );
//! data.process()?;
//!
//! if let Some(ip) = data.get(IP_DATA_KEY) {
//!     // RegisteredCountry is a plain string property, read through its accessor.
//!     if let Ok(country) = ip.registered_country().value() {
//!         println!("registered country = {country}");
//!     }
//! }
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

mod builder;
pub mod configuration;
mod engine;
mod operational_copy;
mod retained;

pub use builder::{IpIntelligenceOnPremiseEngineBuilder, DATA_DOWNLOAD_TYPE};
#[allow(deprecated)]
pub use engine::IP_EVIDENCE_KEYS;
pub use engine::{IpIntelligenceOnPremiseEngine, DEFAULT_DATA_SOURCE_TIER};

// Re-export the shared data type and key so a downstream application can depend
// only on this crate and still read the result without importing the shared
// crate directly.
pub use fiftyone_ip_intelligence_shared::{
    IpIntelligenceData, IpIntelligenceDataBase, IP_DATA_KEY, IP_DATA_KEY_NAME,
};
