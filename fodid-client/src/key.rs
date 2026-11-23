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

//! The published signing keys, and choosing the one an identifier was made
//! under.

use chrono::{DateTime, Duration, Utc};

use crate::error::{Error, Result};

/// One published signing key, the moment it comes into force and, where the
/// key route gave one, the moment it stops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DidPublicKey {
    starts_at: DateTime<Utc>,
    ends_at: Option<DateTime<Utc>>,
    public_key_pem: String,
}

impl DidPublicKey {
    /// Creates a key entry with no end, which stays in force until the next
    /// entry starts. [`DidPublicKey::with_ends_at`] gives it one.
    pub fn new(starts_at: DateTime<Utc>, public_key_pem: impl Into<String>) -> Self {
        Self {
            starts_at,
            ends_at: None,
            public_key_pem: public_key_pem.into(),
        }
    }

    /// The same entry with the moment it stops being in force, being the
    /// `endsAt` the key route gives, which [`parse_keys`] requires to be
    /// after the start.
    pub fn with_ends_at(mut self, ends_at: DateTime<Utc>) -> Self {
        self.ends_at = Some(ends_at);
        self
    }

    /// The moment this key comes into force. It stays in force until its end,
    /// or until the next entry starts where it has none.
    pub fn starts_at(&self) -> DateTime<Utc> {
        self.starts_at
    }

    /// The moment this key stops being in force, or `None` where the key
    /// route gave no end.
    ///
    /// The end is the start of the key scheduled after this one, and the
    /// newest key carries it although that key is not published until its
    /// period starts. A key may be replaced before its end, for example if it
    /// is compromised, and the key route then gives it an earlier end, being
    /// the start of its replacement.
    pub fn ends_at(&self) -> Option<DateTime<Utc>> {
        self.ends_at
    }

    /// The public key in SPKI PEM form, as the OWID verification takes it.
    pub fn public_key_pem(&self) -> &str {
        &self.public_key_pem
    }
}

/// How far either side of a boundary a creation moment is still treated as
/// belonging to the neighbouring key.
///
/// A creating and a verifying node do not share a clock, so an identifier made
/// within a few minutes of a boundary can be dated on one side by one and the
/// other side by the other. Trying the neighbour is what stops ordinary skew
/// reading as a bad signature.
pub const BOUNDARY_TOLERANCE_MINUTES: i64 = 15;

/// The key in force at the given moment, being the entry whose start is latest
/// on or before it, or `None` when the moment precedes the whole schedule or
/// is at or after that entry's end.
///
/// The keys need not be sorted. A caller that holds its own list, rather than
/// using [`DidClient`](crate::DidClient), keeps it current with [`covers`],
/// which says when to fetch the list again, and [`merge_keys`], which adds
/// the answer to it.
pub fn in_force_at(keys: &[DidPublicKey], at: DateTime<Utc>) -> Option<&DidPublicKey> {
    keys.iter()
        .filter(|k| k.starts_at <= at)
        .max_by_key(|k| k.starts_at)
        .filter(|k| k.ends_at.is_none_or(|end| at < end))
}

/// The keys to try for the given moment, best first.
///
/// That is the key in force at the moment, followed by a neighbouring entry
/// only where the moment sits within [`BOUNDARY_TOLERANCE_MINUTES`] of it.
/// Progressively older keys are NOT tried, because trying every key held would
/// turn a signature made under a key nobody holds into a signature that
/// eventually matches something.
pub fn candidates_for_date(keys: &[DidPublicKey], at: DateTime<Utc>) -> Vec<&DidPublicKey> {
    let tolerance = Duration::minutes(BOUNDARY_TOLERANCE_MINUTES);
    let mut out: Vec<&DidPublicKey> = Vec::with_capacity(2);
    for candidate in [
        in_force_at(keys, at),
        in_force_at(keys, at - tolerance),
        in_force_at(keys, at + tolerance),
    ]
    .into_iter()
    .flatten()
    {
        if !out.iter().any(|k| std::ptr::eq(*k, candidate)) {
            out.push(candidate);
        }
    }
    out
}

