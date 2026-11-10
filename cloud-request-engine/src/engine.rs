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

//! The cloud request engine and its builder.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use fiftyone_caching::PutCache;
use fiftyone_pipeline_core::redact::{mask, redact_with};
use fiftyone_pipeline_core::{
    compare_keys, Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, EvidencePrefix, FlowData,
    FlowElement, FlowError, PropertyMetaData, PropertyValueType, Result, TypedKey,
};
use fiftyone_pipeline_engines::{AspectEngine, AspectPropertyMetaData, EngineDeployment};
use serde::de::IgnoredAny;

use crate::constants;
use crate::data::CloudRequestData;
use crate::http::{CloudHttpClient, CloudHttpRequest, HttpMethod};
use crate::properties::LicensedProducts;
use crate::recovery::{RecoveryConfig, RecoveryGate};
use crate::response::{cloud_error, validate_keeping_warnings, validate_response, ParsedResponse};
use crate::state::CloudEngineState;

/// The longest recovery period the engine holds, 100 years in seconds, so
/// that adding it to the current instant cannot overflow.
const RECOVERY_SECONDS_MAX: f64 = 100.0 * 365.25 * 24.0 * 60.0 * 60.0;

/// What a build with no client and no built-in one says, for a caller of
/// the builder in code.
const NO_CLIENT: &str = "no CloudHttpClient was supplied and the `reqwest-client` feature is \
                         not enabled, so the CloudRequestEngine has no HTTP transport; \
                         supply one with CloudRequestEngineBuilder::http_client(..) or \
                         enable the `reqwest-client` feature to use the built-in reqwest \
                         client";

/// The set of resolved endpoint URLs the engine talks to.
#[derive(Debug, Clone)]
struct Endpoints {
    /// The data (JSON) endpoint, POSTed to with the evidence form body.
    data: String,
    /// The accessible-properties endpoint, asked at build time.
    properties: String,
    /// The evidence-keys endpoint, asked at build time.
    evidence_keys: String,
}

/// An engine that makes requests to the 51Degrees cloud service.
///
/// On
/// [`FlowElement::process`] it filters the flow data's evidence down to the keys
/// the server accepts, strips each key's prefix following the evidence
/// precedence rules, POSTs the result as url-encoded form data to the `json`
/// endpoint, and stores the raw JSON response body in its element data under the
/// `cloud-response` data key. Downstream cloud aspect engines read that JSON.
///
/// # Credentials
///
/// A request needs a resource key, a license key, or both, and which of them
/// is present decides whether the caller names the properties it wants.
/// [`CloudRequestEngineBuilder::build`] settles the combination and refuses
/// the ones the service cannot answer as the caller intends.
///
/// A resource key states which properties it carries, and the service returns
/// all of them, adding any named in a property list sent with it.
///
/// ```no_run
/// # use fiftyone_cloud_request_engine::CloudRequestEngine;
/// let _engine = CloudRequestEngine::builder()
///     .resource_key("my-resource-key")
///     .build()
///     .unwrap();
/// ```
///
/// A property list beside a resource key adds the listed properties to those
/// the resource key carries. A license key alongside the resource key grants
/// the listed properties it covers, a listed property that no license grants
/// comes back with no value and the reason that it is a paid feature, and a
/// name the service does not know is not returned.
///
/// ```no_run
/// # use fiftyone_cloud_request_engine::CloudRequestEngine;
/// let _engine = CloudRequestEngine::builder()
///     .resource_key("my-resource-key")
///     .license_key("my-license-key")
///     .values(["device.hardwarevendor"])
///     .build()
///     .unwrap();
/// ```
///
/// A license key on its own carries no property list, so the caller names the
/// properties it wants and the service answers with those alone. The service
/// refuses a license-key request that names none.
///
/// ```no_run
/// # use fiftyone_cloud_request_engine::CloudRequestEngine;
/// let _engine = CloudRequestEngine::builder()
///     .license_key("my-license-key")
///     .values(["device.ismobile", "device.iscrawler"])
///     .build()
///     .unwrap();
/// ```
///
/// Settling the combination at build time means a request cannot fail for this
/// reason afterwards, so a downstream element always receives an answer rather
/// than meeting a configuration mistake as a failed request on the critical
/// path, where the pipeline has no data to work with.
///
/// # Discovery at build time
///
/// The accepted evidence keys (`evidencekeys`) and accessible properties
/// (`accessibleproperties`) depend on the keys, so they are fetched from the
/// cloud. The accessible properties are asked for with the resource key and,
/// when one is set, the license key, because a license key adds products
/// to those the resource key carries and the data request sends both. The
/// builder fetches both when it builds the engine, so a built engine is
/// normally ready to process with no further discovery.
///
/// A failure is handled as the
/// [start-up section](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/cloud-request-engine.md#start-up-activity)
/// of the specification says. A 4xx answer, such as an unknown resource key,
/// is a configuration error no retry can fix, so
/// [`CloudRequestEngineBuilder::build`] returns it. Any other failure, being
/// the service unreachable, a timeout, a 5xx answer or a body that cannot be
/// read, is logged and the build completes. Each process call
/// then retries the discovery that is missing and fails until it succeeds,
/// so a cloud outage at start-up cannot stop the application starting.
/// Until then [`CloudRequestEngine::public_properties`] and
/// [`CloudRequestEngine::accepted_evidence_keys`] return an error and the
/// engine's evidence key filter accepts nothing.
///
/// An engine holding a license key and no resource key is the exception,
/// because the accessible-properties endpoint takes a resource key and refuses
/// a request without one. The builder therefore does not call it, and
/// [`CloudRequestEngine::public_properties`] is empty for such an engine. A
/// downstream cloud aspect engine treats that the same way as a resource key
/// that grants it no product, reading the response JSON and inferring each
/// property's type from the value.
///
/// # Persisting discovered state
///
/// Both discovery results depend only on the keys, so they can be lifted
/// out of one engine and injected into another to skip the build-time fetch
/// entirely. This matters on a short-lived host such as a `wasm32-wasip1` edge
/// instance, which would otherwise repeat the two round-trips on every cold
/// start. Build an engine, call [`CloudRequestEngineBuilder::export_state`] on the
/// builder to obtain a serializable [`CloudEngineState`], persist it in the host's
/// store, and pass it to the next builder's
/// [`CloudRequestEngineBuilder::set_state`]. When a state is supplied the builder
/// uses it and makes no discovery call. The engine itself holds only the working
/// values it needs and knows nothing about the snapshot.
///
/// # Recovery mode
///
/// Repeated request failures within a window trip a [`RecoveryGate`], which then
/// short-circuits requests for a recovery period so a slow or failing cloud
/// cannot stall consumer requests. A failure counts when the request did not
/// complete, or the service answered with a status of 500 or more, a 429, or a
/// success status whose body could not be used. A status from 400 to 499 other
/// than 429 answers the request's own content, which any visitor can choose,
/// so it does not count, because the gate is shared by every visitor. See the
/// [recovery-mode section](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/cloud-request-engine.md#recovery-mode).
///
/// # Example
///
/// ```no_run
/// use std::sync::Arc;
/// use fiftyone_pipeline_core::{Evidence, Pipeline};
/// use fiftyone_cloud_request_engine::CloudRequestEngine;
///
/// let engine = CloudRequestEngine::builder()
///     .resource_key("my-resource-key")
///     .build()
///     .unwrap();
///
/// let pipeline = Pipeline::builder()
///     .add_element(Arc::new(engine))
///     .suppress_process_exceptions(true)
///     .build()
///     .unwrap();
///
/// let mut data = pipeline.create_flow_data_with(
///     Evidence::builder().add("header.user-agent", "Mozilla/5.0").build(),
/// );
/// data.process().unwrap();
/// let cloud = data.get(CloudRequestEngine::DATA_KEY).unwrap();
/// if let Some(json) = cloud.json_response() {
///     println!("cloud JSON: {json}");
/// }
/// ```
pub struct CloudRequestEngine {
    /// The credential the request authenticates with. At least one of this and
    /// `license_key` is set, which the builder settled.
    resource_key: Option<String>,
    /// The other credential, which either stands alone or, beside a resource
    /// key, grants listed properties the resource key does not carry.
    license_key: Option<String>,
    /// The resource key, and the license key where one is set, kept so that an
    /// error on the way out can have them taken out of it by value. Matching
    /// the value itself catches a credential that the shape-based rules in
    /// [`fiftyone_pipeline_core::redact`] would not recognise.
    secrets: Vec<String>,
    /// The properties the caller asked for, sent as the `values` parameter.
    /// Never empty with a license key alone, and empty or not with a resource
    /// key.
    values: Vec<String>,
    /// Whether the asked-for properties have been compared against a response
    /// yet. The comparison runs once per engine, see
    /// [`CloudRequestEngine::missing_values_warning`].
    values_checked: AtomicBool,
    cloud_request_origin: Option<String>,
    endpoints: Endpoints,
    http: Arc<dyn CloudHttpClient>,
    /// The request timeout the builder was given, which the built-in client
    /// applies. A client supplied to the builder keeps its own.
    timeout: Duration,
    recovery: RecoveryGate,

    /// An optional response cache, keyed by the request's evidence. When present,
    /// an identical request is served from the cache instead of calling the cloud.
    /// A response carrying a 51Did is never stored or served, see
    /// [`CloudRequestEngineBuilder::cache`]. A short-lived host (for example a
    /// `wasm32-wasip1` edge instance) can supply a cache backed by its own store,
    /// so responses survive across cold starts.
    response_cache: Option<Arc<dyn PutCache<String, String>>>,

    /// The core property metadata: `cloud-response`, `json-response` and
    /// `process-started`. Returned by [`FlowElement::properties`].
    properties: Vec<PropertyMetaData>,
    /// The aspect view of the same metadata.
    aspect_properties: Vec<AspectPropertyMetaData>,

    /// The accepted evidence keys the cloud advertises for this resource key,
    /// fetched from the `evidencekeys` endpoint or supplied via
    /// [`CloudRequestEngineBuilder::set_state`]. Set at build time, or after a
    /// transient failure then, by the first process call that fetches them.
    evidence_filter: OnceLock<EvidenceKeyFilterWhitelist>,
    /// The accessible properties for this resource key, from the
    /// `accessibleproperties` endpoint, set as the evidence keys are.
    public_properties: OnceLock<LicensedProducts>,
    /// Held while a process call retries discovery. Requests that arrive
    /// during an attempt wait for it and use its result if it succeeded. After
    /// a failed attempt each makes its own in turn, unless the failures opened
    /// a recovery period, which then stops them before anything is sent.
    discovery: Mutex<()>,
    /// The evidence key filter until the evidence keys are known, which
    /// accepts nothing.
    no_evidence_keys: EvidenceKeyFilterWhitelist,
}

