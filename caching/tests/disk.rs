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
//! its space. The store follows the disk cache page of the pipeline
//! specification, and these tests show each behavior it requires: the
//! folder structure, every time in the file and none from the file system,
//! the key compared on every read, the limits, and the safety rules. The
//! test that a file found for another key answers absent is a unit test in
//! the store, which can put one there.

#![cfg(all(feature = "disk", not(target_family = "wasm")))]

mod common;

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex, Once, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::*;
use fiftyone_caching::machine::MemoryInfo;
use fiftyone_caching::{
    decode_entry, encode_entry, ByteLookup, ByteStore, Codec, EncodedStore, Entry, FileStore,
    LoadingCache, Lookup, LruStore, MemoryBudget, Raw, Store, Utf8,
};

/// The file in the folder root that holds the secret.
const SECRET: &str = "cache.secret";

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
        let _ = fs::remove_dir_all(&self.0);
        // A test may leave a file where the folder was.
        let _ = fs::remove_file(&self.0);
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

/// An entry written at `at`, as `EncodedStore` writes it, to be dropped
/// after `keep_for`.
fn written_at(at: SystemTime, value: &[u8], keep_for: Option<Duration>) -> Vec<u8> {
    let entry = Entry {
        value: value.to_vec(),
        written: at,
        expires: None,
        renewed: at,
    };
    encode_entry(&Raw, &entry, keep_for.map(|keep_for| at + keep_for)).expect("encodes")
}

/// Writes `value` for `key` now, to be dropped after `keep_for`.
fn put(disk: &FileStore, clock: &TestClock, key: &str, value: &[u8], keep_for: Option<Duration>) {
    block_on(disk.put(key, written_at(clock.now(), value, keep_for), keep_for));
}

/// The value the store holds for `key`, or `None` when it answers absent.
fn value(disk: &FileStore, key: &str) -> Option<Vec<u8>> {
    match block_on(disk.get(key)) {
        ByteLookup::Hit(bytes) => Some(decode_entry(&Raw, &bytes).expect("an entry").entry.value),
        _ => None,
    }
}

/// Every file under `path`.
fn all_files(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(path).into_iter().flatten().flatten() {
        if entry.metadata().expect("metadata").is_dir() {
            files.extend(all_files(&entry.path()));
        } else {
            files.push(entry.path());
        }
    }
    files
}

/// The file that holds `key`, found by the key inside it.
fn file_of(root: &Path, key: &str) -> PathBuf {
    all_files(root)
        .into_iter()
        .find(|path| {
            let bytes = fs::read(path).unwrap_or_default();
            bytes.len() >= 9 + key.len()
                && bytes[..4] == *b"51DF"
                && bytes[5..9] == (key.len() as u32).to_le_bytes()
                && bytes[9..9 + key.len()] == *key.as_bytes()
        })
        .unwrap_or_else(|| panic!("no file holds {key}"))
}

/// The Renewed time in the file that holds `key`.
fn renewed_in_file(root: &Path, key: &str) -> SystemTime {
    let bytes = fs::read(file_of(root, key)).expect("reads");
    let offset = 19 + key.len();
    let nanos = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
    UNIX_EPOCH + Duration::from_nanos(nanos)
}

fn nanos(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
}

/// Sets the modified, accessed and, where the system allows, created times
/// of every file under `root` to `at`.
fn set_file_times(root: &Path, at: SystemTime) {
    for path in all_files(root) {
        let file = fs::File::options().write(true).open(&path).expect("opens");
        #[allow(unused_mut)]
        let mut times = fs::FileTimes::new().set_accessed(at).set_modified(at);
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileTimesExt;
            times = times.set_created(at);
        }
        #[cfg(target_vendor = "apple")]
        {
            use std::os::darwin::fs::FileTimesExt;
            times = times.set_created(at);
        }
        file.set_times(times).expect("sets the times");
    }
}

/// The names in `path`, sorted.
fn names(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(path)
        .expect("lists")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
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

    // A new process: memory is empty, the files are not. The disk answers
    // with no call to the source.
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

    put(&disk, &clock, "short", b"a", Some(secs(60)));
    put(&disk, &clock, "long", b"b", Some(secs(60 * 60)));
    put(&disk, &clock, "forever", b"c", None);
    assert_eq!(disk.file_count(), 3);

    clock.advance_secs(2 * 60);
    assert_eq!(disk.clear_expired(), 1);
    assert_eq!(disk.file_count(), 2);
    assert_eq!(value(&disk, "short"), None);
    assert_eq!(value(&disk, "long"), Some(b"b".to_vec()));

    clock.advance_secs(60 * 60);
    assert_eq!(disk.clear_expired(), 1);
    assert_eq!(disk.file_count(), 1);
    assert_eq!(value(&disk, "forever"), Some(b"c".to_vec()));
}

