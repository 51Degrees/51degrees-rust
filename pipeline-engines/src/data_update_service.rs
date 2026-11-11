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
//!   server can answer `304 Not Modified` cheaply. The first poll, and the
//!   first after a scheduled poll or [`DataUpdateService::check_for_update`]
//!   applies an update, waits instead for the next update date the data gives
//!   while that date is ahead.
//! - **Automatic update from file.** A [`notify`] file-system watcher refreshes
//!   the engine when the data file changes on disk. Like polling, it runs only
//!   while automatic updates are on, which the specification makes the master
//!   switch for both.
//! - **Programmatic update.** [`DataUpdateService::check_for_update`] triggers an
//!   immediate check on demand.
//!
//! # Reporting
//!
//! Each check starts by telling the callbacks added with
//! [`DataUpdateService::on_update_started`], and ends by telling those added
//! with [`DataUpdateService::on_update_complete`] how it ended, as .NET's
//! `CheckForUpdateStarted` and `CheckForUpdateComplete` events do. A refresh
//! that fails or panics is logged at error level with the message the
//! specification's errors table gives and reported as
//! [`AutoUpdateStatus::RefreshFailed`], never as success. The engine keeps
//! the data it had, and polling and watching go on.
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

use std::any::Any;
use std::collections::hash_map::RandomState;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, BufWriter, ErrorKind, Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
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

/// How a check for an update of one data file ended, as the callbacks added
/// with [`DataUpdateService::on_update_complete`] are told.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DataUpdateComplete {
    /// The data file checked.
    pub data_file: Arc<AspectEngineDataFile>,
    /// How the check ended. [`AutoUpdateStatus::is_ok`] is false for a
    /// check that failed.
    pub status: AutoUpdateStatus,
}

/// The data file a check for an update is starting on, as the callbacks
/// added with [`DataUpdateService::on_update_started`] are told.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DataUpdateStarted {
    /// The data file being checked.
    pub data_file: Arc<AspectEngineDataFile>,
}

/// A callback told how each check ends.
type UpdateCallback = Arc<dyn Fn(&DataUpdateComplete) + Send + Sync>;

/// A callback told when each check starts.
type StartCallback = Arc<dyn Fn(&DataUpdateStarted) + Send + Sync>;

/// The callbacks the service reports to. The file-system watchers share
/// them, as a watcher holds no reference to the service.
#[derive(Default)]
struct UpdateCallbacks {
    started: Mutex<Vec<StartCallback>>,
    complete: Mutex<Vec<UpdateCallback>>,
}

impl UpdateCallbacks {
    fn add(&self, callback: UpdateCallback) {
        self.complete
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(callback);
    }

    fn add_started(&self, callback: StartCallback) {
        self.started
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(callback);
    }

    /// Start a check for `engine` on `data_file` by telling every start
    /// callback, in the order they were added. A callback that panics is
    /// logged and the others are still told.
    fn start(&self, engine: &str, data_file: Option<&Arc<AspectEngineDataFile>>) {
        let Some(data_file) = data_file else {
            return;
        };
        let callbacks = self
            .started
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if callbacks.is_empty() {
            return;
        }
        let report = DataUpdateStarted {
            data_file: Arc::clone(data_file),
        };
        for callback in callbacks {
            if let Err(panic) = catch_unwind(AssertUnwindSafe(|| callback(&report))) {
                log_callback_panic(engine, panic.as_ref());
            }
        }
    }

    /// End a check for `engine` that gave `result`. A refresh that failed is
    /// logged, then every callback is told, in the order they were added,
    /// when the check had a data file. A callback that panics is logged and
    /// the others are still told.
    fn finish(
        &self,
        engine: &str,
        data_file: Option<&Arc<AspectEngineDataFile>>,
        result: &DataUpdateResult,
    ) {
        if let Err(error) = result {
            if error.status == AutoUpdateStatus::RefreshFailed {
                log_refresh_failed(engine, error);
            }
        }
        let Some(data_file) = data_file else {
            return;
        };
        let callbacks = self
            .complete
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if callbacks.is_empty() {
            return;
        }
        let report = DataUpdateComplete {
            data_file: Arc::clone(data_file),
            status: match result {
                Ok(status) => *status,
                Err(error) => error.status,
            },
        };
        for callback in callbacks {
            if let Err(panic) = catch_unwind(AssertUnwindSafe(|| callback(&report))) {
                log_callback_panic(engine, panic.as_ref());
            }
        }
    }
}

/// Log an update callback for `engine` that panicked.
fn log_callback_panic(engine: &str, panic: &(dyn Any + Send)) {
    log::error!(
        "An update callback for {engine} panicked. The data update service carries on. \
         Error detail: {}",
        panic_message(panic)
    );
}

/// Run an engine's refresh, giving a refresh that fails or panics as
/// [`AutoUpdateStatus::RefreshFailed`], so a panic in a refresh is reported
/// like a failure and never ends the thread that made the check.
fn guarded_refresh(
    refresh: impl FnOnce() -> fiftyone_pipeline_core::Result<()>,
) -> Result<(), DataUpdateError> {
    match catch_unwind(AssertUnwindSafe(refresh)) {
        Ok(result) => {
            result.map_err(|e| DataUpdateError::new(AutoUpdateStatus::RefreshFailed, e.to_string()))
        }
        Err(panic) => Err(DataUpdateError::new(
            AutoUpdateStatus::RefreshFailed,
            format!("the refresh panicked: {}", panic_message(panic.as_ref())),
        )),
    }
}

