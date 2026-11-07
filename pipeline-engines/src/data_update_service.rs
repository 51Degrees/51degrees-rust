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

//! The data update service.
//!
//! The [`DataUpdateService`] keeps the data files of on-premise engines current.
//! It implements the four modes from the
//! [data-updates specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/data-updates.md):
//!
//! - **Update on startup.** When a data file whose configuration sets
//!   `update_on_startup` is registered, the service polls its update URL once,
//!   synchronously, so the engine starts with current data.
//! - **Automatic update via HTTP.** A background scheduler thread polls each
//!   file's update URL on its configured interval (offset by a random amount to
//!   stagger many instances), sending an `If-Modified-Since` header so the
//!   server can answer `304 Not Modified` cheaply.
//! - **Automatic update from file.** A [`notify`] file-system watcher refreshes
//!   the engine when the data file changes on disk. Like polling, it runs only
//!   while automatic updates are on, which the specification makes the master
//!   switch for both.
//! - **Programmatic update.** [`DataUpdateService::check_for_update`] triggers an
//!   immediate check on demand.
//!
//! # Threading
//!
//! The remote poll uses a blocking [`reqwest`] client on the service's own
//! background thread, so no async runtime is required. Engine refreshes happen
//! on that background thread (or the watcher's thread), and the engine's
//! `refresh` is required to be thread-safe with respect to concurrent `process`
//! calls, so request handling continues with minimal impact during a refresh.
//!
//! # Downloads
//!
//! A download is written beside the data file as it arrives, with its MD5
//! taken as it is read, then decompressed into a second file there that
//! replaces the data file with a rename. The memory a download needs does not
//! grow with the size of the file, the HTTP client's timeout limits each wait
//! for more of the response rather than the whole download, and both files are
//! removed when an update fails part way.
//!
//! # Scope of this baseline
//!
//! The service downloads, verifies, decompresses and applies updates, and runs
//! the scheduler and watcher. Two behaviors are deliberate baseline limitations,
//! noted inline: the publish timestamp is taken as the apply time rather than
//! parsed out of the data-file header (the engine supplies the true value today),
//! and a `429` response surfaces the server's `Retry-After` in the error but the
//! next poll still uses the configured interval rather than that delay.

use std::collections::hash_map::RandomState;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, BufWriter, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use fiftyone_pipeline_core::redact::redact_with;
use md5::{Digest, Md5};

use crate::data_file::AspectEngineDataFile;
use crate::data_update_status::{AutoUpdateStatus, DataUpdateError, DataUpdateResult};
use crate::on_premise_aspect_engine::OnPremiseAspectEngine;

/// One registered data file together with the engine that owns it.
struct Registration {
    /// Held weakly, so dropping the engine frees its data set and ends the
    /// registration.
    engine: Weak<dyn OnPremiseAspectEngine>,
    data_file: Arc<AspectEngineDataFile>,
    /// The next time the scheduler should poll this file's update URL.
    next_poll: Mutex<Option<Instant>>,
    /// The file-system watcher kept alive for this file, if watching is on.
    watcher: Mutex<Option<Box<dyn notify::Watcher + Send>>>,
}

impl Registration {
    /// The engine, unless it has been dropped.
    fn engine(&self) -> Option<Arc<dyn OnPremiseAspectEngine>> {
        self.engine.upgrade()
    }
}

/// The shared inner state of the service, behind one lock plus a condition
/// variable the scheduler waits on.
struct Inner {
    registrations: Mutex<Vec<Arc<Registration>>>,
    /// Signalled to wake the scheduler when a registration is added or the
    /// service is shutting down.
    wake: Condvar,
    /// True once shutdown has been requested.
    shutting_down: Mutex<bool>,
}

impl Inner {
    /// The registrations whose engines are still alive, after removing the
    /// others. A removed registration is dropped after the lock is released,
    /// which stops its watcher.
    fn live_registrations(&self) -> Vec<Arc<Registration>> {
        let mut ended = Vec::new();
        let live = {
            let mut registrations = self.registrations.lock().unwrap_or_else(|e| e.into_inner());
            registrations.retain(|registration| {
                let alive = registration.engine.strong_count() > 0;
                if !alive {
                    ended.push(Arc::clone(registration));
                }
                alive
            });
            registrations.clone()
        };
        drop(ended);
        live
    }
}

