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

//! Behavioral tests for the 51Did reader, covering parsing, construction,
//! the reasons a read fails and the separation of reading from signature
//! verification.
//!
//! Every failed read is checked for the same three facts: the read failed,
//! no value came back, and the status names the reason. Reading never
//! touches a key, so none of the failure cases here constructs one.

use fodid::{
    Creator, Crypto, Error, FodId, IdType, Owid, ParseDetail, ParseStatus, SignatureStatus, Usage,
};

mod layout;

const TEST_DOMAIN: &str = "51degrees.com";

/// Bits 4 and 5 are the payload version and are left clear, because this
/// crate reads version 0 and refuses any other, so a fixture with them
/// set would be refused rather than read.
const CANONICAL_FLAGS: u8 = 0b1000_0101;
const CANONICAL_LICENSE_ID: u32 = 0x1234_5678;

/// Flags bytes whose bits 6-7 select each identifier type. The lower usage
/// bits are set differently in each so the type decode is shown to ignore
/// them, and every one sets a usage bit, because a payload with none is
/// refused.
const PROBABILISTIC_FLAGS: u8 = 0b0000_0101;
const RANDOM_FLAGS: u8 = 0b0100_0001;
const HASHED_EMAIL_FLAGS: u8 = 0b1000_0011;
const RESERVED_FLAGS: u8 = 0b1100_0111;

/// The random type bits alone, for the tests that set the usage bits
/// themselves.
const RANDOM_TYPE: u8 = 0b0100_0000;

/// The stable 32-byte hash used across the field-level assertions: 0x20..0x3F.
fn canonical_hash() -> [u8; layout::MATCH_KEY_LENGTH] {
    let mut hash = [0u8; layout::MATCH_KEY_LENGTH];
    for (i, b) in hash.iter_mut().enumerate() {
        *b = 0x20 + i as u8;
    }
    hash
}

/// A 51Did payload cut off at the end of the match key, so it carries no
/// terms byte. A reader takes a missing byte as a terms index of zero,
/// and this is the fixture for that rule rather than anything an issuer
/// would write.
fn payload_ending_at_match_key() -> Vec<u8> {
    let mut payload = vec![0u8; layout::PAYLOAD_LENGTH];
    payload[layout::FLAGS_OFFSET] = CANONICAL_FLAGS;
    payload[layout::LICENSE_ID_OFFSET..layout::LICENSE_ID_OFFSET + layout::LICENSE_ID_LENGTH]
        .copy_from_slice(&CANONICAL_LICENSE_ID.to_le_bytes());
    payload[layout::MATCH_KEY_OFFSET..layout::MATCH_KEY_OFFSET + layout::MATCH_KEY_LENGTH]
        .copy_from_slice(&canonical_hash());
    payload
}

/// Build a payload of `value_len` value bytes after the header, with the given
/// flags byte and the canonical license id. The value bytes run 0x50, 0x51, ...
fn typed_payload(flags: u8, value_len: usize) -> Vec<u8> {
    let mut payload = vec![0u8; layout::HEADER_LENGTH + value_len];
    payload[layout::FLAGS_OFFSET] = flags;
    payload[layout::LICENSE_ID_OFFSET..layout::LICENSE_ID_OFFSET + layout::LICENSE_ID_LENGTH]
        .copy_from_slice(&CANONICAL_LICENSE_ID.to_le_bytes());
    for (i, b) in payload[layout::MATCH_KEY_OFFSET..].iter_mut().enumerate() {
        *b = 0x50 + i as u8;
    }
    payload
}

/// Asserts the canonical flags byte both as the byte the envelope carries
/// and as the values the typed accessors read out of it. Bits 6-7 are 10, so
/// the type is hashed email, and the usage bits are 101, so the highest usage
/// granted is personalized, stated directly rather than worked out from
/// another signal.
#[track_caller]
fn assert_canonical_flags(fod_id: &FodId) {
    assert_eq!(CANONICAL_FLAGS, fod_id.payload()[layout::FLAGS_OFFSET]);
    assert_eq!(IdType::HashedEmail, fod_id.id_type());
    assert_eq!(Usage::Personalized, fod_id.usage());
    assert!(!fod_id.usage_is_indirect());
}

/// Generates a key pair and exposes the PEM forms, used to set up each test.
struct Fixture {
    public_pem: String,
    private_pem: String,
}

impl Fixture {
    fn new() -> Self {
        let crypto = Crypto::new();
        Fixture {
            public_pem: crypto.public_key_pem().expect("export public key"),
            private_pem: crypto.private_key_pem().expect("export private key"),
        }
    }

    /// Creates and signs a real OWID with the given payload under the test
    /// domain.
    fn signed_owid(&self, payload: Vec<u8>) -> Owid {
        self.signed_owid_for(TEST_DOMAIN, payload)
    }

    /// Creates and signs a real OWID with the given payload under the domain
    /// given, standing in for whichever creator issued the 51Did.
    fn signed_owid_for(&self, domain: &str, payload: Vec<u8>) -> Owid {
        let crypto = Crypto::new_sign_only(&self.private_pem).expect("import private key");
        let creator = Creator::new(domain, crypto).expect("create creator");
        creator
            .create(payload)
            .expect("create and sign the envelope")
    }

    fn signed_owid_base64(&self, payload: Vec<u8>) -> String {
        self.signed_owid(payload).as_base64().expect("encode owid")
    }
}

/// The status of a read, in the cross language vocabulary, read off the
/// [`Error`] variant and never off its message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Parsed,
    Owid(ParseStatus),
    PayloadTooShort,
    InvalidTypePayloadLength,
    NoUsage,
}

fn status_of(result: &fodid::Result<FodId>) -> Status {
    match result {
        Ok(_) => Status::Parsed,
        Err(Error::Parse(e)) => Status::Owid(e.status()),
        Err(Error::PayloadTooShort { .. }) => Status::PayloadTooShort,
        Err(Error::InvalidTypePayloadLength { .. }) => Status::InvalidTypePayloadLength,
        Err(Error::NoUsage) => Status::NoUsage,
        Err(other) => panic!("a read never produces {other:?}"),
    }
}

/// Asserts the three facts of a failed read: it failed, no value came back,
/// and the status names the expected reason.
#[track_caller]
fn assert_failed(result: &fodid::Result<FodId>, expected: Status) {
    assert!(result.is_err(), "the read should fail with {expected:?}");
    assert!(
        result.as_ref().ok().is_none(),
        "a failed read must hand back no value"
    );
    assert_eq!(status_of(result), expected);
}

/// Asserts the three facts of a successful read: it succeeded, a value came
/// back, and the status is parsed.
#[track_caller]
fn assert_parsed(result: &fodid::Result<FodId>) -> &FodId {
    assert!(result.is_ok(), "the read should succeed, got {result:?}");
    assert_eq!(status_of(result), Status::Parsed);
    result
        .as_ref()
        .expect("a successful read hands back the value")
}

/// `owid()`, `into_owid()` and the `Deref` are deprecated rather than
/// removed, so code written against earlier releases still compiles and
/// still reaches the same envelope.
#[test]
#[allow(deprecated)]
fn fod_id_still_derefs_to_owid() {
    let fixture = Fixture::new();
    let owid = fixture.signed_owid(canonical_payload());
    let fod_id = FodId::from_owid(owid.clone()).unwrap();

    let via_deref: &Owid = &fod_id;
    assert_eq!(via_deref, &owid);
    assert_eq!(fod_id.owid(), &owid);
    assert_eq!(fod_id.into_owid(), owid);
}

