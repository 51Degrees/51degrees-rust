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

//! A store of bytes in files, the persistent tier for native hosts that have
//! no platform store.

use std::convert::Infallible;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::loading::{ByteLookup, ByteStore, Clock, ListKeys};
use crate::machine;

/// The first bytes of every file the store writes.
const MAGIC: &[u8; 4] = b"51DF";

/// The version of the file layout. A file with another version is a miss.
const FILE_FORMAT: u8 = 1;

/// The bytes before the key: the magic, the version, the drop time and the
/// key's length.
const HEADER_LEN: usize = 4 + 1 + 8 + 4;

/// The share of the volume's space a store takes when no limit is given,
/// as a fraction of the space it holds plus the space still free.
const DEFAULT_SHARE_OF_DISK: u64 = 4;

/// The most bytes a store takes when no limit is given, however large the
/// volume.
pub const DEFAULT_MOST_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// The bytes a store takes when no limit is given and the volume's space
/// cannot be read.
const FALLBACK_BYTES: u64 = 1024 * 1024 * 1024;

/// How often expired files are cleared, when no period is given.
const DEFAULT_SWEEP_EVERY: Duration = Duration::from_secs(10 * 60);

/// A temporary file older than this is left from a write that never
/// finished, and is removed when expired files are cleared.
const ABANDONED_AFTER: Duration = Duration::from_secs(60 * 60);

/// A [`ByteStore`] that keeps each entry in a file of its own, for a native
/// host with no platform store. Wrapped in an
/// [`EncodedStore`](crate::EncodedStore) it is the persistent tier of a
/// [`LoadingCache`](crate::LoadingCache), below a cache in memory.
///
/// - A file is named by a hash of its key, so any key is a safe file name,
///   and holds the key itself, so two keys that share a hash are never
///   confused: the second simply replaces the first.
/// - A write goes to a temporary file that is then renamed over the entry,
///   so a reader sees the old entry or the new one, never part of either,
///   even in another process sharing the folder.
/// - Each file holds the time it must be dropped. An entry read after that
///   time answers as absent and its file is removed.
/// - Reading an entry marks its file as used. An entry not read for the
///   [time to idle](FileStoreBuilder::time_to_idle) answers as absent.
/// - The files are held within a number of bytes. Once over it, the files
///   used least recently are removed until the store is back under nine
///   tenths of it.
/// - Expired files are cleared on [`FileStore::clear_expired`], when the
///   store opens, and as writes arrive, at most once per
///   [sweep period](FileStoreBuilder::sweep_every).
///
/// The files outlive the process, so a store opened again on the same
/// folder holds what it held before. The folder belongs to the store, which
/// may remove anything in it.
///
/// Files are read and written on the calling thread. A read or write of a
/// small entry takes a few microseconds on a local disk, which suits the
/// sizes a cache holds. A store that fails to read reports a miss, and one
/// that fails to write drops the write, as every [`ByteStore`] does.
pub struct FileStore {
    entries: PathBuf,
    temp: PathBuf,
    max_bytes: u64,
    time_to_idle: Option<Duration>,
    sweep_every: Duration,
    clock: Arc<dyn Clock>,
    /// The bytes the files take, as last counted and kept up to date by each
    /// write and removal since.
    held: AtomicU64,
    /// Held while files are being counted and removed, so only one caller
    /// does it at a time.
    maintaining: Mutex<()>,
    /// When expired files are next cleared, as nanoseconds since the epoch.
    next_sweep: AtomicU64,
    /// Numbers the temporary files of this store.
    writes: AtomicU64,
}

/// One file as the store found it.
struct Found {
    path: PathBuf,
    len: u64,
    used: SystemTime,
}

impl FileStore {
    /// Starts building a store over the folder `dir`, which is made if it
    /// does not exist.
    pub fn builder(dir: impl Into<PathBuf>) -> FileStoreBuilder {
        FileStoreBuilder {
            dir: dir.into(),
            max_bytes: None,
            time_to_idle: None,
            sweep_every: DEFAULT_SWEEP_EVERY,
            clock: None,
        }
    }

    /// Opens a store over the folder `dir` with the defaults: a limit taken
    /// from the volume's space, no time to idle and the system clock.
    pub fn open(dir: impl Into<PathBuf>) -> io::Result<Self> {
        Self::builder(dir).open()
    }

    /// The most bytes the files take.
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// The bytes the files take, as last counted and kept up to date since.
    pub fn held_bytes(&self) -> u64 {
        self.held.load(Ordering::Relaxed)
    }

