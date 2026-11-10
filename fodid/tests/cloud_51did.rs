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

//! Live integration test: obtains a real 51Did from the 51Degrees cloud and
//! checks that it parses into a [`FodId`].
//!
//! The test uses a single resource key from the environment. Set
//! `_51DEGREES_RESOURCE_KEY` (or one of the CI-exported tiered names
//! `_51DEGREES_RESOURCE_KEY_BESPOKE` / `_51DEGREES_RESOURCE_KEY_BASIC`) to a key
//! whose properties include `fodid.*`. Create one with the 51Degrees Configurator
//! sharing link <https://configure.51degrees.com/N57Wygby> (the free tier now
//! includes 51Did).
//!
//! This is a live test that needs the key plus network access, so it is marked
//! `#[ignore]` and is skipped by a plain `cargo test`. Run it explicitly with
//! `--include-ignored`:
//!
//! ```text
//! # set the key as an environment variable, then run:
//! #   PowerShell: $env:_51DEGREES_RESOURCE_KEY = '<key>'
//! #   bash:       export _51DEGREES_RESOURCE_KEY=<key>
//! cargo test -p fodid --test cloud_51did -- --include-ignored
//!
//! # or supply it inline for a single run:
//! _51DEGREES_RESOURCE_KEY=<key> cargo test -p fodid --test cloud_51did -- --include-ignored
//! ```
//!
//! Run without a key it fails with those instructions instead of passing
//! silently.
//!
//! To exercise more than one key (for example a free key and a paid key), the
//! CI workflow runs this test once per `_51DEGREES_RESOURCE_KEY*` secret,
//! setting `_51DEGREES_RESOURCE_KEY` to each in turn. The test itself only ever
//! reads the single variable.
//!
//! For each key the test checks the cloud `id.usage` levels. `non-marketing` is
//! available on any key that includes `fodid.*`, so it is required. `standard`
//! and `personalized` are marketing usages that paid keys are expected to grant
//! in due course; they are validated when the key returns them and reported
//! when it does not, so this test starts covering them automatically once a
//! paid key is expanded for marketing.

use fodid::{FodId, IdType, Usage};

mod layout;

/// The resource-key environment variable names, in the workspace's resolution
/// order: `_51DEGREES_RESOURCE_KEY` first, then the CI-exported bespoke and
/// basic tiered names. Mirrors `examples-shared::keys::resource_key_from_env`.
const RESOURCE_KEY_ENVS: [&str; 3] = [
    "_51DEGREES_RESOURCE_KEY",
    "_51DEGREES_RESOURCE_KEY_BESPOKE",
    "_51DEGREES_RESOURCE_KEY_BASIC",
];

/// The 51Degrees cloud V4 JSON endpoint.
const CLOUD_JSON_URL: &str = "https://cloud.51degrees.com/api/v4/json";

/// A representative mobile User-Agent. The cloud needs Device Detection
/// evidence plus a client IP to derive a 51Did.
const USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

/// A client IP for the request. 203.0.113.0/24 is the TEST-NET-3 range
/// reserved for documentation (RFC 5737).
const CLIENT_IP: &str = "203.0.113.42";

/// A cloud `id.usage` level, whether every resource key must return a 51Did
/// for it, and what the identifier must then say about itself.
struct UsageCase {
    /// The `id.usage` request value.
    name: &'static str,
    /// Whether a 51Did is required for this usage. Only `non-marketing` is
    /// required today; the marketing usages are validated when returned.
    required: bool,
    /// The usage the reader must answer with.
    expected: Usage,
    /// The terms address the identifier must carry, or `None` where it must
    /// state none. A non-marketing identifier may not reach a demand source
    /// at all, so there is nothing for a receiver to agree to.
    terms: Option<&'static str>,
}

/// The versioned Model Terms for Marketing document a marketing 51Did is
/// created under.
///
/// Written out here rather than read from the crate, because a test that
/// asked the crate what it expects would agree with itself whatever the
/// crate said. The literal is what a receiver has to be able to fetch.
const MODEL_TERMS_FOR_MARKETING_2: &str = "https://m4ow.uk/mtm/2.txt";

