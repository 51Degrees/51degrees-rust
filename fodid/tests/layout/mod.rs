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

//! The 51Did payload layout, as the tests need it to build payloads byte by
//! byte.
//!
//! The crate does not publish these offsets and lengths, because the only
//! use a caller has for an offset is to read a field out of the payload by
//! hand, and reading by hand is how the usage comes out wrong. The tests
//! stand outside the crate and so cannot see the internal constants, and
//! they carry their own copy here, taken from the specification at
//! <https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md>.
//! That is what a test of a byte format should do anyway, because a payload
//! built from the reader's own constants would agree with the reader
//! whatever either of them said. The unit tests in `src/fodid.rs` tie the
//! reader's constants to the same published numbers.
//!
//! Each test binary uses the part of the layout it needs, so the ones it
//! does not use are not dead code in the ordinary sense.
#![allow(dead_code)]

/// Byte offset of the flags field within the payload.
pub const FLAGS_OFFSET: usize = 0;

/// Byte offset of the License Id field within the payload.
pub const LICENSE_ID_OFFSET: usize = 1;

/// Byte length of the License Id field.
pub const LICENSE_ID_LENGTH: usize = 4;

/// Byte offset of the match key within the payload, being the byte after the
/// header.
pub const MATCH_KEY_OFFSET: usize = 5;

/// Byte length of the match key carried by probabilistic and hashed-email
/// identifiers (a SHA-256 hash).
pub const MATCH_KEY_LENGTH: usize = 32;

/// Byte length of the payload header, being the flags byte and the License
/// Id, which every identifier type carries.
pub const HEADER_LENGTH: usize = 5;

/// Byte length of the GUID match key carried by random identifiers.
pub const GUID_LENGTH: usize = 16;

/// Minimum byte length of a random 51Did payload, being the header and the
/// GUID.
pub const RANDOM_PAYLOAD_LENGTH: usize = 21;

/// Minimum byte length of a probabilistic or hashed-email 51Did payload,
/// being the header and the hash.
pub const PAYLOAD_LENGTH: usize = 37;
