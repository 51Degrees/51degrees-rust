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

//! A [`TdlSourceResolver`] that lists a GitHub folder and takes the highest
//! numbered file.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use fiftyone_pipeline_core::{Error, Result};
use url::Url;

use crate::tdl::{latest_url, TdlSourceResolver, TdlSources};

/// How long a resolved URI is used before the folder is listed again. Twelve
/// hours suits a source published once or twice a day and stays well under
/// GitHub's limit of 60 requests an hour without a token.
pub const DEFAULT_CACHE_TTL_SECONDS: u64 = 12 * 60 * 60;

/// How long one listing of a folder may take.
pub const REQUEST_TIMEOUT_SECONDS: u64 = 10;

/// The GitHub REST API the folders are listed through.
pub const GITHUB_API_URL: &str = "https://api.github.com/";

/// The media type GitHub's REST API asks for.
const GITHUB_ACCEPT: &str = "application/vnd.github+json";

/// The version of GitHub's REST API the listing is read as.
const GITHUB_API_VERSION: &str = "2022-11-28";

/// A response the client received, whatever its status.
#[derive(Debug, Clone, Default)]
pub struct TdlHttpResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The body.
    pub body: Vec<u8>,
}

/// Sends the GET requests that list a GitHub folder.
///
/// Implementations must be `Send + Sync`, because one engine serves many
/// requests at once. The request is made on the thread that processes the
/// request naming the TDL source, so the client blocks.
pub trait TdlHttpClient: Send + Sync {
    /// Send a GET to `url` with `headers`, abandoning it after `timeout`.
    /// Return the response for any response received, whatever its status,
    /// and `Err` with a readable reason only where no response was received.
    fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        timeout: Duration,
    ) -> std::result::Result<TdlHttpResponse, String>;
}

/// Resolves a TDL source id by listing the source's GitHub folder and taking
/// the file with the highest version number, as .NET's
/// `GitHubTdlSourceResolver` does.
///
/// Each answer is kept for the cache time, [`DEFAULT_CACHE_TTL_SECONDS`]
/// unless set otherwise. Only one request lists a source's folder at a time,
/// and others asking for the same source wait for its answer. When a listing
/// fails or finds no file, the last answer is given, if there is one.
pub struct GitHubTdlSourceResolver {
    client: Arc<dyn TdlHttpClient>,
    sources: TdlSources,
    user_agent: String,
    api_url: String,
    ttl: Duration,
    cache: Mutex<HashMap<String, CacheEntry>>,
    refreshing: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

/// A resolved URI and when it is next to be looked up again, or [`None`] when
/// the cache time is too long to count.
#[derive(Clone)]
struct CacheEntry {
    url: Url,
    fresh_until: Option<Instant>,
}

impl CacheEntry {
    fn is_fresh(&self, now: Instant) -> bool {
        self.fresh_until.is_none_or(|until| until > now)
    }
}

impl GitHubTdlSourceResolver {
    /// A resolver for `sources` that lists folders with `client`, sending
    /// `user_agent` as the `User-Agent` header. GitHub refuses a request
    /// without one and asks that it name the caller.
    pub fn new(
        client: Arc<dyn TdlHttpClient>,
        sources: TdlSources,
        user_agent: impl Into<String>,
    ) -> Result<Self> {
        let user_agent = user_agent.into();
        if user_agent.trim().is_empty() {
            return Err(Error::configuration("User-Agent must be supplied."));
        }
        Ok(GitHubTdlSourceResolver {
            client,
            sources,
            user_agent,
            api_url: GITHUB_API_URL.to_owned(),
            ttl: Duration::from_secs(DEFAULT_CACHE_TTL_SECONDS),
            cache: Mutex::new(HashMap::new()),
            refreshing: Mutex::new(HashMap::new()),
        })
    }

    /// A resolver for the sources compiled into this crate, listing folders
    /// through GitHub with the built-in client.
    ///
    /// Compiled only with the `reqwest-client` feature.
    #[cfg(feature = "reqwest-client")]
    pub fn create_default(user_agent: impl Into<String>) -> Result<Self> {
        let client = ReqwestTdlHttpClient::new().map_err(Error::configuration)?;
        Self::new(Arc::new(client), TdlSources::load_default()?, user_agent)
    }

    /// Keep each answer for `ttl` rather than the default twelve hours.
    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// List folders through the API at `api_url`, ending in `/`, rather than
    /// GitHub's, as for a GitHub Enterprise server or a test.
    pub fn with_api_url(mut self, api_url: impl Into<String>) -> Self {
        self.api_url = api_url.into();
        self
    }

    /// The sources this resolver knows.
    pub fn sources(&self) -> &TdlSources {
        &self.sources
    }

