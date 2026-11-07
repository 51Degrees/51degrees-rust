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

//! The builder for the agent signature element.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use fiftyone_pipeline_core::{Error, Result};

use crate::constants::{
    DEFAULT_ALLOW_INLINE_DIRECTORY, DEFAULT_ALLOW_LEGACY_SIGNATURE_AGENT, DEFAULT_CACHE_LIFETIME,
    DEFAULT_CACHE_SIZE, DEFAULT_CLOCK_SKEW, DEFAULT_FETCH_TIMEOUT, DEFAULT_MAX_LIFETIME,
    DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_NEGATIVE_CACHE_LIFETIME, DEFAULT_TRUST_FORWARDED_EVIDENCE,
    DEFAULT_WAIT_BUDGET, MAXIMUM_PERIOD,
};
use crate::element::AgentSignatureElement;
use crate::fetcher::{is_https_without_user_info, Fetcher};
use crate::http::HttpClient;
use crate::Clock;

/// What a build with no client and no built-in one says, for a caller of
/// the builder in code.
const NO_CLIENT: &str = "The agent signature element needs an HTTP client. Turn on the \
                         reqwest-client feature or supply one with set_http_client.";

/// The settings an element runs with, once the builder has checked them.
pub(crate) struct Settings {
    pub(crate) clock: Clock,
    pub(crate) cache_size: usize,
    pub(crate) cache_lifetime: Duration,
    pub(crate) negative_cache_lifetime: Duration,
    pub(crate) wait_budget: Duration,
    pub(crate) fetch_timeout: Duration,
    pub(crate) clock_skew: Duration,
    pub(crate) max_lifetime: Duration,
    pub(crate) allow_legacy_signature_agent: bool,
    pub(crate) allow_inline_directory: bool,
    pub(crate) trust_forwarded_evidence: bool,
    pub(crate) registries: Vec<String>,
    pub(crate) reachability_check_url: Option<String>,
}

/// Builds an [`AgentSignatureElement`].
///
/// Every setting has the default the specification gives, so
/// `AgentSignatureElement::builder().build()` makes a working element where
/// the `reqwest-client` feature is on. Without the feature, supply a client
/// with [`AgentSignatureElementBuilder::set_http_client`].
///
/// Every period is checked when the element is built, and one longer than a
/// year is refused there, so a bad setting never reaches the request path.
pub struct AgentSignatureElementBuilder {
    http_client: Option<Arc<dyn HttpClient>>,
    /// What the build says when it has no client and no built-in one.
    no_client_message: &'static str,
    cache_size: usize,
    cache_lifetime: Duration,
    negative_cache_lifetime: Duration,
    wait_budget: Duration,
    fetch_timeout: Duration,
    clock_skew: Duration,
    max_lifetime: Duration,
    allow_legacy_signature_agent: bool,
    allow_inline_directory: bool,
    trust_forwarded_evidence: bool,
    max_response_bytes: usize,
    registries: Vec<String>,
    reachability_check_url: Option<String>,
    clock: Clock,
}

impl Default for AgentSignatureElementBuilder {
    fn default() -> Self {
        AgentSignatureElementBuilder {
            http_client: None,
            no_client_message: NO_CLIENT,
            cache_size: DEFAULT_CACHE_SIZE,
            cache_lifetime: DEFAULT_CACHE_LIFETIME,
            negative_cache_lifetime: DEFAULT_NEGATIVE_CACHE_LIFETIME,
            wait_budget: DEFAULT_WAIT_BUDGET,
            fetch_timeout: DEFAULT_FETCH_TIMEOUT,
            clock_skew: DEFAULT_CLOCK_SKEW,
            max_lifetime: DEFAULT_MAX_LIFETIME,
            allow_legacy_signature_agent: DEFAULT_ALLOW_LEGACY_SIGNATURE_AGENT,
            allow_inline_directory: DEFAULT_ALLOW_INLINE_DIRECTORY,
            trust_forwarded_evidence: DEFAULT_TRUST_FORWARDED_EVIDENCE,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            registries: Vec::new(),
            reachability_check_url: None,
            clock: Arc::new(Utc::now),
        }
    }
}

