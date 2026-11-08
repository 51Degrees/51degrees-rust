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

//! The agent signature element.

use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use fiftyone_pipeline_core::constants::{
    EVIDENCE_HTTP_HEADER_PREFIX, EVIDENCE_PROTOCOL_KEY, EVIDENCE_QUERY_PREFIX,
    EVIDENCE_REQUEST_METHOD_KEY, EVIDENCE_REQUEST_PATH_KEY, EVIDENCE_REQUEST_QUERY_KEY,
};
use fiftyone_pipeline_core::{
    Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    PropertyMetaData, PropertyValueType, Result, TypedKey,
};
use fiftyone_pipeline_engines::AspectPropertyValue;

use crate::builder::{AgentSignatureElementBuilder, Settings};
use crate::cache::{CacheTimes, DirectoryCache, Lookup};
use crate::constants::*;
use crate::data::{AgentSignatureData, AgentSignatureValues, PROPERTY_NAMES};
use crate::fetcher::{log_registry_failure, Fetcher};
use crate::keys::{AgentCard, EARLIEST_UNIX_SECONDS, LATEST_UNIX_SECONDS};
use crate::messages::*;
use crate::pending::{lock, Pending};
use crate::signature_agent::{parse_signature_agents, SignatureAgentEntry};
use crate::signature_base::{
    ascii_bytes, build_authority, build_signature_base, ComponentResolver,
};
use crate::signature_input::SignatureCandidate;
use crate::structured_field::{parse_dictionary, serialize_member, Item};
use crate::verify::{resolve_algorithm, verify};

/// The number of signatures carrying the Web Bot Auth tag one request has
/// checked. The protocol draft's reverse proxy case sends two, so three
/// leaves room for a longer chain whilst holding the worst case, being one
/// directory wait for each signature, to a few wait budgets.
const MAXIMUM_SIGNATURES_CHECKED: usize = 3;

/// The number of key ids and directories remembered for the warnings written
/// once each, after which no more are remembered and no more are written.
const MAXIMUM_LOGGED_KEYS: usize = 1000;

/// The precedence the element gives the evidence it accepts.
const EVIDENCE_ORDER: i32 = 100;

/// The nanoseconds in a second.
const NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;

/// The name the background threads carry.
const BACKGROUND_THREAD_NAME: &str = "fiftyone-agent-signature";

/// The agent cards from the configured registries, keyed by `jwks_uri`.
type CardsByKeyUrl = Arc<HashMap<String, AgentCard>>;

/// Where one request's signature arrived, and so where every other part of
/// the request is read from.
struct Source {
    prefix: &'static str,
    signature: &'static str,
    input: &'static str,
    agent: &'static str,
    method: &'static str,
    path: &'static str,
    query: &'static str,
}

/// A request that arrived here directly.
static HEADER_SOURCE: Source = Source {
    prefix: EVIDENCE_HTTP_HEADER_PREFIX,
    signature: EVIDENCE_SIGNATURE_KEY,
    input: EVIDENCE_SIGNATURE_INPUT_KEY,
    agent: EVIDENCE_SIGNATURE_AGENT_KEY,
    method: EVIDENCE_REQUEST_METHOD_KEY,
    path: EVIDENCE_REQUEST_PATH_KEY,
    query: EVIDENCE_REQUEST_QUERY_KEY,
};

/// A request forwarded by a caller's own pipeline, which takes the prefix off
/// every key it sends on.
static QUERY_SOURCE: Source = Source {
    prefix: EVIDENCE_QUERY_PREFIX,
    signature: "query.signature",
    input: "query.signature-input",
    agent: "query.signature-agent",
    method: "query.request-method",
    path: "query.request-path",
    query: "query.request-query",
};

/// Reads the three HTTP headers an automated agent sends when it signs its
/// request under the IETF Web Bot Auth protocol, checks the signature against
/// the public key the agent publishes, and reports what the signature proves.
///
/// A request with no signature reports the Absent status, which is the normal
/// case, because only a handful of agents sign today. An absent signature is
/// never evidence against a request. The element never fails a request,
/// whatever the three headers hold, and reports every failure through the
/// status and the reason.
///
/// The first request from an agent the element has not seen starts a fetch
/// of the agent's key directory and waits for it for at most the wait
/// budget. Where the fetch has not finished by then the request reports
/// Timeout, and the fetch carries on so that a later request finds the keys.
///
/// The registries of agent cards are read once for the life of the element.
/// A registry or card that cannot be fetched at that read is not tried again
/// until the element is built again. Only the card properties are affected.
///
/// Build one with [`AgentSignatureElement::builder`].
pub struct AgentSignatureElement {
    settings: Settings,
    filter: AgentSignatureEvidenceKeyFilter,
    properties: Vec<PropertyMetaData>,
    fetcher: Arc<Fetcher>,
    cache: DirectoryCache,
    absent: Arc<AgentSignatureValues>,
    cards: OnceLock<Arc<Pending<CardsByKeyUrl>>>,
    shared_secret_keys_logged: Mutex<HashSet<String>>,
    pending_directories_logged: Mutex<HashSet<String>>,
    disposing: Arc<AtomicBool>,
}

