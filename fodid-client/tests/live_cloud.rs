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

//! Live integration test for the server side of a 51Did: create one through
//! the cloud, verify it two ways, seal a creator context result and redeem
//! that result with the license key.
//!
//! Every other test in this crate answers from a stub transport, so nothing
//! here had ever run against the service the crate is written for. This test
//! covers the whole server-side sequence:
//!
//! 1. Create. The cloud `json` endpoint is asked for an identifier with
//!    `id.usage=standard`, falling back to `non-marketing` for a key that
//!    grants no marketing usage, which is what a free key does.
//! 2. Read it back. [`FodId::from_base64`] parses what the cloud returned.
//! 3. Verify through the cloud, with [`DidClient::verify`].
//! 4. Verify offline, with [`DidClient::verify_signature_detailed`], which
//!    fetches the signing keys and checks the signature against the key in
//!    force when the identifier was created.
//! 5. Seal a creator context result, with a plain call to `verify-context`.
//!    That endpoint is a browser call, so the client does not carry it, and
//!    this test stands in for the browser.
//! 6. Redeem, with [`DidClient::redeem_encoded`], and read the factors.
//!
//! The test needs a resource key and network access, so it is `#[ignore]`d
//! and a plain `cargo test` leaves it alone. Run it with:
//!
//! ```text
//! cargo test -p fodid-client --test live_cloud -- --include-ignored
//! ```
//!
//! The key is read from `_51DEGREES_RESOURCE_KEY`, then the CI-exported names
//! `_51DEGREES_RESOURCE_KEY_51DID`, `_51DEGREES_RESOURCE_KEY_BESPOKE`,
//! `_51DEGREES_RESOURCE_KEY_BESPOKE` and `_51DEGREES_RESOURCE_KEY_BASIC`. With
//! none of them set the test says why and passes, so a fork with no secrets
//! stays green.
//!
//! The license key decides how far step 6 goes. It is read from
//! `_51DEGREES_LICENSE_KEY_51DID`, then `_51DEGREES_LICENSE_KEY_BESPOKE`, then
//! `_51DEGREES_LICENSE_KEY`, in that order, because the first two are named for
//! the product they carry whilst the last is the general runtime name and may
//! hold a data file license that carries no 51Did product at all.
//!
//! With no license key the cloud answers `unreadable`, which is the correct
//! refusal and is asserted as such, because that is what a caller who forgot
//! the license has to see. With one the redemption must read the sealed
//! verdict, and the signature and every factor are checked. A supplied key
//! that still answers `unreadable` fails the test, because the service only
//! opens a sealed result for a license key of the account whose resource key
//! sealed it, so the run was given a mismatched pair. A pass is given a
//! license key only when it matches the resource key.

#![cfg(feature = "reqwest-client")]

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use fodid::FodId;
use fodid_client::{ContextOutcome, DidClient, SignatureCheck, SignatureOutcome};

/// The cloud the test runs against, overridden with
/// `_51DEGREES_CLOUD_ENDPOINT` for a private deployment, exactly as the client
/// itself is.
const DEFAULT_ENDPOINT: &str = "https://cloud.51degrees.com/api/v4/";

/// The User-Agent supplied as evidence at creation. It travels as a query
/// parameter rather than a header, because a browser-shaped User-Agent header
/// makes the service apply the create-last rule and refuse to form an
/// identifier until the page has run its snippets, which no test can do.
const USER_AGENT_EVIDENCE: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
     AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// The client IP supplied as evidence at creation.
const CLIENT_IP_EVIDENCE: &str = "8.8.8.8";

/// The challenge that binds the sealed result to this one transaction.
const CHALLENGE: &str = "fodid-client-live-test";

/// The nine creator context factors the cloud reports, in the order the
/// specification gives them. A tenth name, or one of these missing, is a
/// change this crate has to follow, so the test names them all.
const FACTORS: [&str; 9] = [
    "transport",
    "device",
    "browserip",
    "connectionip",
    "asn",
    "platformname",
    "platformversion",
    "browsername",
    "browserversion",
];