impl CloudRequestEngine {
    /// The typed key under which this engine's [`CloudRequestData`] is stored in
    /// a flow data.
    pub const DATA_KEY: TypedKey<CloudRequestData> = TypedKey::new(constants::ELEMENT_DATA_KEY);

    /// Start building a cloud request engine.
    pub fn builder() -> CloudRequestEngineBuilder {
        CloudRequestEngineBuilder::new()
    }

    /// The resource key this engine sends with every request, or [`None`] when
    /// it authenticates with a license key instead.
    pub fn resource_key(&self) -> Option<&str> {
        self.resource_key.as_deref()
    }

    /// The properties this engine asks the cloud service for, as the caller
    /// named them. Empty when none were named, which a resource key allows.
    pub fn values(&self) -> &[String] {
        &self.values
    }

    /// The configured cloud-request origin, if any.
    pub fn cloud_request_origin(&self) -> Option<&str> {
        self.cloud_request_origin.as_deref()
    }

    /// The data endpoint URL POSTed to for each flow data.
    pub fn data_endpoint(&self) -> &str {
        &self.endpoints.data
    }

    /// The request timeout the engine was built with, where zero means no
    /// limit. The built-in client applies it, and a client supplied with
    /// [`CloudRequestEngineBuilder::http_client`] keeps its own.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// The failures that open a recovery period, the window they must fall
    /// in, and the period's length.
    pub fn recovery_config(&self) -> RecoveryConfig {
        self.recovery.config()
    }

    /// The accessible properties for the configured credential.
    ///
    /// The builder normally resolves these at build time (fetched from the
    /// cloud, or supplied via [`CloudRequestEngineBuilder::set_state`]), and
    /// this accessor never performs I/O. Downstream cloud aspect engines call it
    /// to discover which properties the credential grants. After a transient
    /// failure at build time it returns an [`Error::CloudRequest`] until a
    /// process call has fetched them.
    ///
    /// It is empty for an engine authenticating with a license key, because the
    /// accessible-properties endpoint takes a resource key and refuses a request
    /// without one, so there is nothing for the builder to fetch.
    pub fn public_properties(&self) -> Result<&LicensedProducts> {
        self.public_properties.get().ok_or_else(discovery_pending)
    }

    /// The accepted evidence keys for the configured resource key.
    ///
    /// Resolved as [`CloudRequestEngine::public_properties`] are, so this never
    /// performs I/O and returns an [`Error::CloudRequest`] until they are known.
    pub fn accepted_evidence_keys(&self) -> Result<&EvidenceKeyFilterWhitelist> {
        self.evidence_filter.get().ok_or_else(discovery_pending)
    }

    /// True once both discovery results are available, which is at build time
    /// unless discovery failed transiently then.
    pub fn has_loaded_metadata(&self) -> bool {
        self.evidence_filter.get().is_some() && self.public_properties.get().is_some()
    }

    /// The evidence key filter the engine sends evidence through, which
    /// accepts nothing until the evidence keys are known.
    fn accepted(&self) -> &EvidenceKeyFilterWhitelist {
        self.evidence_filter.get().unwrap_or(&self.no_evidence_keys)
    }

    /// Fetch whichever discovery result is missing after a transient failure
    /// at build time. Does nothing once both are known.
    ///
    /// The recovery gate is checked once, before either request is sent, and
    /// a call it stops returns the gate's error with no record, because no
    /// request was made. .NET also checks the gate before the code that logs.
    /// The evidence keys are fetched first and a failure stops there, so an
    /// outage costs one failed request per call. A failed request logs the
    /// discovery error, as .NET does, and is returned, so suppressed process
    /// exceptions cover it. The warm-up warning is for the build alone.
    fn discover(&self) -> Result<()> {
        if self.has_loaded_metadata() {
            return Ok(());
        }
        let _discovery = self
            .discovery
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.has_loaded_metadata() {
            return Ok(());
        }
        self.recovery
            .check()
            .map_err(|message| cloud_error(0, None, message))?;
        let origin = self.cloud_request_origin.as_deref();
        if self.evidence_filter.get().is_none() {
            let filter = fetch_evidence_keys(
                self.http.as_ref(),
                &self.recovery,
                &self.endpoints,
                origin,
                &self.secrets,
            )?;
            let _ = self.evidence_filter.set(filter);
        }
        if self.public_properties.get().is_none() {
            let products = match &self.resource_key {
                Some(key) => fetch_public_properties(
                    self.http.as_ref(),
                    &self.recovery,
                    &self.endpoints,
                    key,
                    self.license_key.as_deref(),
                    origin,
                    &self.secrets,
                ),
                None => Ok(LicensedProducts::default()),
            }?;
            let _ = self.public_properties.set(products);
        }
        Ok(())
    }

    /// Build the url-encoded form body for a flow data.
    ///
    /// The credentials lead the body, followed by the asked-for property list
    /// when there is one. Every evidence value then has its prefix stripped, so
    /// `query.user-agent` becomes `user-agent`. When two evidence values map to
    /// the same stripped key, the evidence precedence order (query > header >
    /// cookie > others) decides the winner. This realises the
    /// [processing rules](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/cloud-request-engine.md#processing).
    ///
    /// The property list travels in the form body rather than the query string,
    /// which the service also accepts, because a body has no practical length
    /// limit and a long list of property names would otherwise be at risk of one.
    fn build_content(&self, data: &FlowData) -> Vec<(String, String)> {
        let mut form: Vec<(String, String)> = Vec::new();
        // The builder settled which credentials are present, so at least one
        // of these two writes a field.
        if let Some(resource_key) = &self.resource_key {
            form.push((
                constants::RESOURCE_PARAMETER.to_owned(),
                resource_key.clone(),
            ));
        }
        if let Some(license) = &self.license_key {
            form.push((constants::LICENSE_PARAMETER.to_owned(), license.clone()));
        }
        if !self.values.is_empty() {
            form.push((
                constants::VALUES_PARAMETER.to_owned(),
                self.values.join(","),
            ));
        }

        // Collect the evidence the server accepts, so only the keys the
        // accepted-evidence filter includes are sent.
        let accepted = self.accepted();
        let mut entries: Vec<(&str, &str)> = data
            .evidence()
            .iter()
            .filter(|(key, _)| accepted.include(key))
            .collect();

        // Sort so that lower-precedence evidence is written first and
        // higher-precedence evidence overwrites it. `compare_keys` orders by
        // precedence ascending (query first), so reverse it to apply query last.
        entries.sort_by(|(left, _), (right, _)| compare_keys(left, right).reverse());

        // Strip prefixes and de-duplicate on the stripped key, keeping the last
        // (highest-precedence) writer.
        let mut stripped: Vec<(String, String)> = Vec::new();
        for &(key, value) in &entries {
            let field = strip_prefix(key);
            // The engine writes the credential and the property list itself, and
            // the service advertises `query.values` as accepted evidence, so
            // evidence of that name would put a second `values` field in the body
            // and leave which list applied to the request undecided. The engine's
            // own fields win, which also keeps the request in step with the list
            // the missing-property check compares against.
            if is_engine_owned_field(&field) {
                continue;
            }
            if let Some(existing) = stripped.iter_mut().find(|(k, _)| k == &field) {
                // Query evidence is expected to replace what the request
                // carried, so only other clashes are logged.
                if !has_prefix(key, EvidencePrefix::Query) {
                    log_conflict(key, value, &field, &entries);
                }
                existing.1 = value.to_owned();
            } else {
                stripped.push((field, value.to_owned()));
            }
        }
        form.extend(stripped);
        form
    }

    /// Compare the properties the caller asked for against the ones the response
    /// carried, returning a warning naming any that did not arrive.
    ///
    /// The cloud service leaves out a property the credential does not cover
    /// without saying so. Asked for on its own it answers `200` with a top-level
    /// `errors` list, which [`validate_response`] already raises, but asked for
    /// alongside a property the credential does cover it answers `200` carrying
    /// the covered one and nothing at all about the one it dropped. Silence is
    /// then the only symptom, so the engine looks for it rather than leaving a
    /// caller to work out why a property it asked for never appears. With a
    /// resource key and no license key, a listed property the credential does
    /// not cover comes back with no value and the reason that it is a paid
    /// feature, which counts as carried, so the warning then names only
    /// properties the service does not know.
    ///
    /// The comparison runs once per engine. Its outcome is decided by the
    /// credential and the asked-for list, both fixed when the engine was built,
    /// so repeating it learns nothing. Evidence does not change it either,
    /// because a property that the evidence cannot populate still comes back as
    /// a key with a null value and a reason beside it.
    fn missing_values_warning(&self, json: &str) -> Option<String> {
        if self.values.is_empty() || self.values_checked.swap(true, Ordering::Relaxed) {
            return None;
        }
        // The body reached here through validate_response, which parses it, so a
        // body that will not parse cannot arrive. Treat one as nothing to report
        // rather than disturbing a request that otherwise succeeded.
        let body: serde_json::Value = serde_json::from_str(json).ok()?;
        let missing: Vec<&str> = self
            .values
            .iter()
            .filter(|asked| !response_carries(&body, asked))
            .map(String::as_str)
            .collect();
        if missing.is_empty() {
            return None;
        }
        if self.resource_key.is_some() && self.license_key.is_none() {
            // A listed property no license grants comes back null with its
            // reason beside a resource key alone, so only an unknown name is
            // left out, as the cloud request engine specification says.
            return Some(format!(
                "the cloud service did not return {} of the {} properties this \
                 engine asked for: {}. With a resource key and no license key, a \
                 listed property no license grants comes back with no value and \
                 its reason, so a name left out is not a property the cloud \
                 service knows. Check the names, or drop them from the property \
                 list the engine was built with. Reported once per engine.",
                missing.len(),
                self.values.len(),
                missing.join(", ")
            ));
        }
        Some(format!(
            "the cloud service did not return {} of the {} properties this \
             engine asked for: {}. This is an entitlement matter rather than a \
             fault. The service answers 200 and says nothing when it leaves out \
             a property the credential does not cover, so the request itself \
             succeeded and the remaining properties are present. Check that the \
             credential covers the named properties, or drop them from the \
             property list the engine was built with. Reported once per engine.",
            missing.len(),
            self.values.len(),
            missing.join(", ")
        ))
    }

