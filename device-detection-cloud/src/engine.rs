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

//! The cloud device-detection engine and its builder.

use std::sync::{Arc, OnceLock};

use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
    Result,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectPropertyMetaData, EngineDeployment, EngineMissingPropertyContext,
    MissingPropertyResult,
};

use fiftyone_cloud_request_engine::{CloudRequestEngine, ProductPropertyNames};
use fiftyone_device_detection_shared::{DEVICE_DATA_KEY, DEVICE_ELEMENT_DATA_KEY};

use crate::dto::map_device_object;
use crate::meta::{cloud_json, refresh_product_metadata};

/// The engine's name in configuration errors and log records.
const ENGINE_NAME: &str = "DeviceDetectionCloudEngine";

/// An engine that turns the cloud JSON response into a
/// [`DeviceDataBase`](fiftyone_device_detection_shared::DeviceDataBase).
///
/// It implements the
/// [device-detection-cloud specification](https://github.com/51Degrees/specifications/blob/main/device-detection-specification/pipeline-elements/device-detection-cloud.md).
///
/// # Place it after a cloud request engine
///
/// This engine consumes no evidence of its own. Instead it reads the raw JSON a
/// [`CloudRequestEngine`] stored under the `cloud-response` data key, slices
/// out the `device` member, and deserializes it into the shared
/// [`DeviceDataBase`](fiftyone_device_detection_shared::DeviceDataBase). It must
/// therefore sit *after* a `CloudRequestEngine` in the pipeline. The engine is
/// constructed from an [`Arc<CloudRequestEngine>`], the same instance added to
/// the pipeline, so it can also derive its property metadata from that engine's
/// accessible-properties discovery.
///
/// # Interface compatibility with the on-premise engine
///
/// The data it produces is the exact same type, under the exact same key
/// ([`DEVICE_DATA_KEY`], data key string `"device"`), as the on-premise Hash
/// engine. A consuming application reads its result through `DEVICE_DATA_KEY` or
/// the shared [`DeviceData`](fiftyone_device_detection_shared::DeviceData)
/// accessors and can swap a cloud engine for an on-premise one without changing
/// its result-reading code.
///
/// # Metadata is derived lazily
///
/// The set of device properties a resource key grants is only known once the
/// cloud request engine has fetched its accessible properties, which it does
/// lazily on first use. So [`AspectEngine::aspect_properties`] returns the
/// engine's statically-known core metadata until [`DeviceDetectionCloudEngine::refresh_properties`]
/// (or the first `process`) has pulled the device product's properties from the
/// request engine. The metadata is not hard-cached at construction, matching the
/// request engine's own lazy initialization.
///
/// # Example
///
/// ```no_run
/// use std::sync::Arc;
/// use fiftyone_pipeline_core::{Evidence, Pipeline};
/// use fiftyone_cloud_request_engine::CloudRequestEngine;
/// use fiftyone_device_detection_cloud::DeviceDetectionCloudEngine;
/// use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY};
///
/// let request_engine = Arc::new(
///     CloudRequestEngine::builder()
///         .resource_key("my-resource-key")
///         .build()
///         .unwrap(),
/// );
/// let device_engine = DeviceDetectionCloudEngine::builder()
///     .cloud_request_engine(request_engine.clone())
///     .build();
///
/// let pipeline = Pipeline::builder()
///     .add_element(request_engine)
///     .add_element(Arc::new(device_engine))
///     .build()
///     .unwrap();
///
/// let mut data = pipeline.create_flow_data_with(
///     Evidence::builder().add("header.user-agent", "Mozilla/5.0").build(),
/// );
/// data.process().unwrap();
/// if let Some(device) = data.get(DEVICE_DATA_KEY) {
///     println!("IsMobile: {:?}", device.is_mobile());
/// }
/// ```
pub struct DeviceDetectionCloudEngine {
    /// The request engine that fetches the cloud JSON and discovers properties.
    /// Held so the engine can derive its metadata and so a builder need only be
    /// given the one instance that is also added to the pipeline.
    request_engine: Arc<CloudRequestEngine>,