/// The message a panic carries, when it is text.
fn panic_message(panic: &(dyn Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("a panic with no message")
}

/// What the scheduler waits on, under the lock its condition variable uses.
#[derive(Default)]
struct SchedulerState {
    /// True once shutdown has been requested.
    shutting_down: bool,
    /// True when a registration was added or a scheduled poll moved since
    /// the scheduler's round began. A wake sent while a round runs finds no
    /// thread waiting, so this keeps it for the wait that follows the round.
    woken: bool,
}

/// The shared inner state of the service, behind one lock plus a condition
/// variable the scheduler waits on.
struct Inner {
    registrations: Mutex<Vec<Arc<Registration>>>,
    /// Signalled to wake the scheduler when a registration is added, a
    /// scheduled poll moves, or the service is shutting down.
    wake: Condvar,
    /// Whether the scheduler has been woken or asked to shut down.
    state: Mutex<SchedulerState>,
    callbacks: Arc<UpdateCallbacks>,
}

impl Inner {
    /// No registrations, and a scheduler neither woken nor shutting down.
    fn new() -> Inner {
        Inner {
            registrations: Mutex::new(Vec::new()),
            wake: Condvar::new(),
            state: Mutex::new(SchedulerState::default()),
            callbacks: Arc::default(),
        }
    }

    /// Wake the scheduler, at once if it is waiting, or as soon as its round
    /// ends if it is running one.
    fn wake_scheduler(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).woken = true;
        self.wake.notify_all();
    }

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
            inner: Arc::new(Inner::new()),
            client,
            scheduler: Mutex::new(None),
        }
    }

    /// Add a callback to be told how each check for an update ends.
    ///
    /// Every callback is told once for each check, being the update on
    /// start-up, each scheduled poll, each change to a watched file, and each
    /// call to [`Self::check_for_update`] or [`Self::update_from_memory`] for
    /// a data file the engine has. A change that leaves the watched file no
    /// newer than the one last applied is reported as
    /// [`AutoUpdateStatus::NotNeeded`]. A refresh that fails or panics is
    /// reported as [`AutoUpdateStatus::RefreshFailed`] and the engine keeps
    /// the data it had. Add callbacks before registering, so they hear the
    /// update on start-up.
    ///
    /// A callback runs on the thread that made the check, which is the
    /// service's own thread for a scheduled poll and the watcher's for a
    /// file change, so one that blocks delays the checks after it. One that
    /// panics is logged and the others are still told.
    pub fn on_update_complete(
        &self,
        callback: impl Fn(&DataUpdateComplete) + Send + Sync + 'static,
    ) {
        self.inner.callbacks.add(Arc::new(callback));
    }

    /// Add a callback to be told when each check for an update starts.
    ///
    /// Every callback is told once at the start of each check that
    /// [`Self::on_update_complete`] reports, before those callbacks are told
    /// how it ended, as .NET's `CheckForUpdateStarted` event is raised. It
    /// runs on the thread that makes the check, as those callbacks do, and
    /// one that panics is logged and the others are still told.
    pub fn on_update_started(&self, callback: impl Fn(&DataUpdateStarted) + Send + Sync + 'static) {
        self.inner.callbacks.add_started(Arc::new(callback));
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
    /// The registration lasts as long as the caller keeps `engine`. Given the
    /// only reference, the engine is dropped as this returns, so nothing is
    /// registered, polled, watched or scheduled. That is logged as a warning
    /// and [`AutoUpdateStatus::NotNeeded`] is returned.
    pub fn register(
        &self,
        engine: Arc<dyn OnPremiseAspectEngine>,
        data_file: Arc<AspectEngineDataFile>,
    ) -> DataUpdateResult {
        if Arc::strong_count(&engine) == 1 {
            log::warn!(
                "The data file for {} will not be updated, because the data update \
                 service was given the only reference to its engine, which is dropped \
                 when registration ends. Keep a reference to the engine for as long as \
                 it is to be updated.",
                engine.data_key()
            );
            return Ok(AutoUpdateStatus::NotNeeded);
        }
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
            self.inner
                .callbacks
                .start(engine.data_key(), Some(&data_file));
            startup = self.check_for_update_from_url(&registration);
            if let Err(error) = &startup {
                if error.status != AutoUpdateStatus::RefreshFailed {
                    log::warn!(
                        "An error occurred while updating the data file for {} on start-up. \
                         The data it has is used. Error detail: {error}",
                        engine.data_key()
                    );
                }
            }
            self.inner
                .callbacks
                .finish(engine.data_key(), Some(&data_file), &startup);
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
            let due = Self::poll_due(&data_file);
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
        self.inner.wake_scheduler();

        startup
    }

    /// Trigger an immediate update check for the engine's data file matching
    /// `data_file_identifier`.
    ///
    /// This is the programmatic-update entry point. If the file has an update
    /// URL the remote server is polled immediately; otherwise, if the file has
    /// a path on disk, the engine is refreshed from that file. A single-file
    /// engine ignores the identifier.
    ///
    /// When the check applies new data to a registered file whose updates are
    /// scheduled, the next scheduled poll waits for the next update date the
    /// new data gives, as it does after a scheduled poll applies new data.
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

        self.inner
            .callbacks
            .start(engine.data_key(), Some(&registration.data_file));
        let result = if registration.data_file.data_update_url().is_some() {
            self.check_for_update_from_url(&registration)
        } else if registration.data_file.data_file_path().is_some() {
            // No URL: refresh from the file on disk.
            Self::refresh_engine(engine, &registration)
        } else {
            Ok(AutoUpdateStatus::NoConfiguration)
        };
        if matches!(result, Ok(AutoUpdateStatus::Success)) {
            self.reschedule_after_update(&registration);
        }
        self.inner
            .callbacks
            .finish(engine.data_key(), Some(&registration.data_file), &result);
        result
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
        let data_file = engine.data_file_meta_data(data_file_identifier);
        self.inner.callbacks.start(engine.data_key(), data_file);
        let result = guarded_refresh(|| engine.refresh_from_memory(data_file_identifier, data))
            .map(|()| AutoUpdateStatus::Success);
        self.inner
            .callbacks
            .finish(engine.data_key(), data_file, &result);
        result
    }

    /// The number of data files currently registered, whose engines have
    /// not been dropped.
    pub fn registered_count(&self) -> usize {
        self.inner.live_registrations().len()
    }

    // ----- internal -----

    /// When the next scheduled poll for a file is due, at registration and
    /// once an update has been applied. While the time the engine expects
    /// new data is still ahead, that time plus a random delay, otherwise one
    /// polling interval plus a random delay.
    fn poll_due(data_file: &AspectEngineDataFile) -> Instant {
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

    /// After an update has been applied, take the next scheduled poll from
    /// the dates the new data gives, and wake the scheduler so it sleeps
    /// until then. A file with no scheduled poll keeps none.
    fn reschedule_after_update(&self, registration: &Registration) {
        {
            let mut next_poll = registration
                .next_poll
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if next_poll.is_none() {
                return;
            }
            *next_poll = Some(Self::poll_due(&registration.data_file));
        }
        self.inner.wake_scheduler();
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
    /// The registration owns the watcher, so the callback holds the engine
    /// weakly and the data file, never the registration. A watcher is then
    /// only ever dropped by a thread that is not its own, which macOS needs,
    /// because dropping one waits for its callback to finish.
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

        let engine = Weak::clone(&registration.engine);
        let data_file = Arc::clone(&registration.data_file);
        let callbacks = Arc::clone(&self.inner.callbacks);
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
                ServiceHandle::on_file_changed(&engine, &data_file, &callbacks);
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
        guarded_refresh(|| engine.refresh(Some(registration.data_file.identifier())))
            .map(|()| AutoUpdateStatus::Success)
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
        self.inner
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .shutting_down = true;
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
    /// The scheduler loop, which runs the polls that are due, then sleeps
    /// until the next one is due or a registration or a moved poll wakes
    /// it. A wake that comes during a round starts the next round at once.
    /// Exits when the service is dropped.
    fn run_scheduler(self) {
        loop {
            {
                let mut state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
                if state.shutting_down {
                    return;
                }
                // The round reads every registration and poll afresh, so it
                // answers any wake sent before it begins.
                state.woken = false;
            }

            let wait = self.run_due_polls();

            let state = self.inner.state.lock().unwrap_or_else(|e| e.into_inner());
            let _unused = self
                .inner
                .wake
                .wait_timeout_while(state, wait, |state| !state.woken && !state.shutting_down)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    /// Run every poll that is currently due and return how long to sleep before
    /// the next one. Polls run one after another, so the time is read again
    /// for each file and after each poll. The earliest next poll is kept
    /// through the round and the sleep counted from the end of the round, so
    /// a slow poll does not delay a file seen before it.
    fn run_due_polls(&self) -> Duration {
        let registrations = self.inner.live_registrations();

        let mut earliest: Option<Instant> = None;

        for registration in registrations {
            let now = Instant::now();
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
                self.inner
                    .callbacks
                    .start(engine.data_key(), Some(&registration.data_file));
                let result = self.check_for_update_from_url(&registration);
                let applied = matches!(result, Ok(AutoUpdateStatus::Success));
                log_poll(engine.data_key(), &registration, &result);
                self.inner.callbacks.finish(
                    engine.data_key(),
                    Some(&registration.data_file),
                    &result,
                );
                // Once new data is in use, the next poll waits for the next
                // update date that data gives, as at registration. A poll
                // that applied nothing tries again one interval after it
                // ended.
                let next = if applied {
                    DataUpdateService::poll_due(&registration.data_file)
                } else {
                    Instant::now() + DataUpdateService::poll_interval(&registration.data_file)
                };
                *registration
                    .next_poll
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(next);
                earliest = Some(earliest.map_or(next, |e| e.min(next)));
            } else {
                earliest = Some(earliest.map_or(due, |e| e.min(due)));
            }
        }

        sleep_until(earliest, Instant::now())
    }

    /// Handle a file-system-watcher event, telling the callbacks when the
    /// check starts and how it ended, as .NET reports every event. The
    /// engine is refreshed from disk only when the file is newer than the one
    /// last applied, and an event that finds it no newer, as when one write
    /// raises several events, is reported as needing no update. A file whose
    /// refresh fails is not tried again, and a file written after it is.
    fn on_file_changed(
        engine: &Weak<dyn OnPremiseAspectEngine>,
        data_file: &Arc<AspectEngineDataFile>,
        callbacks: &UpdateCallbacks,
    ) {
        let Some(engine) = engine.upgrade() else {
            return;
        };
        let key = engine.data_key();
        callbacks.start(key, Some(data_file));
        let result = Self::refresh_from_watched_file(&engine, data_file);
        callbacks.finish(key, Some(data_file), &result);
    }

    /// Refresh `engine` from its watched data file when the file is newer
    /// than the one last applied. A file that is gone, or no newer, needs no
    /// update, as in .NET, where a missing file's write time is the earliest
    /// there is.
    fn refresh_from_watched_file(
        engine: &Arc<dyn OnPremiseAspectEngine>,
        data_file: &AspectEngineDataFile,
    ) -> DataUpdateResult {
        let Some(path) = data_file.data_file_path() else {
            return Ok(AutoUpdateStatus::NotNeeded);
        };
        let Some(modified) = file_modified_time(path) else {
            return Ok(AutoUpdateStatus::NotNeeded);
        };
        if matches!(data_file.last_applied_modified(), Some(last) if modified <= last) {
            return Ok(AutoUpdateStatus::NotNeeded);
        }
        data_file.set_last_applied_modified(modified);
        log::info!(
            "Attempting to refresh Engine '{}' with new data",
            engine.data_key()
        );
        let result = guarded_refresh(|| engine.refresh(Some(data_file.identifier())))
            .map(|()| AutoUpdateStatus::Success);
        if result.is_ok() {
            log::info!(
                "The data file for {} was refreshed from '{}'.",
                engine.data_key(),
                path.display()
            );
        }
        result
    }

    /// True once the service has been dropped.
    fn shutting_down(&self) -> bool {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .shutting_down
    }

    /// Poll the remote update URL, verify, decompress, write and refresh.
    ///
    /// Every failing exit comes through here, so no error leaves the service
    /// carrying the data file's licence keys in full, however the poll failed.
    /// A key the message names shows only its last 4 characters.
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

        let name = engine.data_key();
        // An error message is masked where the check ends, but a log entry
        // written here is not, so the URL these entries name is masked now.
        let logged_url = redact_with(&url, &config.data_update_license_keys);
        let response = request.send().map_err(|e| {
            // A request that never reached the service and one that reached it
            // and gave nothing back are the same failure here, where .NET tells
            // them apart, so the message is the one for the first.
            DataUpdateError::new(
                AutoUpdateStatus::HttpsError,
                format!(
                    "Error accessing data update service at '{url}' for engine \
                     '{name}'. Error detail: {}",
                    request_error_message(e)
                ),
            )
        })?;

        let status = response.status();
        if status == reqwest::StatusCode::NOT_MODIFIED {
            let since = data_file
                .data_published()
                .or_else(|| data_file.data_file_path().and_then(file_modified_time))
                .map(httpdate_format)
                .unwrap_or_else(|| "the data it has".to_owned());
            log::info!("No data newer than {since} found at '{logged_url}' for Engine '{name}'");
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
                format!("Too many requests to '{url}' for engine '{name}'{retry_after}"),
            ));
        }
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(DataUpdateError::new(
                AutoUpdateStatus::Forbidden,
                format!("Access denied to data update service at '{url}' for engine '{name}'"),
            ));
        }
        if !status.is_success() {
            return Err(DataUpdateError::new(
                AutoUpdateStatus::HttpsError,
                format!(
                    "HTTP status code '{}' from data update service at '{url}' for \
                     engine '{name}'",
                    status.as_u16()
                ),
            ));
        }

        // Capture the expected MD5 before consuming the body.
        let expected_md5 = response
            .headers()
            .get("content-md5")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if config.verify_md5 && expected_md5.is_none() {
            log::warn!(
                "No MD5 hash included in data update response for '{logged_url}'. \
                 Unable to verify data integrity"
            );
        }

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
            log::info!("Downloaded new data from '{logged_url}' for Engine '{name}'");
            verify_md5(
                config.verify_md5,
                expected_md5.as_deref(),
                &body.hex(),
                name,
            )?;
            let data = if config.decompress_content {
                gzip_decompress(&compressed).map_err(|e| decompress_error(&url, e))?
            } else {
                compressed
            };
            let recorded = data_file.published_changes();
            log::info!("Attempting to refresh Engine '{name}' with new data");
            guarded_refresh(|| engine.refresh_from_memory(Some(data_file.identifier()), &data))?;
            Self::record_published(data_file, recorded);
            return Ok(AutoUpdateStatus::Success);
        };

        // File data source: the download, and the file it decompresses to, are
        // written beside the data file so the new file replaces it with a
        // rename. Each is removed when it goes out of scope, so an update that
        // fails part way leaves nothing behind.
        let download = TempFile::beside(path, "download");
        save_body(&mut body, download.path(), &url)?;
        log::info!("Downloaded new data from '{logged_url}' for Engine '{name}'");
        verify_md5(
            config.verify_md5,
            expected_md5.as_deref(),
            &body.hex(),
            name,
        )?;
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
                format!(
                    "An error occurred when copying a data file to replace the \
                     existing one at '{}'. Error detail: {e}",
                    path.display()
                ),
            )
        })?;
        let recorded = data_file.published_changes();
        log::info!("Attempting to refresh Engine '{name}' with new data");
        guarded_refresh(|| engine.refresh(Some(data_file.identifier())))?;
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
/// logging section gives. The URL logged is the configured one, without the
/// keys the formatter adds, and with any licence key it carries itself masked
/// to its last 4 characters. A refresh that failed is logged where the check
/// ends.
fn log_poll(engine: &str, registration: &Registration, result: &DataUpdateResult) {
    let data_file = &registration.data_file;
    let url = redact_with(
        data_file.data_update_url().unwrap_or(""),
        &data_file.configuration().data_update_license_keys,
    );
    match result {
        Ok(AutoUpdateStatus::Success) => {
            log::info!("A data file update for {engine} was downloaded from '{url}' and applied.")
        }
        Ok(_) => {}
        // A refresh that failed is logged where the check ends.
        Err(error) if error.status == AutoUpdateStatus::RefreshFailed => {}
        // Every other error already carries the message the errors table
        // gives, written where the failure was found and its detail was to
        // hand. Every row of that table is at Error apart from the two
        // warnings, which are written where they happen and never reach here.
        Err(error) => log::error!("{}", error.message),
    }
}

