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

//! A store of bytes in files, the disk tier for native hosts.
//!
//! The folder holds a secret and two levels of folders. Each file is named
//! by the lowercase hexadecimal HMAC-SHA-256 of its key under the secret,
//! and sits at `<first two characters>/<next two>/<name>`. Each file holds,
//! in order:
//!
//! | Bytes | Field |
//! |---|---|
//! | 4 | Marker, the ASCII characters `51DF` |
//! | 1 | File format version, 1 |
//! | 4 | Length of the key in bytes, a little-endian `u32` |
//! | Length | The key |
//! | Rest | The entry, in the [entry format](crate::encode_entry) |
//!
//! The entry holds Data published and the value format version, which
//! [`EncodedStore`](crate::EncodedStore) compares on every read. An entry
//! from other data or in another value format answers absent, and its file
//! is removed.
//!
//! Every time the store relies on is in the entry: Written, Renewed and the
//! drop time. No decision reads or writes a file system time.
//!
//! A folder is open in one store at a time. Within the process a list of
//! the folders open, by their canonical paths, says which are taken, and
//! across processes each store holds an exclusive lock on its secret file.
//! The list is needed as well, because some systems' locks belong to the
//! whole process and do not stop a second store in it.

use std::convert::Infallible;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::loading::{read_header, ByteLookup, ByteStore, Clock, Header, ListKeys};
use crate::machine;

/// The first bytes of every file the store writes.
const MARKER: &[u8; 4] = b"51DF";

/// The version of the file layout. A file with another version is removed.
const FILE_FORMAT: u8 = 1;

/// The bytes before the key: the marker, the version and the key's length.
const PREFIX_LEN: usize = 4 + 1 + 4;

/// The most bytes of an entry before its value: the version, the flags, four
/// times, Data published and the value format version.
const MOST_ENTRY_HEADER_LEN: usize = 2 + 4 * 8 + 8 + 2;

/// Where Renewed sits in an entry: after the version, the flags and Written.
const RENEWED_IN_ENTRY: usize = 2 + 8;

/// The file in the folder root that holds the secret.
const SECRET_FILE: &str = "cache.secret";

/// The bytes of the secret.
const SECRET_LEN: usize = 32;

/// The hexadecimal characters of a file name.
const NAME_LEN: usize = 64;

/// The suffix of a file being written, before it is renamed into place.
const TEMP_SUFFIX: &str = ".tmp";

/// The share of the volume's space a store takes when no limit is given,
/// as a fraction of the space it holds plus the space still free.
const DEFAULT_SHARE_OF_DISK: u64 = 4;

/// The most bytes a store takes when no limit is given, however large the
/// volume.
pub const DEFAULT_MOST_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// The bytes a store takes when no limit is given and the volume's space
/// cannot be read.
const FALLBACK_BYTES: u64 = 1024 * 1024 * 1024;

/// With no time to idle, a read rewrites Renewed once it is this old, so
/// trimming still goes by last use, to within this.
const RENEW_WITH_NO_IDLE: Duration = Duration::from_secs(60 * 60);

/// The locks that order the writes, renewals and removals of each file.
const WRITE_LOCKS: usize = 16;

/// How often expired files are cleared, when no period is given.
const DEFAULT_SWEEP_EVERY: Duration = Duration::from_secs(10 * 60);

/// A temporary file whose name carries a time this far from now is left
/// from a write that never finished, and is removed.
const ABANDONED_AFTER: Duration = Duration::from_secs(10 * 60);

