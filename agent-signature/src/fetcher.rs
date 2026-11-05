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

//! Fetches key directories, agent cards and registries, checking every
//! address before it is fetched and every response before it is used.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, TimeDelta, Utc};
use ring::digest;
use url::{Host, Url};

use crate::constants::{
    AGENT_TYPE_CIMD, AGENT_TYPE_JWKS_URI, DIRECTORY_MEDIA_TYPE, JSON_MEDIA_TYPE,
    REGISTRY_MEDIA_TYPE, TAG_DIRECTORY,
};
use crate::http::{HttpClient, HttpRequest, HttpResponse};
use crate::keys::{AgentCard, KeyDirectory};
use crate::signature_base::{
    ascii_bytes, build_authority, build_signature_base, ComponentResolver,
};
use crate::signature_input::SignatureCandidate;
use crate::structured_field::{parse_dictionary, BareItem, Item, Member};
use crate::verify::{resolve_algorithm, verify};
use crate::Clock;

/// The status a directory, card or registry response must have.
const STATUS_OK: u16 = 200;

/// The content digest member this element checks.
const DIGEST_SHA_256: &str = "sha-256";

/// What one fetch of a key directory found.
#[derive(Debug)]
pub(crate) struct DirectoryEntry {
    /// The directory, present when the fetch succeeded.
    pub(crate) directory: Option<KeyDirectory>,
    /// The agent card the keys came from, for a member of type `cimd`.
    pub(crate) card: Option<AgentCard>,
    /// When the fetch finished, which the cache ages the entry from.
    pub(crate) fetched_at: DateTime<Utc>,
    /// Why the fetch failed.
    pub(crate) failure_reason: Option<String>,
    /// The `max-age` the response carried.
    pub(crate) max_age: Option<TimeDelta>,
}

impl DirectoryEntry {
    pub(crate) fn succeeded(
        directory: KeyDirectory,
        card: Option<AgentCard>,
        fetched_at: DateTime<Utc>,
        max_age: Option<TimeDelta>,
    ) -> Self {
        DirectoryEntry {
            directory: Some(directory),
            card,
            fetched_at,
            failure_reason: None,
            max_age,
        }
    }

    pub(crate) fn failed(fetched_at: DateTime<Utc>, reason: impl Into<String>) -> Self {
        DirectoryEntry {
            directory: None,
            card: None,
            fetched_at,
            failure_reason: Some(reason.into()),
            max_age: None,
        }
    }

    /// True when the directory was read.
    pub(crate) fn is_success(&self) -> bool {
        self.directory.is_some()
    }
}

/// Fetches and reads the documents an agent publishes.
pub(crate) struct Fetcher {
    client: Arc<dyn HttpClient>,
    clock: Clock,
    user_agent: String,
    max_response_bytes: usize,
}

impl Fetcher {
    pub(crate) fn new(
        client: Arc<dyn HttpClient>,
        clock: Clock,
        max_response_bytes: usize,
    ) -> Self {
        Fetcher {
            client,
            clock,
            user_agent: format!(
                "51Degrees-Pipeline-AgentSignature/{}",
                env!("CARGO_PKG_VERSION")
            ),
            max_response_bytes,
        }
    }

    /// Fetch the keys at `url` for a member of type `agent_type`, abandoning
    /// the fetch at `deadline`. Never panics on what the response holds, and
    /// gives a failed entry for anything that goes wrong.
    pub(crate) fn fetch(&self, url: &str, agent_type: &str, deadline: Instant) -> DirectoryEntry {
        if !is_safe_url(url) {
            return self.failed(url, "the address may not be fetched");
        }
        if agent_type == AGENT_TYPE_CIMD {
            return self.fetch_card_and_keys(url, deadline);
        }
        self.fetch_directory(url, agent_type, None, deadline)
    }