/// Log a refresh that failed, with the level and message the specification's
/// [errors table](https://github.com/51Degrees/specifications/blob/16ecea439490552e3d1f13c17dc48ac8ae8d9684/pipeline-specification/features/data-updates.md#errors)
/// gives for an error when calling refresh on an engine.
fn log_refresh_failed(engine: &str, error: &DataUpdateError) {
    log::error!(
        "An error occurred when applying a data update to engine '{engine}'. \
         Error detail: {error}"
    );
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

/// The longest the scheduler sleeps between rounds when no poll is due
/// sooner.
const LONGEST_SLEEP_SECONDS: u64 = 60 * 60;

/// The shortest the scheduler sleeps between rounds, so a poll that is
/// already due does not spin the loop.
const SHORTEST_SLEEP_MILLISECONDS: u64 = 50;

/// How long the scheduler sleeps at `now` when the earliest next poll is
/// `earliest`. That is until the poll, but no longer than the longest sleep,
/// which is also the sleep when no poll is scheduled, and no shorter than
/// the shortest sleep.
fn sleep_until(earliest: Option<Instant>, now: Instant) -> Duration {
    let longest = Duration::from_secs(LONGEST_SLEEP_SECONDS);
    earliest
        .map_or(longest, |e| e.saturating_duration_since(now).min(longest))
        .max(Duration::from_millis(SHORTEST_SLEEP_MILLISECONDS))
}

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
fn verify_md5(
    check: bool,
    expected: Option<&str>,
    actual: &str,
    engine: &str,
) -> Result<(), DataUpdateError> {
    match expected {
        Some(expected) if check && !actual.eq_ignore_ascii_case(expected) => {
            Err(DataUpdateError::new(
                AutoUpdateStatus::Md5ValidationFailed,
                format!(
                    "Integrity check failed. MD5 hash in HTTP response '{expected}' \
                     for '{engine}' data update does not match calculated hash for \
                     the downloaded file '{actual}'."
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
        let data_file = polled_file(0, 600);
        let delays: std::collections::BTreeSet<u64> = (0..200)
            .map(|_| {
                let start = Instant::now();
                DataUpdateService::poll_due(&data_file)
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
            inner: Arc::new(Inner::new()),
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

    /// How far ahead [`DatedEngine`]'s new data says the next data is due.
    const NEXT_UPDATE: Duration = Duration::from_secs(24 * 60 * 60);

    /// More than a slow test takes between noting the time and scheduling
    /// the next poll.
    const SLOW_TEST: Duration = Duration::from_secs(30);

    /// The polling interval of [`dated_engine`]'s data file.
    const POLL_SECONDS: u64 = 60;

    /// The next update date [`NEXT_UPDATE`] from now.
    fn next_update_date() -> DateTime<Utc> {
        Utc::now() + chrono::TimeDelta::from_std(NEXT_UPDATE).unwrap()
    }

    /// An engine whose refresh from memory records the next update date the
    /// new data gives, [`NEXT_UPDATE`] from now, as a data file's header
    /// does, and counts the refreshes.
    struct DatedEngine {
        filter: fiftyone_pipeline_core::EvidenceKeyFilterWhitelist,
        data_files: Vec<Arc<AspectEngineDataFile>>,
        refreshes: std::sync::atomic::AtomicUsize,
    }

    impl fiftyone_pipeline_core::FlowElement for DatedEngine {
        fn process(
            &self,
            _data: &mut fiftyone_pipeline_core::FlowData,
        ) -> fiftyone_pipeline_core::Result<()> {
            Ok(())
        }
        fn data_key(&self) -> &str {
            "dated"
        }
        fn evidence_key_filter(&self) -> &dyn fiftyone_pipeline_core::EvidenceKeyFilter {
            &self.filter
        }
        fn properties(&self) -> &[fiftyone_pipeline_core::PropertyMetaData] {
            &[]
        }
    }

    impl crate::AspectEngine for DatedEngine {
        fn data_source_tier(&self) -> &str {
            "Lite"
        }
        fn aspect_properties(&self) -> &[crate::AspectPropertyMetaData] {
            &[]
        }
    }

    impl OnPremiseAspectEngine for DatedEngine {
        fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
            &self.data_files
        }
        fn refresh(&self, _identifier: Option<&str>) -> fiftyone_pipeline_core::Result<()> {
            Ok(())
        }
        fn refresh_from_memory(
            &self,
            _identifier: Option<&str>,
            _data: &[u8],
        ) -> fiftyone_pipeline_core::Result<()> {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            self.data_files[0].set_update_available_time(next_update_date());
            Ok(())
        }
    }

    /// A [`DatedEngine`] with one memory data file updated from `url`, with
    /// automatic updates as `automatic` says, polled every [`POLL_SECONDS`]
    /// with no random delay. The engine is returned twice, once to count its
    /// refreshes and once as the service takes it.
    fn dated_engine(
        url: &str,
        automatic: bool,
    ) -> (Arc<DatedEngine>, Arc<dyn OnPremiseAspectEngine>) {
        dated_engine_every(url, automatic, POLL_SECONDS)
    }

    /// A [`dated_engine`] polled every `poll_seconds`.
    fn dated_engine_every(
        url: &str,
        automatic: bool,
        poll_seconds: u64,
    ) -> (Arc<DatedEngine>, Arc<dyn OnPremiseAspectEngine>) {
        let data_file = Arc::new(AspectEngineDataFile::new(
            crate::DataFileConfiguration::memory_builder()
                .data_update_url(url)
                .automatic_updates_enabled(automatic)
                .polling_interval_seconds(poll_seconds)
                .max_randomisation_seconds(0)
                .decompress_content(false)
                .build(),
        ));
        let engine = Arc::new(DatedEngine {
            filter: fiftyone_pipeline_core::EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            data_files: vec![data_file],
            refreshes: std::sync::atomic::AtomicUsize::new(0),
        });
        let shared: Arc<dyn OnPremiseAspectEngine> = engine.clone();
        (engine, shared)
    }

    /// Answer one request to a URL on this machine with the status line
    /// `status` and `body`.
    fn answer_once(status: &'static str, body: &'static [u8]) -> (String, JoinHandle<()>) {
        answer_once_after(Duration::ZERO, status, body)
    }

    /// Answer one request as [`answer_once`] does, `delay` after reading it.
    fn answer_once_after(
        delay: Duration,
        status: &'static str,
        body: &'static [u8],
    ) -> (String, JoinHandle<()>) {
        let (url, server, _read) = answer_once_after_telling(delay, status, body);
        (url, server)
    }

    /// Answer one request as [`answer_once_after`] does, and send the time
    /// the request was read to the receiver returned.
    fn answer_once_after_telling(
        delay: Duration,
        status: &'static str,
        body: &'static [u8],
    ) -> (String, JoinHandle<()>, std::sync::mpsc::Receiver<Instant>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/datafile", listener.local_addr().unwrap());
        let (read, told) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = Read::read(&mut stream, &mut request);
            let _ = read.send(Instant::now());
            std::thread::sleep(delay);
            let head = format!(
                "{status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            Write::write_all(&mut stream, head.as_bytes()).unwrap();
            Write::write_all(&mut stream, body).unwrap();
        });
        (url, server, told)
    }

    /// A scheduler handle with no registrations, for a test to add its own.
    fn empty_handle() -> ServiceHandle {
        ServiceHandle {
            client: reqwest::blocking::Client::new(),
            inner: Arc::new(Inner::new()),
        }
    }

    /// Add a registration for `engine`'s data file whose poll is due now.
    fn add_due(
        handle: &ServiceHandle,
        engine: &Arc<dyn OnPremiseAspectEngine>,
    ) -> Arc<Registration> {
        let registration = Arc::new(Registration {
            engine: Arc::downgrade(engine),
            data_file: Arc::clone(&engine.data_files()[0]),
            next_poll: Mutex::new(Some(Instant::now())),
            watcher: Mutex::new(None),
        });
        handle
            .inner
            .registrations
            .lock()
            .unwrap()
            .push(Arc::clone(&registration));
        registration
    }

    /// How long after `from` the registration's next poll is due.
    fn next_poll_after(registration: &Registration, from: Instant) -> Duration {
        registration
            .next_poll
            .lock()
            .unwrap()
            .expect("no poll is scheduled")
            .saturating_duration_since(from)
    }

    /// A scheduled poll that applies new data waits next for the next update
    /// date the new data gives, as registration does, rather than one
    /// polling interval.
    #[test]
    fn a_scheduled_poll_that_applies_data_waits_for_its_next_update_date() {
        let (url, server) = answer_once("HTTP/1.1 200 OK", b"new data");
        let (engine, shared) = dated_engine(&url, true);
        let handle = empty_handle();
        let registration = add_due(&handle, &shared);

        let polled = Instant::now();
        handle.run_due_polls();
        server.join().unwrap();

        assert_eq!(
            engine.refreshes.load(Ordering::SeqCst),
            1,
            "no update was applied"
        );
        let wait = next_poll_after(&registration, polled);
        assert!(
            wait > NEXT_UPDATE - SLOW_TEST && wait < NEXT_UPDATE + SLOW_TEST,
            "the next poll is due after {wait:?}, not at the next update date"
        );
    }

    /// A scheduled poll that applies nothing tries again after one polling
    /// interval, even with a next update date ahead.
    #[test]
    fn a_scheduled_poll_that_applies_nothing_waits_one_interval() {
        let (url, server) = answer_once("HTTP/1.1 304 Not Modified", b"");
        let (engine, shared) = dated_engine(&url, true);
        shared.data_files()[0].set_update_available_time(next_update_date());
        let handle = empty_handle();
        let registration = add_due(&handle, &shared);

        let polled = Instant::now();
        handle.run_due_polls();
        server.join().unwrap();

        assert_eq!(
            engine.refreshes.load(Ordering::SeqCst),
            0,
            "an update was applied"
        );
        let wait = next_poll_after(&registration, polled);
        assert!(
            wait > Duration::from_secs(POLL_SECONDS) - SLOW_TEST
                && wait < Duration::from_secs(POLL_SECONDS) + SLOW_TEST,
            "the next poll is due after {wait:?}, not one interval"
        );
    }

    /// How long the slow server in the tests below takes to answer, longer
    /// than [`SHORT_INTERVAL`].
    const SLOW_ANSWER: Duration = Duration::from_secs(2);

    /// A polling interval shorter than [`SLOW_ANSWER`].
    const SHORT_INTERVAL: Duration = Duration::from_secs(1);

    /// A scheduled poll that applies nothing and ends after its interval has
    /// passed waits one interval from when it ended, rather than polling
    /// again at once.
    #[test]
    fn a_slow_poll_that_applies_nothing_waits_one_interval_after_it_ends() {
        let (url, server) = answer_once_after(SLOW_ANSWER, "HTTP/1.1 304 Not Modified", b"");
        let (engine, shared) = dated_engine_every(&url, true, SHORT_INTERVAL.as_secs());
        let handle = empty_handle();
        let registration = add_due(&handle, &shared);

        let sleep = handle.run_due_polls();
        let ended = Instant::now();
        server.join().unwrap();

        assert_eq!(
            engine.refreshes.load(Ordering::SeqCst),
            0,
            "an update was applied"
        );
        let wait = next_poll_after(&registration, ended);
        assert!(
            wait > SHORT_INTERVAL / 2 && wait <= SHORT_INTERVAL,
            "the next poll is due {wait:?} after the poll ended, not one interval"
        );
        assert!(
            sleep > SHORT_INTERVAL / 2 && sleep <= SHORT_INTERVAL,
            "the scheduler sleeps {sleep:?} after the poll, not one interval"
        );
    }

    /// A poll that comes due while another file's poll runs is run in the
    /// same round, rather than left until a sleep counted from before the
    /// round began has passed.
    #[test]
    fn a_poll_that_comes_due_during_a_slow_poll_runs_in_the_same_round() {
        let (slow_url, slow_server) =
            answer_once_after(SLOW_ANSWER, "HTTP/1.1 304 Not Modified", b"");
        let (_slow_engine, slow) = dated_engine(&slow_url, true);
        let (url, server) = answer_once("HTTP/1.1 304 Not Modified", b"");
        let (_engine, shared) = dated_engine(&url, true);
        let handle = empty_handle();
        add_due(&handle, &slow);
        let registration = add_due(&handle, &shared);
        *registration.next_poll.lock().unwrap() = Some(Instant::now() + SLOW_ANSWER / 2);

        handle.run_due_polls();
        slow_server.join().unwrap();

        let wait = next_poll_after(&registration, Instant::now());
        assert!(
            wait > Duration::from_secs(POLL_SECONDS) - SLOW_TEST,
            "the poll that came due was not run, the next is due after {wait:?}"
        );
        server.join().unwrap();
    }

    /// After a slow poll the scheduler sleeps until the next poll is due,
    /// counted from when the slow poll ended.
    #[test]
    fn the_sleep_after_a_slow_poll_ends_when_the_next_poll_is_due() {
        let (slow_url, slow_server) =
            answer_once_after(SLOW_ANSWER, "HTTP/1.1 304 Not Modified", b"");
        let (_slow_engine, slow) = dated_engine(&slow_url, true);
        let (_engine, shared) = dated_engine("http://127.0.0.1:9/unused", true);
        let handle = empty_handle();
        add_due(&handle, &slow);
        let registration = add_due(&handle, &shared);
        let due = Instant::now() + SLOW_ANSWER + SHORT_INTERVAL;
        *registration.next_poll.lock().unwrap() = Some(due);

        let sleep = handle.run_due_polls();
        let ended = Instant::now();
        slow_server.join().unwrap();

        let until_due = due.saturating_duration_since(ended);
        assert!(
            sleep <= until_due + SHORT_INTERVAL / 2,
            "the scheduler sleeps {sleep:?} when the next poll is due in {until_due:?}"
        );
    }

    /// A file that is not yet due and comes before a slow poll in the round
    /// still has its poll at its due time, because the sleep is counted from
    /// the end of the round rather than from when that file was seen.
    #[test]
    fn the_sleep_ends_when_a_file_seen_before_a_slow_poll_is_due() {
        let (_engine, shared) = dated_engine("http://127.0.0.1:9/unused", true);
        let (slow_url, slow_server) =
            answer_once_after(SLOW_ANSWER, "HTTP/1.1 304 Not Modified", b"");
        let (_slow_engine, slow) = dated_engine(&slow_url, true);
        let handle = empty_handle();
        let registration = add_due(&handle, &shared);
        add_due(&handle, &slow);
        let due = Instant::now() + SLOW_ANSWER + SHORT_INTERVAL;
        *registration.next_poll.lock().unwrap() = Some(due);

        let sleep = handle.run_due_polls();
        let ended = Instant::now();
        slow_server.join().unwrap();

        let until_due = due.saturating_duration_since(ended);
        assert!(
            sleep <= until_due + SHORT_INTERVAL / 2 && sleep + SHORT_INTERVAL / 2 >= until_due,
            "the scheduler sleeps {sleep:?} when the next poll is due in {until_due:?}"
        );
    }

    /// The longest sleep, written apart from [`LONGEST_SLEEP_SECONDS`] so that
    /// a change to that constant fails the tests below.
    const ONE_HOUR: Duration = Duration::from_secs(60 * 60);

    /// The shortest sleep, written apart from [`SHORTEST_SLEEP_MILLISECONDS`]
    /// so that a change to that constant fails the test below.
    const FIFTY_MILLISECONDS: Duration = Duration::from_millis(50);

    /// With no poll scheduled the scheduler sleeps for the longest sleep.
    #[test]
    fn with_no_poll_scheduled_the_sleep_is_one_hour() {
        assert_eq!(sleep_until(None, Instant::now()), ONE_HOUR);
    }

    /// A poll further away than the longest sleep is slept towards for the
    /// longest sleep, and the scheduler then looks again.
    #[test]
    fn a_poll_two_hours_away_gives_a_sleep_of_one_hour() {
        let now = Instant::now();
        assert_eq!(sleep_until(Some(now + 2 * ONE_HOUR), now), ONE_HOUR);
    }

    /// A poll that is already due gives the shortest sleep.
    #[test]
    fn a_poll_already_due_gives_a_sleep_of_fifty_milliseconds() {
        let due = Instant::now();
        let now = due + Duration::from_secs(1);
        assert_eq!(sleep_until(Some(due), now), FIFTY_MILLISECONDS);
    }

    /// A poll due within the longest sleep is slept towards exactly.
    #[test]
    fn a_poll_three_seconds_away_gives_a_sleep_of_three_seconds() {
        let now = Instant::now();
        let ahead = Duration::from_secs(3);
        assert_eq!(sleep_until(Some(now + ahead), now), ahead);
    }

    /// A polling interval longer than [`SLOW_ANSWER`], so a file registered
    /// as a slow poll starts comes due after that poll has ended.
    const LONG_INTERVAL: Duration = Duration::from_secs(3);

    /// How long a test waits for a poll it expects.
    const POLL_WAIT: Duration = Duration::from_secs(10);

    /// A file registered while another file's slow poll runs has its first
    /// poll when it is due. The slow poll applies new data, so the round
    /// that runs it plans to sleep for the longest sleep, and the wake the
    /// registration sends during that round must not be lost.
    #[test]
    fn a_file_registered_during_a_slow_round_is_polled_when_due() {
        let (slow_url, slow_server, slow_read) =
            answer_once_after_telling(SLOW_ANSWER, "HTTP/1.1 200 OK", b"new data");
        let (_slow_engine, slow) = dated_engine_every(&slow_url, true, SHORT_INTERVAL.as_secs());
        let (url, server, read) =
            answer_once_after_telling(Duration::ZERO, "HTTP/1.1 304 Not Modified", b"");
        let (_engine, shared) = dated_engine_every(&url, true, LONG_INTERVAL.as_secs());
        let service = DataUpdateService::new();
        service
            .register(Arc::clone(&slow), Arc::clone(&slow.data_files()[0]))
            .unwrap();

        slow_read
            .recv_timeout(POLL_WAIT)
            .expect("the slow poll did not start");
        let due = Instant::now() + LONG_INTERVAL;
        service
            .register(Arc::clone(&shared), Arc::clone(&shared.data_files()[0]))
            .unwrap();
        let polled = read.recv_timeout(POLL_WAIT);
        drop(service);
        slow_server.join().unwrap();

        let polled = polled.unwrap_or_else(|_| {
            panic!("the file registered during the round was not polled within {POLL_WAIT:?}")
        });
        assert!(
            polled + SHORT_INTERVAL / 2 >= due && polled <= due + SHORT_INTERVAL,
            "the first poll ran {:?} from when it was due",
            polled.max(due) - polled.min(due)
        );
        server.join().unwrap();
    }

    /// A programmatic check that applies new data to a registered file moves
    /// its next scheduled poll to the next update date the new data gives.
    #[test]
    fn a_programmatic_update_that_applies_data_waits_for_its_next_update_date() {
        let (url, server) = answer_once("HTTP/1.1 200 OK", b"new data");
        let (engine, shared) = dated_engine(&url, true);
        let service = DataUpdateService::new();
        service
            .register(Arc::clone(&shared), Arc::clone(&shared.data_files()[0]))
            .unwrap();
        let registration = only_registration(&service);
        assert!(
            next_poll_after(&registration, Instant::now()) <= Duration::from_secs(POLL_SECONDS),
            "the first poll is not one interval away"
        );

        let checked = Instant::now();
        let status = service.check_for_update(&shared, None);
        server.join().unwrap();

        assert_eq!(
            status.map_err(|error| error.to_string()),
            Ok(AutoUpdateStatus::Success)
        );
        assert_eq!(engine.refreshes.load(Ordering::SeqCst), 1);
        let wait = next_poll_after(&registration, checked);
        assert!(
            wait > NEXT_UPDATE - SLOW_TEST && wait < NEXT_UPDATE + SLOW_TEST,
            "the next poll is due after {wait:?}, not at the next update date"
        );
        drop(registration);
        drop(service);
    }

    /// A programmatic update of a file whose automatic updates are off
    /// schedules no poll.
    #[test]
    fn a_programmatic_update_schedules_no_poll_when_automatic_updates_are_off() {
        let (url, server) = answer_once("HTTP/1.1 200 OK", b"new data");
        let (engine, shared) = dated_engine(&url, false);
        let service = DataUpdateService::new();
        service
            .register(Arc::clone(&shared), Arc::clone(&shared.data_files()[0]))
            .unwrap();

        let status = service.check_for_update(&shared, None);
        server.join().unwrap();

        assert_eq!(
            status.map_err(|error| error.to_string()),
            Ok(AutoUpdateStatus::Success)
        );
        assert_eq!(engine.refreshes.load(Ordering::SeqCst), 1);
        assert!(
            only_registration(&service)
                .next_poll
                .lock()
                .unwrap()
                .is_none(),
            "a poll was scheduled"
        );
        drop(service);
    }

    /// How long a test waits for something it expects of a watcher.
    const WATCHER_WAIT: Duration = Duration::from_secs(10);

    /// How long the watcher is given to start before the file changes.
    const WATCHER_START: Duration = Duration::from_millis(300);

    /// How often a test checks again for something it is waiting on.
    const WATCHER_POLL: Duration = Duration::from_millis(20);

    /// An engine with one watched data file, whose refresh tells the test it
    /// has started and then waits until the test lets it finish.
    struct WatchedEngine {
        filter: fiftyone_pipeline_core::EvidenceKeyFilterWhitelist,
        data_files: Vec<Arc<AspectEngineDataFile>>,
        started: Mutex<std::sync::mpsc::Sender<()>>,
        finish: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl fiftyone_pipeline_core::FlowElement for WatchedEngine {
        fn process(
            &self,
            _data: &mut fiftyone_pipeline_core::FlowData,
        ) -> fiftyone_pipeline_core::Result<()> {
            Ok(())
        }
        fn data_key(&self) -> &str {
            "watched"
        }
        fn evidence_key_filter(&self) -> &dyn fiftyone_pipeline_core::EvidenceKeyFilter {
            &self.filter
        }
        fn properties(&self) -> &[fiftyone_pipeline_core::PropertyMetaData] {
            &[]
        }
    }

    impl crate::AspectEngine for WatchedEngine {
        fn data_source_tier(&self) -> &str {
            "Lite"
        }
        fn aspect_properties(&self) -> &[crate::AspectPropertyMetaData] {
            &[]
        }
    }

    impl OnPremiseAspectEngine for WatchedEngine {
        fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
            &self.data_files
        }
        fn refresh(&self, _identifier: Option<&str>) -> fiftyone_pipeline_core::Result<()> {
            let _ = self.started.lock().unwrap().send(());
            let _ = self.finish.lock().unwrap().recv_timeout(WATCHER_WAIT);
            Ok(())
        }
    }

    /// A watched data file in a new folder for one test, its engine, and the
    /// ends of the channels its refresh uses.
    struct Watched {
        dir: PathBuf,
        path: PathBuf,
        engine: Arc<dyn OnPremiseAspectEngine>,
        data_file: Arc<AspectEngineDataFile>,
        started: std::sync::mpsc::Receiver<()>,
        finish: std::sync::mpsc::Sender<()>,
    }

    fn watched(test: &str) -> Watched {
        watched_with(test, |config| config)
    }

    /// As [`watched`], with `configure` adding to the data file's
    /// configuration.
    fn watched_with(
        test: &str,
        configure: impl FnOnce(
            crate::data_file::DataFileConfigurationBuilder,
        ) -> crate::data_file::DataFileConfigurationBuilder,
    ) -> Watched {
        let dir = std::env::temp_dir().join(format!(
            "fiftyone-update-service-{test}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("device.dat");
        std::fs::write(&path, b"version 1").unwrap();
        let data_file = Arc::new(AspectEngineDataFile::new(
            configure(
                crate::DataFileConfiguration::builder(&path)
                    .automatic_updates_enabled(true)
                    .file_system_watcher_enabled(true),
            )
            .build(),
        ));
        let (started_sender, started) = std::sync::mpsc::channel();
        let (finish, finish_receiver) = std::sync::mpsc::channel();
        let engine: Arc<dyn OnPremiseAspectEngine> = Arc::new(WatchedEngine {
            filter: fiftyone_pipeline_core::EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            data_files: vec![Arc::clone(&data_file)],
            started: Mutex::new(started_sender),
            finish: Mutex::new(finish_receiver),
        });
        Watched {
            dir,
            path,
            engine,
            data_file,
            started,
            finish,
        }
    }

    /// The service's only registration.
    fn only_registration(service: &DataUpdateService) -> Arc<Registration> {
        let registrations = service.inner.registrations.lock().unwrap();
        assert_eq!(registrations.len(), 1);
        Arc::clone(&registrations[0])
    }

    /// While the watcher's callback refreshes the engine, the callback holds
    /// no reference to the registration that owns the watcher. Were it to hold
    /// one, a prune on another thread could leave the callback the last
    /// reference, and the watcher would be dropped on its own thread, which
    /// on macOS waits for that same callback to finish and never returns.
    #[test]
    fn a_file_event_does_not_hold_its_registration() {
        let watched = watched("event");
        let service = DataUpdateService::new();
        service
            .register(Arc::clone(&watched.engine), Arc::clone(&watched.data_file))
            .unwrap();
        std::thread::sleep(WATCHER_START);
        let registration = only_registration(&service);

        std::fs::write(&watched.path, b"version 2").unwrap();
        let started = watched.started.recv_timeout(WATCHER_WAIT);
        // The service's list and this test hold the registration.
        let held = Arc::strong_count(&registration);
        drop(watched.finish);

        assert!(started.is_ok(), "the watcher did not refresh the engine");
        assert_eq!(held, 2, "the watcher's callback holds the registration");
        drop(registration);
        drop(service);
        let _ = std::fs::remove_dir_all(&watched.dir);
    }

    /// Dropping the engine drops its registration at the next prune, and with
    /// it the watcher the registration owns.
    #[test]
    fn dropping_the_engine_drops_its_registration_and_watcher() {
        let watched = watched("drop");
        let service = DataUpdateService::new();
        service
            .register(Arc::clone(&watched.engine), Arc::clone(&watched.data_file))
            .unwrap();
        let registration = {
            let registration = only_registration(&service);
            assert!(
                registration.watcher.lock().unwrap().is_some(),
                "no watcher was installed"
            );
            Arc::downgrade(&registration)
        };

        drop(watched.engine);
        assert_eq!(service.registered_count(), 0);

        // A scheduler round that began before the prune may hold the
        // registration until the round ends.
        let start = Instant::now();
        while registration.strong_count() > 0 && start.elapsed() < WATCHER_WAIT {
            std::thread::sleep(WATCHER_POLL);
        }
        assert!(
            registration.upgrade().is_none(),
            "the registration and its watcher outlived the engine"
        );
        drop(service);
        let _ = std::fs::remove_dir_all(&watched.dir);
    }

    /// An update URL on this machine that counts the connections made to it
    /// and closes each at once, so a poll of it fails straight away.
    fn counting_url() -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/datafile", listener.local_addr().unwrap());
        let connections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&connections);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                counted.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
        (url, connections)
    }

    /// Given the only reference to an engine, the service polls nothing on
    /// start-up and installs no watcher or schedule, as the engine is dropped
    /// when `register` returns. Given a reference the caller keeps, the same
    /// configuration is polled and watched, so the test reaches both.
    #[test]
    fn the_only_reference_to_an_engine_is_not_registered() {
        let (url, connections) = counting_url();
        let polled = |config: crate::data_file::DataFileConfigurationBuilder| {
            config.data_update_url(url.clone()).update_on_startup(true)
        };
        let service = DataUpdateService::new();

        let only = watched_with("only", polled);
        let status = service.register(only.engine, Arc::clone(&only.data_file));
        assert_eq!(status.unwrap(), AutoUpdateStatus::NotNeeded);
        assert_eq!(
            connections.load(Ordering::SeqCst),
            0,
            "the start-up poll ran"
        );
        assert!(
            only.data_file.last_applied_modified().is_none(),
            "a watcher was installed"
        );
        assert!(!only.data_file.is_registered());
        assert!(service.inner.registrations.lock().unwrap().is_empty());
        assert!(
            service.scheduler.lock().unwrap().is_none(),
            "the scheduler started"
        );

        let kept = watched_with("kept", polled);
        let status = service.register(Arc::clone(&kept.engine), Arc::clone(&kept.data_file));
        assert_eq!(status.unwrap_err().status, AutoUpdateStatus::HttpsError);
        assert!(
            connections.load(Ordering::SeqCst) > 0,
            "no start-up poll ran"
        );
        assert!(
            only_registration(&service)
                .watcher
                .lock()
                .unwrap()
                .is_some(),
            "no watcher was installed"
        );
        assert!(service.scheduler.lock().unwrap().is_some());

        drop(kept.engine);
        drop(service);
        let _ = std::fs::remove_dir_all(&only.dir);
        let _ = std::fs::remove_dir_all(&kept.dir);
    }
}