    /// This engine reads no evidence directly; the request engine before it
    /// gathers the evidence. An empty whitelist advertises that.
    evidence_filter: EvidenceKeyFilterWhitelist,

    /// The property metadata for the device aspect, discovered from the request
    /// engine's accessible properties. It is empty until the first process call
    /// (or an eager build) populates it, then cached for the life of the engine.
    /// A OnceLock is used because process takes `&self`, so the metadata is
    /// filled through a shared reference once the cloud discovery succeeds. This
    /// is what lets later elements (the set-headers element in particular) see
    /// the device `SetHeader*` properties at request time.
    metadata: OnceLock<DiscoveredMetadata>,

    /// The device properties the resource key is granted, which each
    /// response is checked against.
    granted: ProductPropertyNames,
}

/// The property metadata a cloud engine discovers from the request engine's
/// accessible properties, cached after the first successful discovery.
#[derive(Default)]
struct DiscoveredMetadata {
    /// The core property metadata for the device aspect.
    properties: Vec<PropertyMetaData>,
    /// The aspect view of the same metadata.
    aspect_properties: Vec<AspectPropertyMetaData>,
    /// The data tier reported for the device product.
    data_source_tier: String,
}

impl DeviceDetectionCloudEngine {
    /// Start building a cloud device-detection engine.
    pub fn builder() -> DeviceDetectionCloudEngineBuilder {
        DeviceDetectionCloudEngineBuilder::new()
    }

    /// Construct the engine directly from the [`CloudRequestEngine`] that
    /// precedes it in the pipeline.
    ///
    /// Property metadata is not fetched here. It is populated lazily on the first
    /// [`FlowElement::process`] call, or eagerly by calling
    /// [`DeviceDetectionCloudEngine::refresh_properties`]. An error is logged when the
    /// request engine has a resource key that is not granted the device
    /// product.
    pub fn new(request_engine: Arc<CloudRequestEngine>) -> Self {
        let granted =
            ProductPropertyNames::new(&request_engine, ENGINE_NAME, DEVICE_ELEMENT_DATA_KEY, None);
        DeviceDetectionCloudEngine {
            request_engine,
            granted,
            evidence_filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            // Metadata is intentionally empty until discovery runs on the first
            // process call, or until an eager build pre-fills it.
            metadata: OnceLock::new(),
        }
    }

    /// The cloud request engine this engine reads from.
    pub fn request_engine(&self) -> &Arc<CloudRequestEngine> {
        &self.request_engine
    }

    /// Pull the device product's accessible properties from the request engine
    /// and rebuild this engine's metadata from them.
    ///
    /// This triggers the request engine's lazy accessible-properties fetch if it
    /// has not happened yet, so it makes one cloud request the first time it is
    /// called. It returns the number of device properties discovered, or an
    /// [`Error::CloudRequest`] if the request engine could not fetch them.
    /// Calling it is optional; [`FlowElement::process`] does the same work on
    /// first use.
    pub fn refresh_properties(&mut self) -> Result<usize> {
        let (properties, aspect_properties, data_source_tier) =
            refresh_product_metadata(&self.request_engine, DEVICE_ELEMENT_DATA_KEY)?;

        let count = aspect_properties.len();
        // Cache the freshly discovered metadata. If a process call already
        // populated the slot, the existing value is kept and this copy dropped;
        // both describe the same resource key.
        let _ = self.metadata.set(DiscoveredMetadata {
            properties,
            aspect_properties,
            data_source_tier,
        });
        Ok(count)
    }