/// A [`ByteStore`] that keeps each entry in a file of its own, for a native
/// host. Wrapped in an [`EncodedStore`](crate::EncodedStore) it is the disk
/// tier of a [`LoadingCache`](crate::LoadingCache), below a cache in memory.
/// It comes with the `disk` feature, which is off by default.
///
/// - The store creates a 32-byte random secret in the folder root the first
///   time it uses the folder, and reads it back each time it opens. A
///   secret that is missing, unreadable or not 32 bytes long is replaced,
///   and the files named under the old one are removed.
/// - A file is named by the HMAC-SHA-256 of its key under the secret, so a
///   sender who chooses keys cannot tell which folder a key lands in, and
///   any key is a safe file name. The file sits two folders down, which
///   spreads the files over 65,536 folders.
/// - The file holds the key, compared on every read, so a file found for
///   another key answers as absent.
/// - The file holds the entry as [`EncodedStore`](crate::EncodedStore)
///   wrote it, with its Written, Renewed and drop times. A `put` of bytes
///   that are not an entry is dropped.
/// - A write goes to a temporary file in the same folder that is then
///   renamed into place, so a reader sees the old entry or the new one,
///   never part of either.
/// - An entry past its drop time, unused for the
///   [time to idle](FileStoreBuilder::time_to_idle), or written later than
///   now answers as absent and its file is removed.
/// - A read rewrites Renewed in the file when it is more than a tenth of
///   the time to idle in the past, or more than an hour when no time to
///   idle is set, so idle and last use are measured without a write on
///   every read. The rewrite writes the whole file under a temporary name
///   and renames it into place, and is dropped when a newer write of the
///   entry has landed since the read.
/// - The files are held within a number of bytes. Once over it, the files
///   with the oldest Renewed are removed until the store is back under
///   nine tenths of it.
/// - Expired files, and temporary files left by a write that never
///   finished, are cleared on [`FileStore::clear_expired`], when the store
///   opens, and as writes arrive, at most once per
///   [sweep period](FileStoreBuilder::sweep_every).
///
/// The store removes only files whose names and contents it recognizes as
/// its own, so a folder setting pointed at the wrong place never deletes
/// anything else. On Unix it creates its folders readable only by the
/// process's user. The files outlive the process, so a store opened again
/// on the same folder holds what it held before.
///
/// Each store needs a folder of its own. A store opened on a folder that
/// another store has open, in this process or another, is
/// [refused](FileStore::is_refused): it logs a warning, leaves every file
/// in the folder untouched, and carries on without the disk, answering
/// absent and dropping writes, so no request fails. The folder is free
/// again once the store that has it is dropped.
///
/// Files are read and written on the calling thread. A read or write of a
/// small entry takes a few microseconds on a local disk, which suits the
/// sizes a cache holds. A failure to read or write, a full disk included,
/// never fails a request: a read answers absent, a write is dropped, and
/// either logs a warning.
pub struct FileStore {
    root: PathBuf,
    secret: [u8; SECRET_LEN],
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
    /// Held while a file is written, renewed or removed, chosen by its
    /// name, so a renewal never lands over a newer write.
    writing: [Mutex<()>; WRITE_LOCKS],
    /// The store's hold on its folder, or `None` when another store has the
    /// folder open and this one carries on without it.
    hold: Option<Hold>,
}

/// The canonical paths of the folders the stores in this process have
/// open.
static OPEN_FOLDERS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn open_folders() -> std::sync::MutexGuard<'static, Vec<PathBuf>> {
    OPEN_FOLDERS.lock().unwrap_or_else(|e| e.into_inner())
}

/// A store's hold on its folder: its place in [`OPEN_FOLDERS`] and its
/// secret file, locked against stores in other processes.
struct Hold {
    folder: PathBuf,
    secret: Option<File>,
}

impl Drop for Hold {
    fn drop(&mut self) {
        // Unlocked first, so a store that finds the folder gone from the
        // list finds the secret free too.
        drop(self.secret.take());
        open_folders().retain(|open| *open != self.folder);
    }
}

/// Numbers the temporary files of every store in the process, so two stores
/// on one folder never pick the same name.
static TEMP_FILES: AtomicU64 = AtomicU64::new(0);

/// What a file's bytes are, as far as the store is concerned.
#[derive(Debug, PartialEq, Eq)]
enum Contents<'a> {
    /// Not a file the store writes. It is left alone.
    Foreign,
    /// Starts with the store's marker but cannot be read, being truncated,
    /// of another version or holding no entry. It is removed.
    Unreadable,
    /// A file the store wrote.
    Entry {
        key: &'a [u8],
        header: Header,
        /// The entry, from its format version to the end.
        entry: &'a [u8],
    },
}

/// An entry file the store found, with what it needs to trim.
struct Found {
    path: PathBuf,
    len: u64,
    renewed: SystemTime,
}

/// The files found under the root that the store's names match.
#[derive(Default)]
struct Listing {
    /// Files named as entries are.
    entries: Vec<(PathBuf, u64)>,
    /// Files named as temporary files are, with the time in the name.
    temps: Vec<(PathBuf, SystemTime)>,
}

impl FileStore {
    /// Starts building a store over the folder `dir`, which is made when the
    /// store opens if it does not exist. Nothing is made before then.
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
    ///
    /// # Errors
    ///
    /// As [`FileStoreBuilder::open`].
    pub fn open(dir: impl Into<PathBuf>) -> io::Result<Self> {
        Self::builder(dir).open()
    }

    /// True when another store had the folder open when this one opened, so
    /// this one carries on without the disk: every read answers absent and
    /// every write is dropped.
    pub fn is_refused(&self) -> bool {
        self.hold.is_none()
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
        if self.is_refused() {
            return 0;
        }
        self.list()
            .entries
            .iter()
            .filter(|(path, _)| self.own_entry(path).is_some())
            .count()
    }

    /// Removes every file past its drop time or its time to idle, every
    /// file the store cannot read or find by name, and any temporary file
    /// left by a write that never finished. Returns the number of entries
    /// removed.
    pub fn clear_expired(&self) -> usize {
        if self.is_refused() {
            return 0;
        }
        let _maintaining = self.maintaining.lock().unwrap_or_else(|e| e.into_inner());
        self.clear_expired_locked()
    }