/// Whether a held key list answers for a 51Did created at the given moment
/// without being fetched again.
///
/// The list covers every moment earlier than its end less
/// [`BOUNDARY_TOLERANCE_MINUTES`]. Its end is the end of its newest entry, or
/// that entry's start where it has none. Nearer the end than that, the key
/// after it may be a candidate, and the list does not hold that key. An empty
/// list covers nothing.
///
/// Where this is false, fetch the key route again with the newest start held
/// as `datetime`, for example with
/// [`DidClient::fetch_keys_from`](crate::DidClient::fetch_keys_from), add
/// the answer with [`merge_keys`], and then choose the keys with
/// [`candidates_for_date`]. Fetch for this reason at most once a
/// minute, so that a 51Did dated in a period not published yet, or given a
/// false date, cannot send every lookup to the cloud. A list that covers the
/// moment is still fetched again when a signature fails under every
/// candidate, as [`merge_keys`] says, and as a whole, with no `datetime`,
/// when it is [`KEY_CACHE_LIFETIME`](crate::KEY_CACHE_LIFETIME) old. Only a
/// fetch of the whole list resets the list's age, and the minute never holds
/// one back.
///
/// # Example
///
/// ```
/// use chrono::{DateTime, Duration, Utc};
/// use fodid_client::{covers, in_force_at, merge_keys, parse_keys};
///
/// let mut held = parse_keys(
///     r#"[{"startsAt":"2026-09-21T00:00:00Z",
///          "endsAt":"2026-09-28T00:00:00Z","publicKey":"first"}]"#,
/// )?;
/// let monday: DateTime<Utc> = "2026-09-28T09:00:00Z".parse().unwrap();
/// assert!(covers(&held, monday - Duration::days(1)));
///
/// // Not covered, so fetch with datetime=2026-09-21T00:00:00Z and merge.
/// assert!(!covers(&held, monday));
/// let answer = parse_keys(
///     r#"[{"startsAt":"2026-09-21T00:00:00Z",
///          "endsAt":"2026-09-28T00:00:00Z","publicKey":"first"},
///         {"startsAt":"2026-09-28T00:00:00Z",
///          "endsAt":"2026-10-05T00:00:00Z","publicKey":"second"}]"#,
/// )?;
/// merge_keys(&mut held, answer);
/// assert!(covers(&held, monday));
/// assert_eq!(in_force_at(&held, monday).unwrap().public_key_pem(), "second");
/// # Ok::<(), fodid_client::Error>(())
/// ```
pub fn covers(keys: &[DidPublicKey], at: DateTime<Utc>) -> bool {
    let Some(newest) = keys.iter().max_by_key(|k| k.starts_at) else {
        return false;
    };
    let end = newest.ends_at.unwrap_or(newest.starts_at);
    at.checked_add_signed(Duration::minutes(BOUNDARY_TOLERANCE_MINUTES))
        .is_some_and(|near| near < end)
}

/// Adds a key route answer to a held list, matching entries by start.
///
/// An entry of the answer replaces the held entry with the same start,
/// because a later answer can give an end, or an earlier end, that the held
/// copy lacks, and is added where none is held. No held entry is dropped,
/// because a 51Did made long ago verifies against the key of its own period.
/// The list is left sorted by start.
///
/// When a signature fails under every candidate held, fetch the list again
/// with the start of the key held for the 51Did's date as `datetime`, being
/// the newest held key starting at or before that date, merge the answer,
/// and check once more before reporting a failure, within the same once a
/// minute limit as [`covers`] describes. A key may be replaced before its
/// end, and the answer then carries its entry with the earlier end and the
/// replacement, which starts inside its period. [`covers`] says when else
/// to fetch.
pub fn merge_keys(held: &mut Vec<DidPublicKey>, answer: impl IntoIterator<Item = DidPublicKey>) {
    for entry in answer {
        held.retain(|k| k.starts_at != entry.starts_at);
        held.push(entry);
    }
    held.sort_by_key(|k| k.starts_at);
}

/// Reads the key endpoint's answer, which is a JSON array of entries carrying
/// `startsAt` (or `created`, the older spelling), `publicKey` and, where the
/// service gives it, `endsAt`. An entry that lacks its start or its key, or
/// whose end is not after its start, makes the whole answer unreadable, so
/// nothing from it is merged.
///
/// The result is sorted by start, so [`in_force_at`] and
/// [`candidates_for_date`] read it in the order they expect however the
/// service happened to order it.
pub fn parse_keys(json: &str) -> Result<Vec<DidPublicKey>> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| {
        Error::Protocol(format!(
            "the 51Did key endpoint did not answer with JSON: {e}"
        ))
    })?;
    let array = value.as_array().ok_or_else(|| {
        Error::Protocol("the 51Did key endpoint did not answer with a JSON array".to_string())
    })?;

    let mut keys = Vec::with_capacity(array.len());
    for entry in array {
        let start = entry
            .get("startsAt")
            .and_then(|v| v.as_str())
            .or_else(|| entry.get("created").and_then(|v| v.as_str()));
        let pem = entry.get("publicKey").and_then(|v| v.as_str());
        match (start, pem) {
            (Some(start), Some(pem)) => {
                let start = parse_utc(start)?;
                let key = DidPublicKey::new(start, pem);
                keys.push(match read_end(entry)? {
                    Some(end) if end <= start => {
                        return Err(Error::Protocol(
                            "a 51Did key entry does not end after it starts".to_string(),
                        ))
                    }
                    Some(end) => key.with_ends_at(end),
                    None => key,
                });
            }
            _ => {
                return Err(Error::Protocol(
                    "a 51Did key entry lacks its start or its public key".to_string(),
                ))
            }
        }
    }
    keys.sort_by_key(|k| k.starts_at);
    Ok(keys)
}