/// Each envelope method of `FodId` answers as the same method of the
/// envelope it was read from.
#[test]
fn the_envelope_methods_answer_as_the_envelope() {
    let fixture = Fixture::new();
    let owid = fixture.signed_owid(canonical_payload());
    let fod_id = FodId::from_owid(owid.clone()).unwrap();
    let crypto = Crypto::new_verify_only(&fixture.public_pem).unwrap();
    let someone_else = Fixture::new();

    assert_eq!(fod_id.version(), owid.version());
    assert_eq!(fod_id.domain(), TEST_DOMAIN);
    assert_eq!(fod_id.date(), owid.date());
    assert_eq!(fod_id.signature(), owid.signature());
    assert_eq!(fod_id.signature().len(), fodid::SIGNATURE_LENGTH);
    // Each call reads the clock, so a minute can turn between the two.
    let age = fod_id.age_minutes();
    assert!((0..=1).contains(&(owid.age_minutes() - age)));
    assert_eq!(
        fod_id.as_byte_array().unwrap(),
        owid.as_byte_array().unwrap()
    );
    assert_eq!(fod_id.as_base64().unwrap(), owid.as_base64().unwrap());
    assert!(fod_id.verify_with_crypto(&crypto).unwrap());
    assert!(fod_id.verify_with_public_key(&fixture.public_pem).unwrap());
    assert_eq!(
        fod_id.verify_status_with_crypto(&crypto),
        SignatureStatus::Valid
    );
    assert_eq!(
        fod_id.verify_status_with_public_key(&fixture.public_pem),
        SignatureStatus::Valid
    );
    assert_eq!(
        fod_id.verify_status_with_public_key(&someone_else.public_pem),
        SignatureStatus::Invalid
    );
}

/// Two 51Dids for the same inputs carry the same type and match key in
/// envelopes that differ, so `==` compares the type and match key and
/// nothing else.
#[test]
fn equal_when_the_type_and_match_key_are_equal() {
    let fixture = Fixture::new();
    let first = fixture.signed_owid(canonical_payload());

    // The same type and match key, with every other field changed. The
    // flags keep the hashed email type and set a lower usage, indirect,
    // the License Id differs and the payload ends at the match key.
    let mut payload = payload_ending_at_match_key();
    payload[layout::FLAGS_OFFSET] = 0b1000_1001;
    payload[layout::LICENSE_ID_OFFSET..layout::LICENSE_ID_OFFSET + layout::LICENSE_ID_LENGTH]
        .copy_from_slice(&0x0BAD_F00Du32.to_le_bytes());
    let domain = "example.com";
    let mut second = fixture
        .signed_owid_for(domain, payload)
        .as_byte_array()
        .unwrap();
    // A different date, written after signing, which reading never checks.
    let at = 1 + domain.len() + 1;
    second[at..at + 4].copy_from_slice(&1u32.to_le_bytes());

    let a = FodId::from_owid(first).unwrap();
    let b = FodId::from_byte_array(&second).unwrap();
    assert_ne!(a.as_byte_array().unwrap(), b.as_byte_array().unwrap());
    assert_ne!(a.domain(), b.domain());
    assert_ne!(a.date(), b.date());
    assert_ne!(a.usage(), b.usage());
    assert_ne!(a.usage_is_indirect(), b.usage_is_indirect());
    assert_ne!(a.license_id(), b.license_id());
    assert_ne!(a.terms(), b.terms());
    assert_eq!(a.id_type(), b.id_type());
    assert_eq!(a.match_key(), b.match_key());
    assert_eq!(a, b);
    assert_eq!(b, a);
}

#[test]
fn not_equal_when_the_type_or_match_key_differs() {
    let fixture = Fixture::new();
    let read = |payload: Vec<u8>| FodId::from_owid(fixture.signed_owid(payload)).unwrap();
    let hashed_email = read(canonical_payload());

    // The same 32 match key bytes under the probabilistic type.
    let mut payload = canonical_payload();
    payload[layout::FLAGS_OFFSET] = CANONICAL_FLAGS & 0b0011_1111;
    let probabilistic = read(payload);
    assert_eq!(probabilistic.id_type(), IdType::Probabilistic);
    assert_eq!(probabilistic.match_key(), hashed_email.match_key());
    assert_ne!(probabilistic, hashed_email);

    // The same type with the last match key byte changed.
    let mut payload = canonical_payload();
    payload[layout::MATCH_KEY_OFFSET + layout::MATCH_KEY_LENGTH - 1] ^= 1;
    let other_key = read(payload);
    assert_eq!(other_key.id_type(), hashed_email.id_type());
    assert_ne!(other_key, hashed_email);
}

#[test]
fn constructor_from_base64_unpacks_all_three_fields() {
    let fixture = Fixture::new();
    let base64 = fixture.signed_owid_base64(canonical_payload());

    let result = FodId::from_base64(&base64);
    let fod_id = assert_parsed(&result);

    assert_canonical_flags(fod_id);
    assert_eq!(CANONICAL_LICENSE_ID, fod_id.license_id());
    assert_eq!(&canonical_hash(), fod_id.match_key());
    assert_eq!(TEST_DOMAIN, fod_id.domain());
}

#[test]
fn constructor_from_bytes_unpacks_all_three_fields() {
    let fixture = Fixture::new();
    let bytes = fixture
        .signed_owid(canonical_payload())
        .as_byte_array()
        .unwrap();

    let result = FodId::from_byte_array(&bytes);
    let fod_id = assert_parsed(&result);

    assert_canonical_flags(fod_id);
    assert_eq!(CANONICAL_LICENSE_ID, fod_id.license_id());
    assert_eq!(&canonical_hash(), fod_id.match_key());
    assert_eq!(TEST_DOMAIN, fod_id.domain());
}

#[test]
fn constructor_from_owid_unpacks_all_three_fields() {
    let fixture = Fixture::new();
    let owid = fixture.signed_owid(canonical_payload());
    let expected = owid.clone();

    let result = FodId::from_owid(owid);
    let fod_id = assert_parsed(&result);

    assert_canonical_flags(fod_id);
    assert_eq!(CANONICAL_LICENSE_ID, fod_id.license_id());
    assert_eq!(&canonical_hash(), fod_id.match_key());
    assert_eq!(expected.domain(), fod_id.domain());
    assert_eq!(expected.date(), fod_id.date());
    assert_eq!(expected.version(), fod_id.version());
    // The whole envelope is preserved, not just the parsed fields.
    assert_eq!(
        expected.as_byte_array().unwrap(),
        fod_id.as_byte_array().unwrap()
    );
}

#[test]
fn license_id_is_little_endian() {
    let fixture = Fixture::new();
    let mut payload = canonical_payload();
    // 0x01 0x00 0x00 0x00 little endian -> 1
    payload[layout::LICENSE_ID_OFFSET..layout::LICENSE_ID_OFFSET + 4]
        .copy_from_slice(&[0x01, 0x00, 0x00, 0x00]);

    let fod_id = FodId::from_base64(&fixture.signed_owid_base64(payload)).unwrap();

    assert_eq!(1u32, fod_id.license_id());
}

#[test]
fn license_id_max_value_is_little_endian() {
    let fixture = Fixture::new();
    let mut payload = canonical_payload();
    payload[layout::LICENSE_ID_OFFSET..layout::LICENSE_ID_OFFSET + 4]
        .copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);

    let fod_id = FodId::from_base64(&fixture.signed_owid_base64(payload)).unwrap();

    assert_eq!(u32::MAX, fod_id.license_id());
}

#[test]
fn license_id_high_bit_set_stays_unsigned() {
    let fixture = Fixture::new();
    let mut payload = canonical_payload();
    // 0x80000000 little endian: 00 00 00 80
    payload[layout::LICENSE_ID_OFFSET..layout::LICENSE_ID_OFFSET + 4]
        .copy_from_slice(&[0x00, 0x00, 0x00, 0x80]);

    let fod_id = FodId::from_base64(&fixture.signed_owid_base64(payload)).unwrap();

    assert_eq!(0x8000_0000u32, fod_id.license_id());
}

/// Usage bits 000 are not a usage. The cloud sets bit 0 on every usage it
/// accepts, so such a payload is damaged or forged and is refused the way a
/// payload that cannot be read is, whatever the other bits say, with a
/// refusal that names what it found.
#[test]
fn a_payload_with_usage_bits_000_is_refused_and_the_refusal_names_it() {
    let fixture = Fixture::new();
    // Every combination of the type bits and the indirect bit, with the
    // usage bits clear and the payload version 0.
    for flags in [0x00u8, 0x08, 0x40, 0x48, 0x80, 0x88, 0xC0, 0xC8] {
        let mut payload = canonical_payload();
        payload[layout::FLAGS_OFFSET] = flags;
        let base64 = fixture.signed_owid_base64(payload.clone());

        let result = FodId::from_base64(&base64);
        assert_failed(&result, Status::NoUsage);
        let message = result.unwrap_err().to_string();
        assert!(
            message.starts_with("NoUsage") && message.contains("usage bits are 000"),
            "flags {flags:#010b}: the refusal did not name what it found: {message}"
        );

        // Every reading route refuses it the same way.
        let owid = fixture.signed_owid(payload);
        let bytes = owid.as_byte_array().unwrap();
        assert_failed(&FodId::from_byte_array(&bytes), Status::NoUsage);
        assert_failed(&FodId::from_owid(owid), Status::NoUsage);
    }
}

