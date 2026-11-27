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

//! The bytes a store of bytes keeps for each entry.
//!
//! Every [`ByteStore`](crate::ByteStore) holds entries in one format, so a
//! value written on one host reads back the same on any other. Version 1 is
//! laid out as follows, with every time a little-endian `u64` count of
//! nanoseconds since the Unix epoch.
//!
//! | Bytes | Field |
//! |---|---|
//! | 1 | Format version, 1 |
//! | 1 | Flags. Bit 0 is set when an expiry follows, bit 1 when a drop time follows. Other bits are 0 |
//! | 8 | Written |
//! | 8 | Renewed |
//! | 8 | Expires, when bit 0 is set |
//! | 8 | Drop time, when bit 1 is set |
//! | 8 | Data published, the published date of the data that produced the value |
//! | 2 | Value format version, a little-endian `u16`, from [`Codec::version`] |
//! | rest | The value, as its [`Codec`] wrote it |
//!
//! A time before the epoch is written as the epoch, and one after the year
//! 2554 as the largest count. Both keep their order against any real time,
//! so freshness checks still come out right.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::entry::Entry;

/// The entry format version this crate writes, and the only one it reads.
pub const ENTRY_FORMAT: u8 = 1;

const HAS_EXPIRES: u8 = 1;
const HAS_DROP_TIME: u8 = 2;
const TIME_LEN: usize = 8;
const VERSION_LEN: usize = 2;
const FIXED_LEN: usize = 2 + 3 * TIME_LEN + VERSION_LEN;

/// Turns a cached value into bytes and back, for stores that keep bytes.
///
/// [`Raw`] keeps `Vec<u8>` values as they are and [`Utf8`] keeps `String`
/// values as their text. Other types implement this, for example over
/// `serde_json`.
pub trait Codec<V> {
    /// The value's bytes, or `None` when the value cannot be written, in
    /// which case the store drops the write.
    fn encode(&self, value: &V) -> Option<Vec<u8>>;

    /// The value the bytes hold, or `None` when they do not hold one, which
    /// the store treats as a miss.
    fn decode(&self, bytes: &[u8]) -> Option<V>;

    /// The version of the format [`Codec::encode`] writes. A store answers
    /// absent for an entry written in another version, and removes it, so
    /// raise this whenever the bytes for a value change. Defaults to 0.
    fn version(&self) -> u16 {
        0
    }
}

/// Keeps `Vec<u8>` values as they are.
#[derive(Debug, Clone, Copy, Default)]
pub struct Raw;

impl Codec<Vec<u8>> for Raw {
    fn encode(&self, value: &Vec<u8>) -> Option<Vec<u8>> {
        Some(value.clone())
    }

    fn decode(&self, bytes: &[u8]) -> Option<Vec<u8>> {
        Some(bytes.to_vec())
    }
}

/// Keeps `String` values as their UTF-8 text.
#[derive(Debug, Clone, Copy, Default)]
pub struct Utf8;

impl Codec<String> for Utf8 {
    fn encode(&self, value: &String) -> Option<Vec<u8>> {
        Some(value.as_bytes().to_vec())
    }

    fn decode(&self, bytes: &[u8]) -> Option<String> {
        String::from_utf8(bytes.to_vec()).ok()
    }
}

/// An entry read back from its bytes, with the time its store must drop it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored<V> {
    /// The entry.
    pub entry: Entry<V>,
    /// When the store must stop returning the entry, or `None` for never.
    pub drop_at: Option<SystemTime>,
    /// The published date of the data that produced the value, or the
    /// epoch when the source has none.
    pub published: SystemTime,
    /// The version of the format the codec wrote the value in.
    pub value_version: u16,
}