#[test]
fn an_expired_entry_answers_as_absent_before_it_is_cleared() {
    let folder = Folder::new("absent");
    let clock = TestClock::new();
    let disk = open(&folder, &clock);

    put(&disk, &clock, "key", b"value", Some(secs(60)));
    clock.advance_secs(61);
    assert_eq!(value(&disk, "key"), None);
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

    put(&disk, &clock, "used", b"a", None);
    put(&disk, &clock, "unused", b"b", None);
    for _ in 0..3 {
        clock.advance_secs(30 * 60);
        assert_eq!(value(&disk, "used"), Some(b"a".to_vec()));
    }

    assert_eq!(disk.clear_expired(), 1);
    assert_eq!(value(&disk, "used"), Some(b"a".to_vec()));
    assert_eq!(value(&disk, "unused"), None);
}

#[test]
fn renewed_is_rewritten_only_once_more_than_a_tenth_of_the_time_to_idle_has_passed() {
    let folder = Folder::new("renewed");
    let clock = TestClock::new();
    let disk = FileStore::builder(folder.path())
        .time_to_idle(secs(100))
        .clock(clock.shared())
        .open()
        .expect("opens");

    let start = clock.now();
    put(&disk, &clock, "key", b"value", None);
    assert_eq!(renewed_in_file(folder.path(), "key"), start);

    // Within a tenth of the time to idle, a read leaves the file alone.
    clock.advance_secs(10);
    assert!(value(&disk, "key").is_some());
    assert_eq!(renewed_in_file(folder.path(), "key"), start);

    // Past it, a read rewrites Renewed.
    clock.advance_secs(1);
    assert!(value(&disk, "key").is_some());
    let renewed = clock.now();
    assert_eq!(renewed_in_file(folder.path(), "key"), renewed);

    clock.advance_secs(9);
    assert!(value(&disk, "key").is_some());
    assert_eq!(renewed_in_file(folder.path(), "key"), renewed);
}

#[test]
fn with_no_time_to_idle_renewed_is_rewritten_only_once_more_than_an_hour_has_passed() {
    let folder = Folder::new("renewed-hourly");
    let clock = TestClock::new();
    let disk = open(&folder, &clock);

    let start = clock.now();
    put(&disk, &clock, "key", b"value", None);

    // An hour on, a read leaves the file alone.
    clock.advance_secs(60 * 60);
    assert!(value(&disk, "key").is_some());
    assert_eq!(renewed_in_file(folder.path(), "key"), start);

    // Past it, a read rewrites Renewed.
    clock.advance_secs(1);
    assert!(value(&disk, "key").is_some());
    let renewed = clock.now();
    assert_eq!(renewed_in_file(folder.path(), "key"), renewed);

    clock.advance_secs(60 * 60);
    assert!(value(&disk, "key").is_some());
    assert_eq!(renewed_in_file(folder.path(), "key"), renewed);
}

#[test]
fn with_no_time_to_idle_trimming_removes_the_least_recently_used_first() {
    let folder = Folder::new("limit-hourly");
    let clock = TestClock::new();
    let limit = 8 * 1024;
    let disk = FileStore::builder(folder.path())
        .max_bytes(limit)
        .clock(clock.shared())
        .open()
        .expect("opens");

    let value_bytes = vec![b'x'; 1000];
    put(&disk, &clock, "first", &value_bytes, None);
    for key in 0..9 {
        clock.advance_secs(60 * 60 + 1);
        // Reading the first entry renews it, so it is never the least
        // recently used.
        assert!(value(&disk, "first").is_some());
        clock.advance_secs(1);
        put(&disk, &clock, &format!("key-{key}"), &value_bytes, None);
    }

    assert!(value(&disk, "first").is_some());
    assert_eq!(value(&disk, "key-0"), None);
    assert!(value(&disk, "key-8").is_some());
}

#[test]
fn a_read_that_renews_writes_a_whole_new_file_and_renames_it_into_place() {
    let folder = Folder::new("renew-rename");
    let clock = TestClock::new();
    let disk = FileStore::builder(folder.path())
        .time_to_idle(secs(100))
        .clock(clock.shared())
        .open()
        .expect("opens");
    put(&disk, &clock, "key", b"value", None);
    let path = file_of(folder.path(), "key");
    let before = fs::read(&path).unwrap();
    // A handle open on the file sees it unchanged when another file is
    // renamed over it, and sees any write made in place.
    let mut held = fs::File::open(&path).unwrap();

    clock.advance_secs(11);
    assert_eq!(value(&disk, "key"), Some(b"value".to_vec()));
    assert_eq!(renewed_in_file(folder.path(), "key"), clock.now());
    let mut through_held = Vec::new();
    held.read_to_end(&mut through_held).unwrap();
    assert_eq!(through_held, before, "the file was rewritten in place");
    let leaf = path.parent().unwrap();
    assert_eq!(names(leaf), [path.file_name().unwrap().to_string_lossy()]);
}

