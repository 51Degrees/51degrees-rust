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

use std::ops::Deref;
use std::str::FromStr;

use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::Engine as _;

use crate::owid::{Owid, ParseError, ParseStatus};

use crate::error::{Error, Result};

// The byte layout of a 51Did payload. These offsets and lengths are internal
// to the crate, because the only use a caller has for an offset is to read a
// field out of the payload by hand, and reading by hand is how the usage
// comes out wrong. The usage bits are cumulative, so anyone masking the flags
// byte for the non-marketing bit reads every marketing identifier as
// non-marketing, which is the opposite of the truth. The typed accessors on
// `FodId` are the way to read every field.
//
// The layout is specified at
// https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md
// and the accessors every 51Did package offers at
// https://github.com/51Degrees/specifications/blob/main/did-specification/package-surface.md
// Those pages are the authority, and the unit tests at the end of this file
// check these values against them.
//
// There is no constant for the minimum length of a whole payload, because
// that is the header plus the match key the identifier type requires, and
// `FodId::from_owid` adds the two together at the point it needs the number.

/// Byte offset of the flags field within the payload.
pub(crate) const FLAGS_OFFSET: usize = 0;

/// Byte offset of the License Id field within the payload.
pub(crate) const LICENSE_ID_OFFSET: usize = 1;

/// Byte length of the License Id field.
pub(crate) const LICENSE_ID_LENGTH: usize = 4;

/// Byte offset of the match key within the payload (the byte after the
/// header). For a probabilistic or hashed-email identifier this is the start of
/// the SHA-256 hash, and for a random identifier the start of the GUID.
pub(crate) const MATCH_KEY_OFFSET: usize = 5;

/// Byte length of the match key carried by probabilistic and hashed-email
/// identifiers (a SHA-256 hash).
pub(crate) const MATCH_KEY_LENGTH: usize = 32;

/// Byte length of the payload header (flags and License Id) that is common to
/// every identifier type in payload version 0. A version 0 payload shorter
/// than this, or an empty payload, is [`Error::PayloadTooShort`].
pub(crate) const HEADER_LENGTH: usize = MATCH_KEY_OFFSET;

/// Byte length of the GUID match key carried by [`IdType::Random`]
/// identifiers.
pub(crate) const GUID_LENGTH: usize = 16;

/// The payload layout version this crate reads, carried in bits 4 and 5 of
/// the flags byte. Any other version is refused with
/// [`Error::UnsupportedPayloadVersion`] rather than read under this layout.
pub(crate) const SUPPORTED_PAYLOAD_VERSION: u8 = 0;

/// The Terms index that says the terms are not stated in the identifier. A
/// payload ending at the match key reads as this, so absence and a zero
/// byte mean the same thing and nothing has to tell them apart. It is not a
/// row in [`TERMS_TABLE`] because it names no document.
const NOT_STATED_INDEX: u8 = 0;

/// The identifier type carried in bits 6-7 of the 51Did flags byte.
///
/// Existing identifiers were issued with those bits zeroed, so they decode as
/// [`IdType::Probabilistic`]. The type selects the length and meaning of the
/// value bytes that follow the payload header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdType {
    /// Derived from the device fingerprint and IP address. The value is a
    /// 32-byte SHA-256.
    Probabilistic,
    /// A server-generated random GUID. The value is 16 GUID bytes.
    Random,
    /// Derived from the caller-supplied email and salt. The value is a 32-byte
    /// SHA-256.
    HashedEmail,
    /// Not yet assigned. Parsed best-effort: the header is unpacked and the
    /// remaining payload bytes are exposed as the value as-is.
    Reserved,
}