    /// Add a warning naming any asked-for property the response did not carry,
    /// so the difference travels with the response's own warnings.
    fn add_missing_values_warning(&self, parsed: &mut ParsedResponse) {
        if let Some(warning) = self.missing_values_warning(&parsed.json) {
            // Also written to stderr, because a consumer that never reads the
            // warnings would otherwise have no sign of it at all, and silence is
            // the very fault being reported. At most one line per engine.
            eprintln!("51Degrees cloud request engine: {warning}");
            parsed.warnings.push(warning);
        }
    }

    /// Log and record that a cloud engine did not process because this engine
    /// left no response, as .NET's cloud aspect engines do.
    ///
    /// Logs at info level naming `engine_name`, under `log_target`, and adds a
    /// flow error for `element_data_key` that neither fails processing nor is
    /// logged again. Pass `module_path!()` as `log_target`, so a log filter on
    /// the calling engine's crate shows the line, as .NET writes it through
    /// the engine's own logger. This engine records the failed request itself,
    /// so the record only says which engines had nothing to read.
    pub fn record_not_processed(
        data: &mut FlowData,
        engine_name: &str,
        element_data_key: &str,
        log_target: &str,
    ) {
        log::info!(
            target: log_target,
            "The '{engine_name}' did not process as the JSON response from the \
             CloudRequestEngine was null or empty. Please refer to errors generated by \
             the CloudRequestEngine in the logs as this indicates an error occurred there."
        );
        data.add_error(
            FlowError::with_should_throw(
                element_data_key,
                Error::Pipeline {
                    message: format!(
                        "The '{engine_name}' did not process because the JSON response from \
                         the CloudRequestEngine was null or empty. This indicates that the \
                         cloud request failed."
                    ),
                },
                false,
            ),
            false,
        );
    }
}

/// The cloud service's warnings with the engine's own credentials masked. The
/// shape rules that cleaned them cannot find a licence key, which has no
/// shape of its own, so the values the engine holds are masked here.
fn mask_warnings(warnings: Vec<String>, secrets: &[String]) -> Vec<String> {
    warnings
        .into_iter()
        .map(|warning| redact_with(&warning, secrets).into_owned())
        .collect()
}

/// Log each warning the cloud service returned, on every response, as .NET
/// does. Called before the engine adds its own missing property warning,
/// which goes to standard error instead.
fn log_cloud_warnings(warnings: &[String]) {
    for warning in warnings {
        log::warn!("{warning}");
    }
}

/// Whether a form field is one the engine writes itself, so evidence of the same
/// name is left out of the body rather than duplicating it.
fn is_engine_owned_field(field: &str) -> bool {
    matches!(
        field,
        constants::RESOURCE_PARAMETER | constants::LICENSE_PARAMETER | constants::VALUES_PARAMETER
    )
}

/// Whether the cloud response carries the property that `asked` names.
///
/// A name is `product.property`, matching the response's shape of one object per
/// product. The service lowercases both parts in its answer whatever case the
/// caller wrote, so the comparison ignores case. A property that is present but
/// null counts as carried, because the service returns a `<property>nullreason`
/// beside it saying why, which is an answer rather than a silent omission.
fn response_carries(body: &serde_json::Value, asked: &str) -> bool {
    let Some(products) = body.as_object() else {
        return false;
    };
    match asked.split_once('.') {
        Some((product, property)) => products
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(product))
            .and_then(|(_, value)| value.as_object())
            .is_some_and(|properties| {
                properties
                    .keys()
                    .any(|name| name.eq_ignore_ascii_case(property))
            }),
        // A name with no product part is not something the service can look up
        // either, so the response will not carry it and reporting it is right.
        None => products.keys().any(|name| name.eq_ignore_ascii_case(asked)),
    }
}

/// Whether a response body carries 51Did values, being a top-level member
/// named [`constants::IDENTIFIER_MEMBER`] in any case.
///
/// Such a body answers the one request it was made for, so it is never stored
/// in the response cache or served from it. A body that is not a JSON object
/// cannot be checked, so it counts as carrying them.
fn carries_identifier(json: &str) -> bool {
    match serde_json::from_str::<HashMap<String, IgnoredAny>>(json) {
        Ok(members) => members
            .keys()
            .any(|name| name.eq_ignore_ascii_case(constants::IDENTIFIER_MEMBER)),
        Err(_) => true,
    }
}

/// Validate that a resolved endpoint is a well-formed absolute http(s) URL.
///
/// The base endpoint can come from the builder's `endpoint`/`*_endpoint` setters,
/// the `51DEGREES_CLOUD_ENDPOINT` environment variable, or the default, so a value with
/// no scheme, an empty host, or stray whitespace is caught here rather than
/// producing a malformed request URL later. This checks the format only: it
/// cannot know a deployment-specific path (such as `/api/v4`) is missing, since a
/// self-hosted cloud may serve the endpoints at a different path.
fn validate_endpoint_url(url: &str, what: &str) -> Result<()> {
    let after_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    // A host is present when there is something after the scheme that does not
    // immediately start the path, so `https://` and `http:///path` are rejected.
    let has_host = matches!(after_scheme, Some(rest) if !rest.is_empty() && !rest.starts_with('/'));
    if has_host {
        Ok(())
    } else {
        Err(Error::configuration(format!(
            "the {what} '{url}' is not a valid absolute URL; it must begin with \
             http:// or https:// and include a host, for example \
             'https://cloud.51degrees.com/api/v4/'"
        )))
    }
}

/// Build a stable cache key from a request's url-encoded form.
///
/// The form holds the credential, the asked-for property list when there is one,
/// and the stripped evidence fields. The pairs are sorted so the key is
/// independent of evidence insertion order. Each name and each value is then
/// written after its length in bytes, so no value can be read as the end of its
/// own pair and the start of another, whatever characters it holds. Two requests
/// therefore share a key only when their credential, property list and evidence
/// are the same. Including the credential and the property list means a cache
/// shared between engines never returns one engine's response for another's
/// request, which matters because two engines asking for different properties
/// get different answers to the same evidence.
fn cache_key(form: &[(String, String)]) -> String {
    let mut pairs: Vec<&(String, String)> = form.iter().collect();
    pairs.sort();
    let mut key = String::new();
    for (name, value) in pairs {
        for part in [name, value] {
            key.push_str(&part.len().to_string());
            key.push(':');
            key.push_str(part);
        }
    }
    key
}

/// Strip a known evidence prefix from a key, leaving the field name. A key with
/// no recognized `prefix.field` separator is returned unchanged.
fn strip_prefix(key: &str) -> String {
    match key.split_once('.') {
        Some((prefix, field)) if EvidencePrefix::parse(prefix).is_some() => field.to_owned(),
        // An unknown prefix is still split off, taking the part after the first
        // separator as the suffix.
        Some((_, field)) => field.to_owned(),
        None => key.to_owned(),
    }
}

/// Whether an evidence key starts with `prefix` and a separator.
fn has_prefix(key: &str, prefix: EvidencePrefix) -> bool {
    key.split_once('.')
        .is_some_and(|(start, _)| start.eq_ignore_ascii_case(prefix.as_str()))
}

/// Log a warning that `key` replaces evidence sent under the same `field`,
/// naming every other entry whose key ends with that field, as .NET does.
/// A credential can be among them, as `query.resource` ends with `source`, so
/// each value is written as [`shown_value`] gives it.
fn log_conflict(key: &str, value: &str, field: &str, entries: &[(&str, &str)]) {
    let conflicts: Vec<String> = entries
        .iter()
        .filter(|(other, _)| *other != key && other.ends_with(field))
        .map(|(other, other_value)| format!("{other}={}", shown_value(other, other_value)))
        .collect();
    log::warn!(
        "'{key}={}' evidence conflicts with '{}'",
        shown_value(key, value),
        conflicts.join("', '")
    );
}

/// An evidence value as a log record shows it. The value of a resource key
/// or licence key field is masked to its last 4 characters.
fn shown_value<'a>(key: &str, value: &'a str) -> Cow<'a, str> {
    let field = strip_prefix(key);
    let is_credential = [constants::RESOURCE_PARAMETER, constants::LICENSE_PARAMETER]
        .iter()
        .any(|name| field.eq_ignore_ascii_case(name));
    if is_credential {
        Cow::Owned(mask(value))
    } else {
        Cow::Borrowed(value)
    }
}

impl FlowElement for CloudRequestEngine {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        // Record that the engine started before making the request, so a
        // consumer can tell the engine ran even if the request then fails.
        data.get_or_add(Self::DATA_KEY, || {
            CloudRequestData::new(constants::ELEMENT_DATA_KEY).with_process_started(true)
        })?;

        // Discovery normally completed at build time. After a transient
        // failure then, it is retried here and the request fails until it
        // succeeds.
        self.discover()?;

        let form = self.build_content(data);

        // When a response cache is configured, key on this request's evidence and
        // serve a stored response without calling the cloud. The lookup runs
        // before the recovery gate, so a cache hit also avoids being blocked
        // during a recovery period. A short-lived host supplies its own cache so
        // hits survive across cold starts. A stored body carrying 51Did values,
        // which a store shared with other writers can hold, was made for another
        // request, so it is not served and the cloud is asked instead.
        let cache_key = self.response_cache.as_ref().map(|_| cache_key(&form));
        if let (Some(cache), Some(key)) = (&self.response_cache, &cache_key) {
            if let Some(body) = cache.get(key).filter(|body| !carries_identifier(body)) {
                // The body validated when it was stored, so re-validating it
                // reproduces the same JSON and warnings a live response would,
                // without a network call.
                let cached = crate::http::CloudHttpResponse {
                    status: 200,
                    body,
                    retry_after: None,
                };
                if let Ok(mut parsed) = validate_response(&cached, &self.endpoints.data, true) {
                    // A cached body is as good a sample as a live one for the
                    // asked-for property check, and on a short-lived host the
                    // first response after a cold start may well be this one.
                    // A live response logs its warnings as it is validated, so
                    // a cached one is logged here to match.
                    parsed.warnings = mask_warnings(parsed.warnings, &self.secrets);
                    log_cloud_warnings(&parsed.warnings);
                    self.add_missing_values_warning(&mut parsed);
                    if let Some(cloud) = data.get_mut_cloud() {
                        cloud.set_cache_hit();
                        cloud.set_json_response(parsed.json);
                        if !parsed.warnings.is_empty() {
                            cloud.set_warnings(parsed.warnings);
                        }
                    }
                    return Ok(());
                }
            }
        }