    /// Populate the cached metadata from the cloud on first use, ignoring a
    /// discovery failure so a transient cloud error is retried on the next
    /// process rather than freezing empty metadata. Called from
    /// [`FlowElement::process`] so the engine's property metadata becomes
    /// available to later elements, in particular the set-headers element that
    /// derives Accept-CH from the device `SetHeader*` properties.
    fn ensure_metadata(&self) {
        if self.metadata.get().is_some() {
            return;
        }
        if let Ok((properties, aspect_properties, data_source_tier)) =
            refresh_product_metadata(&self.request_engine, DEVICE_ELEMENT_DATA_KEY)
        {
            let _ = self.metadata.set(DiscoveredMetadata {
                properties,
                aspect_properties,
                data_source_tier,
            });
        }
    }

    /// Read the cloud JSON, slice the `device` member, and store the resulting
    /// [`DeviceDataBase`](fiftyone_device_detection_shared::DeviceDataBase) under
    /// [`DEVICE_DATA_KEY`].
    ///
    /// Returns an [`Error::PipelineConfiguration`] if no cloud request engine ran
    /// before this engine. An empty or absent JSON response (which signals the
    /// request engine itself failed and already reported the error) leaves the
    /// device data unpopulated and does not raise a new error.
    fn populate(&self, data: &mut FlowData) -> Result<()> {
        let json = match cloud_json(data, ENGINE_NAME)? {
            Some(json) => json,
            None => return Ok(()),
        };

        let device = slice_device_json(&json)?;
        if let Some(device) = device.as_object() {
            self.granted
                .warn_unlisted(device.keys().map(String::as_str));
        }
        let device_data = map_device_object(&device);
        data.get_or_add(DEVICE_DATA_KEY, || device_data)?;
        Ok(())
    }
}

/// Parse the cloud response and return its `device` member as a JSON value.
///
/// Returns an [`Error::CloudRequest`] if the body is not valid JSON. A missing
/// `device` member yields a JSON `null`, which the mapper renders as empty
/// device data rather than an error.
fn slice_device_json(json: &str) -> Result<serde_json::Value> {
    let document: serde_json::Value =
        serde_json::from_str(json).map_err(|e| Error::CloudRequest {
            status_code: 0,
            retry_after_seconds: None,
            message: format!("failed to parse the cloud device-detection response: {e}"),
        })?;
    Ok(document
        .get(DEVICE_ELEMENT_DATA_KEY)
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

impl FlowElement for DeviceDetectionCloudEngine {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        // Discover the property metadata on first use so later elements can see
        // it, then map the cloud response into device data.
        self.ensure_metadata();
        self.populate(data)
    }

    fn data_key(&self) -> &str {
        DEVICE_ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        // This engine reads the cloud request engine's output, not raw evidence,
        // so it advertises an empty filter.
        &self.evidence_filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        self.metadata
            .get()
            .map_or(&[], |metadata| metadata.properties.as_slice())
    }
}

impl AspectEngine for DeviceDetectionCloudEngine {
    fn data_source_tier(&self) -> &str {
        self.metadata
            .get()
            .map_or("", |metadata| metadata.data_source_tier.as_str())
    }

    fn deployment(&self) -> EngineDeployment {
        EngineDeployment::Cloud
    }

    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        self.metadata
            .get()
            .map_or(&[], |metadata| metadata.aspect_properties.as_slice())
    }

    fn has_loaded_properties(&self) -> bool {
        // Properties come from the request engine's lazy discovery, so they are
        // loaded once that discovery has completed.
        self.request_engine.has_loaded_properties()
    }

    fn missing_property_reason(&self, property_name: &str) -> MissingPropertyResult {
        let ctx = EngineMissingPropertyContext {
            element_data_key: self.data_key(),
            deployment: self.deployment(),
            data_source_tier: self.data_source_tier(),
            properties_loaded: self.has_loaded_properties(),
            properties: self.aspect_properties(),
        };
        fiftyone_pipeline_engines::missing_property_reason(property_name, &ctx)
    }
}

