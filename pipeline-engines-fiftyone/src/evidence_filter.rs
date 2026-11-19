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

//! The evidence filter used by usage sharing.
//!
//! Usage sharing accepts no evidence. It shares what it finds in the flow
//! data's evidence and in the web request, and this filter decides which of
//! those keys are shared, by rule rather than by a fixed whitelist, so it
//! implements [`EvidenceKeyFilter`] directly. It realises the rules of the
//! [usage sharing specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/usage-sharing-element.md).
//! The prefix of a key is everything before its first `.`, and keys are
//! compared without regard to case.
//!
//! `id.email`, `id.salt`, `request-query` and `request-path`, as the whole key
//! or after a `.`, are never shared, even when sharing all evidence. So
//! `server.request-path` and `query.id.email` are never shared, while
//! `query.valid.email` is an ordinary query string parameter. Nor is the key
//! `server.http-request`, under which a web integration gives the web
//! request, only the values read from it. Nor is a `cookie.*` key whose name
//! does not start with `51d_`, so a site's own cookies, its session cookie
//! among them, are never shared.
//!
//! A `query.*` key, which carries a query string or form parameter, is shared
//! only when the parameter name starts with `51d_` or the key is evidence the
//! pipeline uses, being a key an element of the pipeline accepts, whatever the
//! configuration and even when all evidence is shared. A filter on its own
//! knows no pipeline, so [`EvidenceKeyFilter::include`] shares only `51d_`
//! parameters, and [`EvidenceKeyFilterShareUsage::include_for`] is given the
//! pipeline's filter, as usage sharing does for each request.
//!
//! Unless all evidence is shared:
//!
//! - `header.*` is shared unless the header name is in the blocked list (the
//!   `cookie` header, which carries every cookie, is always blocked).
//! - `session.*` is shared only when the name starts with `51d_`.
//! - everything else is shared.

use std::borrow::Cow;
use std::collections::HashSet;

use fiftyone_pipeline_core::constants::{
    EVIDENCE_COOKIE_PREFIX, EVIDENCE_HTTP_HEADER_PREFIX, EVIDENCE_HTTP_REQUEST_KEY,
    EVIDENCE_QUERY_PREFIX, EVIDENCE_SEPARATOR, EVIDENCE_SESSION_PREFIX,
};
use fiftyone_pipeline_core::EvidenceKeyFilter;

use crate::constants::{
    EVIDENCE_HTTPHEADER_COOKIE_SUFFIX, EVIDENCE_SEQUENCE, EVIDENCE_SESSIONID, FIFTYONE_PREFIX,
};

/// The keys never shared, matched as the whole key or after a `.`, whatever
/// the configuration. `id.email` is a raw email address and `id.salt` the salt
/// used to make hashed email identifiers. `request-query` is the whole query
/// string, so sharing it would undo the rules for query string parameters.
/// `request-path` would make each page a visitor opens look like a new
/// session to the repeat evidence tracker, and is the part of a URL most
/// likely to carry a name or an identifier. `header.cookie` is the Cookie
/// header, which carries every cookie. The `51D_` cookies are shared under
/// their own `cookie.` keys instead.
const NEVER_SHARED: [&str; 5] = [
    "id.email",
    "id.salt",
    "request-query",
    "request-path",
    "header.cookie",
];

/// True if the lowercase `key` is one of [`NEVER_SHARED`], or ends with a `.`
/// and one of them.
fn is_never_shared(key: &str) -> bool {
    NEVER_SHARED.iter().any(|never| {
        key.strip_suffix(never)
            .is_some_and(|rest| rest.is_empty() || rest.ends_with(EVIDENCE_SEPARATOR))
    })
}

/// Accepts no key, for a filter asked without a pipeline.
struct NoKeys;

impl EvidenceKeyFilter for NoKeys {
    fn include(&self, _key: &str) -> bool {
        false
    }
}