/// The resource key, from the aligned name first and then the CI-exported
/// tiered names, in the order that puts the most capable key first.
fn resource_key() -> Option<String> {
    [
        "_51DEGREES_RESOURCE_KEY",
        "_51DEGREES_RESOURCE_KEY_51DID",
        "_51DEGREES_RESOURCE_KEY_BESPOKE",
        "_51DEGREES_RESOURCE_KEY_BESPOKE",
        "_51DEGREES_RESOURCE_KEY_BASIC",
    ]
    .into_iter()
    .find_map(non_blank_variable)
}

/// The license key, read the same way, with the two names that say which
/// product the license carries put ahead of the general runtime name.
fn license_key() -> Option<String> {
    [
        "_51DEGREES_LICENSE_KEY_51DID",
        "_51DEGREES_LICENSE_KEY_BESPOKE",
        "_51DEGREES_LICENSE_KEY",
    ]
    .into_iter()
    .find_map(non_blank_variable)
}

/// The trimmed value of an environment variable, or `None` when it is unset
/// or holds nothing but whitespace.
fn non_blank_variable(name: &str) -> Option<String> {
    crate::deprecated_names::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// What a redacted secret is replaced with, so a failure message says
/// plainly that something was taken out rather than looking truncated.
const REDACTED: &str = "[redacted]";

/// The shortest run after the `AQ` that marks a resource key, chosen so
/// ordinary words starting `AQ` are left alone.
const LEAST_KEY_TAIL: usize = 12;

/// The endpoint the test and the client both use.
fn endpoint() -> String {
    crate::deprecated_names::var_with_old("_51DEGREES_CLOUD_ENDPOINT", "FOD_CLOUD_API_URL")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_owned())
}

/// Percent-encode a query string value, leaving only the characters that
/// never need it. A 51Did is standard base64, so it carries `+` and `/`, and
/// a raw `+` in a query string decodes as a space.
fn encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// A client error as text, redacted.
///
/// The client builds the verify and key URLs with the resource key as a
/// path segment, and its transport errors carry the URL they were given,
/// whilst an unexpected status carries the service's body, which names the
/// key. So none of these errors can be reported as they stand.
fn redacted(error: &fodid_client::Error) -> String {
    redact(&format!("{error:?}"))
}

/// Replace every secret in the text with [`REDACTED`].
///
/// The cloud writes the resource key into its own error text, so a failure
/// body carries the key even though the URL is never printed. Anything
/// this test takes from the service goes through here first.
///
/// Two passes run. The first replaces the values this process actually
/// holds, both as they are and percent-encoded as the URL carries them,
/// which cannot miss whatever shape the key has. The second replaces
/// anything else shaped like a resource key, so a key this process never
/// held, for example one named in a message about another key, does not
/// reach the log either.
fn redact(text: &str) -> String {
    redact_secrets(text, [resource_key(), license_key()].into_iter().flatten())
}

/// The two passes, with the held secrets given rather than read, so the
/// first pass can be tested without touching the process environment.
fn redact_secrets(text: &str, secrets: impl IntoIterator<Item = String>) -> String {
    let mut out = text.to_owned();
    for secret in secrets {
        out = out.replace(&secret, REDACTED);
        let encoded = encode(&secret);
        if encoded != secret {
            out = out.replace(&encoded, REDACTED);
        }
    }
    redact_key_shaped(&out)
}