#[test]
fn the_disk_is_kept_within_its_space_by_removing_the_oldest_renewed() {
    let folder = Folder::new("limit");
    let clock = TestClock::new();
    let limit = 8 * 1024;
    // A read renews an entry once more than 1,000 seconds have passed.
    let disk = FileStore::builder(folder.path())
        .max_bytes(limit)
        .time_to_idle(secs(10_000))
        .clock(clock.shared())
        .open()
        .expect("opens");

    let value_bytes = vec![b'x'; 1000];
    put(&disk, &clock, "first", &value_bytes, None);
    for key in 0..9 {
        clock.advance_secs(1001);
        // Reading the first entry renews it, so it is never the oldest.
        assert!(value(&disk, "first").is_some());
        clock.advance_secs(1);
        put(&disk, &clock, &format!("key-{key}"), &value_bytes, None);
        assert!(disk.held_bytes() <= limit, "{} bytes", disk.held_bytes());
    }

    let on_disk: u64 = all_files(folder.path())
        .iter()
        .map(|path| fs::metadata(path).unwrap().len())
        .sum();
    assert!(on_disk <= limit + 32, "{on_disk} bytes on disk");
    assert!(value(&disk, "first").is_some());
    assert_eq!(value(&disk, "key-0"), None);
    assert!(value(&disk, "key-8").is_some());
}

#[test]
fn a_store_opened_over_its_limit_trims_to_it() {
    let folder = Folder::new("reopen");
    let clock = TestClock::new();
    {
        let disk = open(&folder, &clock);
        for key in 0..10 {
            clock.advance_secs(1);
            put(&disk, &clock, &format!("key-{key}"), &[b'x'; 1000], None);
        }
    }
    let disk = FileStore::builder(folder.path())
        .max_bytes(4 * 1024)
        .clock(clock.shared())
        .open()
        .expect("opens");
    assert!(disk.held_bytes() <= 4 * 1024);
    assert!(value(&disk, "key-9").is_some());
    assert_eq!(value(&disk, "key-0"), None);
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
        put(&disk, &clock, key, &[index as u8], None);
    }
    for (index, key) in keys.iter().enumerate() {
        assert_eq!(value(&disk, key), Some(vec![index as u8]), "{key}");
    }
    assert_eq!(disk.file_count(), keys.len());
    assert!(!folder.path().parent().unwrap().join("outside").exists());
}

#[test]
fn nothing_is_made_before_the_store_opens_and_only_the_secret_until_a_write() {
    let folder = Folder::new("lazy");
    let clock = TestClock::new();
    let builder = FileStore::builder(folder.path()).clock(clock.shared());
    assert!(!folder.path().exists());

    let disk = builder.open().expect("opens");
    assert_eq!(names(folder.path()), [SECRET]);
    // The store holds the secret locked, so only its length is read.
    assert_eq!(fs::metadata(folder.path().join(SECRET)).unwrap().len(), 32);

    put(&disk, &clock, "key", b"value", None);
    let names = names(folder.path());
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&SECRET.to_owned()));
}