/// The key in lowercase, borrowed when it is already lowercase ASCII.
fn lowercase(key: &str) -> Cow<'_, str> {
    if key.bytes().all(|b| b.is_ascii() && !b.is_ascii_uppercase()) {
        Cow::Borrowed(key)
    } else {
        Cow::Owned(key.to_lowercase())
    }
}

/// Filters evidence keys for the usage-sharing element.
///
/// Construct it with [`EvidenceKeyFilterShareUsage::new`] for the rule-based
/// filter, or [`EvidenceKeyFilterShareUsage::share_all`] to share every key
/// but those never shared. Keys are compared without regard to case. The core
/// gives keys in lowercase, and a key in another case is lowered first.
#[derive(Debug, Clone)]
pub struct EvidenceKeyFilterShareUsage {
    /// When true, every key but those never shared is shared, and the lists
    /// below are ignored.
    share_all: bool,
    /// Lower-cased HTTP header names that must never be shared.
    blocked_http_headers: HashSet<String>,
}

impl EvidenceKeyFilterShareUsage {
    /// Create a rule-based filter.
    ///
    /// `blocked_http_headers` are header names (any case) that must not be
    /// shared. The `cookie` header is always added to this set.
    ///
    /// `included_query_string_params` is deprecated and has no effect. A
    /// query string or form parameter is shared only when its name starts
    /// with `51d_` or the pipeline uses it, as the module documentation says.
    pub fn new<I, S>(blocked_http_headers: I, included_query_string_params: Option<Vec<S>>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut blocked: HashSet<String> = blocked_http_headers
            .into_iter()
            .map(|h| h.as_ref().to_lowercase())
            .collect();
        blocked.insert(EVIDENCE_HTTPHEADER_COOKIE_SUFFIX.to_lowercase());
        // Deprecated, with no effect.
        drop(included_query_string_params);

        EvidenceKeyFilterShareUsage {
            share_all: false,
            blocked_http_headers: blocked,
        }
    }

    /// Create a filter that shares every evidence key but those never shared.
    pub fn share_all() -> Self {
        EvidenceKeyFilterShareUsage {
            share_all: true,
            blocked_http_headers: HashSet::new(),
        }
    }

    /// True if `key` is shared in a pipeline whose elements accept the keys
    /// `used` includes, such as the filter
    /// [`FlowData::evidence_key_filter`](fiftyone_pipeline_core::FlowData::evidence_key_filter)
    /// gives. Only a `query.*` key depends on `used`.
    pub fn include_for(&self, key: &str, used: &dyn EvidenceKeyFilter) -> bool {
        self.include_inner(key, used)
    }

    /// The core rule, shared by this filter and the tracker filter.
    fn include_inner(&self, key: &str, used: &dyn EvidenceKeyFilter) -> bool {
        let key = lowercase(key);
        if is_never_shared(&key) || key == EVIDENCE_HTTP_REQUEST_KEY {
            return false;
        }

        let Some((prefix, field)) = key.split_once(EVIDENCE_SEPARATOR) else {
            // No prefix: share by default.
            return true;
        };
        if prefix == EVIDENCE_COOKIE_PREFIX {
            // Only 51D_ cookies, even when all evidence is shared.
            return field.starts_with(FIFTYONE_PREFIX);
        }
        if prefix == EVIDENCE_QUERY_PREFIX {
            // Whatever the configuration, even when all evidence is shared.
            return field.starts_with(FIFTYONE_PREFIX) || used.include(&key);
        }
        if self.share_all || prefix.is_empty() {
            return true;
        }

        if prefix == EVIDENCE_HTTP_HEADER_PREFIX {
            !self.blocked_http_headers.contains(field)
        } else if prefix == EVIDENCE_SESSION_PREFIX {
            field.starts_with(FIFTYONE_PREFIX)
        } else {
            true
        }
    }
}

/// Knows no pipeline, so a `query.*` key is included only for a `51d_` name.
/// See [`EvidenceKeyFilterShareUsage::include_for`].
impl EvidenceKeyFilter for EvidenceKeyFilterShareUsage {
    fn include(&self, key: &str) -> bool {
        self.include_inner(key, &NoKeys)
    }

