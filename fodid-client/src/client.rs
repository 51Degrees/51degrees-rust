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

//! The client, being everything a server does with a 51Did against the
//! 51Degrees cloud.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use fodid::FodId;

use crate::error::{Error, Result};
use crate::http::{DidHttpClient, DidHttpRequest, DidHttpResponse, HttpMethod};
use crate::key::{candidates_for_date, covers, in_force_at, merge_keys, parse_keys, DidPublicKey};
use crate::outcome::SignatureCheck;
use crate::redeem::RedeemResult;

/// The public cloud API base, used when no endpoint is given and
/// [`ENDPOINT_ENVIRONMENT_VARIABLE`] is not set.
pub const DEFAULT_ENDPOINT: &str = "https://cloud.51degrees.com/api/v4/";

/// The environment variable read for the API base when the builder is given
/// none, the same variable the cloud request engine honours. A host other
/// than the public cloud is used for a privately hosted copy of the same
/// service.
pub const ENDPOINT_ENVIRONMENT_VARIABLE: &str = "_51DEGREES_CLOUD_ENDPOINT";

/// How old the held key list may be before a lookup fetches the whole list
/// again, with no `datetime`. Only a fetch of the whole list, which the first
/// fetch is too, resets the list's age.
///
/// A key may be replaced before its end, for example if it is compromised.
/// The client picks up the replacement on the first signature that fails
/// under the keys it holds, or at the latest once the list is this old.
pub const KEY_CACHE_LIFETIME: Duration = Duration::days(1);

/// The least time between two fetches made because the keys held do not
/// cover a 51Did's date, or because a signature failed under every key held
/// for its date. A 51Did dated in a period not published yet, or given a
/// false date, then costs at most one request a minute however often it is
/// presented. The first fetch and the fetch of the whole list once it is
/// [`KEY_CACHE_LIFETIME`] old neither count towards this nor wait for it.
const REFETCH_INTERVAL: Duration = Duration::minutes(1);

/// The `User-Agent` every request carries, naming this crate and its
/// version.
pub const USER_AGENT: &str = concat!("fodid-client/", env!("CARGO_PKG_VERSION"));

/// The request header the signing key fetch carries the licence key in,
/// where the builder was given one. A header rather than the URL, because a
/// URL is written to access logs.
pub const LICENCE_KEY_HEADER: &str = "X-51D-License-Key";

/// The longest encoded value the client will parse or send.
///
/// A guard against obviously malformed input, so the client does no work
/// and makes no call for a value that cannot be an identifier. The figure is
/// arbitrary and deliberately generous, well beyond anything the cloud
/// issues, because the length of a 51Did is the cloud's business and not
/// this crate's.
pub const MAXIMUM_ENCODED_LENGTH: usize = 4096;

/// The clock the key cache ages against, replaceable so a test can move
/// time on without waiting.
type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// The held keys, when they were last fetched, and the fetch in flight.
///
/// The lock around this is only ever held between awaits, never across
/// one, so a slow fetch blocks no other caller. A caller that finds a fetch
/// already in flight waits for that one to land instead of starting a
/// second, which is what keeps concurrent lookups down to one request.
struct KeyCache {
    /// Every key fetched so far, merged by start, or `None` before the first
    /// fetch lands.
    keys: Option<Vec<DidPublicKey>>,
    /// When the whole list last landed, which sets its age.
    fetched_at: DateTime<Utc>,
    /// When the latest fetch [`REFETCH_INTERVAL`] spaces started, landed or
    /// not.
    refetched_at: Option<DateTime<Utc>>,
    /// Counts the fetches that have landed, so a caller that waited on
    /// another caller's fetch can tell whether one did.
    generation: u64,
    /// Whether a fetch is in flight.
    fetching: bool,
    /// The callers waiting for the fetch in flight to finish, woken when it
    /// lands or fails.
    waiters: Vec<Waker>,
}

/// What a lookup needs before the keys held can answer it.
enum Need {
    /// Nothing, because the keys held answer it.
    Nothing,
    /// The whole list, fetched with no `datetime`, which resets its age.
    Whole,
    /// A fetch spaced by [`REFETCH_INTERVAL`] of the entries starting at or
    /// after the moment given, or of the whole list where none is given.
    Refetch(Option<DateTime<Utc>>),
}

/// Everything a server does with a 51Did against the 51Degrees cloud: fetch
/// and cache the signing public keys, verify a signature offline against the
/// key in force when the identifier was created, verify a signature through
/// the cloud, and redeem a sealed creator context result with the account's
/// licence key.
///
/// Creating a 51Did is not part of this client. Creation is the cloud `json`
/// endpoint through the cloud request engine and pipeline, and a page
/// creates from the browser because the identifier describes the browser's
/// own connection. The `verify-context` and `verify-full` endpoints are
/// browser calls for the same reason, so they are not here either. This
/// client is the server side, which holds the licence key the browser never
/// sees.
///
/// The licence key never travels in a URL, because a URL is written to
/// access logs. The redeem call sends it in the POST form body, and the
/// signing key fetch sends it in the [`LICENCE_KEY_HEADER`] header. The
/// resource key is part of the route for the verify call and for a key fetch
/// made without a licence key, as those endpoints accept, and the redeem
/// call sends it in the form body.
///
/// Every method that may reach the network is `async` and is awaited. The
/// futures are driven by whatever runtime the host has, because the crate
/// carries none of its own, and they are not required to be `Send`, so a
/// single-threaded host such as a `wasm32-wasip1` edge runtime can await
/// them. The key cache is per instance and safe to share across threads, so
/// create one client for the process and reuse it. Concurrent callers that
/// each find the cache needs fetching share one fetch rather than each
/// making their own.
pub struct DidClient {
    http: Arc<dyn DidHttpClient>,
    resource_key: String,
    licence_key: Option<String>,
    /// The resource key, and the licence key where one was given, kept so an
    /// error on the way out can have them masked by value. Matching
    /// the value itself catches a credential the shape rules in
    /// [`crate::redact`] would not recognise.
    secrets: Vec<String>,
    endpoint: String,
    clock: Clock,
    cache: Mutex<KeyCache>,
}

impl std::fmt::Debug for DidClient {
    /// Names the endpoint and whether a licence key is held, and never the
    /// licence key itself.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DidClient")
            .field("endpoint", &self.endpoint)
            .field("has_licence_key", &self.licence_key.is_some())
            .finish_non_exhaustive()
    }
}

/// Builds a [`DidClient`]. Start from [`DidClient::builder`].
pub struct DidClientBuilder {
    resource_key: String,
    licence_key: Option<String>,
    endpoint: Option<String>,
    http: Option<Arc<dyn DidHttpClient>>,
    clock: Option<Clock>,
}

impl DidClientBuilder {
    /// A licence key of the same account, server side only, and never put
    /// in a URL. An empty value is the same as none.
    ///
    /// The redeem call sends it in the form body, where it is needed if the
    /// account holds licence keys. The signing key fetch sends it in the
    /// [`LICENCE_KEY_HEADER`] header in place of the resource key in the
    /// route. A call from a server carries no `Origin` or `Referer`, so the
    /// cloud refuses one made on a resource key restricted to named web
    /// domains. The cloud also reads the resource key first when a request
    /// carries both keys, which is why the fetch sends the licence key alone.
    pub fn licence_key(mut self, licence_key: impl Into<String>) -> Self {
        let value = licence_key.into();
        self.licence_key = if value.is_empty() { None } else { Some(value) };
        self
    }

    /// The API base including `/api/v4/`. When not given,
    /// [`ENDPOINT_ENVIRONMENT_VARIABLE`] is read, and when that is unset too
    /// [`DEFAULT_ENDPOINT`] is used. A value with or without a trailing
    /// slash is accepted, and is normalised to end in exactly one.
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// The transport to send through, so a test can stand in for the
    /// network and a host without `reqwest` can supply its own.
    ///
    /// Without the `reqwest-client` feature this is required, because the
    /// crate then carries no transport of its own.
    pub fn http_client(mut self, http: Arc<dyn DidHttpClient>) -> Self {
        self.http = Some(http);
        self
    }

    /// The clock the key cache ages against, so a test can move time on.
    /// The system clock is used when none is given.
    pub fn clock(mut self, clock: impl Fn() -> DateTime<Utc> + Send + Sync + 'static) -> Self {
        self.clock = Some(Arc::new(clock));
        self
    }

    /// Builds the client.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] when the resource key is blank, the
    /// endpoint is not an absolute URL, or no transport was given and the
    /// crate was built without the `reqwest-client` feature.
    pub fn build(self) -> Result<DidClient> {
        if self.resource_key.trim().is_empty() {
            return Err(Error::InvalidArgument(
                "a resource key is required".to_string(),
            ));
        }
        let endpoint = normalise_endpoint(self.endpoint.or_else(read_endpoint_variable))?;
        let http = match self.http {
            Some(http) => http,
            None => default_transport()?,
        };
        let clock: Clock = self.clock.unwrap_or_else(|| Arc::new(Utc::now));
        let fetched_at = clock();
        let mut secrets = vec![self.resource_key.clone()];
        if let Some(licence_key) = &self.licence_key {
            secrets.push(licence_key.clone());
        }
        Ok(DidClient {
            http,
            resource_key: self.resource_key,
            licence_key: self.licence_key,
            secrets,
            endpoint,
            clock,
            cache: Mutex::new(KeyCache {
                keys: None,
                fetched_at,
                refetched_at: None,
                generation: 0,
                fetching: false,
                waiters: Vec::new(),
            }),
        })
    }
}

#[cfg(feature = "reqwest-client")]
fn default_transport() -> Result<Arc<dyn DidHttpClient>> {
    let client = crate::http::ReqwestClient::new(std::time::Duration::from_secs(30))
        .map_err(Error::Transport)?;
    Ok(Arc::new(client))
}

#[cfg(not(feature = "reqwest-client"))]
fn default_transport() -> Result<Arc<dyn DidHttpClient>> {
    Err(Error::InvalidArgument(
        "a transport is required: this build carries no HTTP client of its \
         own, so give the builder a DidHttpClient or enable the \
         reqwest-client feature"
            .to_string(),
    ))
}