/// Keeps on-premise engine data files current.
///
/// Construct one with [`DataUpdateService::new`], then register each data file
/// with [`DataUpdateService::register`]. The background scheduler starts on the
/// first registration. Dropping the service stops the scheduler and releases the
/// watchers.
///
/// The service holds each engine weakly, as a .NET engine unregisters its data
/// files when it is disposed. Dropping an engine, or the pipeline holding it,
/// frees its data set and ends its registration, so the caller keeps the
/// engine for as long as it is to be updated.
pub struct DataUpdateService {
    inner: Arc<Inner>,
    client: reqwest::blocking::Client,
    scheduler: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for DataUpdateService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataUpdateService")
            .field("registered", &self.registered_count())
            .finish_non_exhaustive()
    }
}

impl DataUpdateService {
    /// Create a new data update service with a default HTTP client.
    pub fn new() -> Self {
        let client = reqwest::blocking::Client::builder()
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new());
        DataUpdateService::with_client(client)
    }

    /// Create a service using a caller-supplied blocking HTTP client, for
    /// example one configured with a proxy or custom timeout.
    pub fn with_client(client: reqwest::blocking::Client) -> Self {
        DataUpdateService {
            inner: Arc::new(Inner {
                registrations: Mutex::new(Vec::new()),
                wake: Condvar::new(),
                shutting_down: Mutex::new(false),
            }),
            client,
            scheduler: Mutex::new(None),
        }
    }

    /// Register a data file for automatic updates on behalf of its engine.
    ///
    /// This applies the four modes described on the module:
    ///
    /// - If the configuration sets `update_on_startup`, a synchronous remote
    ///   poll runs now and its result is returned. A failed startup update is
    ///   returned as an error so the caller can decide whether to proceed.
    /// - If `automatic_updates_enabled` and `file_system_watcher_enabled` are
    ///   both on and the file has a path, a watcher is installed. Automatic
    ///   updates are the master switch for the watcher, as the specification
    ///   says.
    /// - The file is added to the scheduler's list so it is polled on its
    ///   interval (when `automatic_updates_enabled`).
    ///
    /// Returns the status of the startup poll, or [`AutoUpdateStatus::NotNeeded`]
    /// when no startup poll was requested. A failed startup poll leaves the
    /// engine on the data it loaded, as the specification says. It is logged
    /// and returned as an error, and the file is registered all the same, so
    /// the updates that follow still run.
    ///
    /// The registration lasts as long as the caller keeps `engine`.
    pub fn register(
        &self,
        engine: Arc<dyn OnPremiseAspectEngine>,
        data_file: Arc<AspectEngineDataFile>,
    ) -> DataUpdateResult {
        let config = data_file.configuration();
        let already_registered = data_file.is_registered();
        data_file.set_registered(true);
        self.inner.live_registrations();

        let registration = Arc::new(Registration {
            engine: Arc::downgrade(&engine),
            data_file: Arc::clone(&data_file),
            next_poll: Mutex::new(None),
            watcher: Mutex::new(None),
        });

        let mut startup = Ok(AutoUpdateStatus::NotNeeded);

        // Update on startup: poll once, synchronously. A failure leaves the
        // engine on the data it loaded, so the file is still registered for
        // the updates that follow.
        if config.update_on_startup && !already_registered {
            startup = self.check_for_update_from_url(&registration);
            if let Err(error) = &startup {
                log::warn!(
                    "An error occurred while updating the data file for {} on start-up. \
                     The data it has is used. Error detail: {error}",
                    engine.data_key()
                );
            }
        }

        // Install the file-system watcher if requested and automatic updates,
        // its master switch, are on.
        if config.automatic_updates_enabled
            && config.file_system_watcher_enabled
            && data_file.data_file_path().is_some()
        {
            self.install_watcher(&registration);
        }

        // Schedule the first poll if automatic updates are enabled.
        if config.automatic_updates_enabled {
            let due = self.initial_poll_due(&data_file);
            *registration
                .next_poll
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = Some(due);
        }

        {
            let mut regs = self
                .inner
                .registrations
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            regs.push(registration);
        }
        self.ensure_scheduler();
        self.inner.wake.notify_all();

        startup
    }

    /// Trigger an immediate update check for the engine's data file matching
    /// `data_file_identifier`.
    ///
    /// This is the programmatic-update entry point. If the file has an update
    /// URL the remote server is polled immediately; otherwise, if the file has
    /// a path on disk, the engine is refreshed from that file. A single-file
    /// engine ignores the identifier.
    pub fn check_for_update(
        &self,
        engine: &Arc<dyn OnPremiseAspectEngine>,
        data_file_identifier: Option<&str>,
    ) -> DataUpdateResult {
        let registration = self.find_registration(engine, data_file_identifier);
        let registration = match registration {
            Some(r) => r,
            None => {
                // Not registered: act directly on the engine's metadata.
                let data_file = engine.data_file_meta_data(data_file_identifier).cloned();
                match data_file {
                    Some(data_file) => Arc::new(Registration {
                        engine: Arc::downgrade(engine),
                        data_file,
                        next_poll: Mutex::new(None),
                        watcher: Mutex::new(None),
                    }),
                    None => return Ok(AutoUpdateStatus::NoConfiguration),
                }
            }
        };

        if registration.data_file.data_update_url().is_some() {
            self.check_for_update_from_url(&registration)
        } else if registration.data_file.data_file_path().is_some() {
            // No URL: refresh from the file on disk.
            Self::refresh_engine(engine, &registration)
        } else {
            Ok(AutoUpdateStatus::NoConfiguration)
        }
    }

    /// Apply an in-memory data update to the engine's matching data file.
    ///
    /// This is the memory data-source programmatic-update path. The bytes are
    /// handed straight to the engine's
    /// [`OnPremiseAspectEngine::refresh_from_memory`].
    pub fn update_from_memory(
        &self,
        engine: &Arc<dyn OnPremiseAspectEngine>,
        data_file_identifier: Option<&str>,
        data: &[u8],
    ) -> DataUpdateResult {
        engine
            .refresh_from_memory(data_file_identifier, data)
            .map(|_| AutoUpdateStatus::Success)
            .map_err(|e| DataUpdateError::new(AutoUpdateStatus::RefreshFailed, e.to_string()))
    }

    /// The number of data files currently registered, whose engines have
    /// not been dropped.
    pub fn registered_count(&self) -> usize {
        self.inner.live_registrations().len()
    }

    // ----- internal -----

    /// Decide when the first scheduled poll for a file is due.
    fn initial_poll_due(&self, data_file: &AspectEngineDataFile) -> Instant {
        let now = Utc::now();
        if let Some(available) = data_file.update_available_time() {
            if available > now {
                // Wait until the engine expects new data to be published, then
                // apply randomisation.
                let wait = (available - now)
                    .to_std()
                    .unwrap_or_else(|_| Duration::from_secs(0));
                return Instant::now()
                    + wait
                    + Self::randomisation(data_file.configuration().max_randomisation_seconds);
            }
        }
        Instant::now() + Self::poll_interval(data_file)
    }

    /// The polling interval for a file plus a random delay of up to its
    /// maximum randomisation, as each poll waits.
    fn poll_interval(data_file: &AspectEngineDataFile) -> Duration {
        let config = data_file.configuration();
        Duration::from_secs(config.polling_interval_seconds)
            + Self::randomisation(config.max_randomisation_seconds)
    }

    /// A random delay between zero and `max_seconds`, so instances that start
    /// together poll at different times.
    ///
    /// Each `RandomState` has its own keys, which the standard library draws
    /// from the operating system, so the delay differs between calls and
    /// between processes without a random number generator dependency.
    fn randomisation(max_seconds: u64) -> Duration {
        if max_seconds == 0 {
            return Duration::from_secs(0);
        }
        let mut hasher = std::hash::BuildHasher::build_hasher(&RandomState::new());
        std::time::SystemTime::now().hash(&mut hasher);
        Duration::from_secs(hasher.finish() % max_seconds)
    }

    /// Find the registration for an engine's data file, by identity and
    /// identifier.
    fn find_registration(
        &self,
        engine: &Arc<dyn OnPremiseAspectEngine>,
        data_file_identifier: Option<&str>,
    ) -> Option<Arc<Registration>> {
        self.inner.live_registrations().into_iter().find(|r| {
            std::ptr::addr_eq(r.engine.as_ptr(), Arc::as_ptr(engine))
                && match data_file_identifier {
                    Some(id) => r.data_file.identifier() == id,
                    None => true,
                }
        })
    }

    /// Install a file-system watcher that refreshes the engine when the data
    /// file changes on disk.
    ///
    /// The registration owns the watcher, so the watcher's callback holds the
    /// registration weakly. A strong reference would keep the registration,
    /// its engine and the watcher's thread alive after the service is dropped.
    fn install_watcher(&self, registration: &Arc<Registration>) {
        use notify::{RecursiveMode, Watcher};

        let path = match registration.data_file.data_file_path() {
            Some(p) => p.to_path_buf(),
            None => return,
        };
        let file_name = path.file_name().map(ToOwned::to_owned);

        // The file as it is now is the one the engine loaded, so only a later
        // change refreshes it.
        if registration.data_file.last_applied_modified().is_none() {
            if let Some(modified) = file_modified_time(&path) {
                registration.data_file.set_last_applied_modified(modified);
            }
        }

        let reg = Arc::downgrade(registration);
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else {
                return;
            };
            // The watch is on the folder, so skip events for other files in it.
            let names_the_file = event
                .paths
                .iter()
                .any(|changed| changed.file_name() == file_name.as_deref());
            if names_the_file && (event.kind.is_modify() || event.kind.is_create()) {
                if let Some(reg) = reg.upgrade() {
                    ServiceHandle::on_file_changed(&reg);
                }
            }
        });

        if let Ok(mut watcher) = watcher {
            // Watch the file's parent directory non-recursively, which is the
            // portable way to catch atomic replace-by-rename updates.
            let watch_target = path.parent().unwrap_or(&path).to_path_buf();
            if watcher
                .watch(&watch_target, RecursiveMode::NonRecursive)
                .is_ok()
            {
                *registration
                    .watcher
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(Box::new(watcher));
            }
        }
    }

    /// Poll the remote update URL for a registration and apply any update.
    fn check_for_update_from_url(&self, registration: &Arc<Registration>) -> DataUpdateResult {
        let handle = ServiceHandle {
            client: self.client.clone(),
            inner: Arc::clone(&self.inner),
        };
        handle.check_for_update_from_url(registration)
    }

    /// Refresh `engine` from its data file currently on disk.
    fn refresh_engine(
        engine: &Arc<dyn OnPremiseAspectEngine>,
        registration: &Registration,
    ) -> DataUpdateResult {
        engine
            .refresh(Some(registration.data_file.identifier()))
            .map(|_| AutoUpdateStatus::Success)
            .map_err(|e| DataUpdateError::new(AutoUpdateStatus::RefreshFailed, e.to_string()))
    }

    /// Start the background scheduler thread if it is not already running.
    fn ensure_scheduler(&self) {
        let mut scheduler = self.scheduler.lock().unwrap_or_else(|e| e.into_inner());
        if scheduler.is_some() {
            return;
        }
        let handle = ServiceHandle {
            client: self.client.clone(),
            inner: Arc::clone(&self.inner),
        };
        let join = std::thread::Builder::new()
            .name("fiftyone-data-update".to_owned())
            .spawn(move || handle.run_scheduler())
            .ok();
        *scheduler = join;
    }
}

