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

//! Evidence: the immutable, case-insensitive input to a flow data instance.
//!
//! This module implements the
//! [evidence specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/evidence.md).
//! The key points it enforces:
//!
//! - Keys are case-insensitive. They are lowercased when an [`Evidence`] is
//!   built, so all later lookups are exact and need no per-lookup case folding.
//! - Values are kept exactly as supplied (values are case-sensitive).
//! - Keys are usually `prefix.field`. [`EvidencePrefix`] models the known
//!   prefixes and their precedence: `query` > `header` > `cookie` > `server` >
//!   `fiftyone` > `location`, then any other prefix in alphabetical order.
//! - Once built, an [`Evidence`] is immutable, so concurrent `&` reads are safe
//!   with no locking (see the
//!   [thread-safety specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/thread-safety.md#evidence)).

use std::cmp::Ordering;
use std::collections::HashMap;

use ahash::RandomState;

use crate::ci_map::ci_get;
use crate::constants;

/// A known evidence prefix, in precedence order.
///
/// The prefix indicates where an evidence value came from. When the same value
/// is supplied under two prefixes (for example `header.user-agent` and
/// `query.user-agent`), the one whose prefix appears earlier in this list MUST
/// win. See the
/// [evidence specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/evidence.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvidencePrefix {
    /// `query`. Set by the application, a query string or a POST body.
    Query,
    /// `header`. An HTTP header.
    Header,
    /// `cookie`. A cookie.
    Cookie,
    /// `server`. The server that received the request.
    Server,
    /// `fiftyone`. Used internally by 51Degrees.
    FiftyOne,
    /// `location`. Geo-location information.
    Location,
}

impl EvidencePrefix {
    /// The lowercase string spelling of this prefix, for example `"header"`.
    pub fn as_str(&self) -> &'static str {
        match self {
            EvidencePrefix::Query => constants::EVIDENCE_QUERY_PREFIX,
            EvidencePrefix::Header => constants::EVIDENCE_HTTP_HEADER_PREFIX,
            EvidencePrefix::Cookie => constants::EVIDENCE_COOKIE_PREFIX,
            EvidencePrefix::Server => constants::EVIDENCE_SERVER_PREFIX,
            EvidencePrefix::FiftyOne => constants::EVIDENCE_FIFTYONE_PREFIX,
            EvidencePrefix::Location => constants::EVIDENCE_LOCATION_PREFIX,
        }
    }

    /// The precedence rank of this prefix, lower meaning higher precedence.
    ///
    /// `query` is `0`, then `header`, `cookie`, `server`, `fiftyone` and
    /// `location` ascend in that order.
    pub fn precedence(&self) -> u32 {
        match self {
            EvidencePrefix::Query => 0,
            EvidencePrefix::Header => 1,
            EvidencePrefix::Cookie => 2,
            EvidencePrefix::Server => 3,
            EvidencePrefix::FiftyOne => 4,
            EvidencePrefix::Location => 5,
        }
    }

    /// Parse a known prefix from its lowercase string spelling. Returns `None`
    /// for any other (unknown) prefix.
    ///
    /// This is intentionally named `parse` (not `from_str`) and returns an
    /// [`Option`] rather than implementing [`std::str::FromStr`], because an
    /// unknown prefix is an ordinary, expected outcome (it just sorts
    /// alphabetically) rather than an error.
    pub fn parse(prefix: &str) -> Option<EvidencePrefix> {
        match prefix {
            constants::EVIDENCE_QUERY_PREFIX => Some(EvidencePrefix::Query),
            constants::EVIDENCE_HTTP_HEADER_PREFIX => Some(EvidencePrefix::Header),
            constants::EVIDENCE_COOKIE_PREFIX => Some(EvidencePrefix::Cookie),
            constants::EVIDENCE_SERVER_PREFIX => Some(EvidencePrefix::Server),
            constants::EVIDENCE_FIFTYONE_PREFIX => Some(EvidencePrefix::FiftyOne),
            constants::EVIDENCE_LOCATION_PREFIX => Some(EvidencePrefix::Location),
            _ => None,
        }
    }
}

