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

//! What produces a value on a miss.

use std::future::Future;
use std::sync::Arc;

use super::entry::Loaded;

/// Produces the value for a key that a cache does not hold.
///
/// The source of the values is a loader, and so is every
/// [`crate::LoadingCache`], which is how caches stack.
pub trait ValueLoader<K, V> {
    /// The error a failed load returns. It is cloned to every caller waiting
    /// on the load, so wrap an error that cannot be cloned in an
    /// [`Arc`].
    type Error: Clone;

    /// Loads the value for `key`.
    fn load(&self, key: &K) -> impl Future<Output = Result<Loaded<V>, Self::Error>>;
}

/// A shared loader, so several caches can load through one cache.
impl<K, V, L: ValueLoader<K, V>> ValueLoader<K, V> for Arc<L> {
    type Error = L::Error;

    fn load(&self, key: &K) -> impl Future<Output = Result<Loaded<V>, Self::Error>> {
        (**self).load(key)
    }
}

/// A [`ValueLoader`] made from a function, see [`from_fn`].
#[derive(Debug, Clone, Copy)]
pub struct FnLoader<F>(F);

/// Makes a [`ValueLoader`] from a function that takes the key and returns a
/// future of the value, or of a [`Loaded`] when it knows the value's expiry.
///
/// ```
/// use fiftyone_caching::from_fn;
///
/// let loader = from_fn(|key: String| async move {
///     Ok::<_, String>(format!("value for {key}"))
/// });
/// # let _ = loader;
/// ```
pub fn from_fn<F>(load: F) -> FnLoader<F> {
    FnLoader(load)
}

impl<K, V, E, T, F, Fut> ValueLoader<K, V> for FnLoader<F>
where
    K: Clone,
    E: Clone,
    T: Into<Loaded<V>>,
    F: Fn(K) -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    type Error = E;

    fn load(&self, key: &K) -> impl Future<Output = Result<Loaded<V>, E>> {
        let load = (self.0)(key.clone());
        async move { load.await.map(Into::into) }
    }
}