#[test]
fn every_file_sits_at_the_two_level_path_its_name_gives() {
    let folder = Folder::new("nested");
    let clock = TestClock::new();
    let disk = open(&folder, &clock);
    for key in 0..2000 {
        put(&disk, &clock, &format!("key-{key}"), &[b'x'; 8], None);
    }

    for path in all_files(folder.path()) {
        let relative = path.strip_prefix(folder.path()).unwrap();
        let parts: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        if parts.len() == 1 {
            assert_eq!(parts[0], SECRET, "the root holds only the secret");
            continue;
        }
        assert_eq!(
            parts.len(),
            3,
            "{} is not two folders down",
            relative.display()
        );
        let name = &parts[2];
        assert_eq!(name.len(), 64, "{name}");
        assert!(name
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')));
        assert_eq!(parts[0], name[..2]);
        assert_eq!(parts[1], name[2..4]);
    }
    // 2000 entries over 65,536 leaf folders put a handful in each, and no
    // folder holds more than 256 folders.
    let mut found = Vec::new();
    folders(folder.path(), 0, &mut found);
    for (path, files, _) in &found {
        assert!(*files <= 16, "{} holds {files} files", path.display());
        let children = fs::read_dir(path).expect("lists").count();
        assert!(
            children <= 257,
            "{} holds {children} entries",
            path.display()
        );
    }
    assert_eq!(disk.file_count(), 2000);
}

#[test]
fn clearing_and_trimming_remove_folders_left_empty() {
    let folder = Folder::new("empty-folders");
    let clock = TestClock::new();
    let disk = FileStore::builder(folder.path())
        .max_bytes(64 * 1024)
        .clock(clock.shared())
        .open()
        .expect("opens");
    for key in 0..50 {
        put(
            &disk,
            &clock,
            &format!("short-{key}"),
            &[b'x'; 100],
            Some(secs(60)),
        );
    }
    clock.advance_secs(61);
    assert_eq!(disk.clear_expired(), 50);
    assert_eq!(names(folder.path()), [SECRET], "only the secret is left");

    // Trimming over the limit removes the folders it empties too.
    for key in 0..200 {
        clock.advance_secs(1);
        put(&disk, &clock, &format!("big-{key}"), &[b'x'; 1000], None);
    }
    let mut found = Vec::new();
    folders(folder.path(), 0, &mut found);
    let leaves = found.iter().filter(|(_, _, depth)| *depth == 2).count();
    let empty = found
        .iter()
        .filter(|(path, files, depth)| {
            *depth == 2 && *files == 0 && fs::read_dir(path).unwrap().count() == 0
        })
        .count();
    assert_eq!(empty, 0, "{empty} of {leaves} leaf folders are empty");
    // Keys that differ only at the end still spread out.
    assert!(
        leaves * 10 >= disk.file_count() * 9,
        "{} files in {leaves} folders",
        disk.file_count()
    );
}

#[test]
fn a_reopened_store_finds_its_entries_in_nested_folders() {
    let folder = Folder::new("reopen-nested");
    let clock = TestClock::new();
    {
        let disk = open(&folder, &clock);
        for key in 0..100 {
            put(
                &disk,
                &clock,
                &format!("key-{key}"),
                format!("value {key}").as_bytes(),
                None,
            );
        }
    }
    let disk = open(&folder, &clock);
    assert_eq!(disk.file_count(), 100);
    for key in 0..100 {
        assert_eq!(
            value(&disk, &format!("key-{key}")),
            Some(format!("value {key}").into_bytes()),
            "key-{key} after reopening"
        );
    }
}

#[test]
fn file_system_times_change_nothing() {
    let epoch = UNIX_EPOCH;
    // 1 January 2400.
    let far_future = UNIX_EPOCH + Duration::from_secs(13_569_465_600);

    // Answers, expiry and idle.
    let folder = Folder::new("times");
    let clock = TestClock::new();
    let disk = FileStore::builder(folder.path())
        .max_bytes(1 << 20)
        .time_to_idle(secs(60 * 60))
        .sweep_every(secs(24 * 60 * 60))
        .clock(clock.shared())
        .open()
        .expect("opens");
    put(&disk, &clock, "expires", b"a", Some(secs(30 * 60)));
    put(&disk, &clock, "idles", b"b", None);
    put(&disk, &clock, "read", b"c", None);
    for at in [epoch, far_future] {
        set_file_times(folder.path(), at);
        assert_eq!(value(&disk, "expires"), Some(b"a".to_vec()));
        assert_eq!(value(&disk, "read"), Some(b"c".to_vec()));
    }

    // After 20 minutes, a read renews "read".
    clock.advance_secs(20 * 60);
    set_file_times(folder.path(), far_future);
    assert_eq!(value(&disk, "read"), Some(b"c".to_vec()));
    set_file_times(folder.path(), epoch);

    // After 45 minutes, "expires" is past its drop time, however new its
    // file looks.
    clock.advance_secs(25 * 60);
    set_file_times(folder.path(), far_future);
    assert_eq!(value(&disk, "expires"), None);

    // After 65 minutes, "idles" has gone unused for the time to idle,
    // however recently its file seems to have been used, and "read" has
    // not, however old its file looks.
    clock.advance_secs(20 * 60);
    set_file_times(folder.path(), far_future);
    for path in all_files(folder.path()) {
        if path == file_of(folder.path(), "read") {
            set_file_times(path.parent().unwrap(), epoch);
        }
    }
    assert_eq!(disk.clear_expired(), 1);
    assert_eq!(value(&disk, "idles"), None);
    assert_eq!(value(&disk, "read"), Some(b"c".to_vec()));

    // Trimming. The older entry's file looks the newest, and the newer
    // entry's the oldest, but the older entry is the one trimmed.
    let folder = Folder::new("times-trim");
    {
        let disk = open(&folder, &clock);
        put(&disk, &clock, "older", &[b'x'; 1000], None);
        clock.advance_secs(1);
        put(&disk, &clock, "newer", &[b'y'; 1000], None);
    }
    set_file_times(folder.path(), epoch);
    set_file_times(
        file_of(folder.path(), "older").parent().unwrap(),
        far_future,
    );
    let disk = FileStore::builder(folder.path())
        .max_bytes(1500)
        .clock(clock.shared())
        .open()
        .expect("opens");
    assert_eq!(value(&disk, "older"), None);
    assert_eq!(value(&disk, "newer"), Some(vec![b'y'; 1000]));
}

#[test]
fn an_entry_written_later_than_now_answers_absent_and_is_removed() {
    let folder = Folder::new("ahead");
    let clock = TestClock::new();
    let disk = open(&folder, &clock);
    let ahead = clock.now() + secs(60 * 60);
    block_on(disk.put("ahead", written_at(ahead, b"value", None), None));
    assert_eq!(disk.file_count(), 1);
    assert_eq!(value(&disk, "ahead"), None);
    assert_eq!(disk.file_count(), 0);
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

#[test]
fn an_entry_from_other_data_or_in_another_value_format_answers_absent_and_is_removed() {
    let folder = Folder::new("published");
    let clock = TestClock::new();
    let disk = open(&folder, &clock);
    let store = |published: SystemTime, version: u16| {
        EncodedStore::builder(Arc::clone(&disk), Versioned(version))
            .published(move || published)
            .clock(clock.shared())
            .build()
    };
    let get = |store: &EncodedStore<Disk, Versioned>, key: u32| {
        block_on(Store::<u32, String>::get(store, &key))
    };
    let yesterday = clock.now() - secs(24 * 60 * 60);
    let written = Entry {
        value: "value".to_owned(),
        written: clock.now(),
        expires: None,
        renewed: clock.now(),
    };
    let first = store(yesterday, 1);
    block_on(first.put(&1u32, &written, None));
    block_on(first.put(&2u32, &written, None));
    assert!(matches!(get(&first, 1), Lookup::Hit(_)));
    assert_eq!(disk.file_count(), 2);

    // The source moves to data published today.
    let updated = store(clock.now(), 1);
    assert!(matches!(get(&updated, 1), Lookup::Miss));
    assert_eq!(disk.file_count(), 1);

    // An upgrade writes values in version 2.
    let upgraded = store(yesterday, 2);
    assert!(matches!(get(&upgraded, 2), Lookup::Miss));
    assert_eq!(disk.file_count(), 0);
}

#[test]
fn bytes_that_are_not_an_entry_are_not_written() {
    let folder = Folder::new("not-an-entry");
    let clock = TestClock::new();
    let disk = open(&folder, &clock);
    block_on(disk.put("raw", b"plain bytes".to_vec(), None));
    assert_eq!(disk.file_count(), 0);
    assert!(matches!(block_on(disk.get("raw")), ByteLookup::Miss));
}

#[test]
fn temporary_files_left_behind_are_cleared_when_the_store_opens() {
    let folder = Folder::new("temporary");
    let clock = TestClock::new();
    let path = {
        let disk = open(&folder, &clock);
        put(&disk, &clock, "key", b"value", None);
        file_of(folder.path(), "key")
    };
    let leaf = path.parent().unwrap();
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let now = nanos(clock.now());
    let twenty_minutes = 20 * 60 * 1_000_000_000;
    // Left by a crash 20 minutes ago.
    let crashed = leaf.join(format!("{name}.{}-1-0.tmp", now - twenty_minutes));
    let crashed_secret = folder
        .path()
        .join(format!("{SECRET}.{}-1-0.tmp", now - twenty_minutes));
    // Left by a crash a moment ago. One store at a time has the folder, so
    // no other store can be writing it, and it goes however recent it is.
    let recent = leaf.join(format!("{name}.{now}-1-1.tmp"));
    let recent_secret = folder.path().join(format!("{SECRET}.{now}-1-1.tmp"));
    // Not a name the store gives.
    let other = leaf.join("notes.tmp");
    for path in [&crashed, &crashed_secret, &recent, &recent_secret, &other] {
        fs::write(path, b"part of a file").unwrap();
    }

    let disk = open(&folder, &clock);
    assert!(!crashed.exists());
    assert!(!crashed_secret.exists());
    assert!(!recent.exists(), "a recent temporary file was kept at open");
    assert!(
        !recent_secret.exists(),
        "a recent temporary secret was kept at open"
    );
    assert!(other.exists());
    assert_eq!(value(&disk, "key"), Some(b"value".to_vec()));
}

#[test]
fn a_sweep_while_open_keeps_a_temporary_file_being_written() {
    let folder = Folder::new("temporary-sweep");
    let clock = TestClock::new();
    let disk = open(&folder, &clock);
    put(&disk, &clock, "key", b"value", None);
    let path = file_of(folder.path(), "key");
    let leaf = path.parent().unwrap();
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let now = nanos(clock.now());
    let twenty_minutes = 20 * 60 * 1_000_000_000;
    // Left by a write that never finished.
    let crashed = leaf.join(format!("{name}.{}-1-0.tmp", now - twenty_minutes));
    // Being written by this store now.
    let writing = leaf.join(format!("{name}.{now}-1-1.tmp"));
    for path in [&crashed, &writing] {
        fs::write(path, b"part of a file").unwrap();
    }

    disk.clear_expired();
    assert!(!crashed.exists());
    assert!(writing.exists(), "a sweep removed a write in progress");
    assert_eq!(value(&disk, "key"), Some(b"value".to_vec()));
}

#[test]
fn corrupt_and_truncated_files_are_removed() {
    let folder = Folder::new("corrupt");
    let clock = TestClock::new();
    let (truncated, wrong_version, bad_entry) = {
        let disk = open(&folder, &clock);
        for key in ["a", "b", "c", "d"] {
            put(&disk, &clock, key, &[b'x'; 100], None);
        }
        (
            file_of(folder.path(), "a"),
            file_of(folder.path(), "b"),
            file_of(folder.path(), "c"),
        )
    };
    let disk = {
        let bytes = fs::read(&truncated).unwrap();
        fs::write(&truncated, &bytes[..20]).unwrap();
        let mut bytes = fs::read(&wrong_version).unwrap();
        bytes[4] = 9;
        fs::write(&wrong_version, &bytes).unwrap();
        let mut bytes = fs::read(&bad_entry).unwrap();
        // The entry format version, after the 9 bytes and the 1-byte key.
        bytes[10] = 7;
        fs::write(&bad_entry, &bytes).unwrap();
        FileStore::builder(folder.path())
            .clock(clock.shared())
            .sweep_every(secs(24 * 60 * 60))
            .open()
            .expect("opens")
    };
    // Opening cleared all three.
    assert!(!truncated.exists());
    assert!(!wrong_version.exists());
    assert!(!bad_entry.exists());
    assert_eq!(value(&disk, "d"), Some(vec![b'x'; 100]));

    // A file found damaged on a read is removed then.
    put(&disk, &clock, "e", b"value", None);
    let path = file_of(folder.path(), "e");
    let bytes = fs::read(&path).unwrap();
    fs::write(&path, &bytes[..12]).unwrap();
    assert_eq!(value(&disk, "e"), None);
    assert!(!path.exists());
}

#[test]
fn files_the_store_did_not_write_are_left_untouched() {
    let folder = Folder::new("foreign");
    let clock = TestClock::new();
    fs::create_dir_all(folder.path().join("ab").join("cd")).unwrap();
    fs::create_dir_all(folder.path().join("zz")).unwrap();
    let foreign_name = "ab".to_owned() + "cd" + &"0".repeat(60);
    let foreign = [
        (folder.path().join("notes.txt"), b"in the root".to_vec()),
        (
            folder.path().join("ab").join("readme.md"),
            b"in a folder".to_vec(),
        ),
        (
            folder.path().join("ab").join("cd").join(&foreign_name),
            b"named like an entry".to_vec(),
        ),
        (
            folder.path().join("ab").join("cd").join("x.tmp"),
            b"temporary".to_vec(),
        ),
        (folder.path().join("zz").join("data.bin"), vec![0, 1, 2]),
    ];
    for (path, bytes) in &foreign {
        fs::write(path, bytes).unwrap();
    }

    {
        let disk = FileStore::builder(folder.path())
            .max_bytes(4 * 1024)
            .clock(clock.shared())
            .open()
            .expect("opens");
        for key in 0..20 {
            clock.advance_secs(1);
            put(
                &disk,
                &clock,
                &format!("key-{key}"),
                &[b'x'; 1000],
                Some(secs(60)),
            );
        }
        assert_eq!(value(&disk, &foreign_name), None);
        clock.advance_secs(24 * 60 * 60);
        disk.clear_expired();
    }
    clock.advance_secs(24 * 60 * 60);
    drop(open(&folder, &clock));

    for (path, bytes) in &foreign {
        assert_eq!(
            fs::read(path).ok().as_ref(),
            Some(bytes),
            "{}",
            path.display()
        );
    }
}

/// The warnings the stores have logged, kept from the first call on.
fn warnings() -> &'static Mutex<Vec<String>> {
    struct Kept(Mutex<Vec<String>>);

    impl log::Log for Kept {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::Level::Warn
        }

        fn log(&self, record: &log::Record) {
            if self.enabled(record.metadata()) {
                let mut kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
                kept.push(record.args().to_string());
            }
        }

        fn flush(&self) {}
    }

    static KEPT: Kept = Kept(Mutex::new(Vec::new()));
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = log::set_logger(&KEPT);
        log::set_max_level(log::LevelFilter::Warn);
    });
    &KEPT.0
}