        let request = CloudHttpRequest {
            method: HttpMethod::Post,
            url: self.endpoints.data.clone(),
            form,
            origin: self.cloud_request_origin.clone(),
        };
        let mut parsed = send_and_validate(
            self.http.as_ref(),
            &self.recovery,
            Gate::Check,
            &request,
            true,
            &self.secrets,
        )?;
        self.add_missing_values_warning(&mut parsed);

        // Store the successful response so the next identical request is a hit,
        // unless it carries 51Did values, which were created for this request.
        if let (Some(cache), Some(key)) = (&self.response_cache, cache_key) {
            if !carries_identifier(&parsed.json) {
                cache.put(key, parsed.json.clone());
            }
        }

        if let Some(cloud) = data.get_mut_cloud() {
            cloud.set_json_response(parsed.json);
            // Warnings from the cloud are non-fatal. They are stored on the
            // element data so a consumer can surface them, rather than the
            // `warnings` array being treated as an error.
            if !parsed.warnings.is_empty() {
                cloud.set_warnings(parsed.warnings);
            }
        }
        Ok(())
    }

    fn data_key(&self) -> &str {
        constants::ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.accepted()
    }

    fn is_evidence_key_filter_complete(&self) -> bool {
        self.evidence_filter.get().is_some()
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

impl AspectEngine for CloudRequestEngine {
    fn data_source_tier(&self) -> &str {
        // Cloud engines have no on-premise data tier.
        "cloud"
    }

    fn deployment(&self) -> EngineDeployment {
        EngineDeployment::Cloud
    }

    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &self.aspect_properties
    }

    fn has_loaded_properties(&self) -> bool {
        // The engine's own three properties are fixed, so they are always
        // loaded.
        true
    }
}

/// Helper extension on [`FlowData`] for fetching this engine's mutable data.
///
/// The data was inserted earlier in `process`, so this only re-borrows it
/// mutably. It is a free function rather than a trait so it stays private to the
/// engine.
trait CloudDataAccess {
    fn get_mut_cloud(&mut self) -> Option<&mut CloudRequestData>;
}

impl CloudDataAccess for FlowData {
    fn get_mut_cloud(&mut self) -> Option<&mut CloudRequestData> {
        // get_or_add returns a &mut T, so re-add with a no-op create closure to
        // obtain the existing instance mutably.
        self.get_or_add(CloudRequestEngine::DATA_KEY, || {
            CloudRequestData::new(constants::ELEMENT_DATA_KEY).with_process_started(true)
        })
        .ok()
    }
}

/// A fluent builder for [`CloudRequestEngine`] instances.
///
/// One credential is required, either a resource key on its own or a license key
/// with the properties the caller wants, and everything else has a sensible
/// default. See [`CloudRequestEngineBuilder::build`] for the combinations it
/// refuses and why. Set an alternative `endpoint` to target a different cloud
/// deployment, or set the individual endpoints for full control. Recovery
/// tunables and the HTTP client can be overridden, the latter chiefly for
/// testing.
pub struct CloudRequestEngineBuilder {
    resource_key: Option<String>,
    license_key: Option<String>,
    values: Vec<String>,
    cloud_request_origin: Option<String>,
    endpoint: Option<String>,
    data_endpoint: Option<String>,
    properties_endpoint: Option<String>,
    evidence_keys_endpoint: Option<String>,
    timeout: Duration,
    failures_to_enter_recovery: u32,
    failures_window: Duration,
    recovery: Duration,
    http: Option<Arc<dyn CloudHttpClient>>,
    /// What the build says when it has no client and no built-in one.
    no_client_message: &'static str,
    cloud_state: Option<CloudEngineState>,
    response_cache: Option<Arc<dyn PutCache<String, String>>>,
}

impl CloudRequestEngineBuilder {
    /// Create a builder with the specification defaults.
    pub fn new() -> Self {
        CloudRequestEngineBuilder {
            resource_key: None,
            license_key: None,
            values: Vec::new(),
            cloud_request_origin: None,
            endpoint: None,
            data_endpoint: None,
            properties_endpoint: None,
            evidence_keys_endpoint: None,
            timeout: Duration::from_secs(constants::TIMEOUT_DEFAULT_SECONDS),
            failures_to_enter_recovery: constants::FAILURES_TO_ENTER_RECOVERY_DEFAULT,
            failures_window: Duration::from_secs(constants::FAILURES_WINDOW_SECONDS_DEFAULT),
            recovery: Duration::from_secs_f64(constants::RECOVERY_SECONDS_DEFAULT),
            http: None,
            no_client_message: NO_CLIENT,
            cloud_state: None,
            response_cache: None,
        }
    }

    /// Set the resource key, one of the two ways to authenticate. A resource key
    /// authenticates the request and specifies which properties are returned,
    /// and a property list given with [`CloudRequestEngineBuilder::values`] adds
    /// to them. Create one for free at
    /// <https://configure.51degrees.com?utm_source=code&utm_medium=comment&utm_campaign=51degrees-rust&utm_content=cloud-request-engine-src-engine.rs&utm_term=resource_key>.
    ///
    /// A resource key is public by design, since it travels to the browser inside
    /// a script URL, so it is scoped to what a page is meant to read. A caller
    /// that runs only on a server and wants to keep its credential off the client
    /// uses [`CloudRequestEngineBuilder::license_key`] instead.
    pub fn resource_key(mut self, resource_key: impl Into<String>) -> Self {
        self.resource_key = Some(resource_key.into());
        self
    }

    /// Set the license key, the other way to authenticate, used either
    /// alongside a resource key or instead of one.
    ///
    /// Alongside a resource key it grants the properties of the property list
    /// that it covers, on top of those the resource key carries, and with no
    /// property list it adds none. On its own it names no
    /// properties, so an engine built with it and no resource key must also be
    /// given the properties it wants with
    /// [`CloudRequestEngineBuilder::values`]. The cloud service refuses a
    /// license-key request that names none.
    pub fn license_key(mut self, license_key: impl Into<String>) -> Self {
        self.license_key = Some(license_key.into());
        self
    }

    /// Name the properties the engine asks the cloud service for, as
    /// `product.property` names such as `device.ismobile`. Calling it again
    /// replaces the list rather than adding to it.
    ///
    /// With a license key alone the service returns those properties and no
    /// others, which keeps a response to what the caller actually reads. With a
    /// resource key it adds them to the properties the resource key carries. A
    /// listed property that no license grants, through the resource key or a
    /// license key given alongside it, comes back with no value and the reason
    /// that it is a paid feature, and a name the service does not know is not
    /// returned.
    ///
    /// A property that does not come back at all is left out of the answer
    /// without comment, so the engine compares what it asked for against what
    /// arrived and reports the difference once, as a warning on its element data
    /// and one line on stderr. With a resource key and no license key the
    /// difference is only a name the service does not know, because a listed
    /// property no license grants comes back with no value and counts as
    /// carried.
    pub fn values<I, S>(mut self, values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.values = values.into_iter().map(Into::into).collect();
        self
    }

    /// Set the value of the `Origin` header sent with each request. The cloud
    /// service checks this against the origins the resource key permits.
    pub fn cloud_request_origin(mut self, origin: impl Into<String>) -> Self {
        self.cloud_request_origin = Some(origin.into());
        self
    }

    /// Set the base endpoint, from which the data, properties and evidence-keys
    /// endpoints are derived by appending `json`, `accessibleproperties` and
    /// `evidencekeys`. A trailing slash is added if missing.
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Set the data (JSON) endpoint explicitly, overriding the one derived from
    /// the base endpoint.
    pub fn data_endpoint(mut self, url: impl Into<String>) -> Self {
        self.data_endpoint = Some(url.into());
        self
    }

    /// Set the accessible-properties endpoint explicitly.
    pub fn properties_endpoint(mut self, url: impl Into<String>) -> Self {
        self.properties_endpoint = Some(url.into());
        self
    }

    /// Set the evidence-keys endpoint explicitly.
    pub fn evidence_keys_endpoint(mut self, url: impl Into<String>) -> Self {
        self.evidence_keys_endpoint = Some(url.into());
        self
    }

    /// Set the request timeout. A zero timeout disables the timeout. Defaults to
    /// two seconds.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the request timeout in seconds, a convenience over
    /// [`CloudRequestEngineBuilder::timeout`].
    pub fn timeout_seconds(mut self, seconds: u64) -> Self {
        self.timeout = Duration::from_secs(seconds);
        self
    }

    /// Set the number of failures, within the failures window, that opens a
    /// recovery period. Clamped to the permitted range.
    pub fn failures_to_enter_recovery(mut self, failures: u32) -> Self {
        self.failures_to_enter_recovery = failures.clamp(
            constants::FAILURES_TO_ENTER_RECOVERY_MIN,
            constants::FAILURES_TO_ENTER_RECOVERY_MAX,
        );
        self
    }

    /// Set the window within which the failure threshold must be reached.
    pub fn failures_window_seconds(mut self, seconds: u64) -> Self {
        self.failures_window = Duration::from_secs(seconds.max(1));
        self
    }

    /// Set the recovery-period duration. Zero, a negative value or one that is
    /// not a number disables recovery mode, and a period longer than 100 years
    /// is held at 100 years.
    pub fn recovery_seconds(mut self, seconds: f64) -> Self {
        self.recovery = if seconds > 0.0 {
            Duration::from_secs_f64(seconds.min(RECOVERY_SECONDS_MAX))
        } else {
            Duration::ZERO
        };
        self
    }

    /// Supply the [`CloudHttpClient`] the engine sends requests through.
    ///
    /// Required unless the `reqwest-client` feature is enabled, in which case
    /// leaving it unset uses the built-in blocking reqwest client with the
    /// configured timeout. A consumer on a target without reqwest (for example
    /// `wasm32-wasip1`) supplies its own synchronous client here.
    pub fn http_client(mut self, client: Arc<dyn CloudHttpClient>) -> Self {
        self.http = Some(client);
        self
    }