impl AgentSignatureElement {
    /// The typed key the element's data is stored under.
    pub const KEY: TypedKey<AgentSignatureData> = TypedKey::new(DEFAULT_ELEMENT_DATA_KEY);

    /// Start building an element.
    pub fn builder() -> AgentSignatureElementBuilder {
        AgentSignatureElementBuilder::new()
    }

    /// The number of key directories the cache holds.
    pub fn cache_size(&self) -> usize {
        self.settings.cache_size
    }

    /// How long a fetched key directory is reused for.
    pub fn cache_lifetime(&self) -> Duration {
        self.settings.cache_lifetime
    }

    /// How long a failed fetch is remembered for.
    pub fn negative_cache_lifetime(&self) -> Duration {
        self.settings.negative_cache_lifetime
    }

    /// How long a request waits for a key directory fetch.
    pub fn wait_budget(&self) -> Duration {
        self.settings.wait_budget
    }

    /// The time limit on a single fetch.
    pub fn fetch_timeout(&self) -> Duration {
        self.settings.fetch_timeout
    }

    /// The tolerance on the `created` and `expires` signature parameters.
    pub fn clock_skew(&self) -> Duration {
        self.settings.clock_skew
    }

    /// The longest a signature may be valid for, where zero is no limit.
    pub fn max_lifetime(&self) -> Duration {
        self.settings.max_lifetime
    }

    /// The addresses of the registries of agent cards.
    pub fn registries(&self) -> &[String] {
        &self.settings.registries
    }

    /// Whether evidence under the `query` prefix is trusted to describe the
    /// request that was signed.
    pub fn trust_forwarded_evidence(&self) -> bool {
        self.settings.trust_forwarded_evidence
    }

    /// The address fetched once when the element was built, if any.
    pub fn reachability_check_url(&self) -> Option<&str> {
        self.settings.reachability_check_url.as_deref()
    }

    /// Whether the bare quoted string form of the `Signature-Agent` header
    /// is accepted.
    pub fn allow_legacy_signature_agent(&self) -> bool {
        self.settings.allow_legacy_signature_agent
    }

    /// Whether a key set carried inline in a `data:` URI is accepted.
    pub fn allow_inline_directory(&self) -> bool {
        self.settings.allow_inline_directory
    }

    /// The most read from a key directory, an agent card or a registry.
    pub fn max_response_bytes(&self) -> usize {
        self.fetcher.max_response_bytes()
    }

    pub(crate) fn new(settings: Settings, fetcher: Arc<Fetcher>) -> Self {
        let times = CacheTimes {
            lifetime: to_time_delta(settings.cache_lifetime),
            negative_lifetime: to_time_delta(settings.negative_cache_lifetime),
            wait_budget: settings.wait_budget,
            fetch_timeout: settings.fetch_timeout,
        };
        let cache = DirectoryCache::new(
            Arc::clone(&fetcher),
            Arc::clone(&settings.clock),
            settings.cache_size,
            times,
        );
        let properties = PROPERTY_NAMES
            .iter()
            .map(|name| {
                PropertyMetaData::new(*name, DEFAULT_ELEMENT_DATA_KEY, PropertyValueType::String)
            })
            .collect();
        let element = AgentSignatureElement {
            filter: AgentSignatureEvidenceKeyFilter::new(settings.trust_forwarded_evidence),
            properties,
            fetcher,
            cache,
            absent: Arc::new(absent_values()),
            cards: OnceLock::new(),
            shared_secret_keys_logged: Mutex::new(HashSet::new()),
            pending_directories_logged: Mutex::new(HashSet::new()),
            disposing: Arc::new(AtomicBool::new(false)),
            settings,
        };
        element.start_reachability_check();
        element
    }