    fn clear_expired_locked(&self) -> usize {
        let now = self.clock.now();
        let listing = self.list();
        let mut removed = 0;
        let mut held = 0;
        for (path, len) in listing.entries {
            let remove = match read_start(&path) {
                Ok(bytes) => match contents(&bytes) {
                    Contents::Foreign => continue,
                    Contents::Unreadable => true,
                    Contents::Entry { key, header, .. } => {
                        // A file not at its key's name was written under
                        // another secret, or by a test, and is never found.
                        self.name_for(key) != file_name(&path) || self.is_dead(&header, now)
                    }
                },
                // Removed by another process since it was listed.
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    log::warn!("The disk cache could not read {}: {error}", path.display());
                    held += len;
                    continue;
                }
            };
            if remove && self.remove_entry(&path) {
                removed += 1;
            } else if !remove {
                held += len;
            }
        }
        for (path, at) in listing.temps {
            if abandoned(at, now) {
                let _ = fs::remove_file(&path);
            }
        }
        self.clear_abandoned_secrets(now);
        self.held.store(held, Ordering::Relaxed);
        self.next_sweep.store(
            nanos(now.checked_add(self.sweep_every).unwrap_or(now)),
            Ordering::Relaxed,
        );
        removed
    }

    /// Removes the files with the oldest Renewed until the files take no
    /// more than nine tenths of the limit, and returns the number removed.
    /// Does nothing when they are within the limit.
    fn trim(&self) -> usize {
        // Another caller is already counting and removing files.
        let Ok(_maintaining) = self.maintaining.try_lock() else {
            return 0;
        };
        let mut files: Vec<Found> = self
            .list()
            .entries
            .into_iter()
            .filter_map(|(path, len)| {
                let renewed = self.own_entry(&path)?.renewed;
                Some(Found { path, len, renewed })
            })
            .collect();
        let mut held: u64 = files.iter().map(|found| found.len).sum();
        if held <= self.max_bytes {
            self.held.store(held, Ordering::Relaxed);
            return 0;
        }
        let target = self.max_bytes / 10 * 9;
        files.sort_by_key(|found| found.renewed);
        let mut removed = 0;
        for found in files {
            if held <= target {
                break;
            }
            if self.remove_entry(&found.path) {
                held = held.saturating_sub(found.len);
                removed += 1;
            }
        }
        self.held.store(held, Ordering::Relaxed);
        removed
    }

    /// Clears expired files when the sweep period has passed, and removes
    /// the files with the oldest Renewed when over the limit.
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

    /// True when the entry must no longer be returned: written later than
    /// now, past its drop time or unused for the time to idle.
    fn is_dead(&self, header: &Header, now: SystemTime) -> bool {
        header.written > now
            || header.drop_at.is_some_and(|drop_at| now >= drop_at)
            || self.time_to_idle.is_some_and(|idle| {
                header
                    .renewed
                    .checked_add(idle)
                    .is_some_and(|until| now >= until)
            })
    }

    /// True when a read now should rewrite Renewed, being when it is more
    /// than a tenth of the time to idle in the past, or more than an hour
    /// when no time to idle is set.
    fn renewal_due(&self, renewed: SystemTime, now: SystemTime) -> bool {
        let after = self
            .time_to_idle
            .map_or(RENEW_WITH_NO_IDLE, |idle| idle / 10);
        now.duration_since(renewed).is_ok_and(|since| since > after)
    }

    /// The lock that orders the writes, renewals and removals of the file at
    /// `path`.
    fn write_lock(&self, path: &Path) -> std::sync::MutexGuard<'_, ()> {
        let first = file_name(path).bytes().next().unwrap_or(0);
        self.writing[usize::from(first) % WRITE_LOCKS]
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// The Header of the entry at `path` when it is a file this store wrote
    /// under its key's name.
    fn own_entry(&self, path: &Path) -> Option<Header> {
        let bytes = read_start(path).ok()?;
        match contents(&bytes) {
            Contents::Entry { key, header, .. } if self.name_for(key) == file_name(path) => {
                Some(header)
            }
            _ => None,
        }
    }

    /// The file name for `key`: the lowercase hexadecimal HMAC-SHA-256 of
    /// the key under the secret.
    fn name_for(&self, key: &[u8]) -> String {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.secret).expect("HMAC takes a key of any length");
        mac.update(key);
        hex(&mac.finalize().into_bytes())
    }

    /// The file `key` is kept in, two folders down, named by the first two
    /// pairs of characters of its name, so the files spread over 65,536
    /// folders and no folder holds more than 256 folders. 8 million entries
    /// come to about 120 files in each.
    fn path_for(&self, key: &str) -> PathBuf {
        let name = self.name_for(key.as_bytes());
        self.root.join(&name[..2]).join(&name[2..4]).join(&name)
    }

    /// Removes the entry file at `path`, then the folders above it that this
    /// leaves empty, up to the root. True when the file was removed.
    fn remove_entry(&self, path: &Path) -> bool {
        let removed = {
            let _writing = self.write_lock(path);
            fs::remove_file(path).is_ok()
        };
        if !removed {
            return false;
        }
        let mut folder = path.parent();
        while let Some(current) = folder {
            // Removing a folder that still holds anything fails, which ends
            // the climb.
            if current == self.root || fs::remove_dir(current).is_err() {
                break;
            }
            folder = current.parent();
        }
        true
    }

    /// Every file in the two levels of folders below the root whose name is
    /// one the store gives, with its length or the time in its name. Other
    /// files and folders are left out.
    fn list(&self) -> Listing {
        let mut listing = Listing::default();
        let subfolders = |path: &Path| {
            fs::read_dir(path)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|entry| {
                    entry.file_type().is_ok_and(|kind| kind.is_dir())
                        && entry.file_name().to_str().is_some_and(is_folder_name)
                })
                .map(|entry| entry.path())
                .collect::<Vec<_>>()
        };
        for folder in subfolders(&self.root)
            .iter()
            .flat_map(|outer| subfolders(outer))
        {
            let Ok(files) = fs::read_dir(&folder) else {
                continue;
            };
            for file in files.flatten() {
                let Ok(metadata) = file.metadata() else {
                    continue;
                };
                if !metadata.is_file() {
                    continue;
                }
                let path = file.path();
                let Some(name) = file.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if is_entry_name(&name) && in_its_folders(&path, &name) {
                    listing.entries.push((path, metadata.len()));
                } else if let Some(at) = temp_time(&name, is_entry_name) {
                    listing.temps.push((path, at));
                }
            }
        }
        listing
    }

    /// Removes the temporary files a crash left while the secret was being
    /// written.
    fn clear_abandoned_secrets(&self, now: SystemTime) {
        for file in fs::read_dir(&self.root).into_iter().flatten().flatten() {
            let Some(name) = file.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if let Some(at) = temp_time(&name, |stem| stem == SECRET_FILE) {
                if abandoned(at, now) {
                    let _ = fs::remove_file(file.path());
                }
            }
        }
    }

    /// A temporary file name for `stem`, carrying the clock's time, so a
    /// later open can tell an abandoned one without a file system time.
    fn temp_name(&self, stem: &str, now: SystemTime) -> String {
        let number = TEMP_FILES.fetch_add(1, Ordering::Relaxed);
        format!(
            "{stem}.{}-{}-{number}{TEMP_SUFFIX}",
            nanos(now),
            std::process::id()
        )
    }

    /// Writes `bytes` to a temporary file beside `path` and renames it into
    /// place.
    fn write_file(&self, path: &Path, bytes: &[u8], now: SystemTime) -> io::Result<()> {
        let folder = path.parent().unwrap_or(&self.root);
        make_folder(folder)?;
        let name = file_name(path);
        let temp = folder.join(self.temp_name(&name, now));
        let written = (|| {
            let mut file = match create_private(&temp) {
                // Another caller removed the folder as it emptied it, after
                // it was made, so it is made again.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    make_folder(folder)?;
                    create_private(&temp)?
                }
                other => other?,
            };
            file.write_all(bytes)?;
            drop(file);
            fs::rename(&temp, path)
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temp);
        }
        written
    }

    /// Writes the file `read`, as a read of `key` found it at `path`, with
    /// Renewed set to `now`, under a temporary name, and renames it into
    /// place. Does nothing when the file has changed since, so an older
    /// value never lands over a newer write.
    fn renew(&self, path: &Path, key: &str, read: &[u8], now: SystemTime) {
        let offset = PREFIX_LEN + key.len() + RENEWED_IN_ENTRY;
        if read.len() < offset + 8 {
            return;
        }
        let mut bytes = read.to_vec();
        bytes[offset..offset + 8].copy_from_slice(&nanos(now).to_le_bytes());
        let _writing = self.write_lock(path);
        match fs::read(path) {
            Ok(current) if current == read => {}
            // Written again, renewed or removed since the read.
            Ok(_) => return,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return,
            Err(error) => {
                log::warn!("The disk cache could not renew {}: {error}", path.display());
                return;
            }
        }
        if let Err(error) = self.write_file(path, &bytes, now) {
            log::warn!("The disk cache could not renew {}: {error}", path.display());
        }
    }

    fn remove_path(&self, path: &Path, len: u64) {
        if self.remove_entry(path) {
            self.release(len);
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
        if self.is_refused() {
            return ByteLookup::Miss;
        }
        let path = self.path_for(key);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    log::warn!("The disk cache could not read {}: {error}", path.display());
                }
                return ByteLookup::Miss;
            }
        };
        let len = bytes.len() as u64;
        match contents(&bytes) {
            Contents::Foreign => ByteLookup::Miss,
            Contents::Unreadable => {
                self.remove_path(&path, len);
                ByteLookup::Miss
            }
            // Another key's file, which only a forced collision gives.
            Contents::Entry { key: stored, .. } if stored != key.as_bytes() => ByteLookup::Miss,
            Contents::Entry { header, entry, .. } => {
                let now = self.clock.now();
                if self.is_dead(&header, now) {
                    self.remove_path(&path, len);
                    return ByteLookup::Miss;
                }
                if self.renewal_due(header.renewed, now) {
                    self.renew(&path, key, &bytes, now);
                }
                ByteLookup::Hit(entry.to_vec())
            }
        }
    }

    /// Writes the entry `bytes` for `key`. The drop time is the one the
    /// entry holds, which [`EncodedStore`](crate::EncodedStore) sets from
    /// `keep_for`.
    async fn put(&self, key: &str, bytes: Vec<u8>, _keep_for: Option<Duration>) {
        if self.is_refused() {
            return;
        }
        if read_header(&bytes).is_err() {
            log::warn!("The disk cache dropped a write of bytes that are not an entry.");
            return;
        }
        let Ok(key_len) = u32::try_from(key.len()) else {
            return;
        };
        let now = self.clock.now();
        let file = compose(key, key_len, &bytes);
        let path = self.path_for(key);
        let written = {
            let _writing = self.write_lock(&path);
            let replaced = fs::metadata(&path).map(|metadata| metadata.len()).ok();
            self.write_file(&path, &file, now).map(|()| replaced)
        };
        match written {
            Ok(replaced) => {
                if let Some(replaced) = replaced {
                    self.release(replaced);
                }
                self.held.fetch_add(file.len() as u64, Ordering::Relaxed);
            }
            Err(error) => {
                log::warn!(
                    "The disk cache dropped a write to {}: {error}",
                    path.display()
                );
            }
        }
        self.maintain(now);
    }

    async fn remove(&self, key: &str) {
        if self.is_refused() {
            return;
        }
        let path = self.path_for(key);
        if let Ok(metadata) = fs::metadata(&path) {
            if let Ok(bytes) = read_start(&path) {
                if matches!(contents(&bytes), Contents::Entry { key: stored, .. } if stored == key.as_bytes())
                {
                    self.remove_path(&path, metadata.len());
                }
            }
        }
    }
}