    /// Read a directory the agent carried inline in a `data:` URI.
    pub(crate) fn read_inline(&self, url: &str, content: &[u8]) -> DirectoryEntry {
        let json = String::from_utf8_lossy(content);
        match KeyDirectory::parse_text(&json) {
            Some(directory) => {
                self.report_millisecond_times(url, &directory);
                DirectoryEntry::succeeded(directory, None, (self.clock)(), None)
            }
            None => self.failed(url, "the directory could not be read"),
        }
    }

    /// Fetch an agent card, or `None` where it cannot be read.
    pub(crate) fn fetch_card(&self, url: &str, deadline: Instant) -> Option<AgentCard> {
        let response = match self.send(url, JSON_MEDIA_TYPE, deadline) {
            Ok(response) => response,
            Err(reason) => {
                log_card_failure(url, &reason);
                return None;
            }
        };
        if response.status != STATUS_OK {
            log_card_failure(url, &format!("the response status was {}", response.status));
            return None;
        }
        // The registry draft says a card is returned directly with a 200, so
        // a card reached through a redirect is not the card asked for.
        if was_redirected(&response, url) {
            log_card_failure(url, "the request was redirected");
            return None;
        }
        let Some(body) = self.read_body(&response) else {
            log_card_failure(url, "the card was too long to read");
            return None;
        };
        let card = AgentCard::parse_text(&String::from_utf8_lossy(body), url);
        if card.is_none() {
            log_card_failure(url, "the card could not be read");
        }
        card
    }

    /// Fetch a registry of agent cards and return the card addresses it
    /// lists that pass the address checks.
    pub(crate) fn fetch_registry(&self, url: &str, deadline: Instant) -> Vec<String> {
        let response = match self.send(url, REGISTRY_MEDIA_TYPE, deadline) {
            Ok(response) => response,
            Err(reason) => {
                log_registry_failure(url, &reason);
                return Vec::new();
            }
        };
        if response.status != STATUS_OK {
            log_registry_failure(url, &format!("the response status was {}", response.status));
            return Vec::new();
        }
        match self.read_body(&response) {
            Some(body) => parse_registry(&String::from_utf8_lossy(body)),
            None => {
                log_registry_failure(url, "the registry was too long to read");
                Vec::new()
            }
        }
    }

    fn fetch_card_and_keys(&self, url: &str, deadline: Instant) -> DirectoryEntry {
        let Some(card) = self.fetch_card(url, deadline) else {
            return self.failed(url, "the agent card could not be read");
        };
        if let Some(jwks) = card.jwks.clone() {
            return DirectoryEntry::succeeded(jwks, Some(card), (self.clock)(), None);
        }
        let Some(jwks_uri) = card.jwks_uri.clone().filter(|uri| !uri.is_empty()) else {
            return self.failed(url, "the agent card names no keys");
        };
        // The card was fetched because of a header the sender wrote, so the
        // address it names is checked as the header's own is.
        if !is_safe_url(&jwks_uri) {
            return self.failed(
                url,
                "the agent card names an address that may not be fetched",
            );
        }
        self.fetch_directory(&jwks_uri, AGENT_TYPE_JWKS_URI, Some(card), deadline)
    }

    fn fetch_directory(
        &self,
        url: &str,
        agent_type: &str,
        card: Option<AgentCard>,
        deadline: Instant,
    ) -> DirectoryEntry {
        let response = match self.send(url, DIRECTORY_MEDIA_TYPE, deadline) {
            Ok(response) => response,
            Err(reason) => return self.failed(url, reason),
        };
        if response.status != STATUS_OK {
            return self.failed(url, format!("the response status was {}", response.status));
        }
        // The well known path is what ties a key set to a domain, so a
        // response from anywhere else is not the document asked for.
        if was_redirected(&response, url) {
            return self.failed(url, "the request was redirected");
        }
        let media_type = media_type(&response);
        if !is_acceptable_media_type(media_type.as_deref(), agent_type) {
            return self.failed(
                url,
                format!(
                    "the media type was '{}'",
                    media_type.as_deref().unwrap_or("absent")
                ),
            );
        }
        let Some(body) = self.read_body(&response) else {
            return self.failed(url, "the document was too long to read");
        };
        let Some(directory) = KeyDirectory::parse_text(&String::from_utf8_lossy(body)) else {
            return self.failed(url, "the directory could not be read");
        };
        self.report_millisecond_times(url, &directory);
        if let Err(reason) = verify_response_signature(url, &response, body, &directory) {
            return self.failed(url, reason);
        }
        DirectoryEntry::succeeded(directory, card, (self.clock)(), max_age(&response))
    }