/// The usage the identifier was created for, carried in bits 0-2 of the
/// flags byte. It decides where the identifier may go: one created for
/// [`Usage::NonMarketing`] must never be passed to a demand source, and
/// one created for [`Usage::Standard`] or [`Usage::Personalized`] may be
/// passed only to a recipient that has accepted the applicable terms.
///
/// The three usages are cumulative rather than exclusive in the byte.
/// Non-marketing sets bit 0, standard sets bits 0 and 1, and personalized
/// sets bits 0, 1 and 2, so every marketing identifier also carries the
/// non-marketing bit. A caller who masked the byte for that bit alone
/// would read every marketing identifier as non-marketing, which is the
/// wrong way round for a data protection decision. This type answers
/// with the highest usage granted, so that mistake cannot be made.
///
/// The names match the cloud's `id.usage` values, being `non-marketing`,
/// `standard` and `personalized`.
///
/// There are exactly three values. A payload with no usage bit set is not
/// a fourth usage, because the cloud never issues one, so such a payload
/// is refused with [`Error::NoUsage`] rather than read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Usage {
    /// Created for use that is not marketing. Must not be passed to a
    /// demand source.
    NonMarketing,
    /// Created for standard marketing, being targeting unrelated to the
    /// person's browsing history or interactions.
    Standard,
    /// Created for personalized marketing, being targeting related to
    /// the person's browsing history or interactions.
    Personalized,
}

impl Usage {
    /// Decode the usage from a flags byte (bits 0-2), answering the
    /// highest usage granted, or `None` where no usage bit is set, which
    /// the reader refuses.
    fn from_flags(flags: u8) -> Option<Usage> {
        if flags & 0b100 != 0 {
            Some(Usage::Personalized)
        } else if flags & 0b010 != 0 {
            Some(Usage::Standard)
        } else if flags & 0b001 != 0 {
            Some(Usage::NonMarketing)
        } else {
            None
        }
    }

    /// The cloud's `id.usage` value for this usage.
    pub fn id_usage(self) -> &'static str {
        match self {
            Usage::NonMarketing => "non-marketing",
            Usage::Standard => "standard",
            Usage::Personalized => "personalized",
        }
    }
}

impl IdType {
    /// Decode the identifier type from a flags byte (bits 6-7).
    fn from_flags(flags: u8) -> IdType {
        match (flags >> 6) & 0b11 {
            0 => IdType::Probabilistic,
            1 => IdType::Random,
            2 => IdType::HashedEmail,
            _ => IdType::Reserved,
        }
    }
}

/// The terms document a 51Did was created under, carried in the byte that
/// follows the match key, so that the terms travel with the identifier
/// rather than beside it.
///
/// The byte is an index into the table below and is not a version number.
/// An index is used so that a later document can live at any address, rather
/// than only at an address the specification could compose from a number.
///
/// | Index | Document                             | Address                     |
/// |------:|--------------------------------------|-----------------------------|
/// |     0 | Not stated in the identifier         | None                        |
/// |     1 | Model Terms for Marketing, version 2 | `https://m4ow.uk/mtm/2.txt` |
///
/// A new terms document is a new index in that table, and every package has
/// to be released to know it, which is the cost of a receiver being able to
/// trust what it reads. An index is never reused or repointed once
/// published, because repointing one would rewrite what an identifier
/// already issued says it agreed to.
///
/// An identifier whose payload ends at the match key carries no terms byte,
/// and a missing byte is read as index 0, so absence and zero say the same
/// thing and neither has to be told apart from the other.
///
/// The table is published at
/// <https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md>,
/// which is the authority rather than this summary.
///
/// This enumeration is not public, and neither is the index behind it. The
/// crate turns the index into the address that [`FodId::terms`] answers
/// with, so a caller never handles the byte, and the names here are the ones
/// the specification gives so that every package describes one document the
/// same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Terms {
    /// Index 0. The terms are not stated in the identifier, which is also
    /// how an identifier whose payload ends at the match key reads.
    ///
    /// This does not mean the identifier is unrestricted. It means the
    /// identifier does not carry the answer, so the answer has to come from
    /// what accompanies it, being the Terms Document Locator in an OpenRTB
    /// request or whatever the surrounding protocol provides. The usage
    /// says where an identifier may go and the terms say which document it
    /// was created under, and a receiver needs both.
    NotStated,
    /// Index 1. The Model Terms for Marketing, version 2, whose address is
    /// answered by [`FodId::terms`].
    ModelTermsForMarketing2,
    /// An index added to the specification after this release, which this
    /// crate cannot name.
    ///
    /// It answers with no address, as [`Terms::NotStated`] does, because
    /// no address may be built from an index this crate cannot name, since
    /// that would name a document nobody wrote.
    Unknown,
}