fn read_endpoint_variable() -> Option<String> {
    std::env::var(ENDPOINT_ENVIRONMENT_VARIABLE)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// Trims the endpoint, makes it end in exactly one slash, and refuses
/// anything that is not an absolute URL.
fn normalise_endpoint(endpoint: Option<String>) -> Result<String> {
    let value = match endpoint {
        Some(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => DEFAULT_ENDPOINT.to_string(),
    };
    let value = format!("{}/", value.trim_end_matches('/'));
    let absolute = value.split_once("://").is_some_and(|(scheme, rest)| {
        !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && rest.len() > 1
    });
    if !absolute {
        return Err(Error::InvalidArgument(format!(
            "the endpoint '{value}' is not an absolute URL"
        )));
    }
    Ok(value)
}

impl DidClient {
    /// Starts building a client for the resource key, which is public by
    /// nature.
    pub fn builder(resource_key: impl Into<String>) -> DidClientBuilder {
        DidClientBuilder {
            resource_key: resource_key.into(),
            licence_key: None,
            endpoint: None,
            http: None,
            clock: None,
        }
    }

    /// The resource key the client sends.
    pub fn resource_key(&self) -> &str {
        &self.resource_key
    }

    /// The API base every request is built on, ending in one slash.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Whether a licence key was given. The key itself is not exposed.
    pub fn has_licence_key(&self) -> bool {
        self.licence_key.is_some()
    }

    /// The signing public keys held, fetched on first use. A later fetch
    /// adds to them and never removes one, because a 51Did made long ago
    /// verifies against the key of its own period. Use
    /// [`DidClient::public_key_for`] to pick the key for one identifier,
    /// which also fetches the keys again when that is due.
    ///
    /// # Errors
    ///
    /// [`Error::Transport`] when the cloud cannot be reached, and
    /// [`Error::UnexpectedStatus`] when it answers with a status other than
    /// 200. A 401 means the cloud refused the key the fetch was made with,
    /// for example a resource key restricted to named web domains, which a
    /// server avoids by giving the builder a licence key.
    pub async fn public_keys(&self) -> Result<Vec<DidPublicKey>> {
        let (keys, _) = self
            .keys_where(|cache| match cache.keys {
                None => Need::Whole,
                Some(_) => Need::Nothing,
            })
            .await?;
        Ok(keys)
    }

    /// Fetches the key list from the cloud and returns the entries as the
    /// answer gives them, sorted by start, without holding or merging them.
    ///
    /// This is for a caller that keeps its own key list rather than using
    /// this client's, and adds each answer to it with [`merge_keys`].
    /// `since` is sent as the key route's `datetime`, in whole seconds at or
    /// before it, so the answer holds the entries starting at or after it,
    /// and `None` fetches the whole list. [`covers`] and [`merge_keys`] say
    /// which to send when. The request is the one this client's own fetches
    /// make. With a licence key it is the bare `id/key` route carrying that
    /// key in [`LICENCE_KEY_HEADER`] and no resource key, for the reasons
    /// [`DidClientBuilder::licence_key`] gives, and without one the resource
    /// key is the last segment of the route. The answer is read by
    /// [`parse_keys`].
    ///
    /// No limit applies, so the caller decides how often to fetch, and the
    /// keys this client holds for its own lookups are left as they are.
    ///
    /// # Errors
    ///
    /// [`Error::Transport`] when the cloud cannot be reached,
    /// [`Error::UnexpectedStatus`] when it answers with a status other than
    /// 200, and [`Error::Protocol`] when the answer is not a key list or has
    /// an entry whose end is not after its start.
    pub async fn fetch_keys_from(&self, since: Option<DateTime<Utc>>) -> Result<Vec<DidPublicKey>> {
        let (mut url, headers) = match &self.licence_key {
            Some(licence_key) => (
                format!("{}id/key", self.endpoint),
                vec![(LICENCE_KEY_HEADER.to_string(), licence_key.clone())],
            ),
            None => (
                format!(
                    "{}id/key/{}",
                    self.endpoint,
                    escape_data_string(&self.resource_key)
                ),
                Vec::new(),
            ),
        };
        if let Some(since) = since {
            // Whole seconds, which is at or before `since`, so the answer
            // carries the entry starting then, whose end may have changed.
            let datetime = since.to_rfc3339_opts(SecondsFormat::Secs, true);
            url = format!("{url}?datetime={}", escape_data_string(&datetime));
        }
        let response = self.send(HttpMethod::Get, url, headers, Vec::new()).await?;
        if response.status != 200 {
            return Err(self.unexpected("key", &response));
        }
        parse_keys(&response.body)
    }

    /// The key in force when the identifier was created, being the entry
    /// whose start is latest on or before the identifier's date, unless that
    /// entry's end has passed by then.
    ///
    /// The whole list is fetched first when none is held or it is older
    /// than [`KEY_CACHE_LIFETIME`]. The entries from the newest start held
    /// onwards are fetched first when the keys held do not cover the date as
    /// [`covers`] decides, at most once a minute.
    ///
    /// Answers `None` when no key held is in force at the date.
    ///
    /// # Errors
    ///
    /// [`Error::Transport`] and [`Error::UnexpectedStatus`] when a fetch was
    /// needed and did not answer with 200.
    pub async fn public_key_for(&self, fod_id: &FodId) -> Result<Option<DidPublicKey>> {
        let date = fod_id.date();
        let (keys, _) = self.keys_covering(date).await?;
        Ok(in_force_at(&keys, date).cloned())
    }

    /// Verifies the identifier's signature offline against the published
    /// keys, without a cloud call once the keys are cached.
    ///
    /// True only when the signature verifies under a key in force at the
    /// identifier's date. See [`DidClient::verify_signature_detailed`] for
    /// why a check did not pass.
    pub async fn verify_signature(&self, fod_id: &FodId) -> Result<bool> {
        Ok(self.verify_signature_detailed(fod_id).await? == SignatureCheck::Verified)
    }

    /// Verifies the identifier's signature offline and says why when the
    /// check did not pass.
    ///
    /// The keys tried are the one in force at the identifier's date and,
    /// near a boundary in the schedule, the neighbouring key where the two
    /// differ, best first. A longer payload carries a creator context
    /// section and is accepted, because the signature covers the whole
    /// payload.
    ///
    /// The keys are fetched as for [`DidClient::public_key_for`]. When no
    /// key tried verifies the signature, and no fetch landed for this check,
    /// the entries from the start of the key held for the identifier's date
    /// onwards are fetched, within the same once a minute limit, and the
    /// check is made once more before the answer is given, because a key may
    /// be replaced before its end.
    ///
    /// # Errors
    ///
    /// [`Error::Transport`] and [`Error::UnexpectedStatus`] when a key fetch
    /// was needed and did not answer with 200.
    pub async fn verify_signature_detailed(&self, fod_id: &FodId) -> Result<SignatureCheck> {
        let date = fod_id.date();
        let (keys, fetched) = self.keys_covering(date).await?;
        let check = check_signature(fod_id, &keys, date);
        // A list fetched for this check cannot get better by fetching again.
        if fetched || matches!(check, SignatureCheck::Verified | SignatureCheck::NoKey) {
            return Ok(check);
        }
        let fresh = self.keys_after_failure(date).await?;
        if fresh == keys {
            return Ok(check);
        }
        Ok(check_signature(fod_id, &fresh, date))
    }

    /// Verifies the identifier's signature through the cloud's verify
    /// endpoint, which needs no licence key and counts as one use.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] with the cloud's message when the cloud
    /// refused the value, [`Error::Transport`] when the cloud cannot be
    /// reached, and [`Error::UnexpectedStatus`] when it answers with a
    /// status this client does not expect.
    pub async fn verify(&self, fod_id: &FodId) -> Result<bool> {
        // A parsed identifier is already known to be a 51Did, so the string
        // surface's local check is not repeated.
        let encoded = fod_id
            .as_base64()
            .map_err(|e| Error::InvalidArgument(format!("the 51Did could not be encoded: {e}")))?;
        self.verify_encoded_unchecked(&encoded).await
    }

    /// Verifies a 51Did string's signature through the cloud's verify
    /// endpoint, which needs no licence key and counts as one use. The
    /// identifier is sent as `51did` and again as `owid`, the name the
    /// endpoint first went live under, so a service of either age answers.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] when the value is not a 51Did, refused
    /// here before any call is made, or with the cloud's message when the
    /// cloud refused it. [`Error::Transport`] when the cloud cannot be
    /// reached, and [`Error::UnexpectedStatus`] when it answers with a
    /// status this client does not expect.
    pub async fn verify_encoded(&self, fod_id: &str) -> Result<bool> {
        validate_encoded_value(fod_id)?;
        self.verify_encoded_unchecked(fod_id).await
    }

    async fn verify_encoded_unchecked(&self, fod_id: &str) -> Result<bool> {
        // The documented parameter is 51did. The same value is sent again as
        // owid, the name the verify endpoint first went live under, which a
        // service that predates the 51did name reads and a current one
        // accepts as an alias, so both answer.
        let encoded = escape_data_string(fod_id);
        let url = format!(
            "{}id/verify/{}?51did={encoded}&owid={encoded}",
            self.endpoint,
            escape_data_string(&self.resource_key)
        );
        let response = self
            .send(HttpMethod::Get, url, Vec::new(), Vec::new())
            .await?;
        if response.status == 200 || response.status == 400 {
            if let Some(valid) = read_valid(&response.body) {
                return Ok(valid);
            }
            if response.status == 400 {
                if let Some(errors) = read_errors(&response.body) {
                    // The service quotes the key back inside this text when it
                    // is the key it could not read.
                    return Err(Error::InvalidArgument(self.redacted(&errors)));
                }
            }
        }
        Err(self.unexpected("verify", &response))
    }

    /// Redeems a sealed creator context result against the identifier it
    /// was made for, sending the licence key where one was given. Counts as
    /// one use, the second of the two a browser-based context check costs.
    ///
    /// `result` is the sealed result the browser relayed, and `challenge`
    /// the single-use challenge given to the verify call, where one was.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] with the cloud's message when the cloud
    /// answered 400, [`Error::NotSupported`] when the host answered 404 and
    /// so does not offer the creator context, [`Error::Transport`] when the
    /// cloud cannot be reached, and [`Error::UnexpectedStatus`] for any
    /// other status.
    pub async fn redeem(
        &self,
        fod_id: &FodId,
        result: &str,
        challenge: Option<&str>,
    ) -> Result<RedeemResult> {
        // A parsed identifier is already known to be a 51Did, so the string
        // surface's local check is not repeated.
        let encoded = fod_id
            .as_base64()
            .map_err(|e| Error::InvalidArgument(format!("the 51Did could not be encoded: {e}")))?;
        self.redeem_encoded_unchecked(&encoded, result, challenge)
            .await
    }

    /// Redeems a sealed creator context result against a 51Did string. See
    /// [`DidClient::redeem`].
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] when the value is not a 51Did, refused
    /// here before any call is made, and otherwise as [`DidClient::redeem`].
    pub async fn redeem_encoded(
        &self,
        fod_id: &str,
        result: &str,
        challenge: Option<&str>,
    ) -> Result<RedeemResult> {
        validate_encoded_value(fod_id)?;
        self.redeem_encoded_unchecked(fod_id, result, challenge)
            .await
    }

    async fn redeem_encoded_unchecked(
        &self,
        fod_id: &str,
        result: &str,
        challenge: Option<&str>,
    ) -> Result<RedeemResult> {
        // Everything travels in the form body, the resource key included,
        // because the redeem endpoint's POST route is the bare path and reads
        // its parameters from the form. Nothing here is written to an access
        // log.
        let mut form = vec![
            ("resource".to_string(), self.resource_key.clone()),
            ("51did".to_string(), fod_id.to_string()),
            ("result".to_string(), result.to_string()),
            (
                "challenge".to_string(),
                challenge.unwrap_or_default().to_string(),
            ),
        ];
        if let Some(licence_key) = &self.licence_key {
            form.push(("license".to_string(), licence_key.clone()));
        }
        let url = format!("{}id/redeem", self.endpoint);
        let response = self.send(HttpMethod::Post, url, Vec::new(), form).await?;
        match response.status {
            200 | 503 => Ok(RedeemResult::from_response(response.status, &response.body)),
            400 => Err(Error::InvalidArgument(self.redacted(
                &read_errors(&response.body).unwrap_or_else(|| response.body.clone()),
            ))),
            404 => Err(Error::NotSupported(self.endpoint.clone())),
            _ => Err(self.unexpected("redeem", &response)),
        }
    }

    /// The keys held for a question about the date, and whether a fetch
    /// landed for it. The whole list is fetched first when none is held or
    /// it is older than [`KEY_CACHE_LIFETIME`], and the entries from the
    /// newest start held onwards when the keys held do not cover the date as
    /// [`covers`] decides.
    async fn keys_covering(&self, date: DateTime<Utc>) -> Result<(Vec<DidPublicKey>, bool)> {
        self.keys_where(|cache| match &cache.keys {
            None => Need::Whole,
            Some(_) if (self.clock)() - cache.fetched_at > KEY_CACHE_LIFETIME => Need::Whole,
            Some(keys) if !covers(keys, date) => {
                Need::Refetch(keys.iter().map(DidPublicKey::starts_at).max())
            }
            Some(_) => Need::Nothing,
        })
        .await
    }

    /// The keys held after a signature failed under every key held for its
    /// date, fetched again first from the start of the key held for that
    /// date onwards. The key may have been replaced before its end, and the
    /// answer then carries its entry with the earlier end, and the
    /// replacement, which starts inside its period.
    async fn keys_after_failure(&self, date: DateTime<Utc>) -> Result<Vec<DidPublicKey>> {
        let (keys, _) = self
            .keys_where(|cache| match &cache.keys {
                None => Need::Whole,
                Some(keys) => Need::Refetch(start_held_for(keys, date)),
            })
            .await?;
        Ok(keys)
    }

    /// Whether a fetch [`REFETCH_INTERVAL`] spaces may start now, being when
    /// none has started within it. A clock set back is no reason to stop
    /// fetching, so only a start in the past holds the next one back.
    fn refetch_due(&self, cache: &KeyCache) -> bool {
        cache.refetched_at.is_none_or(|started| {
            let elapsed = (self.clock)() - started;
            elapsed < Duration::zero() || elapsed >= REFETCH_INTERVAL
        })
    }

    /// The keys held, fetched first when `need` says the keys as they stand
    /// will not do, and whether a fetch landed on this caller's behalf.
    ///
    /// When another caller's fetch is already in flight this one waits for
    /// that fetch instead of making a second request, and answers from the
    /// keys that fetch landed. Only when the other fetch failed does this
    /// caller make a request of its own, and a fetch [`REFETCH_INTERVAL`]
    /// spaces only where the spacing allows it, so an answer here is always
    /// backed by at most one request made on this caller's behalf.
    async fn keys_where(
        &self,
        need: impl Fn(&KeyCache) -> Need,
    ) -> Result<(Vec<DidPublicKey>, bool)> {
        loop {
            // Everything under the lock is a plain read or a flag write, and
            // the lock is dropped before anything is awaited.
            let (generation, fetch) = {
                let mut cache = self.lock_cache();
                let (since, spaced) = match need(&cache) {
                    Need::Nothing => return Ok((cache.keys.clone().unwrap_or_default(), false)),
                    Need::Whole => (None, false),
                    Need::Refetch(since) => (since, true),
                };
                if cache.fetching {
                    // Waiting for a fetch already in flight costs no request,
                    // so the spacing does not hold it back.
                    (cache.generation, None)
                } else if spaced && !self.refetch_due(&cache) {
                    return Ok((cache.keys.clone().unwrap_or_default(), false));
                } else {
                    cache.fetching = true;
                    if spaced {
                        cache.refetched_at = Some((self.clock)());
                    }
                    (cache.generation, Some(since))
                }
            };
            if let Some(since) = fetch {
                return Ok((self.fetch_keys(since).await?, true));
            }
            FetchFinished { client: self }.await;
            let cache = self.lock_cache();
            if cache.generation != generation {
                return Ok((cache.keys.clone().unwrap_or_default(), true));
            }
            // The fetch waited on did not land, so this caller goes round
            // again and, finding nothing in flight, may make its own.
        }
    }

    /// Fetches the key list as [`DidClient::fetch_keys_from`] does and merges
    /// the answer into the keys held, being the whole list where `since` is
    /// `None`, which also resets its age, and otherwise the entries starting
    /// at or after `since`. Called only by the caller that set the in-flight
    /// flag, and clears that flag however it ends, the future being dropped
    /// before it finishes included, so no waiter is left waiting on a fetch
    /// that will never land.
    async fn fetch_keys(&self, since: Option<DateTime<Utc>>) -> Result<Vec<DidPublicKey>> {
        let _finished = FetchFinishes { client: self };
        let answer = self.fetch_keys_from(since).await?;
        let keys = {
            let mut cache = self.lock_cache();
            let held = cache.keys.get_or_insert_with(Vec::new);
            merge_keys(held, answer);
            let keys = held.clone();
            if since.is_none() {
                cache.fetched_at = (self.clock)();
            }
            cache.generation += 1;
            keys
        };
        Ok(keys)
    }

    fn lock_cache(&self) -> MutexGuard<'_, KeyCache> {
        // A thread that panicked while holding the lock leaves the cache in
        // a state that is still a whole key list or none, so the guard is
        // taken over rather than the poison spread to every later caller.
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    async fn send(
        &self,
        method: HttpMethod,
        url: String,
        headers: Vec<(String, String)>,
        form: Vec<(String, String)>,
    ) -> Result<DidHttpResponse> {
        let request = DidHttpRequest {
            method,
            url,
            form,
            user_agent: USER_AGENT.to_string(),
            headers,
        };
        // Every request in this crate goes through here, so a transport that
        // quotes the address it was given is cleaned in one place rather than
        // each caller having to remember.
        self.http
            .send(&request)
            .await
            .map_err(|message| Error::Transport(self.redacted(&message)))
    }

    /// Masks this client's own credentials in text on its way into an error,
    /// on top of the shape-based masking [`Error`] does whenever it is
    /// printed. A resource key that does not start `AQ`, or a licence key of
    /// any shape, is invisible to the shape rules, so the exact values are
    /// masked here where they are known.
    fn redacted(&self, text: &str) -> String {
        crate::redact::redact_with(text, &self.secrets).into_owned()
    }

    /// The error for a status this client did not expect from `endpoint`,
    /// carrying the start of the body with the credentials masked.
    fn unexpected(&self, endpoint: &'static str, response: &DidHttpResponse) -> Error {
        Error::UnexpectedStatus {
            endpoint,
            status: response.status,
            body: Error::truncate(&self.redacted(&response.body)),
        }
    }
}

/// Resolves once no key fetch is in flight. A caller that found one in
/// flight awaits this rather than making a second request.
struct FetchFinished<'a> {
    client: &'a DidClient,
}

