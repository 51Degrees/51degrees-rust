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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-pipeline-engines-fiftyone-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-pipeline-engines-fiftyone-lib.rs&utm_term=logo)
//!
//! # 51Degrees-specific pipeline elements
//!
//! This crate provides the flow elements and metadata model that are specific
//! to 51Degrees, sitting on top of the generic [`fiftyone_pipeline_core`] and
//! [`fiftyone_pipeline_engines`] crates.
//!
//! ## The elements
//!
//! - [`SequenceElement`] establishes the session id and sequence number used to
//!   correlate the callbacks the client-side JavaScript makes to the server. It
//!   generates a GUID session id when none is supplied, increments the
//!   sequence on each request and writes both into the evidence. See the
//!   [sequence-element specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/sequence-element.md).
//! - [`SetHeadersElement`] scans every element's properties for the
//!   `SetHeader[Identifier][HeaderName]` naming convention and builds the set of
//!   HTTP response headers other elements want the server to send. See the
//!   [set-headers-element specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/set-headers-element.md).
//! - [`ShareUsageElement`] collects a filtered subset of evidence, batches it
//!   into GZip-compressed XML and POSTs it to a configurable 51Degrees endpoint
//!   on a background thread, with repeat-evidence deduplication. Transmission is
//!   gated behind the [`share-usage-transport`](#cargo-features) feature, which
//!   is off by default in this crate, so a default build of this crate sends
//!   nothing. The Device Detection and IP Intelligence facade crates turn it on
//!   through their default `share-usage` feature. A caller can instead take
//!   each batch itself, as the records and as the document, through a
//!   [`UsageSink`] given to [`ShareUsageElement::with_sink`], with or without
//!   the feature.
//!   See the
//!   [usage-sharing-element specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/usage-sharing-element.md).
//!
//! ## Cargo features
//!
//! Both features are **off by default**, so a default build of this crate pulls
//! in neither `reqwest` nor `notify`/`tokio` and compiles for
//! `wasm32-wasip1`.
//!
//! - **`share-usage-transport`** — compiles the built-in blocking `reqwest`
//!   transport that [`ShareUsageElement`] posts usage data through. This is the
//!   switch that lets the element actually send anything to 51Degrees. With the
//!   feature off (the default), the element is fully functional as a pipeline
//!   element (it still samples, filters and batches evidence) but its transport
//!   is a no-op: each batch is dropped instead of being transmitted, so **no
//!   network connection to 51Degrees is ever made**. There are therefore two
//!   independent opt-ins before any data leaves the process: the element must be
//!   added to the pipeline *and* this feature must be enabled. The facade
//!   crates' default `share-usage` feature does both. The feature is
//!   also what `reqwest` (which does not build for `wasm32-wasip1`) is gated
//!   behind, so a wasm consumer leaves it off and the element runs with its
//!   no-op transport. The destination endpoint is
//!   [`constants::SHARE_USAGE_DEFAULT_URL`] unless overridden with
//!   [`ShareUsageConfigBuilder::share_usage_url`].
//! - **`data-update`** — re-exports the `fiftyone-pipeline-engines` data-update
//!   service (reqwest + notify) for native and on-premise consumers that refresh
//!   their data files. It is unrelated to usage sharing and is also off by
//!   default.
//!
//! ## The metadata model
//!
//! [`ComponentMetaData`], [`FiftyOneAspectPropertyMetaData`] and
//! [`ValueMetaData`] mirror the richer metadata that 51Degrees data files
//! publish (components, presentation hints and the values a property can
//! return), following the
//! [data-model specification](https://github.com/51Degrees/specifications/blob/main/data-model-specification/README.md).
//!
//! ## Configuration
//!
//! [`configuration::add_builders`] adds the builders of these three elements
//! to a [`fiftyone_pipeline_core::configuration::BuilderRegistry`] under the
//! names .NET gives them, so a configuration file written for .NET makes the
//! same elements here. [`configuration::use_fiftyone_pipeline_builder`] makes
//! the registry's pipelines as .NET's `FiftyOnePipelineBuilder` does, adding
//! the Sequence element and usage sharing unless the configuration has them
//! or turns usage sharing off with `SetShareUsage`.
//!
//! ## A worked example
//!
//! ```
//! use std::sync::Arc;
//! use fiftyone_pipeline_core::Pipeline;
//! use fiftyone_pipeline_engines_fiftyone::SequenceElement;
//!
//! let pipeline = Pipeline::builder()
//!     .add_element(Arc::new(SequenceElement::new()))
//!     .build()
//!     .unwrap();
//!
//! // No session id supplied, so one is generated and the sequence starts at 1.
//! let mut data = pipeline.create_flow_data();
//! data.process().unwrap();
//!
//! let sequence = data.get(SequenceElement::KEY).unwrap();
//! assert!(sequence.session_id().is_some());
//! assert_eq!(sequence.sequence(), Some(1));
//! ```

#![warn(missing_docs)]

pub mod configuration;
pub mod constants;
mod evidence_filter;
mod host_address;
mod metadata;
mod sequence_element;
mod set_headers_element;
mod share_usage_element;
mod share_usage_tracker;
mod usage_record;
mod usage_sink;

pub use evidence_filter::{EvidenceKeyFilterShareUsage, EvidenceKeyFilterShareUsageTracker};
pub use metadata::{ComponentMetaData, FiftyOneAspectPropertyMetaData, ValueMetaData};
pub use sequence_element::{SequenceData, SequenceElement};
pub use set_headers_element::{
    SetHeadersData, SetHeadersElement, RESPONSE_HEADER_DICTIONARY_PROPERTY,
};
pub use share_usage_element::{
    ShareUsageConfig, ShareUsageConfigBuilder, ShareUsageElement, MIN_ENTRIES_PER_MESSAGE_FLOOR,
};
pub use share_usage_tracker::ShareUsageTracker;
pub use usage_record::{UsageRecord, UsageWriteError, UsageWriter};
pub use usage_sink::{UsageBatch, UsageSink};