    /// Say `message` when the build has no client and no built-in one, for
    /// a caller that gives the client some other way than
    /// [`Self::http_client`].
    pub(crate) fn no_client_message(mut self, message: &'static str) -> Self {
        self.no_client_message = message;
        self
    }

    /// Supply a response cache, keyed by the request's evidence.
    ///
    /// When set, the engine serves an identical request from the cache instead of
    /// calling the cloud, and stores each successful response for the next
    /// equivalent request. The key combines the credential and the asked-for
    /// property list with the accepted, prefix-stripped evidence, so a different
    /// credential, a different property list or different evidence never
    /// collides. The cached value is the raw JSON response body.
    ///
    /// A response carrying 51Did values is never stored, and one found in the
    /// cache is never served. The cloud creates a 51Did afresh for each request
    /// that asks for one, and two visitors can present the same evidence, so a
    /// stored copy would hand one visitor the 51Did created for another. A
    /// request that carries no `id.usage` evidence asks for no 51Did, so in a
    /// pipeline that asks for one on some requests the cache still serves the
    /// others, while each request that asks for one reaches the cloud.
    ///
    /// Any [`fiftyone_caching::PutCache`] works, so a consumer chooses the policy:
    /// the in-process [`fiftyone_caching::LruCache`] for a long-lived host, or a
    /// custom implementation backed by the host's own store on a short-lived host
    /// (for example a `wasm32-wasip1` edge instance), so cached responses survive
    /// across cold starts rather than being lost when the instance is discarded.
    pub fn cache(mut self, cache: Arc<dyn PutCache<String, String>>) -> Self {
        self.response_cache = Some(cache);
        self
    }

    /// Provide a previously exported [`CloudEngineState`], so the builder uses
    /// those accepted evidence keys and accessible properties instead of fetching
    /// them from the cloud.
    ///
    /// This is the inject side of the round-trip with
    /// [`CloudRequestEngineBuilder::export_state`]. When a state is supplied the builder
    /// makes no `evidencekeys` or `accessibleproperties` request, which lets a
    /// short-lived host (for example a `wasm32-wasip1` edge instance) skip
    /// discovery on every cold start. When no state is supplied the builder
    /// fetches both documents from the cloud as it builds the engine.
    pub fn set_state(mut self, state: CloudEngineState) -> Self {
        self.cloud_state = Some(state);
        self
    }

    /// Provide a [`CloudEngineState`] when one is available, otherwise let the
    /// builder fetch it from the cloud.
    ///
    /// A convenience over [`CloudRequestEngineBuilder::set_state`] for the common
    /// pattern of reading a cached state that may be absent: passing [`None`]
    /// leaves the builder to discover the values itself, so the same build code
    /// works whether or not a cached snapshot exists.
    pub fn set_state_opt(mut self, state: Option<CloudEngineState>) -> Self {
        self.cloud_state = state;
        self
    }

    /// Build the engine, resolving its discovery state.
    ///
    /// Unless a [`CloudEngineState`] was supplied with
    /// [`CloudRequestEngineBuilder::set_state`], the builder fetches the
    /// `evidencekeys` and `accessibleproperties` documents from the cloud here, so
    /// the returned engine is normally ready to process with no further
    /// discovery. After a transient failure the engine retries on process, as
    /// [`CloudRequestEngine`] describes under "Discovery at build time".
    ///
    /// The builder takes `&mut self` and retains the resolved state, so after a
    /// build that resolved it [`CloudRequestEngineBuilder::export_state`] returns the
    /// state for persistence. The engine itself holds only the working values it
    /// needs to process flow data and knows nothing about the state snapshot.
    ///
    /// # Credentials
    ///
    /// A resource key, with or without a license key and with or without a
    /// property list, and a license key with the properties the caller wants
    /// are all accepted.
    /// Everything else is refused here, with a message naming the setting to
    /// change.
    ///
    /// Neither credential leaves nothing to authenticate with. A license key
    /// without a property list is refused by the cloud service itself on every
    /// request, so catching it here turns a total failure in service into one
    /// configuration error a deployment meets before it serves anybody.
    ///
    /// Settling all of this at build time is what lets a downstream element rely
    /// on receiving an answer, since a request can no longer fail for a reason
    /// the configuration already decided.
    ///
    /// # Errors
    ///
    /// Returns an [`Error::PipelineConfiguration`] if the credentials are not one
    /// of the accepted combinations, or if no [`CloudHttpClient`] was
    /// supplied and the `reqwest-client` feature is not enabled (there is then no
    /// transport to fall back to). Returns an [`Error::CloudRequest`] if the
    /// built-in client cannot be constructed, or if a discovery request is
    /// answered with a 4xx status. Any other discovery failure is logged as a
    /// warning and left to the first process call to retry. A consumer that
    /// must build with no discovery request at all supplies a cached state
    /// with `set_state`. A discovery request that fails, or is answered with
    /// an error, is also logged at error level.
    pub fn build(&mut self) -> Result<CloudRequestEngine> {
        let (resource_key, license_key, values) = self.resolve_credentials()?;

        let endpoints = self.resolve_endpoints();
        // Validate the resolved endpoint URLs before any request is attempted, so
        // a malformed endpoint (from the builder, the 51DEGREES_CLOUD_ENDPOINT
        // environment variable or a consumer's own config) fails the build with a
        // clear message rather than surfacing later as a confusing request error.
        validate_endpoint_url(&endpoints.data, "data endpoint")?;
        validate_endpoint_url(&endpoints.properties, "accessible-properties endpoint")?;
        validate_endpoint_url(&endpoints.evidence_keys, "evidence-keys endpoint")?;

        let http: Arc<dyn CloudHttpClient> = match &self.http {
            Some(client) => Arc::clone(client),
            None => default_http_client(self.timeout, self.no_client_message)?,
        };

        let recovery = RecoveryGate::new(RecoveryConfig {
            failures_to_enter_recovery: self.failures_to_enter_recovery,
            window: self.failures_window,
            recovery: self.recovery,
        });

        let (properties, aspect_properties) = build_property_metadata();

        // Resolve the discovery state and retain it on the builder. A supplied
        // state is used verbatim and no request is made. Otherwise the builder
        // fetches both discovery documents from the cloud now, and keeps the
        // result when both arrive so it can export the state afterwards.
        let secrets = credentials(resource_key.as_deref(), license_key.as_deref());
        let evidence_filter = OnceLock::new();
        let public_properties = OnceLock::new();
        if let Some(state) = &self.cloud_state {
            let _ = evidence_filter.set(state.evidence_filter());
            let _ = public_properties.set(state.accessible_properties.clone());
        } else {
            let origin = self.cloud_request_origin.as_deref();
            let evidence =
                fetch_evidence_keys(http.as_ref(), &recovery, &endpoints, origin, &secrets);
            let products = match &resource_key {
                Some(key) => fetch_public_properties(
                    http.as_ref(),
                    &recovery,
                    &endpoints,
                    key,
                    license_key.as_deref(),
                    origin,
                    &secrets,
                ),
                // The accessible-properties endpoint takes a resource key and
                // refuses a request without one, so an engine holding only a
                // license key has nothing to fetch and starts with none. A
                // downstream cloud aspect engine already handles empty
                // metadata, reading the response JSON and inferring each
                // property's type from its value, which is what it does today
                // for a resource key that grants it no product.
                None => Ok(LicensedProducts::default()),
            };
            // Both requests are made first, so a 4xx answer to either fails
            // the build even when the other failed for a transient reason.
            // Neither checks the recovery gate, so a failure that opens a
            // recovery period cannot stop the second request, which the
            // specification says the build must attempt.
            let evidence = definitive_fails_build(evidence)?;
            let products = definitive_fails_build(products)?;
            match (evidence, products) {
                (Ok(evidence), Ok(products)) => {
                    self.cloud_state =
                        Some(CloudEngineState::from_parts(&evidence, products.clone()));
                    let _ = evidence_filter.set(evidence);
                    let _ = public_properties.set(products);
                }
                (evidence, products) => {
                    if let Ok(evidence) = evidence {
                        let _ = evidence_filter.set(evidence);
                    }
                    if let Ok(products) = products {
                        let _ = public_properties.set(products);
                    }
                    log::warn!(
                        "Failed to warm up the cloud request engine due to a transient \
                         error. The engine will retry the discovery requests when it is \
                         first used."
                    );
                }
            }
        }

        Ok(CloudRequestEngine {
            resource_key,
            license_key,
            secrets,
            values,
            values_checked: AtomicBool::new(false),
            cloud_request_origin: self.cloud_request_origin.clone(),
            endpoints,
            http,
            timeout: self.timeout,
            recovery,
            response_cache: self.response_cache.clone(),
            properties,
            aspect_properties,
            evidence_filter,
            public_properties,
            discovery: Mutex::new(()),
            no_evidence_keys: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        })
    }

    /// Export the discovery state the builder resolved during
    /// [`build`](CloudRequestEngineBuilder::build).
    ///
    /// After a successful build the builder holds the accepted evidence keys and
    /// accessible properties, whether it fetched them from the cloud or they were
    /// supplied with [`set_state`](CloudRequestEngineBuilder::set_state). Persist
    /// the returned [`CloudEngineState`] in a host store (a config or key-value
    /// store, a baked-in const, and so on) and inject it into a later builder to
    /// skip the build-time fetch.
    ///
    /// Returns [`None`] when no state has been resolved, that is when neither
    /// [`set_state`](CloudRequestEngineBuilder::set_state) has been called nor a
    /// build has fetched both discovery documents, as after a transient failure.
    pub fn export_state(&self) -> Option<CloudEngineState> {
        self.cloud_state.clone()
    }