    /// The number of entries in the folder, including any that have expired
    /// and not yet been cleared.
    pub fn file_count(&self) -> usize {
        self.files().len()
    }

    /// Removes every file past its drop time or its time to idle, and any
    /// temporary file left by a write that never finished. Returns the
    /// number of entries removed.
    pub fn clear_expired(&self) -> usize {
        let _maintaining = self.maintaining.lock().unwrap_or_else(|e| e.into_inner());
        self.clear_expired_locked()
    }

    fn clear_expired_locked(&self) -> usize {
        let now = self.clock.now();
        let mut removed = 0;
        let mut held = 0;
        for found in self.files() {
            let expired = self.idle_past(found.used, now)
                || match read_header(&found.path) {
                    Some((drop_at, _)) => drop_at.is_some_and(|drop_at| now >= drop_at),
                    // Not a file this store reads, so it is of no use.
                    None => true,
                };
            if expired && fs::remove_file(&found.path).is_ok() {
                removed += 1;
            } else {
                held += found.len;
            }
        }
        self.held.store(held, Ordering::Relaxed);
        self.clear_abandoned_writes();
        self.next_sweep.store(
            nanos(now.checked_add(self.sweep_every).unwrap_or(now)),
            Ordering::Relaxed,
        );
        removed
    }

    /// Removes the files used least recently until the files take no more
    /// than nine tenths of the limit, and returns the number removed. Does
    /// nothing when they are within the limit.
    fn trim(&self) -> usize {
        // Another caller is already counting and removing files.
        let Ok(_maintaining) = self.maintaining.try_lock() else {
            return 0;
        };
        let mut files = self.files();
        let mut held: u64 = files.iter().map(|found| found.len).sum();
        if held <= self.max_bytes {
            self.held.store(held, Ordering::Relaxed);
            return 0;
        }
        let target = self.max_bytes / 10 * 9;
        files.sort_by_key(|found| found.used);
        let mut removed = 0;
        for found in files {
            if held <= target {
                break;
            }
            if fs::remove_file(&found.path).is_ok() {
                held = held.saturating_sub(found.len);
                removed += 1;
            }
        }
        self.held.store(held, Ordering::Relaxed);
        removed
    }

    /// Clears expired files when the sweep period has passed, and removes
    /// the least recently used files when over the limit.
    fn maintain(&self, now: SystemTime) {
        if nanos(now) >= self.next_sweep.load(Ordering::Relaxed) {
            if let Ok(_maintaining) = self.maintaining.try_lock() {
                self.clear_expired_locked();
            }
        }
        if self.held.load(Ordering::Relaxed) > self.max_bytes {
            self.trim();
        }
    }

    fn idle_past(&self, used: SystemTime, now: SystemTime) -> bool {
        self.time_to_idle
            .is_some_and(|idle| used.checked_add(idle).is_some_and(|until| now >= until))
    }

    /// The file `key` is kept in. The first two hex digits of the hash name
    /// a folder, so no one folder holds too many files.
    fn path_for(&self, key: &str) -> PathBuf {
        let hash = format!("{:016x}", fnv1a(key.as_bytes()));
        self.entries.join(&hash[..2]).join(&hash)
    }

    /// Every entry file, with its length and when it was last used.
    fn files(&self) -> Vec<Found> {
        let mut found = Vec::new();
        let Ok(folders) = fs::read_dir(&self.entries) else {
            return found;
        };
        for folder in folders.flatten() {
            let Ok(files) = fs::read_dir(folder.path()) else {
                continue;
            };
            for file in files.flatten() {
                let Ok(metadata) = file.metadata() else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }
                found.push(Found {
                    path: file.path(),
                    len: metadata.len(),
                    used: metadata.modified().unwrap_or(UNIX_EPOCH),
                });
            }
        }
        found
    }

    fn clear_abandoned_writes(&self) {
        let Ok(files) = fs::read_dir(&self.temp) else {
            return;
        };
        let now = SystemTime::now();
        for file in files.flatten() {
            let abandoned = file
                .metadata()
                .and_then(|metadata| metadata.modified())
                .map(|modified| {
                    now.duration_since(modified)
                        .is_ok_and(|age| age >= ABANDONED_AFTER)
                })
                .unwrap_or(true);
            if abandoned {
                let _ = fs::remove_file(file.path());
            }
        }
    }

    /// Writes `bytes` to a temporary file and renames it over `path`.
    fn write_file(&self, path: &Path, bytes: &[u8], used: SystemTime) -> io::Result<()> {
        let number = self.writes.fetch_add(1, Ordering::Relaxed);
        let temp = self
            .temp
            .join(format!("{}-{number}.tmp", std::process::id()));
        let written = (|| {
            let mut file = File::create(&temp)?;
            file.write_all(bytes)?;
            file.set_modified(used)?;
            drop(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::rename(&temp, path)
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temp);
        }
        written
    }

    fn remove_path(&self, path: &Path) {
        let len = fs::metadata(path).map(|metadata| metadata.len()).ok();
        if fs::remove_file(path).is_ok() {
            if let Some(len) = len {
                self.release(len);
            }
        }
    }

    fn release(&self, len: u64) {
        let mut held = self.held.load(Ordering::Relaxed);
        while let Err(now) = self.held.compare_exchange_weak(
            held,
            held.saturating_sub(len),
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            held = now;
        }
    }
}

