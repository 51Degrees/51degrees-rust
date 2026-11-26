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

//! A cache's entries kept in a platform store of bytes, against stand-ins
//! for the platform.

use fiftyone_caching_test_helpers as common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::{block_on, secs, MemoryBytes, ReservingBytes, Source, TestClock};
use fiftyone_caching::{
    decode_entry, encode_entry, encode_entry_published, Codec, EncodedStore, Entry, LoadingCache,
    Lookup, Store, Utf8,
};

fn store<B>(bytes: B, clock: &TestClock) -> EncodedStore<B, Utf8> {
    EncodedStore::builder(bytes, Utf8)
        .clock(clock.shared())
        .build()
}

fn entry(clock: &TestClock, value: &str) -> Entry<String> {
    Entry {
        value: value.to_string(),
        written: clock.now(),
        expires: Some(clock.now() + secs(3600)),
        renewed: clock.now(),
    }
}

fn get<B: fiftyone_caching::ByteStore>(
    store: &EncodedStore<B, Utf8>,
    key: u32,
) -> Lookup<String, B::Reservation> {
    block_on(Store::<u32, String>::get(store, &key))
}

fn is_miss<R>(lookup: &Lookup<String, R>) -> bool {
    matches!(lookup, Lookup::Miss)
}

#[test]
fn writes_each_entry_in_the_entry_format_with_its_drop_time() {
    let clock = TestClock::new();
    let store = store(MemoryBytes::<true>::new(), &clock);
    let written = entry(&clock, "value");
    block_on(store.put(&7u32, &written, Some(secs(60))));

    let expected = encode_entry(&Utf8, &written, Some(clock.now() + secs(60)));
    assert_eq!(store.bytes().raw("7"), expected);
    assert_eq!(
        store.bytes().take_puts(),
        vec![("7".to_string(), Some(secs(60)))]
    );
}

#[test]
fn reads_back_what_it_wrote() {
    let clock = TestClock::new();
    let store = store(MemoryBytes::<true>::new(), &clock);
    let written = entry(&clock, "value");
    block_on(store.put(&7u32, &written, Some(secs(60))));
    match get(&store, 7) {
        Lookup::Hit(read) => assert_eq!(read, written),
        _ => panic!("expected the entry"),
    }
}

#[test]
fn misses_from_the_drop_time_and_leaves_the_platform_to_delete() {
    let clock = TestClock::new();
    let store = store(MemoryBytes::<true>::new(), &clock);
    block_on(store.put(&7u32, &entry(&clock, "value"), Some(secs(60))));

    clock.advance_secs(59);
    assert!(matches!(get(&store, 7), Lookup::Hit(_)));
    clock.advance_secs(1);
    assert!(is_miss(&get(&store, 7)));
    assert!(store.bytes().raw("7").is_some(), "the platform deletes it");
    assert!(store.bytes().removed().is_empty());
}

#[test]
fn removes_an_expired_entry_when_the_platform_never_deletes() {
    let clock = TestClock::new();
    let store = store(MemoryBytes::<false>::new(), &clock);
    block_on(store.put(&7u32, &entry(&clock, "value"), Some(secs(60))));

    clock.advance_secs(60);
    assert!(is_miss(&get(&store, 7)));
    assert_eq!(store.bytes().raw("7"), None);
    assert_eq!(store.bytes().removed(), vec!["7".to_string()]);
}

#[test]
fn keeps_an_entry_with_no_lifetime_until_it_is_removed() {
    let clock = TestClock::new();
    let store = store(MemoryBytes::<false>::new(), &clock);
    block_on(store.put(&7u32, &entry(&clock, "value"), None));

    clock.advance_secs(1_000_000);
    assert!(matches!(get(&store, 7), Lookup::Hit(_)));
    block_on(Store::<u32, String>::remove(&store, &7));
    assert!(is_miss(&get(&store, 7)));
}

#[test]
fn misses_on_bytes_it_cannot_read_and_leaves_them() {
    let clock = TestClock::new();
    let store = store(MemoryBytes::<false>::new(), &clock);
    let mut newer = encode_entry(&Utf8, &entry(&clock, "value"), None).unwrap();
    newer[0] = 2;
    store.bytes().insert_raw("7", newer.clone());
    store.bytes().insert_raw("8", b"not an entry".to_vec());

    assert!(is_miss(&get(&store, 7)));
    assert!(is_miss(&get(&store, 8)));
    assert_eq!(store.bytes().raw("7"), Some(newer));
    assert!(store.bytes().raw("8").is_some());
    assert!(store.bytes().removed().is_empty());
}

/// Keeps `String` values as their text, in a value format of the given
/// version.
struct Versioned(u16);

impl Codec<String> for Versioned {
    fn encode(&self, value: &String) -> Option<Vec<u8>> {
        Some(value.as_bytes().to_vec())
    }

    fn decode(&self, bytes: &[u8]) -> Option<String> {
        String::from_utf8(bytes.to_vec()).ok()
    }