impl ListKeys for FileStore {
    async fn keys(&self, prefix: &str) -> Option<Vec<String>> {
        if self.is_refused() {
            return Some(Vec::new());
        }
        let keys = self
            .list()
            .entries
            .into_iter()
            .filter_map(|(path, _)| {
                let bytes = read_start(&path).ok()?;
                match contents(&bytes) {
                    Contents::Entry { key, .. } if self.name_for(key) == file_name(&path) => {
                        String::from_utf8(key.to_vec()).ok()
                    }
                    _ => None,
                }
            })
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
    /// lifetime it was written with. With no time to idle, a read rewrites
    /// Renewed once it is more than an hour old, so trimming goes by last
    /// use to within an hour.
    pub fn time_to_idle(mut self, time_to_idle: Duration) -> Self {
        self.time_to_idle = Some(time_to_idle);
        self
    }

    /// How often writes clear expired files. Defaults to ten minutes.
    pub fn sweep_every(mut self, sweep_every: Duration) -> Self {
        self.sweep_every = sweep_every;
        self
    }

    /// The clock that checks drop times, idle and Written, and that names
    /// temporary files. Defaults to the system clock.
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = Some(clock);
        self
    }

    /// Opens the store, making its folder and secret if needed. Clears the
    /// expired files and, when over the limit, those with the oldest
    /// Renewed. Safe to call inside `OnceLock::get_or_init`: it blocks on
    /// nothing another store holds.
    ///
    /// When another store, in this process or another, has the folder open,
    /// the store is [refused](FileStore::is_refused): it logs a warning,
    /// touches no file, and carries on without the disk.
    ///
    /// # Errors
    ///
    /// When the store lacks the permissions it needs on the folder, to make
    /// it or to read, write and remove files in it, with an error that names
    /// the folder. On Unix, also when the folder belongs to another user or
    /// other users can write to it. A secret that is missing or not 32 bytes
    /// long is not an error: the store makes a new one.
    pub fn open(self) -> io::Result<FileStore> {
        let dir = self.dir;
        let named = |error: io::Error| {
            io::Error::new(
                error.kind(),
                format!(
                    "The disk cache cannot use the folder {}: {error}",
                    dir.display()
                ),
            )
        };
        make_folder(&dir).map_err(named)?;
        check_private(&dir).map_err(named)?;
        let folder = fs::canonicalize(&dir).map_err(named)?;
        let clock = self
            .clock
            .unwrap_or_else(|| Arc::new(crate::loading::SystemClock));
        let mut store = FileStore {
            root: dir.clone(),
            secret: [0; SECRET_LEN],
            max_bytes: 0,
            time_to_idle: self.time_to_idle,
            sweep_every: self.sweep_every,
            clock,
            held: AtomicU64::new(0),
            maintaining: Mutex::new(()),
            next_sweep: AtomicU64::new(0),
            writing: std::array::from_fn(|_| Mutex::new(())),
            hold: None,
        };
        let refused = || {
            log::warn!(
                "The disk cache folder {} is open in another store, so this store carries \
                 on without the disk. Each store needs a folder of its own.",
                dir.display()
            );
        };

        // Another store of this process, which its lock may not stop.
        {
            let mut open = open_folders();
            if open.contains(&folder) {
                refused();
                return Ok(store);
            }
            open.push(folder.clone());
        }
        let mut hold = Hold {
            folder,
            secret: None,
        };

        // Another process, found by the lock on the secret file, which is
        // made empty when missing and filled once locked.
        let mut secret = open_secret(&dir.join(SECRET_FILE)).map_err(named)?;
        match secret.try_lock() {
            Ok(()) => {}
            Err(fs::TryLockError::WouldBlock) => {
                refused();
                return Ok(store);
            }
            // A file system with no locks. The list still keeps out the
            // stores of this process.
            Err(fs::TryLockError::Error(error)) => {
                log::warn!(
                    "The disk cache could not lock {} and cannot tell whether a store in \
                     another process has it open: {error}",
                    dir.display()
                );
            }
        }
        store.check_permissions().map_err(named)?;
        store.secret = store.load_secret(&mut secret).map_err(named)?;
        hold.secret = Some(secret);
        store.hold = Some(hold);

        // A new secret leaves every file under the old one unreachable, and
        // clearing removes those along with the expired.
        store.clear_expired();
        store.max_bytes = self
            .max_bytes
            .unwrap_or_else(|| default_max_bytes(&store.root, store.held_bytes()));
        store.trim();
        Ok(store)
    }
}