/// True when a store has warned that `folder` is open in another store and
/// that each store needs a folder of its own.
fn warned_of_a_shared_folder(folder: &Path) -> bool {
    let folder = folder.display().to_string();
    warnings()
        .lock()
        .unwrap()
        .iter()
        .any(|warning| warning.contains(&folder) && warning.contains("a folder of its own"))
}

/// Every file under `path` with its bytes, sorted by path.
fn snapshot(path: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files: Vec<_> = all_files(path)
        .into_iter()
        .map(|file| {
            let bytes = fs::read(&file).expect("reads");
            (file, bytes)
        })
        .collect();
    files.sort();
    files
}

/// A store that refused its folder answers absent, drops writes and removes
/// nothing.
fn carries_on_without_the_folder(disk: &FileStore, clock: &TestClock) {
    assert!(disk.is_refused());
    assert_eq!(value(disk, "key"), None);
    put(disk, clock, "other", b"value", None);
    assert_eq!(value(disk, "other"), None);
    block_on(disk.remove("key"));
    assert_eq!(disk.file_count(), 0);
    assert_eq!(disk.clear_expired(), 0);
}

#[test]
fn a_second_store_in_the_same_process_is_refused_the_folder() {
    let folder = Folder::new("second");
    let clock = TestClock::new();
    warnings();
    put(&open(&folder, &clock), &clock, "key", b"value", None);
    let before = snapshot(folder.path());

    let first = open(&folder, &clock);
    assert!(!first.is_refused());
    // The same folder by another spelling of its path.
    let second = FileStore::builder(folder.path().join("."))
        .clock(clock.shared())
        .open()
        .expect("refusing a folder is not an error");
    carries_on_without_the_folder(&second, &clock);
    assert!(warned_of_a_shared_folder(folder.path()));
    assert_eq!(value(&first, "key"), Some(b"value".to_vec()));
    drop(second);
    drop(first);
    assert_eq!(snapshot(folder.path()), before);

    // Once the first store has closed, the folder opens again.
    let again = open(&folder, &clock);
    assert!(!again.is_refused());
    assert_eq!(value(&again, "key"), Some(b"value".to_vec()));
}