/// Compute the precedence ordering of two evidence keys.
///
/// Known prefixes are ranked first by [`EvidencePrefix::precedence`]. A known
/// prefix always outranks an unknown one. Two unknown prefixes (and ties within
/// the same prefix) are ordered alphabetically by the full key, which is the
/// deterministic tie-break the specification requires so that conflicts resolve
/// the same way every time.
fn key_precedence(key: &str) -> (u32, &str) {
    // Split into prefix and the remainder. A key without a separator has no
    // recognized prefix, so it falls into the alphabetical bucket.
    let prefix = key
        .split_once(constants::EVIDENCE_SEPARATOR)
        .map(|(p, _)| p);
    match prefix.and_then(EvidencePrefix::parse) {
        // Known prefixes occupy ranks 0..=5.
        Some(p) => (p.precedence(), key),
        // Unknown prefixes are ranked after every known prefix, then ordered
        // alphabetically by the whole key.
        None => (u32::MAX, key),
    }
}

/// Order two evidence keys by precedence, then alphabetically.
///
/// This is the single ordering used by both conflict resolution and
/// [`Evidence::generate_key`], so caching, the web `ETag` and any other
/// consumer all agree on key order.
pub fn compare_keys(left: &str, right: &str) -> Ordering {
    let (lp, lk) = key_precedence(left);
    let (rp, rk) = key_precedence(right);
    lp.cmp(&rp).then_with(|| lk.cmp(rk))
}

/// An immutable, case-insensitive store of evidence key-value pairs.
///
/// Build one with an [`EvidenceBuilder`] (via [`Evidence::builder`]). Keys are
/// lowercased on insertion. Values are stored verbatim. Once built the store
/// cannot change, so it is cheap to share by reference across threads during
/// processing.
#[derive(Debug, Clone, Default)]
pub struct Evidence {
    entries: HashMap<String, String, RandomState>,
}

impl Evidence {
    /// Start building an [`Evidence`] instance.
    pub fn builder() -> EvidenceBuilder {
        EvidenceBuilder::new()
    }

    /// An empty evidence using this one's hasher. Seeding a new hasher writes
    /// a counter every thread shares, so copying one is cheaper under load.
    pub(crate) fn empty_with_same_hasher(&self) -> Evidence {
        Evidence {
            entries: HashMap::with_hasher(self.entries.hasher().clone()),
        }
    }

    /// Look up a value by key. The key is matched case-insensitively.
    pub fn get(&self, key: &str) -> Option<&str> {
        ci_get(&self.entries, key).map(|value| value.as_str())
    }

    /// True if the store contains the given key (matched case-insensitively).
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// The number of evidence entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if there is no evidence.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over the `(key, value)` pairs. Keys are lowercase. The iteration
    /// order is unspecified; use [`Evidence::generate_key`] when a stable order
    /// is required.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Build a deterministic [`DataKey`] from the evidence entries selected by
    /// the supplied filter.
    ///
    /// The selected entries are ordered by [`compare_keys`] (precedence then
    /// name), so the same evidence always produces an equal [`DataKey`]. This
    /// is what makes the key usable as a cache key and as the stable basis for
    /// the web `ETag`. See the
    /// [advertise-accepted-evidence specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/advertize-accepted-evidence.md#usage).
    pub fn generate_key(&self, filter: &dyn EvidenceKeyFilter) -> DataKey {
        let mut selected: Vec<(&str, &str)> = self
            .entries
            .iter()
            .filter(|(k, _)| filter.include(k))
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        // Order primarily by the filter's declared precedence, falling back to
        // the global key precedence so the result is fully deterministic even
        // when the filter assigns equal ranks.
        selected.sort_by(|(lk, _), (rk, _)| {
            let lo = filter.order(lk);
            let ro = filter.order(rk);
            lo.cmp(&ro).then_with(|| compare_keys(lk, rk))
        });
        DataKey {
            entries: selected
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
        }
    }
}

