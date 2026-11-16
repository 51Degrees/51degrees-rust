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

//! A serializable snapshot of the cloud engine's discovered state.
//!
//! A [`CloudRequestEngine`](crate::CloudRequestEngine) normally fetches two
//! things from the cloud the first time it is used: the accepted evidence keys
//! (`evidencekeys`) and the accessible properties (`accessibleproperties`). Both
//! depend only on the keys, so for a given pair of keys they are stable. An
//! engine holding a license key and no resource key has no accessible
//! properties to fetch, because that endpoint takes a resource key, so the
//! snapshot holds the evidence keys alone.
//!
//! On a long-lived host that one-time discovery is cheap. On a short-lived host,
//! such as a `wasm32-wasip1` edge runtime where the instance is created and
//! discarded frequently, repeating those two round-trips on every cold start is
//! wasteful. [`CloudEngineState`] lets a consumer lift the discovered values out
//! of one engine, persist them in whatever store the host provides (a const baked
//! into the module, a key-value or config store, and so on), and inject them into
//! the next engine so it skips discovery entirely.
//!
//! The type round-trips through [`serde`], so it serializes to and from JSON (or
//! any other serde format) without loss. See
//! [`CloudRequestEngineBuilder::export_state`](crate::CloudRequestEngineBuilder::export_state)
//! to obtain a snapshot and
//! [`CloudRequestEngineBuilder::set_state`](crate::CloudRequestEngineBuilder::set_state)
//! to inject one.

use fiftyone_pipeline_core::EvidenceKeyFilterWhitelist;
use serde::{Deserialize, Serialize};

use crate::properties::LicensedProducts;

/// One accepted evidence key with its precedence order.
///
/// The order matches [`EvidenceKeyFilterWhitelist`]: a lower order means higher
/// precedence. Keys discovered from the cloud all share the default order of `0`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceKeyEntry {
    /// The evidence key, for example `header.user-agent`. Stored lowercased,
    /// matching the whitelist's case-insensitive comparison.
    pub key: String,
    /// The precedence order; lower means higher precedence.
    pub order: i32,
}

/// A snapshot of the values the builder fetches from the cloud, which it retains
/// so the consumer can persist them and skip the fetch next time.
///
/// Obtain one with
/// [`CloudRequestEngineBuilder::export_state`](crate::CloudRequestEngineBuilder::export_state)
/// after building an engine, persist it, and feed it back into a later build with
/// [`CloudRequestEngineBuilder::set_state`](crate::CloudRequestEngineBuilder::set_state).
/// When a state is supplied the builder uses it and makes no discovery call; when
/// none is supplied the builder fetches the values from the cloud as it builds.
///
/// # Example
///
/// ```no_run
/// use fiftyone_cloud_request_engine::{CloudEngineState, CloudRequestEngine};
///
/// # fn load() -> Option<String> { None }
/// # fn save(_: &str) {}
/// // Inject a previously cached state if there is one, otherwise the builder
/// // fetches it from the cloud.
/// let cached: Option<CloudEngineState> =
///     load().and_then(|json| serde_json::from_str(&json).ok());
///
/// // The builder is kept (as `mut`) so its retained state can be exported after
/// // the build.
/// let mut builder = CloudRequestEngine::builder()
///     .resource_key("my-resource-key")
///     .set_state_opt(cached);
/// let _engine = builder.build().unwrap();
///
/// // Export the resolved state from the builder and persist it for the next cold
/// // start. Safe to run on every build, whatever the source of the data.
/// if let Some(state) = builder.export_state() {
///     save(&serde_json::to_string(&state).unwrap());
/// }
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloudEngineState {
    /// The accepted evidence keys (the `evidencekeys` discovery result).
    #[serde(default)]
    pub evidence_keys: Vec<EvidenceKeyEntry>,
    /// The accessible properties (the `accessibleproperties` discovery result).
    #[serde(default)]
    pub accessible_properties: LicensedProducts,
}

impl CloudEngineState {
    /// Build a snapshot from an evidence-key whitelist and the accessible
    /// properties. The evidence keys are sorted so the serialized form is stable
    /// regardless of the whitelist's internal ordering.
    pub(crate) fn from_parts(
        filter: &EvidenceKeyFilterWhitelist,
        properties: LicensedProducts,
    ) -> Self {
        let mut evidence_keys: Vec<EvidenceKeyEntry> = filter
            .whitelist()
            .map(|(key, order)| EvidenceKeyEntry {
                key: key.to_owned(),
                order,
            })
            .collect();
        evidence_keys.sort_by(|a, b| a.key.cmp(&b.key));
        CloudEngineState {
            evidence_keys,
            accessible_properties: properties,
        }
    }

    /// The evidence keys as an [`EvidenceKeyFilterWhitelist`], ready to seed an
    /// engine's accepted-evidence filter.
    pub(crate) fn evidence_filter(&self) -> EvidenceKeyFilterWhitelist {
        EvidenceKeyFilterWhitelist::with_orders(
            self.evidence_keys
                .iter()
                .map(|entry| (entry.key.as_str(), entry.order)),
        )
    }
}