    fn send(&self, url: &str, accept: &str, deadline: Instant) -> Result<HttpResponse, String> {
        let timeout = deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            return Err("the fetch timed out".to_owned());
        }
        self.client.get(&HttpRequest {
            url,
            accept,
            user_agent: &self.user_agent,
            timeout,
            max_response_bytes: self.max_response_bytes,
        })
    }

    /// The body, or `None` where it is longer than the limit, whether the
    /// response declared the length or not.
    fn read_body<'a>(&self, response: &'a HttpResponse) -> Option<&'a [u8]> {
        let declared = response
            .header("content-length")
            .and_then(|length| length.trim().parse::<u64>().ok());
        let limit = u64::try_from(self.max_response_bytes).unwrap_or(u64::MAX);
        if declared.is_some_and(|length| length > limit)
            || response.body.len() > self.max_response_bytes
        {
            return None;
        }
        Some(&response.body)
    }

    fn failed(&self, url: &str, reason: impl Into<String>) -> DirectoryEntry {
        let reason = reason.into();
        log::warn!(
            "The key directory at '{url}' could not be read because {reason}. The request \
             reports the Unverified status, which is not evidence against the agent."
        );
        DirectoryEntry::failed((self.clock)(), reason)
    }

    fn report_millisecond_times(&self, url: &str, directory: &KeyDirectory) {
        if directory.times_were_in_milliseconds {
            log::debug!(
                "The key directory at '{url}' carried key times in milliseconds where the \
                 standard uses seconds, so the times were divided by 1,000."
            );
        }
    }
}

/// The card addresses a registry lists, one to a line, where a `#` starts a
/// comment and blank lines are skipped. A line that fails the address checks
/// is dropped, because the lines are whatever the registry served.
pub(crate) fn parse_registry(text: &str) -> Vec<String> {
    text.split('\n')
        .map(|line| line.split('#').next().unwrap_or_default().trim())
        .filter(|line| !line.is_empty() && is_safe_url(line))
        .map(str::to_owned)
        .collect()
}

/// Whether an address may be fetched. It must be HTTPS with no user
/// information, and a host written as an IP address must not be in a range
/// that only appears inside a network, because a sender-chosen address in one
/// is how a server is made to fetch its own internal services.
pub(crate) fn is_safe_url(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        return false;
    }
    match parsed.host() {
        Some(Host::Domain(_)) => true,
        Some(Host::Ipv4(address)) => !is_private_ipv4(address),
        Some(Host::Ipv6(address)) => !is_private_ipv6(address),
        None => false,
    }
}

/// Loopback, private, link local, carrier grade and the unspecified address.
fn is_private_ipv4(address: Ipv4Addr) -> bool {
    let [first, second, ..] = address.octets();
    address.is_unspecified()
        || first == 10
        || first == 127
        || (first == 172 && (16..=31).contains(&second))
        || (first == 192 && second == 168)
        || (first == 169 && second == 254)
        || (first == 100 && (64..=127).contains(&second))
}

/// Loopback, the unspecified address, link local, site local, unique local,
/// and an IPv4 address mapped into IPv6, which is checked as the IPv4
/// address it carries.
fn is_private_ipv6(address: Ipv6Addr) -> bool {
    let first = address.segments()[0];
    if address.is_loopback() || address.is_unspecified() {
        return true;
    }
    // fe80::/10 is link local, fec0::/10 is site local and fc00::/7 is held
    // back for use inside one network.
    if (first & 0xffc0) == 0xfe80 || (first & 0xffc0) == 0xfec0 || (first & 0xfe00) == 0xfc00 {
        return true;
    }
    address.to_ipv4_mapped().is_some_and(is_private_ipv4)
}

