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

//! [![51Degrees](https://51degrees.com/img/logo.png?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fodid-client-lib.rs&utm_term=logo "Data rewards the curious")](https://51degrees.com/?utm_source=docs.rs&utm_medium=docs&utm_campaign=51degrees-rust&utm_content=fodid-client-lib.rs&utm_term=logo)
//!
//! # 51Degrees identifier (51Did) client
//!
//! The server side of the 51Did two-step verification against the
//! 51Degrees cloud, and the Rust port of the client the .NET, Java, Node,
//! Python and PHP packages already carry. It fetches and caches the
//! published signing keys, verifies a 51Did signature offline against the
//! key in force when the identifier was created, verifies a signature
//! through the cloud, and redeems the sealed creator context result a
//! browser relays.
//!
//! Reading a 51Did is the [`fodid`] crate's job, and this crate builds on
//! it. Creating one is not part of either, because a 51Did is created from
//! the browser through the cloud `json` endpoint, since the identifier
//! describes the browser's own connection.
//!
//! ## The two steps
//!
//! A 51Did carries a creator context, being a record of the connection it
//! was created on. Checking that the identifier is being presented from
//! that same connection takes two steps, and the split exists so that the
//! account's licence key never reaches the browser.
//!
//! 1. **The browser verifies.** The page calls the cloud's `verify-context`
//!    (or `verify-full`) endpoint from the browser, so the cloud sees the
//!    browser's own connection and compares it with the context inside the
//!    identifier. The cloud answers with a sealed result, which the browser
//!    cannot read or alter, and the page relays that result to its own
//!    server.
//! 2. **The server redeems.** The server calls [`DidClient::redeem`] with
//!    the identifier it knows independently, the sealed result the browser
//!    relayed, and the licence key only the server holds. The cloud opens
//!    the seal, confirms the result is for that identifier, is fresh and
//!    has not been redeemed before, and answers with a [`RedeemResult`]
//!    carrying the [`ContextOutcome`] and, for a mismatch, the
//!    [`FactorOutcome`] of each factor.
//!
//! One rule matters more than the rest, and every 51Did package applies it.
//! A factor of `misconfigured` or `notrecorded` is read on its own, as
//! [`FactorOutcome::Misconfigured`] or [`FactorOutcome::NotRecorded`], and
//! never falls through to a mismatch, because neither says the connection
//! differs. The first says the checking service could not determine the
//! factor and the second says the creating service recorded no value for it,
//! so reading either as a mismatch would report a replay indicator for
//! something the identifier says nothing about.
//!
//! ## Signature checks without the cloud
//!
//! The cloud publishes the signing keys whose periods have started, each in
//! force from its start until its end, which is the next key's start.
//! [`DidClient::verify_signature`] holds the keys it has fetched in a
//! per-instance cache and checks an identifier's signature against the key
//! in force at its creation time without a cloud call. It adds to the keys
//! it holds by fetching those from the newest one onwards for an identifier
//! dated at or near the end of that key, at most once a minute, and by
//! fetching the whole list once a day. A key may be replaced before its end,
//! for example if it is compromised, so a signature that fails under every
//! key held is checked once more after a fetch within the same limit.
//! [`covers`] and [`merge_keys`] apply the same rule to a list a caller holds
//! itself, and [`DidClient::fetch_keys_from`] fetches for such a list.
//!
//! [`DidClient::verify_signature_detailed`] says why a check did not pass,
//! as a [`SignatureCheck`], and only [`SignatureCheck::Invalid`] means the
//! identifier should be distrusted.
//!
//! ## Awaiting the client
//!
//! Every method that may reach the network is `async` and is awaited. The
//! crate carries no async runtime of its own, so the futures run on
//! whatever runtime the host has, and they are not required to be `Send`,
//! so a single-threaded host such as a `wasm32-wasip1` edge runtime can
//! await them. Concurrent callers that each find the key cache needs
//! fetching share one fetch rather than each making their own.
//!
//! ## Transport
//!
//! Every request goes through the [`DidHttpClient`] trait, whose one
//! method returns a [`LocalBoxFuture`]. The crate builds without any
//! network stack by default, so it compiles for `wasm32-wasip1` and a host
//! such as an edge runtime supplies its own transport through
//! [`DidClientBuilder::http_client`]. The `reqwest-client` feature turns on
//! the built-in `ReqwestClient`, an asynchronous `reqwest` client with
//! rustls that runs on a tokio runtime, which the builder uses when no
//! transport is given.
//!
//! The licence key never travels in a URL, because a URL is written to
//! access logs. The redeem call sends it in the form body, and the signing
//! key fetch sends it in the [`LICENCE_KEY_HEADER`] header instead of
//! putting the resource key in the route, which lets a server fetch the
//! keys when its resource key is restricted to named web domains. A
//! transport therefore sends every header a request carries.
//!
//! ## Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use fodid::FodId;
//! use fodid_client::{ContextOutcome, DidClient, DidHttpClient};
//!
//! # async fn run(
//! #     transport: Arc<dyn DidHttpClient>,
//! #     encoded_51did: &str,
//! #     sealed_result: &str,
//! # ) -> Result<(), Box<dyn std::error::Error>> {
//! // One client for the process. The licence key stays on the server.
//! let client = DidClient::builder("your-resource-key")
//!     .licence_key("your-licence-key")
//!     .http_client(transport)
//!     .build()?;
//!
//! // The identifier the server knows independently, for example from a
//! // cookie it set when the identifier was created.
//! let fod_id = FodId::from_base64(encoded_51did)?;
//!
//! // Step one happened in the browser. Step two is the redemption.
//! let outcome = client.redeem(&fod_id, sealed_result, None).await?;
//! match outcome.context() {
//!     ContextOutcome::Verified => { /* same connection as at creation */ }
//!     ContextOutcome::Mismatch => {
//!         // outcome.factors() names the factors that differ.
//!     }
//!     ContextOutcome::Misconfigured => {
//!         // The checking service, not the identifier, is at fault.
//!     }
//!     other => { /* see ContextOutcome for the rest */ let _ = other; }
//! }
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]

mod client;
mod error;
mod http;
mod key;
mod outcome;
pub mod redact;
mod redeem;

#[allow(deprecated)]
pub use client::ENDPOINT_ENVIRONMENT_VARIABLE;
pub use client::{
    DidClient, DidClientBuilder, CLOUD_ENDPOINT_VARIABLE, DEFAULT_ENDPOINT, KEY_CACHE_LIFETIME,
    LICENCE_KEY_HEADER, MAXIMUM_ENCODED_LENGTH, USER_AGENT,
};
pub use error::{Error, Result};
pub use http::{DidHttpClient, DidHttpRequest, DidHttpResponse, HttpMethod, LocalBoxFuture};
pub use key::{
    candidates_for_date, covers, in_force_at, merge_keys, parse_keys, DidPublicKey,
    BOUNDARY_TOLERANCE_MINUTES,
};
pub use outcome::{ContextOutcome, Factor, FactorOutcome, SignatureCheck, SignatureOutcome};
pub use redeem::RedeemResult;

#[cfg(feature = "reqwest-client")]
pub use http::ReqwestClient;

// The 51Did reader this client builds on, re-exported so a caller can name
// `FodId` without adding the dependency itself.
pub use fodid;

/// The examples in the README are compiled as documentation tests, so the
/// documented way to use this crate cannot quietly stop working.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeExamples;