impl FileStore {
    /// Lists the folder root, and makes, writes and removes a file in it, so
    /// a store that lacks the permissions it needs fails when it opens
    /// rather than on every request. The file has a temporary name, so one
    /// left by a crash is cleared.
    fn check_permissions(&self) -> io::Result<()> {
        fs::read_dir(&self.root)?;
        let probe = self
            .root
            .join(self.temp_name(SECRET_FILE, self.clock.now()));
        let written = create_private(&probe).and_then(|mut file| file.write_all(b"probe"));
        let removed = fs::remove_file(&probe);
        written.and(removed)
    }

    /// The secret in `file`, made when it is missing, unreadable or not 32
    /// bytes long, and written back in place, since the store holds the
    /// file locked.
    ///
    /// # Errors
    ///
    /// When the store lacks the permission to read the secret.
    fn load_secret(&self, file: &mut File) -> io::Result<[u8; SECRET_LEN]> {
        let mut bytes = Vec::new();
        match file.read_to_end(&mut bytes) {
            Ok(_) => {
                if let Ok(secret) = <[u8; SECRET_LEN]>::try_from(bytes.as_slice()) {
                    return Ok(secret);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return Err(error),
            Err(error) => {
                log::warn!(
                    "The disk cache could not read its secret in {}, and makes a new one: {error}",
                    self.root.display()
                );
            }
        }
        let mut secret = [0u8; SECRET_LEN];
        if let Err(error) = getrandom::getrandom(&mut secret) {
            // Without randomness the names are still a hash of the key, so
            // the store works, and a sender could choose keys that crowd one
            // folder.
            log::warn!("The disk cache could not make a random secret: {error}");
        }
        let written = (|| {
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&secret)?;
            file.sync_all()
        })();
        if let Err(error) = written {
            // The next open finds a damaged secret and clears these files.
            log::warn!(
                "The disk cache could not keep its secret in {}, and keeps it in memory: {error}",
                self.root.display()
            );
        }
        Ok(secret)
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

/// Makes `folder` and the folders above it, readable only by the process's
/// user on Unix.
fn make_folder(folder: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(folder)
}

/// On Unix, refuses a folder that belongs to another user, or that other
/// users can write to. The folder may be one another user made first, in a
/// shared place such as /tmp, and whoever can write to it can read or set
/// the secret and plant entries.
///
/// On Windows nothing is checked, as before: who may use a folder there is
/// set by its access control list, which this does not read.
fn check_private(folder: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(folder)?;
        // SAFETY: geteuid takes nothing and cannot fail.
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "it belongs to another user",
            ));
        }
        if metadata.mode() & 0o022 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "other users can write to it",
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = folder;
    Ok(())
}

