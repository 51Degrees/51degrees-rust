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

//! The copy of a data file an engine reads while updates can replace the file.

use std::ffi::OsString;
use std::fs::File;
use std::io::{BufWriter, IntoInnerError, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use fiftyone_native::dd::data_file_header_size;
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_engines::DataFileConfiguration;

/// The size of the buffer a new copy is written through. On Linux and Android
/// `std::io::copy` hands a copy from one file to another to the kernel, with
/// `copy_file_range` or `sendfile`, so only other systems use the buffer.
const COPY_BUFFER_BYTES: usize = 1024 * 1024;

/// A copy of a data file that a data set is read from, removed when dropped.
///
/// Every profile but `InMemory` keeps the data file open while the data set
/// lives, and Windows refuses to replace a file that is open. With automatic
/// updates on, the engine therefore reads a copy in its temporary folder,
/// which the specification calls the operational data copy, and an update can
/// replace the data file itself. A builder can also ask for the copy under
/// every profile, as .NET's `createTempDataCopy` does, and for a copy already
/// in the folder to be read rather than a new one made, as .NET's
/// `SetReuseTempFile` does. Drop the copy after the data set read from it,
/// which closes it. A copy that was found is removed too, as the C library
/// removes the copy a data set read whether it made the copy or found it.
pub(crate) struct OperationalCopy {
    path: PathBuf,
}

impl OperationalCopy {
    /// A copy of `data_file` in `temp_dir`, or in the system's temporary
    /// folder when that is `None`, when `always` is true, or when `profile`
    /// keeps the data file open and automatic updates are on, otherwise
    /// `None`. With `reuse`, a copy already in the folder is taken when one
    /// is found (see [`Self::existing`]), and a new one is made otherwise.
    ///
    /// When the copy cannot be made the engine reads the data file itself, so
    /// a full temporary folder does not stop it from building, and on Windows
    /// an update then cannot replace the file while it is open.
    pub(crate) fn when_needed(
        data_file: &Path,
        temp_dir: Option<&Path>,
        profile: PerformanceProfile,
        config: &DataFileConfiguration,
        always: bool,
        reuse: bool,
    ) -> Option<OperationalCopy> {
        let for_updates =
            profile != PerformanceProfile::InMemory && config.automatic_updates_enabled;
        if !always && !for_updates {
            return None;
        }
        let temp_dir = temp_dir.map_or_else(std::env::temp_dir, Path::to_path_buf);
        if reuse {
            let found = OperationalCopy::existing(data_file, &temp_dir, data_file_header_size());
            if found.is_some() {
                return found;
            }
        }
        OperationalCopy::of(data_file, &temp_dir).ok()
    }

    /// A copy of `data_file` already in `temp_dir`, found as the C library's
    /// `FileGetExistingTempFile` finds one. It is the first file there whose
    /// name starts with the data file's name without its extension and is not
    /// the data file's name, whose length is the data file's, and whose first
    /// `compare` bytes are the data file's.
    fn existing(data_file: &Path, temp_dir: &Path, compare: usize) -> Option<OperationalCopy> {
        let name = data_file.file_name()?;
        let stem = data_file.file_stem()?.as_encoded_bytes();
        let length = std::fs::metadata(data_file).ok()?.len();
        let start = read_start(data_file, compare)?;
        std::fs::read_dir(temp_dir)
            .ok()?
            .flatten()
            .find_map(|entry| {
                let candidate = entry.file_name();
                if candidate.as_os_str() == name || !candidate.as_encoded_bytes().starts_with(stem)
                {
                    return None;
                }
                let path = entry.path();
                let metadata = std::fs::metadata(&path).ok()?;
                if !metadata.is_file() || metadata.len() != length {
                    return None;
                }
                if read_start(&path, compare)? != start {
                    return None;
                }
                Some(OperationalCopy { path })
            })
    }

    /// Copy `data_file` into `temp_dir`, under a name no other copy in this
    /// process has.
    ///
    /// The copy is written from its start, so it has the data file's length
    /// only once it is whole, and a search for a copy to reuse, which compares
    /// lengths first, never takes one still being written. On Windows
    /// `std::fs::copy` gives a copy its whole length before writing it.
    fn of(data_file: &Path, temp_dir: &Path) -> std::io::Result<OperationalCopy> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut name = data_file
            .file_name()
            .map(OsString::from)
            .unwrap_or_default();
        name.push(format!(
            ".{}.{}.operational",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // Made before the copy, so a copy that fails part way is removed.
        let copy = OperationalCopy {
            path: temp_dir.join(name),
        };
        let mut from = File::open(data_file)?;
        let mut to = BufWriter::with_capacity(COPY_BUFFER_BYTES, File::create(&copy.path)?);
        std::io::copy(&mut from, &mut to)?;
        to.into_inner().map_err(IntoInnerError::into_error)?;
        Ok(copy)
    }

    /// Where the copy is.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for OperationalCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The first `compare` bytes of the file at `path`, or all of a shorter file.
fn read_start(path: &Path, compare: usize) -> Option<Vec<u8>> {
    let mut start = Vec::with_capacity(compare);
    File::open(path)
        .ok()?
        .take(u64::try_from(compare).ok()?)
        .read_to_end(&mut start)
        .ok()?;
    Some(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How much of a file the search compares in these tests.
    const COMPARE: usize = 8;

    /// The bytes of the data file, longer than the part compared.
    const DATA: &[u8] = b"51DHEAD:the rest of the data file";

    /// A new, empty folder for `test` holding the data file `51Degrees.hash`,
    /// and its empty temporary folder.
    fn folder(test: &str) -> (PathBuf, PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("fiftyone-dd-reuse-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let temp = dir.join("temp");
        std::fs::create_dir_all(&temp).unwrap();
        let data_file = dir.join("51Degrees.hash");
        std::fs::write(&data_file, DATA).unwrap();
        (dir, data_file, temp)
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// `DATA` with the byte at `at` changed.
    fn changed_at(at: usize) -> Vec<u8> {
        let mut bytes = DATA.to_vec();
        bytes[at] ^= 0xFF;
        bytes
    }

    /// The search takes a file as the C library's does, leaves one it does
    /// not take where it is, and removes one it took once that is dropped.
    #[test]
    fn a_copy_is_found_as_the_c_library_finds_one() {
        let (dir, data_file, temp) = folder("found");
        let mut longer = DATA.to_vec();
        longer.push(0);
        let cases: [(&str, Vec<u8>, bool); 8] = [
            ("51Degrees.hash.1.2.operational", DATA.to_vec(), true),
            ("51Degrees-1234-abcdefgh", DATA.to_vec(), true),
            ("51Degrees.copy", changed_at(DATA.len() - 1), true),
            ("51Degrees.hash", DATA.to_vec(), false),
            ("Other.hash.1.2.operational", DATA.to_vec(), false),
            ("51Degrees.copy", changed_at(COMPARE - 1), false),
            ("51Degrees.copy", longer, false),
            ("51Degrees.copy", DATA[..DATA.len() - 1].to_vec(), false),
        ];
        for (name, bytes, taken) in cases {
            std::fs::write(temp.join(name), bytes).unwrap();
            let found = OperationalCopy::existing(&data_file, &temp, COMPARE);
            assert_eq!(
                found.as_ref().map(|copy| copy.path().to_path_buf()),
                taken.then(|| temp.join(name)),
                "{name}"
            );
            assert_eq!(files_in(&temp), [name], "{name}");
            drop(found);
            let _ = std::fs::remove_file(temp.join(name));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A folder named as a copy is never taken, and nor is anything when the
    /// temporary folder does not exist.
    #[test]
    fn only_a_file_is_found() {
        let (dir, data_file, temp) = folder("folders");
        std::fs::create_dir(temp.join("51Degrees.hash.1.2.operational")).unwrap();
        assert!(OperationalCopy::existing(&data_file, &temp, COMPARE).is_none());
        assert_eq!(files_in(&temp), ["51Degrees.hash.1.2.operational"]);
        let missing = dir.join("missing");
        assert!(OperationalCopy::existing(&data_file, &missing, COMPARE).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The watcher runs on its own setting, so with the watcher on the engine
    /// reads a copy even with automatic updates off, which lets a new file
    /// replace the data file while the data set is open. With both off it
    /// reads the data file itself, and `InMemory` holds no file open.
    #[test]
    fn the_watcher_on_its_own_asks_for_a_copy() {
        let (dir, data_file, temp) = folder("watcher");
        let copy = |updates: bool, watcher: bool, profile: PerformanceProfile| {
            let config = DataFileConfiguration::builder(data_file.clone())
                .automatic_updates_enabled(updates)
                .file_system_watcher_enabled(watcher)
                .build();
            OperationalCopy::when_needed(
                &data_file,
                Some(temp.as_path()),
                profile,
                &config,
                false,
                false,
            )
            .is_some()
        };
        assert!(
            copy(false, true, PerformanceProfile::Default),
            "no copy with the watcher on and automatic updates off"
        );
        assert!(copy(true, false, PerformanceProfile::Default));
        assert!(copy(true, true, PerformanceProfile::Default));
        assert!(!copy(false, false, PerformanceProfile::Default));
        assert!(!copy(false, true, PerformanceProfile::InMemory));
        assert_eq!(files_in(&temp), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With `reuse`, a copy already in the folder is read, and without it a
    /// new copy holding the data file is made beside it.
    #[test]
    fn reuse_takes_a_copy_already_in_the_folder() {
        let (dir, data_file, temp) = folder("reuse");
        let config = DataFileConfiguration::builder(data_file.clone())
            .automatic_updates_enabled(false)
            .file_system_watcher_enabled(false)
            .build();
        let left = temp.join("51Degrees-left.hash");
        std::fs::write(&left, DATA).unwrap();
        let when_needed = |reuse: bool| {
            OperationalCopy::when_needed(
                &data_file,
                Some(temp.as_path()),
                PerformanceProfile::Default,
                &config,
                true,
                reuse,
            )
            .expect("a copy")
        };

        let made = when_needed(false);
        assert_ne!(made.path(), left);
        assert_eq!(std::fs::read(made.path()).unwrap(), DATA);
        drop(made);
        assert_eq!(files_in(&temp), ["51Degrees-left.hash"]);

        let found = when_needed(true);
        assert_eq!(found.path(), left);
        drop(found);
        assert_eq!(files_in(&temp), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
