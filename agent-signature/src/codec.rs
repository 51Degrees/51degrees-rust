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

//! The base64 forms the headers, the key directories and the thumbprints use.

use base64::alphabet;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::Engine as _;

/// Standard base64 that needs its padding and accepts unused bits in the last
/// character, as the .NET decoder does, so both languages read the same
/// values.
const LENIENT: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::RequireCanonical),
);

/// Standard base64 with padding.
pub(crate) fn encode_standard(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}

/// URL safe base64 with no padding, the form a JWK thumbprint takes.
pub(crate) fn encode_url_safe(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Standard base64 that must carry its padding.
pub(crate) fn decode_standard(text: &str) -> Option<Vec<u8>> {
    LENIENT.decode(text).ok()
}

/// Standard base64 whose padding may be left off. A length that leaves one
/// character over cannot be base64 at all.
pub(crate) fn decode_standard_padding_optional(text: &str) -> Option<Vec<u8>> {
    decode_standard(&pad(text)?)
}

/// URL safe base64, padded or not, as the members of a JWK are written.
pub(crate) fn decode_url_safe(text: &str) -> Option<Vec<u8>> {
    let standard: String = text
        .chars()
        .map(|character| match character {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    decode_standard_padding_optional(&standard)
}

fn pad(text: &str) -> Option<String> {
    match text.len() % 4 {
        1 => None,
        2 => Some(format!("{text}==")),
        3 => Some(format!("{text}=")),
        _ => Some(text.to_owned()),
    }
}