    /// Where an address to check has been configured, fetch it once in the
    /// background and say in the log whether this deployment can reach an
    /// agent's keys at all. A deployment with no outbound access answers
    /// every signed request Unverified, which otherwise looks like agents
    /// behaving oddly. Nothing waits for the check and its outcome changes
    /// nothing, so a network that is down never stops the element being
    /// built.
    fn start_reachability_check(&self) {
        let Some(url) = self.settings.reachability_check_url.clone() else {
            return;
        };
        let fetcher = Arc::clone(&self.fetcher);
        let disposing = Arc::clone(&self.disposing);
        let fetch_timeout = self.settings.fetch_timeout;
        let spawned = thread::Builder::new()
            .name(BACKGROUND_THREAD_NAME.to_owned())
            .spawn(move || {
                let deadline = deadline_after(fetch_timeout);
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    fetcher.fetch(&url, AGENT_TYPE_DIRECTORY, deadline)
                }));
                let reason = match outcome {
                    Ok(entry) if entry.is_success() => {
                        log::info!(
                            "Agent signature verification reached the key directory at \
                             '{url}' at start up, so this deployment can fetch the keys \
                             agents publish."
                        );
                        return;
                    }
                    Ok(entry) => entry
                        .failure_reason
                        .clone()
                        .unwrap_or_else(|| "no reason was given".to_owned()),
                    Err(_) => "the check did not complete".to_owned(),
                };
                // An element dropped whilst the check ran is an ordinary
                // shutdown, not a deployment that cannot reach the keys.
                if !disposing.load(Ordering::SeqCst) {
                    log::error!(
                        "Agent signature verification could NOT reach the key directory at \
                         '{url}' at start up because {reason}. Unless outbound HTTPS is \
                         opened for this deployment, every signed request will report the \
                         Unverified status with the DirectoryUnavailable reason, because \
                         no agent's keys can be fetched. Signature checking is effectively \
                         switched off until this is fixed."
                    );
                }
            });
        if let Err(error) = spawned {
            log::error!("The agent signature reachability check could not be started: {error}");
        }
    }

    /// The values for a request. Nearly every request has no signature at
    /// all, so that case is answered before anything is parsed or allocated,
    /// from values shared by every such request.
    fn values_for(&self, evidence: &Evidence) -> Arc<AgentSignatureValues> {
        let source = self.find_source(evidence);
        let signature = evidence.get(source.signature);
        let input = evidence.get(source.input);
        if signature.is_none() && input.is_none() {
            return Arc::clone(&self.absent);
        }
        Arc::new(
            self.evaluate(evidence, source, signature, input)
                .into_values(),
        )
    }

    /// The source a request's signature arrived under. Evidence under the
    /// query prefix is used only where the caller trusts forwarded evidence,
    /// because a web integration turns query string parameters into evidence
    /// under that prefix, and a visitor could otherwise put a captured
    /// signature, a host and a path in the address bar and have those
    /// checked in place of the request that arrived.
    fn find_source(&self, evidence: &Evidence) -> &'static Source {
        if self.settings.trust_forwarded_evidence
            && (evidence.contains_key(QUERY_SOURCE.signature)
                || evidence.contains_key(QUERY_SOURCE.input))
        {
            &QUERY_SOURCE
        } else {
            &HEADER_SOURCE
        }
    }

    fn evaluate(
        &self,
        evidence: &Evidence,
        source: &Source,
        signature: Option<&str>,
        input: Option<&str>,
    ) -> Outcome {
        let mut outcome = Outcome::new(NO_VALUE_DETAIL_NOT_SENT);
        let candidates = match (signature, input) {
            (Some(signature), Some(input)) => parse_dictionary(input)
                .zip(parse_dictionary(signature))
                .and_then(|(input, signature)| SignatureCandidate::build_all(&input, &signature)),
            _ => None,
        };
        let Some(candidates) = candidates else {
            return outcome.fail(STATUS_INVALID, REASON_MALFORMED, NO_VALUE_DETAIL_MALFORMED);
        };
        let mut agents = None;
        if let Some(header) = evidence.get(source.agent) {
            let Some(parsed) =
                parse_signature_agents(header, self.settings.allow_legacy_signature_agent)
            else {
                return outcome.fail(STATUS_INVALID, REASON_MALFORMED, NO_VALUE_DETAIL_MALFORMED);
            };
            outcome.agent = parsed.first().map(|agent| agent.value.clone());
            agents = Some(parsed);
        }
        // Every signature carrying the Web Bot Auth tag is checked, because
        // the protocol draft has a verifier check each one and its reverse
        // proxy case sends two. The first that verifies answers the request,
        // and where none does the first tagged signature's outcome is
        // reported. The number checked is bounded, because each can cost a
        // directory wait and the header is written by the sender.
        let mut first_tagged = None;
        let tagged = candidates
            .iter()
            .filter(|candidate| candidate.tag.as_deref() == Some(TAG_WEB_BOT_AUTH))
            .take(MAXIMUM_SIGNATURES_CHECKED);
        for candidate in tagged {
            let mut attempt = Outcome::new(NO_VALUE_DETAIL_NOT_SENT);
            attempt.agent = outcome.agent.clone();
            let result =
                self.evaluate_candidate(evidence, source, candidate, agents.as_deref(), attempt);
            if result.status == STATUS_VERIFIED {
                return result;
            }
            if first_tagged.is_none() {
                first_tagged = Some(result);
            }
        }
        match first_tagged {
            Some(result) => result,
            None => outcome.fail(
                STATUS_INVALID,
                REASON_TAG_MISMATCH,
                NO_VALUE_DETAIL_WRONG_TAG,
            ),
        }
    }

    /// Take one signature through the checks, from its parameters to the
    /// check against the key the agent publishes.
    fn evaluate_candidate(
        &self,
        evidence: &Evidence,
        source: &Source,
        candidate: &SignatureCandidate,
        agents: Option<&[SignatureAgentEntry]>,
        mut outcome: Outcome,
    ) -> Outcome {
        outcome.key_id = candidate.key_id.clone();
        outcome.nonce = candidate.nonce.clone();
        outcome.algorithm = candidate.algorithm.clone();
        // A time outside the range the element can hold is not a time at
        // all. Carrying it forward as the nearest time that can be held
        // would make a created far in the future read as one long ago, which
        // would then pass the check below rather than fail it.
        if let Some(seconds) = candidate.created {
            match from_unix_seconds(seconds) {
                Some(time) => outcome.created = Some(time),
                None => {
                    return outcome.fail(
                        STATUS_INVALID,
                        REASON_MALFORMED,
                        NO_VALUE_DETAIL_MALFORMED,
                    )
                }
            }
        }
        if let Some(seconds) = candidate.expires {
            match from_unix_seconds(seconds) {
                Some(time) => outcome.expires = Some(time),
                None => {
                    return outcome.fail(
                        STATUS_INVALID,
                        REASON_MALFORMED,
                        NO_VALUE_DETAIL_MALFORMED,
                    )
                }
            }
        }
        let (Some(created), Some(expires), Some(key_id)) = (
            candidate.created,
            candidate.expires,
            candidate.key_id.as_deref().filter(|id| !id.is_empty()),
        ) else {
            return outcome.fail(
                STATUS_INVALID,
                REASON_MISSING_PARAMETER,
                NO_VALUE_DETAIL_NOT_SENT,
            );
        };
        if let Some(reason) = self.check_times(created, expires) {
            return outcome.fail(STATUS_INVALID, reason, NO_VALUE_DETAIL_NOT_SENT);
        }
        let Some(agent) = find_agent(candidate, agents) else {
            return outcome.fail(STATUS_UNVERIFIED, REASON_NO_AGENT, NO_VALUE_DETAIL_NO_AGENT);
        };
        outcome.agent = Some(agent.value.clone());
        // A key set carried in the header is chosen by whoever sent the
        // request, so a signature that checks out against it shows only that
        // the sender holds the matching private key.
        if agent.inline_directory.is_some() && !self.settings.allow_inline_directory {
            return outcome.fail(
                STATUS_UNVERIFIED,
                REASON_INLINE_DIRECTORY,
                NO_VALUE_DETAIL_INLINE_DIRECTORY,
            );
        }
        // A signature covering neither '@authority' nor '@target-uri' would
        // check out just as well against a request to any other site, so one
        // captured anywhere could be replayed here.
        if !covers_the_request(candidate) {
            return outcome.fail(
                STATUS_INVALID,
                REASON_UNBOUND_SIGNATURE,
                NO_VALUE_DETAIL_UNBOUND,
            );
        }
        let resolver = EvidenceResolver {
            evidence,
            source,
            filter: &self.filter,
        };
        let Some(base) = build_signature_base(
            &candidate.covered_components,
            &candidate.signature_params,
            &resolver,
        ) else {
            return outcome.fail(
                STATUS_UNVERIFIED,
                REASON_COMPONENT_UNAVAILABLE,
                NO_VALUE_DETAIL_NOT_SENT,
            );
        };
        let entry = match self.cache.lookup(agent) {
            Lookup::Pending => {
                self.log_directory_pending(agent.key_url.as_deref().unwrap_or_default());
                return outcome.fail(
                    STATUS_TIMEOUT,
                    REASON_DIRECTORY_PENDING,
                    NO_VALUE_DETAIL_NOT_SENT,
                );
            }
            Lookup::Resolved(entry) => entry,
        };
        let Some(directory) = entry.directory.as_ref() else {
            return outcome.fail(
                STATUS_UNVERIFIED,
                REASON_DIRECTORY_UNAVAILABLE,
                NO_VALUE_DETAIL_NOT_SENT,
            );
        };
        // Once the directory has been read, what the agent publishes about
        // itself is reported whatever the later checks decide.
        outcome.directory_was_read = true;
        outcome.purpose = directory.purpose.clone();
        match &entry.card {
            Some(card) => apply_card(&mut outcome, card),
            None => {
                if let Some(cards) = self.registry_cards() {
                    let card = agent.key_url.as_ref().and_then(|url| cards.get(url));
                    if let Some(card) = card {
                        apply_card(&mut outcome, card);
                    }
                }
            }
        }
        let Some(key) = directory.find_key(key_id) else {
            return outcome.fail(STATUS_INVALID, REASON_UNKNOWN_KEY, NO_VALUE_DETAIL_NOT_SENT);
        };
        if !key.is_valid_at(created) {
            return outcome.fail(STATUS_INVALID, REASON_KEY_EXPIRED, NO_VALUE_DETAIL_NOT_SENT);
        }
        let algorithm = resolve_algorithm(key, candidate.algorithm.as_deref());
        outcome.algorithm = Some(algorithm.name.clone());
        if !algorithm.supported {
            self.report_unsupported_algorithm(&algorithm.name, key_id);
            return outcome.fail(
                STATUS_UNVERIFIED,
                REASON_UNSUPPORTED_ALGORITHM,
                NO_VALUE_DETAIL_NOT_SENT,
            );
        }
        if !verify(
            &algorithm.name,
            key,
            &ascii_bytes(&base),
            &candidate.signature,
        ) {
            return outcome.fail(
                STATUS_INVALID,
                REASON_SIGNATURE_MISMATCH,
                NO_VALUE_DETAIL_NOT_SENT,
            );
        }
        outcome.status = STATUS_VERIFIED;
        outcome.reason = REASON_VERIFIED;
        outcome
    }

    /// The reason a signature's times rule it out, if they do. The sums are
    /// made in nanoseconds over a wide integer so that no setting the builder
    /// allows and no time a signature can name overflows them.
    fn check_times(&self, created: i64, expires: i64) -> Option<&'static str> {
        let now = (self.settings.clock)();
        let now = i128::from(now.timestamp()) * NANOSECONDS_PER_SECOND
            + i128::from(now.timestamp_subsec_nanos());
        let skew = to_nanoseconds(self.settings.clock_skew);
        let created = i128::from(created) * NANOSECONDS_PER_SECOND;
        let expires = i128::from(expires) * NANOSECONDS_PER_SECOND;
        if created - skew > now {
            return Some(REASON_NOT_YET_VALID);
        }
        if expires + skew < now {
            return Some(REASON_EXPIRED);
        }
        let max_lifetime = to_nanoseconds(self.settings.max_lifetime);
        if max_lifetime > 0 && expires - created > max_lifetime {
            return Some(REASON_EXPIRED);
        }
        None
    }

    /// The cards from the configured registries. The registries are read
    /// once, on a thread of its own, the first time a request needs them. A
    /// request that arrives before the read finishes waits for it for at most
    /// the wait budget and then gets no card, because a card never changes
    /// whether a signature is valid.
    fn registry_cards(&self) -> Option<CardsByKeyUrl> {
        if self.settings.registries.is_empty() {
            return None;
        }
        let pending = self.cards.get_or_init(|| {
            let pending = Arc::new(Pending::default());
            let target = Arc::clone(&pending);
            let fetcher = Arc::clone(&self.fetcher);
            let registries = self.settings.registries.clone();
            let fetch_timeout = self.settings.fetch_timeout;
            let spawned = thread::Builder::new()
                .name(BACKGROUND_THREAD_NAME.to_owned())
                .spawn(move || {
                    let loaded = catch_unwind(AssertUnwindSafe(|| {
                        load_cards(&fetcher, &registries, fetch_timeout)
                    }));
                    let cards = loaded.unwrap_or_else(|_| {
                        log_registry_failure(&registries.join(", "), "the read did not complete");
                        HashMap::new()
                    });
                    target.complete(Arc::new(cards));
                });
            if let Err(error) = spawned {
                log_registry_failure(
                    &self.settings.registries.join(", "),
                    &format!("the read could not be started: {error}"),
                );
                pending.complete(Arc::new(HashMap::new()));
            }
            pending
        });
        pending.wait(self.settings.wait_budget)
    }

    /// A shared secret arriving from an agent points at a misconfigured
    /// agent, which is worth an operator's attention once for each key rather
    /// than on every request. The key id is chosen by whoever holds the
    /// directory, so the set remembered is bounded.
    fn report_unsupported_algorithm(&self, algorithm: &str, key_id: &str) {
        if algorithm == ALGORITHM_HMAC_SHA256
            && log::log_enabled!(log::Level::Warn)
            && remember_once(&self.shared_secret_keys_logged, key_id)
        {
            log::warn!(
                "A request was signed with the shared secret algorithm '{algorithm}' using key \
                 id '{key_id}'. The Web Bot Auth protocol forbids shared secrets, so the \
                 signature was not verified. This points at a misconfigured agent and is worth \
                 an operator's attention."
            );
        }
    }

    /// The first Timeout for each directory is written at warning level, so
    /// a slow or unreachable directory shows in production logs, and every
    /// later one at debug level, so a popular agent's slow directory does not
    /// drown the log.
    fn log_directory_pending(&self, key_url: &str) {
        let budget = self.settings.wait_budget.as_millis();
        if log::log_enabled!(log::Level::Warn)
            && remember_once(&self.pending_directories_logged, key_url)
        {
            log::warn!(
                "The key directory at '{key_url}' had not been fetched after {budget}ms, so \
                 this request reports the Timeout status. The fetch keeps running, so a later \
                 request from this agent finds the result."
            );
        } else {
            log::debug!(
                "The key directory at '{key_url}' had not been fetched after {budget}ms, so \
                 this request reports the Timeout status. The fetch keeps running, so a later \
                 request from this agent finds the result."
            );
        }
    }
}

