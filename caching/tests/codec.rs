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

//! The entry format every store of bytes writes.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fiftyone_caching::{
    decode_entry, encode_entry, Codec, DecodeError, Entry, Raw, Stored, Utf8, ENTRY_FORMAT,
};

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn entry(value: &str, expires: Option<SystemTime>) -> Entry<String> {
    Entry {
        value: value.to_string(),
        written: at(1_000_000),
        expires,
        renewed: at(1_000_030),
    }
}

/// The bytes of version 1, worked out from the layout apart from this
/// crate's code, so a change to the format fails here rather than in a
/// store that reads old entries.
#[test]
fn writes_the_documented_layout() {
    let bytes = encode_entry(
        &Utf8,
        &entry("hello", Some(at(1_003_600))),
        Some(at(1_000_600)),
    );
    // Data published is the epoch, and the value format version is 0, as
    // encode_entry writes them for a codec with no version of its own.
    let expected: [u8; 49] = [
        0x01, 0x03, 0x00, 0x80, 0xC6, 0xA4, 0x7E, 0x8D, 0x03, 0x00, 0x00, 0x2C, 0xEA, 0xA0, 0x85,
        0x8D, 0x03, 0x00, 0x00, 0x20, 0x7F, 0xD5, 0xC4, 0x90, 0x03, 0x00, 0x00, 0xF0, 0x8F, 0x57,
        0x0A, 0x8E, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x68,
        0x65, 0x6C, 0x6C, 0x6F,
    ];
    assert_eq!(bytes.as_deref(), Some(&expected[..]));
    assert_eq!(ENTRY_FORMAT, 1);
}

#[test]
fn leaves_out_the_times_that_are_not_set() {
    let entry = Entry {
        value: "hi".to_string(),
        written: at(1_000_000),
        expires: None,
        renewed: at(1_000_000),
    };
    let expected: [u8; 30] = [
        0x01, 0x00, 0x00, 0x80, 0xC6, 0xA4, 0x7E, 0x8D, 0x03, 0x00, 0x00, 0x80, 0xC6, 0xA4, 0x7E,
        0x8D, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x68, 0x69,
    ];
    assert_eq!(
        encode_entry(&Utf8, &entry, None).as_deref(),
        Some(&expected[..])
    );
}

#[test]
fn reads_back_every_combination_of_times() {
    for expires in [None, Some(at(1_003_600))] {
        for drop_at in [None, Some(at(1_000_600))] {
            let written = entry("value", expires);
            let bytes = encode_entry(&Utf8, &written, drop_at).unwrap();
            let read = decode_entry(&Utf8, &bytes).unwrap();
            assert_eq!(
                read,
                Stored {
                    entry: written,
                    drop_at,
                    published: UNIX_EPOCH,
                    value_version: 0,
                }
            );
        }
    }
}

#[test]
fn keeps_times_to_the_nanosecond() {
    let written = Entry {
        value: vec![0, 1, 2, 255],
        written: at(1_000_000) + Duration::from_nanos(123_456_789),
        expires: Some(at(2_000_000) + Duration::from_nanos(1)),
        renewed: at(1_500_000) + Duration::from_nanos(999_999_999),
    };
    let drop_at = Some(at(1_600_000) + Duration::from_nanos(7));
    let bytes = encode_entry(&Raw, &written, drop_at).unwrap();
    let read = decode_entry(&Raw, &bytes).unwrap();
    assert_eq!(read.entry, written);
    assert_eq!(read.drop_at, drop_at);
}

#[test]
fn reads_an_empty_value() {
    let written = Entry {
        value: Vec::new(),
        written: at(1),
        expires: None,
        renewed: at(1),
    };
    let bytes = encode_entry(&Raw, &written, None).unwrap();
    assert_eq!(decode_entry(&Raw, &bytes).unwrap().entry, written);
}

#[test]
fn refuses_a_version_it_does_not_know() {
    let mut bytes = encode_entry(&Utf8, &entry("value", None), None).unwrap();
    bytes[0] = 2;
    assert_eq!(
        decode_entry(&Utf8, &bytes),
        Err(DecodeError::UnknownVersion(2))
    );
    bytes[0] = 0;
    assert_eq!(
        decode_entry(&Utf8, &bytes),
        Err(DecodeError::UnknownVersion(0))
    );
}

#[test]
fn refuses_bytes_shorter_than_their_header() {
    let bytes = encode_entry(&Utf8, &entry("", Some(at(1_003_600))), Some(at(1_000_600))).unwrap();
    // Every cut short of the full header, the empty input included.
    for len in 0..44 {
        assert_eq!(
            decode_entry(&Utf8, &bytes[..len]),
            Err(DecodeError::Malformed),
            "cut to {len} bytes"
        );
    }
    assert!(decode_entry(&Utf8, &bytes).is_ok());
}

#[test]
fn refuses_flags_it_does_not_define() {
    let mut bytes = encode_entry(&Utf8, &entry("value", None), None).unwrap();
    bytes[1] = 4;
    assert_eq!(decode_entry(&Utf8, &bytes), Err(DecodeError::Malformed));
}

#[test]
fn refuses_a_value_the_codec_does_not_accept() {
    let written = Entry {
        value: vec![0xFF, 0xFE],
        written: at(1),
        expires: None,
        renewed: at(1),
    };
    let bytes = encode_entry(&Raw, &written, None).unwrap();
    assert_eq!(decode_entry(&Utf8, &bytes), Err(DecodeError::Value));
}

/// A codec that cannot write its value.
struct Refuses;

impl Codec<String> for Refuses {
    fn encode(&self, _: &String) -> Option<Vec<u8>> {
        None
    }

    fn decode(&self, _: &[u8]) -> Option<String> {
        None
    }
}

#[test]
fn writes_nothing_when_the_codec_cannot_write_the_value() {
    assert_eq!(encode_entry(&Refuses, &entry("value", None), None), None);
}

#[test]
fn keeps_order_for_times_outside_the_range() {
    // wasm32-unknown-unknown cannot hold a time before the epoch at all.
    let Some(before_epoch) = UNIX_EPOCH.checked_sub(Duration::from_secs(10)) else {
        return;
    };
    let written = Entry {
        value: "old".to_string(),
        written: before_epoch,
        expires: Some(before_epoch),
        renewed: before_epoch,
    };
    let bytes = encode_entry(&Utf8, &written, Some(before_epoch)).unwrap();
    let read = decode_entry(&Utf8, &bytes).unwrap();
    assert_eq!(read.entry.written, UNIX_EPOCH);
    assert_eq!(read.entry.expires, Some(UNIX_EPOCH));
    assert_eq!(read.drop_at, Some(UNIX_EPOCH));
    assert!(read.drop_at.unwrap() < at(1));
}
