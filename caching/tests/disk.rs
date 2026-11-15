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

//! A copy in memory, bounded by bytes, over a store on disk, over the
//! source. Data from the source is kept on disk and in memory, a copy
//! evicted from memory is read back from disk, a stale entry goes back to
//! the source, and the disk is cleared of expired entries and kept within
//! its space.

#![cfg(not(target_family = "wasm"))]

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use common::*;
use fiftyone_caching::machine::MemoryInfo;
use fiftyone_caching::{
    ByteLookup, ByteStore, EncodedStore, FileStore, LoadingCache, LruStore, MemoryBudget, Utf8,
};

/// A folder of its own for each test, removed when the test ends.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "fiftyone-caching-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        Folder(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

type Disk = Arc<FileStore>;
type OnDisk = LoadingCache<u32, String, EncodedStore<Disk, Utf8>, Source>;
type Memory = Arc<LruStore<u32, String>>;
type InMemory = LoadingCache<u32, String, Memory, Counted<OnDisk>>;

fn open(folder: &Folder, clock: &TestClock) -> Disk {
    Arc::new(
        FileStore::builder(folder.path())
            .max_bytes(1024 * 1024)
            .clock(clock.shared())
            .open()
            .expect("opens"),
    )
}

/// The cache over the store on disk, loading from the source. Entries live
/// for an hour.
fn on_disk(clock: &TestClock, disk: &Disk, source: &Source) -> OnDisk {
    let store = EncodedStore::builder(Arc::clone(disk), Utf8)
        .clock(clock.shared())
        .build();
    LoadingCache::builder(store, source.clone())
        .time_to_live(secs(60 * 60))
        .clock(clock.shared())
        .build()
}

/// The memory a test reports, which it lowers to put memory under
/// pressure.
#[derive(Clone)]
struct Machine(Arc<AtomicU64>);

impl Machine {
    fn new(available: u64) -> Self {
        Machine(Arc::new(AtomicU64::new(available)))
    }

    fn set_available(&self, available: u64) {
        self.0.store(available, Ordering::SeqCst);
    }

    fn budget(&self) -> MemoryBudget {
        let available = Arc::clone(&self.0);
        MemoryBudget::with_probe(move || {
            Some(MemoryInfo {
                total: 1 << 40,
                available: Some(available.load(Ordering::SeqCst)),
            })
        })
        .period(secs(0))
    }
}

/// A store in memory, one shard so eviction order is exact, bounded by the
/// machine's memory.
fn memory(clock: &TestClock, machine: &Machine) -> Memory {
    Arc::new(
        LruStore::builder()
            .size(1000)
            .concurrency(1)
            .weigh(|value: &String| value.len())
            .memory_budget(machine.budget())
            .clock(clock.shared())
            .build(),
    )
}

/// The cache in memory over the cache on disk. Copies live for a minute.
fn in_memory(clock: &TestClock, memory: &Memory, below: OnDisk) -> (InMemory, Arc<AtomicUsize>) {
    let (below, calls) = Counted::new(below);
    let cache = LoadingCache::builder(Arc::clone(memory), below)
        .time_to_live(secs(60))
        .clock(clock.shared())
        .build();
    (cache, calls)
}

#[test]
fn data_from_the_source_is_loaded_once_and_kept_on_disk_and_in_memory() {
    let folder = Folder::new("once");
    let clock = TestClock::new();
    let source = Source::gated();
    let disk = open(&folder, &clock);
    let memory = memory(&clock, &Machine::new(1 << 30));
    let (cache, calls_to_disk) = in_memory(&clock, &memory, on_disk(&clock, &disk, &source));

    let callers = (0..8).map(|_| boxed(cache.get(&1))).collect();
    let results = run_rounds(callers, |round, _| {
        if round == 1 {
            source.gate.open();
        }
    });

    for result in results {
        assert_eq!(result, Some(Ok("1 from load 1".to_owned())));
    }
    assert_eq!(source.loads(), 1);
    assert_eq!(calls_to_disk.load(Ordering::SeqCst), 1);
    assert_eq!(memory.len(), 1);
    assert_eq!(disk.file_count(), 1);
    assert!(disk.held_bytes() > 0);
}

