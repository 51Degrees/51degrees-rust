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

//! The documents an agent publishes: a key directory holding its public keys
//! as JSON Web Keys, and an agent card saying who it is and where its keys
//! are.

use ring::digest;
use serde_json::{Map, Value};

use crate::codec;

/// A `nbf` or `exp` above this is a time more than a thousand years away,
/// so it is read as milliseconds. At least one published directory serves
/// milliseconds.
const MILLISECOND_THRESHOLD: i64 = 100_000_000_000;

/// The milliseconds in a second.
const MILLISECONDS_PER_SECOND: i64 = 1000;

/// The earliest time a signature or a key may name, in Unix seconds, being
/// the start of the year 1. The .NET packages hold no earlier time, so both
/// languages refuse the same values.
pub(crate) const EARLIEST_UNIX_SECONDS: i64 = -62_135_596_800;

/// The latest time a signature or a key may name, in Unix seconds, being the
/// end of the year 9999.
pub(crate) const LATEST_UNIX_SECONDS: i64 = 253_402_300_799;

/// One public key from a key directory.
#[derive(Debug, Clone)]
pub(crate) struct JsonWebKey {
    pub(crate) key_type: String,
    pub(crate) key_id: Option<String>,
    pub(crate) algorithm: Option<String>,
    pub(crate) curve: Option<String>,
    pub(crate) x: Option<String>,
    pub(crate) y: Option<String>,
    pub(crate) modulus: Option<String>,
    pub(crate) exponent: Option<String>,
    /// The `nbf` time in Unix seconds.
    pub(crate) not_before: Option<i64>,
    /// The `exp` time in Unix seconds.
    pub(crate) expires: Option<i64>,
    /// True where `nbf` or `exp` was given in milliseconds.
    pub(crate) times_were_in_milliseconds: bool,
    /// The RFC 7638 thumbprint, empty for a key type it is not defined for.
    pub(crate) thumbprint: String,
}

impl JsonWebKey {
    /// Read a key, or `None` where it has no `kty`.
    fn parse(source: &Map<String, Value>) -> Option<JsonWebKey> {
        let key_type = get_string(source, "kty").filter(|kty| !kty.is_empty())?;
        let mut key = JsonWebKey {
            key_type: key_type.to_owned(),
            key_id: get_owned(source, "kid"),
            algorithm: get_owned(source, "alg"),
            curve: get_owned(source, "crv"),
            x: get_owned(source, "x"),
            y: get_owned(source, "y"),
            modulus: get_owned(source, "n"),
            exponent: get_owned(source, "e"),
            not_before: None,
            expires: None,
            times_were_in_milliseconds: false,
            thumbprint: String::new(),
        };
        key.not_before = key.read_time(get_long(source, "nbf"));
        key.expires = key.read_time(get_long(source, "exp"));
        key.thumbprint = thumbprint(&key).unwrap_or_default();
        Some(key)
    }

    /// Whether the key was valid at the given time in Unix seconds.
    pub(crate) fn is_valid_at(&self, seconds: i64) -> bool {
        if self
            .not_before
            .is_some_and(|not_before| seconds < not_before)
        {
            return false;
        }
        !self.expires.is_some_and(|expires| seconds > expires)
    }

    fn read_time(&mut self, value: Option<i64>) -> Option<i64> {
        let mut seconds = value?;
        if seconds > MILLISECOND_THRESHOLD {
            seconds /= MILLISECONDS_PER_SECOND;
            self.times_were_in_milliseconds = true;
        }
        (EARLIEST_UNIX_SECONDS..=LATEST_UNIX_SECONDS)
            .contains(&seconds)
            .then_some(seconds)
    }
}

/// A key directory, or the keys an agent card carries inline.
#[derive(Debug, Clone)]
pub(crate) struct KeyDirectory {
    pub(crate) keys: Vec<JsonWebKey>,
    pub(crate) purpose: Option<String>,
    pub(crate) times_were_in_milliseconds: bool,
}

impl KeyDirectory {
    /// Read a directory from its JSON text.
    pub(crate) fn parse_text(json: &str) -> Option<KeyDirectory> {
        match serde_json::from_str::<Value>(json).ok()? {
            Value::Object(root) => KeyDirectory::parse(&root),
            _ => None,
        }
    }

    /// Read a directory, which must hold at least one key with a `kty`.
    fn parse(root: &Map<String, Value>) -> Option<KeyDirectory> {
        let keys: Vec<JsonWebKey> = get_array(root, "keys")?
            .iter()
            .filter_map(Value::as_object)
            .filter_map(JsonWebKey::parse)
            .collect();
        if keys.is_empty() {
            return None;
        }
        let times_were_in_milliseconds = keys.iter().any(|key| key.times_were_in_milliseconds);
        Some(KeyDirectory {
            keys,
            purpose: get_owned(root, "purpose"),
            times_were_in_milliseconds,
        })
    }