impl Drop for AgentSignatureElement {
    fn drop(&mut self) {
        // Read by the reachability check, which would otherwise report an
        // ordinary shutdown as a deployment that cannot reach the keys.
        self.disposing.store(true, Ordering::SeqCst);
    }
}

impl FlowElement for AgentSignatureElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let values = self.values_for(data.evidence());
        let element_data =
            data.get_or_add(Self::KEY, || AgentSignatureData::new(Arc::clone(&values)))?;
        *element_data = AgentSignatureData::new(values);
        Ok(())
    }

    fn data_key(&self) -> &str {
        DEFAULT_ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// What the element worked out about one request, before it becomes the
/// element data.
struct Outcome {
    status: &'static str,
    reason: &'static str,
    agent: Option<String>,
    key_id: Option<String>,
    algorithm: Option<String>,
    created: Option<DateTime<Utc>>,
    expires: Option<DateTime<Utc>>,
    nonce: Option<String>,
    purpose: Option<String>,
    name: Option<String>,
    product_token: Option<String>,
    card_url: Option<String>,
    directory_was_read: bool,
    /// The message a detail property carries when it has no value.
    missing: &'static str,
}

impl Outcome {
    fn new(missing: &'static str) -> Self {
        Outcome {
            status: STATUS_INVALID,
            reason: REASON_MALFORMED,
            agent: None,
            key_id: None,
            algorithm: None,
            created: None,
            expires: None,
            nonce: None,
            purpose: None,
            name: None,
            product_token: None,
            card_url: None,
            directory_was_read: false,
            missing,
        }
    }

    fn fail(mut self, status: &'static str, reason: &'static str, missing: &'static str) -> Self {
        self.status = status;
        self.reason = reason;
        self.missing = missing;
        self
    }

    fn into_values(self) -> AgentSignatureValues {
        let missing = self.missing;
        let text = |value: Option<String>| match value.filter(|text| !text.is_empty()) {
            Some(value) => AspectPropertyValue::new(value),
            None => AspectPropertyValue::no_value(missing),
        };
        let time = |value: Option<DateTime<Utc>>| match value {
            Some(value) => AspectPropertyValue::new(value),
            None => AspectPropertyValue::no_value(missing),
        };
        let card = |value: Option<String>| match value.filter(|text| !text.is_empty()) {
            Some(value) => AspectPropertyValue::new(value),
            None => AspectPropertyValue::no_value(NO_VALUE_NO_CARD),
        };
        let purpose = match self.purpose.filter(|text| !text.is_empty()) {
            Some(purpose) => AspectPropertyValue::new(purpose),
            None if self.directory_was_read => {
                AspectPropertyValue::no_value(NO_VALUE_PURPOSE_NOT_STATED)
            }
            None => AspectPropertyValue::no_value(NO_VALUE_PURPOSE_NOT_READ),
        };
        AgentSignatureValues {
            status: AspectPropertyValue::new(self.status.to_owned()),
            reason: AspectPropertyValue::new(self.reason.to_owned()),
            agent: text(self.agent),
            key_id: text(self.key_id),
            algorithm: text(self.algorithm),
            created: time(self.created),
            expires: time(self.expires),
            nonce: text(self.nonce),
            purpose,
            name: card(self.name),
            product_token: card(self.product_token),
            card_url: card(self.card_url),
        }
    }
}

/// The values for a request with no signature headers.
fn absent_values() -> AgentSignatureValues {
    AgentSignatureValues {
        status: AspectPropertyValue::new(STATUS_ABSENT.to_owned()),
        reason: AspectPropertyValue::new(REASON_NO_SIGNATURE.to_owned()),
        agent: AspectPropertyValue::no_value(NO_VALUE_DETAIL_ABSENT),
        key_id: AspectPropertyValue::no_value(NO_VALUE_DETAIL_ABSENT),
        algorithm: AspectPropertyValue::no_value(NO_VALUE_DETAIL_ABSENT),
        created: AspectPropertyValue::no_value(NO_VALUE_DETAIL_ABSENT),
        expires: AspectPropertyValue::no_value(NO_VALUE_DETAIL_ABSENT),
        nonce: AspectPropertyValue::no_value(NO_VALUE_DETAIL_ABSENT),
        purpose: AspectPropertyValue::no_value(NO_VALUE_PURPOSE_NOT_READ),
        name: AspectPropertyValue::no_value(NO_VALUE_NO_CARD),
        product_token: AspectPropertyValue::no_value(NO_VALUE_NO_CARD),
        card_url: AspectPropertyValue::no_value(NO_VALUE_NO_CARD),
    }
}

/// Fill the card properties, and the purpose where the directory does not
/// give one.
fn apply_card(outcome: &mut Outcome, card: &AgentCard) {
    outcome.name = card.client_name.clone();
    outcome.product_token = card.product_token.clone();
    outcome.card_url = Some(card.client_id.clone());
    if outcome.purpose.as_deref().is_none_or(str::is_empty) {
        outcome.purpose = card.purpose.clone();
    }
}

/// Read every configured registry and the cards it lists, one after another,
/// all within one fetch timeout.
fn load_cards(
    fetcher: &Fetcher,
    registries: &[String],
    fetch_timeout: Duration,
) -> HashMap<String, AgentCard> {
    let deadline = deadline_after(fetch_timeout);
    let mut cards = HashMap::new();
    for registry in registries {
        if Instant::now() >= deadline {
            break;
        }
        for card_url in fetcher.fetch_registry(registry, deadline) {
            if Instant::now() >= deadline {
                break;
            }
            let Some(card) = fetcher.fetch_card(&card_url, deadline) else {
                continue;
            };
            if let Some(jwks_uri) = card.jwks_uri.clone().filter(|uri| !uri.is_empty()) {
                cards.insert(jwks_uri, card);
            }
        }
    }
    cards
}

/// Whether the signature covers something that ties it to the request it
/// arrived on.
fn covers_the_request(candidate: &SignatureCandidate) -> bool {
    candidate
        .covered_components
        .iter()
        .any(|component| matches!(component.as_string(), Some("@authority" | "@target-uri")))
}

/// The `Signature-Agent` member the signature covers. A member the signature
/// does not cover says nothing about the agent, because anyone can add a
/// header to a request.
fn find_agent<'a>(
    candidate: &SignatureCandidate,
    agents: Option<&'a [SignatureAgentEntry]>,
) -> Option<&'a SignatureAgentEntry> {
    let agents = agents.filter(|agents| !agents.is_empty())?;
    let component = candidate
        .covered_components
        .iter()
        .find(|component| component.as_string() == Some(EVIDENCE_SIGNATURE_AGENT_NAME))?;
    match component.string_parameter("key") {
        // The whole header is covered, so it has to name one agent for there
        // to be no doubt which one signed.
        None => match agents {
            [only] => Some(only),
            _ => None,
        },
        Some(label) => agents.iter().find(|agent| agent.label == label),
    }
}