impl AgentSignatureElementBuilder {
    /// A builder with every setting at its default.
    pub fn new() -> Self {
        Self::default()
    }

    /// The client key directories, agent cards and registries are fetched
    /// with. The element uses it exactly as it is given, so whoever supplies
    /// one decides whether it follows redirects, and a document that arrives
    /// from any address other than the one asked for is refused whichever
    /// they decide. The client the element makes for itself follows none.
    pub fn set_http_client(mut self, client: Arc<dyn HttpClient>) -> Self {
        self.http_client = Some(client);
        self
    }

    /// Say `message` when the build has no client and no built-in one, for
    /// a caller that gives the client some other way than
    /// [`Self::set_http_client`].
    pub(crate) fn no_client_message(mut self, message: &'static str) -> Self {
        self.no_client_message = message;
        self
    }

    /// The number of key directories held in the cache. Default 1000. Must
    /// be at least one.
    pub fn set_cache_size(mut self, size: usize) -> Self {
        self.cache_size = size;
        self
    }

    /// How long a fetched key directory is reused for. Default 24 hours. A
    /// shorter `max-age` on the response is used where there is one.
    pub fn set_cache_lifetime(mut self, lifetime: Duration) -> Self {
        self.cache_lifetime = lifetime;
        self
    }

    /// How long a failed fetch is remembered for. Default 5 minutes.
    pub fn set_negative_cache_lifetime(mut self, lifetime: Duration) -> Self {
        self.negative_cache_lifetime = lifetime;
        self
    }

    /// How long a request waits for a key directory fetch before it reports
    /// Timeout. Default 350 milliseconds. The fetch carries on after the
    /// budget runs out, so a later request finds the result.
    pub fn set_wait_budget(mut self, wait_budget: Duration) -> Self {
        self.wait_budget = wait_budget;
        self
    }

    /// The time limit on a single fetch. Default 5 seconds.
    pub fn set_fetch_timeout(mut self, fetch_timeout: Duration) -> Self {
        self.fetch_timeout = fetch_timeout;
        self
    }

    /// The tolerance on the `created` and `expires` signature parameters,
    /// which allows for clocks that differ a little. Default 60 seconds.
    pub fn set_clock_skew(mut self, clock_skew: Duration) -> Self {
        self.clock_skew = clock_skew;
        self
    }

    /// The longest a signature may be valid for. A signature valid for
    /// longer reports Invalid with the reason Expired. Default zero, meaning
    /// no limit, because the protocol draft only recommends one.
    pub fn set_max_lifetime(mut self, max_lifetime: Duration) -> Self {
        self.max_lifetime = max_lifetime;
        self
    }

    /// Add the address of a registry of agent cards. More than one may be
    /// added. Each must be HTTPS with no user information, or building the
    /// element fails. The registries are read once for the life of the
    /// element, so a registry or card that cannot be fetched then is not
    /// tried again until the element is built again. Only the card
    /// properties are affected.
    pub fn set_registry(mut self, url: impl Into<String>) -> Self {
        let url = url.into();
        if !url.is_empty() {
            self.registries.push(url);
        }
        self
    }

    /// Whether evidence under the `query` prefix is trusted to describe the
    /// request that was signed. Default false. Only a service that knows it
    /// receives evidence forwarded by a caller's own pipeline may turn this
    /// on, because forwarded evidence and a typed query string cannot be
    /// told apart once they have arrived.
    pub fn set_trust_forwarded_evidence(mut self, trust: bool) -> Self {
        self.trust_forwarded_evidence = trust;
        self
    }

    /// An address to fetch once in the background when the element is
    /// built, so the log says whether this deployment can reach an agent's
    /// keys at all. Default none.
    pub fn set_reachability_check_url(mut self, url: impl Into<String>) -> Self {
        let url = url.into();
        self.reachability_check_url = (!url.is_empty()).then_some(url);
        self
    }

    /// Whether the bare quoted string form of the `Signature-Agent` header,
    /// which the earlier drafts used, is accepted. Default true.
    pub fn set_allow_legacy_signature_agent(mut self, allow: bool) -> Self {
        self.allow_legacy_signature_agent = allow;
        self
    }