    fn version(&self) -> u16 {
        self.0
    }
}

/// The bytes worked out from the layout on the disk cache page, apart from
/// this crate's code.
#[test]
fn writes_data_published_and_the_value_format_version() {
    let at = |secs: u64| UNIX_EPOCH + Duration::from_secs(secs);
    let entry = Entry {
        value: "hi".to_string(),
        written: at(1_000_000),
        expires: None,
        renewed: at(1_000_000),
    };
    let bytes = encode_entry_published(&Versioned(0x0102), &entry, None, at(1_000_000));
    let expected: [u8; 30] = [
        0x01, 0x00, 0x00, 0x80, 0xC6, 0xA4, 0x7E, 0x8D, 0x03, 0x00, 0x00, 0x80, 0xC6, 0xA4, 0x7E,
        0x8D, 0x03, 0x00, 0x00, 0x80, 0xC6, 0xA4, 0x7E, 0x8D, 0x03, 0x00, 0x02, 0x01, 0x68, 0x69,
    ];
    assert_eq!(bytes.as_deref(), Some(&expected[..]));
    let read = decode_entry(&Versioned(0x0102), &expected).unwrap();
    assert_eq!(read.entry, entry);
    assert_eq!(read.published, at(1_000_000));
    assert_eq!(read.value_version, 0x0102);
}

/// A store over `bytes` whose source's data was published at the time
/// `published` holds, with values in a format of version `version`.
fn published_store<B>(
    bytes: B,
    clock: &TestClock,
    published: &Arc<Mutex<SystemTime>>,
    version: u16,
) -> EncodedStore<B, Versioned> {
    let published = Arc::clone(published);
    EncodedStore::builder(bytes, Versioned(version))
        .published(move || *published.lock().unwrap())
        .clock(clock.shared())
        .build()
}

#[test]
fn writes_the_published_date_of_the_data_the_source_uses_now() {
    let clock = TestClock::new();
    let published = Arc::new(Mutex::new(clock.now() - secs(86_400)));
    let store = published_store(MemoryBytes::<true>::new(), &clock, &published, 3);
    let written = entry(&clock, "value");
    block_on(store.put(&7u32, &written, None));

    let expected =
        encode_entry_published(&Versioned(3), &written, None, clock.now() - secs(86_400));
    assert_eq!(store.bytes().raw("7"), expected);
}

#[test]
fn an_entry_from_other_data_misses_and_is_removed() {
    let clock = TestClock::new();
    let published = Arc::new(Mutex::new(clock.now() - secs(86_400)));
    let store = published_store(MemoryBytes::<true>::new(), &clock, &published, 1);
    block_on(store.put(&7u32, &entry(&clock, "value"), None));
    assert!(matches!(
        block_on(Store::<u32, String>::get(&store, &7)),
        Lookup::Hit(_)
    ));

    // The source moves to newer data.
    *published.lock().unwrap() = clock.now();
    assert!(matches!(
        block_on(Store::<u32, String>::get(&store, &7)),
        Lookup::Miss
    ));
    assert_eq!(store.bytes().raw("7"), None);
    assert_eq!(store.bytes().removed(), vec!["7".to_string()]);
}

#[test]
fn an_entry_in_another_value_format_misses_and_is_removed() {
    let clock = TestClock::new();
    let bytes = Arc::new(MemoryBytes::<true>::new());
    let published = Arc::new(Mutex::new(UNIX_EPOCH));
    let older = published_store(Arc::clone(&bytes), &clock, &published, 1);
    block_on(older.put(&7u32, &entry(&clock, "value"), None));

    // An upgrade writes values in version 2.
    let newer = published_store(Arc::clone(&bytes), &clock, &published, 2);
    assert!(matches!(
        block_on(Store::<u32, String>::get(&newer, &7)),
        Lookup::Miss
    ));
    assert_eq!(bytes.raw("7"), None);
    assert_eq!(bytes.removed(), vec!["7".to_string()]);
}

#[test]
fn escapes_keys_and_puts_them_behind_the_namespace() {
    let clock = TestClock::new();
    let plain = store(MemoryBytes::<true>::new(), &clock);
    assert_eq!(plain.key_for(&"Az09-._~"), "Az09-._~");
    assert_eq!(
        plain.key_for(&"a b/c?d%#;^|"),
        "a%20b%2Fc%3Fd%25%23%3B%5E%7C"
    );
    assert_eq!(plain.key_for(&"é\n"), "%C3%A9%0A");

    let spaced = EncodedStore::builder(MemoryBytes::<true>::new(), Utf8)
        .namespace("cache:v1")
        .clock(clock.shared())
        .build();
    assert_eq!(spaced.key_for(&42u32), "cache:v1/42");
}