#[test]
fn stores_opening_one_folder_at_once_leave_it_to_one() {
    let folder = Folder::new("at-once");
    let clock = TestClock::new();
    let stores = 4;
    let barrier = Arc::new(Barrier::new(stores));
    let threads: Vec<_> = (0..stores)
        .map(|_| {
            let path = folder.path().to_path_buf();
            let clock = clock.clone();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                FileStore::builder(&path)
                    .clock(clock.shared())
                    .open()
                    .expect("opens")
            })
        })
        .collect();
    let opened: Vec<FileStore> = threads
        .into_iter()
        .map(|thread| thread.join().expect("opens"))
        .collect();
    let open: Vec<&FileStore> = opened.iter().filter(|disk| !disk.is_refused()).collect();
    assert_eq!(open.len(), 1);
    put(open[0], &clock, "key", b"value", None);
    assert_eq!(value(open[0], "key"), Some(b"value".to_vec()));
    for refused in opened.iter().filter(|disk| disk.is_refused()) {
        assert_eq!(value(refused, "key"), None);
    }
}

/// Names the folder `opens_a_folder_another_process_holds` opens, in a
/// process of its own.
const HELD_FOLDER: &str = "_51DEGREES_CACHING_HELD_FOLDER";

#[test]
fn a_store_in_another_process_is_refused_the_folder() {
    let folder = Folder::new("process");
    let clock = TestClock::new();
    put(&open(&folder, &clock), &clock, "key", b"value", None);
    let before = snapshot(folder.path());

    let disk = open(&folder, &clock);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "opens_a_folder_another_process_holds",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env(HELD_FOLDER, folder.path())
        .output()
        .expect("runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("refused and warned"), "{stdout}{stderr}");
    assert_eq!(value(&disk, "key"), Some(b"value".to_vec()));
    drop(disk);
    assert_eq!(snapshot(folder.path()), before);
}

/// Run by `a_store_in_another_process_is_refused_the_folder` in a process of
/// its own. Does nothing otherwise.
#[test]
fn opens_a_folder_another_process_holds() {
    let Some(path) = std::env::var_os(HELD_FOLDER) else {
        return;
    };
    let path = PathBuf::from(path);
    let clock = TestClock::new();
    warnings();
    let disk = FileStore::builder(&path)
        .clock(clock.shared())
        .open()
        .expect("refusing a folder is not an error");
    carries_on_without_the_folder(&disk, &clock);
    assert!(warned_of_a_shared_folder(&path));
    println!("refused and warned");
}

#[test]
fn a_store_opens_once_in_once_lock_get_or_init() {
    let folder = Folder::new("once-lock");
    let clock = TestClock::new();
    let disk: OnceLock<Option<FileStore>> = OnceLock::new();
    let open = || {
        disk.get_or_init(|| {
            FileStore::builder(folder.path())
                .clock(clock.shared())
                .open()
                .ok()
        })
        .as_ref()
        .expect("opens")
    };
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                let disk = open();
                assert!(!disk.is_refused());
                put(disk, &clock, "key", b"value", None);
                assert_eq!(value(disk, "key"), Some(b"value".to_vec()));
            });
        }
    });
}