    /// Whether a key set carried inline in a `data:` URI is accepted.
    /// Default false, because such a key set is chosen by whoever sent the
    /// request. Turn it on only where every caller is already trusted, such
    /// as a test harness.
    pub fn set_allow_inline_directory(mut self, allow: bool) -> Self {
        self.allow_inline_directory = allow;
        self
    }

    /// The most read from a key directory, an agent card or a registry
    /// before the fetch is abandoned. Default 262,144 bytes. Must be at
    /// least one.
    pub fn set_max_response_bytes(mut self, bytes: usize) -> Self {
        self.max_response_bytes = bytes;
        self
    }

    /// The clock the element reads the time from, so the tests can move it.
    #[cfg(test)]
    pub(crate) fn set_clock(
        mut self,
        clock: impl Fn() -> chrono::DateTime<Utc> + Send + Sync + 'static,
    ) -> Self {
        self.clock = Arc::new(clock);
        self
    }

    /// The client given and what the build says without one, so the tests
    /// can see what a configuration starts from.
    #[cfg(test)]
    pub(crate) fn client_and_message(&self) -> (Option<&Arc<dyn HttpClient>>, &'static str) {
        (self.http_client.as_ref(), self.no_client_message)
    }

    /// Build the element, refusing any setting out of range.
    pub fn build(self) -> Result<AgentSignatureElement> {
        let periods = [
            ("cache lifetime", self.cache_lifetime),
            ("negative cache lifetime", self.negative_cache_lifetime),
            ("wait budget", self.wait_budget),
            ("fetch timeout", self.fetch_timeout),
            ("clock skew", self.clock_skew),
            ("maximum lifetime", self.max_lifetime),
        ];
        for (name, period) in periods {
            if period > MAXIMUM_PERIOD {
                return Err(Error::configuration(format!(
                    "The {name} must not be longer than 365 days."
                )));
            }
        }
        if self.cache_size == 0 {
            return Err(Error::configuration(
                "The cache must be allowed to hold at least one key directory.",
            ));
        }
        if self.max_response_bytes == 0 {
            return Err(Error::configuration(
                "The response limit must allow at least one byte to be read.",
            ));
        }
        // A registry read over plain HTTP would let anyone on the network
        // path add a card naming a real agent's keys.
        if let Some(registry) = self
            .registries
            .iter()
            .find(|registry| !is_https_without_user_info(registry))
        {
            return Err(Error::configuration(format!(
                "The registry address '{registry}' must be HTTPS and carry no user \
                 information."
            )));
        }
        let client = match self.http_client {
            Some(client) => client,
            None => default_client(self.no_client_message)?,
        };
        let fetcher = Arc::new(Fetcher::new(
            client,
            Arc::clone(&self.clock),
            self.max_response_bytes,
        ));
        let settings = Settings {
            clock: self.clock,
            cache_size: self.cache_size,
            cache_lifetime: self.cache_lifetime,
            negative_cache_lifetime: self.negative_cache_lifetime,
            wait_budget: self.wait_budget,
            fetch_timeout: self.fetch_timeout,
            clock_skew: self.clock_skew,
            max_lifetime: self.max_lifetime,
            allow_legacy_signature_agent: self.allow_legacy_signature_agent,
            allow_inline_directory: self.allow_inline_directory,
            trust_forwarded_evidence: self.trust_forwarded_evidence,
            registries: self.registries,
            reachability_check_url: self.reachability_check_url,
        };
        Ok(AgentSignatureElement::new(settings, fetcher))
    }
}

/// The client the element makes for itself.
#[cfg(feature = "reqwest-client")]
fn default_client(_: &'static str) -> Result<Arc<dyn HttpClient>> {
    let client = crate::http::ReqwestClient::new().map_err(Error::configuration)?;
    Ok(Arc::new(client))
}

/// Without the built-in client a caller has to supply one, and `message`
/// says how.
#[cfg(not(feature = "reqwest-client"))]
fn default_client(message: &'static str) -> Result<Arc<dyn HttpClient>> {
    Err(Error::configuration(message))
}