/// Opens the secret file at `path` to read and write, making it empty and
/// readable only by the process's user on Unix when it is missing.
fn open_secret(path: &Path) -> io::Result<File> {
    let mut options = File::options();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Creates a new file readable only by the process's user on Unix.
fn create_private(path: &Path) -> io::Result<File> {
    let mut options = File::options();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// The start of the file at `path`, as far as the end of the longest entry
/// header its key allows, without the value.
fn read_start(path: &Path) -> io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut prefix = [0u8; PREFIX_LEN];
    let read = read_up_to(&mut file, &mut prefix)?;
    let mut bytes = prefix[..read].to_vec();
    if read == PREFIX_LEN && prefix[..4] == *MARKER {
        let key_len = u32::from_le_bytes([prefix[5], prefix[6], prefix[7], prefix[8]]) as usize;
        let mut rest = Vec::new();
        file.take((key_len + MOST_ENTRY_HEADER_LEN) as u64)
            .read_to_end(&mut rest)?;
        bytes.extend_from_slice(&rest);
    }
    Ok(bytes)
}

/// Reads until `buffer` is full or the file ends, and returns the bytes read.
fn read_up_to(file: &mut File, buffer: &mut [u8]) -> io::Result<usize> {
    let mut read = 0;
    while read < buffer.len() {
        match file.read(&mut buffer[read..])? {
            0 => break,
            more => read += more,
        }
    }
    Ok(read)
}

/// What `bytes`, a whole file or the start of one, are.
fn contents(bytes: &[u8]) -> Contents<'_> {
    if bytes.get(..4) != Some(&MARKER[..]) {
        return Contents::Foreign;
    }
    let read = || {
        if *bytes.get(4)? != FILE_FORMAT {
            return None;
        }
        let key_len = u32::from_le_bytes(bytes.get(5..9)?.try_into().ok()?) as usize;
        let key = bytes.get(PREFIX_LEN..PREFIX_LEN.checked_add(key_len)?)?;
        let entry = &bytes[PREFIX_LEN + key_len..];
        let (header, _) = read_header(entry).ok()?;
        Some(Contents::Entry { key, header, entry })
    };
    read().unwrap_or(Contents::Unreadable)
}