/// Replace every run that starts `AQ` and carries at least
/// [`LEAST_KEY_TAIL`] more characters of the base64 url alphabet, which is
/// the shape every 51Degrees resource key has.
fn redact_key_shaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("AQ") {
        out.push_str(&rest[..start]);
        let tail = &rest[start + "AQ".len()..];
        let length = tail
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(tail.len());
        if length >= LEAST_KEY_TAIL {
            out.push_str(REDACTED);
            rest = &tail[length..];
        } else {
            out.push_str("AQ");
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

/// GET a URL and return the body, failing with the status and the body when
/// the service answers anything other than 200. The URL is never printed,
/// because it carries the resource key, and the body is put through
/// [`redact`] first, because the service writes the key into its own error
/// text and so the body carries it too.
fn get(url: &str, what: &str) -> String {
    match ureq::get(url).call() {
        Ok(response) => response
            .into_string()
            .unwrap_or_else(|e| panic!("reading the {what} response: {}", redact(&e.to_string()))),
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            panic!("{what} answered {status}: {}", redact(&body));
        }
        // A transport failure is reported by its kind alone. The error's own
        // Display writes the URL it was given, and that URL carries the
        // resource key, so formatting the error whole would put the key in
        // the panic message and so into the job log.
        Err(e) => panic!("the {what} request failed: {}", e.kind()),
    }
}

/// Ask the cloud for an identifier at the given usage and return the whole
/// `fodid` member, or `None` when the response carries none at all.
fn create(resource_key: &str, usage: &str) -> Option<serde_json::Value> {
    let url = format!(
        "{}json?resource={}&user-agent={}&client-ip={}&id.usage={}",
        endpoint(),
        encode(resource_key),
        encode(USER_AGENT_EVIDENCE),
        encode(CLIENT_IP_EVIDENCE),
        encode(usage),
    );
    let body = get(&url, "cloud json endpoint");
    let parsed: serde_json::Value =
        serde_json::from_str(&body).expect("the cloud json response should be JSON");
    parsed.get("fodid").cloned()
}

/// The first identifier the `fodid` member carries, with its name, or `None`
/// when every one of them is null.
fn first_identifier(fodid: &serde_json::Value) -> Option<(String, String)> {
    let members = fodid.as_object()?;
    members.iter().find_map(|(name, value)| {
        if name.ends_with("nullreason") {
            return None;
        }
        value
            .as_str()
            .filter(|encoded| !encoded.is_empty())
            .map(|encoded| (name.clone(), encoded.to_owned()))
    })
}

/// Every null reason the `fodid` member carries, joined, so a skip says why.
/// The text is the service's own, so it is redacted like any other.
fn null_reasons(fodid: &serde_json::Value) -> String {
    let joined = fodid
        .as_object()
        .map(|members| {
            members
                .iter()
                .filter(|(name, _)| name.ends_with("nullreason"))
                .filter_map(|(_, value)| value.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    redact(&joined)
}

/// Seal a creator context result for the identifier, standing in for the
/// browser call the client deliberately does not carry.
fn verify_context(resource_key: &str, encoded_51did: &str) -> Option<String> {
    let url = format!(
        "{}id/verify-context/{}?51did={}&challenge={}",
        endpoint(),
        encode(resource_key),
        encode(encoded_51did),
        encode(CHALLENGE),
    );
    let body = get(&url, "verify-context endpoint");
    let parsed: serde_json::Value =
        serde_json::from_str(&body).expect("the verify-context response should be JSON");
    parsed
        .get("result")
        .and_then(|result| result.as_str())
        .map(str::to_owned)
}

/// A resource key that is not a real one, used wherever a test needs a
/// key-shaped value. It is deliberately not a key any service accepts.
const FAKE_RESOURCE_KEY: &str = "AQ-NOT-A-REAL-KEY-000000";

/// The cloud writes the key it was given into its own error text, so a
/// failure body carries it. This is the shape of that text, with the
/// fake key in place of a real one.
fn error_body_naming(key: &str) -> String {
    format!("{{ \"errors\":[\"\\u0027{key}\\u0027 could not be read as a valid resource key.\"]}}")
}

/// Answer one request on loopback with 400 and the given body, so a
/// test can make [`get`] fail without a network or a real key.
fn serve_once(body: String) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let url = format!(
        "http://{}/answer",
        listener.local_addr().expect("the bound address")
    );
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("one connection");
        let mut buffer = [0u8; 1024];
        let _ = stream.read(&mut buffer);
        let response = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    });
    (url, handle)
}

/// The fix, proved where it matters. The call really fails, on the status
/// path that used to print the body as it stands, and the panic message
/// carries no key. The server is on loopback and the key is a fake one, so
/// this needs no network and no secret and runs in an ordinary test pass.
#[test]
fn a_failing_call_panics_without_naming_the_key() {
    let body = error_body_naming(FAKE_RESOURCE_KEY);
    assert!(
        body.contains(FAKE_RESOURCE_KEY),
        "the service body names the key, which is the leak being closed"
    );

    let (url, handle) = serve_once(body);
    let failure = std::panic::catch_unwind(|| get(&url, "test endpoint"))
        .expect_err("a 400 makes the call fail");
    let _ = handle.join();

    let message = failure
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| failure.downcast_ref::<&str>().copied())
        .expect("the panic carries a message");
    assert!(
        !message.contains(FAKE_RESOURCE_KEY),
        "the panic message names no key, and said: {message}"
    );
    assert!(
        message.contains("400") && message.contains("test endpoint"),
        "the panic still says what failed and how, and said: {message}"
    );
    assert!(
        message.contains("could not be read as a valid resource key"),
        "the reason the service gave survives, and said: {message}"
    );
}