/// IAB TCF v2 consent strings, and the usage the service must derive from
/// each without the caller stating one.
///
/// The first sets all twelve purposes, which is personalized. The second
/// sets the Appendix 1 standard set, being purposes 1, 2, 7, 8 and 11, which
/// is standard. Both are the strings the cloud's own IabTcfElement tests use,
/// repeated here rather than shared, for the same reason as the address
/// above.
const CONSENT_STRINGS: &[(&str, Usage)] = &[
    ("AAAAAAAAAAAAAAAAAAAAAAAAAP_w", Usage::Personalized),
    ("AAAAAAAAAAAAAAAAAAAAAAAAAMMg", Usage::Standard),
];

/// The usage levels checked for the resource key. Ordered with the required
/// `non-marketing` usage first.
const USAGES: &[UsageCase] = &[
    UsageCase {
        name: "non-marketing",
        required: true,
        expected: Usage::NonMarketing,
        terms: None,
    },
    UsageCase {
        name: "standard",
        required: false,
        expected: Usage::Standard,
        terms: Some(MODEL_TERMS_FOR_MARKETING_2),
    },
    UsageCase {
        name: "personalized",
        required: false,
        expected: Usage::Personalized,
        terms: Some(MODEL_TERMS_FOR_MARKETING_2),
    },
];

/// Returns the resource key from the environment, trying the aligned name
/// first and then the CI-exported bespoke and basic tiered names. Returns `None`
/// when none is set.
fn resource_key() -> Option<String> {
    RESOURCE_KEY_ENVS.into_iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .filter(|key| !key.trim().is_empty())
    })
}

/// Calls the cloud JSON endpoint for the given `id.usage` and returns the
/// parsed response body.
fn request_usage(resource_key: &str, usage: &str) -> serde_json::Value {
    request_with(resource_key, "id.usage", usage)
}

/// Calls the cloud JSON endpoint with one extra query parameter and returns
/// the parsed response body.
fn request_with(resource_key: &str, name: &str, value: &str) -> serde_json::Value {
    let body = ureq::get(CLOUD_JSON_URL)
        .query("resource", resource_key)
        .query("user-agent", USER_AGENT)
        .query("client-ip", CLIENT_IP)
        .query(name, value)
        .call()
        .unwrap_or_else(|e| panic!("cloud request for {name}={value} should succeed: {e}"))
        .into_string()
        .expect("cloud response should be readable");
    serde_json::from_str(&body).expect("cloud response should be JSON")
}

/// A consent management platform sends an IAB TCF consent string and no usage
/// of its own. The service decodes the string, decides the usage from the
/// purposes it grants, and records in the identifier that it did so, which is
/// bit 3 of the flags byte.
///
/// This is the half a caller cannot state for itself. An identifier whose
/// usage was stated in the request and one whose usage was decoded from a
/// consent string are both legitimate, and they are different assertions
/// about how the permission was obtained, so a receiver has to be able to
/// tell them apart. The service signs the answer, and this proves the two
/// ends agree about which bit it is and which way round it reads.
///
/// Marked `#[ignore]` for the same reason as the test above.
#[test]
#[ignore = "live cloud test: set _51DEGREES_RESOURCE_KEY and run with `--include-ignored` (see module docs)"]
fn consent_string_sets_the_usage_is_indirect_bit() {
    let Some(resource_key) = resource_key() else {
        panic!(
            "no resource key found for the live cloud 51Did test. See the \
             message on resource_key_returns_51did_for_supported_usages for \
             how to set one."
        );
    };

    let mut proven = 0;
    for (tc_string, expected) in CONSENT_STRINGS {
        // No id.usage is sent. A stated usage wins over a consent string, so
        // sending one would leave the bit clear and this would prove the
        // opposite of what it says.
        let response = request_with(&resource_key, "tcstring", tc_string);

        let Some(fodid) = response.get("fodid") else {
            eprintln!(
                "consent string granting {expected:?}: no 'fodid' element \
                 returned, so this key is not entitled to that marketing usage"
            );
            continue;
        };

        for name in ["idprobglobal", "idproblic"] {
            if let Some(value) = string_field(fodid, name) {
                // A consent string granting a marketing usage produces a
                // marketing identifier, so the terms travel with it too.
                assert_valid_51did(
                    &format!("consent/{expected:?}/{name}"),
                    value,
                    Some(MODEL_TERMS_FOR_MARKETING_2),
                    *expected,
                    true,
                );
                proven += 1;
            }
        }
    }

    // Rust has no inconclusive result, so this says plainly what the run did
    // rather than leaving a pass to be read as proof.
    if proven == 0 {
        eprintln!(
            "NOTHING PROVEN: this resource key returned no identifier for \
             either consent string, so the usage is indirect bit was never \
             read. Use a key entitled to the standard or personalized usage."
        );
    } else {
        eprintln!("Usage is indirect read on {proven} identifier(s).");
    }
}

