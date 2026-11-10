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

//! Constants for the 51Degrees-specific pipeline elements.
//!
//! These fix the evidence key spellings, the default usage-sharing endpoint and
//! the usage-sharing defaults.

use fiftyone_pipeline_core::constants::{
    EVIDENCE_QUERY_PREFIX, EVIDENCE_SEPARATOR, FIFTYONE_COOKIE_PREFIX,
};

/// The suffix for the session id evidence populated and used by the
/// [`crate::SequenceElement`]. The full key is [`EVIDENCE_SESSIONID`].
pub const EVIDENCE_SESSIONID_SUFFIX: &str = "session-id";

/// The suffix for the sequence evidence populated and used by the
/// [`crate::SequenceElement`]. The full key is [`EVIDENCE_SEQUENCE`].
pub const EVIDENCE_SEQUENCE_SUFFIX: &str = "sequence";

/// The complete `query.session-id` evidence key.
///
/// This is a compile-time concatenation of the query prefix, the evidence
/// separator and [`EVIDENCE_SESSIONID_SUFFIX`].
pub const EVIDENCE_SESSIONID: &str = concatcp_session();

/// The complete `query.sequence` evidence key.
pub const EVIDENCE_SEQUENCE: &str = concatcp_sequence();

// `concat!` only accepts literals, so the composite keys are spelled out as
// const functions that assert they match the parts. Keeping them as `const`
// keeps the keys usable in array initialisers and match arms.
const fn concatcp_session() -> &'static str {
    // query + "." + "session-id"
    assert!(str_eq(EVIDENCE_QUERY_PREFIX, "query"));
    assert!(str_eq(EVIDENCE_SEPARATOR, "."));
    "query.session-id"
}

const fn concatcp_sequence() -> &'static str {
    assert!(str_eq(EVIDENCE_QUERY_PREFIX, "query"));
    assert!(str_eq(EVIDENCE_SEPARATOR, "."));
    "query.sequence"
}

/// Compile-time string equality, used by the composite-key assertions so that
/// the spelled-out constants cannot silently drift from the core prefixes.
const fn str_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// The default element data key for the sequence element.
pub const SEQUENCE_DEFAULT_ELEMENT_DATA_KEY: &str = "sequence";

/// The default element data key for the set-headers element.
pub const SET_HEADERS_DEFAULT_ELEMENT_DATA_KEY: &str = "set-headers";

/// The default element data key for the share-usage element.
pub const SHARE_USAGE_DEFAULT_ELEMENT_DATA_KEY: &str = "shareusage";

/// The property name prefix that marks a property whose value should be written
/// to an HTTP response header, per the set-headers naming convention.
pub const SET_HEADER_PROPERTY_PREFIX: &str = "SetHeader";

/// The 51Degrees cookie/query prefix. Re-exported from the core crate so the
/// usage-sharing filter need not import the core constant directly.
pub const FIFTYONE_PREFIX: &str = FIFTYONE_COOKIE_PREFIX;

/// The HTTP cookie header name. Blocked from usage sharing by default so that
/// raw cookie blobs are never shared.
pub const EVIDENCE_HTTPHEADER_COOKIE_SUFFIX: &str = "cookie";

/// The most characters of an evidence value that usage sharing writes,
/// counted as Unicode code points before any character is replaced. A longer
/// value is cut to this length and its entry marked `truncated="true"`.
pub const SHARE_USAGE_MAX_EVIDENCE_LENGTH: usize = 10_000;

/// The default maximum size of the usage-sharing queue.
pub const SHARE_USAGE_DEFAULT_MAX_QUEUE_SIZE: usize = 1000;

/// The default timeout in milliseconds used when adding to the queue.
pub const SHARE_USAGE_DEFAULT_ADD_TIMEOUT_MS: u64 = 5;

/// The default take timeout in milliseconds, how long the usage-sharing
/// element waits for another entry once a message holds the minimum entries.
pub const SHARE_USAGE_DEFAULT_TAKE_TIMEOUT_MS: u64 = 100;

/// The default repeat-evidence interval in minutes (the sliding-window size).
pub const SHARE_USAGE_DEFAULT_REPEAT_EVIDENCE_INTERVAL_MINUTES: u64 = 20;

/// The default share percentage. `1.0` means every request is shared.
pub const SHARE_USAGE_DEFAULT_SHARE_PERCENTAGE: f64 = 1.0;

/// The default number of entries per usage-sharing message. No data is sent
/// until this many entries are queued, except when the element is dropped.
pub const SHARE_USAGE_DEFAULT_MIN_ENTRIES_PER_MESSAGE: usize = 50;

/// The default usage-sharing endpoint.
pub const SHARE_USAGE_DEFAULT_URL: &str = "https://devices-v4.51degrees.com/new.ashx";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_keys_are_correct() {
        assert_eq!(EVIDENCE_SESSIONID, "query.session-id");
        assert_eq!(EVIDENCE_SEQUENCE, "query.sequence");
    }

    #[test]
    fn fiftyone_prefix_matches_core() {
        assert_eq!(FIFTYONE_PREFIX, "51d_");
    }
}