/// Only 000 is refused. The patterns 010, 100, 101 and 110 are not ones the
/// cloud writes, and they keep the highest-bit reading they always had.
#[test]
fn usage_patterns_other_than_000_keep_their_reading() {
    let fixture = Fixture::new();
    let cases = [
        (0b010, Usage::Standard),
        (0b100, Usage::Personalized),
        (0b101, Usage::Personalized),
        (0b110, Usage::Personalized),
    ];
    for (bits, expected) in cases {
        let payload = typed_payload(RANDOM_TYPE | bits, layout::GUID_LENGTH);
        let result = FodId::from_base64(&fixture.signed_owid_base64(payload));
        let fod_id = assert_parsed(&result);
        assert_eq!(fod_id.usage(), expected, "usage bits {bits:#05b}");
    }
}

#[test]
fn a_flags_byte_with_every_other_bit_set_reads_as_the_highest_usage_and_reserved() {
    let fixture = Fixture::new();
    let mut payload = canonical_payload();
    // Every bit except 4 and 5, which are the payload version. A byte of
    // 0xFF names version 3, which this crate refuses rather than reads,
    // and that refusal is covered by the payload version tests below.
    payload[layout::FLAGS_OFFSET] = 0xCF;

    let fod_id = FodId::from_base64(&fixture.signed_owid_base64(payload)).unwrap();

    assert_eq!(Usage::Personalized, fod_id.usage());
    assert!(fod_id.usage_is_indirect());
    assert_eq!(IdType::Reserved, fod_id.id_type());
    // The byte itself is still reachable through the envelope payload.
    assert_eq!(0xCF, fod_id.payload()[layout::FLAGS_OFFSET]);
}

#[test]
fn hash_is_independent_of_payload() {
    let fixture = Fixture::new();
    let fod_id = FodId::from_base64(&fixture.signed_owid_base64(canonical_payload())).unwrap();

    // The hash is an owned copy; the payload it was unpacked from is intact.
    assert_eq!(&canonical_hash(), fod_id.match_key());
    assert_eq!(
        canonical_hash()[0],
        fod_id.payload()[layout::MATCH_KEY_OFFSET]
    );
    assert_eq!(
        canonical_hash()[layout::MATCH_KEY_LENGTH - 1],
        fod_id.payload()[layout::MATCH_KEY_OFFSET + layout::MATCH_KEY_LENGTH - 1]
    );
}

#[test]
fn longer_self_hosted_creator_domain_is_accepted() {
    // A self hosted creator has whatever domain it has. The reader takes the
    // domain from the envelope and applies no expectation of its own.
    let fixture = Fixture::new();
    let domain = "identifiers.self-hosted.example-company-with-a-long-name.co.uk";
    let base64 = fixture
        .signed_owid_for(domain, canonical_payload())
        .as_base64()
        .unwrap();

    let result = FodId::from_base64(&base64);
    let fod_id = assert_parsed(&result);

    assert_eq!(fod_id.domain(), domain);
    assert_eq!(&canonical_hash(), fod_id.match_key());
    assert_eq!(
        fod_id.verify_status_with_public_key(&fixture.public_pem),
        SignatureStatus::Valid
    );
}

#[test]
fn longer_payload_is_accepted_and_the_value_still_read() {
    // A payload longer than the value the type requires is a newer shape
    // carrying more after the value, not a fault. The reader takes the
    // header and the value and leaves the rest in the payload, so a reader
    // built today keeps reading identifiers issued later. The extra lengths
    // are arbitrary.
    let fixture = Fixture::new();
    for extra in [1usize, 7, 64, 300] {
        let mut payload = canonical_payload();
        payload.extend((0..extra).map(|i| 0xC0 | (i as u8 & 0x0F)));
        let expected_payload = payload.clone();

        let result = FodId::from_base64(&fixture.signed_owid_base64(payload));
        let fod_id = assert_parsed(&result);

        assert_canonical_flags(fod_id);
        assert_eq!(CANONICAL_LICENSE_ID, fod_id.license_id(), "extra {extra}");
        assert_eq!(&canonical_hash(), fod_id.match_key(), "extra {extra}");
        assert_eq!(
            layout::MATCH_KEY_LENGTH,
            fod_id.match_key().len(),
            "extra {extra}"
        );
        assert_eq!(
            fod_id.payload(),
            expected_payload.as_slice(),
            "extra {extra}"
        );
    }
}

#[test]
fn longer_payload_is_accepted_for_every_identifier_type() {
    // Forward compatibility holds for each type, not only the probabilistic
    // one: the value length stays what the type says and the bytes after it
    // stay in the payload.
    let fixture = Fixture::new();
    let cases = [
        (
            PROBABILISTIC_FLAGS,
            IdType::Probabilistic,
            layout::MATCH_KEY_LENGTH,
        ),
        (RANDOM_FLAGS, IdType::Random, layout::GUID_LENGTH),
        (
            HASHED_EMAIL_FLAGS,
            IdType::HashedEmail,
            layout::MATCH_KEY_LENGTH,
        ),
    ];
    for (flags, id_type, value_len) in cases {
        let payload = typed_payload(flags, value_len + 40);
        let result = FodId::from_base64(&fixture.signed_owid_base64(payload));
        let fod_id = assert_parsed(&result);
        assert_eq!(fod_id.id_type(), id_type);
        assert_eq!(fod_id.match_key().len(), value_len, "{id_type:?}");
        assert_eq!(fod_id.match_key()[0], 0x50, "{id_type:?}");
        assert_eq!(
            fod_id.payload().len(),
            layout::HEADER_LENGTH + value_len + 40
        );
    }
}

#[test]
fn probabilistic_payload_one_byte_short_is_invalid_type_payload_length() {
    let fixture = Fixture::new();
    // 36 bytes, one short of the 37 a probabilistic identifier needs.
    let base64 = fixture.signed_owid_base64(typed_payload(
        PROBABILISTIC_FLAGS,
        layout::MATCH_KEY_LENGTH - 1,
    ));

    let result = FodId::from_base64(&base64);
    assert_failed(&result, Status::InvalidTypePayloadLength);
    assert!(matches!(
        result.unwrap_err(),
        Error::InvalidTypePayloadLength {
            id_type: IdType::Probabilistic,
            expected: layout::PAYLOAD_LENGTH,
            actual,
        } if actual == layout::PAYLOAD_LENGTH - 1
    ));
}

#[test]
fn hashed_email_payload_one_byte_short_is_invalid_type_payload_length() {
    let fixture = Fixture::new();
    let base64 = fixture.signed_owid_base64(typed_payload(
        HASHED_EMAIL_FLAGS,
        layout::MATCH_KEY_LENGTH - 1,
    ));

    let result = FodId::from_base64(&base64);
    assert_failed(&result, Status::InvalidTypePayloadLength);
    assert!(matches!(
        result.unwrap_err(),
        Error::InvalidTypePayloadLength {
            id_type: IdType::HashedEmail,
            expected: layout::PAYLOAD_LENGTH,
            actual,
        } if actual == layout::PAYLOAD_LENGTH - 1
    ));
}

#[test]
fn random_payload_shorter_than_guid_is_invalid_type_payload_length() {
    let fixture = Fixture::new();
    // Header present, but one short of the 16 GUID bytes.
    let base64 = fixture.signed_owid_base64(typed_payload(RANDOM_FLAGS, layout::GUID_LENGTH - 1));

    let result = FodId::from_base64(&base64);
    assert_failed(&result, Status::InvalidTypePayloadLength);
    assert!(matches!(
        result.unwrap_err(),
        Error::InvalidTypePayloadLength {
            id_type: IdType::Random,
            expected: layout::RANDOM_PAYLOAD_LENGTH,
            actual,
        } if actual == layout::RANDOM_PAYLOAD_LENGTH - 1
    ));
}

