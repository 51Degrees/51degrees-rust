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

//! Status and error types for the data update service.
//!
//! [`AutoUpdateStatus`] reports the outcome of a single update check.
//! [`DataUpdateError`] is the error raised when a check fails, and it carries
//! the status so a caller can branch on the cause.

use thiserror::Error;

/// The outcome of a data-file update check.
///
/// A successful or "no update needed" outcome is not an error. The failure
/// outcomes accompany a [`DataUpdateError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AutoUpdateStatus {
    /// The update completed successfully and the engine was refreshed.
    Success,
    /// No update was needed because the local data is already current.
    NotNeeded,
    /// A file-system-watcher event waited while another event for the same
    /// file was applied, and then had nothing left to do.
    InProgress,
    /// The HTTPS request to the update URL failed (connection or transport
    /// error), or its response ended before the whole file arrived.
    HttpsError,
    /// The server responded `429 Too Many Requests`.
    TooManyRequests,
    /// The server responded `403 Forbidden` (for example, a revoked key).
    Forbidden,
    /// Writing the download to a file, or decompressing it, failed. The data
    /// update service gives such a failure as [`AutoUpdateStatus::UnknownError`],
    /// the status the specification's errors table and .NET give it, so it no
    /// longer gives this one.
    StreamError,
    /// The downloaded content failed MD5 verification.
    Md5ValidationFailed,
    /// The new data file could not replace the existing one on disk.
    NewFileCannotRename,
    /// Refreshing the engine with the new data failed.
    RefreshFailed,
    /// There is no data-file configuration matching the requested identifier.
    NoConfiguration,
    /// A temporary directory path was required but is not configured.
    TempPathNotSet,
    /// A data file the file system watcher saw change could not be opened
    /// for reading within ten minutes, so the engine was not refreshed.
    Timeout,
    /// An unanticipated error occurred during the check.
    UnknownError,
}

impl AutoUpdateStatus {
    /// True if the status represents a successful or no-op outcome rather than
    /// a failure.
    pub fn is_ok(&self) -> bool {
        matches!(
            self,
            AutoUpdateStatus::Success | AutoUpdateStatus::NotNeeded | AutoUpdateStatus::InProgress
        )
    }

    /// The name the specification's errors table and .NET give the status,
    /// such as `AUTO_UPDATE_ERR_403_FORBIDDEN`, so a message names it the
    /// same way in every language. A stream error is given as an unknown
    /// error, as the service reports it.
    pub fn name(&self) -> &'static str {
        match self {
            AutoUpdateStatus::Success => "AUTO_UPDATE_SUCCESS",
            AutoUpdateStatus::NotNeeded => "AUTO_UPDATE_NOT_NEEDED",
            AutoUpdateStatus::InProgress => "AUTO_UPDATE_IN_PROGRESS",
            AutoUpdateStatus::HttpsError => "AUTO_UPDATE_HTTPS_ERR",
            AutoUpdateStatus::TooManyRequests => "AUTO_UPDATE_ERR_429_TOO_MANY_ATTEMPTS",
            AutoUpdateStatus::Forbidden => "AUTO_UPDATE_ERR_403_FORBIDDEN",
            AutoUpdateStatus::StreamError => "AUTO_UPDATE_UNKNOWN_ERROR",
            AutoUpdateStatus::Md5ValidationFailed => "AUTO_UPDATE_ERR_MD5_VALIDATION_FAILED",
            AutoUpdateStatus::NewFileCannotRename => "AUTO_UPDATE_NEW_FILE_CANT_RENAME",
            AutoUpdateStatus::RefreshFailed => "AUTO_UPDATE_REFRESH_FAILED",
            AutoUpdateStatus::NoConfiguration => "AUTO_UPDATE_NO_CONFIGURATION",
            AutoUpdateStatus::TempPathNotSet => "AUTO_UPDATE_TEMP_PATH_NOT_SET",
            AutoUpdateStatus::Timeout => "AUTO_UPDATE_TIMEOUT",
            AutoUpdateStatus::UnknownError => "AUTO_UPDATE_UNKNOWN_ERROR",
        }
    }
}

/// An error raised while checking for or applying a data-file update.
///
/// The accompanying [`AutoUpdateStatus`] identifies the stage that failed.
#[derive(Debug, Error)]
#[error("data update failed ({status:?}): {message}")]
pub struct DataUpdateError {
    /// The status describing which stage of the update failed.
    pub status: AutoUpdateStatus,
    /// A human-readable description of the failure.
    pub message: String,
}

impl DataUpdateError {
    /// Create a new data-update error with the supplied status and message.
    pub fn new(status: AutoUpdateStatus, message: impl Into<String>) -> Self {
        DataUpdateError {
            status,
            message: message.into(),
        }
    }

    /// The message for an update on start-up of the data file `identifier`
    /// that ended with this error, being the errors table's "Update on
    /// startup failed for \<data file identifier\> with status \<status\>.
    /// See log for details", with this error's message after it.
    pub fn on_startup_message(&self, identifier: &str) -> String {
        format!(
            "Update on startup failed for {identifier} with status {}. See log for details. \
             Error detail: {}",
            self.status.name(),
            self.message
        )
    }
}

/// The result type used by the data update service.
pub type DataUpdateResult = std::result::Result<AutoUpdateStatus, DataUpdateError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// A check that applied an update or needed none is ok, as is a watcher
    /// event that had nothing left to do, and every failure is not.
    #[test]
    fn only_a_failure_is_not_ok() {
        for status in [
            AutoUpdateStatus::Success,
            AutoUpdateStatus::NotNeeded,
            AutoUpdateStatus::InProgress,
        ] {
            assert!(status.is_ok(), "{status:?}");
        }
        for status in [
            AutoUpdateStatus::HttpsError,
            AutoUpdateStatus::TooManyRequests,
            AutoUpdateStatus::Forbidden,
            AutoUpdateStatus::StreamError,
            AutoUpdateStatus::Md5ValidationFailed,
            AutoUpdateStatus::NewFileCannotRename,
            AutoUpdateStatus::RefreshFailed,
            AutoUpdateStatus::NoConfiguration,
            AutoUpdateStatus::TempPathNotSet,
            AutoUpdateStatus::Timeout,
            AutoUpdateStatus::UnknownError,
        ] {
            assert!(!status.is_ok(), "{status:?}");
        }
    }

    /// The update on start-up's message names the data file and the status as
    /// the errors table does, then the error's own message.
    #[test]
    fn the_start_up_message_names_the_file_and_the_status_as_the_table_does() {
        let error = DataUpdateError::new(AutoUpdateStatus::Forbidden, "denied");
        assert_eq!(
            error.on_startup_message("hash"),
            "Update on startup failed for hash with status \
             AUTO_UPDATE_ERR_403_FORBIDDEN. See log for details. Error detail: denied"
        );
    }
}