/// Asserts that `base64` is a real 51Did: a signed OWID envelope whose payload
/// carries the three 51Did fields, including the 32-byte probabilistic hash.
fn assert_valid_51did(
    label: &str,
    base64: &str,
    expected_terms: Option<&str>,
    expected_usage: Usage,
    indirect: bool,
) {
    assert!(!base64.is_empty(), "{label} should not be empty");

    let fod_id = FodId::from_base64(base64)
        .unwrap_or_else(|e| panic!("{label} should parse as a 51Did: {e}"));

    // A 51Did wraps a payload of at least PAYLOAD_LENGTH bytes carrying a
    // MATCH_KEY_LENGTH byte probabilistic value, inside a domain bearing envelope.
    assert_eq!(
        fod_id.match_key().len(),
        layout::MATCH_KEY_LENGTH,
        "{label}: hash length"
    );
    assert!(
        fod_id.payload().len() >= layout::PAYLOAD_LENGTH,
        "{label}: payload length {} is below the {} byte minimum",
        fod_id.payload().len(),
        layout::PAYLOAD_LENGTH
    );
    assert!(
        !fod_id.domain().is_empty(),
        "{label}: domain should not be empty"
    );

    // The identifier round trips byte for byte and re-parses to the same
    // probabilistic value.
    let round_trip = fod_id.as_base64().expect("should re-encode");
    let reparsed = FodId::from_base64(&round_trip).expect("should re-parse");
    assert_eq!(
        fod_id.match_key(),
        reparsed.match_key(),
        "{label}: hash should survive a base64 round trip"
    );

    // The terms travel with the identifier, so a receiver can read what it
    // was created under without asking anyone. A payload that stops at the
    // match key reads as no terms, which is why this is the assertion that
    // fails where the service has not been updated to write the byte.
    assert_eq!(
        fod_id.terms(),
        expected_terms,
        "{label}: expected the terms to be {expected_terms:?} and the \
         identifier carries {:?}. Where this reads None for a marketing \
         usage the service that answered is older than the release that \
         writes the Terms byte.",
        fod_id.terms()
    );
    assert_eq!(
        reparsed.terms(),
        expected_terms,
        "{label}: terms should survive a base64 round trip"
    );

    // The flags byte, read through the accessors rather than by masking.
    // The usage values are cumulative, being 001, 011 and 111, so a caller
    // masking the byte for the non-marketing bit reads every marketing
    // identifier as non-marketing. These assertions are the alignment
    // between what the service wrote and what this crate answers.
    assert_eq!(
        fod_id.usage(),
        expected_usage,
        "{label}: the service was asked for a {expected_usage:?} identifier \
         and this reads as {:?}",
        fod_id.usage()
    );
    assert_eq!(
        fod_id.usage_is_indirect(),
        indirect,
        "{label}: expected the usage to be recorded as {}, and it reads as {}",
        if indirect {
            "indirect, worked out from a consent string"
        } else {
            "stated by the caller"
        },
        if fod_id.usage_is_indirect() {
            "indirect, worked out from a consent string"
        } else {
            "stated by the caller"
        }
    );
    assert_eq!(
        fod_id.id_type(),
        IdType::Probabilistic,
        "{label}: an idprob* value must be a probabilistic identifier and \
         this reads as {:?}",
        fod_id.id_type()
    );

    let hash_hex: String = fod_id
        .match_key()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    println!(
        "{label}: domain={} usage={:?} indirect={} type={:?} license_id={:#010x} hash={hash_hex}",
        fod_id.domain(),
        fod_id.usage(),
        fod_id.usage_is_indirect(),
        fod_id.id_type(),
        fod_id.license_id()
    );
}