#[test]
fn random_payload_with_only_the_header_is_invalid_type_payload_length() {
    // The header is complete, so the type is read, and then the value is
    // missing entirely.
    let fixture = Fixture::new();
    let base64 = fixture.signed_owid_base64(typed_payload(RANDOM_FLAGS, 0));

    let result = FodId::from_base64(&base64);
    assert_failed(&result, Status::InvalidTypePayloadLength);
    assert!(matches!(
        result.unwrap_err(),
        Error::InvalidTypePayloadLength {
            id_type: IdType::Random,
            expected: layout::RANDOM_PAYLOAD_LENGTH,
            actual: layout::HEADER_LENGTH,
        }
    ));
}

#[test]
fn payload_shorter_than_the_header_is_payload_too_short() {
    // With fewer than the header's 5 bytes the type cannot even be read, so
    // a payload naming version 0, or an empty one, is given the header
    // status whatever the usage and type bits say.
    let fixture = Fixture::new();
    for length in 0..layout::HEADER_LENGTH {
        let mut payload = vec![0u8; length];
        if length > 0 {
            payload[layout::FLAGS_OFFSET] = RANDOM_FLAGS;
        }
        let base64 = fixture.signed_owid_base64(payload);

        let result = FodId::from_base64(&base64);
        assert_failed(&result, Status::PayloadTooShort);
        assert!(
            matches!(
                result.unwrap_err(),
                Error::PayloadTooShort {
                    expected: layout::HEADER_LENGTH,
                    actual,
                } if actual == length
            ),
            "payload of {length} bytes"
        );
    }
}

#[test]
fn a_short_payload_naming_another_version_is_refused_as_that_version() {
    // The header's length is part of the version 0 layout, so a payload too
    // short for it that names another version is refused naming the version
    // it found, as identifier-layout.md "Version" asks. An empty payload has
    // no flags byte to name a version, so it stays too short.
    let fixture = Fixture::new();
    for version in [1u8, 2, 3] {
        for length in 1..layout::HEADER_LENGTH {
            let mut payload = vec![0u8; length];
            payload[layout::FLAGS_OFFSET] = RANDOM_FLAGS;
            let payload = with_payload_version(&payload, version);

            match FodId::from_base64(&fixture.signed_owid_base64(payload)) {
                Err(Error::UnsupportedPayloadVersion { version: found }) => {
                    assert_eq!(found, version, "payload of {length} bytes");
                }
                other => panic!("version {version}, {length} bytes: {other:?}"),
            }
        }
    }
}

#[test]
fn constructor_from_owid_short_payload_errors() {
    // Promoting an OWID whose payload is too short is rejected through the
    // same checks as the other reading routes.
    let fixture = Fixture::new();

    let result = FodId::from_owid(fixture.signed_owid(vec![0u8; layout::HEADER_LENGTH - 1]));
    assert_failed(&result, Status::PayloadTooShort);

    let result = FodId::from_owid(fixture.signed_owid(typed_payload(
        PROBABILISTIC_FLAGS,
        layout::MATCH_KEY_LENGTH - 1,
    )));
    assert_failed(&result, Status::InvalidTypePayloadLength);
}

#[test]
fn constructor_from_bytes_short_payload_errors() {
    let fixture = Fixture::new();

    let bytes = fixture
        .signed_owid(vec![0u8; layout::HEADER_LENGTH - 1])
        .as_byte_array()
        .unwrap();
    assert_failed(&FodId::from_byte_array(&bytes), Status::PayloadTooShort);

    let bytes = fixture
        .signed_owid(typed_payload(
            PROBABILISTIC_FLAGS,
            layout::MATCH_KEY_LENGTH - 1,
        ))
        .as_byte_array()
        .unwrap();
    assert_failed(
        &FodId::from_byte_array(&bytes),
        Status::InvalidTypePayloadLength,
    );
}

#[test]
fn invalid_base64_is_the_owid_invalid_base64_status() {
    let result = FodId::from_base64("This is not valid Base64!@#$");
    assert_failed(&result, Status::Owid(ParseStatus::InvalidBase64));
}

#[test]
fn empty_input_is_the_owid_missing_input_status() {
    // Rust has no null, so absent input is the empty string or the empty
    // buffer, and both are the OWID missing input status.
    assert_failed(
        &FodId::from_base64(""),
        Status::Owid(ParseStatus::MissingInput),
    );
    assert_failed(
        &FodId::from_byte_array(&[]),
        Status::Owid(ParseStatus::MissingInput),
    );
}

#[test]
fn owid_declaration_mismatch_is_propagated_unchanged() {
    // A byte after the signature makes the declared payload length disagree
    // with the bytes present. The OWID status for that comes through as it
    // is, not mapped down to a generic failure, and the 51Did payload rules
    // are never reached because no envelope formed.
    let fixture = Fixture::new();
    let mut bytes = fixture
        .signed_owid(canonical_payload())
        .as_byte_array()
        .unwrap();
    bytes.push(0);

    let result = FodId::from_byte_array(&bytes);
    assert_failed(&result, Status::Owid(ParseStatus::ByteCountMismatch));

    // The whole OWID parse error is kept as the source, detail included.
    let Err(Error::Parse(parse_error)) = result else {
        panic!("expected the OWID parse error to be carried unchanged");
    };
    assert_eq!(parse_error.status(), ParseStatus::ByteCountMismatch);
    assert!(parse_error.detail().is_some());
}

#[test]
fn truncated_envelope_is_the_owid_unexpected_end_status() {
    let fixture = Fixture::new();
    let bytes = fixture
        .signed_owid(canonical_payload())
        .as_byte_array()
        .unwrap();

    // Cut inside the domain, before any length field was read.
    let result = FodId::from_byte_array(&bytes[..3]);
    assert_failed(&result, Status::Owid(ParseStatus::UnexpectedEnd));
}

#[test]
fn unsupported_version_is_the_owid_unsupported_version_status() {
    let result = FodId::from_byte_array(&[9, 9, 9]);
    assert_failed(&result, Status::Owid(ParseStatus::UnsupportedVersion));
}

/// The bytes of a signed 51Did under the test domain, with the first bytes
/// of the domain replaced after signing. Reading never looks at the
/// signature, so the bytes still read as an envelope.
fn with_domain_starting(fixture: &Fixture, replacement: &[u8]) -> Vec<u8> {
    let mut bytes = fixture
        .signed_owid(canonical_payload())
        .as_byte_array()
        .unwrap();
    // The domain starts after the version byte.
    bytes[1..1 + replacement.len()].copy_from_slice(replacement);
    bytes
}

#[test]
fn a_creator_domain_that_is_not_ascii_is_invalid_domain_encoding() {
    // identifier-layout.md gives the creator domain as ASCII text. The OWID
    // reader accepts any UTF-8, so the 51Did reader refuses a domain that is
    // UTF-8 but not ASCII, with the status the OWID reader gives a domain
    // that is not UTF-8, on every reading route.
    use base64::Engine as _;

    let fixture = Fixture::new();
    // "é" as UTF-8 in place of the "51" of the test domain.
    let bytes = with_domain_starting(&fixture, &[0xC3, 0xA9]);
    let owid = Owid::from_byte_array(&bytes).expect("the OWID reader takes UTF-8");
    assert_eq!(owid.domain(), "édegrees.com");
    let base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

    for result in [
        FodId::from_byte_array(&bytes),
        FodId::from_base64(&base64),
        base64.parse::<FodId>(),
        FodId::try_from(bytes.as_slice()),
        FodId::from_owid(owid.clone()),
        FodId::try_from(owid),
    ] {
        assert_failed(&result, Status::Owid(ParseStatus::InvalidDomainEncoding));
        let Err(Error::Parse(error)) = result else {
            panic!("expected the OWID parse error");
        };
        assert_eq!(
            error.detail(),
            Some(ParseDetail::Field("the creator domain"))
        );
    }
}

#[test]
fn a_creator_domain_reads_up_to_the_last_ascii_byte() {
    // 7F is the last ASCII byte, so a domain holding it reads. 80 is not
    // UTF-8 on its own, so the OWID reader refuses it with the same status
    // as a domain that is UTF-8 but not ASCII.
    let fixture = Fixture::new();

    let result = FodId::from_byte_array(&with_domain_starting(&fixture, &[0x7F]));
    assert_eq!(assert_parsed(&result).domain(), "\u{7f}1degrees.com");

    let result = FodId::from_byte_array(&with_domain_starting(&fixture, &[0x80]));
    assert_failed(&result, Status::Owid(ParseStatus::InvalidDomainEncoding));
}