    fn order(&self, key: &str) -> Option<i32> {
        if self.include(key) {
            Some(100)
        } else {
            None
        }
    }
}

/// The filter used by the usage-sharing repeat-evidence tracker.
///
/// It wraps [`EvidenceKeyFilterShareUsage`] but always excludes the session id
/// and sequence evidence. Those values are unique per request (the sequence
/// element makes the session id unique and increments the sequence), so
/// including them in the dedup key would make every request look unique and
/// defeat the repeat-evidence suppression. With track session on it also
/// includes the session cookie, which the wrapped filter does not share
/// unless its name starts with `51D_`.
#[derive(Debug, Clone)]
pub struct EvidenceKeyFilterShareUsageTracker {
    inner: EvidenceKeyFilterShareUsage,
    /// The lower-cased key of the session cookie track session compares.
    session_cookie_key: Option<String>,
}

impl EvidenceKeyFilterShareUsageTracker {
    /// Wrap a share-usage filter for tracker use.
    pub fn new(inner: EvidenceKeyFilterShareUsage) -> Self {
        EvidenceKeyFilterShareUsageTracker {
            inner,
            session_cookie_key: None,
        }
    }

    /// Include the cookie with this name, in any case, as track session
    /// compares it. Returns `self` for chaining.
    pub fn with_session_cookie(mut self, name: &str) -> Self {
        self.session_cookie_key = Some(format!(
            "{EVIDENCE_COOKIE_PREFIX}{EVIDENCE_SEPARATOR}{}",
            name.to_lowercase()
        ));
        self
    }

    /// True for the session cookie track session compares.
    fn is_session_cookie(&self, key: &str) -> bool {
        self.session_cookie_key.as_deref() == Some(lowercase(key).as_ref())
    }

    /// True if `key` forms the tracker's key in a pipeline whose elements
    /// accept the keys `used` includes, as
    /// [`EvidenceKeyFilterShareUsage::include_for`] says.
    pub fn include_for(&self, key: &str, used: &dyn EvidenceKeyFilter) -> bool {
        if key == EVIDENCE_SESSIONID || key == EVIDENCE_SEQUENCE {
            return false;
        }
        self.is_session_cookie(key) || self.inner.include_for(key, used)
    }
}

/// Knows no pipeline, so a `query.*` key is included only for a `51d_` name.
/// See [`EvidenceKeyFilterShareUsageTracker::include_for`].
impl EvidenceKeyFilter for EvidenceKeyFilterShareUsageTracker {
    fn include(&self, key: &str) -> bool {
        self.include_for(key, &NoKeys)
    }

    fn order(&self, key: &str) -> Option<i32> {
        if self.is_session_cookie(key) {
            return Some(100);
        }
        if key == EVIDENCE_SESSIONID || key == EVIDENCE_SEQUENCE {
            return None;
        }
        self.inner.order(key)
    }
}

#[cfg(test)]
mod tests {
    use fiftyone_pipeline_core::EvidenceKeyFilterWhitelist;

    use super::*;

    /// A pipeline's filter that accepts `keys`.
    fn used<const N: usize>(keys: [&str; N]) -> EvidenceKeyFilterWhitelist {
        EvidenceKeyFilterWhitelist::new(keys)
    }

    #[test]
    fn the_web_request_key_is_never_shared() {
        for filter in [
            EvidenceKeyFilterShareUsage::new(Vec::<&str>::new(), None),
            EvidenceKeyFilterShareUsage::share_all(),
        ] {
            assert!(!filter.include("server.http-request"));
            assert!(!filter.include("Server.HTTP-Request"));
            assert!(filter.include("server.http-requests"));
            assert!(filter.include_for("query.http-request", &used(["query.http-request"])));
        }
    }