/// Whether the response came from an address other than the one asked for.
/// An address that is empty or cannot be read counts as another address, so
/// a client that does not say where its answer came from is never trusted.
fn was_redirected(response: &HttpResponse, url: &str) -> bool {
    match (Url::parse(url), Url::parse(&response.url)) {
        (Ok(mut asked), Ok(mut answered)) => {
            asked.set_fragment(None);
            answered.set_fragment(None);
            asked != answered
        }
        _ => true,
    }
}

/// The media type of the response, without its parameters.
fn media_type(response: &HttpResponse) -> Option<String> {
    response.header("content-type").map(|value| {
        value
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned()
    })
}

/// A directory must carry the directory media type. A set of keys named
/// directly is an ordinary JSON document, so plain JSON is accepted for it.
fn is_acceptable_media_type(media_type: Option<&str>, agent_type: &str) -> bool {
    let Some(media_type) = media_type else {
        return false;
    };
    media_type.eq_ignore_ascii_case(DIRECTORY_MEDIA_TYPE)
        || (agent_type == AGENT_TYPE_JWKS_URI && media_type.eq_ignore_ascii_case(JSON_MEDIA_TYPE))
}

/// The `max-age` of a `Cache-Control` header.
fn max_age(response: &HttpResponse) -> Option<TimeDelta> {
    response
        .header("cache-control")?
        .split(',')
        .filter_map(|directive| {
            let (name, value) = directive.trim().split_once('=')?;
            if !name.trim().eq_ignore_ascii_case("max-age") {
                return None;
            }
            value.trim().trim_matches('"').parse::<u32>().ok()
        })
        .next()
        .and_then(|seconds| TimeDelta::try_seconds(i64::from(seconds)))
}

/// Check the signature a key directory response carries under the directory
/// tag, with the key in the body its `keyid` names. A response with no
/// signature headers is accepted, because the directory draft only
/// recommends them, and one signed under another tag says nothing about the
/// directory.
fn verify_response_signature(
    url: &str,
    response: &HttpResponse,
    body: &[u8],
    directory: &KeyDirectory,
) -> Result<(), String> {
    let signature_header = response.header("signature");
    let input_header = response.header("signature-input");
    let (signature_header, input_header) = match (signature_header, input_header) {
        (None, None) => return Ok(()),
        (Some(signature), Some(input)) => (signature, input),
        _ => return Err("the response signature headers were incomplete".to_owned()),
    };
    let candidates = parse_dictionary(&input_header)
        .zip(parse_dictionary(&signature_header))
        .and_then(|(input, signature)| SignatureCandidate::build_all(&input, &signature))
        .ok_or_else(|| "the response signature headers could not be read".to_owned())?;
    let Some(candidate) = candidates
        .iter()
        .find(|candidate| candidate.tag.as_deref() == Some(TAG_DIRECTORY))
    else {
        return Ok(());
    };
    if let Some(digest_header) = response.header("content-digest") {
        if !digest_matches(&digest_header, body) {
            return Err("the content digest did not match the body".to_owned());
        }
    }
    let resolver = ResponseResolver {
        authority: request_authority(url),
        response,
    };
    let base = build_signature_base(
        &candidate.covered_components,
        &candidate.signature_params,
        &resolver,
    )
    .ok_or_else(|| {
        "a covered component of the response signature could not be rebuilt".to_owned()
    })?;
    let key = directory
        .find_key(candidate.key_id.as_deref().unwrap_or_default())
        .ok_or_else(|| {
            "the response signature named a key the directory does not hold".to_owned()
        })?;
    let algorithm = resolve_algorithm(key, candidate.algorithm.as_deref());
    if !algorithm.supported
        || !verify(
            &algorithm.name,
            key,
            &ascii_bytes(&base),
            &candidate.signature,
        )
    {
        return Err("the response signature did not check out".to_owned());
    }
    Ok(())
}

