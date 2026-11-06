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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_engines::DataFileConfiguration;

/// A copy of a data file that a data set is read from, removed when dropped.
///
/// Every profile but `InMemory` keeps the data file open while the data set
/// lives, and Windows refuses to replace a file that is open. With automatic
/// updates on, the engine therefore reads a copy in its temporary folder,
/// which the specification calls the operational data copy, and an update can
/// replace the data file itself. A builder can also ask for the copy under
/// every profile, as .NET's `createTempDataCopy` does. Drop the copy after
/// the data set read from it, which closes it.
pub(crate) struct OperationalCopy {
    path: PathBuf,
}

impl OperationalCopy {
    /// A copy of `data_file` in `temp_dir`, or in the system's temporary
    /// folder when that is `None`, when `always` is true, or when `profile`
    /// keeps the data file open and automatic updates are on, otherwise
    /// `None`.
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
    ) -> Option<OperationalCopy> {
        let for_updates =
            profile != PerformanceProfile::InMemory && config.automatic_updates_enabled;
        if !always && !for_updates {
            return None;
        }
        let temp_dir = temp_dir.map_or_else(std::env::temp_dir, Path::to_path_buf);
        OperationalCopy::of(data_file, &temp_dir).ok()
    }

    /// Copy `data_file` into `temp_dir`, under a name no other copy in this
    /// process has.
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
        std::fs::copy(data_file, &copy.path)?;
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