#[test]
fn never_gives_two_keys_one_platform_key() {
    let clock = TestClock::new();
    let store = store(MemoryBytes::<true>::new(), &clock);
    let keys = ["a/b", "a%2Fb", "a%252Fb", "", "%", "%25"];
    let mut platform: Vec<String> = keys.iter().map(|key| store.key_for(key)).collect();
    platform.sort();
    platform.dedup();
    assert_eq!(platform.len(), keys.len());
}

#[test]
fn passes_a_reservation_through_and_fills_it() {
    let clock = TestClock::new();
    let store = store(ReservingBytes::default(), &clock);
    let Lookup::Reserved(reservation) = get(&store, 7) else {
        panic!("expected a reservation");
    };
    let written = entry(&clock, "value");
    block_on(store.fill(&7u32, reservation, &written, Some(secs(60))));

    assert_eq!(
        store.bytes().fills(),
        vec![("7".to_string(), Some(secs(60)))]
    );
    assert_eq!(store.bytes().released(), 1);
    match get(&store, 7) {
        Lookup::Hit(read) => assert_eq!(read, written),
        _ => panic!("expected the filled entry"),
    }
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
fn releases_the_reservation_when_the_value_cannot_be_written() {
    let clock = TestClock::new();
    let store = EncodedStore::builder(ReservingBytes::default(), Refuses)
        .clock(clock.shared())
        .build();
    let lookup: Lookup<String, _> = block_on(store.get(&7u32));
    let Lookup::Reserved(reservation) = lookup else {
        panic!("expected a reservation");
    };
    block_on(store.fill(&7u32, reservation, &entry(&clock, "value"), Some(secs(60))));

    assert!(store.bytes().fills().is_empty());
    assert_eq!(store.bytes().released(), 1);
    assert_eq!(store.bytes().bytes.raw("7"), None);
}

#[test]
fn sweeps_only_its_own_entries_past_their_drop_time() {
    let clock = TestClock::new();
    let store = EncodedStore::builder(MemoryBytes::<false>::new(), Utf8)
        .namespace("ns")
        .clock(clock.shared())
        .build();
    block_on(store.put(&1u32, &entry(&clock, "one"), Some(secs(60))));
    block_on(store.put(&2u32, &entry(&clock, "two"), Some(secs(600))));
    block_on(store.put(&3u32, &entry(&clock, "three"), None));
    store
        .bytes()
        .insert_raw("ns/junk", b"not an entry".to_vec());
    let expired = encode_entry(&Utf8, &entry(&clock, "other"), Some(clock.now())).unwrap();
    store.bytes().insert_raw("other/9", expired);

    clock.advance_secs(61);
    assert_eq!(block_on(store.sweep()), 1);
    assert_eq!(
        store.bytes().held(),
        vec!["ns/2", "ns/3", "ns/junk", "other/9"]
    );
    assert_eq!(block_on(store.sweep()), 0);
}

#[test]
fn a_second_cache_reads_what_the_first_wrote() {
    let clock = TestClock::new();
    let bytes = Arc::new(MemoryBytes::<true>::new());
    let source = Source::new();
    let cache = || {
        LoadingCache::builder(store(Arc::clone(&bytes), &clock), source.clone())
            .time_to_live(secs(60))
            .clock(clock.shared())
            .build()
    };
    let (first, second) = (cache(), cache());

    let written = clock.now();
    assert_eq!(block_on(first.get(&7)), Ok("7 from load 1".to_string()));
    clock.advance_secs(10);
    let loaded = block_on(second.get_loaded(&7)).unwrap();
    assert_eq!(loaded.value, "7 from load 1");
    assert_eq!(loaded.written, Some(written));
    assert_eq!(source.loads(), 1);

    clock.advance_secs(50);
    assert_eq!(block_on(second.get(&7)), Ok("7 from load 2".to_string()));
}

#[test]
fn a_cache_fills_the_reservation_it_was_given() {
    let clock = TestClock::new();
    let bytes = Arc::new(ReservingBytes::default());
    let source = Source::new();
    let cache = || {
        LoadingCache::builder(store(Arc::clone(&bytes), &clock), source.clone())
            .time_to_live(secs(60))
            .clock(clock.shared())
            .build()
    };

    assert_eq!(block_on(cache().get(&7)), Ok("7 from load 1".to_string()));
    assert_eq!(bytes.fills(), vec![("7".to_string(), Some(secs(60)))]);
    assert_eq!(bytes.released(), 1);
    assert_eq!(block_on(cache().get(&7)), Ok("7 from load 1".to_string()));
    assert_eq!(source.loads(), 1);
}

#[test]
fn a_failed_load_releases_the_reservation_unfilled() {
    let clock = TestClock::new();
    let bytes = Arc::new(ReservingBytes::default());
    let source = Source::new();
    source.fail(true);
    let cache = LoadingCache::builder(store(Arc::clone(&bytes), &clock), source.clone())
        .clock(clock.shared())
        .build();

    assert_eq!(
        block_on(cache.get(&7)),
        Err("load 1 of 7 failed".to_string())
    );
    assert!(bytes.fills().is_empty());
    assert_eq!(bytes.released(), 1);
}