/// Why bytes could not be read as an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecodeError {
    /// The bytes start with a format version this crate does not read, for
    /// example one written by a newer release.
    UnknownVersion(u8),
    /// The bytes are shorter than their header says, or set flags this
    /// version does not define.
    Malformed,
    /// The [`Codec`] did not accept the value's bytes.
    Value,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::UnknownVersion(version) => {
                write!(
                    f,
                    "entry format version {version} is not one this crate reads"
                )
            }
            DecodeError::Malformed => f.write_str("the entry's bytes are malformed"),
            DecodeError::Value => f.write_str("the codec did not accept the entry's value"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Writes `entry` and the time its store must drop it in the entry format,
/// with Data published as the epoch, for a source with no published date.
/// `None` when the codec cannot write the value.
pub fn encode_entry<V, C: Codec<V>>(
    codec: &C,
    entry: &Entry<V>,
    drop_at: Option<SystemTime>,
) -> Option<Vec<u8>> {
    encode_entry_published(codec, entry, drop_at, UNIX_EPOCH)
}

/// Writes `entry`, the time its store must drop it and `published`, the
/// published date of the data that produced the value, in the entry format.
/// `None` when the codec cannot write the value.
pub fn encode_entry_published<V, C: Codec<V>>(
    codec: &C,
    entry: &Entry<V>,
    drop_at: Option<SystemTime>,
    published: SystemTime,
) -> Option<Vec<u8>> {
    let value = codec.encode(&entry.value)?;
    let mut flags = 0;
    let mut times = Vec::with_capacity(2 * TIME_LEN);
    if let Some(expires) = entry.expires {
        flags |= HAS_EXPIRES;
        times.extend_from_slice(&to_nanos(expires).to_le_bytes());
    }
    if let Some(drop_at) = drop_at {
        flags |= HAS_DROP_TIME;
        times.extend_from_slice(&to_nanos(drop_at).to_le_bytes());
    }
    let mut bytes = Vec::with_capacity(FIXED_LEN + times.len() + value.len());
    bytes.push(ENTRY_FORMAT);
    bytes.push(flags);
    bytes.extend_from_slice(&to_nanos(entry.written).to_le_bytes());
    bytes.extend_from_slice(&to_nanos(entry.renewed).to_le_bytes());
    bytes.extend_from_slice(&times);
    bytes.extend_from_slice(&to_nanos(published).to_le_bytes());
    bytes.extend_from_slice(&codec.version().to_le_bytes());
    bytes.extend_from_slice(&value);
    Some(bytes)
}

/// Reads an entry and the time its store must drop it from the entry
/// format.
pub fn decode_entry<V, C: Codec<V>>(codec: &C, bytes: &[u8]) -> Result<Stored<V>, DecodeError> {
    let (header, value) = read_header(bytes)?;
    let value = codec.decode(value).ok_or(DecodeError::Value)?;
    Ok(Stored {
        entry: Entry {
            value,
            written: header.written,
            expires: header.expires,
            renewed: header.renewed,
        },
        drop_at: header.drop_at,
        published: header.published,
        value_version: header.value_version,
    })
}

/// The times at the front of an entry's bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) written: SystemTime,
    pub(crate) renewed: SystemTime,
    pub(crate) expires: Option<SystemTime>,
    pub(crate) drop_at: Option<SystemTime>,
    pub(crate) published: SystemTime,
    pub(crate) value_version: u16,
}

/// Reads the header and returns it with the value's bytes, without decoding
/// the value.
pub(crate) fn read_header(bytes: &[u8]) -> Result<(Header, &[u8]), DecodeError> {
    let (&version, rest) = bytes.split_first().ok_or(DecodeError::Malformed)?;
    if version != ENTRY_FORMAT {
        return Err(DecodeError::UnknownVersion(version));
    }
    let (&flags, mut rest) = rest.split_first().ok_or(DecodeError::Malformed)?;
    if flags & !(HAS_EXPIRES | HAS_DROP_TIME) != 0 {
        return Err(DecodeError::Malformed);
    }
    let mut time = || -> Result<SystemTime, DecodeError> {
        let (head, tail) = rest
            .split_first_chunk::<TIME_LEN>()
            .ok_or(DecodeError::Malformed)?;
        rest = tail;
        Ok(from_nanos(u64::from_le_bytes(*head)))
    };
    let written = time()?;
    let renewed = time()?;
    let expires = if flags & HAS_EXPIRES != 0 {
        Some(time()?)
    } else {
        None
    };
    let drop_at = if flags & HAS_DROP_TIME != 0 {
        Some(time()?)
    } else {
        None
    };
    let published = time()?;
    let (version, rest) = rest
        .split_first_chunk::<VERSION_LEN>()
        .ok_or(DecodeError::Malformed)?;
    let header = Header {
        written,
        renewed,
        expires,
        drop_at,
        published,
        value_version: u16::from_le_bytes(*version),
    };
    Ok((header, rest))
}

fn to_nanos(time: SystemTime) -> u64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(since) => u64::try_from(since.as_nanos()).unwrap_or(u64::MAX),
        Err(_) => 0,
    }
}

fn from_nanos(nanos: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_nanos(nanos)
}