/// A file's bytes: the marker, the version, the key and the entry.
fn compose(key: &str, key_len: u32, entry: &[u8]) -> Vec<u8> {
    let mut file = Vec::with_capacity(PREFIX_LEN + key.len() + entry.len());
    file.extend_from_slice(MARKER);
    file.push(FILE_FORMAT);
    file.extend_from_slice(&key_len.to_le_bytes());
    file.extend_from_slice(key.as_bytes());
    file.extend_from_slice(entry);
    file
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned()
}

fn is_lower_hex(text: &str) -> bool {
    text.bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_folder_name(name: &str) -> bool {
    name.len() == 2 && is_lower_hex(name)
}

fn is_entry_name(name: &str) -> bool {
    name.len() == NAME_LEN && is_lower_hex(name)
}

/// True when the file named `name` sits in the two folders its name gives.
fn in_its_folders(path: &Path, name: &str) -> bool {
    let inner = path.parent();
    let outer = inner.and_then(Path::parent);
    inner.map(file_name).as_deref() == Some(&name[2..4])
        && outer.map(file_name).as_deref() == Some(&name[..2])
}

/// The time in a temporary file's name, `<stem>.<nanoseconds>-<process>-
/// <number>.tmp`, when the name has that form and `stem` passes `is_stem`.
fn temp_time(name: &str, is_stem: impl Fn(&str) -> bool) -> Option<SystemTime> {
    let rest = name.strip_suffix(TEMP_SUFFIX)?;
    let (stem, numbers) = rest.rsplit_once('.')?;
    if !is_stem(stem) {
        return None;
    }
    let mut parts = numbers.split('-');
    let at = parts.next()?.parse::<u64>().ok()?;
    let all_digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if !(parts.next().is_some_and(all_digits)
        && parts.next().is_some_and(all_digits)
        && parts.next().is_none())
    {
        return None;
    }
    Some(UNIX_EPOCH + Duration::from_nanos(at))
}

/// True when a temporary file named at `at` belongs to a write that never
/// finished: its time is further than [`ABANDONED_AFTER`] from now, in
/// either direction.
fn abandoned(at: SystemTime, now: SystemTime) -> bool {
    match now.duration_since(at) {
        Ok(age) => age >= ABANDONED_AFTER,
        Err(ahead) => ahead.duration() >= ABANDONED_AFTER,
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[usize::from(byte >> 4)] as char);
        text.push(DIGITS[usize::from(byte & 0xf)] as char);
    }
    text
}