/// The terms table from the specification, which is the whole of the
/// definition of which index is which document. It is published at
/// <https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md#terms>
/// and this is the only place in the shipped code that carries it. The
/// tests write the address out again on purpose, so that a test never
/// compares the reader with itself.
///
/// One row per terms document, holding the index the payload carries, the
/// name for it and the address it stands for. A new terms document is one
/// new row here and one new variant of [`Terms`], and nothing else in the
/// crate changes. That is the point of the byte being an index rather than
/// a version number, so the cost of a new document is a row and not a
/// search for every place a number was written down.
///
/// [`Terms::NotStated`] and [`Terms::Unknown`] are deliberately absent.
/// Neither names a document, so neither has an address, and a lookup that
/// finds no row is the answer for both.
///
/// Each address names an exact version rather than a landing page, because
/// a document at an unversioned address can be edited afterwards and a
/// receiver has to know the document that was in force when the identifier
/// was made.
const TERMS_TABLE: &[(u8, Terms, &str)] = &[(
    1,
    Terms::ModelTermsForMarketing2,
    "https://m4ow.uk/mtm/2.txt",
)];

impl Terms {
    /// Decode the terms from the index byte that follows the match key. An
    /// index this crate does not know decodes as [`Terms::Unknown`] and
    /// never as [`Terms::NotStated`].
    fn from_index(index: u8) -> Terms {
        if index == NOT_STATED_INDEX {
            return Terms::NotStated;
        }
        TERMS_TABLE
            .iter()
            .find(|(row_index, _, _)| *row_index == index)
            .map_or(Terms::Unknown, |(_, terms, _)| *terms)
    }

    /// The address of the terms document, or `None` where there is none to
    /// give, being index 0 and an index this crate does not know. The
    /// address is answered and never fetched, and the caller decides what
    /// to do with it.
    fn url(self) -> Option<&'static str> {
        TERMS_TABLE
            .iter()
            .find(|(_, terms, _)| *terms == self)
            .map(|(_, _, url)| *url)
    }
}

/// A parsed 51Did: an [`Owid`] envelope whose payload encodes the fields of a
/// 51Degrees identifier.
///
/// The payload starts with a fixed five byte header, being a flags byte and a
/// four byte little endian [`license_id`](FodId::license_id), and the match
/// key follows it. The flags byte is not handed out whole, because every bit
/// in it has a name, so the usage is read through [`usage`](FodId::usage) and
/// [`usage_is_indirect`](FodId::usage_is_indirect) and the identifier type
/// through [`id_type`](FodId::id_type). The type decides the length and
/// meaning of the match key, being 16 GUID bytes for [`IdType::Random`] and a
/// 32 byte SHA-256 for the other types, and the match key is read through
/// [`match_key`](FodId::match_key).
///
/// The byte layout is specified at
/// <https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md>
/// and the accessors every 51Did package offers at
/// <https://github.com/51Degrees/specifications/blob/main/did-specification/package-surface.md>,
/// and those two pages are the authority rather than any summary here.
///
/// The terms byte follows the match key, so where it sits depends on the
/// match key length the type requires. It is read through
/// [`terms`](FodId::terms), which answers with the address of the document,
/// and a payload that ends at the match key carries no terms byte and
/// answers with no address.
///
/// Bits 4 and 5 of the flags byte say which payload layout the identifier
/// follows, and this crate reads version 0. A payload naming any other
/// version is refused with [`Error::UnsupportedPayloadVersion`] rather than
/// read under the layout this crate knows, because a later version exists
/// precisely because a field moved, so reading one here would answer with
/// values that are wrong rather than absent. The version is not exposed,
/// because a caller has nothing to decide with it.
///
/// Those lengths are minimums. A payload may carry more bytes after the match
/// key, which this reader accepts and leaves in place, reachable through
/// [`payload`](Owid::payload). There is no upper bound in this crate.
///
/// `FodId` [`Deref`]s to [`Owid`], so the OWID level fields and operations
/// (`domain()`, `date()`, `payload()`, `signature()`, `as_base64`,
/// `verify_with_public_key`, ...) are available directly on a `FodId` value.
///
/// A parsed `FodId` is not necessarily cryptographically valid. Reading
/// does **not** verify the OWID signature. Call
/// [`verify_status_with_public_key`](Owid::verify_status_with_public_key)
/// (reached through the [`Deref`]) when cryptographic verification is
/// required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FodId {
    owid: Owid,
    flags: u8,
    license_id: u32,
    match_key: Vec<u8>,
    terms_index: u8,
}