#[test]
fn date_past_the_year_9999_reads_as_15_february_10186() {
    // The four byte minute count of the wire format runs to 0xFFFFFFFF,
    // which is 04:15 on 15 February 10186. Runtimes whose date type stops
    // at the end of the year 9999 (.NET and Python) refuse such a count
    // with ImplementationCapacityExceeded, whereas chrono's range reaches
    // the year 262142, so here the same bytes read fine and the date is
    // the one the sender wrote. The bytes are changed after signing, which
    // is fine because reading never looks at the signature.
    use base64::Engine as _;
    use chrono::{Datelike as _, Timelike as _};

    let fixture = Fixture::new();
    let mut bytes = fixture
        .signed_owid(canonical_payload())
        .as_byte_array()
        .unwrap();
    // The four little endian date bytes sit after the version byte, the
    // domain and its terminator.
    let at = 1 + TEST_DOMAIN.len() + 1;
    bytes[at..at + 4].copy_from_slice(&[0xFF; 4]);
    let base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

    let result = FodId::from_base64(&base64);
    let fod_id = assert_parsed(&result);
    let date = fod_id.date();
    assert_eq!(
        (
            date.year(),
            date.month(),
            date.day(),
            date.hour(),
            date.minute()
        ),
        (10186, 2, 15, 4, 15)
    );
    assert_eq!(fod_id.match_key(), &canonical_hash());
}

#[test]
fn error_display_names_the_status_and_keeps_the_owid_source() {
    use std::error::Error as _;

    let error = FodId::from_base64("not base 64!").unwrap_err();
    assert!(
        error.source().is_some(),
        "the OWID parse error is the source"
    );
    assert!(error.to_string().contains("InvalidBase64"));

    let fixture = Fixture::new();
    let error = FodId::from_base64(&fixture.signed_owid_base64(vec![0u8; 2])).unwrap_err();
    assert!(error.source().is_none());
    assert!(error.to_string().starts_with("PayloadTooShort"));

    let error = FodId::from_base64(&fixture.signed_owid_base64(typed_payload(RANDOM_FLAGS, 3)))
        .unwrap_err();
    assert!(error.source().is_none());
    assert!(error.to_string().starts_with("InvalidTypePayloadLength"));
}

#[test]
fn the_other_reading_routes_report_the_same_statuses() {
    // FromStr, TryFrom<&[u8]> and TryFrom<Owid> delegate to the same checks,
    // so a caller gets the same answer whichever route they use.
    let fixture = Fixture::new();

    let parsed: fodid::Result<FodId> = fixture
        .signed_owid_base64(canonical_payload())
        .parse::<FodId>();
    assert_parsed(&parsed);

    let from_str: fodid::Result<FodId> = "not base 64!".parse::<FodId>();
    assert_failed(&from_str, Status::Owid(ParseStatus::InvalidBase64));

    let short = fixture.signed_owid(vec![0u8; 1]).as_byte_array().unwrap();
    let try_from_bytes: fodid::Result<FodId> = FodId::try_from(short.as_slice());
    assert_failed(&try_from_bytes, Status::PayloadTooShort);

    let try_from_owid: fodid::Result<FodId> =
        FodId::try_from(fixture.signed_owid(typed_payload(RANDOM_FLAGS, 2)));
    assert_failed(&try_from_owid, Status::InvalidTypePayloadLength);
}

#[test]
fn fod_id_is_cryptographically_verifiable() {
    let fixture = Fixture::new();
    let fod_id = FodId::from_base64(&fixture.signed_owid_base64(canonical_payload())).unwrap();

    assert!(fod_id.verify_with_public_key(&fixture.public_pem).unwrap());
    assert_eq!(
        fod_id.verify_status_with_public_key(&fixture.public_pem),
        SignatureStatus::Valid
    );
}

#[test]
fn a_cryptographically_invalid_51did_parses_and_then_verifies_as_invalid() {
    // Reading and verifying are two questions. A structurally valid 51Did
    // whose payload was altered after signing reads successfully, with the
    // altered value, and only the signature check says it is not genuine.
    let fixture = Fixture::new();
    let envelope = fixture.signed_owid(canonical_payload());
    let signature_length = envelope.signature().len();
    let mut bytes = envelope.as_byte_array().unwrap();
    // The payload is the 37 bytes before the signature. Flip a bit in the
    // hash without changing any length.
    let hash_start = bytes.len() - signature_length - layout::MATCH_KEY_LENGTH;
    bytes[hash_start] ^= 0x01;

    let result = FodId::from_byte_array(&bytes);
    let fod_id = assert_parsed(&result);
    assert_ne!(fod_id.match_key(), &canonical_hash());

    assert_eq!(
        fod_id.verify_status_with_public_key(&fixture.public_pem),
        SignatureStatus::Invalid
    );
    assert!(!fod_id.verify_with_public_key(&fixture.public_pem).unwrap());
}

#[test]
fn a_51did_signed_by_another_key_verifies_as_invalid() {
    let issuer = Fixture::new();
    let someone_else = Fixture::new();
    let fod_id = FodId::from_base64(&issuer.signed_owid_base64(canonical_payload())).unwrap();

    assert_eq!(
        fod_id.verify_status_with_public_key(&someone_else.public_pem),
        SignatureStatus::Invalid
    );
}

#[test]
fn a_key_that_cannot_be_read_is_not_signature_invalid() {
    // A check that could not be made must never read as a forgery. Key
    // material that cannot be read leaves the signature unjudged, which is
    // a different answer from a signature that does not match. Every
    // `Crypto` this crate can hold in hand carries a verifying key, so the
    // key unavailable status is reached only through the OWID `fetch`
    // feature, which this crate does not enable, and it is asserted here
    // only to be distinct from the forgery answer.
    let fixture = Fixture::new();
    let fod_id = FodId::from_base64(&fixture.signed_owid_base64(canonical_payload())).unwrap();

    let status = fod_id.verify_status_with_public_key("not a PEM");
    assert_eq!(status, SignatureStatus::InvalidKey);
    assert_ne!(status, SignatureStatus::Invalid);
    assert_ne!(SignatureStatus::KeyUnavailable, SignatureStatus::Invalid);

    // The Result form of the same check is an error, never Ok(false).
    let result = fod_id.verify_with_public_key("not a PEM");
    assert!(result.is_err());
    // And that error, taken into this crate's error type through `?`, is
    // the exceptional variant rather than a read failure.
    let error: Error = result.unwrap_err().into();
    assert!(matches!(error, Error::Owid(_)));
}

#[test]
fn base64_roundtrip_preserves_all_fields() {
    let fixture = Fixture::new();
    let fod_id1 = FodId::from_base64(&fixture.signed_owid_base64(canonical_payload())).unwrap();
    let fod_id2 = FodId::from_base64(&fod_id1.as_base64().unwrap()).unwrap();

    assert_eq!(fod_id1.usage(), fod_id2.usage());
    assert_eq!(fod_id1.usage_is_indirect(), fod_id2.usage_is_indirect());
    assert_eq!(fod_id1.id_type(), fod_id2.id_type());
    assert_eq!(fod_id1.license_id(), fod_id2.license_id());
    assert_eq!(fod_id1.match_key(), fod_id2.match_key());
    assert_eq!(fod_id1.domain(), fod_id2.domain());
    assert_eq!(
        fod_id1.as_byte_array().unwrap(),
        fod_id2.as_byte_array().unwrap()
    );
}

#[test]
fn id_type_decodes_from_flag_bits_6_and_7() {
    let fixture = Fixture::new();
    // The lower usage bits do not affect the decoded type.
    let cases = [
        (
            PROBABILISTIC_FLAGS,
            IdType::Probabilistic,
            layout::MATCH_KEY_LENGTH,
        ),
        (RANDOM_FLAGS, IdType::Random, layout::GUID_LENGTH),
        (
            HASHED_EMAIL_FLAGS,
            IdType::HashedEmail,
            layout::MATCH_KEY_LENGTH,
        ),
    ];
    for (flags, expected_type, value_len) in cases {
        let payload = typed_payload(flags, value_len);
        let fod_id = FodId::from_base64(&fixture.signed_owid_base64(payload)).unwrap();
        assert_eq!(fod_id.id_type(), expected_type, "flags {flags:#010b}");
        assert_eq!(fod_id.match_key().len(), value_len);
        assert_eq!(fod_id.license_id(), CANONICAL_LICENSE_ID);
    }
}