    fn cached(&self, key: &str) -> Option<CacheEntry> {
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned()
    }

    /// The latest URI for `id`, listing the folder, or the reason it could
    /// not be listed.
    fn fetch_latest(&self, id: &str) -> std::result::Result<Option<Url>, String> {
        let Some((source, pattern)) = self.sources.get_compiled(id) else {
            return Ok(None);
        };
        let url = format!(
            "{}repos/{}/contents/{}",
            self.api_url, source.repository, source.directory
        );
        let headers = [
            ("User-Agent", self.user_agent.as_str()),
            ("Accept", GITHUB_ACCEPT),
            ("X-GitHub-Api-Version", GITHUB_API_VERSION),
        ];
        let response =
            self.client
                .get(&url, &headers, Duration::from_secs(REQUEST_TIMEOUT_SECONDS))?;
        if !(200..300).contains(&response.status) {
            log::warn!(
                "GitHub returned {} listing '{}/{}'.",
                response.status,
                source.repository,
                source.directory
            );
            return Ok(None);
        }
        latest_url(source, pattern, &response.body)
    }
}

impl TdlSourceResolver for GitHubTdlSourceResolver {
    fn is_known(&self, id: &str) -> bool {
        self.sources.contains(id)
    }

    fn resolve(&self, id: &str) -> Option<Url> {
        if !self.sources.contains(id) {
            return None;
        }
        let key = id.to_lowercase();
        let cached = self.cached(&key);
        if let Some(entry) = cached
            .as_ref()
            .filter(|entry| entry.is_fresh(Instant::now()))
        {
            return Some(entry.url.clone());
        }

        // One listing for each source at a time. Others asking for the same
        // source wait here and take the answer the first one stored.
        let gate = Arc::clone(
            self.refreshing
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(key.clone())
                .or_default(),
        );
        let _turn = gate.lock().unwrap_or_else(PoisonError::into_inner);
        let cached = self.cached(&key);
        let now = Instant::now();
        if let Some(entry) = cached.as_ref().filter(|entry| entry.is_fresh(now)) {
            return Some(entry.url.clone());
        }
        match self.fetch_latest(id) {
            Ok(Some(url)) => {
                let entry = CacheEntry {
                    url: url.clone(),
                    fresh_until: now.checked_add(self.ttl),
                };
                self.cache
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(key, entry);
                return Some(url);
            }
            Ok(None) => {
                log::warn!("TDL source '{id}' has no files matching its pattern.");
            }
            Err(reason) => {
                log::warn!(
                    "TDL source '{id}' refresh failed; falling back to cached value if any. \
                     {reason}"
                );
            }
        }
        cached.map(|entry| entry.url)
    }
}

#[cfg(feature = "reqwest-client")]
pub use reqwest_client::ReqwestTdlHttpClient;

#[cfg(feature = "reqwest-client")]
mod reqwest_client {
    use std::time::Duration;

    use super::{TdlHttpClient, TdlHttpResponse};

    /// The built-in [`TdlHttpClient`], backed by a blocking reqwest client.
    ///
    /// Compiled only with the `reqwest-client` feature.
    #[derive(Debug, Clone)]
    pub struct ReqwestTdlHttpClient {
        client: reqwest::blocking::Client,
    }

    impl ReqwestTdlHttpClient {
        /// Create a client.
        pub fn new() -> Result<Self, String> {
            let client = reqwest::blocking::Client::builder()
                .build()
                .map_err(|error| format!("the HTTP client could not be built: {error}"))?;
            Ok(ReqwestTdlHttpClient { client })
        }
    }