impl FodId {
    /// Reads a 51Did from its base64 encoded OWID string, as produced by the
    /// 51Degrees cloud service, without verifying its signature.
    ///
    /// Malformed input is an ordinary outcome and is answered with an
    /// [`Error`] naming the reason, never a panic. The OWID envelope is read
    /// first, then the 51Did payload rules are applied through
    /// [`from_owid`](FodId::from_owid), so every reading route makes the
    /// same checks.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Parse`] carrying the OWID status if the string is not
    /// a valid OWID envelope (for example
    /// [`ParseStatus::InvalidBase64`](crate::ParseStatus::InvalidBase64)),
    /// [`Error::UnsupportedPayloadVersion`] if the flags byte names a payload
    /// version other than 0, [`Error::PayloadTooShort`] if the payload is
    /// empty or cannot hold the 51Did header, [`Error::NoUsage`] if the flags
    /// byte sets no usage bit, or [`Error::InvalidTypePayloadLength`] if the
    /// payload is shorter than the minimum for its identifier type.
    /// Either base64 alphabet is accepted, standard or URL-safe, with or
    /// without padding, because a 51Did travels in URLs and comes back in
    /// the alphabet whoever sent it chose. Leading and trailing white space
    /// is stripped first, so a value carrying a trailing newline from a
    /// file, a header or a copied link reads back to the same envelope as
    /// the clean form. White space inside the text is read as the .NET
    /// package reads it, so every text gets the same answer in both. Spaces,
    /// tabs and line breaks are skipped, but padding is first added for a
    /// length that counts them, so two or three of them inside a text can
    /// add padding the text does not need, and the text is then refused.
    /// Other white space inside the text is refused.
    pub fn from_base64(base64: &str) -> Result<Self> {
        Self::from_owid(Owid::from_byte_array(&decode_base64(base64)?)?)
    }

    /// Reads a 51Did from the raw bytes of an OWID envelope, without verifying
    /// its signature.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Parse`] carrying the OWID status if the bytes are not
    /// a valid OWID envelope, [`Error::UnsupportedPayloadVersion`] if the
    /// flags byte names a payload version other than 0,
    /// [`Error::PayloadTooShort`] if the payload is empty or cannot hold the
    /// 51Did header, [`Error::NoUsage`] if the flags byte sets no usage bit,
    /// or [`Error::InvalidTypePayloadLength`] if the payload is shorter than
    /// the minimum for its identifier type.
    pub fn from_byte_array(buffer: &[u8]) -> Result<Self> {
        Self::from_owid(Owid::from_byte_array(buffer)?)
    }

