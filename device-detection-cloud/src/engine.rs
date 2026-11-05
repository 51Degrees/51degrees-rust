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
use fiftyone_pipeline_engines::{AspectEngine, AspectPropertyMetaData, EngineDeployment};

use fiftyone_cloud_request_engine::CloudRequestEngine;
use fiftyone_device_detection_shared::{
    declared_property_value_type, DeviceDataBase, DEVICE_DATA_KEY, DEVICE_ELEMENT_DATA_KEY,
};

use crate::dto::map_device_object;
use crate::meta::{cloud_json, fill_metadata, DiscoveredMetadata};

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
/// # Metadata
///
/// The device properties a resource key grants come from the cloud request
/// engine's accessible properties, which it normally fetches when it is built.
/// The engine reads them when it is constructed. After a transient failure
/// the request engine retries discovery on first use, so until then
/// [`AspectEngine::aspect_properties`] is empty, and the engine reads the
/// properties on the first `process` after discovery completes. A resource
/// key that grants no device product leaves the metadata empty and logs an
/// error once.
///
/// # A failed cloud request
///
/// When the cloud request engine left no response, the engine logs that at
/// info level, records a flow error that does not fail processing, and stores
/// empty device data marked with
/// [`AspectDataBase::mark_cloud_request_failed`](fiftyone_pipeline_engines::AspectDataBase::mark_cloud_request_failed),
/// so each property read gives
/// [`MissingPropertyReason::CloudRequestFailed`](fiftyone_pipeline_core::MissingPropertyReason::CloudRequestFailed),
/// as .NET does.
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

    /// The property metadata for the device aspect, read from the request
    /// engine's accessible properties once its discovery has completed, then
    /// kept for the life of the engine. A OnceLock is used because process
    /// takes `&self`. This is what lets later elements (the set-headers element
    /// in particular) see the device `SetHeader*` properties at request time.
    metadata: OnceLock<DiscoveredMetadata>,
}

impl DeviceDetectionCloudEngine {
    /// Start building a cloud device-detection engine.
    pub fn builder() -> DeviceDetectionCloudEngineBuilder {
        DeviceDetectionCloudEngineBuilder::new()
    }

    /// Construct the engine directly from the [`CloudRequestEngine`] that
    /// precedes it in the pipeline.
    ///
    /// The metadata is read from the request engine here, which makes no
    /// cloud request. When the request engine's discovery has not completed it
    /// is read on the first [`FlowElement::process`] after it has. An error is
    /// logged when the request engine has a resource key that is not granted
    /// the device product.
    pub fn new(request_engine: Arc<CloudRequestEngine>) -> Self {
        let engine = DeviceDetectionCloudEngine {
            request_engine,
            evidence_filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            metadata: OnceLock::new(),
        };
        engine.ensure_metadata();
        engine
    }

    /// The cloud request engine this engine reads from.
    pub fn request_engine(&self) -> &Arc<CloudRequestEngine> {
        &self.request_engine
    }

    /// Read the device product's properties from the request engine into this
    /// engine's metadata, if they are not there already.
    ///
    /// Makes no cloud request. Returns the number of device properties, or the
    /// request engine's [`Error::CloudRequest`] while its discovery has not
    /// completed. Calling it is optional, because construction and
    /// [`FlowElement::process`] do the same.
    pub fn refresh_properties(&mut self) -> Result<usize> {
        fill_metadata(
            &self.metadata,
            &self.request_engine,
            DEVICE_ELEMENT_DATA_KEY,
            ENGINE_NAME,
        )?;
        Ok(self.aspect_properties().len())
    }

    /// Read the metadata if it is not there yet, leaving it to a later call
    /// while the request engine's discovery has not completed rather than
    /// keeping empty metadata.
    fn ensure_metadata(&self) {
        let _ = fill_metadata(
            &self.metadata,
            &self.request_engine,
            DEVICE_ELEMENT_DATA_KEY,
            ENGINE_NAME,
        );
    }

    /// Read the cloud JSON, slice the `device` member, and store the resulting
    /// [`DeviceDataBase`](fiftyone_device_detection_shared::DeviceDataBase) under
    /// [`DEVICE_DATA_KEY`].
    ///
    /// Returns an [`Error::PipelineConfiguration`] if no cloud request engine ran
    /// before this engine. An empty or absent JSON response, which means the
    /// request engine failed and recorded its own error, gives empty device
    /// data marked as from a failed cloud request, and a flow error that does
    /// not fail processing. Data from a response is given the reason for each
    /// property the response did not return.
    fn populate(&self, data: &mut FlowData) -> Result<()> {
        let json = match cloud_json(data, ENGINE_NAME)? {
            Some(json) => json,
            None => {
                CloudRequestEngine::record_not_processed(
                    data,
                    ENGINE_NAME,
                    DEVICE_ELEMENT_DATA_KEY,
                );
                let mut device = DeviceDataBase::new();
                device.base_mut().mark_cloud_request_failed();
                data.get_or_add(DEVICE_DATA_KEY, || device)?;
                return Ok(());
            }
        };

        let device = slice_device_json(&json)?;
        let mut device_data = map_device_object(&device);
        if let Some(metadata) = self.metadata.get() {
            metadata.add_missing_property_source(&mut device_data);
        }
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
        // Read the metadata if discovery has completed since construction, so
        // later elements can see it, then map the cloud response.
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
    fn engine_type_name(&self) -> &str {
        ENGINE_NAME
    }

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
        self.metadata.get().is_some()
    }

    /// Whether `DeviceData` has a typed accessor for the property, matched
    /// ignoring case. Before discovery completes, such a property gives a
    /// data file upgrade as the reason it is missing, as .NET does.
    fn has_typed_property(&self, property_name: &str) -> bool {
        declared_property_value_type(property_name).is_some()
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

    /// Ask [`DeviceDetectionCloudEngineBuilder::try_build`] to fail when the
    /// metadata cannot be read yet, because the request engine's discovery
    /// has not completed, rather than leaving it to the first process call.
    /// [`DeviceDetectionCloudEngineBuilder::build`] leaves it to the first
    /// process call either way.
    pub fn eager_properties(mut self, eager: bool) -> Self {
        self.eager_properties = eager;
        self
    }

    /// Build the engine, panicking if no cloud request engine was supplied.
    ///
    /// Use [`DeviceDetectionCloudEngineBuilder::try_build`] for a non-panicking
    /// variant. Metadata that cannot be read yet is left to the first process
    /// call, whether or not eager properties were asked for.
    pub fn build(self) -> DeviceDetectionCloudEngine {
        let request_engine = self
            .request_engine
            .expect("a CloudRequestEngine is required to build a DeviceDetectionCloudEngine");
        DeviceDetectionCloudEngine::new(request_engine)
    }

    /// Build the engine, returning an [`Error::PipelineConfiguration`] if no
    /// cloud request engine was supplied, or the request engine's
    /// [`Error::CloudRequest`] when
    /// [`DeviceDetectionCloudEngineBuilder::eager_properties`] was set and its
    /// discovery has not completed.
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