/// Rebuilds the covered components of a request signature from the
/// evidence, reading only the source the signature arrived under, because
/// a base built from two different requests is one no agent ever signed.
struct EvidenceResolver<'a> {
    evidence: &'a Evidence,
    source: &'a Source,
    filter: &'a AgentSignatureEvidenceKeyFilter,
}

impl EvidenceResolver<'_> {
    fn header(&self, name: &str) -> Option<&str> {
        self.evidence
            .get(&format!("{}.{}", self.source.prefix, name))
    }

    /// A header the signature covers. One the element accepts is read from
    /// the evidence alone. Any other is read from the evidence, and where the
    /// evidence does not hold it, from the web request, which no element
    /// accepts, so such a header never reaches a cache key, the ETag or the
    /// Vary header.
    fn covered_header(&self, name: &str) -> Option<&str> {
        let key = format!("{}.{}", self.source.prefix, name);
        self.evidence.get(&key).or_else(|| {
            if self.filter.include(&key) {
                return None;
            }
            self.evidence.http_request()?.get(&key)
        })
    }

    fn scheme(&self) -> Option<String> {
        self.header("protocol")
            .map(|protocol| protocol.trim().to_lowercase())
    }

    fn derived(&self, name: &str) -> Option<String> {
        match name {
            "@authority" => build_authority(self.header("host")?, self.scheme().as_deref()),
            "@scheme" => self.scheme(),
            // RFC 9421 section 2.2.1 takes the method as the request carried
            // it, with no change to its case.
            "@method" => self
                .evidence
                .get(self.source.method)
                .filter(|method| !method.is_empty())
                .map(str::to_owned),
            // RFC 9421 section 2.2.6. An empty path is the single slash.
            "@path" => self.evidence.get(self.source.path).map(|path| {
                if path.is_empty() {
                    "/".to_owned()
                } else {
                    path.to_owned()
                }
            }),
            // RFC 9421 section 2.2.7. A request with no query still has the
            // question mark on its own.
            "@query" => self
                .evidence
                .get(self.source.query)
                .map(|query| format!("?{query}")),
            "@target-uri" => self.target_uri(),
            _ => None,
        }
    }

    /// RFC 9421 section 2.2.2, the whole address the request was made to.
    /// Every part has to be there, because an address missing its path or
    /// its query is a different address.
    fn target_uri(&self) -> Option<String> {
        let scheme = self.scheme()?;
        let host = self.header("host")?;
        let path = self.evidence.get(self.source.path)?;
        let query = self.evidence.get(self.source.query)?;
        let authority = build_authority(host, Some(&scheme))?;
        // The evidence holds the query without its '?', so a request with no
        // query and one with an empty query look the same, and the common
        // case, no query at all, is what is built.
        let path = if path.is_empty() { "/" } else { path };
        let query = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        Some(format!("{scheme}://{authority}{path}{query}"))
    }
}

