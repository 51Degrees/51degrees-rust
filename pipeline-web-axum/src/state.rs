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

//! The shared state the middleware and handlers carry.
//!
//! [`FiftyOneState`] bundles the built [`Pipeline`], the
//! [`WebIntegrationOptions`] that drive endpoint routing and the
//! [`EvidenceKeyFilterWhitelist`] used to derive the `Vary` header. It is cloned
//! cheaply (every field is an `Arc` or small value) and shared by the tower
//! middleware, the two endpoint handlers and the route helper.
//!
//! Build one from a [`fiftyone_pipeline_web::WebPipeline`] with
//! [`FiftyOneState::from_web_pipeline`], or from the raw parts with
//! [`FiftyOneState::new`].

use std::sync::{Arc, OnceLock};

use fiftyone_pipeline_core::{EvidenceKeyFilterWhitelist, Pipeline};
use fiftyone_pipeline_web::{EndpointOptions, WebEndpoint, WebIntegrationOptions, WebPipeline};

/// The 51Degrees web state shared across the middleware and handlers.
///
/// Cloning is cheap: the pipeline is an `Arc`, and the options and whitelist are
/// wrapped in an `Arc` so a clone copies only pointers. Store one in the axum
/// [`axum::Router`] state, or pass it to [`crate::middleware::fiftyone_middleware`]
/// and the route helper.
#[derive(Clone)]
pub struct FiftyOneState {
    inner: Arc<Inner>,
}

/// The owned contents of a [`FiftyOneState`], referenced through one `Arc`.
struct Inner {
    pipeline: Arc<Pipeline>,
    options: WebIntegrationOptions,
    endpoint_options: EndpointOptions,
}

impl FiftyOneState {
    /// Build the shared state from a pipeline, its web options and the `Vary`
    /// whitelist.
    ///
    /// Prefer [`FiftyOneState::from_web_pipeline`], which takes the three pieces
    /// straight from a [`WebPipeline`] so they cannot drift apart.
    pub fn new(
        pipeline: Arc<Pipeline>,
        options: WebIntegrationOptions,
        vary_whitelist: EvidenceKeyFilterWhitelist,
    ) -> Self {
        Self::with_endpoint_options(pipeline, options, EndpointOptions::new(vary_whitelist))
    }

    /// Build the shared state from a [`WebPipeline`], taking its pipeline,
    /// options and `Vary` whitelist together. The `Vary` whitelist follows the
    /// pipeline's, as [`WebPipeline::vary_whitelist`] describes.
    pub fn from_web_pipeline(web: &WebPipeline) -> Self {
        Self::with_endpoint_options(
            Arc::clone(web.pipeline()),
            web.options().clone(),
            web.endpoint_options(),
        )
    }

    fn with_endpoint_options(
        pipeline: Arc<Pipeline>,
        options: WebIntegrationOptions,
        endpoint_options: EndpointOptions,
    ) -> Self {
        FiftyOneState {
            inner: Arc::new(Inner {
                pipeline,
                options,
                endpoint_options,
            }),
        }
    }

    /// The shared, immutable pipeline. Clone the `Arc` to create flow data.
    pub fn pipeline(&self) -> &Arc<Pipeline> {
        &self.inner.pipeline
    }

    /// The resolved web-integration options.
    pub fn options(&self) -> &WebIntegrationOptions {
        &self.inner.options
    }

    /// The evidence-key whitelist used to derive the `Vary` header.
    pub fn vary_whitelist(&self) -> &EvidenceKeyFilterWhitelist {
        // Both constructors supply a list, so the empty one is never used.
        static NONE: OnceLock<EvidenceKeyFilterWhitelist> = OnceLock::new();
        self.inner
            .endpoint_options
            .current_vary_whitelist()
            .unwrap_or_else(|| NONE.get_or_init(EvidenceKeyFilterWhitelist::default))
    }

    /// The [`EndpointOptions`] the client-side endpoints expect, carrying the
    /// configured `Vary` whitelist.
    pub fn endpoint_options(&self) -> EndpointOptions {
        self.inner.endpoint_options.clone()
    }

    /// Decide which client-side endpoint, if any, a request path targets,
    /// honouring the configured endpoint paths.
    pub fn endpoint_for(&self, path: &str) -> Option<WebEndpoint> {
        let options = &self.inner.options;
        if fiftyone_pipeline_web::endpoint_matches(path, &options.javascript_endpoint) {
            Some(WebEndpoint::JavaScript)
        } else if fiftyone_pipeline_web::endpoint_matches(path, &options.json_endpoint) {
            Some(WebEndpoint::Json)
        } else {
            None
        }
    }
}