    /// Check the credentials and property list against what the cloud service
    /// answers, returning them ready for the engine.
    ///
    /// Each value is trimmed, and one that is empty or only whitespace counts as
    /// not supplied, because a key read from an environment variable or a config
    /// file often arrives with a stray newline and sending it would fail the
    /// request for a reason the message would not explain.
    ///
    /// See [`CloudRequestEngineBuilder::build`] for why each refused combination
    /// is refused.
    fn resolve_credentials(&self) -> Result<(Option<String>, Option<String>, Vec<String>)> {
        fn supplied(value: &Option<String>) -> Option<String> {
            value
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        }

        let resource_key = supplied(&self.resource_key);
        let license_key = supplied(&self.license_key);
        let values: Vec<String> = self
            .values
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect();

        match (&resource_key, &license_key) {
            (None, None) => Err(Error::configuration(
                "a CloudRequestEngine needs a credential and this one was given \
                 none. Either set a resource key with resource_key(..), which \
                 you can create for free at \
                 https://configure.51degrees.com?utm_source=code&utm_medium=comment&utm_campaign=51degrees-rust&utm_content=cloud-request-engine-src-engine.rs&utm_term=credential-required, \
                 or set a license key with license_key(..) and name the \
                 properties you want with values(..).",
            )),
            (None, Some(_)) if values.is_empty() => Err(Error::configuration(
                "a CloudRequestEngine was given a license key and no property \
                 list. A license key names no properties of its own, so the \
                 cloud service refuses a request that authenticates with one \
                 without saying which properties to return, and every request \
                 this engine made would fail. Name them with values(..), for \
                 example values([\"device.ismobile\"]), or add a resource key \
                 with resource_key(..), which states what it carries itself.",
            )),
            _ => Ok((resource_key, license_key, values)),
        }
    }

    /// Resolve the three endpoint URLs from the explicit overrides, the base
    /// endpoint, the `51DEGREES_CLOUD_ENDPOINT` environment variable, or the
    /// default.
    fn resolve_endpoints(&self) -> Endpoints {
        let base = self
            .endpoint
            .clone()
            .or_else(|| std::env::var(constants::CLOUD_ENDPOINT_ENV_VAR).ok())
            .unwrap_or_else(|| constants::CLOUD_URI_DEFAULT.to_owned());
        let base = if base.ends_with('/') {
            base
        } else {
            format!("{base}/")
        };

        Endpoints {
            data: self
                .data_endpoint
                .clone()
                .unwrap_or_else(|| format!("{base}{}", constants::DATA_FILENAME)),
            properties: self
                .properties_endpoint
                .clone()
                .unwrap_or_else(|| format!("{base}{}", constants::PROPERTIES_FILENAME)),
            evidence_keys: self
                .evidence_keys_endpoint
                .clone()
                .unwrap_or_else(|| format!("{base}{}", constants::EVIDENCE_KEYS_FILENAME)),
        }
    }
}

impl Default for CloudRequestEngineBuilder {
    fn default() -> Self {
        CloudRequestEngineBuilder::new()
    }
}

/// Construct the built-in reqwest-backed transport, used when the builder was
/// not given a [`CloudHttpClient`]. Compiled only with the `reqwest-client`
/// feature.
#[cfg(feature = "reqwest-client")]
fn default_http_client(timeout: Duration, _: &'static str) -> Result<Arc<dyn CloudHttpClient>> {
    Ok(Arc::new(
        crate::http::ReqwestClient::new(timeout).map_err(|m| cloud_error(0, None, m))?,
    ))
}

/// Without the `reqwest-client` feature there is no built-in transport, so a
/// builder that was not given a [`CloudHttpClient`] cannot produce an engine.
/// Return a clear configuration error, being `message`, rather than silently
/// falling back to reqwest.
#[cfg(not(feature = "reqwest-client"))]
fn default_http_client(
    _timeout: Duration,
    message: &'static str,
) -> Result<Arc<dyn CloudHttpClient>> {
    Err(Error::configuration(message))
}

/// Whether a send checks the recovery gate first.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Gate {
    /// Refuse the send during a recovery period, as for the data request.
    Check,
    /// Send whatever the gate says, as for the two discovery requests, which
    /// `discover` checks once for both and the build always sends.
    Skip,
}

/// Send a request through the transport, record success or failure with the
/// recovery gate, and validate the response. Shared by the discovery fetches
/// and the per-process data request.
fn send_and_validate(
    http: &dyn CloudHttpClient,
    recovery: &RecoveryGate,
    gate: Gate,
    request: &CloudHttpRequest,
    check_for_error_messages: bool,
    secrets: &[String],
) -> Result<crate::response::ParsedResponse> {
    // Every failing exit of the inner function goes through one place here, so
    // no error can leave this crate carrying the engine's own credentials
    // however the request failed.
    send_and_validate_inner(
        http,
        recovery,
        gate,
        request,
        check_for_error_messages,
        secrets,
    )
    .map_err(|error| redact_secrets(error, secrets))
}

/// Mask the engine's own credentials in an error by value, on top of the
/// shape-based masking the error types do whenever they are printed.
///
/// A resource key that does not start `AQ`, or a licence key of any shape, is
/// invisible to the shape rules, so the exact values are masked here where
/// they are known. Only the two variants that carry free text need it, and the
/// enum is marked `non_exhaustive`, so the remaining arm returns the error as
/// it stands.
fn redact_secrets(error: Error, secrets: &[String]) -> Error {
    match error {
        Error::CloudRequest {
            status_code,
            retry_after_seconds,
            message,
        } => Error::CloudRequest {
            status_code,
            retry_after_seconds,
            message: redact_with(&message, secrets).into_owned(),
        },
        Error::PipelineConfiguration { message } => Error::PipelineConfiguration {
            message: redact_with(&message, secrets).into_owned(),
        },
        other => other,
    }
}

/// The credentials an engine holds, as the list the redaction takes. Either
/// key may be absent, and the license key is included because it is sent on
/// the data request and the service can repeat it back.
fn credentials(resource_key: Option<&str>, license_key: Option<&str>) -> Vec<String> {
    let mut secrets = Vec::with_capacity(2);
    if let Some(resource_key) = resource_key {
        secrets.push(resource_key.to_owned());
    }
    if let Some(license_key) = license_key {
        secrets.push(license_key.to_owned());
    }
    secrets
}

/// The body of [`send_and_validate`], kept separate so that its caller can
/// clean every failing exit in one place.
fn send_and_validate_inner(
    http: &dyn CloudHttpClient,
    recovery: &RecoveryGate,
    gate: Gate,
    request: &CloudHttpRequest,
    check_for_error_messages: bool,
    secrets: &[String],
) -> Result<crate::response::ParsedResponse> {
    if gate == Gate::Check {
        recovery
            .check_at(Instant::now())
            .map_err(|message| cloud_error(0, None, message))?;
    }

    let response = match http.send(request) {
        Ok(response) => response,
        Err(message) => {
            // The request did not complete: a transport failure. Record it and
            // surface it as a zero-status cloud error.
            recovery.record_failure();
            return Err(cloud_error(0, None, message));
        }
    };

    // The query string of a discovery request carries the keys, so messages
    // name the endpoint without it.
    let endpoint = url_without_query(&request.url);
    let (outcome, warnings) =
        validate_keeping_warnings(&response, endpoint, check_for_error_messages);
    let warnings = mask_warnings(warnings, secrets);
    // Every live response's warnings are logged here, discovery included, and
    // before any errors it carries are raised, as .NET does.
    log_cloud_warnings(&warnings);
    match outcome {
        Ok(json) => {
            recovery.record_success();
            Ok(ParsedResponse { json, warnings })
        }
        Err(error) => {
            if signals_service_trouble(response.status) {
                recovery.record_failure();
            }
            Err(error)
        }
    }
}

/// Whether a failed answer with this status counts towards a recovery period.
///
/// The gate is shared by every request the engine makes, so only an answer
/// that says the service is failing or asks callers to slow down counts, being
/// a status of 500 or more, a 429, or a success status whose body could not be
/// used. A status from 400 to 499 answers the request's own content, which one
/// visitor can choose, such as a user agent too long for the service.
fn signals_service_trouble(status: u16) -> bool {
    status == 429 || !(400..500).contains(&status)
}

/// The error an accessor returns while discovery has not completed.
fn discovery_pending() -> Error {
    cloud_error(
        0,
        None,
        "the cloud request engine has not completed discovery, and the next process \
         call retries it"
            .to_owned(),
    )
}

/// Return a discovery request's 4xx answer as the build's error, and pass any
/// other result back. A 4xx answer, such as an unknown resource key, is a
/// definitive configuration error, which a retry with the same configuration
/// can never fix. Anything else, including a request that did not complete,
/// is transient.
fn definitive_fails_build<T>(result: Result<T>) -> Result<Result<T>> {
    match result {
        Err(
            error @ Error::CloudRequest {
                status_code: 400..=499,
                ..
            },
        ) => Err(error),
        other => Ok(other),
    }
}

/// Log an error for a discovery request that failed or was answered with an
/// error, as .NET does. JSON of the wrong shape is only returned.
fn log_discovery_failure(what: &str, endpoint: &str) {
    log::error!(
        "Failed to retrieve {what} from cloud service at {}.",
        url_without_query(endpoint)
    );
}

/// Fetch the accepted evidence keys from the cloud, mapping any failure to an
/// [`Error::CloudRequest`]. The evidence-keys body is a flat JSON array, so
/// error-message checking is disabled for it.
fn fetch_evidence_keys(
    http: &dyn CloudHttpClient,
    recovery: &RecoveryGate,
    endpoints: &Endpoints,
    origin: Option<&str>,
    secrets: &[String],
) -> Result<EvidenceKeyFilterWhitelist> {
    let request = CloudHttpRequest {
        method: HttpMethod::Get,
        url: endpoints.evidence_keys.clone(),
        form: Vec::new(),
        origin: origin.map(str::to_owned),
    };
    let parsed = send_and_validate(http, recovery, Gate::Skip, &request, false, secrets)
        .inspect_err(|_| log_discovery_failure("evidence keys", &endpoints.evidence_keys))?;
    let keys: Vec<String> = serde_json::from_str(&parsed.json).map_err(|e| {
        redact_secrets(
            cloud_error(
                0,
                None,
                format!(
                    "failed to parse evidence keys from '{}': {e}",
                    endpoints.evidence_keys
                ),
            ),
            secrets,
        )
    })?;
    Ok(EvidenceKeyFilterWhitelist::new(keys))
}