    #[test]
    fn headers_share_by_default_but_blocked_are_excluded() {
        let filter = EvidenceKeyFilterShareUsage::new(["referer"], Some(Vec::<&str>::new()));
        assert!(filter.include("header.user-agent"));
        assert!(!filter.include("header.referer"));
        // The cookie header is always blocked.
        assert!(!filter.include("header.cookie"));
    }

    #[test]
    fn cookies_only_share_fiftyone_prefixed() {
        let filter = EvidenceKeyFilterShareUsage::new(Vec::<&str>::new(), Some(Vec::<&str>::new()));
        assert!(filter.include("cookie.51d_profile"));
        assert!(!filter.include("cookie.sessionid"));
    }

    #[test]
    fn query_allow_list_and_fiftyone_prefix() {
        let filter = EvidenceKeyFilterShareUsage::new(Vec::<&str>::new(), Some(vec!["myparam"]));
        assert!(filter.include("query.51d_something"));
        // The allow list has no effect, and a parameter the pipeline uses is
        // shared.
        assert!(!filter.include("query.myparam"));
        assert!(filter.include_for("query.myparam", &used(["query.myparam"])));
        assert!(!filter.include("query.other"));
    }

    #[test]
    fn null_query_list_shares_all_query() {
        let filter = EvidenceKeyFilterShareUsage::new(Vec::<&str>::new(), None::<Vec<&str>>);
        // No list has no effect either, so only a parameter the pipeline uses
        // is shared.
        assert!(!filter.include("query.anything"));
        assert!(filter.include_for("query.anything", &used(["query.anything"])));
    }

    #[test]
    fn share_all_includes_every_key_but_those_never_shared() {
        let filter = EvidenceKeyFilterShareUsage::share_all();
        assert!(filter.include("cookie.51d_profile"));
        // Even when all evidence is shared, only a parameter the pipeline
        // uses or with a 51D_ name.
        assert!(!filter.include("query.other"));
        assert!(filter.include_for("query.other", &used(["query.other"])));
        assert!(filter.include("session.other"));
    }

    /// usage-sharing-element.md: a cookie whose name does not start with
    /// `51D_` is never shared, even when all evidence is shared.
    #[test]
    fn no_cookie_but_51d_ones_is_shared_even_when_all_evidence_is() {
        for (name, filter) in both_filters() {
            for key in spellings("cookie.sessionid") {
                assert!(!filter.include(&key), "{name}: {key} shared");
                assert_eq!(filter.order(&key), None, "{name}: {key}");
            }
            for key in spellings("cookie.51d_profile") {
                assert!(filter.include(&key), "{name}: {key} not shared");
            }
        }
    }

    /// The keys never shared, bare and after a prefix.
    const NEVER_SHARED_KEYS: &[&str] = &[
        "id.email",
        "id.salt",
        "request-path",
        "request-query",
        "server.request-path",
        "server.request-query",
        "query.id.email",
        "query.id.salt",
        "header.request-path",
        "header.cookie",
    ];

    /// `key` as given, in upper case, and with each letter after a `.` or
    /// `-` and the first letter in upper case.
    fn spellings(key: &str) -> [String; 3] {
        let mut title = String::with_capacity(key.len());
        let mut upper = true;
        for c in key.chars() {
            if upper {
                title.extend(c.to_uppercase());
            } else {
                title.push(c);
            }
            upper = c == '.' || c == '-';
        }
        [key.to_owned(), key.to_uppercase(), title]
    }