/// The usage is the highest granted, because the bits are cumulative. A
/// mask for the non-marketing bit alone would say yes for every marketing
/// identifier, which is the wrong answer for a data protection decision.
#[test]
fn usage_is_the_highest_granted() {
    let fixture = Fixture::new();
    let cases = [
        (0b001, Usage::NonMarketing, "non-marketing"),
        (0b011, Usage::Standard, "standard"),
        (0b111, Usage::Personalized, "personalized"),
    ];
    for (bits, expected, id_usage) in cases {
        let payload = typed_payload(RANDOM_TYPE | bits, layout::GUID_LENGTH);
        let fod_id = FodId::from_base64(&fixture.signed_owid_base64(payload)).unwrap();
        assert_eq!(fod_id.usage(), expected, "usage bits {bits:#05b}");
        assert_eq!(fod_id.usage().id_usage(), id_usage);
        assert_eq!(
            fod_id.id_type(),
            IdType::Random,
            "the type bits are untouched"
        );
        assert!(!fod_id.usage_is_indirect());
    }
}

/// Bit 3 records whether the usage is indirect, being worked out by the
/// issuer from another signal rather than stated by the caller. It reads
/// independently of which usage it is, true when set and false when clear.
#[test]
fn usage_is_indirect_is_bit_three() {
    let fixture = Fixture::new();
    for (bits, usage) in [
        (0b001, Usage::NonMarketing),
        (0b011, Usage::Standard),
        (0b111, Usage::Personalized),
    ] {
        let set = typed_payload(RANDOM_TYPE | 0b1000 | bits, layout::GUID_LENGTH);
        let fod_id = FodId::from_base64(&fixture.signed_owid_base64(set)).unwrap();
        assert!(fod_id.usage_is_indirect(), "bit 3 set, usage {usage:?}");
        assert_eq!(fod_id.usage(), usage);

        let clear = typed_payload(RANDOM_TYPE | bits, layout::GUID_LENGTH);
        let fod_id = FodId::from_base64(&fixture.signed_owid_base64(clear)).unwrap();
        assert!(!fod_id.usage_is_indirect(), "bit 3 clear, usage {usage:?}");
        assert_eq!(fod_id.usage(), usage);
    }
}

#[test]
fn random_identifier_carries_a_16_byte_guid() {
    let fixture = Fixture::new();
    let payload = typed_payload(RANDOM_FLAGS, layout::GUID_LENGTH);
    let fod_id = FodId::from_base64(&fixture.signed_owid_base64(payload)).unwrap();

    assert_eq!(fod_id.id_type(), IdType::Random);
    assert_eq!(fod_id.match_key().len(), layout::GUID_LENGTH);
    assert_eq!(fod_id.match_key()[0], 0x50);
    assert_eq!(fod_id.match_key()[layout::GUID_LENGTH - 1], 0x50 + 15);
}

#[test]
fn reserved_type_exposes_remaining_payload_best_effort() {
    let fixture = Fixture::new();
    // Eight arbitrary value bytes after the header.
    let payload = typed_payload(RESERVED_FLAGS, 8);
    let fod_id = FodId::from_base64(&fixture.signed_owid_base64(payload)).unwrap();

    assert_eq!(fod_id.id_type(), IdType::Reserved);
    assert_eq!(fod_id.match_key().len(), 8);
    assert_eq!(fod_id.match_key()[0], 0x50);

    // A reserved payload holding only the header has an empty value, which
    // is the documented best effort answer rather than a failure.
    let payload = typed_payload(RESERVED_FLAGS, 0);
    let result = FodId::from_base64(&fixture.signed_owid_base64(payload));
    let fod_id = assert_parsed(&result);
    assert!(fod_id.match_key().is_empty());
}

/// The canonical payload as an issuer writes one, carrying the payload
/// version 0 in its flags byte and the terms byte of the document a
/// personalized marketing identifier is created under.
fn canonical_payload() -> Vec<u8> {
    payload_with_terms(MODEL_TERMS_INDEX)
}

/// The payload with its version bits set to the given version, leaving every
/// other bit of the flags byte alone.
fn with_payload_version(payload: &[u8], version: u8) -> Vec<u8> {
    let mut changed = payload.to_vec();
    changed[layout::FLAGS_OFFSET] = (payload[layout::FLAGS_OFFSET] & 0b1100_1111) | (version << 4);
    changed
}

/// The terms index the specification gives to the Model Terms for Marketing,
/// version 2, and the address that index stands for. Both are written out
/// here rather than taken from the crate, so the test checks the crate
/// against the specification rather than against itself.
const MODEL_TERMS_INDEX: u8 = 1;

const MODEL_TERMS_URL: &str = "https://m4ow.uk/mtm/2.txt";

/// An index no release of this crate knows, standing in for one added to the
/// specification after this one.
const UNKNOWN_TERMS_INDEX: u8 = 200;

/// The canonical payload with a terms index byte after the match key, which
/// is the shape an issuer writes. The byte is appended by hand, because the
/// offset it lands at is what these tests are checking.
fn payload_with_terms(index: u8) -> Vec<u8> {
    let mut payload = payload_ending_at_match_key();
    payload.push(index);
    payload
}

#[test]
fn a_payload_ending_at_the_match_key_has_no_terms_address() {
    // There is no byte after the match key to read. A missing byte is index
    // 0, which says the terms are not stated in the identifier, so such an
    // identifier answers with no address and every other field reads as it
    // does with the byte present.
    let fixture = Fixture::new();
    let result = FodId::from_base64(&fixture.signed_owid_base64(payload_ending_at_match_key()));
    let fod_id = assert_parsed(&result);

    assert_eq!(fod_id.terms(), None);

    // Every other field reads as it does with the byte present.
    assert_canonical_flags(fod_id);
    assert_eq!(fod_id.license_id(), CANONICAL_LICENSE_ID);
    assert_eq!(fod_id.match_key(), &canonical_hash());
}

#[test]
fn an_explicit_zero_reads_the_same_as_a_missing_terms_byte() {
    // Absence and zero say the same thing, so nothing has to tell them
    // apart and no presence flag is needed.
    let fixture = Fixture::new();
    let absent =
        FodId::from_base64(&fixture.signed_owid_base64(payload_ending_at_match_key())).unwrap();
    let zero = FodId::from_base64(&fixture.signed_owid_base64(payload_with_terms(0))).unwrap();

    assert_eq!(absent.terms(), zero.terms());
    assert_eq!(zero.terms(), None);

    // The payloads still differ by the byte, which stays in place.
    assert_eq!(absent.payload().len() + 1, zero.payload().len());
}

#[test]
fn index_one_is_the_model_terms_for_marketing_and_carries_its_address() {
    let fixture = Fixture::new();
    let payload = payload_with_terms(MODEL_TERMS_INDEX);
    let result = FodId::from_base64(&fixture.signed_owid_base64(payload));
    let fod_id = assert_parsed(&result);

    assert_eq!(fod_id.terms(), Some(MODEL_TERMS_URL));
    assert_eq!(fod_id.match_key(), &canonical_hash());
}

#[test]
fn an_index_this_crate_does_not_know_has_no_address() {
    // No address is ever built from an index this crate cannot name,
    // because that would name a document nobody wrote and a receiver would
    // record having accepted terms that do not exist.
    let fixture = Fixture::new();
    for index in [2u8, 127, UNKNOWN_TERMS_INDEX, 255] {
        let payload = payload_with_terms(index);
        let result = FodId::from_base64(&fixture.signed_owid_base64(payload));
        let fod_id = assert_parsed(&result);

        assert_eq!(fod_id.terms(), None, "index {index}");
    }
}

#[test]
fn no_terms_stated_and_an_unknown_index_both_have_no_address() {
    // A caller cannot tell the two apart, which is deliberate, since both
    // say the identifier does not give the terms and the answer has to come
    // from somewhere else.
    let fixture = Fixture::new();
    let stated_none =
        FodId::from_base64(&fixture.signed_owid_base64(payload_with_terms(0))).unwrap();
    let unknown =
        FodId::from_base64(&fixture.signed_owid_base64(payload_with_terms(UNKNOWN_TERMS_INDEX)))
            .unwrap();

    assert_eq!(stated_none.terms(), None);
    assert_eq!(unknown.terms(), None);
}

