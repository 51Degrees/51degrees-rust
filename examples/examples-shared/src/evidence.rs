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

//! Sample evidence values examples and tests reuse.
//!
//! A fixed set of inputs that exercise the common detection paths (a desktop and
//! a mobile User-Agent, a full User-Agent Client Hints header set, a base64
//! `getHighEntropyValues` blob, and sample IP addresses for IP Intelligence).
//! Each set is a list of `(evidence-key, value)` pairs ready to feed into an
//! [`fiftyone_pipeline_core::EvidenceBuilder`].

use fiftyone_pipeline_core::{Evidence, EvidenceBuilder};

/// A typical desktop browser User-Agent (Chrome on 64-bit Windows).
pub const DESKTOP_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
     (KHTML, like Gecko) Chrome/78.0.3904.108 Safari/537.36";

/// A typical mobile browser User-Agent (Samsung Internet on Android).
pub const MOBILE_USER_AGENT: &str =
    "Mozilla/5.0 (Linux; Android 9; SAMSUNG SM-G960U) AppleWebKit/537.36 \
     (KHTML, like Gecko) SamsungBrowser/10.1 Chrome/71.0.3578.99 Mobile Safari/537.36";

/// A base64-encoded JSON `getHighEntropyValues()` result, supplied under the
/// `query.51d_gethighentropyvalues` evidence key.
///
/// Decoded it describes a macOS Chrome 131 client. The engine consumes it as
/// either a query or cookie parameter and converts it internally to the
/// HTTP-header UACH representation.
pub const GET_HIGH_ENTROPY_VALUES: &str = "eyJhcmNoaXRlY3R1cmUiOiJhcm0iLCJicmFuZHMiOlt7ImJyYW5kIjoiR29vZ2xlIENocm9tZSIsInZlcnNpb24iOiIxMzEifSx7ImJyYW5kIjoiQ2hyb21pdW0iLCJ2ZXJzaW9uIjoiMTMxIn0seyJicmFuZCI6Ik5vdF9BIEJyYW5kIiwidmVyc2lvbiI6IjI0In1dLCJmdWxsVmVyc2lvbkxpc3QiOlt7ImJyYW5kIjoiR29vZ2xlIENocm9tZSIsInZlcnNpb24iOiIxMzEuMC42Nzc4LjE0MCJ9LHsiYnJhbmQiOiJDaHJvbWl1bSIsInZlcnNpb24iOiIxMzEuMC42Nzc4LjE0MCJ9LHsiYnJhbmQiOiJOb3RfQSBCcmFuZCIsInZlcnNpb24iOiIyNC4wLjAuMCJ9XSwibW9iaWxlIjpmYWxzZSwibW9kZWwiOiIiLCJwbGF0Zm9ybSI6Im1hY09TIiwicGxhdGZvcm1WZXJzaW9uIjoiMTUuMS4xIn0=";

/// A sample IPv4 address for IP Intelligence examples (a 51Degrees test
/// address).
pub const SAMPLE_IPV4: &str = "185.28.167.77";

/// A sample IPv6 address for IP Intelligence examples.
pub const SAMPLE_IPV6: &str = "2001:4860:4860::8888";

/// The desktop User-Agent as an evidence pair list.
pub fn desktop_user_agent_evidence() -> Vec<(&'static str, &'static str)> {
    vec![("header.user-agent", DESKTOP_USER_AGENT)]
}

/// The mobile User-Agent as an evidence pair list.
pub fn mobile_user_agent_evidence() -> Vec<(&'static str, &'static str)> {
    vec![("header.user-agent", MOBILE_USER_AGENT)]
}

/// A full User-Agent Client Hints header set for a Windows 11 Chrome client.
///
/// A combination of `Sec-CH-UA*` headers (plus the base User-Agent) that
/// exercises the UACH detection path.
pub fn uach_header_evidence() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "header.user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/98.0.4758.102 Safari/537.36",
        ),
        ("header.sec-ch-ua-mobile", "?0"),
        (
            "header.sec-ch-ua",
            "\" Not A; Brand\";v=\"99\", \"Chromium\";v=\"98\", \"Google Chrome\";v=\"98\"",
        ),
        ("header.sec-ch-ua-platform", "\"Windows\""),
        ("header.sec-ch-ua-platform-version", "\"14.0.0\""),
    ]
}

/// The base64 `getHighEntropyValues` blob as an evidence pair list, supplied as
/// a query parameter.
pub fn get_high_entropy_values_evidence() -> Vec<(&'static str, &'static str)> {
    vec![("query.51d_gethighentropyvalues", GET_HIGH_ENTROPY_VALUES)]
}

/// A sample IPv4 client-IP evidence pair list for IP Intelligence.
pub fn ipv4_evidence() -> Vec<(&'static str, &'static str)> {
    vec![("query.client-ip", SAMPLE_IPV4)]
}

/// A sample IPv6 client-IP evidence pair list for IP Intelligence.
pub fn ipv6_evidence() -> Vec<(&'static str, &'static str)> {
    vec![("query.client-ip", SAMPLE_IPV6)]
}

/// Every sample evidence set, so an example can iterate over a representative
/// spread of inputs.
///
/// Each entry is a `(label, pairs)` tuple: the label names the scenario for
/// display, and the pairs are ready to add to an [`EvidenceBuilder`].
pub fn all_sample_evidence() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
        ("Mobile User-Agent", mobile_user_agent_evidence()),
        ("Desktop User-Agent", desktop_user_agent_evidence()),
        ("UACH headers", uach_header_evidence()),
        (
            "getHighEntropyValues blob",
            get_high_entropy_values_evidence(),
        ),
        ("IPv4 address", ipv4_evidence()),
        ("IPv6 address", ipv6_evidence()),
    ]
}

/// Build an [`Evidence`] instance from a list of `(key, value)` pairs, such as
/// any of the sample sets in this module.
pub fn evidence_from_pairs<I, K, V>(pairs: I) -> Evidence
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: Into<String>,
{
    EvidenceBuilder::new().add_all(pairs).build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    #[test]
    fn high_entropy_blob_is_valid_base64_json() {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(GET_HIGH_ENTROPY_VALUES)
            .expect("the sample blob should be valid base64");
        let text = String::from_utf8(decoded).expect("decoded blob should be UTF-8");
        assert!(text.contains("\"platform\":\"macOS\""));
    }

    #[test]
    fn uach_set_has_expected_keys() {
        let pairs = uach_header_evidence();
        assert!(pairs.iter().any(|(k, _)| *k == "header.sec-ch-ua-mobile"));
        assert!(pairs
            .iter()
            .any(|(k, _)| *k == "header.sec-ch-ua-platform-version"));
    }

    #[test]
    fn builds_evidence_from_a_sample_set() {
        let ev = evidence_from_pairs(desktop_user_agent_evidence());
        assert_eq!(ev.get("header.user-agent"), Some(DESKTOP_USER_AGENT));
    }

    #[test]
    fn all_sample_evidence_covers_every_scenario() {
        let all = all_sample_evidence();
        assert_eq!(all.len(), 6);
        // Each set must produce non-empty evidence.
        for (label, pairs) in all {
            let ev = evidence_from_pairs(pairs);
            assert!(!ev.is_empty(), "scenario '{label}' produced no evidence");
        }
    }
}