/// Builder for an immutable [`Evidence`] instance.
///
/// Adding a key that already exists (after case folding) overwrites the earlier
/// value, because evidence keys are matched case-insensitively.
#[derive(Debug, Default)]
pub struct EvidenceBuilder {
    entries: HashMap<String, String, RandomState>,
}

impl EvidenceBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        EvidenceBuilder {
            entries: HashMap::default(),
        }
    }

    /// Add a single evidence entry. The key is lowercased; the value is kept
    /// verbatim. Returns `self` for chaining.
    pub fn add(mut self, key: impl AsRef<str>, value: impl Into<String>) -> Self {
        self.entries
            .insert(key.as_ref().to_lowercase(), value.into());
        self
    }

    /// Add several evidence entries from any iterator of key-value pairs.
    pub fn add_all<K, V, I>(mut self, entries: I) -> Self
    where
        K: AsRef<str>,
        V: Into<String>,
        I: IntoIterator<Item = (K, V)>,
    {
        for (key, value) in entries {
            self.entries
                .insert(key.as_ref().to_lowercase(), value.into());
        }
        self
    }

    /// Consume the builder and produce the immutable [`Evidence`].
    pub fn build(self) -> Evidence {
        Evidence {
            entries: self.entries,
        }
    }
}

/// A multi-field key built from selected evidence, for use as a cache key.
///
/// Two `DataKey`s are equal when they contain the same key-value entries in the
/// same order, so they implement [`Eq`] and [`Hash`]. The entries are stored in
/// the deterministic order produced by [`Evidence::generate_key`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DataKey {
    entries: Vec<(String, String)>,
}

impl DataKey {
    /// The ordered `(key, value)` entries that make up this key.
    pub fn entries(&self) -> &[(String, String)] {
        &self.entries
    }

    /// True if no evidence matched the filter that produced this key.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Decides which evidence keys an element or pipeline can make use of, and the
/// relative precedence of those keys.
///
/// Every flow element MUST advertise the evidence it accepts via one of these,
/// per the
/// [advertise-accepted-evidence specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/advertize-accepted-evidence.md).
/// The filter is also what derives a cache key (and the web `Vary` whitelist)
/// from a flow data's evidence.
pub trait EvidenceKeyFilter: Send + Sync {
    /// True if the given evidence key is accepted by this filter. The key is
    /// supplied lowercased.
    fn include(&self, key: &str) -> bool;

    /// The precedence order of the given key, where a lower number means higher
    /// precedence. Returns `None` if the key is not included.
    ///
    /// The default implementation returns `Some(0)` for any included key (all
    /// keys equal precedence) and `None` otherwise, which suits filters that do
    /// not care about ordering.
    fn order(&self, key: &str) -> Option<i32> {
        if self.include(key) {
            Some(0)
        } else {
            None
        }
    }

    /// The keys this filter names, lowercased, when it has a list of them,
    /// so a caller can advertise them or name the headers among them in a
    /// `Vary` header. `None` when the filter decides key by key and has no
    /// list. A filter that lists its keys may still accept further keys by
    /// pattern, as the Device Detection engine accepts any `query.51d_` value.
    fn named_keys(&self) -> Option<Vec<String>> {
        None
    }
}

/// An [`EvidenceKeyFilter`] that includes only the keys present in a fixed
/// inclusion list (a whitelist).
///
/// This is the common case described in the
/// [advertise-accepted-evidence specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/advertize-accepted-evidence.md#flow-elements):
/// an element returns the finite set of keys it understands. Keys are stored
/// lowercased so matching is case-insensitive. Each key may carry an explicit
/// precedence order; keys added without one default to `0`.
#[derive(Debug, Clone, Default)]
pub struct EvidenceKeyFilterWhitelist {
    inclusion: HashMap<String, i32, RandomState>,
}

impl EvidenceKeyFilterWhitelist {
    /// Create a whitelist from a list of keys, all at the same (default)
    /// precedence of `0`.
    pub fn new<I, S>(keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let inclusion = keys
            .into_iter()
            .map(|k| (k.as_ref().to_lowercase(), 0))
            .collect();
        EvidenceKeyFilterWhitelist { inclusion }
    }