#[test]
fn every_terms_index_decodes_as_the_specification_publishes_it() {
    // The published table holds two indexes today. Everything else is an
    // index added later, whatever its value, including the one immediately
    // after the last published index and the largest a byte can hold.
    let fixture = Fixture::new();
    let cases = [
        (0u8, None),
        (1, Some(MODEL_TERMS_URL)),
        (2, None),
        (UNKNOWN_TERMS_INDEX, None),
        (255, None),
    ];
    for (index, expected_url) in cases {
        let payload = payload_with_terms(index);
        let fod_id = FodId::from_base64(&fixture.signed_owid_base64(payload)).unwrap();

        assert_eq!(fod_id.terms(), expected_url, "index {index}");
    }
}

#[test]
fn the_terms_byte_is_read_after_the_match_key_for_both_match_key_lengths() {
    // The terms byte follows the match key, so where it sits moves with the
    // length the identifier type requires. Reading it at a fixed offset
    // would take a hash byte for a random identifier.
    let fixture = Fixture::new();
    let cases = [
        (
            PROBABILISTIC_FLAGS,
            IdType::Probabilistic,
            layout::MATCH_KEY_LENGTH,
        ),
        (RANDOM_FLAGS, IdType::Random, layout::GUID_LENGTH),
        (
            HASHED_EMAIL_FLAGS,
            IdType::HashedEmail,
            layout::MATCH_KEY_LENGTH,
        ),
    ];
    for (flags, id_type, value_len) in cases {
        let mut payload = typed_payload(flags, value_len);
        payload.push(MODEL_TERMS_INDEX);
        let fod_id = FodId::from_base64(&fixture.signed_owid_base64(payload)).unwrap();

        assert_eq!(fod_id.id_type(), id_type);
        assert_eq!(fod_id.match_key().len(), value_len, "{id_type:?}");
        assert_eq!(fod_id.match_key()[0], 0x50, "{id_type:?}");
        assert_eq!(
            fod_id.match_key()[value_len - 1],
            0x50 + (value_len as u8 - 1),
            "{id_type:?}"
        );
        assert_eq!(fod_id.terms(), Some(MODEL_TERMS_URL), "{id_type:?}");
    }
}

#[test]
fn a_creator_context_after_the_terms_leaves_both_the_match_key_and_terms_read() {
    // The terms sit between the match key and the creator context, so a
    // payload carrying a context proves the byte is taken from the right
    // offset rather than from the end of the payload. The context bytes are
    // arbitrary here, because their meaning belongs to the issuer and this
    // crate leaves them in the payload untouched.
    let fixture = Fixture::new();
    for context_len in [1usize, 40, 300] {
        let mut payload = payload_with_terms(MODEL_TERMS_INDEX);
        payload.extend((0..context_len).map(|i| 0xC0 | (i as u8 & 0x0F)));
        let expected_payload = payload.clone();

        let result = FodId::from_base64(&fixture.signed_owid_base64(payload));
        let fod_id = assert_parsed(&result);

        assert_eq!(fod_id.match_key(), &canonical_hash(), "{context_len}");
        assert_eq!(fod_id.terms(), Some(MODEL_TERMS_URL), "{context_len}");
        assert_eq!(fod_id.license_id(), CANONICAL_LICENSE_ID, "{context_len}");
        assert_eq!(
            fod_id.payload(),
            expected_payload.as_slice(),
            "{context_len}"
        );
    }
}

#[test]
fn a_reserved_identifier_states_no_terms() {
    // A reserved type has no defined match key length, so every byte after
    // the header is read as its value best effort and there is no byte the
    // terms could be taken from. Index 0 is the answer, being the same one
    // an identifier that ends at its match key gives.
    let fixture = Fixture::new();
    let mut payload = typed_payload(RESERVED_FLAGS, 8);
    payload.push(MODEL_TERMS_INDEX);
    let fod_id = FodId::from_base64(&fixture.signed_owid_base64(payload)).unwrap();

    assert_eq!(fod_id.id_type(), IdType::Reserved);
    assert_eq!(fod_id.terms(), None);
}

#[test]
fn the_terms_survive_a_base64_round_trip() {
    let fixture = Fixture::new();
    let payload = payload_with_terms(MODEL_TERMS_INDEX);
    let fod_id = FodId::from_base64(&fixture.signed_owid_base64(payload)).unwrap();

    let round_tripped = FodId::from_base64(&fod_id.as_base64().unwrap()).unwrap();

    assert_eq!(round_tripped.terms(), fod_id.terms());
    assert_eq!(round_tripped.terms(), Some(MODEL_TERMS_URL));
}

#[test]
fn payload_version_zero_reads_every_field() {
    // Bits 4 and 5 clear is version 0, which is the layout this crate
    // reads, so every field reads as it does on the canonical payload.
    let fixture = Fixture::new();
    let fod_id = FodId::from_base64(&fixture.signed_owid_base64(canonical_payload())).unwrap();

    assert_canonical_flags(&fod_id);
    assert_eq!(fod_id.license_id(), CANONICAL_LICENSE_ID);
    assert_eq!(fod_id.match_key(), &canonical_hash());
    assert_eq!(fod_id.terms(), Some(MODEL_TERMS_URL));
}

#[test]
fn an_unassigned_payload_version_is_refused_and_names_the_version() {
    // Versions 1, 2 and 3 are not assigned, so a payload naming one is
    // refused rather than read under the layout this crate knows, and
    // nothing is handed back because there is no identifier to expose
    // fields for when the layout was not understood.
    let fixture = Fixture::new();
    for version in [1u8, 2, 3] {
        let payload = with_payload_version(&canonical_payload(), version);
        let result = FodId::from_base64(&fixture.signed_owid_base64(payload));

        match result {
            Err(Error::UnsupportedPayloadVersion { version: found }) => {
                assert_eq!(found, version);
            }
            other => panic!("version {version} was not refused: {other:?}"),
        }
    }
}

#[test]
fn a_refused_payload_version_names_itself_in_the_message() {
    let fixture = Fixture::new();
    for version in [1u8, 2, 3] {
        let payload = with_payload_version(&canonical_payload(), version);
        let error = FodId::from_base64(&fixture.signed_owid_base64(payload)).unwrap_err();

        let message = error.to_string();
        assert!(
            message.contains(&format!("version {version}")),
            "message did not name the version: {message}"
        );
    }
}

#[test]
fn the_payload_version_is_read_apart_from_the_usage_and_type_bits() {
    // A reader masking the wrong bits would refuse a version 0 identifier
    // or let a later version through, so every combination of the usage and
    // type bits is tried.
    let fixture = Fixture::new();
    for usage in [0b000u8, 0b001, 0b011, 0b111] {
        for id_type in [0b00u8, 0b10, 0b11] {
            let flags = (id_type << 6) | usage;
            let mut payload = payload_ending_at_match_key();
            payload[layout::FLAGS_OFFSET] = flags;

            // Usage bits 000 are refused at version 0, and every other
            // usage reads. The version is checked first, so a later version
            // is refused as a version whatever the usage bits say.
            let read = FodId::from_base64(&fixture.signed_owid_base64(payload.clone()));
            if usage == 0 {
                assert!(matches!(read, Err(Error::NoUsage)), "flags {flags}");
            } else {
                assert!(read.is_ok(), "flags {flags}");
            }

            for version in [1u8, 2, 3] {
                let refused = FodId::from_base64(
                    &fixture.signed_owid_base64(with_payload_version(&payload, version)),
                );
                assert!(
                    matches!(refused, Err(Error::UnsupportedPayloadVersion { .. })),
                    "flags {flags} version {version}"
                );
            }
        }
    }
}

#[test]
fn from_base64_ignores_surrounding_whitespace() {
    let fixture = Fixture::new();
    // A payload chosen so the encoding needs padding, which the URL-safe
    // form leaves out and the reader works back from the stripped length.
    let mut payload = canonical_payload();
    for (i, b) in payload[layout::MATCH_KEY_OFFSET..].iter_mut().enumerate() {
        *b = 0xFB + (i as u8 % 5);
    }
    let standard = fixture.signed_owid_base64(payload);
    assert!(standard.ends_with('='));
    let url_safe = standard
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_owned();

    // Compared as bytes, because `==` compares only the type and match key.
    let envelope = |text: &str| FodId::from_base64(text).unwrap().as_byte_array().unwrap();
    let expected = envelope(&standard);
    for clean in [&standard, &url_safe] {
        for spaced in [
            format!("{clean}\n"),
            format!(" {clean}"),
            format!("{clean} "),
            format!("\r\n\t {clean} \t\r\n"),
        ] {
            assert_eq!(envelope(&spaced), expected);
        }
    }
}