    /// Promotes an already parsed [`Owid`] into a 51Did by unpacking its payload
    /// fields. The OWID is moved into the returned value and remains reachable
    /// through [`owid`](FodId::owid) and the [`Deref`].
    ///
    /// This is the one place the 51Did payload rules live. The payload
    /// version in the flags byte is read first, because the header's length
    /// and every field after the version belong to the version 0 layout.
    /// The payload must then hold the five byte header before the type can
    /// be read, and then the value length that type requires, being 16 GUID
    /// bytes for [`IdType::Random`] and a 32 byte hash for
    /// [`IdType::Probabilistic`] and [`IdType::HashedEmail`]. A
    /// [`IdType::Reserved`] payload has no defined value length and is read
    /// best effort, taking every byte after the header as its value, so a
    /// reserved identifier states no terms until that length is assigned.
    /// Bytes after the value are accepted and left in the payload, because a
    /// longer payload is a newer shape rather than a fault.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedPayloadVersion`] if the flags byte names a
    /// payload version other than 0, whatever the payload's length,
    /// [`Error::PayloadTooShort`] if the payload is empty or shorter than the
    /// header, [`Error::NoUsage`] if the flags byte sets no usage bit, or
    /// [`Error::InvalidTypePayloadLength`] if the payload is shorter than the
    /// header plus the value length the type requires.
    pub fn from_owid(owid: Owid) -> Result<Self> {
        let payload = owid.payload();
        let Some(&flags) = payload.get(FLAGS_OFFSET) else {
            return Err(Error::PayloadTooShort {
                expected: HEADER_LENGTH,
                actual: payload.len(),
            });
        };
        // The version is read before any field, and before the length,
        // because a later version exists precisely because a field moved.
        // Reading a payload of a version this crate does not know under the
        // layout it does know would answer with values that are wrong rather
        // than absent, and the five byte header is part of that layout, so a
        // short payload naming another version is refused as that version.
        let payload_version = (flags >> 4) & 0b11;
        if payload_version != SUPPORTED_PAYLOAD_VERSION {
            return Err(Error::UnsupportedPayloadVersion {
                version: payload_version,
            });
        }
        if payload.len() < HEADER_LENGTH {
            return Err(Error::PayloadTooShort {
                expected: HEADER_LENGTH,
                actual: payload.len(),
            });
        }
        // Every usage the cloud accepts sets bit 0, so a payload with no
        // usage bit set did not come from it and is damaged or forged. It
        // is refused rather than offered as a fourth usage, because the
        // only safe answer to it is not to pass the identifier on.
        if Usage::from_flags(flags).is_none() {
            return Err(Error::NoUsage);
        }
        let license_id = u32::from_le_bytes(
            payload[LICENSE_ID_OFFSET..LICENSE_ID_OFFSET + LICENSE_ID_LENGTH]
                .try_into()
                .expect("slice is LICENSE_ID_LENGTH bytes"),
        );
        let id_type = IdType::from_flags(flags);
        let value_length = match id_type {
            IdType::Random => GUID_LENGTH,
            // A reserved type has no defined value length yet: expose whatever
            // payload bytes follow the header, best effort.
            IdType::Reserved => payload.len() - HEADER_LENGTH,
            IdType::Probabilistic | IdType::HashedEmail => MATCH_KEY_LENGTH,
        };
        if payload.len() < HEADER_LENGTH + value_length {
            return Err(Error::InvalidTypePayloadLength {
                id_type,
                expected: HEADER_LENGTH + value_length,
                actual: payload.len(),
            });
        }
        let match_key = payload[MATCH_KEY_OFFSET..MATCH_KEY_OFFSET + value_length].to_vec();
        // The terms index is the byte after the match key, so where it sits
        // follows the match key length the type selects. A missing byte is
        // index 0, which says the terms are not stated in the identifier, so
        // absence and zero are one answer.
        //
        // A reserved type has no assigned match key length, so its value is
        // every byte after the header and there is no byte left for the
        // terms to be taken from. Such an identifier reads as index 0, which
        // is correct rather than a fault, and it stops being a special case
        // as soon as a reserved type is assigned a length.
        let terms_index = payload
            .get(MATCH_KEY_OFFSET + value_length)
            .copied()
            .unwrap_or(NOT_STATED_INDEX);
        Ok(FodId {
            owid,
            flags,
            license_id,
            match_key,
            terms_index,
        })
    }

