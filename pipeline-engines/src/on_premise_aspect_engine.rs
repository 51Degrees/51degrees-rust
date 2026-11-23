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

//! The on-premise aspect engine trait.
//!
//! An on-premise engine reads its evidence using one or more local data files.
//! In addition to being an [`AspectEngine`] it can refresh its data on demand
//! (when an update is downloaded or a new file appears on disk), report when the
//! data it is using was published, and report its update URL. The
//! [`crate::DataUpdateService`] drives these methods on a background thread, so
//! they MUST be thread-safe, which is why the trait requires `Send + Sync`
//! (inherited from [`FlowElement`]).

use std::sync::Arc;

use chrono::{DateTime, Utc};
use fiftyone_pipeline_core::Result;

use crate::aspect_engine::AspectEngine;
use crate::data_file::AspectEngineDataFile;

/// An aspect engine backed by one or more local data files.
///
/// The refresh methods are called both by application code (a programmatic
/// update) and by the
/// [`crate::DataUpdateService`] from a background thread, so an implementation
/// MUST swap its data behind a synchronisation primitive such that concurrent
/// `process` calls keep working throughout, as the
/// [data-updates specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/data-updates.md)
/// requires.
pub trait OnPremiseAspectEngine: AspectEngine {
    /// The data files this engine uses, as shared run-time state.
    ///
    /// The [`crate::DataUpdateService`] registers each of these for automatic
    /// updates. Most engines have a single file.
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>];

    /// Reload the data for the file matching `data_file_identifier` from its
    /// path on disk.
    ///
    /// A `None` identifier refreshes every data file. A single-file engine
    /// ignores the identifier. This MUST be thread-safe with respect to
    /// concurrent `process` calls.
    fn refresh(&self, data_file_identifier: Option<&str>) -> Result<()>;

    /// Reload the data for the file matching `data_file_identifier` from the
    /// supplied in-memory bytes.
    ///
    /// Used for the memory-only update path and for engines that load entirely
    /// into memory. The default implementation returns a configuration error,
    /// so engines that support memory refresh override it.
    fn refresh_from_memory(&self, data_file_identifier: Option<&str>, _data: &[u8]) -> Result<()> {
        let _ = data_file_identifier;
        Err(fiftyone_pipeline_core::Error::configuration(format!(
            "Engine '{}' does not support refreshing from an in-memory data \
             source.",
            self.data_key()
        )))
    }

    /// When the data this engine is currently using was published, if known.
    ///
    /// The update service uses this for the `If-Modified-Since` header and to
    /// decide when to begin polling. The default reads it from the first
    /// registered data file's state.
    fn data_file_published(&self) -> Option<DateTime<Utc>> {
        self.data_files()
            .first()
            .and_then(|file| file.data_published())
    }

    /// The remote update URL for the file matching `data_file_identifier`, if
    /// one is configured.
    ///
    /// The default reads it from the matching (or first) data file's
    /// configuration.
    fn data_update_url(&self, data_file_identifier: Option<&str>) -> Option<String> {
        self.data_file_meta_data(data_file_identifier)
            .and_then(|file| file.data_update_url().map(str::to_owned))
    }

    /// The directory this engine uses for temporary copies of its data files,
    /// if any. The data update service does not read it, as it writes each
    /// download beside the data file, so that the new file replaces the old
    /// one with a rename.
    fn temp_data_dir(&self) -> Option<&std::path::Path> {
        None
    }

    /// Get the run-time state for a specific data file.
    ///
    /// Returns the single file for a single-file engine (ignoring the
    /// identifier), the file whose identifier matches for a multi-file engine,
    /// or `None` if the engine has no data files. The default implementation is
    /// correct for every engine.
    fn data_file_meta_data(
        &self,
        data_file_identifier: Option<&str>,
    ) -> Option<&Arc<AspectEngineDataFile>> {
        let files = self.data_files();
        match files.len() {
            0 => None,
            1 => files.first(),
            _ => match data_file_identifier {
                Some(id) => files.iter().find(|f| f.identifier() == id),
                None => files.first(),
            },
        }
    }
}