impl ComponentResolver for EvidenceResolver<'_> {
    fn resolve(&self, name: &str, component: &Item) -> Option<String> {
        // A component of a related request, which only arises when signing a
        // response.
        if name.is_empty() || component.parameter("req").is_some() {
            return None;
        }
        if name.starts_with('@') {
            return self.derived(name);
        }
        let header = self.covered_header(name)?;
        if let Some(label) = component.string_parameter("key") {
            // RFC 9421 section 2.1.2 puts the strict serialisation of the
            // member value on the line, not the text the sender wrote.
            let dictionary = parse_dictionary(header)?;
            return dictionary.get(label).map(serialize_member);
        }
        // The strict and byte sequence forms need the header written again,
        // which this element does not do.
        if ["sf", "bs", "tr"]
            .iter()
            .any(|flag| component.parameter(flag).is_some())
        {
            return None;
        }
        Some(header.trim().to_owned())
    }
}

/// The evidence the element accepts, being the three signature headers, the
/// host, the protocol and the request line, and where forwarded evidence is
/// trusted, everything under the query prefix. Only these can change the
/// element's results for every signature, so only these reach a cache key,
/// the ETag or the Vary header. Any other header a signature covers is read
/// from the evidence or the web request without being accepted.
struct AgentSignatureEvidenceKeyFilter {
    trust_forwarded: bool,
    named: EvidenceKeyFilterWhitelist,
}