    /// The key the `keyid` names, matched first against each key's
    /// thumbprint and then against each key's `kid`, because the protocol
    /// draft says `keyid` carries the thumbprint while some directories use
    /// the thumbprint as the `kid` as well.
    pub(crate) fn find_key(&self, key_id: &str) -> Option<&JsonWebKey> {
        if key_id.is_empty() {
            return None;
        }
        self.keys
            .iter()
            .find(|key| key.thumbprint == key_id)
            .or_else(|| {
                self.keys
                    .iter()
                    .find(|key| key.key_id.as_deref() == Some(key_id))
            })
    }
}

/// An agent card, which the registry draft calls a Signature Agent Card.
#[derive(Debug, Clone)]
pub(crate) struct AgentCard {
    pub(crate) client_id: String,
    pub(crate) client_name: Option<String>,
    pub(crate) jwks_uri: Option<String>,
    pub(crate) jwks: Option<KeyDirectory>,
    pub(crate) product_token: Option<String>,
    pub(crate) purpose: Option<String>,
}

impl AgentCard {
    /// Read a card fetched from `card_url`. A card whose `client_id` is not
    /// that address, or that carries both `jwks` and `jwks_uri`, is refused.
    pub(crate) fn parse_text(json: &str, card_url: &str) -> Option<AgentCard> {
        let Value::Object(root) = serde_json::from_str::<Value>(json).ok()? else {
            return None;
        };
        let client_id = get_string(&root, "client_id").filter(|id| !id.is_empty())?;
        if client_id != card_url {
            return None;
        }
        let jwks_uri = get_owned(&root, "jwks_uri");
        let jwks_object = get_object(&root, "jwks");
        if jwks_uri.as_deref().is_some_and(|uri| !uri.is_empty()) && jwks_object.is_some() {
            return None;
        }
        let jwks = match jwks_object {
            Some(object) => Some(KeyDirectory::parse(object)?),
            None => None,
        };
        let web_bot_auth = get_object(&root, "web_bot_auth");
        Some(AgentCard {
            client_id: client_id.to_owned(),
            client_name: get_owned(&root, "client_name"),
            jwks_uri,
            jwks,
            product_token: web_bot_auth.and_then(|w| get_owned(w, "rfc9309-product-token")),
            purpose: web_bot_auth.and_then(|w| get_owned(w, "purpose")),
        })
    }
}

/// The RFC 7638 thumbprint of a key, being the SHA-256 of its required
/// members in a fixed form, base64url encoded. RFC 8037 Appendix A.3 names
/// the members of an `OKP` key.
pub(crate) fn thumbprint(key: &JsonWebKey) -> Option<String> {
    canonical_json(key).map(|json| {
        codec::encode_url_safe(digest::digest(&digest::SHA256, json.as_bytes()).as_ref())
    })
}

/// The members a thumbprint is worked out from, in the order and form RFC
/// 7638 section 3 fixes.
pub(crate) fn canonical_json(key: &JsonWebKey) -> Option<String> {
    let present = |value: &Option<String>| value.clone().filter(|text| !text.is_empty());
    let members: Vec<(&str, String)> = match key.key_type.as_str() {
        "OKP" => vec![
            ("crv", present(&key.curve)?),
            ("kty", key.key_type.clone()),
            ("x", present(&key.x)?),
        ],
        "EC" => vec![
            ("crv", present(&key.curve)?),
            ("kty", key.key_type.clone()),
            ("x", present(&key.x)?),
            ("y", present(&key.y)?),
        ],
        "RSA" => vec![
            ("e", present(&key.exponent)?),
            ("kty", key.key_type.clone()),
            ("n", present(&key.modulus)?),
        ],
        _ => return None,
    };
    let mut json = String::from("{");
    for (index, (name, value)) in members.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push('"');
        json.push_str(name);
        json.push_str("\":\"");
        for character in value.chars() {
            match character {
                '"' => json.push_str("\\\""),
                '\\' => json.push_str("\\\\"),
                control if u32::from(control) < 0x20 => {
                    json.push_str(&format!("\\u{:04x}", u32::from(control)));
                }
                other => json.push(other),
            }
        }
        json.push('"');
    }
    json.push('}');
    Some(json)
}

fn get_string<'a>(source: &'a Map<String, Value>, name: &str) -> Option<&'a str> {
    source.get(name).and_then(Value::as_str)
}

fn get_owned(source: &Map<String, Value>, name: &str) -> Option<String> {
    get_string(source, name).map(str::to_owned)
}