    /// The identifier type carried in bits 6-7 of the flags byte.
    pub fn id_type(&self) -> IdType {
        IdType::from_flags(self.flags)
    }

    /// The usage carried in bits 0-2 of the flags byte, as the highest usage
    /// granted. See [`Usage`] for why it is read that way.
    pub fn usage(&self) -> Usage {
        // from_owid is the only place a FodId is built, every reading route
        // goes through it, and it refuses a payload with no usage bit set, so
        // the flags held here always name a usage.
        Usage::from_flags(self.flags).expect("from_owid refuses a payload with no usage bit set")
    }

    /// Whether the usage is indirect, being bit 3 of the flags byte.
    ///
    /// `false` means the caller stated the usage directly. `true` means the
    /// issuer worked the usage out from some other signal the caller sent.
    /// Today the only such signal is a consent string, so today this is
    /// `true` only when the usage was derived from one, but the bit records
    /// direct against indirect rather than consent strings as such, and a
    /// later signal of another kind sets it too. Both are legitimate ways to
    /// arrive at a usage, and this says nothing about which usage it is.
    pub fn usage_is_indirect(&self) -> bool {
        self.flags & 0b1000 != 0
    }

    /// The 4-byte little endian License Id from the payload, as the raw value
    /// of the field.
    ///
    /// On an identifier carrying a creator context these four bytes hold an
    /// encrypted value that only the issuer can turn back into a license, so
    /// the value identifies nothing outside 51Degrees, and two identifiers
    /// issued under one license need not carry the same value. A caller must
    /// not use it as a grouping or lookup key, as
    /// <https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md#license-id>
    /// says.
    pub fn license_id(&self) -> u32 {
        self.license_id
    }

    /// The match key from the payload: a 32-byte SHA-256 for
    /// [`IdType::Probabilistic`] and [`IdType::HashedEmail`] identifiers, 16 GUID
    /// bytes for [`IdType::Random`] ones.
    ///
    /// This is the stable field for comparing two 51Dids: two identifiers for
    /// the same inputs share the same match key even though their wrapping
    /// envelopes (date, signature) differ on every issue. Compare match keys,
    /// never envelopes.
    pub fn match_key(&self) -> &[u8] {
        &self.match_key
    }

    /// The identifier in the URL-safe base64 alphabet without padding, the
    /// form to put in a URL without any further encoding. It is the inverse
    /// of the normalisation [`from_base64`](FodId::from_base64) applies, so
    /// the value reads back to the same envelope.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Owid`] if the envelope cannot be encoded, which only
    /// happens to an envelope that has never been signed.
    pub fn as_base64_url(&self) -> Result<String> {
        Ok(to_base64_url(&self.owid.as_base64()?))
    }

    /// The address of the terms document the identifier was created under,
    /// read from the byte after the match key.
    ///
    /// The byte is an index into a table in the specification and this
    /// crate turns the index into the address, so a caller never handles
    /// the byte. The address is answered and never fetched, and it is never
    /// an empty string and never built from the index, so `Some` means this
    /// crate knows the document and the caller can rely on the address it
    /// holds.
    ///
    /// `None` covers both an index of zero, which says the terms are not
    /// stated in the identifier, and an index added to the table after this
    /// release, which this crate cannot name. A caller cannot tell those
    /// two apart, which is deliberate, because both lead to the same place,
    /// being that the identifier does not say which terms it was created
    /// under and the answer has to come from somewhere else.
    ///
    /// No address does not mean the identifier is unrestricted. Where an
    /// identifier may go is a separate question [`usage`](FodId::usage)
    /// answers, which still bars a non-marketing identifier from a demand
    /// source.
    pub fn terms(&self) -> Option<&'static str> {
        Terms::from_index(self.terms_index).url()
    }
    /// A reference to the underlying OWID envelope.
    pub fn owid(&self) -> &Owid {
        &self.owid
    }

    /// Consumes the 51Did and returns the underlying OWID envelope.
    pub fn into_owid(self) -> Owid {
        self.owid
    }
}

