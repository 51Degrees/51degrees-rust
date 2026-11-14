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
    /// Writing the download to a file, or decompressing it, failed.
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
            AutoUpdateStatus::UnknownError,
        ] {
            assert!(!status.is_ok(), "{status:?}");
        }
    }
}