#[test]
fn a_folder_that_cannot_be_written_never_fails_a_request() {
    let folder = Folder::new("unwritable");
    let clock = TestClock::new();
    let source = Source::new();
    let disk = open(&folder, &clock);
    let cache = on_disk(&clock, &disk, &source);

    // The folder goes, and a file takes its place, so no read or write of
    // the store can succeed, as with a full or failed disk.
    fs::remove_dir_all(folder.path()).unwrap();
    fs::write(folder.path(), b"not a folder").unwrap();

    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 1".to_owned()));
    // Nothing was kept, so the source is asked again.
    assert_eq!(block_on(cache.get(&1)), Ok("1 from load 2".to_owned()));
    put(&disk, &clock, "key", b"value", None);
    assert_eq!(value(&disk, "key"), None);
    assert_eq!(disk.clear_expired(), 0);
}

/// Asserts `opened` failed with an error that names `folder`.
fn fails_naming(opened: std::io::Result<FileStore>, folder: &Path) {
    let error = opened.err().expect("fails");
    assert!(
        error.to_string().contains(&folder.display().to_string()),
        "{error}"
    );
}

#[test]
fn a_store_that_cannot_make_its_folder_fails_naming_it() {
    let folder = Folder::new("not-a-folder");
    fs::write(folder.path(), b"a file").unwrap();
    let inside = folder.path().join("cache");
    fails_naming(FileStore::open(&inside), &inside);
}

/// On Unix, a folder the store cannot write in, list, or reach the files
/// of.
#[cfg(unix)]
#[test]
fn a_store_without_the_permissions_it_needs_on_its_folder_fails_naming_it() {
    use std::os::unix::fs::PermissionsExt;

    // The system checks no permission for root.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    for mode in [0o500, 0o300, 0o600] {
        let folder = Folder::new("permissions");
        let clock = TestClock::new();
        put(&open(&folder, &clock), &clock, "key", b"value", None);
        let set = |mode| fs::set_permissions(folder.path(), fs::Permissions::from_mode(mode));
        set(mode).unwrap();
        let opened = FileStore::builder(folder.path())
            .clock(clock.shared())
            .open();
        set(0o700).unwrap();
        fails_naming(opened, folder.path());
        assert_eq!(
            value(&open(&folder, &clock), "key"),
            Some(b"value".to_vec())
        );
    }
}