/// The first pass, on the values this process holds. The secrets are given
/// rather than read, so nothing touches the process environment.
#[test]
fn a_held_secret_is_replaced_whatever_shape_it_has() {
    let secret = "not-key-shaped-at-all";
    let text = format!(
        "the service said '{secret}' and the url carried {}",
        encode(secret)
    );

    let redacted = redact_secrets(&text, [secret.to_owned()]);

    assert!(
        !redacted.contains(secret),
        "the held value is gone, and said: {redacted}"
    );
    assert!(
        !redacted.contains(&encode(secret)),
        "the percent-encoded form is gone too, and said: {redacted}"
    );
    assert!(
        redacted.contains("the service said"),
        "the rest of the message survives, and said: {redacted}"
    );
}

/// A client error carries the URL it was given, and the verify and key
/// URLs put the resource key in the path, so the error has to be
/// redacted before it is reported.
#[test]
fn a_client_error_is_reported_without_its_url() {
    let error = fodid_client::Error::Transport(format!(
        "failed to send request to 'https://cloud.51degrees.com/api/v4/id/key/{FAKE_RESOURCE_KEY}': timed out"
    ));

    let reported = redacted(&error);

    assert!(
        !reported.contains(FAKE_RESOURCE_KEY),
        "the reported error names no key, and said: {reported}"
    );
    assert!(
        reported.contains("timed out"),
        "what went wrong still survives, and said: {reported}"
    );
}

/// The second pass, on a key this process never held, and the words
/// around it that must be left alone.
#[test]
fn a_key_shaped_run_is_replaced_and_ordinary_words_are_not() {
    let redacted = redact_secrets(
        &format!("AQ and AQUA and {FAKE_RESOURCE_KEY} and AQ12345678901."),
        [],
    );

    assert_eq!(
        "AQ and AQUA and [redacted] and AQ12345678901.", redacted,
        "only the run long enough to be a key is replaced"
    );
}

