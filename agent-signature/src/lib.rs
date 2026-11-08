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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-agent-signature-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fiftyone-agent-signature-lib.rs&utm_term=logo)
//!
//! # Agent signatures
//!
//! [`AgentSignatureElement`] reads the three HTTP request headers an
//! automated agent sends when it signs its request under the IETF Web Bot
//! Auth protocol, checks the signature against the public key the agent
//! publishes, and reports what the signature proves. An agent here is a
//! program that fetches pages on someone's behalf, such as a search crawler
//! or a tool that reads pages for a person.
//!
//! It implements the
//! [Agent Signature Element specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/agent-signature-element.md),
//! which builds on
//! [RFC 9421, HTTP Message Signatures](https://www.rfc-editor.org/rfc/rfc9421.html),
//! and gives the answers the .NET package `FiftyOne.Pipeline.AgentSignature`
//! gives.
//!
//! ## What it reports
//!
//! The element data, under the key `agent-signature`, holds twelve
//! properties. `AgentSignature` is the status, one of Absent, Invalid,
//! Unverified, Timeout and Verified, and `AgentSignatureReason` says why.
//! Both always have a value, and the [`constants`] module holds every status
//! and reason code to compare against. The other ten carry the details the
//! signature, the key directory and the agent card give, and each says why
//! where it has no value.
//!
//! Almost no traffic is signed today, so a request with no signature reports
//! Absent, which is never evidence against the request. The element never
//! fails a request, whatever the headers hold.
//!
//! ## The evidence
//!
//! The element accepts eight evidence keys and no others, being
//! `header.signature`, `header.signature-input`, `header.signature-agent`,
//! `header.host`, `header.protocol`, `server.request-method`,
//! `server.request-path` and `server.request-query`. A signature covering
//! `@method`, `@path`, `@query` or `@target-uri` needs the last three, exactly
//! as the request line carried them. Where they are missing, such a signature
//! reports Unverified with the reason ComponentUnavailable. A signature
//! covering `@authority` and `signature-agent`, which is what the protocol
//! draft's own example does, needs only the headers.
//!
//! A signature may cover any other header. Such a header is read from the
//! evidence where the flow data holds it, and otherwise from the web request
//! a web integration gives, read with
//! [`fiftyone_pipeline_core::Evidence::http_request`]. The element does not
//! accept such a header, so it never adds one to a cache key, the ETag or the
//! Vary header. Where neither place holds a covered header, the signature
//! reports Unverified with the reason ComponentUnavailable.
//!
//! ## Fetching keys
//!
//! The first request from an agent starts one fetch of the agent's key
//! directory on a thread of its own, and waits for at most the wait budget,
//! 350 milliseconds by default. Where the fetch has not finished by then the
//! request reports Timeout and the fetch carries on, so a later request from
//! that agent finds the keys. The directories are held in a bounded cache.
//!
//! The built-in client is compiled with the `reqwest-client` feature and
//! follows no redirects. Without the feature, supply an [`HttpClient`] with
//! [`AgentSignatureElementBuilder::set_http_client`].
//!
//! ## Configuration
//!
//! [`configuration::add_builders`] adds the element's builder to a
//! [`fiftyone_pipeline_core::configuration::BuilderRegistry`] under the name
//! .NET gives it, so a configuration file written for .NET makes the same
//! element here, with the same settings.
//!
//! ## Example
//!
//! ```
//! use std::sync::Arc;
//!
//! use fiftyone_agent_signature::{
//!     constants, AgentSignatureElement, HttpClient, HttpRequest, HttpResponse,
//! };
//! use fiftyone_pipeline_core::{Evidence, Pipeline};
//!
//! // A client that reaches nothing, which is enough for a request that
//! // carries no signature.
//! struct NoNetwork;
//!
//! impl HttpClient for NoNetwork {
//!     fn get(&self, _request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
//!         Err("no network here".to_owned())
//!     }
//! }
//!
//! let element = AgentSignatureElement::builder()
//!     .set_http_client(Arc::new(NoNetwork))
//!     .build()
//!     .unwrap();
//! let pipeline = Pipeline::builder()
//!     .add_element(Arc::new(element))
//!     .build()
//!     .unwrap();
//!
//! let mut data = pipeline
//!     .create_flow_data_with(Evidence::builder().add("header.host", "example.com").build());
//! data.process().unwrap();
//!
//! let result = data.get(AgentSignatureElement::KEY).unwrap();
//! assert_eq!(
//!     result.agent_signature().value().map(String::as_str),
//!     Ok(constants::STATUS_ABSENT)
//! );
//! ```

#![warn(missing_docs)]

mod builder;
mod cache;
mod codec;
pub mod configuration;
pub mod constants;
mod data;
mod element;
mod fetcher;
mod http;
mod keys;
mod messages;
mod pending;
mod signature_agent;
mod signature_base;
mod signature_input;
mod structured_field;
mod verify;

use std::sync::Arc;

use chrono::{DateTime, Utc};

pub use builder::AgentSignatureElementBuilder;
pub use data::AgentSignatureData;
pub use element::AgentSignatureElement;
#[cfg(feature = "reqwest-client")]
pub use http::ReqwestClient;
pub use http::{HttpClient, HttpRequest, HttpResponse};

/// The clock the element reads the time from.
pub(crate) type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

#[cfg(test)]
mod tests;