/// Fetch the accessible properties from the cloud, mapping any failure to an
/// [`Error::CloudRequest`].
///
/// The license key is sent with the resource key when one is set and not
/// blank, as the data request does, because the cloud adds the products the
/// license key grants to those of the resource key. Asking with the resource
/// key alone left those products out of the engine's metadata.
fn fetch_public_properties(
    http: &dyn CloudHttpClient,
    recovery: &RecoveryGate,
    endpoints: &Endpoints,
    resource_key: &str,
    license_key: Option<&str>,
    origin: Option<&str>,
    secrets: &[String],
) -> Result<LicensedProducts> {
    let mut url = format!(
        "{}?{}={}",
        endpoints.properties,
        constants::RESOURCE_PARAMETER,
        encode_query_value(resource_key)
    );
    if let Some(license) = license_key.filter(|l| !l.trim().is_empty()) {
        url.push_str(&format!(
            "&{}={}",
            constants::LICENSE_PARAMETER,
            encode_query_value(license)
        ));
    }
    let request = CloudHttpRequest {
        method: HttpMethod::Get,
        url,
        form: Vec::new(),
        origin: origin.map(str::to_owned),
    };
    let parsed = send_and_validate(http, recovery, Gate::Skip, &request, true, secrets)
        .inspect_err(|_| log_discovery_failure("available properties", &endpoints.properties))?;
    LicensedProducts::parse(&parsed.json).map_err(|e| {
        // The address of this one request carries both keys, so the message
        // names the endpoint without its query string and is then cleaned
        // before it becomes an error.
        redact_secrets(
            cloud_error(
                0,
                None,
                format!(
                    "failed to parse accessible properties from '{}': {e}",
                    endpoints.properties
                ),
            ),
            secrets,
        )
    })
}

/// Percent-encode a query string value, leaving only the characters that
/// never need it (letters, digits, `-`, `.`, `_` and `~`).
fn encode_query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// The URL without its query string, for messages. The discovery requests
/// carry the resource and license keys in the query string, and neither
/// belongs in an error message or a log.
pub(crate) fn url_without_query(url: &str) -> &str {
    url.split_once('?').map_or(url, |(endpoint, _)| endpoint)
}