impl Future for FetchFinished<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut cache = self.client.lock_cache();
        if !cache.fetching {
            return Poll::Ready(());
        }
        // The same caller polled again registers once.
        if !cache.waiters.iter().any(|w| w.will_wake(cx.waker())) {
            cache.waiters.push(cx.waker().clone());
        }
        Poll::Pending
    }
}

/// Clears the in-flight flag and wakes every waiter when dropped, which is
/// when the fetch that set the flag ends, however it ends.
struct FetchFinishes<'a> {
    client: &'a DidClient,
}

impl Drop for FetchFinishes<'_> {
    fn drop(&mut self) {
        let mut cache = self.client.lock_cache();
        cache.fetching = false;
        for waker in cache.waiters.drain(..) {
            waker.wake();
        }
    }
}

/// The start of the key held for the moment, being the newest held key that
/// starts at or before it, or the first key held where none does.
fn start_held_for(keys: &[DidPublicKey], at: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let starts = keys.iter().map(DidPublicKey::starts_at);
    starts
        .clone()
        .filter(|start| *start <= at)
        .max()
        .or_else(|| starts.min())
}

/// Checks the identifier's signature under the keys held for its date, best
/// first, and says why when none of them verifies it.
fn check_signature(fod_id: &FodId, keys: &[DidPublicKey], date: DateTime<Utc>) -> SignatureCheck {
    let candidates = candidates_for_date(keys, date);
    if candidates.is_empty() {
        return SignatureCheck::NoKey;
    }
    let mut unusable = false;
    for candidate in candidates {
        match fod_id.verify_with_public_key(candidate.public_key_pem()) {
            Ok(true) => return SignatureCheck::Verified,
            Ok(false) => {}
            Err(_) => unusable = true,
        }
    }
    if unusable {
        SignatureCheck::KeyUnusable
    } else {
        SignatureCheck::Invalid
    }
}