    /// Create a whitelist from `(key, order)` pairs, where a lower order means
    /// higher precedence.
    pub fn with_orders<I, S>(keys: I) -> Self
    where
        I: IntoIterator<Item = (S, i32)>,
        S: AsRef<str>,
    {
        let inclusion = keys
            .into_iter()
            .map(|(k, order)| (k.as_ref().to_lowercase(), order))
            .collect();
        EvidenceKeyFilterWhitelist { inclusion }
    }

    /// The keys in the whitelist, with their precedence orders.
    pub fn whitelist(&self) -> impl Iterator<Item = (&str, i32)> {
        self.inclusion.iter().map(|(k, o)| (k.as_str(), *o))
    }
}

impl EvidenceKeyFilter for EvidenceKeyFilterWhitelist {
    fn include(&self, key: &str) -> bool {
        ci_get(&self.inclusion, key).is_some()
    }

    fn order(&self, key: &str) -> Option<i32> {
        ci_get(&self.inclusion, key).copied()
    }

    fn named_keys(&self) -> Option<Vec<String>> {
        Some(self.inclusion.keys().cloned().collect())
    }
}

/// An [`EvidenceKeyFilter`] that aggregates several child filters with a
/// logical OR.
///
/// A key is included if any child filter includes it. Its order is taken from
/// the first child that recognizes it. This is how a pipeline combines the
/// filters of all its elements into one pipeline-wide filter (used for the web
/// `Vary` whitelist).
#[derive(Default)]
pub struct EvidenceKeyFilterAggregator {
    filters: Vec<Box<dyn EvidenceKeyFilter>>,
}

impl EvidenceKeyFilterAggregator {
    /// Create an empty aggregator.
    pub fn new() -> Self {
        EvidenceKeyFilterAggregator {
            filters: Vec::new(),
        }
    }

    /// Add a child filter.
    pub fn add_filter(&mut self, filter: Box<dyn EvidenceKeyFilter>) {
        self.filters.push(filter);
    }
}

impl EvidenceKeyFilter for EvidenceKeyFilterAggregator {
    fn include(&self, key: &str) -> bool {
        self.filters.iter().any(|f| f.include(key))
    }

    fn order(&self, key: &str) -> Option<i32> {
        self.filters.iter().find_map(|f| f.order(key))
    }

    /// The union of the children's named keys, or `None` when any child has
    /// no list, because a union missing one child's keys would misname what
    /// the aggregate accepts.
    fn named_keys(&self) -> Option<Vec<String>> {
        let mut keys = Vec::new();
        for filter in &self.filters {
            keys.extend(filter.named_keys()?);
        }
        keys.sort();
        keys.dedup();
        Some(keys)
    }
}

#[cfg(test)]
mod tests {
    use std::hash::BuildHasher;

    use super::*;

    /// Evidence made for a parallel group copies the hasher of the evidence
    /// it was made from, so a key hashes the same through both, where a
    /// freshly seeded hasher hashes it differently.
    #[test]
    fn empty_with_same_hasher_copies_the_hasher() {
        const KEY: &str = "header.user-agent";
        let evidence = Evidence::builder().add(KEY, "A").build();
        let hash = evidence.entries.hasher().hash_one(KEY);
        assert_ne!(RandomState::new().hash_one(KEY), hash);

        let copy = evidence.empty_with_same_hasher();
        assert_eq!(copy.entries.hasher().hash_one(KEY), hash);
    }
}