/// On Unix, a folder that other users can also write to, such as one made
/// under a umask of 002. The store has every permission it needs, so it
/// opens. Whether other users can reach the folder is for whoever deploys
/// the package to decide.
/// See https://github.com/51Degrees/specifications/blob/d79bbe3b2aef86273570742f6b660f29bfe72816/pipeline-specification/features/disk-cache.md?plain=1#L171-L176
#[cfg(unix)]
#[test]
fn a_store_opens_a_folder_others_can_also_write_to() {
    use std::os::unix::fs::PermissionsExt;

    for mode in [0o777, 0o775, 0o770, 0o702] {
        let folder = Folder::new("writable");
        fs::create_dir(folder.path()).unwrap();
        fs::set_permissions(folder.path(), fs::Permissions::from_mode(mode)).unwrap();
        let clock = TestClock::new();
        let disk = open(&folder, &clock);
        put(&disk, &clock, "key", b"value", None);
        assert_eq!(value(&disk, "key"), Some(b"value".to_vec()), "mode {mode:o}");
    }
}

/// On Unix, a folder that only its owner can write to opens, whoever can
/// read it.
#[cfg(unix)]
#[test]
fn a_store_opens_a_folder_only_its_owner_can_write_to() {
    use std::os::unix::fs::PermissionsExt;

    let folder = Folder::new("owner-writes");
    fs::create_dir(folder.path()).unwrap();
    fs::set_permissions(folder.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let clock = TestClock::new();
    put(&open(&folder, &clock), &clock, "key", b"value", None);
    assert_eq!(
        value(&open(&folder, &clock), "key"),
        Some(b"value".to_vec())
    );
}

/// On Windows, a folder the store cannot add a file to.
#[cfg(windows)]
#[test]
fn a_store_that_cannot_write_in_its_folder_fails_naming_it() {
    let folder = Folder::new("permissions");
    let clock = TestClock::new();
    put(&open(&folder, &clock), &clock, "key", b"value", None);
    let icacls = |args: &[&str]| {
        let status = std::process::Command::new("icacls")
            .arg(folder.path())
            .args(args)
            .stdout(std::process::Stdio::null())
            .status()
            .expect("runs icacls");
        assert!(status.success());
    };
    // Everyone is denied adding a file to the folder itself.
    icacls(&["/deny", "*S-1-1-0:(WD)"]);
    let opened = FileStore::builder(folder.path())
        .clock(clock.shared())
        .open();
    icacls(&["/remove:d", "*S-1-1-0"]);
    fails_naming(opened, folder.path());
    assert_eq!(
        value(&open(&folder, &clock), "key"),
        Some(b"value".to_vec())
    );
}

#[test]
fn a_missing_or_damaged_secret_gives_a_new_secret_and_an_empty_store() {
    let folder = Folder::new("secret");
    let clock = TestClock::new();
    let secret_path = folder.path().join(SECRET);
    put(&open(&folder, &clock), &clock, "a", b"value", None);
    let first = fs::read(&secret_path).unwrap();
    assert_eq!(first.len(), 32);

    // Missing.
    fs::remove_file(&secret_path).unwrap();
    {
        let disk = open(&folder, &clock);
        assert_eq!(disk.file_count(), 0);
        assert_eq!(value(&disk, "a"), None);
        put(&disk, &clock, "b", b"value", None);
    }
    let second = fs::read(&secret_path).unwrap();
    assert_eq!(second.len(), 32);
    assert_ne!(first, second);

    // The wrong length.
    fs::write(&secret_path, b"short").unwrap();
    {
        let disk = open(&folder, &clock);
        assert_eq!(disk.file_count(), 0);
        assert_eq!(value(&disk, "b"), None);
    }
    assert_eq!(fs::read(&secret_path).unwrap().len(), 32);

    // A folder where the secret should be cannot be held open, so the
    // store fails, naming its folder, and removes nothing.
    fs::remove_file(&secret_path).unwrap();
    fs::create_dir(&secret_path).unwrap();
    let before = snapshot(folder.path());
    let error = FileStore::builder(folder.path())
        .clock(clock.shared())
        .open()
        .err()
        .expect("fails");
    assert!(
        error
            .to_string()
            .contains(&folder.path().display().to_string()),
        "{error}"
    );
    assert_eq!(snapshot(folder.path()), before);
}

/// Every folder under `path` with the number of files directly in it and its
/// depth below `path`.
fn folders(path: &Path, depth: usize, found: &mut Vec<(PathBuf, usize, usize)>) {
    let mut files = 0;
    for entry in fs::read_dir(path).into_iter().flatten().flatten() {
        if entry.metadata().expect("metadata").is_dir() {
            folders(&entry.path(), depth + 1, found);
        } else {
            files += 1;
        }
    }
    found.push((path.to_path_buf(), files, depth));
}