/// Refuses a string that cannot be a 51Did before any key is fetched or any
/// call is made. The length guard comes first, so that nothing is parsed for
/// a value far larger than any identifier, then the value is parsed, so that
/// a malformed one is named for what it is here rather than sent to the
/// cloud to be refused there. The parse says nothing about the signature,
/// which is the question the call is being made to answer.
fn validate_encoded_value(fod_id: &str) -> Result<()> {
    if fod_id.trim().is_empty() {
        return Err(Error::InvalidArgument("a 51Did is required".to_string()));
    }
    if fod_id.chars().count() > MAXIMUM_ENCODED_LENGTH {
        return Err(Error::InvalidArgument(
            "the value is too long to be a 51Did".to_string(),
        ));
    }
    FodId::from_base64(fod_id)
        .map(|_| ())
        .map_err(|e| Error::InvalidArgument(format!("the value is not a 51Did ({e})")))
}

/// Percent-encodes a value for a URL path segment or query value, leaving
/// only the unreserved characters of RFC 3986 as they are.
fn escape_data_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The `valid` boolean of a verify answer, or `None` when the body is not a
/// JSON object carrying one.
fn read_valid(body: &str) -> Option<bool> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .as_object()?
        .get("valid")?
        .as_bool()
}

/// The cloud's `errors` array joined into one message, or `None` when the
/// body carries none.
fn read_errors(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let errors = value.as_object()?.get("errors")?.as_array()?;
    if errors.is_empty() {
        return None;
    }
    Some(
        errors
            .iter()
            .map(|e| match e.as_str() {
                Some(text) => text.to_string(),
                None => e.to_string(),
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::sync::Mutex;

    use chrono::SubsecRound;
    use fodid::{Creator, Crypto};

    use super::*;
    use crate::http::LocalBoxFuture;
    use crate::key::BOUNDARY_TOLERANCE_MINUTES;
    use crate::outcome::ContextOutcome;

    const RESOURCE_KEY: &str = "AQS5HKcy-resource";
    const ENDPOINT: &str = "https://example.test/api/v4/";

    /// Returns pending once, waking itself, and is ready on the next poll.
    /// Every stub transport yields through this before answering, so that
    /// a second caller can reach the client while the first is still
    /// waiting on the network, which is how the shared fetch is exercised.
    struct YieldOnce(bool);

    impl Future for YieldOnce {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if self.0 {
                Poll::Ready(())
            } else {
                self.0 = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    /// Stands in for the network, recording every request and answering
    /// canned responses in order.
    #[derive(Default)]
    struct FakeHttp {
        requests: Mutex<Vec<DidHttpRequest>>,
        responses: Mutex<VecDeque<std::result::Result<DidHttpResponse, String>>>,
    }

    impl FakeHttp {
        fn answering(responses: Vec<(u16, &str)>) -> Arc<Self> {
            let fake = Self::default();
            for (status, body) in responses {
                fake.responses
                    .lock()
                    .unwrap()
                    .push_back(Ok(DidHttpResponse {
                        status,
                        body: body.to_string(),
                    }));
            }
            Arc::new(fake)
        }

        fn failing(message: &str) -> Arc<Self> {
            let fake = Self::default();
            fake.responses
                .lock()
                .unwrap()
                .push_back(Err(message.to_string()));
            Arc::new(fake)
        }

        fn requests(&self) -> Vec<DidHttpRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl DidHttpClient for FakeHttp {
        fn send<'a>(
            &'a self,
            request: &'a DidHttpRequest,
        ) -> LocalBoxFuture<'a, std::result::Result<DidHttpResponse, String>> {
            Box::pin(async move {
                YieldOnce(false).await;
                self.requests.lock().unwrap().push(request.clone());
                self.responses
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or_else(|| Err("no canned response left for this request".to_string()))
            })
        }
    }

    /// A transport whose future holds an `Rc` across an await. An `Rc`
    /// cannot cross threads, so this compiles only because the trait does
    /// not require the future to be `Send`, which is the point of the test
    /// that uses it.
    struct RcHolding {
        body: String,
    }

    impl DidHttpClient for RcHolding {
        fn send<'a>(
            &'a self,
            request: &'a DidHttpRequest,
        ) -> LocalBoxFuture<'a, std::result::Result<DidHttpResponse, String>> {
            Box::pin(async move {
                let held = Rc::new(request.url.clone());
                YieldOnce(false).await;
                assert!(held.ends_with(&escape_data_string(RESOURCE_KEY)));
                Ok(DidHttpResponse {
                    status: 200,
                    body: self.body.clone(),
                })
            })
        }
    }

    /// The payload header and match key lengths, taken from the
    /// specification at
    /// <https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md>
    /// rather than from the crate.
    ///
    /// `fodid` does not publish its offsets and lengths, because the only
    /// use a caller has for an offset is to read a field out of the payload
    /// by hand and that is how the usage comes out wrong. A test that builds
    /// a payload byte by byte needs them, and taking them from the reader
    /// would make the fixture agree with the reader whatever either said.
    const HEADER_LENGTH: usize = 5;
    const MATCH_KEY_LENGTH: usize = 32;

    /// A signing key pair standing in for the cloud's, and a 51Did it
    /// signs.
    struct Fixture {
        public_pem: String,
        creator: Creator,
        fod_id: FodId,
    }

    impl Fixture {
        fn new() -> Self {
            let crypto = Crypto::new();
            let public_pem = crypto.public_key_pem().expect("export public key");
            let creator = Creator::new("51degrees.com", crypto).expect("create creator");
            let fod_id = sign(&creator);
            Self {
                public_pem,
                creator,
                fod_id,
            }
        }

        /// Another 51Did signed now under the same key.
        fn another(&self) -> FodId {
            sign(&self.creator)
        }

        fn encoded(&self) -> String {
            self.fod_id.as_base64().expect("encode")
        }

        /// A key list as the key route gives it, being this fixture's key in
        /// force from yesterday for a week and the key before it, so an
        /// identifier created now is inside the period of the newest key.
        fn keys_json(&self) -> String {
            self.keys_json_with(&self.public_pem)
        }

        fn keys_json_with(&self, pem: &str) -> String {
            let now = whole_second_now();
            key_list(&[
                (
                    now - Duration::days(8),
                    Some(now - Duration::days(1)),
                    "earlier",
                ),
                (now - Duration::days(1), Some(now + Duration::days(6)), pem),
            ])
        }
    }

    /// A 51Did signed now. A non-marketing probabilistic identifier, whose
    /// flags byte sets usage bit 0 because a payload with no usage bit is
    /// refused.
    fn sign(creator: &Creator) -> FodId {
        let mut payload = vec![0u8; HEADER_LENGTH + MATCH_KEY_LENGTH];
        payload[0] = 0b0000_0001;
        let owid = creator.create(payload).expect("sign the envelope");
        FodId::from_owid(owid).expect("a 51Did")
    }

    /// Now, to the whole second, the form a start is sent back in as
    /// `datetime`.
    fn whole_second_now() -> DateTime<Utc> {
        Utc::now().trunc_subsecs(0)
    }

    /// A time as the key route writes it, with seven fractional digits and
    /// `Z`.
    fn cloud_time(time: DateTime<Utc>) -> String {
        format!(
            "{}.{:07}Z",
            time.format("%Y-%m-%dT%H:%M:%S"),
            time.timestamp_subsec_nanos() / 100
        )
    }

    /// One entry of a key route answer, being a start, an end where there
    /// is one, and a public key.
    type Entry<'a> = (DateTime<Utc>, Option<DateTime<Utc>>, &'a str);

    /// A key route answer holding the entries given.
    fn key_list(entries: &[Entry<'_>]) -> String {
        let entries: Vec<serde_json::Value> = entries
            .iter()
            .map(|(start, end, pem)| {
                let mut entry = serde_json::json!({
                    "startsAt": cloud_time(*start),
                    "publicKey": pem,
                });
                if let Some(end) = end {
                    entry["endsAt"] = cloud_time(*end).into();
                }
                entry
            })
            .collect();
        serde_json::Value::from(entries).to_string()
    }

    /// A start as a key fetch sends it in `datetime`.
    fn as_datetime(start: DateTime<Utc>) -> String {
        start.to_rfc3339_opts(SecondsFormat::Secs, true)
    }

    /// The `datetime` a key fetch carried, decoded, or `None` without one.
    fn datetime_of(request: &DidHttpRequest) -> Option<String> {
        let (_, value) = request.url.split_once("?datetime=")?;
        Some(value.replace("%3A", ":"))
    }

    /// A clock the test moves on by hand.
    #[derive(Clone)]
    struct TestClock(Arc<Mutex<DateTime<Utc>>>);

    impl TestClock {
        fn advance(&self, by: Duration) {
            *self.0.lock().unwrap() += by;
        }
    }

    fn new_client(http: Arc<dyn DidHttpClient>) -> DidClient {
        DidClient::builder(RESOURCE_KEY)
            .endpoint(ENDPOINT)
            .http_client(http)
            .build()
            .expect("the client builds")
    }

    fn new_client_with_licence(http: Arc<FakeHttp>) -> DidClient {
        DidClient::builder(RESOURCE_KEY)
            .endpoint(ENDPOINT)
            .licence_key("licence-value")
            .http_client(http)
            .build()
            .expect("the client builds")
    }

    /// A client whose key cache ages against a clock the test moves on.
    fn new_client_with_clock(http: Arc<FakeHttp>) -> (DidClient, TestClock) {
        let clock = TestClock(Arc::new(Mutex::new(Utc::now())));
        let reading = clock.clone();
        let client = DidClient::builder(RESOURCE_KEY)
            .endpoint(ENDPOINT)
            .http_client(http)
            .clock(move || *reading.0.lock().unwrap())
            .build()
            .expect("the client builds");
        (client, clock)
    }

    fn form_value<'a>(request: &'a DidHttpRequest, name: &str) -> Option<&'a str> {
        request
            .form
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    // Building.

    #[test]
    fn a_blank_resource_key_is_refused() {
        let error = DidClient::builder("  ")
            .endpoint(ENDPOINT)
            .http_client(FakeHttp::answering(vec![]))
            .build()
            .unwrap_err();
        assert!(matches!(error, Error::InvalidArgument(_)), "{error}");
    }

    #[test]
    fn the_endpoint_ends_in_exactly_one_slash() {
        let with_none = DidClient::builder(RESOURCE_KEY)
            .endpoint("https://example.test/api/v4")
            .http_client(FakeHttp::answering(vec![]))
            .build()
            .unwrap();
        assert_eq!(with_none.endpoint(), ENDPOINT);
        let with_two = DidClient::builder(RESOURCE_KEY)
            .endpoint(" https://example.test/api/v4// ")
            .http_client(FakeHttp::answering(vec![]))
            .build()
            .unwrap();
        assert_eq!(with_two.endpoint(), ENDPOINT);
    }

    #[test]
    fn a_relative_endpoint_is_refused() {
        let error = DidClient::builder(RESOURCE_KEY)
            .endpoint("api/v4")
            .http_client(FakeHttp::answering(vec![]))
            .build()
            .unwrap_err();
        assert!(matches!(error, Error::InvalidArgument(_)), "{error}");
    }

    #[test]
    fn the_default_endpoint_and_the_environment_variable() {
        // Only this test touches the variable, and every other test gives
        // the builder an endpoint, so nothing else reads it.
        std::env::remove_var(ENDPOINT_ENVIRONMENT_VARIABLE);
        let default = DidClient::builder(RESOURCE_KEY)
            .http_client(FakeHttp::answering(vec![]))
            .build()
            .unwrap();
        assert_eq!(default.endpoint(), DEFAULT_ENDPOINT);

        std::env::set_var(ENDPOINT_ENVIRONMENT_VARIABLE, "https://private.test/api/v4");
        let from_variable = DidClient::builder(RESOURCE_KEY)
            .http_client(FakeHttp::answering(vec![]))
            .build()
            .unwrap();
        std::env::remove_var(ENDPOINT_ENVIRONMENT_VARIABLE);
        assert_eq!(from_variable.endpoint(), "https://private.test/api/v4/");
    }

    #[test]
    fn the_endpoint_variable_carries_the_one_name() {
        // The cloud request engine of this workspace reads
        // _51DEGREES_CLOUD_ENDPOINT, and this client says it honours the same
        // variable, so the two names have to be the one name. The name is
        // written out rather than taken from the engine, because this crate
        // does not depend on it, and a test that asked this crate what it
        // expects would agree with itself whatever the name became.
        assert_eq!(
            ENDPOINT_ENVIRONMENT_VARIABLE, "_51DEGREES_CLOUD_ENDPOINT",
            "the endpoint variable has to be the one the cloud request engine reads"
        );
    }

    #[test]
    fn the_licence_key_is_held_but_never_shown() {
        let without = new_client(FakeHttp::answering(vec![]));
        assert!(!without.has_licence_key());
        let with = new_client_with_licence(FakeHttp::answering(vec![]));
        assert!(with.has_licence_key());
        let shown = format!("{with:?}");
        assert!(!shown.contains("licence-value"), "{shown}");
        assert!(shown.contains("has_licence_key: true"), "{shown}");
    }

    // Keys and the cache.

    #[tokio::test]
    async fn keys_are_fetched_from_the_key_endpoint_with_the_user_agent() {
        let fixture = Fixture::new();
        let http = FakeHttp::answering(vec![(200, &fixture.keys_json())]);
        let client = new_client(http.clone());
        let keys = client.public_keys().await.unwrap();
        assert_eq!(keys.len(), 2);
        let requests = http.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, HttpMethod::Get);
        assert_eq!(
            requests[0].url,
            format!("{ENDPOINT}id/key/{}", escape_data_string(RESOURCE_KEY))
        );
        assert!(requests[0].headers.is_empty(), "no licence key, no header");
        assert!(requests[0].form.is_empty());
        assert_eq!(requests[0].user_agent, USER_AGENT);
        assert_eq!(
            USER_AGENT,
            concat!("fodid-client/", env!("CARGO_PKG_VERSION"))
        );
    }

    #[tokio::test]
    async fn with_a_licence_key_the_keys_are_fetched_on_it_in_a_header() {
        let fixture = Fixture::new();
        let http = FakeHttp::answering(vec![(200, &fixture.keys_json())]);
        let client = new_client_with_licence(http.clone());
        assert_eq!(client.public_keys().await.unwrap().len(), 2);
        let requests = http.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method, HttpMethod::Get);
        // The bare route, with no resource key segment.
        assert_eq!(request.url, format!("{ENDPOINT}id/key"));
        assert_eq!(
            request.headers,
            vec![(LICENCE_KEY_HEADER.to_string(), "licence-value".to_string())]
        );
        assert_eq!(LICENCE_KEY_HEADER, "X-51D-License-Key");
        assert!(request.form.is_empty());
        assert_eq!(request.user_agent, USER_AGENT);
    }

    #[tokio::test]
    async fn the_licence_key_is_in_no_url_of_any_call() {
        let fixture = Fixture::new();
        let http = FakeHttp::answering(vec![
            (200, &fixture.keys_json()),
            (200, r#"{"valid":true}"#),
            (200, r#"{"context":"verified"}"#),
        ]);
        let client = new_client_with_licence(http.clone());
        assert!(client.verify_signature(&fixture.fod_id).await.unwrap());
        assert!(client.verify(&fixture.fod_id).await.unwrap());
        client
            .redeem(&fixture.fod_id, "sealed", None)
            .await
            .unwrap();
        let requests = http.requests();
        assert_eq!(requests.len(), 3, "a key fetch, a verify and a redeem");
        for request in &requests {
            assert!(
                !request.url.contains("licence-value"),
                "the licence key is in {}",
                request.url
            );
        }
        assert_eq!(requests[0].url, format!("{ENDPOINT}id/key"));
        // Verify keeps the resource key in its route and sends no header.
        assert!(requests[1].url.starts_with(&format!(
            "{ENDPOINT}id/verify/{}?",
            escape_data_string(RESOURCE_KEY)
        )));
        assert!(requests[1].headers.is_empty());
        assert!(requests[2].headers.is_empty());
    }

    #[tokio::test]
    async fn a_key_answer_other_than_200_is_unexpected() {
        let http = FakeHttp::answering(vec![(500, "down")]);
        let error = new_client(http).public_keys().await.unwrap_err();
        match error {
            Error::UnexpectedStatus {
                endpoint, status, ..
            } => {
                assert_eq!(endpoint, "key");
                assert_eq!(status, 500);
            }
            other => panic!("expected UnexpectedStatus, got {other}"),
        }
    }

    #[tokio::test]
    async fn a_transport_failure_is_reported_as_one() {
        let error = new_client(FakeHttp::failing("no route"))
            .public_keys()
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::Transport(ref m) if m == "no route"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn a_fresh_cache_inside_the_schedule_is_not_fetched_again() {
        let fixture = Fixture::new();
        let http = FakeHttp::answering(vec![(200, &fixture.keys_json())]);
        let client = new_client(http.clone());
        assert!(client
            .public_key_for(&fixture.fod_id)
            .await
            .unwrap()
            .is_some());
        assert!(client
            .public_key_for(&fixture.fod_id)
            .await
            .unwrap()
            .is_some());
        assert!(client.verify_signature(&fixture.fod_id).await.unwrap());
        assert_eq!(http.requests().len(), 1, "one fetch serves every lookup");
    }

    #[tokio::test]
    async fn a_cache_older_than_a_day_is_fetched_again() {
        let fixture = Fixture::new();
        let keys = fixture.keys_json();
        let http = FakeHttp::answering(vec![(200, &keys), (200, &keys)]);
        let now = Arc::new(Mutex::new(Utc::now()));
        let clock_now = now.clone();
        let client = DidClient::builder(RESOURCE_KEY)
            .endpoint(ENDPOINT)
            .http_client(http.clone())
            .clock(move || *clock_now.lock().unwrap())
            .build()
            .unwrap();
        client.public_key_for(&fixture.fod_id).await.unwrap();
        *now.lock().unwrap() += KEY_CACHE_LIFETIME - Duration::minutes(1);
        client.public_key_for(&fixture.fod_id).await.unwrap();
        assert_eq!(http.requests().len(), 1, "still inside the lifetime");
        *now.lock().unwrap() += Duration::minutes(2);
        client.public_key_for(&fixture.fod_id).await.unwrap();
        let requests = http.requests();
        assert_eq!(requests.len(), 2, "stale, so fetched again");
        assert_eq!(datetime_of(&requests[1]), None, "as a whole");
    }

    #[tokio::test]
    async fn the_whole_list_is_fetched_after_a_day_whatever_the_minute_limit() {
        // A list with no end, so every identifier made now sends the client
        // back for the entries from the newest start onwards. Those fetches
        // do not reset the list's age, and the fetch of the whole list once
        // it is a day old neither waits for the minute nor restarts it.
        let fixture = Fixture::new();
        let started = whole_second_now() - Duration::days(1);
        let json = key_list(&[(started, None, &fixture.public_pem)]);
        let http = FakeHttp::answering(vec![(200, json.as_str()); 4]);
        let (client, clock) = new_client_with_clock(http.clone());
        client.public_keys().await.unwrap();
        clock.advance(KEY_CACHE_LIFETIME - Duration::seconds(30));
        assert!(client.verify_signature(&fixture.another()).await.unwrap());
        clock.advance(Duration::seconds(40));
        assert!(client.verify_signature(&fixture.another()).await.unwrap());
        clock.advance(Duration::seconds(25));
        assert!(client.verify_signature(&fixture.another()).await.unwrap());
        let requests = http.requests();
        assert_eq!(requests.len(), 4);
        let sent: Vec<Option<String>> = requests.iter().map(datetime_of).collect();
        assert_eq!(
            sent,
            vec![
                None,
                Some(as_datetime(started)),
                None,
                Some(as_datetime(started)),
            ],
            "whole, from the newest start, whole 40 seconds later, and from \
             the newest start a minute after the one before"
        );
    }

    #[tokio::test]
    async fn a_date_before_every_key_held_has_no_key_without_a_fetch() {
        let fixture = Fixture::new();
        // A list that starts tomorrow holds no key for an identifier created
        // now. A fetch only brings keys that start at or after the newest
        // start held, so the client answers from what it holds rather than
        // asking again.
        let tomorrow = whole_second_now() + Duration::days(1);
        let later = key_list(&[(tomorrow, Some(tomorrow + Duration::days(7)), "x")]);
        let http = FakeHttp::answering(vec![(200, &later)]);
        let (client, clock) = new_client_with_clock(http.clone());
        assert!(client
            .public_key_for(&fixture.fod_id)
            .await
            .unwrap()
            .is_none());
        clock.advance(REFETCH_INTERVAL);
        assert!(client
            .public_key_for(&fixture.fod_id)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            client
                .verify_signature_detailed(&fixture.fod_id)
                .await
                .unwrap(),
            SignatureCheck::NoKey
        );
        assert_eq!(http.requests().len(), 1, "only the first lookup fetched");
    }

    #[tokio::test]
    async fn a_list_with_no_ends_is_fetched_again_at_most_once_a_minute() {
        let fixture = Fixture::new();
        // Only keys whose periods have started, and no ends. The list covers
        // nothing after its newest start, which has passed, so an identifier
        // created now sends the client back to the cloud, but no more than
        // once a minute. The start has a fraction of a second, which the
        // fetch sends as the whole second before it.
        let started = whole_second_now() - Duration::days(1) + Duration::milliseconds(500);
        let json = key_list(&[(started, None, &fixture.public_pem)]);
        let http = FakeHttp::answering(vec![(200, json.as_str()); 3]);
        let (client, clock) = new_client_with_clock(http.clone());
        assert!(client
            .public_key_for(&fixture.fod_id)
            .await
            .unwrap()
            .is_some());
        assert_eq!(http.requests().len(), 1, "the first fetch answers itself");
        assert!(client.verify_signature(&fixture.another()).await.unwrap());
        assert_eq!(
            http.requests().len(),
            2,
            "the first fetch does not hold back one the date needs"
        );
        assert!(client.verify_signature(&fixture.another()).await.unwrap());
        assert_eq!(http.requests().len(), 2, "not again within the minute");
        clock.advance(REFETCH_INTERVAL);
        assert!(client.verify_signature(&fixture.another()).await.unwrap());
        let requests = http.requests();
        assert_eq!(requests.len(), 3, "again once the minute had passed");
        assert_eq!(datetime_of(&requests[0]), None, "nothing was held");
        let since = Some(as_datetime(started.trunc_subsecs(0)));
        assert_eq!(datetime_of(&requests[1]), since);
        assert_eq!(datetime_of(&requests[2]), since);
    }

    #[tokio::test]
    async fn concurrent_lookups_share_one_fetch() {
        let fixture = Fixture::new();
        // One canned answer only, so a second request would fail with no
        // response left and show up as an error below.
        let http = FakeHttp::answering(vec![(200, &fixture.keys_json())]);
        let client = new_client(http.clone());
        // Both futures start before either finishes. The stub yields before
        // answering, so the second lookup finds the first one's fetch in
        // flight and waits for it rather than sending its own.
        let (first, second, third) = tokio::join!(
            client.public_keys(),
            client.public_key_for(&fixture.fod_id),
            client.verify_signature(&fixture.fod_id),
        );
        assert_eq!(first.unwrap().len(), 2);
        assert!(second.unwrap().is_some());
        assert!(third.unwrap());
        assert_eq!(http.requests().len(), 1, "one request served all three");
    }

    #[tokio::test]
    async fn a_waiter_fetches_for_itself_when_the_shared_fetch_fails() {
        let fixture = Fixture::new();
        let http = FakeHttp::answering(vec![(500, "down"), (200, &fixture.keys_json())]);
        let client = new_client(http.clone());
        let (first, second) = tokio::join!(client.public_keys(), client.public_keys());
        assert!(
            matches!(first, Err(Error::UnexpectedStatus { status: 500, .. })),
            "the caller that fetched sees the failure"
        );
        assert_eq!(
            second.unwrap().len(),
            2,
            "the caller that waited fetched again and got the keys"
        );
        assert_eq!(http.requests().len(), 2);
    }

    #[tokio::test]
    async fn a_dropped_fetch_does_not_leave_waiters_stranded() {
        let fixture = Fixture::new();
        let http = FakeHttp::answering(vec![(200, &fixture.keys_json())]);
        let client = new_client(http.clone());
        // Poll a fetch far enough to mark it in flight, then drop it before
        // it lands. The in-flight mark must go with it.
        {
            let waker = Waker::noop();
            let mut cx = Context::from_waker(waker);
            let mut fetch = Box::pin(client.public_keys());
            assert!(fetch.as_mut().poll(&mut cx).is_pending());
            assert!(client.lock_cache().fetching, "the fetch is in flight");
        }
        assert!(
            !client.lock_cache().fetching,
            "the dropped fetch cleared the mark"
        );
        // The stub recorded nothing, because the dropped future never got
        // past its first yield, so the canned answer is still there for
        // this lookup.
        assert_eq!(client.public_keys().await.unwrap().len(), 2);
        assert_eq!(http.requests().len(), 1);
    }

    #[tokio::test]
    async fn the_transport_future_need_not_be_send() {
        // The stub holds an Rc across an await inside its send future. That
        // future is not Send, and the test compiles and passes because the
        // trait never asks it to be.
        let fixture = Fixture::new();
        let http: Arc<dyn DidHttpClient> = Arc::new(RcHolding {
            body: fixture.keys_json(),
        });
        let client = new_client(http);
        assert_eq!(client.public_keys().await.unwrap().len(), 2);
        assert!(client.verify_signature(&fixture.fod_id).await.unwrap());
    }

    // Keeping the key list current.

    #[tokio::test]
    async fn identifiers_inside_the_period_held_verify_with_no_more_fetches() {
        // The newest key held ends a week from now, so every identifier made
        // now is covered, however long passes between them.
        let fixture = Fixture::new();
        let http = FakeHttp::answering(vec![(200, &fixture.keys_json())]);
        let (client, clock) = new_client_with_clock(http.clone());
        for _ in 0..5 {
            assert!(client.verify_signature(&fixture.another()).await.unwrap());
            clock.advance(REFETCH_INTERVAL);
        }
        assert_eq!(http.requests().len(), 1, "only the first fetch");
    }

    #[tokio::test]
    async fn a_date_near_the_end_held_fetches_once_for_the_new_key() {
        let old = Fixture::new();
        let new = Fixture::new();
        // The newest key held ends the boundary tolerance after the
        // identifier's date, so the key after it may be a candidate and is
        // fetched. That key signed the identifier. The answer starts at the
        // newest start held, as the key route's does for that `datetime`.
        let started = whole_second_now() - Duration::days(7);
        let tolerance = Duration::minutes(BOUNDARY_TOLERANCE_MINUTES);
        let ends = (new.fod_id.date() + tolerance).trunc_subsecs(0);
        let held = key_list(&[
            (started - Duration::days(7), Some(started), "earlier"),
            (started, Some(ends), &old.public_pem),
        ]);
        let published = key_list(&[
            (started, Some(ends), &old.public_pem),
            (ends, Some(ends + Duration::days(7)), &new.public_pem),
        ]);
        let http = FakeHttp::answering(vec![(200, &held), (200, &published)]);
        let client = new_client(http.clone());
        assert_eq!(client.public_keys().await.unwrap().len(), 2);
        // The lookup fetches before any signature is checked.
        assert!(client.public_key_for(&new.fod_id).await.unwrap().is_some());
        let requests = http.requests();
        assert_eq!(requests.len(), 2, "exactly one fetch for the identifier");
        assert_eq!(
            requests[1].url,
            format!(
                "{ENDPOINT}id/key/{}?datetime={}",
                escape_data_string(RESOURCE_KEY),
                escape_data_string(&as_datetime(started))
            ),
            "the newest start held"
        );
        assert_eq!(
            client.verify_signature_detailed(&new.fod_id).await.unwrap(),
            SignatureCheck::Verified
        );
        assert_eq!(http.requests().len(), 2, "and none for the check");
        assert_eq!(
            client.public_keys().await.unwrap().len(),
            3,
            "the answer was merged, and the earlier key kept"
        );
    }

    #[tokio::test]
    async fn a_later_licence_key_fetch_sends_the_datetime_on_the_bare_route() {
        let fixture = Fixture::new();
        let started = whole_second_now() - Duration::days(1);
        let json = key_list(&[(started, None, &fixture.public_pem)]);
        let http = FakeHttp::answering(vec![(200, &json), (200, &json)]);
        let client = new_client_with_licence(http.clone());
        client.public_keys().await.unwrap();
        assert!(client.verify_signature(&fixture.fod_id).await.unwrap());
        let requests = http.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1].url,
            format!(
                "{ENDPOINT}id/key?datetime={}",
                escape_data_string(&as_datetime(started))
            )
        );
        assert_eq!(
            requests[1].headers,
            vec![(LICENCE_KEY_HEADER.to_string(), "licence-value".to_string())]
        );
    }

    #[tokio::test]
    async fn keys_ahead_with_no_ends_cover_a_current_identifier() {
        // A list that carries keys whose periods start later, and no ends,
        // covers an identifier made now until the newest of those starts.
        let fixture = Fixture::new();
        let now = whole_second_now();
        let list = key_list(&[
            (now - Duration::days(1), None, &fixture.public_pem),
            (now + Duration::days(6), None, "next"),
            (now + Duration::days(13), None, "after"),
        ]);
        let http = FakeHttp::answering(vec![(200, &list)]);
        let (client, clock) = new_client_with_clock(http.clone());
        for _ in 0..3 {
            assert!(client.verify_signature(&fixture.another()).await.unwrap());
            clock.advance(REFETCH_INTERVAL);
        }
        assert_eq!(http.requests().len(), 1);
    }

    #[tokio::test]
    async fn dates_after_the_end_fetch_once_a_minute_and_have_no_key() {
        // The newest key held ended yesterday and no key after it is
        // published. An identifier made now is past that end, so the key
        // that signed it is not a candidate and the answer is no key rather
        // than a failed signature.
        let fixture = Fixture::new();
        let now = whole_second_now();
        let list = key_list(&[(
            now - Duration::days(8),
            Some(now - Duration::days(1)),
            &fixture.public_pem,
        )]);
        let http = FakeHttp::answering(vec![(200, &list), (200, &list), (200, &list)]);
        let (client, clock) = new_client_with_clock(http.clone());
        client.public_keys().await.unwrap();
        for fod_id in [fixture.another(), fixture.another()] {
            assert_eq!(
                client.verify_signature_detailed(&fod_id).await.unwrap(),
                SignatureCheck::NoKey
            );
            assert!(client.public_key_for(&fod_id).await.unwrap().is_none());
        }
        assert_eq!(http.requests().len(), 2, "one fetch within the minute");
        clock.advance(REFETCH_INTERVAL);
        assert_eq!(
            client
                .verify_signature_detailed(&fixture.fod_id)
                .await
                .unwrap(),
            SignatureCheck::NoKey
        );
        assert_eq!(
            http.requests().len(),
            3,
            "and one more once the minute had passed"
        );
    }

    #[tokio::test]
    async fn a_later_answer_with_an_end_replaces_the_entry_held_without_one() {
        let fixture = Fixture::new();
        let now = whole_second_now();
        let started = now - Duration::days(1);
        let ends = now + Duration::days(6);
        let without = key_list(&[(started, None, &fixture.public_pem)]);
        let with = key_list(&[(started, Some(ends), &fixture.public_pem)]);
        let http = FakeHttp::answering(vec![(200, &without), (200, &with)]);
        let (client, clock) = new_client_with_clock(http.clone());
        assert_eq!(client.public_keys().await.unwrap()[0].ends_at(), None);
        // The newest start has passed and the list has no end, so the
        // identifier is not covered and the list is fetched again.
        assert!(client.verify_signature(&fixture.fod_id).await.unwrap());
        let keys = client.public_keys().await.unwrap();
        assert_eq!(keys.len(), 1, "replaced rather than added");
        assert_eq!(keys[0].ends_at(), Some(ends));
        clock.advance(REFETCH_INTERVAL);
        assert!(client.verify_signature(&fixture.another()).await.unwrap());
        assert_eq!(
            http.requests().len(),
            2,
            "the end now covers the identifier"
        );
    }

    #[tokio::test]
    async fn a_key_replaced_before_its_end_is_picked_up_on_the_first_failure() {
        let old = Fixture::new();
        let new = Fixture::new();
        let now = whole_second_now();
        let started = now - Duration::days(1);
        // The next key is held too, published in the short window before it
        // starts, so it is the newest start held.
        let next = now + Duration::minutes(10);
        let next_pem = Crypto::new().public_key_pem().unwrap();
        let ends = next + Duration::days(7);
        // The old key was replaced an hour ago. The answer gives it that
        // earlier end and adds the replacement, which starts at that moment.
        let replaced = now - Duration::hours(1);
        let held = key_list(&[
            (started, Some(next), &old.public_pem),
            (next, Some(ends), &next_pem),
        ]);
        let answer = key_list(&[
            (started, Some(replaced), &old.public_pem),
            (replaced, Some(next), &new.public_pem),
            (next, Some(ends), &next_pem),
        ]);
        let http = FakeHttp::answering(vec![(200, &held), (200, &answer)]);
        let client = new_client(http.clone());
        client.public_keys().await.unwrap();
        // Signed with the replacement after it started, so the keys held
        // fail it, and the one fetch that follows brings the replacement.
        assert_eq!(
            client.verify_signature_detailed(&new.fod_id).await.unwrap(),
            SignatureCheck::Verified
        );
        let requests = http.requests();
        assert_eq!(requests.len(), 2, "exactly one fetch");
        assert_eq!(
            datetime_of(&requests[1]),
            Some(as_datetime(started)),
            "the start of the key held for the date, not the newest start"
        );
        // Signed with the replaced key after the replacement started, which
        // the merged list refuses, with no further fetch inside the minute.
        assert_eq!(
            client
                .verify_signature_detailed(&old.another())
                .await
                .unwrap(),
            SignatureCheck::Invalid
        );
        assert_eq!(http.requests().len(), 2);
    }

    #[tokio::test]
    async fn concurrent_identifiers_the_list_does_not_cover_share_one_fetch() {
        let old = Fixture::new();
        let new = Fixture::new();
        // The held key ended five minutes ago, and the answer adds the key
        // that started then, which signed both identifiers.
        let now = whole_second_now();
        let started = now - Duration::days(7);
        let boundary = now - Duration::minutes(5);
        let held = key_list(&[(started, Some(boundary), &old.public_pem)]);
        let published = key_list(&[
            (started, Some(boundary), &old.public_pem),
            (
                boundary,
                Some(boundary + Duration::days(7)),
                &new.public_pem,
            ),
        ]);
        // Two answers only, so a third request would fail and show below.
        let http = FakeHttp::answering(vec![(200, &held), (200, &published)]);
        let client = new_client(http.clone());
        client.public_keys().await.unwrap();
        let second_fod_id = new.another();
        let (first, second) = tokio::join!(
            client.verify_signature(&new.fod_id),
            client.verify_signature(&second_fod_id),
        );
        assert!(first.unwrap());
        assert!(
            second.unwrap(),
            "the second waited for the first one's fetch"
        );
        assert_eq!(http.requests().len(), 2, "one fetch served both");
    }

    #[tokio::test]
    async fn a_clock_set_back_does_not_stop_fetching() {
        let fixture = Fixture::new();
        let started = whole_second_now() - Duration::days(1);
        let json = key_list(&[(started, None, &fixture.public_pem)]);
        let http = FakeHttp::answering(vec![(200, json.as_str()); 3]);
        let (client, clock) = new_client_with_clock(http.clone());
        client.public_keys().await.unwrap();
        assert!(client.verify_signature(&fixture.fod_id).await.unwrap());
        assert_eq!(http.requests().len(), 2);
        clock.advance(-Duration::minutes(10));
        assert!(client.verify_signature(&fixture.fod_id).await.unwrap());
        assert_eq!(
            http.requests().len(),
            3,
            "only a fetch in the past holds the next one back"
        );
    }

    #[tokio::test]
    async fn fetching_from_a_cutoff_sends_it_and_returns_the_answer_unmerged() {
        let fixture = Fixture::new();
        let now = whole_second_now();
        let started = now - Duration::days(1);
        let next = now + Duration::minutes(10);
        let answer = key_list(&[
            (started, Some(next), &fixture.public_pem),
            (next, Some(next + Duration::days(7)), "next"),
        ]);
        let http = FakeHttp::answering(vec![
            (200, &fixture.keys_json()),
            (200, &answer),
            (200, &answer),
        ]);
        let client = new_client(http.clone());
        let held = client.public_keys().await.unwrap();
        // A cutoff with a fraction of a second is sent as the whole second
        // at or before it.
        let since = started + Duration::milliseconds(500);
        let fetched = client.fetch_keys_from(Some(since)).await.unwrap();
        assert_eq!(
            fetched,
            parse_keys(&answer).unwrap(),
            "the answer alone, without the earlier key the client holds"
        );
        assert_eq!(
            client.public_keys().await.unwrap(),
            held,
            "the keys the client holds are left as they are"
        );
        let requests = http.requests();
        assert_eq!(
            requests[1].url,
            format!(
                "{ENDPOINT}id/key/{}?datetime={}",
                escape_data_string(RESOURCE_KEY),
                escape_data_string(&as_datetime(started))
            )
        );
        client.fetch_keys_from(None).await.unwrap();
        let requests = http.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(datetime_of(&requests[2]), None, "no cutoff, the whole list");
    }

    #[tokio::test]
    async fn fetching_from_a_cutoff_on_a_licence_key_reads_the_answer_strictly() {
        let now = whole_second_now();
        let unreadable = key_list(&[(now, Some(now), "no period")]);
        let http = FakeHttp::answering(vec![(200, &unreadable)]);
        let client = new_client_with_licence(http.clone());
        let error = client.fetch_keys_from(Some(now)).await.unwrap_err();
        assert!(matches!(error, Error::Protocol(_)), "{error}");
        let requests = http.requests();
        assert_eq!(
            requests[0].url,
            format!(
                "{ENDPOINT}id/key?datetime={}",
                escape_data_string(&as_datetime(now))
            )
        );
        assert_eq!(
            requests[0].headers,
            vec![(LICENCE_KEY_HEADER.to_string(), "licence-value".to_string())]
        );
        assert!(client.lock_cache().keys.is_none(), "nothing was held");
    }

    #[tokio::test]
    async fn an_answer_with_an_end_not_after_its_start_merges_nothing() {
        let fixture = Fixture::new();
        let now = whole_second_now();
        let started = now - Duration::days(1);
        let held = key_list(&[(started, None, &fixture.public_pem)]);
        let unreadable = key_list(&[
            (started, Some(now + Duration::days(6)), &fixture.public_pem),
            (now, Some(now), "no period"),
        ]);
        let http = FakeHttp::answering(vec![(200, &held), (200, &unreadable)]);
        let client = new_client(http.clone());
        client.public_keys().await.unwrap();
        let error = client.public_key_for(&fixture.fod_id).await.unwrap_err();
        assert!(matches!(error, Error::Protocol(_)), "{error}");
        assert_eq!(
            client.public_keys().await.unwrap(),
            vec![DidPublicKey::new(started, fixture.public_pem.clone())],
            "nothing from the answer was merged, the valid entry included"
        );
    }

    // Offline signature checking.

    #[tokio::test]
    async fn a_genuine_signature_verifies_under_the_key_in_force() {
        let fixture = Fixture::new();
        let client = new_client(FakeHttp::answering(vec![(200, &fixture.keys_json())]));
        assert_eq!(
            client
                .verify_signature_detailed(&fixture.fod_id)
                .await
                .unwrap(),
            SignatureCheck::Verified
        );
        assert!(client.verify_signature(&fixture.fod_id).await.unwrap());
    }

    #[tokio::test]
    async fn a_signature_under_another_key_is_invalid() {
        let fixture = Fixture::new();
        let other = Crypto::new().public_key_pem().unwrap();
        let keys = fixture.keys_json_with(&other);
        let http = FakeHttp::answering(vec![(200, &keys), (200, &keys)]);
        let client = new_client(http.clone());
        assert_eq!(
            client
                .verify_signature_detailed(&fixture.fod_id)
                .await
                .unwrap(),
            SignatureCheck::Invalid
        );
        assert_eq!(
            http.requests().len(),
            1,
            "a list fetched for the check is not fetched again for it"
        );
        assert!(!client.verify_signature(&fixture.fod_id).await.unwrap());
        assert_eq!(
            http.requests().len(),
            2,
            "fetched once more in case the key was replaced"
        );
        assert!(!client.verify_signature(&fixture.fod_id).await.unwrap());
        assert_eq!(http.requests().len(), 2, "not again within the minute");
    }

    #[tokio::test]
    async fn a_key_that_cannot_be_read_is_unusable_not_invalid() {
        let fixture = Fixture::new();
        let client = new_client(FakeHttp::answering(vec![(
            200,
            &fixture.keys_json_with("not a PEM"),
        )]));
        assert_eq!(
            client
                .verify_signature_detailed(&fixture.fod_id)
                .await
                .unwrap(),
            SignatureCheck::KeyUnusable
        );
    }

    // The online verify call.

    #[tokio::test]
    async fn verify_gets_the_verify_route_with_both_parameter_names() {
        let fixture = Fixture::new();
        let http = FakeHttp::answering(vec![(200, r#"{"valid":true}"#)]);
        let client = new_client(http.clone());
        assert!(client.verify(&fixture.fod_id).await.unwrap());
        let requests = http.requests();
        assert_eq!(requests.len(), 1);
        let encoded = escape_data_string(&fixture.encoded());
        assert_eq!(requests[0].method, HttpMethod::Get);
        assert_eq!(
            requests[0].url,
            format!(
                "{ENDPOINT}id/verify/{}?51did={encoded}&owid={encoded}",
                escape_data_string(RESOURCE_KEY)
            )
        );
        assert!(
            !requests[0].url.contains('+') && !requests[0].url.contains("/?"),
            "the base64 is percent-encoded: {}",
            requests[0].url
        );
        assert!(requests[0].form.is_empty());
        assert_eq!(requests[0].user_agent, USER_AGENT);
    }

    #[tokio::test]
    async fn verify_reads_a_false_answer() {
        let fixture = Fixture::new();
        let client = new_client(FakeHttp::answering(vec![(200, r#"{"valid":false}"#)]));
        assert!(!client.verify_encoded(&fixture.encoded()).await.unwrap());
    }

    #[tokio::test]
    async fn verify_reports_the_service_errors_on_400() {
        let fixture = Fixture::new();
        let client = new_client(FakeHttp::answering(vec![(
            400,
            r#"{"errors":["first problem","second problem"]}"#,
        )]));
        let error = client.verify_encoded(&fixture.encoded()).await.unwrap_err();
        assert!(
            matches!(error, Error::InvalidArgument(ref m) if m == "first problem second problem"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn verify_treats_any_other_answer_as_unexpected() {
        let fixture = Fixture::new();
        let client = new_client(FakeHttp::answering(vec![(500, "oops")]));
        let error = client.verify(&fixture.fod_id).await.unwrap_err();
        assert!(
            matches!(
                error,
                Error::UnexpectedStatus {
                    endpoint: "verify",
                    status: 500,
                    ..
                }
            ),
            "{error}"
        );
        let client = new_client_with_licence(FakeHttp::answering(vec![(200, "not json")]));
        let error = client.verify(&fixture.fod_id).await.unwrap_err();
        assert!(matches!(error, Error::UnexpectedStatus { .. }), "{error}");
    }

    #[tokio::test]
    async fn a_value_that_is_not_a_51did_is_refused_before_any_call() {
        let http = FakeHttp::answering(vec![]);
        let client = new_client(http.clone());
        for value in ["", "   ", "not base 64!", "AAAA"] {
            let error = client.verify_encoded(value).await.unwrap_err();
            assert!(
                matches!(error, Error::InvalidArgument(_)),
                "{value:?}: {error}"
            );
            let error = client
                .redeem_encoded(value, "sealed", None)
                .await
                .unwrap_err();
            assert!(
                matches!(error, Error::InvalidArgument(_)),
                "{value:?}: {error}"
            );
        }
        let too_long = "A".repeat(MAXIMUM_ENCODED_LENGTH + 1);
        let error = client.verify_encoded(&too_long).await.unwrap_err();
        assert!(
            matches!(error, Error::InvalidArgument(ref m) if m.contains("too long")),
            "{error}"
        );
        assert!(http.requests().is_empty(), "nothing was sent");
    }

    // Redeem.

    #[tokio::test]
    async fn redeem_posts_the_form_without_a_licence_field_when_none_was_given() {
        let fixture = Fixture::new();
        let http = FakeHttp::answering(vec![(
            200,
            r#"{"context":"verified","signature":"verified"}"#,
        )]);
        let client = new_client(http.clone());
        let result = client
            .redeem(&fixture.fod_id, "sealed", None)
            .await
            .unwrap();
        assert_eq!(result.context(), ContextOutcome::Verified);
        let requests = http.requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.url, format!("{ENDPOINT}id/redeem"));
        assert!(!request.url.contains('?'), "no credential in the URL");
        assert_eq!(form_value(request, "resource"), Some(RESOURCE_KEY));
        assert_eq!(
            form_value(request, "51did"),
            Some(fixture.encoded().as_str())
        );
        assert_eq!(form_value(request, "result"), Some("sealed"));
        assert_eq!(form_value(request, "challenge"), Some(""));
        assert!(
            form_value(request, "license").is_none(),
            "no licence key, no field"
        );
        assert_eq!(request.form.len(), 4);
        assert!(request.headers.is_empty());
        assert_eq!(request.user_agent, USER_AGENT);
    }

    #[tokio::test]
    async fn redeem_carries_the_licence_key_and_challenge_in_the_form_only() {
        let fixture = Fixture::new();
        let http = FakeHttp::answering(vec![(200, r#"{"context":"verified"}"#)]);
        let client = new_client_with_licence(http.clone());
        client
            .redeem_encoded(&fixture.encoded(), "sealed", Some("nonce-1"))
            .await
            .unwrap();
        let requests = http.requests();
        let request = &requests[0];
        assert_eq!(form_value(request, "license"), Some("licence-value"));
        assert_eq!(form_value(request, "challenge"), Some("nonce-1"));
        assert_eq!(request.form.len(), 5);
        assert!(!request.url.contains("licence-value"));
        assert!(
            request.headers.is_empty(),
            "redeem sends the licence key in the form and no header"
        );
    }

    #[tokio::test]
    async fn redeem_maps_a_mismatch_and_a_misconfigured_factor() {
        let fixture = Fixture::new();
        let client = new_client(FakeHttp::answering(vec![(
            200,
            r#"{"context":"mismatch","signature":"verified",
                "factors":{"device":"mismatch","asn":"misconfigured"}}"#,
        )]));
        let result = client
            .redeem(&fixture.fod_id, "sealed", None)
            .await
            .unwrap();
        assert_eq!(result.context(), ContextOutcome::Mismatch);
        let factors = result.factors().unwrap();
        assert_eq!(factors["device"], crate::FactorOutcome::Mismatch);
        assert_eq!(factors["asn"], crate::FactorOutcome::Misconfigured);
    }

    #[tokio::test]
    async fn redeem_reads_503_as_unconfirmed() {
        let fixture = Fixture::new();
        let client = new_client(FakeHttp::answering(vec![(503, "")]));
        let result = client
            .redeem(&fixture.fod_id, "sealed", None)
            .await
            .unwrap();
        assert_eq!(result.context(), ContextOutcome::Unconfirmed);
        assert_eq!(result.status(), 503);
    }

    #[tokio::test]
    async fn redeem_reports_the_service_errors_on_400() {
        let fixture = Fixture::new();
        let client = new_client(FakeHttp::answering(vec![(
            400,
            r#"{"errors":["bad 51did"]}"#,
        )]));
        let error = client
            .redeem(&fixture.fod_id, "sealed", None)
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::InvalidArgument(ref m) if m == "bad 51did"),
            "{error}"
        );
        // A 400 with no errors array carries the body as the message.
        let client = new_client(FakeHttp::answering(vec![(400, "plain refusal")]));
        let error = client
            .redeem(&fixture.fod_id, "sealed", None)
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::InvalidArgument(ref m) if m == "plain refusal"),
            "{error}"
        );
    }

    #[tokio::test]
    async fn redeem_reports_404_as_not_supported() {
        let fixture = Fixture::new();
        let client = new_client(FakeHttp::answering(vec![(404, "")]));
        let error = client
            .redeem(&fixture.fod_id, "sealed", None)
            .await
            .unwrap_err();
        assert!(
            matches!(error, Error::NotSupported(ref e) if e == ENDPOINT),
            "{error}"
        );
    }

    #[tokio::test]
    async fn redeem_treats_any_other_status_as_unexpected() {
        let fixture = Fixture::new();
        let client = new_client(FakeHttp::answering(vec![(502, "gateway")]));
        let error = client
            .redeem(&fixture.fod_id, "sealed", None)
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                Error::UnexpectedStatus {
                    endpoint: "redeem",
                    status: 502,
                    ..
                }
            ),
            "{error}"
        );
    }

    #[tokio::test]
    async fn redeem_reports_a_transport_failure() {
        let fixture = Fixture::new();
        let client = new_client(FakeHttp::failing("timed out"));
        let error = client
            .redeem(&fixture.fod_id, "sealed", None)
            .await
            .unwrap_err();
        assert!(matches!(error, Error::Transport(_)), "{error}");
    }

    // Helpers.

    #[test]
    fn escaping_leaves_only_the_unreserved_characters() {
        assert_eq!(escape_data_string("AZaz09-_.~"), "AZaz09-_.~");
        assert_eq!(escape_data_string("a+b/c=d e&f"), "a%2Bb%2Fc%3Dd%20e%26f");
        assert_eq!(escape_data_string("é"), "%C3%A9");
    }

    #[test]
    fn errors_are_joined_and_non_strings_kept_as_json() {
        assert_eq!(
            read_errors(r#"{"errors":["a",{"code":1}]}"#).as_deref(),
            Some(r#"a {"code":1}"#)
        );
        assert!(read_errors(r#"{"errors":[]}"#).is_none());
        assert!(read_errors(r#"{"other":1}"#).is_none());
        assert!(read_errors("nope").is_none());
    }

    #[test]
    fn the_client_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DidClient>();
    }
}