/// The entry's `endsAt`, or `None` where it has none or it is null. An entry
/// without an end is valid, and is in force until the next entry starts.
fn read_end(entry: &serde_json::Value) -> Result<Option<DateTime<Utc>>> {
    match entry.get("endsAt") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(end)) => parse_utc(end).map(Some),
        Some(_) => Err(Error::Protocol(
            "a 51Did key entry's end is not a time".to_string(),
        )),
    }
}

/// Reads one of the timestamp forms the key endpoint uses.
pub(crate) fn parse_utc(value: &str) -> Result<DateTime<Utc>> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Ok(parsed.with_timezone(&Utc));
    }
    // The endpoint has also written a bare "YYYY-MM-DDTHH:MM:SS" with no zone,
    // which is UTC by the service's own definition.
    chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .map(|naive| naive.and_utc())
        .map_err(|_| Error::Protocol(format!("'{value}' is not a time this client can read")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(day: u32) -> DateTime<Utc> {
        chrono::NaiveDate::from_ymd_opt(2026, 9, day)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
    }

    fn schedule() -> Vec<DidPublicKey> {
        vec![
            DidPublicKey::new(at(1), "first"),
            DidPublicKey::new(at(8), "second"),
            DidPublicKey::new(at(15), "third"),
        ]
    }

    fn ending(start: u32, end: u32, pem: &str) -> DidPublicKey {
        DidPublicKey::new(at(start), pem).with_ends_at(at(end))
    }

    fn tolerance() -> Duration {
        Duration::minutes(BOUNDARY_TOLERANCE_MINUTES)
    }

    #[test]
    fn in_force_takes_the_latest_start_on_or_before() {
        let keys = schedule();
        assert_eq!(
            in_force_at(&keys, at(10)).unwrap().public_key_pem(),
            "second"
        );
        assert_eq!(
            in_force_at(&keys, at(8)).unwrap().public_key_pem(),
            "second"
        );
    }

    #[test]
    fn a_date_before_the_schedule_has_no_key() {
        assert!(in_force_at(&schedule(), at(1) - Duration::days(1)).is_none());
    }

    #[test]
    fn a_moment_at_a_boundary_tries_both_sides() {
        let keys = schedule();
        let candidates = candidates_for_date(&keys, at(8));
        assert_eq!(candidates.len(), 2, "the neighbour is tried too");
        assert_eq!(candidates[0].public_key_pem(), "second", "best first");
        assert_eq!(candidates[1].public_key_pem(), "first");
    }

    #[test]
    fn a_moment_well_inside_a_period_tries_one() {
        let keys = schedule();
        assert_eq!(candidates_for_date(&keys, at(10)).len(), 1);
    }

    #[test]
    fn a_key_is_not_in_force_at_or_after_its_end() {
        let keys = vec![ending(1, 8, "first")];
        assert_eq!(in_force_at(&keys, at(7)).unwrap().public_key_pem(), "first");
        assert!(in_force_at(&keys, at(8)).is_none(), "the end is not in it");
        assert!(in_force_at(&keys, at(9)).is_none());
    }

    #[test]
    fn a_moment_just_past_the_end_still_tries_the_key_before_it() {
        let keys = vec![ending(1, 8, "first")];
        let just_past = at(8) + tolerance() - Duration::seconds(1);
        assert_eq!(candidates_for_date(&keys, just_past).len(), 1);
        assert!(
            candidates_for_date(&keys, at(8) + tolerance()).is_empty(),
            "no key held covers a moment further past the end"
        );
    }

    #[test]
    fn a_list_covers_until_the_tolerance_before_its_newest_end() {
        let keys = vec![ending(1, 8, "first"), ending(8, 15, "second")];
        let edge = at(15) - tolerance();
        assert!(covers(&keys, edge - Duration::seconds(1)));
        assert!(!covers(&keys, edge), "at the end less the tolerance");
        assert!(!covers(&keys, at(16)));
        assert!(
            covers(&keys, at(1) - Duration::days(1)),
            "a fetch only adds later entries, so an earlier moment is answered"
        );
    }

    #[test]
    fn without_ends_the_newest_start_is_the_end() {
        let keys = schedule();
        let edge = at(15) - tolerance();
        assert!(covers(&keys, edge - Duration::seconds(1)));
        assert!(!covers(&keys, edge));
    }

    #[test]
    fn an_empty_list_covers_nothing_and_the_far_future_does_not_overflow() {
        assert!(!covers(&[], at(1)));
        assert!(!covers(&schedule(), DateTime::<Utc>::MAX_UTC));
    }

    #[test]
    fn merging_replaces_by_start_adds_later_entries_and_keeps_earlier_ones() {
        let mut held = vec![
            DidPublicKey::new(at(8), "second"),
            DidPublicKey::new(at(1), "first"),
        ];
        merge_keys(
            &mut held,
            vec![ending(15, 22, "third"), ending(8, 15, "second")],
        );
        assert_eq!(
            held,
            vec![
                DidPublicKey::new(at(1), "first"),
                ending(8, 15, "second"),
                ending(15, 22, "third"),
            ],
            "the copy with an end replaced the one without, sorted by start"
        );
    }

    #[test]
    fn merging_a_replacement_moves_the_old_end_earlier() {
        let mut held = vec![ending(1, 8, "first")];
        merge_keys(
            &mut held,
            vec![ending(1, 4, "first"), ending(4, 8, "replacement")],
        );
        assert_eq!(held.len(), 2);
        assert_eq!(in_force_at(&held, at(3)).unwrap().public_key_pem(), "first");
        assert_eq!(
            in_force_at(&held, at(5)).unwrap().public_key_pem(),
            "replacement"
        );
    }

    #[test]
    fn keys_are_read_and_sorted() {
        let keys = parse_keys(
            r#"[{"startsAt":"2026-09-08T00:00:00Z","publicKey":"b"},
                {"startsAt":"2026-09-01T00:00:00Z","publicKey":"a"}]"#,
        )
        .unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].public_key_pem(), "a", "sorted by start");
    }

    #[test]
    fn the_older_created_spelling_is_read() {
        let keys = parse_keys(r#"[{"created":"2026-09-01T00:00:00Z","publicKey":"a"}]"#).unwrap();
        assert_eq!(keys[0].starts_at(), at(1));
    }

    #[test]
    fn the_end_is_read_in_the_form_the_service_writes() {
        let keys = parse_keys(
            r#"[{"startsAt":"2026-09-01T00:00:00.0000000Z",
                 "endsAt":"2026-09-08T00:00:00.0000000Z","publicKey":"a"}]"#,
        )
        .unwrap();
        assert_eq!(keys[0].starts_at(), at(1));
        assert_eq!(keys[0].ends_at(), Some(at(8)));
    }

    #[test]
    fn an_entry_without_an_end_is_valid() {
        let keys = parse_keys(
            r#"[{"startsAt":"2026-09-01T00:00:00Z","publicKey":"a"},
                {"startsAt":"2026-09-08T00:00:00Z","endsAt":null,
                 "publicKey":"b"}]"#,
        )
        .unwrap();
        assert_eq!(keys[0].ends_at(), None);
        assert_eq!(keys[1].ends_at(), None);
    }

    #[test]
    fn an_end_not_after_its_start_makes_the_answer_unreadable() {
        for end in ["2026-09-01T00:00:00Z", "2026-08-31T00:00:00Z"] {
            let json = format!(
                r#"[{{"startsAt":"2026-08-25T00:00:00Z","publicKey":"a"}},
                    {{"startsAt":"2026-09-01T00:00:00Z","endsAt":"{end}",
                      "publicKey":"b"}}]"#
            );
            assert!(
                matches!(parse_keys(&json), Err(Error::Protocol(_))),
                "{end}"
            );
        }
    }

    #[test]
    fn an_end_that_is_not_a_time_is_refused() {
        for end in [r#""soon""#, "7"] {
            let json = format!(
                r#"[{{"startsAt":"2026-09-01T00:00:00Z","endsAt":{end},
                     "publicKey":"a"}}]"#
            );
            assert!(parse_keys(&json).is_err(), "{end}");
        }
    }

    #[test]
    fn an_entry_missing_its_key_is_refused() {
        assert!(parse_keys(r#"[{"startsAt":"2026-09-01T00:00:00Z"}]"#).is_err());
    }

    #[test]
    fn an_answer_that_is_not_an_array_is_refused() {
        assert!(parse_keys(r#"{"startsAt":"2026-09-01T00:00:00Z"}"#).is_err());
    }
}