    /// A rule-based filter that shares every query string parameter, so a
    /// query key is refused only by the never shared rule, and a filter that
    /// shares all evidence.
    fn both_filters() -> [(&'static str, EvidenceKeyFilterShareUsage); 2] {
        [
            (
                "rule-based",
                EvidenceKeyFilterShareUsage::new(Vec::<&str>::new(), None::<Vec<&str>>),
            ),
            ("share all", EvidenceKeyFilterShareUsage::share_all()),
        ]
    }

    #[test]
    fn the_keys_never_shared_are_not_shared_in_any_case() {
        for (name, filter) in both_filters() {
            for key in NEVER_SHARED_KEYS {
                for spelling in spellings(key) {
                    assert!(!filter.include(&spelling), "{name}: {spelling} shared");
                    assert_eq!(filter.order(&spelling), None, "{name}: {spelling}");
                }
            }
        }
    }

    #[test]
    fn a_key_that_only_ends_like_one_never_shared_is_shared() {
        for (name, filter) in both_filters() {
            for key in [
                "query.valid.email",
                "query.xid.email",
                "query.id.email.copy",
                "server.my-request-path",
                "header.x-request-query-id",
            ] {
                // Each query parameter is one the pipeline uses.
                let uses = used([key]);
                assert!(filter.include_for(key, &uses), "{name}: {key} not shared");
            }
        }
    }

    /// usage-sharing-element.md: "the filter function MUST return true for
    /// both, whatever the configuration, because usage is counted against
    /// them", being `fiftyone.resource-key` and `fiftyone.license-key`.
    #[test]
    fn the_resource_key_and_license_key_are_shared_whatever_the_configuration() {
        let narrow = EvidenceKeyFilterShareUsage::new(
            ["user-agent", "fiftyone", "resource-key", "license-key"],
            Some(vec!["other"]),
        );
        let mut filters: Vec<(&str, EvidenceKeyFilterShareUsage)> = both_filters().into();
        filters.push(("headers blocked and one query value", narrow));
        for (name, filter) in filters {
            let tracker = EvidenceKeyFilterShareUsageTracker::new(filter.clone());
            for key in ["fiftyone.resource-key", "fiftyone.license-key"] {
                for spelling in spellings(key) {
                    assert!(filter.include(&spelling), "{name}: {spelling} not shared");
                    assert!(filter.order(&spelling).is_some(), "{name}: {spelling}");
                    assert!(
                        tracker.include(&spelling),
                        "{name}: {spelling} not in the tracker's key"
                    );
                }
            }
        }
    }

    #[test]
    fn session_values_are_shared_only_when_they_start_51d() {
        let filter = EvidenceKeyFilterShareUsage::new(Vec::<&str>::new(), None::<Vec<&str>>);
        for key in spellings("session.51d_x") {
            assert!(filter.include(&key), "{key} not shared");
        }
        for key in spellings("session.other") {
            assert!(!filter.include(&key), "{key} shared");
        }
    }

    #[test]
    fn sharing_all_shares_every_session_value() {
        let filter = EvidenceKeyFilterShareUsage::share_all();
        for key in ["session.51d_x", "session.other"] {
            for spelling in spellings(key) {
                assert!(filter.include(&spelling), "{spelling} not shared");
            }
        }
    }

    #[test]
    fn the_cookie_header_is_never_shared() {
        for (name, filter) in both_filters() {
            for key in spellings("header.cookie") {
                assert!(!filter.include(&key), "{name}: {key} shared");
            }
        }
    }

    #[test]
    fn the_rules_compare_keys_ignoring_case() {
        let filter = EvidenceKeyFilterShareUsage::new(["Referer"], Some(vec!["MyParam"]));
        assert!(!filter.include("HEADER.REFERER"));
        assert!(filter.include("Header.User-Agent"));
        assert!(!filter.include("Query.MYPARAM"));
        assert!(filter.include_for("Query.MYPARAM", &used(["query.myparam"])));
        assert!(filter.include("QUERY.51D_Other"));
        assert!(!filter.include("Query.Other"));
        assert!(filter.include("Cookie.51D_Profile"));
        assert!(!filter.include("COOKIE.other"));
    }

    #[test]
    fn tracker_excludes_session_and_sequence() {
        let tracker = EvidenceKeyFilterShareUsageTracker::new(EvidenceKeyFilterShareUsage::new(
            Vec::<&str>::new(),
            None::<Vec<&str>>,
        ));
        assert!(!tracker.include(EVIDENCE_SESSIONID));
        assert!(!tracker.include(EVIDENCE_SEQUENCE));
        assert!(tracker.include("header.user-agent"));
    }
}