impl Default for DataUpdateService {
    fn default() -> Self {
        DataUpdateService::new()
    }
}

impl Drop for DataUpdateService {
    fn drop(&mut self) {
        *self
            .inner
            .shutting_down
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = true;
        self.inner.wake.notify_all();
        if let Some(handle) = self
            .scheduler
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            // Best-effort join: the scheduler wakes on the condvar and exits.
            let _ = handle.join();
        }
    }
}

/// A cheap, cloneable handle to the parts of the service the background threads
/// need, so closures and the scheduler do not borrow the service itself.
#[derive(Clone)]
struct ServiceHandle {
    client: reqwest::blocking::Client,
    inner: Arc<Inner>,
}

impl ServiceHandle {
    /// The scheduler loop: sleep until the next poll is due, run due polls, and
    /// reschedule them. Exits when the service is dropped.
    fn run_scheduler(self) {
        loop {
            if *self
                .inner
                .shutting_down
                .lock()
                .unwrap_or_else(|e| e.into_inner())
            {
                return;
            }

            let wait = self.run_due_polls();

            // Wait until the next due time or until woken by a new
            // registration / shutdown.
            let guard = self
                .inner
                .shutting_down
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if *guard {
                return;
            }
            let _unused = self
                .inner
                .wake
                .wait_timeout(guard, wait)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Run every poll that is currently due and return how long to sleep before
    /// the next one.
    fn run_due_polls(&self) -> Duration {
        let registrations = self.inner.live_registrations();

        let now = Instant::now();
        let mut next_wait = Duration::from_secs(60 * 60);

        for registration in registrations {
            if !registration.data_file.automatic_updates_enabled() {
                continue;
            }
            let due = {
                *registration
                    .next_poll
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
            };
            let due = match due {
                Some(d) => d,
                None => continue,
            };

            if due <= now {
                // Poll now, keeping the engine for the poll. A failure is
                // logged, and the next interval retries.
                let Some(engine) = registration.engine() else {
                    continue;
                };
                let result = self.check_for_update_from_url(&registration);
                log_poll(engine.data_key(), &registration, result);
                let interval = DataUpdateService::poll_interval(&registration.data_file);
                let next = now + interval;
                *registration
                    .next_poll
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(next);
                next_wait = next_wait.min(interval);
            } else {
                next_wait = next_wait.min(due - now);
            }
        }

        next_wait.max(Duration::from_millis(50))
    }

    /// Handle a file-system-watcher event by refreshing the engine from disk if
    /// the file is genuinely newer than the one already applied.
    fn on_file_changed(registration: &Registration) {
        let path = match registration.data_file.data_file_path() {
            Some(p) => p.to_path_buf(),
            None => return,
        };
        let modified = match file_modified_time(&path) {
            Some(m) => m,
            None => return,
        };
        // Debounce: only act if the file is newer than the last one applied.
        if let Some(last) = registration.data_file.last_applied_modified() {
            if modified <= last {
                return;
            }
        }
        let Some(engine) = registration.engine() else {
            return;
        };
        registration.data_file.set_last_applied_modified(modified);
        let key = engine.data_key();
        match engine.refresh(Some(registration.data_file.identifier())) {
            Ok(()) => log::info!(
                "The data file for {key} was refreshed from '{}'.",
                path.display()
            ),
            Err(error) => log::warn!(
                "An error occurred while refreshing {key} from '{}'. Error detail: {error}",
                path.display()
            ),
        }
    }

    /// True once the service has been dropped.
    fn shutting_down(&self) -> bool {
        *self
            .inner
            .shutting_down
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// Poll the remote update URL, verify, decompress, write and refresh.
    ///
    /// Every failing exit comes through here, so no error leaves the service
    /// carrying the data file's licence keys, however the poll failed.
    fn check_for_update_from_url(&self, registration: &Arc<Registration>) -> DataUpdateResult {
        let keys = &registration
            .data_file
            .configuration()
            .data_update_license_keys;
        self.poll_update_url(registration).map_err(|mut error| {
            error.message = redact_with(&error.message, keys).into_owned();
            error
        })
    }

    /// Poll the remote update URL, verify, decompress, write and refresh.
    /// An engine that has been dropped needs no update.
    fn poll_update_url(&self, registration: &Arc<Registration>) -> DataUpdateResult {
        let data_file = &registration.data_file;
        let config = data_file.configuration();
        let url = match data_file.data_update_url() {
            Some(u) => u.to_owned(),
            None => return Ok(AutoUpdateStatus::NotNeeded),
        };
        let Some(engine) = registration.engine() else {
            return Ok(AutoUpdateStatus::NotNeeded);
        };

        // The formatter adds the Distributor's query parameters, the licence
        // keys among them, so the formatted URL is used for the request alone
        // and never written into an error.
        let request_url = data_file
            .formatted_update_url()
            .unwrap_or_else(|| url.clone());

        // Build the request, adding If-Modified-Since when configured. It is
        // the date the data was published, or the file's modified time when
        // the data holds no publish date, as the specification allows.
        let mut request = self.client.get(&request_url);
        if config.verify_modified_since {
            let since = data_file
                .data_published()
                .or_else(|| data_file.data_file_path().and_then(file_modified_time));
            if let Some(since) = since {
                request =
                    request.header(reqwest::header::IF_MODIFIED_SINCE, httpdate_format(since));
            }
        }

        let response = request.send().map_err(|e| {
            DataUpdateError::new(
                AutoUpdateStatus::HttpsError,
                format!(
                    "error accessing data update service at '{url}': {}",
                    request_error_message(e)
                ),
            )
        })?;

        let status = response.status();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(AutoUpdateStatus::NotNeeded);
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            // Surface the server's requested back-off in the error so an operator
            // can see how long it asked us to wait. The scheduler's next poll
            // still uses the configured interval; using this value as an explicit
            // next-poll delay would mean threading it through the scheduler, which
            // this baseline does not do.
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .map(|value| format!(" (Retry-After: {value})"))
                .unwrap_or_default();
            return Err(DataUpdateError::new(
                AutoUpdateStatus::TooManyRequests,
                format!("too many requests to '{url}'{retry_after}"),
            ));
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(DataUpdateError::new(
                AutoUpdateStatus::Forbidden,
                format!("access denied to data update service at '{url}'"),
            ));
        }
        if !status.is_success() {
            return Err(DataUpdateError::new(
                AutoUpdateStatus::HttpsError,
                format!("HTTP status '{status}' from data update service at '{url}'"),
            ));
        }

        // Capture the expected MD5 before consuming the body.
        let expected_md5 = response
            .headers()
            .get("content-md5")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        // The body is read as it arrives, so the client's timeout limits each
        // wait for more of it rather than the whole download, and its MD5 is
        // taken as it is read. Reading stops once the service is dropped, so
        // dropping it does not wait for a long download to finish.
        let stop = || self.shutting_down();
        let mut body = Md5Reader::new(Stoppable {
            inner: response,
            stop: &stop,
        });

        let Some(path) = data_file.data_file_path() else {
            // Memory data source: the engine takes the data as bytes.
            let mut compressed = Vec::new();
            body.read_to_end(&mut compressed)
                .map_err(|e| read_error(&url, e))?;
            verify_md5(config.verify_md5, expected_md5.as_deref(), &body.hex())?;
            let data = if config.decompress_content {
                gzip_decompress(&compressed).map_err(|e| decompress_error(&url, e))?
            } else {
                compressed
            };
            let recorded = data_file.published_changes();
            engine
                .refresh_from_memory(Some(data_file.identifier()), &data)
                .map_err(|e| {
                    DataUpdateError::new(AutoUpdateStatus::RefreshFailed, e.to_string())
                })?;
            Self::record_published(data_file, recorded);
            return Ok(AutoUpdateStatus::Success);
        };

        // File data source: the download, and the file it decompresses to, are
        // written beside the data file so the new file replaces it with a
        // rename. Each is removed when it goes out of scope, so an update that
        // fails part way leaves nothing behind.
        let download = TempFile::beside(path, "download");
        save_body(&mut body, download.path(), &url)?;
        verify_md5(config.verify_md5, expected_md5.as_deref(), &body.hex())?;
        let new_file = if config.decompress_content {
            let decompressed = TempFile::beside(path, "new");
            gunzip_file(download.path(), decompressed.path(), &stop)
                .map_err(|e| decompress_error(&url, e))?;
            drop(download);
            decompressed
        } else {
            download
        };

        // Record the new file's modified time before it replaces the data
        // file, which the rename keeps, so the watcher does not refresh the
        // engine a second time.
        if let Some(modified) = file_modified_time(new_file.path()) {
            data_file.set_last_applied_modified(modified);
        }
        std::fs::rename(new_file.path(), path).map_err(|e| {
            DataUpdateError::new(
                AutoUpdateStatus::NewFileCannotRename,
                format!("error replacing data file '{}': {e}", path.display()),
            )
        })?;
        let recorded = data_file.published_changes();
        engine
            .refresh(Some(data_file.identifier()))
            .map_err(|e| DataUpdateError::new(AutoUpdateStatus::RefreshFailed, e.to_string()))?;
        Self::record_published(data_file, recorded);

        Ok(AutoUpdateStatus::Success)
    }

    /// After an engine has taken a download, keep the publish date it read
    /// from the new data, which the next `If-Modified-Since` header carries.
    /// For an engine that records none since `recorded`, use the current time,
    /// so the next request is no earlier than this update.
    fn record_published(data_file: &AspectEngineDataFile, recorded: u64) {
        if data_file.published_changes() == recorded {
            data_file.set_data_published(Utc::now());
        }
    }
}

/// Log the outcome of a scheduled poll, with the messages the specification's
/// logging section gives. The URL logged is the configured one, which carries
/// no licence key.
fn log_poll(engine: &str, registration: &Registration, result: DataUpdateResult) {
    let url = registration.data_file.data_update_url().unwrap_or("");
    match result {
        Ok(AutoUpdateStatus::Success) => {
            log::info!("A data file update for {engine} was downloaded from '{url}' and applied.")
        }
        Ok(_) => {}
        Err(error) => log::warn!(
            "An error occurred while downloading a data file update for {engine} from '{url}'. \
             Update will be attempted again later. Error detail: {error}"
        ),
    }
}

/// The message of a failed request and each of its causes, without the URL
/// the request error names. That URL is the formatted one, whose query
/// carries the licence keys.
fn request_error_message(error: reqwest::Error) -> String {
    let error = error.without_url();
    let mut message = error.to_string();
    let mut cause = std::error::Error::source(&error);
    while let Some(source) = cause {
        message.push_str(": ");
        message.push_str(&source.to_string());
        cause = source.source();
    }
    message
}

/// Format a timestamp as an HTTP-date for the `If-Modified-Since` header.
fn httpdate_format(when: DateTime<Utc>) -> String {
    // RFC 7231 IMF-fixdate, for example "Sun, 06 Nov 1994 08:49:37 GMT".
    when.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// The last-modified time of a file as a UTC timestamp, if obtainable.
fn file_modified_time(path: &Path) -> Option<DateTime<Utc>> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    Some(DateTime::<Utc>::from(modified))
}

/// Decompress a gzip byte slice into the original bytes.
fn gzip_decompress(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut decoder = flate2::read::GzDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

/// The size of each read when a download is written to disk.
const COPY_BUFFER_BYTES: usize = 64 * 1024;

/// Reads through to another reader, taking the MD5 of every byte read.
struct Md5Reader<R> {
    inner: R,
    hasher: Md5,
}

impl<R> Md5Reader<R> {
    fn new(inner: R) -> Self {
        Md5Reader {
            inner,
            hasher: Md5::new(),
        }
    }

    /// The lowercase hex MD5 of the bytes read so far.
    fn hex(&self) -> String {
        self.hasher
            .clone()
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

impl<R: Read> Read for Md5Reader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..read]);
        Ok(read)
    }
}

/// Reads through to another reader, and fails once `stop` answers true, so a
/// download or decompression in progress ends when the service is dropped.
struct Stoppable<'a, R> {
    inner: R,
    stop: &'a dyn Fn() -> bool,
}