/// A whole number, given as a JSON number or as text.
fn get_long(source: &Map<String, Value>, name: &str) -> Option<i64> {
    match source.get(name)? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|fraction| fraction as i64)),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn get_object<'a>(source: &'a Map<String, Value>, name: &str) -> Option<&'a Map<String, Value>> {
    source.get(name).and_then(Value::as_object)
}

fn get_array<'a>(source: &'a Map<String, Value>, name: &str) -> Option<&'a Vec<Value>> {
    source.get(name).and_then(Value::as_array)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The public part of the Ed25519 test key in RFC 9421 Appendix B.1.4.
    const RFC_9421_ED25519_KEY: &str = r#"{"keys":[{"kty":"OKP","crv":"Ed25519",
        "kid":"test-key-ed25519","x":"JrQLj5P_89iXES9-vFgrIy29clF9CC_oPPsw3c5D0bs"}]}"#;

    #[test]
    fn the_rfc_9421_ed25519_key_has_the_known_thumbprint() {
        let directory = KeyDirectory::parse_text(RFC_9421_ED25519_KEY).unwrap();
        assert_eq!(
            directory.keys[0].thumbprint,
            "poqkLGiymh_W0uP6PZFw-dvez3QJT5SolqXBCW38r0U"
        );
    }

    #[test]
    fn the_rfc_7638_example_key_has_the_published_thumbprint() {
        // RFC 7638 section 3.1.
        let json = r#"{"keys":[{"kty":"RSA","e":"AQAB","alg":"RS256",
            "kid":"2011-04-29",
            "n":"0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw"}]}"#;
        let directory = KeyDirectory::parse_text(json).unwrap();
        assert_eq!(
            directory.keys[0].thumbprint,
            "NzbLsXh8uDCcd-6MNwXF4W_7noWXFZAfHkxZsRGC9Xs"
        );
    }

    #[test]
    fn the_key_id_is_matched_on_the_thumbprint_then_the_kid() {
        let directory = KeyDirectory::parse_text(RFC_9421_ED25519_KEY).unwrap();
        assert!(directory
            .find_key("poqkLGiymh_W0uP6PZFw-dvez3QJT5SolqXBCW38r0U")
            .is_some());
        assert!(directory.find_key("test-key-ed25519").is_some());
        assert!(directory.find_key("another").is_none());
        assert!(directory.find_key("").is_none());
    }

    #[test]
    fn key_times_in_milliseconds_are_read_as_seconds() {
        let json = r#"{"keys":[{"kty":"OKP","crv":"Ed25519","x":"AA",
            "nbf":1735689600000,"exp":"4889289600"}]}"#;
        let directory = KeyDirectory::parse_text(json).unwrap();
        let key = &directory.keys[0];
        assert_eq!(key.not_before, Some(1_735_689_600));
        assert_eq!(key.expires, Some(4_889_289_600));
        assert!(directory.times_were_in_milliseconds);
        assert!(!key.is_valid_at(1_735_689_599));
        assert!(key.is_valid_at(1_735_689_600));
        assert!(!key.is_valid_at(4_889_289_601));
    }

    #[test]
    fn a_directory_with_no_usable_key_is_refused() {
        assert!(KeyDirectory::parse_text(r#"{"keys":[]}"#).is_none());
        assert!(KeyDirectory::parse_text(r#"{"keys":[{"kid":"a"}]}"#).is_none());
        assert!(KeyDirectory::parse_text("[]").is_none());
        assert!(KeyDirectory::parse_text("not json").is_none());
    }

    #[test]
    fn a_card_must_name_the_address_it_was_fetched_from() {
        let json = r#"{"client_id":"https://agent.example/card","client_name":"Agent",
            "jwks_uri":"https://agent.example/keys",
            "web_bot_auth":{"rfc9309-product-token":"AgentBot","purpose":"search"}}"#;
        let card = AgentCard::parse_text(json, "https://agent.example/card").unwrap();
        assert_eq!(card.client_name.as_deref(), Some("Agent"));
        assert_eq!(card.product_token.as_deref(), Some("AgentBot"));
        assert_eq!(card.purpose.as_deref(), Some("search"));
        assert!(AgentCard::parse_text(json, "https://agent.example/other").is_none());
    }

    #[test]
    fn a_card_carrying_both_forms_of_keys_is_refused() {
        let json = r#"{"client_id":"https://a.example/c","jwks_uri":"https://a.example/k",
            "jwks":{"keys":[{"kty":"OKP","crv":"Ed25519","x":"AA"}]}}"#;
        assert!(AgentCard::parse_text(json, "https://a.example/c").is_none());
    }
}