/// Whether a `Content-Digest` header's `sha-256` member matches the body.
/// Only SHA-256 is checked, because that is what the directory draft uses,
/// and a digest of any other kind is not evidence against the response.
fn digest_matches(header: &str, body: &[u8]) -> bool {
    let Some(dictionary) = parse_dictionary(header) else {
        return false;
    };
    let Some(Member::Item(Item {
        value: BareItem::Bytes(expected),
        ..
    })) = dictionary.get(DIGEST_SHA_256)
    else {
        return true;
    };
    digest::digest(&digest::SHA256, body).as_ref() == expected.as_slice()
}

/// The `@authority` of the request that fetched the directory.
fn request_authority(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let authority = match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    };
    build_authority(&authority, Some(parsed.scheme()))
}

/// Finds the components of a key directory response signature.
struct ResponseResolver<'a> {
    authority: Option<String>,
    response: &'a HttpResponse,
}

impl ComponentResolver for ResponseResolver<'_> {
    fn resolve(&self, name: &str, component: &Item) -> Option<String> {
        if name == "@authority" {
            // The directory draft has the response signature cover the
            // authority of the request, written with ';req'.
            component.parameter("req")?;
            return self.authority.clone();
        }
        // A request header or any other derived component, which this
        // element does not carry back from the request it sent.
        if name.starts_with('@') || component.parameter("req").is_some() {
            return None;
        }
        self.response
            .header(name)
            .map(|value| value.trim().to_owned())
    }
}

/// Logs an agent card that could not be read.
fn log_card_failure(url: &str, reason: &str) {
    log::warn!(
        "The agent card at '{url}' could not be read because {reason}. The signature \
         status is unaffected."
    );
}

/// Logs a registry of agent cards that could not be read.
pub(crate) fn log_registry_failure(url: &str, reason: &str) {
    log::warn!(
        "The agent card registry at '{url}' could not be read because {reason}. The \
         signature status is unaffected."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_addresses_outside_private_ranges_may_be_fetched() {
        for url in [
            "https://agent.example/keys",
            "https://8.8.8.8/keys",
            "https://[2001:4860:4860::8888]/keys",
        ] {
            assert!(is_safe_url(url), "{url}");
        }
        for url in [
            "http://agent.example/keys",
            "https://user@agent.example/keys",
            "https://127.0.0.1/keys",
            "https://127.1/keys",
            "https://2130706433/keys",
            "https://10.0.0.1/keys",
            "https://172.16.0.1/keys",
            "https://192.168.1.1/keys",
            "https://169.254.169.254/keys",
            "https://100.64.0.1/keys",
            "https://0.0.0.0/keys",
            "https://[::1]/keys",
            "https://[::]/keys",
            "https://[fe80::1]/keys",
            "https://[fd00::1]/keys",
            "https://[::ffff:10.0.0.1]/keys",
            "not an address",
        ] {
            assert!(!is_safe_url(url), "{url}");
        }
    }

    #[test]
    fn a_registry_lists_one_safe_card_address_to_a_line() {
        let text = "# cards\nhttps://a.example/card\n\n  https://b.example/card # b\r\n\
                    http://c.example/card\nhttps://10.0.0.1/card\n";
        assert_eq!(
            parse_registry(text),
            ["https://a.example/card", "https://b.example/card"]
        );
    }

    #[test]
    fn the_shorter_max_age_is_read_from_cache_control() {
        let response = HttpResponse {
            headers: vec![("Cache-Control".to_owned(), "public, max-age=600".to_owned())],
            ..HttpResponse::default()
        };
        assert_eq!(max_age(&response), TimeDelta::try_seconds(600));
    }
}