/// A signed 51Did in the URL-safe alphabet without padding whose length is
/// `over` characters over a group of four, found by adding bytes after the
/// payload's value one at a time, with what the clean text reads as.
fn url_safe_text_over_a_group_of_four(fixture: &Fixture, over: usize) -> (String, FodId) {
    for extra in 0..3 {
        let mut payload = canonical_payload();
        payload.resize(payload.len() + extra, 0xA5);
        let url_safe = fixture
            .signed_owid_base64(payload)
            .replace('+', "-")
            .replace('/', "_")
            .trim_end_matches('=')
            .to_owned();
        if url_safe.len() % 4 == over {
            let clean = FodId::from_base64(&url_safe).expect("the clean text reads");
            return (url_safe, clean);
        }
    }
    panic!("no payload length gives a text {over} over a group of four");
}

/// Puts `fill` after each character position given, counting from 1.
fn insert_after(text: &str, positions: &[usize], fill: &str) -> String {
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        out.push(c);
        if positions.contains(&(i + 1)) {
            out.push_str(fill);
        }
    }
    out
}

/// White space inside the text is refused, whatever its kind, how much of
/// it there is and how long the text is, whilst white space only before
/// and after the text is ignored. Each row is the case of the same name in
/// `parity/corpus/fodid.json`.
#[test]
fn white_space_inside_the_text_is_refused() {
    let fixture = Fixture::new();
    let (whole, whole_read) = url_safe_text_over_a_group_of_four(&fixture, 0);
    let (short, _) = url_safe_text_over_a_group_of_four(&fixture, 3);
    let standard = format!("{}=", short.replace('-', "+").replace('_', "/"));
    let (w, s) = (whole.len(), short.len());
    let invalid = Err(Status::Owid(ParseStatus::InvalidBase64));

    let cases: [(&str, String, Result<&FodId, Status>); 14] = [
        (
            "text-url-space-1",
            insert_after(&whole, &[w / 2], " "),
            invalid,
        ),
        (
            "text-url-space-2",
            insert_after(&whole, &[w / 4, 3 * w / 4], " "),
            invalid,
        ),
        (
            "text-url-space-3",
            insert_after(&whole, &[w / 4, w / 2, 3 * w / 4], " "),
            invalid,
        ),
        (
            "text-url-tabs",
            insert_after(&whole, &[w / 5, 2 * w / 5, 3 * w / 5, 4 * w / 5], "\t"),
            invalid,
        ),
        (
            "text-url-line-breaks",
            insert_after(&whole, &[w / 3, 2 * w / 3], "\r\n"),
            invalid,
        ),
        (
            "text-url-no-break-spaces",
            insert_after(&whole, &[w / 5, 2 * w / 5, 3 * w / 5, 4 * w / 5], "\u{a0}"),
            invalid,
        ),
        (
            "text-url-no-break-space-surrounded",
            format!("\u{a0}{whole}\u{a0}"),
            Ok(&whole_read),
        ),
        (
            "text-url-unpadded-space-1",
            insert_after(&short, &[s / 2], " "),
            invalid,
        ),
        (
            "text-url-unpadded-space-2",
            insert_after(&short, &[s / 3, 2 * s / 3], " "),
            invalid,
        ),
        (
            "text-url-unpadded-space-3",
            insert_after(&short, &[s / 4, s / 2, 3 * s / 4], " "),
            invalid,
        ),
        (
            "text-url-unpadded-space-4",
            insert_after(&short, &[s / 5, 2 * s / 5, 3 * s / 5, 4 * s / 5], " "),
            invalid,
        ),
        (
            "text-standard-space-1",
            insert_after(&standard, &[standard.len() / 2], " "),
            invalid,
        ),
        (
            "text-url-vertical-tab",
            insert_after(&whole, &[w / 2], "\u{b}"),
            invalid,
        ),
        (
            "text-white-space",
            " \t\n\u{a0}".to_owned(),
            Err(Status::Owid(ParseStatus::MissingInput)),
        ),
    ];

    for (name, text, expected) in cases {
        let result = FodId::from_base64(&text);
        match expected {
            Ok(clean) => {
                assert_eq!(status_of(&result), Status::Parsed, "{name}");
                assert_eq!(
                    result.unwrap().as_base64().unwrap(),
                    clean.as_base64().unwrap(),
                    "{name} reads as the clean text"
                );
            }
            Err(status) => assert_eq!(status_of(&result), status, "{name}"),
        }
    }
}

/// The unused low bits of the last group are ignored, as the .NET decoder
/// ignores them, so a text whose last character sets them reads as the
/// text an encoder wrote. `text-url-unpadded-unused-bits` in
/// `parity/corpus/fodid.json` is the same case for the comparison.
#[test]
fn unused_bits_in_the_last_character_are_ignored() {
    const URL_SAFE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let fixture = Fixture::new();
    // Two characters over a group carry one byte in 12 bits and three carry
    // two bytes in 18 bits, so the last character has 4 or 2 unused bits.
    for (over, unused) in [(2, 0b1111), (3, 0b11)] {
        let (text, clean) = url_safe_text_over_a_group_of_four(&fixture, over);
        let last = URL_SAFE.find(text.chars().last().unwrap()).unwrap();
        assert_eq!(last & unused, 0, "an encoder writes the unused bits as 0");
        let changed = format!(
            "{}{}",
            &text[..text.len() - 1],
            URL_SAFE.as_bytes()[last | 1] as char
        );
        let read = FodId::from_base64(&changed);
        assert_eq!(status_of(&read), Status::Parsed, "{over} over");
        assert_eq!(
            read.unwrap().as_base64().unwrap(),
            clean.as_base64().unwrap(),
            "{over} over"
        );
    }
}

#[test]
fn from_base64_accepts_both_alphabets_with_or_without_padding() {
    let fixture = Fixture::new();
    // A payload chosen so the encoding carries both `+` and `/` and needs
    // padding, which the assertions below check rather than assume.
    let mut payload = canonical_payload();
    for (i, b) in payload[layout::MATCH_KEY_OFFSET..].iter_mut().enumerate() {
        *b = 0xFB + (i as u8 % 5);
    }
    let standard = fixture.signed_owid_base64(payload);
    assert!(standard.contains('+') || standard.contains('/'));
    assert!(standard.ends_with('='));

    let url_safe_padded = standard.replace('+', "-").replace('/', "_");
    let url_safe = url_safe_padded.trim_end_matches('=').to_owned();
    assert_ne!(url_safe, standard);

    // Each form reads back to the same envelope, compared as bytes because
    // `==` compares only the type and match key.
    let envelope = |text: &str| FodId::from_base64(text).unwrap().as_byte_array().unwrap();
    let expected = envelope(&standard);
    assert_eq!(envelope(&url_safe_padded), expected);
    assert_eq!(envelope(&url_safe), expected);
    // Padding stripped from the standard alphabet is restored too.
    assert_eq!(envelope(standard.trim_end_matches('=')), expected);
}

#[test]
fn as_base64_url_round_trips_and_carries_no_padding_or_standard_symbols() {
    let fixture = Fixture::new();
    let mut payload = canonical_payload();
    for (i, b) in payload[layout::MATCH_KEY_OFFSET..].iter_mut().enumerate() {
        *b = 0xFB + (i as u8 % 5);
    }
    let fod_id = FodId::from_base64(&fixture.signed_owid_base64(payload)).unwrap();

    let url_safe = fod_id.as_base64_url().unwrap();
    assert!(!url_safe.contains('+'));
    assert!(!url_safe.contains('/'));
    assert!(!url_safe.contains('='));
    assert_eq!(
        url_safe,
        fod_id
            .as_base64()
            .unwrap()
            .replace('+', "-")
            .replace('/', "_")
            .trim_end_matches('=')
    );
    assert_eq!(
        FodId::from_base64(&url_safe)
            .unwrap()
            .as_byte_array()
            .unwrap(),
        fod_id.as_byte_array().unwrap()
    );
}