/// A fluent builder for [`DeviceDetectionCloudEngine`].
///
/// The cloud request engine
/// is required, as this engine reads its output and derives its metadata from it.
/// Optionally call [`DeviceDetectionCloudEngineBuilder::eager_properties`] to pull
/// the property metadata from the cloud at build time rather than on first
/// process.
pub struct DeviceDetectionCloudEngineBuilder {
    request_engine: Option<Arc<CloudRequestEngine>>,
    eager_properties: bool,
}

impl DeviceDetectionCloudEngineBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        DeviceDetectionCloudEngineBuilder {
            request_engine: None,
            eager_properties: false,
        }
    }

    /// Set the [`CloudRequestEngine`] this engine reads from and derives its
    /// metadata from (required). Supply the same `Arc` that is added to the
    /// pipeline.
    pub fn cloud_request_engine(mut self, engine: Arc<CloudRequestEngine>) -> Self {
        self.request_engine = Some(engine);
        self
    }

    /// Request that property metadata is fetched from the cloud at
    /// [`DeviceDetectionCloudEngineBuilder::try_build`] time rather than lazily on
    /// the first process call. This makes one cloud request during the build. A
    /// fetch failure is ignored by [`DeviceDetectionCloudEngineBuilder::build`]
    /// (the metadata is then filled lazily) but surfaced by
    /// [`DeviceDetectionCloudEngineBuilder::try_build`].
    pub fn eager_properties(mut self, eager: bool) -> Self {
        self.eager_properties = eager;
        self
    }

    /// Build the engine, panicking if no cloud request engine was supplied.
    ///
    /// Use [`DeviceDetectionCloudEngineBuilder::try_build`] for a non-panicking
    /// variant. Eager property fetching, if requested, is best-effort here: a
    /// failure leaves the metadata to be filled lazily on first process.
    pub fn build(self) -> DeviceDetectionCloudEngine {
        self.try_build()
            .expect("a CloudRequestEngine is required to build a DeviceDetectionCloudEngine")
    }

    /// Build the engine, returning an [`Error::PipelineConfiguration`] if no
    /// cloud request engine was supplied, or propagating an
    /// [`Error::CloudRequest`] from eager property discovery when
    /// [`DeviceDetectionCloudEngineBuilder::eager_properties`] was set.
    pub fn try_build(self) -> Result<DeviceDetectionCloudEngine> {
        let request_engine = self.request_engine.ok_or_else(|| {
            Error::configuration(
                "a CloudRequestEngine is required to build a DeviceDetectionCloudEngine",
            )
        })?;

        let mut engine = DeviceDetectionCloudEngine::new(request_engine);
        if self.eager_properties {
            engine.refresh_properties()?;
        }
        Ok(engine)
    }
}

impl Default for DeviceDetectionCloudEngineBuilder {
    fn default() -> Self {
        DeviceDetectionCloudEngineBuilder::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_device_json_extracts_device_member() {
        let json = r#"{"device":{"ismobile":true},"javascriptProperties":[]}"#;
        let device = slice_device_json(json).unwrap();
        assert_eq!(device.get("ismobile").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn slice_device_json_missing_member_is_null() {
        let json = r#"{"location":{}}"#;
        let device = slice_device_json(json).unwrap();
        assert!(device.is_null());
    }

    #[test]
    fn slice_device_json_invalid_is_cloud_error() {
        match slice_device_json("not json") {
            Err(Error::CloudRequest { .. }) => {}
            other => panic!("expected a cloud request error, got {other:?}"),
        }
    }

    #[test]
    fn try_build_requires_request_engine() {
        match DeviceDetectionCloudEngine::builder().try_build() {
            Err(Error::PipelineConfiguration { .. }) => {}
            Err(other) => panic!("expected a configuration error, got {other:?}"),
            Ok(_) => panic!("expected a configuration error, but build succeeded"),
        }
    }
}
