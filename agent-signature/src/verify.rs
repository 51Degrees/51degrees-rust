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

//! Settles the algorithm from the key and the `alg` signature parameter, and
//! checks a signature with it.

use ring::signature::{
    RsaPublicKeyComponents, UnparsedPublicKey, ECDSA_P256_SHA256_FIXED, ED25519,
    RSA_PSS_2048_8192_SHA512,
};

use crate::codec;
use crate::constants::{
    ALGORITHM_ECDSA_P256_SHA256, ALGORITHM_ED25519, ALGORITHM_HMAC_SHA256, ALGORITHM_RSA_PSS_SHA512,
};
use crate::keys::JsonWebKey;

/// The length of an Ed25519 public key, and of each P-256 coordinate.
const KEY_COORDINATE_BYTES: usize = 32;

/// The first byte of an uncompressed elliptic curve point.
const UNCOMPRESSED_POINT_PREFIX: u8 = 0x04;

/// The algorithm a signature is checked with, and whether this element
/// checks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AlgorithmResolution {
    pub(crate) name: String,
    pub(crate) supported: bool,
}

/// Settle the algorithm from the key and the signature's `alg` parameter.
///
/// Where both name one and they differ there is nothing safe to check with,
/// so the algorithm is unsupported and the name reported is the one the
/// signature asked for. Where neither names one, the name reported is
/// whatever the key said about itself.
pub(crate) fn resolve_algorithm(
    key: &JsonWebKey,
    signature_algorithm: Option<&str>,
) -> AlgorithmResolution {
    let from_key = from_key(key);
    let from_signature = signature_algorithm
        .filter(|name| !name.is_empty())
        .map(str::to_lowercase);
    if let (Some(from_key), Some(from_signature)) = (&from_key, &from_signature) {
        if from_key != from_signature {
            return AlgorithmResolution {
                name: from_signature.clone(),
                supported: false,
            };
        }
    }
    match from_signature.or(from_key) {
        Some(name) => {
            let supported = is_supported(&name);
            AlgorithmResolution { name, supported }
        }
        None => AlgorithmResolution {
            name: key
                .algorithm
                .clone()
                .unwrap_or_else(|| key.key_type.clone()),
            supported: false,
        },
    }
}

/// Whether this element verifies the named algorithm.
fn is_supported(algorithm: &str) -> bool {
    matches!(
        algorithm,
        ALGORITHM_ED25519 | ALGORITHM_RSA_PSS_SHA512 | ALGORITHM_ECDSA_P256_SHA256
    )
}

/// The RFC 9421 name of the algorithm the key is for. A key names its own
/// algorithm in the JOSE names RFC 7518 and RFC 8037 register, which are not
/// the RFC 9421 names a signature uses. An RSA key on its own does not say
/// which RSA algorithm it is for, so the signature has to.
fn from_key(key: &JsonWebKey) -> Option<String> {
    if let Some(algorithm) = key.algorithm.as_deref().filter(|name| !name.is_empty()) {
        let name = match algorithm.to_uppercase().as_str() {
            "EDDSA" => ALGORITHM_ED25519.to_owned(),
            "PS512" => ALGORITHM_RSA_PSS_SHA512.to_owned(),
            "ES256" => ALGORITHM_ECDSA_P256_SHA256.to_owned(),
            "HS256" => ALGORITHM_HMAC_SHA256.to_owned(),
            // A JOSE algorithm with no RFC 9421 name this element verifies
            // is reported as it was written.
            _ => algorithm.to_lowercase(),
        };
        return Some(name);
    }
    match key.key_type.as_str() {
        "OKP" if key.curve.as_deref() == Some("Ed25519") => Some(ALGORITHM_ED25519.to_owned()),
        "EC" if key.curve.as_deref() == Some("P-256") => {
            Some(ALGORITHM_ECDSA_P256_SHA256.to_owned())
        }
        "OKP" | "EC" => key.curve.as_deref().map(str::to_lowercase),
        // A shared secret, which the Web Bot Auth protocol forbids for
        // signing requests.
        "oct" => Some(ALGORITHM_HMAC_SHA256.to_owned()),
        _ => None,
    }
}

/// Check `signature` over `message` with the key. A key that cannot be read
/// counts as a signature that did not check out, because a signature that
/// cannot be checked has not been checked.
pub(crate) fn verify(algorithm: &str, key: &JsonWebKey, message: &[u8], signature: &[u8]) -> bool {
    match algorithm {
        ALGORITHM_ED25519 => verify_ed25519(key, message, signature),
        ALGORITHM_RSA_PSS_SHA512 => verify_rsa_pss(key, message, signature),
        ALGORITHM_ECDSA_P256_SHA256 => verify_ecdsa(key, message, signature),
        _ => false,
    }
}

fn verify_ed25519(key: &JsonWebKey, message: &[u8], signature: &[u8]) -> bool {
    let Some(public) = key.x.as_deref().and_then(codec::decode_url_safe) else {
        return false;
    };
    public.len() == KEY_COORDINATE_BYTES
        && UnparsedPublicKey::new(&ED25519, &public)
            .verify(message, signature)
            .is_ok()
}

/// RSA-PSS with SHA-512 and a salt as long as the hash, as RFC 9421 section
/// 3.3.1 specifies.
fn verify_rsa_pss(key: &JsonWebKey, message: &[u8], signature: &[u8]) -> bool {
    let modulus = key.modulus.as_deref().and_then(codec::decode_url_safe);
    let exponent = key.exponent.as_deref().and_then(codec::decode_url_safe);
    let (Some(n), Some(e)) = (modulus, exponent) else {
        return false;
    };
    RsaPublicKeyComponents { n, e }
        .verify(&RSA_PSS_2048_8192_SHA512, message, signature)
        .is_ok()
}

/// ECDSA over P-256 with SHA-256, where the signature is r and s side by
/// side as RFC 9421 section 3.3.4 specifies rather than DER.
fn verify_ecdsa(key: &JsonWebKey, message: &[u8], signature: &[u8]) -> bool {
    let x = key.x.as_deref().and_then(codec::decode_url_safe);
    let y = key.y.as_deref().and_then(codec::decode_url_safe);
    let (Some(x), Some(y)) = (x, y) else {
        return false;
    };
    if x.len() != KEY_COORDINATE_BYTES || y.len() != KEY_COORDINATE_BYTES {
        return false;
    }
    let mut point = Vec::with_capacity(1 + 2 * KEY_COORDINATE_BYTES);
    point.push(UNCOMPRESSED_POINT_PREFIX);
    point.extend_from_slice(&x);
    point.extend_from_slice(&y);
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, &point)
        .verify(message, signature)
        .is_ok()
}