/// Build the static property metadata the engine always exposes: the raw JSON
/// under both field names, and the process-started flag.
fn build_property_metadata() -> (Vec<PropertyMetaData>, Vec<AspectPropertyMetaData>) {
    let core = vec![
        PropertyMetaData::new(
            constants::ELEMENT_DATA_KEY,
            constants::ELEMENT_DATA_KEY,
            PropertyValueType::String,
        ),
        PropertyMetaData::new(
            constants::JSON_RESPONSE_KEY,
            constants::ELEMENT_DATA_KEY,
            PropertyValueType::String,
        ),
        PropertyMetaData::new(
            constants::PROCESS_STARTED_KEY,
            constants::ELEMENT_DATA_KEY,
            PropertyValueType::Bool,
        ),
    ];
    let aspect = core
        .iter()
        .cloned()
        .map(AspectPropertyMetaData::from_core)
        .collect();
    (core, aspect)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fiftyone_pipeline_core::Evidence;

    #[test]
    fn build_rejects_a_malformed_endpoint() {
        // An endpoint with no scheme, or a non-http(s) scheme, is invalid.
        // Validation runs before any network work, so the build fails directly.
        // A stub client is supplied so the only possible configuration error is
        // the endpoint format, regardless of whether the reqwest-client feature
        // is enabled.
        for bad in ["cloud.51degrees.com", "ftp://cloud.51degrees.com"] {
            match CloudRequestEngine::builder()
                .resource_key("rk")
                .endpoint(bad)
                .http_client(Arc::new(NoopClient))
                .build()
            {
                Err(Error::PipelineConfiguration { .. }) => {}
                Err(other) => panic!("expected a configuration error for {bad:?}, got {other:?}"),
                Ok(_) => {
                    panic!("expected a configuration error for {bad:?}, but the build succeeded")
                }
            }
        }
    }

    #[test]
    fn validate_endpoint_url_accepts_well_formed_urls() {
        assert!(validate_endpoint_url("https://cloud.51degrees.com/api/v4/json", "x").is_ok());
        assert!(validate_endpoint_url("http://localhost:8080/json", "x").is_ok());
        assert!(validate_endpoint_url("https://", "x").is_err());
        assert!(validate_endpoint_url("http:///json", "x").is_err());
        assert!(validate_endpoint_url("cloud.51degrees.com", "x").is_err());
        assert!(validate_endpoint_url(" https://x.test/json", "x").is_err());
    }

    #[test]
    fn cache_key_is_order_independent_and_separates_distinct_requests() {
        // The same pairs in a different order produce the same key.
        let a = cache_key(&[
            ("resource".to_owned(), "rk".to_owned()),
            ("user-agent".to_owned(), "UA".to_owned()),
        ]);
        let b = cache_key(&[
            ("user-agent".to_owned(), "UA".to_owned()),
            ("resource".to_owned(), "rk".to_owned()),
        ]);
        assert_eq!(a, b, "key is independent of form order");

        // Different evidence, or a different resource key, yields a different key.
        let other_evidence = cache_key(&[
            ("resource".to_owned(), "rk".to_owned()),
            ("user-agent".to_owned(), "Other".to_owned()),
        ]);
        let other_key = cache_key(&[
            ("resource".to_owned(), "rk2".to_owned()),
            ("user-agent".to_owned(), "UA".to_owned()),
        ]);
        assert_ne!(a, other_evidence, "different evidence keys differently");
        assert_ne!(a, other_key, "different resource key keys differently");
    }

    #[test]
    fn cache_key_cannot_be_matched_by_a_value_shaped_like_another_field() {
        // A query value can hold a line break and an equals sign, so one value
        // must not be able to spell out the rest of another request's key.
        let crafted = cache_key(&[
            ("resource".to_owned(), "rk".to_owned()),
            (
                "user-agent".to_owned(),
                "UA\nx-device-user-agent=Other".to_owned(),
            ),
        ]);
        let both = cache_key(&[
            ("resource".to_owned(), "rk".to_owned()),
            ("user-agent".to_owned(), "UA".to_owned()),
            ("x-device-user-agent".to_owned(), "Other".to_owned()),
        ]);
        assert_ne!(crafted, both);

        // Nor can a value carry the start of the next field's length.
        let split = cache_key(&[("a".to_owned(), "1:b".to_owned())]);
        let joined = cache_key(&[
            ("a".to_owned(), "1".to_owned()),
            ("b".to_owned(), "".to_owned()),
        ]);
        assert_ne!(split, joined);
    }

    #[test]
    fn strip_prefix_removes_known_prefix() {
        assert_eq!(strip_prefix("query.user-agent"), "user-agent");
        assert_eq!(strip_prefix("header.user-agent"), "user-agent");
        assert_eq!(strip_prefix("cookie.session"), "session");
        // Unknown prefix still split on the first separator.
        assert_eq!(strip_prefix("custom.field"), "field");
        // No separator is returned unchanged.
        assert_eq!(strip_prefix("bare"), "bare");
    }

    use crate::state::EvidenceKeyEntry;

    /// A resolved state with a few accepted evidence keys, so a built engine
    /// needs no discovery fetch. The keys cover the ones the content test uses.
    fn sample_state() -> CloudEngineState {
        CloudEngineState {
            evidence_keys: ["header.user-agent", "query.user-agent", "server.host"]
                .into_iter()
                .map(|key| EvidenceKeyEntry {
                    key: key.to_owned(),
                    order: 0,
                })
                .collect(),
            accessible_properties: LicensedProducts::default(),
        }
    }

    fn engine_with_dummy_client() -> CloudRequestEngine {
        struct Dummy;
        impl CloudHttpClient for Dummy {
            fn send(
                &self,
                _request: &CloudHttpRequest,
            ) -> std::result::Result<crate::http::CloudHttpResponse, String> {
                Err("not used".to_owned())
            }
        }
        // A supplied state means the builder makes no discovery call, so the
        // dummy client (which would error) is never used during build.
        CloudRequestEngine::builder()
            .resource_key("rk")
            .http_client(Arc::new(Dummy))
            .set_state(sample_state())
            .build()
            .unwrap()
    }

    #[test]
    fn a_recovery_period_too_long_to_hold_is_held_at_100_years() {
        let longest = Duration::from_secs_f64(RECOVERY_SECONDS_MAX);
        for seconds in [f64::INFINITY, f64::MAX, 1e300, 1e20, RECOVERY_SECONDS_MAX] {
            let builder = CloudRequestEngine::builder()
                .failures_to_enter_recovery(1)
                .recovery_seconds(seconds);
            assert_eq!(builder.recovery, longest, "{seconds}");
            let gate = RecoveryGate::new(RecoveryConfig {
                failures_to_enter_recovery: builder.failures_to_enter_recovery,
                window: builder.failures_window,
                recovery: builder.recovery,
            });
            let now = Instant::now();
            gate.record_failure_at(now);
            assert!(gate.is_recovering_at(now), "{seconds}");
        }
        for seconds in [f64::NAN, f64::NEG_INFINITY, -1.0, 0.0] {
            let builder = CloudRequestEngine::builder().recovery_seconds(seconds);
            assert_eq!(builder.recovery, Duration::ZERO, "{seconds}");
        }
        let builder = CloudRequestEngine::builder().recovery_seconds(2.5);
        assert_eq!(builder.recovery, Duration::from_millis(2500));
    }

    /// The configuration error message from a build that was expected to fail.
    fn refusal(result: Result<CloudRequestEngine>) -> String {
        match result {
            Err(Error::PipelineConfiguration { message, .. }) => message,
            Ok(_) => panic!("expected a configuration error, but the build succeeded"),
            Err(other) => panic!("expected a configuration error, got {other:?}"),
        }
    }

    #[test]
    fn build_requires_a_credential() {
        // Neither credential leaves nothing to authenticate with, so the build
        // fails before it looks at a transport or an endpoint.
        let message = refusal(CloudRequestEngine::builder().build());
        assert!(
            message.contains("needs a credential") && message.contains("resource_key(.."),
            "the message should name the settings to change, got {message}"
        );
    }

    #[test]
    fn both_credentials_build_an_engine() {
        // A license key alongside a resource key is accepted and both keys are
        // sent. With no property list the license key adds no property.
        let engine = CloudRequestEngine::builder()
            .resource_key("rk")
            .license_key("lk")
            .http_client(Arc::new(NoopClient))
            .set_state(sample_state())
            .build()
            .unwrap();
        assert_eq!(engine.resource_key(), Some("rk"));
        assert!(engine.values().is_empty());
    }

    #[test]
    fn build_refuses_a_license_key_without_a_property_list() {
        // The cloud service answers 400 to every such request, so the engine is
        // refused at build time rather than failing in service.
        let message = refusal(
            CloudRequestEngine::builder()
                .license_key("lk")
                .http_client(Arc::new(NoopClient))
                .set_state(sample_state())
                .build(),
        );
        assert!(
            message.contains("license key and no property list") && message.contains("values(.."),
            "the message should point at values(..), got {message}"
        );
    }

    #[test]
    fn a_name_left_out_beside_a_resource_key_alone_is_reported_as_unknown() {
        // Beside a resource key alone a listed property no license grants
        // comes back null with its reason, so a name left out of the answer is
        // one the service does not know, and the warning says so rather than
        // pointing at a license key that was never given.
        let engine = CloudRequestEngine::builder()
            .resource_key("rk")
            .values(["device.ismobile", "device.nosuchproperty"])
            .http_client(Arc::new(NoopClient))
            .set_state(sample_state())
            .build()
            .expect("a resource key and a list build");
        let warning = engine
            .missing_values_warning(r#"{"device":{"ismobile":true}}"#)
            .expect("the unknown name is reported");
        assert!(
            warning.contains("device.nosuchproperty") && !warning.contains("device.ismobile"),
            "{warning}"
        );
        assert!(
            warning.contains("not a property the cloud service knows")
                && !warning.contains("entitlement matter"),
            "{warning}"
        );
    }

    #[test]
    fn a_resource_key_and_a_property_list_build_without_a_license_key() {
        // The service adds the listed properties to those the resource key
        // carries, so the list is sent with the resource key alone too.
        let engine = CloudRequestEngine::builder()
            .resource_key("rk")
            .values(["device.hardwarevendor"])
            .http_client(Arc::new(NoopClient))
            .set_state(sample_state())
            .build()
            .expect("a resource key and a list build");
        let pipeline = fiftyone_pipeline_core::Pipeline::builder()
            .add_element(Arc::new(engine_with_dummy_client()))
            .suppress_process_exceptions(true)
            .build()
            .unwrap();
        let data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("header.user-agent", "header-ua")
                .build(),
        );
        let form = engine.build_content(&data);
        assert_eq!(form[0], ("resource".to_owned(), "rk".to_owned()));
        assert_eq!(
            form[1],
            ("values".to_owned(), "device.hardwarevendor".to_owned())
        );
    }

    #[test]
    fn a_resource_key_a_license_key_and_a_property_list_build_and_are_all_sent() {
        // The license key grants the listed properties it covers, on top of
        // those the resource key carries, so all three go in the request.
        let engine = CloudRequestEngine::builder()
            .resource_key("rk")
            .license_key("lk")
            .values(["device.hardwarevendor", "device.ismobile"])
            .http_client(Arc::new(NoopClient))
            .set_state(sample_state())
            .build()
            .expect("a resource key, a license key and a list build");
        assert_eq!(engine.resource_key(), Some("rk"));
        assert_eq!(
            engine.values(),
            ["device.hardwarevendor", "device.ismobile"]
        );
        let pipeline = fiftyone_pipeline_core::Pipeline::builder()
            .add_element(Arc::new(engine_with_dummy_client()))
            .suppress_process_exceptions(true)
            .build()
            .unwrap();
        let data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("header.user-agent", "header-ua")
                .build(),
        );
        let form = engine.build_content(&data);
        assert_eq!(form[0], ("resource".to_owned(), "rk".to_owned()));
        assert_eq!(form[1], ("license".to_owned(), "lk".to_owned()));
        assert_eq!(
            form[2],
            (
                "values".to_owned(),
                "device.hardwarevendor,device.ismobile".to_owned()
            )
        );
    }

    #[test]
    fn blank_credentials_and_property_names_count_as_absent() {
        // A key read from an environment variable or a config file can arrive
        // empty or as whitespace, which is not a credential.
        for blank in ["", "   ", "\n"] {
            let message = refusal(CloudRequestEngine::builder().resource_key(blank).build());
            assert!(
                message.contains("needs a credential"),
                "a blank resource key should not count as one, got {message}"
            );
        }
        // The same for a property list of nothing but blanks, which names no
        // property for the license key to be answered on.
        let message = refusal(
            CloudRequestEngine::builder()
                .license_key("lk")
                .values(["", "  "])
                .build(),
        );
        assert!(
            message.contains("no property list"),
            "a blank property list should not count as one, got {message}"
        );
        // A credential with a stray newline is trimmed rather than refused, and
        // reaches the request without it.
        let engine = CloudRequestEngine::builder()
            .resource_key(" rk\n")
            .http_client(Arc::new(NoopClient))
            .set_state(sample_state())
            .build()
            .unwrap();
        assert_eq!(engine.resource_key(), Some("rk"));
    }

    #[test]
    fn a_license_key_and_a_property_list_build_an_engine() {
        let engine = CloudRequestEngine::builder()
            .license_key("lk")
            .values(["device.ismobile", " device.iscrawler "])
            .http_client(Arc::new(NoopClient))
            .set_state(sample_state())
            .build()
            .unwrap();
        assert_eq!(engine.resource_key(), None);
        // Each name is trimmed, so a list written across several lines in a
        // config file reaches the service as the service expects it.
        assert_eq!(engine.values(), ["device.ismobile", "device.iscrawler"]);
    }

    #[test]
    fn response_carries_ignores_case_and_counts_a_null_property() {
        // The service lowercases both parts of the name in its answer whatever
        // the caller wrote, so a caller's casing must not read as missing.
        let body: serde_json::Value = serde_json::from_str(
            r#"{"device":{"ismobile":true},
                "location":{"country":null,
                            "countrynullreason":"needs JavaScript evidence"},
                "javascriptProperties":["device.javascript"]}"#,
        )
        .unwrap();
        assert!(response_carries(&body, "device.ismobile"));
        assert!(response_carries(&body, "Device.IsMobile"));
        // Present but null is an answer, because the reason beside it says why.
        assert!(response_carries(&body, "location.country"));
        assert!(!response_carries(&body, "device.iscrawler"));
        assert!(!response_carries(&body, "ip.registeredcountry"));
        // A name with no product part cannot be looked up in the response, and
        // a product that is not an object carries no properties.
        assert!(!response_carries(&body, "ismobile"));
        assert!(!response_carries(&body, "javascriptProperties.device"));
    }

    #[test]
    fn carries_identifier_finds_the_member_in_any_case() {
        assert!(carries_identifier(
            r#"{"device":{"ismobile":true},"fodid":{"idrandglobal":"x"}}"#
        ));
        assert!(carries_identifier(r#"{"FODid":{"idprobglobal":null}}"#));
        // A member of another product that merely mentions the name is not one.
        assert!(!carries_identifier(
            r#"{"device":{"ismobile":true,"fodid":"text"},"javascriptProperties":[]}"#
        ));
        assert!(!carries_identifier(r#"{"device":{"ismobile":true}}"#));
        // What cannot be read as an object cannot be checked, so it is not shared.
        assert!(carries_identifier("[]"));
        assert!(carries_identifier("not json"));
    }

    #[cfg(not(feature = "reqwest-client"))]
    #[test]
    fn build_without_client_errors_when_reqwest_disabled() {
        // With the reqwest-client feature off and no CloudHttpClient supplied,
        // the builder must fail clearly rather than silently falling back to
        // reqwest.
        match CloudRequestEngine::builder().resource_key("rk").build() {
            Err(Error::PipelineConfiguration { .. }) => {}
            Ok(_) => panic!("expected a configuration error without a client"),
            Err(other) => panic!("unexpected error {other:?}"),
        }
    }

    #[cfg(feature = "reqwest-client")]
    #[test]
    fn build_uses_builtin_client_when_reqwest_enabled() {
        // With the feature on and no client supplied, the built-in reqwest client
        // is constructed and the engine builds. A supplied state keeps the build
        // offline (no discovery fetch), so the test does not touch the network.
        assert!(
            CloudRequestEngine::builder()
                .resource_key("rk")
                .set_state(sample_state())
                .build()
                .is_ok(),
            "the built-in reqwest client should be used when no client is supplied"
        );
    }

    #[test]
    fn default_endpoints_use_cloud_default() {
        let engine = engine_with_dummy_client();
        assert_eq!(
            engine.data_endpoint(),
            "https://cloud.51degrees.com/api/v4/json"
        );
    }

    #[test]
    fn custom_base_endpoint_adds_trailing_slash() {
        let engine = CloudRequestEngine::builder()
            .resource_key("rk")
            .endpoint("https://example.test/api")
            .http_client(Arc::new(NoopClient))
            .set_state(sample_state())
            .build()
            .unwrap();
        assert_eq!(engine.data_endpoint(), "https://example.test/api/json");
        assert_eq!(
            engine.endpoints.properties,
            "https://example.test/api/accessibleproperties"
        );
        assert_eq!(
            engine.endpoints.evidence_keys,
            "https://example.test/api/evidencekeys"
        );
    }

    struct NoopClient;
    impl CloudHttpClient for NoopClient {
        fn send(
            &self,
            _request: &CloudHttpRequest,
        ) -> std::result::Result<crate::http::CloudHttpResponse, String> {
            Err("noop".to_owned())
        }
    }

    #[test]
    fn content_strips_prefixes_and_applies_precedence() {
        let engine = engine_with_dummy_client();
        // Build a flow data through a pipeline so the evidence is set.
        let pipeline = fiftyone_pipeline_core::Pipeline::builder()
            .add_element(Arc::new(engine_with_dummy_client()))
            .suppress_process_exceptions(true)
            .build()
            .unwrap();
        let data = pipeline.create_flow_data_with(
            Evidence::builder()
                .add("header.user-agent", "header-ua")
                .add("query.user-agent", "query-ua")
                .add("server.host", "example.com")
                .build(),
        );

        let form = engine.build_content(&data);
        // Resource key leads.
        assert_eq!(form[0], ("resource".to_owned(), "rk".to_owned()));
        // user-agent is present once, with the query value winning over header.
        let ua: Vec<&String> = form
            .iter()
            .filter(|(k, _)| k == "user-agent")
            .map(|(_, v)| v)
            .collect();
        assert_eq!(ua.len(), 1, "deduplicated to one user-agent");
        assert_eq!(ua[0], "query-ua", "query precedence wins");
        // The server.host value is stripped to `host`.
        assert!(form.iter().any(|(k, v)| k == "host" && v == "example.com"));
    }
}