/// Standard base64 with or without padding. The unused low bits of the last
/// group are ignored rather than refused, as the .NET decoder ignores them.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// Reads the base64 text of an envelope in either alphabet into its bytes,
/// in the same three steps as the .NET package.
///
/// 1. Surrounding white space is trimmed, `-` and `_` become `+` and `/`,
///    and `==` or `=` is added when the trimmed length modulo 4 is 2 or 3.
///    White space inside the text counts towards that length.
/// 2. `=` is added again when the characters that are not white space are
///    2 or 3 over a group of four. A text 1 over is refused.
/// 3. Space, tab, carriage return and line feed are skipped and the rest is
///    decoded. Any other white space inside the text is refused.
///
/// Because step 1 counts white space inside the text, two or three spaces
/// inside can add padding the text does not need, and the text is then
/// refused. Lengths are counted in UTF-16 code units, as .NET counts them.
fn decode_base64(value: &str) -> std::result::Result<Vec<u8>, ParseError> {
    let invalid = || ParseError::new(ParseStatus::InvalidBase64, None);
    let trimmed = value.trim();
    let mut text = trimmed.replace('-', "+").replace('_', "/");
    match trimmed.encode_utf16().count() % 4 {
        2 => text.push_str("=="),
        3 => text.push('='),
        _ => {}
    }
    if text.is_empty() {
        return Err(ParseError::new(ParseStatus::MissingInput, None));
    }
    let significant: usize = text
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(char::len_utf16)
        .sum();
    match significant % 4 {
        0 => {}
        1 => return Err(invalid()),
        over => text.extend(std::iter::repeat_n('=', 4 - over)),
    }
    text.retain(|c| !matches!(c, ' ' | '\t' | '\r' | '\n'));
    BASE64.decode(text).map_err(|_| invalid())
}

/// Writes standard base64 in the URL-safe alphabet without padding, a form
/// [`decode_base64`] reads back to the same bytes.
pub(crate) fn to_base64_url(standard: &str) -> String {
    standard
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_owned()
}

impl Deref for FodId {
    type Target = Owid;

    fn deref(&self) -> &Self::Target {
        &self.owid
    }
}

impl TryFrom<Owid> for FodId {
    type Error = Error;

    fn try_from(owid: Owid) -> Result<Self> {
        FodId::from_owid(owid)
    }
}

impl TryFrom<&[u8]> for FodId {
    type Error = Error;

    fn try_from(buffer: &[u8]) -> Result<Self> {
        FodId::from_byte_array(buffer)
    }
}

impl FromStr for FodId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        FodId::from_base64(s)
    }
}
#[cfg(test)]
mod terms_table_tests {
    use super::{Terms, NOT_STATED_INDEX, TERMS_TABLE};

    /// Every row reads back through the pair of lookups it feeds, so a row
    /// whose index and variant were mistyped apart is caught here rather
    /// than by a caller reading no address for a document this crate is
    /// meant to know.
    #[test]
    fn every_row_round_trips_through_the_lookups() {
        for (index, terms, url) in TERMS_TABLE {
            assert_eq!(Terms::from_index(*index), *terms, "index {index}");
            assert_eq!(terms.url(), Some(*url), "{terms:?}");
            assert!(url.starts_with("https://"), "{url} is not https");
        }
    }

    /// No row may claim index 0. That index says the terms are not stated,
    /// which names no document, so a row there would give an address to an
    /// identifier that states none.
    #[test]
    fn no_row_claims_the_not_stated_index() {
        assert!(TERMS_TABLE
            .iter()
            .all(|(index, _, _)| *index != NOT_STATED_INDEX));
        assert_eq!(Terms::from_index(NOT_STATED_INDEX), Terms::NotStated);
        assert_eq!(Terms::NotStated.url(), None);
    }

