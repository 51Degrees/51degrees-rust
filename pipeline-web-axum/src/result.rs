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

//! The processed flow data, shared into request extensions and pulled back out
//! by an extractor.
//!
//! The middleware processes the pipeline once per request and stores the result
//! so a downstream handler can read it without reprocessing. A
//! [`fiftyone_pipeline_core::FlowData`] is `Send + Sync` but not `Clone`, and
//! axum request extensions require a `Clone + Send + Sync` value, so the flow
//! data is shared through an `Arc`. The `Mutex` inside it lets a caller of
//! [`FiftyOneResult::flow_data`] change the flow data. The lock is only ever
//! held briefly and synchronously while reading a result, never across an
//! `await`, so it does not block the runtime.
//!
//! Handlers read the flow data through the [`FiftyOneResult`] extractor, which
//! clones the shared handle out of the request extensions. From it,
//! [`FiftyOneResult::with`] borrows the locked flow data for a closure, and the
//! typed and string getters cover the common single-value reads.

use std::sync::{Arc, Mutex};

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use fiftyone_pipeline_core::{ElementData, FlowData, TypedKey};

/// A handle to the processed flow data for one request.
///
/// Cloning is cheap (it shares one `Arc`). Obtain it as a handler argument
/// through the [`FromRequestParts`] implementation, which requires the
/// [`crate::middleware::fiftyone_middleware`] to have run first so the flow data
/// is present in the request extensions.
#[derive(Clone)]
pub struct FiftyOneResult {
    flow_data: Arc<Mutex<FlowData>>,
}

impl FiftyOneResult {
    /// Wrap an already-processed flow data. The middleware uses this to put the
    /// flow data into the request extensions.
    pub fn new(flow_data: FlowData) -> Self {
        FiftyOneResult {
            flow_data: Arc::new(Mutex::new(flow_data)),
        }
    }

    /// Run a closure with a shared borrow of the locked flow data and return its
    /// result.
    ///
    /// This is the general accessor: the closure can call any [`FlowData`]
    /// method (for example reading a device-detection or IP-intelligence facade
    /// value). The lock is released as soon as the closure returns.
    ///
    /// # Panics
    ///
    /// Panics only if the lock was poisoned by a previous panic while it was
    /// held, which cannot happen here because the guarded section never panics.
    pub fn with<R>(&self, f: impl FnOnce(&FlowData) -> R) -> R {
        let guard = self
            .flow_data
            .lock()
            .expect("flow data lock is not poisoned");
        f(&guard)
    }

    /// Read a typed element-data result by its [`TypedKey`], applying a mapping
    /// closure while the data is borrowed.
    ///
    /// Returns `None` when no element produced data under the key. The mapping
    /// closure exists because the borrowed `&T` cannot escape the lock, so the
    /// caller extracts (and usually clones) what it needs inside it.
    pub fn get<T, R>(&self, key: TypedKey<T>, map: impl FnOnce(&T) -> R) -> Option<R>
    where
        T: ElementData,
    {
        self.with(|data| data.get(key).map(map))
    }

    /// True if the request's processing recorded one or more errors.
    ///
    /// The web pipeline suppresses element errors onto the flow data, so a
    /// failing element shows up here rather than as a failed request.
    pub fn has_errors(&self) -> bool {
        self.with(|data| !data.errors().is_empty())
    }

    /// The shared handle to the locked flow data, for callers that want to hold
    /// or store it directly.
    pub fn flow_data(&self) -> &Arc<Mutex<FlowData>> {
        &self.flow_data
    }
}

impl<S> FromRequestParts<S> for FiftyOneResult
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<FiftyOneResult>().cloned().ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            "51Degrees flow data missing from request extensions; is the \
                 fiftyone_middleware installed?",
        ))
    }
}