impl<R: Read> Read for Stoppable<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if (self.stop)() {
            return Err(std::io::Error::other("the data update service was dropped"));
        }
        self.inner.read(buffer)
    }
}

/// A file beside the data file that is removed when it is dropped, unless it
/// has been renamed away first.
struct TempFile {
    path: PathBuf,
}

impl TempFile {
    /// A new name beside `path`, which no other update in this process uses.
    fn beside(path: &Path, suffix: &str) -> TempFile {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut name = path
            .file_name()
            .map(std::ffi::OsString::from)
            .unwrap_or_default();
        name.push(format!(
            ".{}.{}.{suffix}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        TempFile {
            path: path.with_file_name(name),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        // A file renamed into place is no longer here, so this finds nothing.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Write everything `body` reads into a new file at `path`, flushed to disk.
/// A failed read names the configured URL, and a failed write names the file.
fn save_body(body: &mut impl Read, path: &Path, url: &str) -> Result<(), DataUpdateError> {
    let write_error = |error: std::io::Error| {
        DataUpdateError::new(
            AutoUpdateStatus::StreamError,
            format!("error writing data update to '{}': {error}", path.display()),
        )
    };
    let mut file = std::fs::File::create(path).map_err(write_error)?;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        let read = match body.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(read_error(url, error)),
        };
        file.write_all(&buffer[..read]).map_err(write_error)?;
    }
    file.sync_all().map_err(write_error)
}

/// Decompress the gzip file at `from` into a new file at `to`, flushed to disk.
fn gunzip_file(from: &Path, to: &Path, stop: &dyn Fn() -> bool) -> std::io::Result<()> {
    let mut decoder = flate2::read::GzDecoder::new(Stoppable {
        inner: BufReader::new(std::fs::File::open(from)?),
        stop,
    });
    let mut output = BufWriter::with_capacity(COPY_BUFFER_BYTES, std::fs::File::create(to)?);
    std::io::copy(&mut decoder, &mut output)?;
    output
        .into_inner()
        .map_err(|error| error.into_error())?
        .sync_all()
}

/// Check the MD5 of the download against the one the server sent, when the
/// check is on and the server sent one.
fn verify_md5(check: bool, expected: Option<&str>, actual: &str) -> Result<(), DataUpdateError> {
    match expected {
        Some(expected) if check && !actual.eq_ignore_ascii_case(expected) => {
            Err(DataUpdateError::new(
                AutoUpdateStatus::Md5ValidationFailed,
                format!(
                    "integrity check failed: server MD5 '{expected}' does not match \
                     downloaded '{actual}'"
                ),
            ))
        }
        _ => Ok(()),
    }
}

/// The error for a response body that could not be read.
fn read_error(url: &str, error: std::io::Error) -> DataUpdateError {
    DataUpdateError::new(
        AutoUpdateStatus::StreamError,
        format!(
            "error reading data update response from '{url}': {}",
            read_error_message(error)
        ),
    )
}

/// The error for a download that could not be decompressed.
fn decompress_error(url: &str, error: std::io::Error) -> DataUpdateError {
    DataUpdateError::new(
        AutoUpdateStatus::StreamError,
        format!("error decompressing data update from '{url}': {error}"),
    )
}

/// The message of an error reading a response. A request error inside it
/// names the formatted URL, so its message comes from
/// [`request_error_message`].
fn read_error_message(error: std::io::Error) -> String {
    if !error
        .get_ref()
        .is_some_and(|inner| inner.is::<reqwest::Error>())
    {
        return error.to_string();
    }
    match error
        .into_inner()
        .map(|inner| inner.downcast::<reqwest::Error>())
    {
        Some(Ok(request)) => request_error_message(*request),
        _ => String::from("the response could not be read"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_known_vector() {
        // MD5("abc") = 900150983cd24fb0d6963f7d28e17f72
        let mut reader = Md5Reader::new(&b"abc"[..]);
        std::io::copy(&mut reader, &mut std::io::sink()).unwrap();
        assert_eq!(reader.hex(), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn gzip_round_trips() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;

        let original = b"51Degrees data file contents";
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        let decompressed = gzip_decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn http_date_is_imf_fixdate() {
        let when = DateTime::parse_from_rfc3339("1994-11-06T08:49:37Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(httpdate_format(when), "Sun, 06 Nov 1994 08:49:37 GMT");
    }

    #[test]
    fn new_service_has_no_registrations() {
        let service = DataUpdateService::new();
        assert_eq!(service.registered_count(), 0);
    }

    /// An engine with one data file and no update URL, so a poll of it makes
    /// no request.
    struct IdleEngine {
        filter: fiftyone_pipeline_core::EvidenceKeyFilterWhitelist,
        data_files: Vec<Arc<AspectEngineDataFile>>,
    }

    impl fiftyone_pipeline_core::FlowElement for IdleEngine {
        fn process(
            &self,
            _data: &mut fiftyone_pipeline_core::FlowData,
        ) -> fiftyone_pipeline_core::Result<()> {
            Ok(())
        }
        fn data_key(&self) -> &str {
            "idle"
        }
        fn evidence_key_filter(&self) -> &dyn fiftyone_pipeline_core::EvidenceKeyFilter {
            &self.filter
        }
        fn properties(&self) -> &[fiftyone_pipeline_core::PropertyMetaData] {
            &[]
        }
    }

    impl crate::AspectEngine for IdleEngine {
        fn data_source_tier(&self) -> &str {
            "Lite"
        }
        fn aspect_properties(&self) -> &[crate::AspectPropertyMetaData] {
            &[]
        }
    }

    impl OnPremiseAspectEngine for IdleEngine {
        fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
            &self.data_files
        }
        fn refresh(&self, _identifier: Option<&str>) -> fiftyone_pipeline_core::Result<()> {
            Ok(())
        }
    }

    /// A data file with no update URL that polls every `interval_seconds`
    /// plus up to `max_randomisation_seconds`.
    fn polled_file(interval_seconds: u64, max_randomisation_seconds: u64) -> AspectEngineDataFile {
        AspectEngineDataFile::new(
            crate::DataFileConfiguration::memory_builder()
                .polling_interval_seconds(interval_seconds)
                .max_randomisation_seconds(max_randomisation_seconds)
                .build(),
        )
    }

    /// The delay before the first poll is drawn at random, so instances that
    /// start together poll at different times. 200 draws from 600 seconds
    /// give about 170 different delays.
    #[test]
    fn the_first_poll_is_delayed_at_random() {
        let service = DataUpdateService::new();
        let data_file = polled_file(0, 600);
        let delays: std::collections::BTreeSet<u64> = (0..200)
            .map(|_| {
                let start = Instant::now();
                service
                    .initial_poll_due(&data_file)
                    .duration_since(start)
                    .as_secs()
            })
            .collect();
        eprintln!("{} different delays in 200", delays.len());
        assert!(
            delays.len() > 100,
            "{} different delays in 200: {delays:?}",
            delays.len()
        );
    }

    /// Each poll after the first also waits the polling interval plus a
    /// random delay, as the specification says.
    #[test]
    fn each_later_poll_adds_a_random_delay() {
        const INTERVAL_SECONDS: u64 = 1000;
        let handle = ServiceHandle {
            client: reqwest::blocking::Client::new(),
            inner: Arc::new(Inner {
                registrations: Mutex::new(Vec::new()),
                wake: Condvar::new(),
                shutting_down: Mutex::new(false),
            }),
        };
        let due = Instant::now();
        let mut engines = Vec::new();
        for _ in 0..5 {
            let data_file = Arc::new(polled_file(INTERVAL_SECONDS, INTERVAL_SECONDS));
            let engine: Arc<dyn OnPremiseAspectEngine> = Arc::new(IdleEngine {
                filter: fiftyone_pipeline_core::EvidenceKeyFilterWhitelist::new(
                    Vec::<String>::new(),
                ),
                data_files: vec![Arc::clone(&data_file)],
            });
            handle
                .inner
                .registrations
                .lock()
                .unwrap()
                .push(Arc::new(Registration {
                    engine: Arc::downgrade(&engine),
                    data_file,
                    next_poll: Mutex::new(Some(due)),
                    watcher: Mutex::new(None),
                }));
            engines.push(engine);
        }

        handle.run_due_polls();

        // Without a delay every next poll is due at most one interval after
        // the polls ran. Five delays of under two seconds out of a thousand
        // happen once in about 3 * 10^13 runs.
        let undelayed = Instant::now() + Duration::from_secs(INTERVAL_SECONDS + 1);
        let delayed = handle
            .inner
            .registrations
            .lock()
            .unwrap()
            .iter()
            .filter(|registration| {
                registration
                    .next_poll
                    .lock()
                    .unwrap()
                    .is_some_and(|next| next > undelayed)
            })
            .count();
        assert!(delayed > 0, "no poll after the first added a delay");
    }
}