impl AgentSignatureEvidenceKeyFilter {
    fn new(trust_forwarded: bool) -> Self {
        AgentSignatureEvidenceKeyFilter {
            trust_forwarded,
            named: EvidenceKeyFilterWhitelist::new([
                EVIDENCE_SIGNATURE_KEY,
                EVIDENCE_SIGNATURE_INPUT_KEY,
                EVIDENCE_SIGNATURE_AGENT_KEY,
                EVIDENCE_HOST_KEY,
                EVIDENCE_PROTOCOL_KEY,
                HEADER_SOURCE.method,
                HEADER_SOURCE.path,
                HEADER_SOURCE.query,
            ]),
        }
    }
}

impl EvidenceKeyFilter for AgentSignatureEvidenceKeyFilter {
    fn include(&self, key: &str) -> bool {
        (self.trust_forwarded && starts_with_prefix(key, EVIDENCE_QUERY_PREFIX))
            || self.named.include(key)
    }

    fn order(&self, key: &str) -> Option<i32> {
        self.include(key).then_some(EVIDENCE_ORDER)
    }

    fn named_keys(&self) -> Option<Vec<String>> {
        self.named.named_keys()
    }
}

/// Whether `key` is `prefix` followed by the separator and a field, ignoring
/// case.
fn starts_with_prefix(key: &str, prefix: &str) -> bool {
    key.len() > prefix.len()
        && key.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
        && key.as_bytes()[prefix.len()] == b'.'
}

/// Remember `key` in `set`, returning true the first time and false
/// afterwards, or once the set holds as many as it is allowed to.
fn remember_once(set: &Mutex<HashSet<String>>, key: &str) -> bool {
    let mut set = lock(set);
    set.len() < MAXIMUM_LOGGED_KEYS && set.insert(key.to_owned())
}

/// A signature time, or `None` where it names a time outside the years 1 to
/// 9999.
fn from_unix_seconds(seconds: i64) -> Option<DateTime<Utc>> {
    if !(EARLIEST_UNIX_SECONDS..=LATEST_UNIX_SECONDS).contains(&seconds) {
        return None;
    }
    DateTime::from_timestamp(seconds, 0)
}

fn to_nanoseconds(duration: Duration) -> i128 {
    i128::try_from(duration.as_nanos()).unwrap_or(i128::MAX)
}

fn to_time_delta(duration: Duration) -> chrono::TimeDelta {
    chrono::TimeDelta::from_std(duration).unwrap_or(chrono::TimeDelta::MAX)
}

fn deadline_after(timeout: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(timeout).unwrap_or(now)
}