fn nanos(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loading::{encode_entry, Entry, Raw};

    /// The result of a future the store gives, which never waits.
    fn now_or_never<T>(future: impl std::future::Future<Output = T>) -> T {
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        match std::pin::pin!(future).poll(&mut context) {
            std::task::Poll::Ready(value) => value,
            std::task::Poll::Pending => panic!("the store waited"),
        }
    }

    fn entry(at: SystemTime, value: &[u8]) -> Vec<u8> {
        let entry = Entry {
            value: value.to_vec(),
            written: at,
            expires: None,
            renewed: at,
        };
        encode_entry(&Raw, &entry, None).expect("encodes")
    }

    #[test]
    fn a_file_reads_back_as_written() {
        let at = UNIX_EPOCH + Duration::from_secs(1_000);
        let entry = entry(at, b"value");
        let file = compose("a/key", 5, &entry);
        match contents(&file) {
            Contents::Entry {
                key,
                header,
                entry: read,
            } => {
                assert_eq!(key, b"a/key");
                assert_eq!(header.written, at);
                assert_eq!(header.renewed, at);
                assert_eq!(read, &entry[..]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn renewed_sits_where_the_specification_puts_it() {
        let at = UNIX_EPOCH + Duration::from_secs(1_000);
        let file = compose("key", 3, &entry(at, b"value"));
        let offset = PREFIX_LEN + 3 + RENEWED_IN_ENTRY;
        assert_eq!(offset, 19 + 3);
        assert_eq!(&file[offset..offset + 8], &nanos(at).to_le_bytes());
    }

    #[test]
    fn bytes_are_foreign_unreadable_or_an_entry() {
        assert_eq!(contents(b""), Contents::Foreign);
        assert_eq!(contents(b"hello world"), Contents::Foreign);
        assert_eq!(contents(b"51DF"), Contents::Unreadable);
        assert_eq!(contents(b"51DF\x02\0\0\0\0"), Contents::Unreadable);
        let mut file = compose("key", 3, &entry(UNIX_EPOCH, b"value"));
        file.truncate(PREFIX_LEN + 3 + 5);
        assert_eq!(contents(&file), Contents::Unreadable);
    }

    #[test]
    fn temporary_names_carry_their_time() {
        let name = format!("{}.123-45-6.tmp", "a".repeat(64));
        assert_eq!(
            temp_time(&name, is_entry_name),
            Some(UNIX_EPOCH + Duration::from_nanos(123))
        );
        assert_eq!(temp_time("notes.tmp", is_entry_name), None);
        assert_eq!(
            temp_time(&format!("{}.1-2.tmp", "a".repeat(64)), is_entry_name),
            None
        );
        assert_eq!(
            temp_time("cache.secret.9-1-0.tmp", |stem| stem == SECRET_FILE),
            Some(UNIX_EPOCH + Duration::from_nanos(9))
        );
    }

    #[test]
    fn the_name_is_the_hmac_of_the_key() {
        // RFC 4231 test case 2, with its key padded to the 32 bytes of a
        // secret, which HMAC pads with zeros in the same way.
        let mut secret = [0u8; SECRET_LEN];
        secret[..4].copy_from_slice(b"Jefe");
        let mut mac = Hmac::<Sha256>::new_from_slice(&secret).expect("key");
        mac.update(b"what do ya want for nothing?");
        assert_eq!(
            hex(&mac.finalize().into_bytes()),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    /// A read that found the older value renews it only after a newer write
    /// of the same entry has landed. The newer value stays.
    #[test]
    fn a_renewal_never_puts_an_older_value_over_a_newer_write() {
        let dir =
            std::env::temp_dir().join(format!("fiftyone-caching-renewal-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = FileStore::builder(&dir)
            .time_to_idle(Duration::from_secs(100))
            .open()
            .expect("opens");
        let now = SystemTime::now();
        now_or_never(store.put("a", entry(now, b"older"), None));
        let path = store.path_for("a");
        let older = fs::read(&path).unwrap();
        now_or_never(store.put("a", entry(now, b"newer"), None));

        store.renew(&path, "a", &older, now + Duration::from_secs(20));
        let bytes = fs::read(&path).unwrap();
        let Contents::Entry { entry, .. } = contents(&bytes) else {
            panic!("an entry");
        };
        assert_eq!(read_header(entry).unwrap().1, b"newer");
        drop(store);
        let _ = fs::remove_dir_all(&dir);
    }

    /// The test hook for a name collision: one key's file at another key's
    /// name answers absent, never the other key's value.
    #[test]
    fn a_file_found_for_another_key_answers_absent() {
        let dir =
            std::env::temp_dir().join(format!("fiftyone-caching-collision-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = FileStore::open(&dir).expect("opens");
        let now = SystemTime::now();
        now_or_never(store.put("a", entry(now, b"value of a"), None));
        let a = store.path_for("a");
        let b = store.path_for("b");
        make_folder(b.parent().unwrap()).unwrap();
        fs::copy(&a, &b).unwrap();
        assert!(matches!(now_or_never(store.get("b")), ByteLookup::Miss));
        assert!(matches!(now_or_never(store.get("a")), ByteLookup::Hit(_)));
        // Clearing finds the copy is not at its key's name, and removes it.
        store.clear_expired();
        assert!(!b.exists());
        assert!(a.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