#[tokio::test]
#[ignore = "live cloud test: set a resource key and run with `--include-ignored` (see module docs)"]
async fn creates_verifies_and_redeems_a_51did_against_the_live_cloud() {
    let Some(resource_key) = resource_key() else {
        eprintln!(
            "no resource key in the environment, so the live 51Did client test \
             is skipped. Set _51DEGREES_RESOURCE_KEY to a key that carries the \
             51Did product."
        );
        return;
    };

    // 1. Create. A key that grants no marketing usage refuses `standard`, so
    //    the test falls back to the usage every 51Did key grants.
    let mut usage = "standard";
    let Some(mut fodid) = create(&resource_key, usage) else {
        eprintln!(
            "this resource key carries no 51Did product, so the live 51Did \
             client test is skipped."
        );
        return;
    };
    if first_identifier(&fodid).is_none() {
        eprintln!(
            "no identifier at id.usage=standard ({}), retrying at non-marketing.",
            null_reasons(&fodid)
        );
        usage = "non-marketing";
        fodid = create(&resource_key, usage).expect("the fodid member is present at non-marketing");
    }
    let (name, encoded) = first_identifier(&fodid).unwrap_or_else(|| {
        panic!(
            "the cloud formed no identifier at either usage: {}",
            null_reasons(&fodid)
        )
    });
    eprintln!(
        "created {name} at id.usage={usage}, {} characters",
        encoded.len()
    );

    // 2. Read it back.
    let fod_id = FodId::from_base64(&encoded)
        .unwrap_or_else(|e| panic!("the cloud's {name} should parse: {e}"));
    assert!(
        !fod_id.match_key().is_empty(),
        "the parsed identifier carries a match key"
    );

    let mut builder = DidClient::builder(resource_key.clone()).endpoint(endpoint());
    let license = license_key();
    if let Some(license) = &license {
        builder = builder.licence_key(license.clone());
    }
    let client = builder.build().expect("the client builds");
    assert_eq!(client.has_licence_key(), license.is_some());

    // 3. Verify through the cloud.
    assert!(
        client
            .verify(&fod_id)
            .await
            .unwrap_or_else(|e| panic!("the verify call succeeds: {}", redacted(&e))),
        "the cloud verifies the identifier it just created"
    );

    // 4. Verify offline against the key in force when it was created.
    assert_eq!(
        client
            .verify_signature_detailed(&fod_id)
            .await
            .unwrap_or_else(|e| {
                panic!("the offline signature check succeeds: {}", redacted(&e))
            }),
        SignatureCheck::Verified,
        "the signature checks against the published key for its date"
    );

    // 5. Seal a creator context result. A deployment holding no creator
    //    context secret returns no result, and there is nothing to redeem.
    let Some(sealed) = verify_context(&resource_key, &encoded) else {
        eprintln!(
            "verify-context returned no sealed result, so this deployment holds \
             no creator context secret and the redemption is skipped."
        );
        return;
    };

    // 6. Redeem.
    let outcome = client
        .redeem_encoded(&encoded, &sealed, Some(CHALLENGE))
        .await
        .unwrap_or_else(|e| panic!("the redeem call succeeds: {}", redacted(&e)));

    if license.is_none() {
        assert_eq!(
            outcome.context(),
            ContextOutcome::Unreadable,
            "without a license key the sealed result cannot be read: {}",
            redact(outcome.body())
        );
        eprintln!("no license key, so the redemption correctly answered unreadable.");
        return;
    }

    // A supplied license key must read the result. Returning early on
    // `unreadable` here once kept CI green while the readable verdict was
    // never checked, so a mismatched pair is a failure, not a skip.
    assert_ne!(
        outcome.context(),
        ContextOutcome::Unreadable,
        "a license key was supplied, so the sealed result must be readable. Is the \
         license key from the same account as the resource key? {}",
        redact(outcome.body())
    );

    assert_eq!(
        outcome.signature(),
        SignatureOutcome::Verified,
        "the redemption reports the signature as verified: {}",
        redact(outcome.body())
    );

    // The identifier was created with a User-Agent and a client IP that are
    // not this machine's, so several factors are expected to differ. What
    // matters here is that every factor is named and reported.
    let factors = outcome.factors().unwrap_or_else(|| {
        panic!(
            "the redemption names the factors: {}",
            redact(outcome.body())
        )
    });
    for factor in FACTORS {
        assert!(
            factors.contains_key(factor),
            "the '{factor}' factor is reported: {}",
            redact(outcome.body())
        );
    }
    assert_eq!(
        factors.len(),
        FACTORS.len(),
        "exactly the nine known factors are reported: {}",
        redact(outcome.body())
    );
    eprintln!(
        "redeemed: signature {:?}, context {:?}, {} factors.",
        outcome.signature(),
        outcome.context(),
        factors.len()
    );
}