    impl TdlHttpClient for ReqwestTdlHttpClient {
        fn get(
            &self,
            url: &str,
            headers: &[(&str, &str)],
            timeout: Duration,
        ) -> Result<TdlHttpResponse, String> {
            let mut request = self.client.get(url).timeout(timeout);
            for (name, value) in headers {
                request = request.header(*name, *value);
            }
            let response = request.send().map_err(|error| error.to_string())?;
            let status = response.status().as_u16();
            let body = response
                .bytes()
                .map_err(|error| error.to_string())?
                .to_vec();
            Ok(TdlHttpResponse { status, body })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A client that answers every request from a fixed list, in turn, and
    /// counts the requests.
    struct ScriptedClient {
        answers: Vec<std::result::Result<TdlHttpResponse, String>>,
        requests: AtomicUsize,
        last_url: Mutex<String>,
        last_headers: Mutex<Vec<(String, String)>>,
    }

    impl ScriptedClient {
        fn new(answers: Vec<std::result::Result<TdlHttpResponse, String>>) -> Arc<Self> {
            Arc::new(ScriptedClient {
                answers,
                requests: AtomicUsize::new(0),
                last_url: Mutex::new(String::new()),
                last_headers: Mutex::new(Vec::new()),
            })
        }

        fn requests(&self) -> usize {
            self.requests.load(Ordering::SeqCst)
        }
    }

    impl TdlHttpClient for ScriptedClient {
        fn get(
            &self,
            url: &str,
            headers: &[(&str, &str)],
            _timeout: Duration,
        ) -> std::result::Result<TdlHttpResponse, String> {
            let index = self.requests.fetch_add(1, Ordering::SeqCst);
            *self.last_url.lock().unwrap() = url.to_owned();
            *self.last_headers.lock().unwrap() = headers
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect();
            self.answers[index.min(self.answers.len() - 1)].clone()
        }
    }

    fn listing(names: &[&str]) -> std::result::Result<TdlHttpResponse, String> {
        let entries: Vec<String> = names
            .iter()
            .map(|name| format!(r#"{{"name":"{name}","type":"file"}}"#))
            .collect();
        Ok(TdlHttpResponse {
            status: 200,
            body: format!("[{}]", entries.join(",")).into_bytes(),
        })
    }

    fn resolver(client: &Arc<ScriptedClient>) -> GitHubTdlSourceResolver {
        GitHubTdlSourceResolver::new(
            Arc::clone(client) as Arc<dyn TdlHttpClient>,
            TdlSources::load_default().unwrap(),
            "robots-txt-tests",
        )
        .unwrap()
    }

    #[test]
    fn a_known_id_resolves_to_the_latest_file_and_is_kept() {
        let client = ScriptedClient::new(vec![listing(&["1.txt", "3.txt", "2.txt"])]);
        let resolver = resolver(&client);
        assert!(resolver.is_known("mow-socw"));
        assert!(!resolver.is_known("other"));
        let url = resolver.resolve("MOW-SOCW").expect("the id resolves");
        assert_eq!(url.as_str(), "https://m4ow.uk/socw/3.txt");
        assert_eq!(resolver.resolve("mow-socw"), Some(url));
        assert_eq!(client.requests(), 1);
        assert_eq!(
            *client.last_url.lock().unwrap(),
            "https://api.github.com/repos/movement-for-an-open-web/terms-documents/contents/socw"
        );
        let headers = client.last_headers.lock().unwrap().clone();
        assert!(headers.contains(&("User-Agent".to_owned(), "robots-txt-tests".to_owned())));
        assert!(headers.contains(&("Accept".to_owned(), GITHUB_ACCEPT.to_owned())));
        assert!(headers.contains(&(
            "X-GitHub-Api-Version".to_owned(),
            GITHUB_API_VERSION.to_owned()
        )));
    }

    #[test]
    fn an_unknown_id_is_not_looked_up() {
        let client = ScriptedClient::new(vec![listing(&["1.txt"])]);
        let resolver = resolver(&client);
        assert_eq!(resolver.resolve("other"), None);
        assert_eq!(resolver.resolve(""), None);
        assert_eq!(client.requests(), 0);
    }

    #[test]
    fn a_failed_listing_gives_the_last_answer() {
        let client = ScriptedClient::new(vec![
            listing(&["1.txt"]),
            Err("connection refused".to_owned()),
            Ok(TdlHttpResponse {
                status: 403,
                body: Vec::new(),
            }),
            listing(&["README.md"]),
            listing(&["2.txt"]),
        ]);
        let resolver = resolver(&client).with_cache_ttl(Duration::ZERO);
        let first = resolver.resolve("MOW-SOCW").expect("the id resolves");
        assert_eq!(first.as_str(), "https://m4ow.uk/socw/1.txt");
        for _ in 0..3 {
            assert_eq!(resolver.resolve("MOW-SOCW"), Some(first.clone()));
        }
        assert_eq!(
            resolver.resolve("MOW-SOCW").unwrap().as_str(),
            "https://m4ow.uk/socw/2.txt"
        );
        assert_eq!(client.requests(), 5);
    }

    #[test]
    fn a_failed_first_listing_gives_nothing() {
        let client = ScriptedClient::new(vec![Err("timed out".to_owned())]);
        let resolver = resolver(&client);
        assert_eq!(resolver.resolve("MOW-SOCW"), None);
    }

    #[test]
    fn the_user_agent_is_required() {
        let client = ScriptedClient::new(vec![listing(&[])]);
        let result = GitHubTdlSourceResolver::new(
            client as Arc<dyn TdlHttpClient>,
            TdlSources::load_default().unwrap(),
            " ",
        );
        assert!(result.is_err());
    }
}