    /// One index may stand for one document only. Two rows sharing an index
    /// would make which document an identifier was created under depend on
    /// the order the table happens to be written in.
    #[test]
    fn no_index_appears_twice() {
        for (position, (index, _, _)) in TERMS_TABLE.iter().enumerate() {
            assert!(
                !TERMS_TABLE[position + 1..]
                    .iter()
                    .any(|(later, _, _)| later == index),
                "index {index} appears more than once"
            );
        }
    }

    /// An index the table does not carry is Unknown and never NotStated,
    /// and it answers with no address rather than one built from the
    /// number, since that would name a document nobody wrote.
    #[test]
    fn an_index_outside_the_table_is_unknown_with_no_address() {
        for index in 0..=u8::MAX {
            let known =
                index == NOT_STATED_INDEX || TERMS_TABLE.iter().any(|(row, _, _)| *row == index);
            if known {
                continue;
            }
            assert_eq!(Terms::from_index(index), Terms::Unknown, "index {index}");
            assert_eq!(Terms::from_index(index).url(), None, "index {index}");
        }
    }
}

/// The layout constants are internal, so a consumer cannot read them and the
/// tests that build payloads byte by byte carry their own copy of the layout
/// taken from the specification. These checks are the one place the two
/// copies are tied together, comparing the constants the reader uses against
/// the numbers the specification publishes at
/// <https://github.com/51Degrees/specifications/blob/main/did-specification/identifier-layout.md>.
#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn constants_match_the_published_layout() {
        assert_eq!(FLAGS_OFFSET, 0);
        assert_eq!(LICENSE_ID_OFFSET, 1);
        assert_eq!(LICENSE_ID_LENGTH, 4);
        assert_eq!(MATCH_KEY_OFFSET, 5);
        assert_eq!(MATCH_KEY_LENGTH, 32);
        assert_eq!(HEADER_LENGTH, 5);
        assert_eq!(GUID_LENGTH, 16);
    }

    #[test]
    fn constants_are_internally_consistent() {
        assert_eq!(LICENSE_ID_OFFSET + LICENSE_ID_LENGTH, MATCH_KEY_OFFSET);
        assert_eq!(FLAGS_OFFSET + 1, LICENSE_ID_OFFSET);
        assert_eq!(HEADER_LENGTH, MATCH_KEY_OFFSET);
    }

    /// The usage bits are cumulative, so the highest one set is the answer.
    /// A mask for the non-marketing bit alone would say yes for every
    /// marketing identifier, which is the wrong way round.
    #[test]
    fn usage_decodes_as_the_highest_bit_set() {
        assert_eq!(Usage::from_flags(0b000), None);
        assert_eq!(Usage::from_flags(0b001), Some(Usage::NonMarketing));
        assert_eq!(Usage::from_flags(0b011), Some(Usage::Standard));
        assert_eq!(Usage::from_flags(0b111), Some(Usage::Personalized));
        // Only 000 is refused. The patterns that are not one of the three
        // the cloud writes keep the highest-bit reading.
        assert_eq!(Usage::from_flags(0b010), Some(Usage::Standard));
        assert_eq!(Usage::from_flags(0b100), Some(Usage::Personalized));
        assert_eq!(Usage::from_flags(0b101), Some(Usage::Personalized));
        assert_eq!(Usage::from_flags(0b110), Some(Usage::Personalized));
    }

    #[test]
    fn id_type_decodes_from_the_top_two_bits() {
        assert_eq!(IdType::from_flags(0b0000_0000), IdType::Probabilistic);
        assert_eq!(IdType::from_flags(0b0100_0000), IdType::Random);
        assert_eq!(IdType::from_flags(0b1000_0000), IdType::HashedEmail);
        assert_eq!(IdType::from_flags(0b1100_0000), IdType::Reserved);
    }
}