impl ByteStore for FileStore {
    type Reservation = Infallible;

    /// Files stay until a read finds them past their time or a sweep clears
    /// them.
    const DROPS_EXPIRED: bool = false;

    async fn get(&self, key: &str) -> ByteLookup<Infallible> {
        let path = self.path_for(key);
        let Ok(metadata) = fs::metadata(&path) else {
            return ByteLookup::Miss;
        };
        let Ok(bytes) = fs::read(&path) else {
            return ByteLookup::Miss;
        };
        let Some((drop_at, stored_key, payload)) = parse(&bytes) else {
            return ByteLookup::Miss;
        };
        if stored_key != key.as_bytes() {
            return ByteLookup::Miss;
        }
        let now = self.clock.now();
        let used = metadata.modified().unwrap_or(UNIX_EPOCH);
        if drop_at.is_some_and(|drop_at| now >= drop_at) || self.idle_past(used, now) {
            self.remove_path(&path);
            return ByteLookup::Miss;
        }
        // Marking the file used keeps it from idling out and from being the
        // first removed when the store is over its limit.
        if let Ok(file) = File::options().write(true).open(&path) {
            let _ = file.set_modified(now);
        }
        ByteLookup::Hit(payload.to_vec())
    }

    async fn put(&self, key: &str, bytes: Vec<u8>, keep_for: Option<Duration>) {
        let now = self.clock.now();
        let drop_at = keep_for.and_then(|keep_for| now.checked_add(keep_for));
        let file = compose(key, drop_at, &bytes);
        let path = self.path_for(key);
        let replaced = fs::metadata(&path).map(|metadata| metadata.len()).ok();
        if self.write_file(&path, &file, now).is_ok() {
            if let Some(replaced) = replaced {
                self.release(replaced);
            }
            self.held.fetch_add(file.len() as u64, Ordering::Relaxed);
        }
        self.maintain(now);
    }

    async fn remove(&self, key: &str) {
        self.remove_path(&self.path_for(key));
    }
}

impl ListKeys for FileStore {
    async fn keys(&self, prefix: &str) -> Option<Vec<String>> {
        let keys = self
            .files()
            .into_iter()
            .filter_map(|found| read_header(&found.path).map(|(_, key)| key))
            .filter_map(|key| String::from_utf8(key).ok())
            .filter(|key| key.starts_with(prefix))
            .collect();
        Some(keys)
    }
}

/// Builds a [`FileStore`].
pub struct FileStoreBuilder {
    dir: PathBuf,
    max_bytes: Option<u64>,
    time_to_idle: Option<Duration>,
    sweep_every: Duration,
    clock: Option<Arc<dyn Clock>>,
}

impl FileStoreBuilder {
    /// The most bytes the files take. Defaults to a quarter of the space
    /// the store holds plus the space still free on its volume, at most
    /// [`DEFAULT_MOST_BYTES`].
    pub fn max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = Some(max_bytes);
        self
    }

    /// How long an entry may go unread before it answers as absent and its
    /// file is cleared. Defaults to no limit, leaving each entry to the
    /// lifetime it was written with.
    pub fn time_to_idle(mut self, time_to_idle: Duration) -> Self {
        self.time_to_idle = Some(time_to_idle);
        self
    }

    /// How often writes clear expired files. Defaults to ten minutes.
    pub fn sweep_every(mut self, sweep_every: Duration) -> Self {
        self.sweep_every = sweep_every;
        self
    }

    /// The clock that sets and checks drop times and use. Defaults to the
    /// system clock.
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Opens the store, making its folder if needed. Clears the expired
    /// files and, when over the limit, the least recently used.
    ///
    /// # Errors
    ///
    /// When the folder cannot be made.
    pub fn open(self) -> io::Result<FileStore> {
        let entries = self.dir.join("entries");
        let temp = self.dir.join("tmp");
        fs::create_dir_all(&entries)?;
        fs::create_dir_all(&temp)?;
        let clock = self
            .clock
            .unwrap_or_else(|| Arc::new(crate::loading::SystemClock));
        let mut store = FileStore {
            entries,
            temp,
            max_bytes: u64::MAX,
            time_to_idle: self.time_to_idle,
            sweep_every: self.sweep_every,
            clock,
            held: AtomicU64::new(0),
            maintaining: Mutex::new(()),
            next_sweep: AtomicU64::new(0),
            writes: AtomicU64::new(0),
        };
        store.clear_expired();
        store.max_bytes = self
            .max_bytes
            .unwrap_or_else(|| default_max_bytes(&self.dir, store.held_bytes()));
        store.trim();
        Ok(store)
    }
}