/// Reads a string field from a JSON object, returning `None` when it is
/// absent or not a non-empty string.
fn string_field<'a>(element: &'a serde_json::Value, name: &str) -> Option<&'a str> {
    element
        .get(name)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

// Marked `#[ignore]` because it needs a cloud resource key and network access.
// Run it with `cargo test -p fodid --test cloud_51did -- --include-ignored`.
#[test]
#[ignore = "live cloud test: set _51DEGREES_RESOURCE_KEY and run with `--include-ignored` (see module docs)"]
fn resource_key_returns_51did_for_supported_usages() {
    let Some(resource_key) = resource_key() else {
        panic!(
            "no resource key found for the live cloud 51Did test.\n\
             \n\
             Set a 51Degrees resource key whose properties include `fodid.*` in \
             one of these ways, then re-run:\n\
             \n\
             \x20 - PowerShell env var:  $env:{env} = '<your-key>'\n\
             \x20 - bash env var:        export {env}=<your-key>\n\
             \x20 - inline, single run:  {env}=<your-key> cargo test -p fodid \
             --test cloud_51did -- --include-ignored\n\
             \n\
             The CI-exported tiered names {bespoke} / {basic} are also accepted. Get \
             a free key that includes 51Did from \
             https://configure.51degrees.com/N57Wygby",
            env = RESOURCE_KEY_ENVS[0],
            bespoke = RESOURCE_KEY_ENVS[1],
            basic = RESOURCE_KEY_ENVS[2]
        );
    };

    for usage in USAGES {
        let response = request_usage(&resource_key, usage.name);

        // The cloud groups 51Did properties under a 'fodid' element. It is
        // absent when the resource key does not include the fodid.* properties.
        let Some(fodid) = response.get("fodid") else {
            if usage.required {
                let top_level = response
                    .as_object()
                    .map(|o| o.keys().cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                panic!(
                    "id.usage={}: response has no 'fodid' element; a resource key for \
                     the 51Did tests must include the fodid.* properties. \
                     Top-level elements returned: [{top_level}]",
                    usage.name
                );
            }
            eprintln!(
                "id.usage={}: no 'fodid' element returned (this marketing usage \
                 becomes available once the resource key is expanded for it)",
                usage.name
            );
            continue;
        };

        // idprobglobal is the global 51Did for this usage. It is required for
        // non-marketing and validated when a marketing usage returns it.
        match string_field(fodid, "idprobglobal") {
            Some(idprobglobal) => assert_valid_51did(
                &format!("{}/idprobglobal", usage.name),
                idprobglobal,
                usage.terms,
                usage.expected,
                false,
            ),
            None if usage.required => {
                panic!(
                    "id.usage={}: no idprobglobal returned. fodid element: {fodid}",
                    usage.name
                )
            }
            None => eprintln!(
                "id.usage={}: no idprobglobal returned (becomes available once the \
                 resource key is expanded for this marketing usage)",
                usage.name
            ),
        }

        // idproblic is scoped to the caller's license and is validated whenever
        // it is returned.
        if let Some(idproblic) = string_field(fodid, "idproblic") {
            assert_valid_51did(
                &format!("{}/idproblic", usage.name),
                idproblic,
                usage.terms,
                usage.expected,
                false,
            );
        }
    }
}