#[test]
fn an_entry_evicted_under_memory_pressure_comes_back_from_disk_without_the_source() {
    let folder = Folder::new("pressure");
    let clock = TestClock::new();
    let source = Source::new();
    let machine = Machine::new(1 << 30);
    let disk = open(&folder, &clock);
    let memory = memory(&clock, &machine);
    let (cache, calls_to_disk) = in_memory(&clock, &memory, on_disk(&clock, &disk, &source));

    for key in 1..=3 {
        assert_eq!(
            block_on(cache.get(&key)),
            Ok(format!("{key} from load {key}"))
        );
    }
    assert_eq!(memory.len(), 3);
    assert_eq!(source.loads(), 3);

    // Other work takes the machine's memory. The next write finds the
    // budget fallen, and the store keeps only what a quarter of its own
    // bytes allows, evicting the entries used least recently.
    machine.set_available(0);
    assert_eq!(block_on(cache.get(&4)), Ok("4 from load 4".to_owned()));
    assert!(memory.len() < 4, "{} entries in memory", memory.len());
    assert!(memory.bytes() <= memory.max_bytes().expect("bounded"));

    // Entry 1 was evicted from memory. It is read back from disk, and the
    // source is not asked again.
    machine.set_available(1 << 30);
    let calls_before = calls_to_disk.load(Ordering::SeqCst);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    assert_eq!(calls_to_disk.load(Ordering::SeqCst), calls_before + 1);
    assert_eq!(source.loads(), 4);
}

#[test]
fn a_stale_entry_on_disk_goes_back_to_the_source() {
    let folder = Folder::new("stale");
    let clock = TestClock::new();
    let source = Source::new();
    let disk = open(&folder, &clock);
    let memory = memory(&clock, &Machine::new(1 << 30));
    let (cache, _) = in_memory(&clock, &memory, on_disk(&clock, &disk, &source));

    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));

    // Past the minute in memory but within the hour on disk, the copy on
    // disk answers.
    clock.advance_secs(2 * 60);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    assert_eq!(source.loads(), 1);

    // Past the hour the copy on disk is stale, so the source loads again.
    clock.advance_secs(60 * 60);
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 2".to_owned()));
    assert_eq!(source.loads(), 2);
}

#[test]
fn the_disk_survives_a_restart() {
    let folder = Folder::new("restart");
    let clock = TestClock::new();
    {
        let source = Source::new();
        let disk = open(&folder, &clock);
        let memory = memory(&clock, &Machine::new(1 << 30));
        let (cache, _) = in_memory(&clock, &memory, on_disk(&clock, &disk, &source));
        assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    }

    // A new process: memory is empty, the files are not.
    let source = Source::new();
    let disk = open(&folder, &clock);
    let memory = memory(&clock, &Machine::new(1 << 30));
    let (cache, _) = in_memory(&clock, &memory, on_disk(&clock, &disk, &source));
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    assert_eq!(source.loads(), 0);
}

#[test]
fn expired_files_are_cleared() {
    let folder = Folder::new("expired");
    let clock = TestClock::new();
    let disk = open(&folder, &clock);

    block_on(disk.put("short", b"a".to_vec(), Some(secs(60))));
    block_on(disk.put("long", b"b".to_vec(), Some(secs(60 * 60))));
    block_on(disk.put("forever", b"c".to_vec(), None));
    assert_eq!(disk.file_count(), 3);

    clock.advance_secs(2 * 60);
    assert_eq!(disk.clear_expired(), 1);
    assert_eq!(disk.file_count(), 2);
    assert!(matches!(block_on(disk.get("short")), ByteLookup::Miss));
    assert!(matches!(block_on(disk.get("long")), ByteLookup::Hit(_)));

    clock.advance_secs(60 * 60);
    assert_eq!(disk.clear_expired(), 1);
    assert_eq!(disk.file_count(), 1);
    assert!(matches!(block_on(disk.get("forever")), ByteLookup::Hit(_)));
}

#[test]
fn an_expired_entry_answers_as_absent_before_it_is_cleared() {
    let folder = Folder::new("absent");
    let clock = TestClock::new();
    let disk = open(&folder, &clock);

    block_on(disk.put("key", b"value".to_vec(), Some(secs(60))));
    clock.advance_secs(61);
    assert!(matches!(block_on(disk.get("key")), ByteLookup::Miss));
    // Reading it found it expired, so its file is gone too.
    assert_eq!(disk.file_count(), 0);
    assert_eq!(disk.held_bytes(), 0);
}