/// A quarter of the space the store holds plus the space still free, at
/// most [`DEFAULT_MOST_BYTES`].
fn default_max_bytes(dir: &Path, held: u64) -> u64 {
    match machine::disk(dir) {
        Some(disk) => {
            (disk.available.saturating_add(held) / DEFAULT_SHARE_OF_DISK).min(DEFAULT_MOST_BYTES)
        }
        None => FALLBACK_BYTES,
    }
}

/// The 64-bit FNV-1a hash. It is fixed, so a key names the same file in
/// every process and release.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn nanos(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// A file's bytes: the header, the key and the stored bytes.
fn compose(key: &str, drop_at: Option<SystemTime>, bytes: &[u8]) -> Vec<u8> {
    let mut file = Vec::with_capacity(HEADER_LEN + key.len() + bytes.len());
    file.extend_from_slice(MAGIC);
    file.push(FILE_FORMAT);
    // Zero is no drop time. A drop time is never the epoch itself.
    file.extend_from_slice(&drop_at.map_or(0, |at| nanos(at).max(1)).to_le_bytes());
    file.extend_from_slice(&(key.len() as u32).to_le_bytes());
    file.extend_from_slice(key.as_bytes());
    file.extend_from_slice(bytes);
    file
}

/// The drop time, the key and the stored bytes of a file's bytes, or `None`
/// when they are not a file this store wrote.
fn parse(file: &[u8]) -> Option<(Option<SystemTime>, &[u8], &[u8])> {
    let (drop_at, key_len) = parse_header(file.get(..HEADER_LEN)?)?;
    let rest = &file[HEADER_LEN..];
    let key = rest.get(..key_len)?;
    Some((drop_at, key, &rest[key_len..]))
}

fn parse_header(header: &[u8]) -> Option<(Option<SystemTime>, usize)> {
    if header.get(..4)? != MAGIC || *header.get(4)? != FILE_FORMAT {
        return None;
    }
    let drop_at = u64::from_le_bytes(header.get(5..13)?.try_into().ok()?);
    let key_len = u32::from_le_bytes(header.get(13..17)?.try_into().ok()?) as usize;
    let drop_at = (drop_at != 0).then(|| UNIX_EPOCH + Duration::from_nanos(drop_at));
    Some((drop_at, key_len))
}

/// The drop time and the key of the file at `path`, reading only the start
/// of it.
fn read_header(path: &Path) -> Option<(Option<SystemTime>, Vec<u8>)> {
    let mut file = File::open(path).ok()?;
    let mut header = [0u8; HEADER_LEN];
    file.read_exact(&mut header).ok()?;
    let (drop_at, key_len) = parse_header(&header)?;
    let mut key = vec![0u8; key_len];
    file.read_exact(&mut key).ok()?;
    Some((drop_at, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_reads_back_as_written() {
        let at = UNIX_EPOCH + Duration::from_secs(1_000);
        let file = compose("a/key", Some(at), b"value");
        let (drop_at, key, payload) = parse(&file).expect("parses");
        assert_eq!(drop_at, Some(at));
        assert_eq!(key, b"a/key");
        assert_eq!(payload, b"value");
        let file = compose("k", None, b"");
        assert_eq!(parse(&file), Some((None, &b"k"[..], &b""[..])));
    }

    #[test]
    fn bytes_this_store_did_not_write_are_not_a_file() {
        assert!(parse(b"").is_none());
        assert!(parse(b"51DF\x02\0\0\0\0\0\0\0\0\0\0\0\0").is_none());
        let mut file = compose("key", None, b"value");
        file.truncate(HEADER_LEN + 1);
        assert!(parse(&file).is_none());
    }

    #[test]
    fn the_hash_is_fixed() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
