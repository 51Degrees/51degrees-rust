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

//! Where a cache runs its loads, in the caller or as tasks of their own
//! that a host starts.

use std::future::Future;
use std::hash::Hash;
use std::pin::Pin;

use super::cache::Job;
use super::entry::Loaded;
use super::flight::Outcome;
use super::loader::ValueLoader;
use super::store::Store;

/// A load handed to a spawner, as a future made on the thread that runs it,
/// so it need not be `Send`.
pub type LoadTask = Pin<Box<dyn Future<Output = ()>>>;

/// Makes a load's [`LoadTask`]. A [`Spawn`] calls it on the thread that
/// will run the task.
pub type StartLoad = Box<dyn FnOnce() -> LoadTask + Send>;

/// Starts loads as tasks of their own, on any thread, for
/// [`LoadingCacheBuilder::spawner`](crate::LoadingCacheBuilder::spawner).
///
/// A load started this way runs to its end even if every caller waiting on
/// it is dropped, as a .NET `Lazy<Task>` does. Every caller, the first
/// included, waits for its result.
///
/// The task `start` makes is not `Send`, so call `start` on the thread that
/// will run the task. On a multi-threaded tokio runtime, for example, run it
/// with `tokio::task::spawn_blocking` and `Handle::block_on`, or with
/// `tokio_util::task::LocalPoolHandle::spawn_pinned`.
///
/// Run every task to its end. A task dropped before it ends, by a panic in
/// the load or by the spawner, is a lost load. Each caller waiting on it
/// asks for the load once more, and a caller that loses a second load
/// panics rather than ask without end.
///
/// Any `Fn(StartLoad)` closure is a spawner.
pub trait Spawn {
    /// Starts the task `start` makes and runs it to its end.
    fn spawn(&self, start: StartLoad);
}

impl<F: Fn(StartLoad)> Spawn for F {
    fn spawn(&self, start: StartLoad) {
        self(start)
    }
}

/// Starts loads as tasks of their own on the current thread, for
/// [`LoadingCacheBuilder::local_spawner`](crate::LoadingCacheBuilder::local_spawner),
/// such as `spawn_local` on a single-threaded runtime or a JavaScript event
/// loop.
///
/// A load started this way runs to its end even if every caller waiting on
/// it is dropped. Run every task to its end. A task dropped before it ends
/// is a lost load, as for a [`Spawn`].
///
/// Any `Fn(LoadTask)` closure is a local spawner.
pub trait SpawnLocal {
    /// Starts `task` on the current thread and runs it to its end.
    fn spawn_local(&self, task: LoadTask);
}

impl<F: Fn(LoadTask)> SpawnLocal for F {
    fn spawn_local(&self, task: LoadTask) {
        self(task)
    }
}

/// Runs each load in the caller that missed. The first caller does the load,
/// and if it is dropped a waiting caller takes over. The default.
#[derive(Debug, Clone, Copy, Default)]
pub struct Inline;

/// Runs each load as a task of its own on any thread, started by a
/// [`Spawn`].
#[derive(Debug, Clone, Copy)]
pub struct Spawned<P>(pub(crate) P);

/// Runs each load as a task of its own on the current thread, started by a
/// [`SpawnLocal`].
#[derive(Debug, Clone, Copy)]
pub struct SpawnedLocal<P>(pub(crate) P);

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Inline {}
    impl<P> Sealed for super::Spawned<P> {}
    impl<P> Sealed for super::SpawnedLocal<P> {}
}

/// Where a [`LoadingCache`](crate::LoadingCache) runs its loads, being
/// [`Inline`], [`Spawned`] or [`SpawnedLocal`]. The caller looks in the store
/// itself whichever runs the loads, so a hit never waits for a task. The
/// spawned forms ask more of the cache's types, because a load leaves the
/// caller for a task that must own everything it uses.
pub trait LoadRunner<K, V, S, L>: sealed::Sealed
where
    K: Hash + Eq,
    S: Store<K, V>,
    L: ValueLoader<K, V>,
{
    /// Runs `job` and waits for its result.
    #[doc(hidden)]
    fn run(
        &self,
        job: Job<K, V, S, L>,
    ) -> impl Future<Output = Outcome<Result<Loaded<V>, L::Error>>>;
}

impl<K, V, S, L> LoadRunner<K, V, S, L> for Inline
where
    K: Hash + Eq + Clone,
    V: Clone,
    S: Store<K, V>,
    L: ValueLoader<K, V>,
{
    async fn run(&self, job: Job<K, V, S, L>) -> Outcome<Result<Loaded<V>, L::Error>> {
        Outcome::Done(job.run().await)
    }
}

impl<K, V, S, L, P> LoadRunner<K, V, S, L> for Spawned<P>
where
    P: Spawn,
    K: Hash + Eq + Clone + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    S: Store<K, V> + Send + Sync + 'static,
    S::Reservation: Send + 'static,
    L: ValueLoader<K, V> + Send + Sync + 'static,
    L::Error: Send + Sync + 'static,
{
    fn run(
        &self,
        job: Job<K, V, S, L>,
    ) -> impl Future<Output = Outcome<Result<Loaded<V>, L::Error>>> {
        // Waiting before the task starts means a quick task cannot finish
        // unseen.
        let wait = job.wait();
        let job = job.for_task();
        self.0
            .spawn(Box::new(move || -> LoadTask { Box::pin(job.finish()) }));
        wait
    }
}

impl<K, V, S, L, P> LoadRunner<K, V, S, L> for SpawnedLocal<P>
where
    P: SpawnLocal,
    K: Hash + Eq + Clone + 'static,
    V: Clone + 'static,
    S: Store<K, V> + 'static,
    S::Reservation: 'static,
    L: ValueLoader<K, V> + 'static,
    L::Error: 'static,
{
    fn run(
        &self,
        job: Job<K, V, S, L>,
    ) -> impl Future<Output = Outcome<Result<Loaded<V>, L::Error>>> {
        let wait = job.wait();
        self.0.spawn_local(Box::pin(job.for_task().finish()));
        wait
    }
}
