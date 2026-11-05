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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-pipeline-engines-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-pipeline-engines-lib.rs&utm_term=logo)
//!
//! # 51Degrees pipeline engines
//!
//! The engine layer that sits between the reflection-free
//! [`fiftyone_pipeline_core`] and the concrete device-detection and
//! IP-intelligence engines. It implements the
//! [Engines section](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/conceptual-overview.md#aspect-engine)
//! of the conceptual overview.
//!
//! An *aspect engine* is a [`fiftyone_pipeline_core::FlowElement`] that produces
//! data about one aspect of a request (the device, the IP location, and so on).
//! On top of a flow element this crate adds the machinery every such engine
//! needs:
//!
//! - [`AspectEngine`] and [`AspectEngineBase`]. The trait adds a data-source
//!   tier, aspect-aware property metadata, missing-property reasoning and a
//!   results cache. The base centralises the cached-process flow so an engine
//!   only writes the work that happens on a cache miss.
//! - [`AspectData`] and [`AspectDataBase`]. A specialisation of
//!   [`fiftyone_pipeline_core::ElementData`] that records which engine produced
//!   it and whether it came from a cache hit.
//! - [`AspectPropertyValue`]. The strongly-typed "value or no value" wrapper
//!   every engine accessor returns, realising the
//!   [null-values rule](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#null-values)
//!   in the type system.
//! - [`AspectPropertyMetaData`]. Property metadata with the aspect-only
//!   `description` and `data_tiers_where_present` fields.
//! - [`MissingPropertyService`]. Resolves *why* a property is absent (a data
//!   tier upgrade, a configuration exclusion, a resource-key gap) into a core
//!   [`fiftyone_pipeline_core::MissingPropertyReason`].
//! - [`OnPremiseAspectEngine`] and [`DataUpdateService`]. The on-premise trait
//!   adds on-demand refresh and data-file metadata, and the service keeps data
//!   files current through update-on-startup, remote polling with
//!   `If-Modified-Since`, a file-system watcher and a programmatic trigger.
//! - [`LazyLoadingConfiguration`] and [`AspectEngineBuilderOptions`]. Optional
//!   lazy loading and a builder base that captures the options every engine
//!   builder shares.
//!
//! ## How the cache wires in
//!
//! Element data is `Send` but not `Sync`, so the cache cannot hold
//! `dyn AspectData`. Each engine instead picks a concrete, `Send + Sync + Clone`
//! aspect data type and caches that, exactly as the
//! [`fiftyone_caching`] crate requires. [`AspectEngineBase`] is generic over
//! that type and holds a [`fiftyone_caching::DataKeyedCache`] keyed by the
//! engine's evidence. See [`AspectEngineBase`] for a complete worked example.

#![warn(missing_docs)]

mod aspect_data;
mod aspect_engine;
mod aspect_property_metadata;
mod aspect_property_value;
mod data_file;
#[cfg(feature = "data-update")]
mod data_update_service;
mod data_update_status;
mod engine_builder;
mod lazy_loading;
mod missing_property;
mod on_premise_aspect_engine;

pub use aspect_data::{AspectData, AspectDataBase};
pub use aspect_engine::{
    engine_missing_property_reason, property_pair, AspectEngine, AspectEngineBase,
    EnginePropertyPair,
};
pub use aspect_property_metadata::AspectPropertyMetaData;
pub use aspect_property_value::{AspectPropertyValue, DEFAULT_NO_VALUE_MESSAGE};
pub use data_file::{
    AspectEngineDataFile, DataFileConfiguration, DataFileConfigurationBuilder,
    DataUpdateUrlFormatter, FiftyOneUrlFormatter, DEFAULT_IDENTIFIER,
    DEFAULT_MAX_RANDOMISATION_SECONDS, DEFAULT_POLLING_INTERVAL_SECONDS, DISTRIBUTOR_URL,
};
#[cfg(feature = "data-update")]
pub use data_update_service::DataUpdateService;
pub use data_update_status::{AutoUpdateStatus, DataUpdateError, DataUpdateResult};
pub use engine_builder::{AspectEngineBuilderOptions, DEFAULT_CACHE_SIZE};
pub use lazy_loading::{LazyLoadingConfiguration, LAZY_LOADING_DEFAULT_TIMEOUT_MS};
pub use missing_property::{
    missing_property_reason, missing_property_reason_for_engines, EngineDeployment,
    EngineMissingPropertyContext, MissingPropertyResult, MissingPropertyService,
};
pub use on_premise_aspect_engine::OnPremiseAspectEngine;

// Re-export the core missing-property reason so downstream engine crates can
// match on it without importing the core crate directly.
pub use fiftyone_pipeline_core::MissingPropertyReason;