#[test]
fn an_entry_unused_for_the_time_to_idle_is_cleared() {
    let folder = Folder::new("idle");
    let clock = TestClock::new();
    let disk = FileStore::builder(folder.path())
        .time_to_idle(secs(60 * 60))
        .clock(clock.shared())
        .open()
        .expect("opens");

    block_on(disk.put("used", b"a".to_vec(), None));
    block_on(disk.put("unused", b"b".to_vec(), None));
    for _ in 0..3 {
        clock.advance_secs(30 * 60);
        assert!(matches!(block_on(disk.get("used")), ByteLookup::Hit(_)));
    }

    assert_eq!(disk.clear_expired(), 1);
    assert!(matches!(block_on(disk.get("used")), ByteLookup::Hit(_)));
    assert!(matches!(block_on(disk.get("unused")), ByteLookup::Miss));
}

#[test]
fn the_disk_is_kept_within_its_space_by_removing_the_least_recently_used() {
    let folder = Folder::new("limit");
    let clock = TestClock::new();
    let limit = 8 * 1024;
    let disk = FileStore::builder(folder.path())
        .max_bytes(limit)
        .clock(clock.shared())
        .open()
        .expect("opens");

    let value = vec![b'x'; 1000];
    block_on(disk.put("first", value.clone(), None));
    for key in 0..20 {
        clock.advance_secs(1);
        // Reading the first entry keeps it the most recently used.
        assert!(matches!(block_on(disk.get("first")), ByteLookup::Hit(_)));
        clock.advance_secs(1);
        block_on(disk.put(&format!("key-{key}"), value.clone(), None));
        assert!(disk.held_bytes() <= limit, "{} bytes", disk.held_bytes());
    }

    let on_disk: u64 = walk(folder.path()).iter().sum();
    assert!(on_disk <= limit, "{on_disk} bytes on disk");
    assert!(matches!(block_on(disk.get("first")), ByteLookup::Hit(_)));
    assert!(matches!(block_on(disk.get("key-0")), ByteLookup::Miss));
    assert!(matches!(block_on(disk.get("key-19")), ByteLookup::Hit(_)));
}

#[test]
fn a_store_opened_over_its_limit_trims_to_it() {
    let folder = Folder::new("reopen");
    let clock = TestClock::new();
    {
        let disk = open(&folder, &clock);
        for key in 0..10 {
            clock.advance_secs(1);
            block_on(disk.put(&format!("key-{key}"), vec![b'x'; 1000], None));
        }
    }
    let disk = FileStore::builder(folder.path())
        .max_bytes(4 * 1024)
        .clock(clock.shared())
        .open()
        .expect("opens");
    assert!(disk.held_bytes() <= 4 * 1024);
    assert!(matches!(block_on(disk.get("key-9")), ByteLookup::Hit(_)));
    assert!(matches!(block_on(disk.get("key-0")), ByteLookup::Miss));
}

#[test]
fn any_key_is_a_safe_file_name() {
    let folder = Folder::new("names");
    let clock = TestClock::new();
    let disk = open(&folder, &clock);
    let keys = [
        "../../outside",
        "C:\\Windows",
        "con",
        "a/b/c?d*e:f|g<h>\"",
        &"long".repeat(500),
        "",
    ];
    for (index, key) in keys.iter().enumerate() {
        block_on(disk.put(key, vec![index as u8], None));
    }
    for (index, key) in keys.iter().enumerate() {
        match block_on(disk.get(key)) {
            ByteLookup::Hit(bytes) => assert_eq!(bytes, vec![index as u8]),
            _ => panic!("{key} missing"),
        }
    }
    assert_eq!(disk.file_count(), keys.len());
    assert!(!folder.path().parent().unwrap().join("outside").exists());
}

/// The length of every file under `path`.
fn walk(path: &Path) -> Vec<u64> {
    let mut lengths = Vec::new();
    for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
        let metadata = entry.metadata().expect("metadata");
        if metadata.is_dir() {
            lengths.extend(walk(&entry.path()));
        } else {
            lengths.push(metadata.len());
        }
    }
    lengths
}
