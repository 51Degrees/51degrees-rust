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

//! The cloud hardware-profile lookup engine (TAC and native-model) and builder.

use std::sync::{Arc, OnceLock};

use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
    Result,
};
use fiftyone_pipeline_engines::{
    default_missing_property_reason, AspectEngine, AspectPropertyMetaData, EngineDeployment,
    MissingPropertyResult,
};

use fiftyone_cloud_request_engine::CloudRequestEngine;

use crate::dto::map_device_object;
use crate::meta::{cloud_json, fill_metadata, property_list_request_failed, DiscoveredMetadata};
use fiftyone_device_detection_shared::{
    MultiDeviceData, HARDWARE_DATA_KEY, HARDWARE_ELEMENT_DATA_KEY,
};

/// The engine's name in configuration errors and log records.
const ENGINE_NAME: &str = "HardwareProfileCloudEngine";

/// A cloud engine that returns the several device profiles matching a single
/// lookup parameter, such as a Type Allocation Code (TAC) or a native model
/// name.
///
/// It implements the
/// [hardware-profile-lookup specification](https://github.com/51Degrees/specifications/blob/main/device-detection-specification/pipeline-elements/hardware-profile-lookup-cloud.md).
///
/// # Place it after a cloud request engine
///
/// Like the single-device [`DeviceDetectionCloudEngine`](crate::DeviceDetectionCloudEngine),
/// this engine consumes no evidence of its own. It reads the raw JSON a
/// [`CloudRequestEngine`] stored under the `cloud` data key, slices out the
/// `hardware.profiles` array, and turns each entry into a
/// [`DeviceDataBase`](fiftyone_device_detection_shared::DeviceDataBase),
/// collecting them into a [`MultiDeviceData`] stored under [`HARDWARE_DATA_KEY`].
/// It must therefore sit *after* a `CloudRequestEngine` in the pipeline.
///
/// The lookup parameter is supplied as evidence to the request engine, not to
/// this engine: `query.tac` for a TAC, `query.nativemodel` for a native model
/// name. Those values are inputs only and are never returned.
///
/// # Resource key requirement
///
/// Hardware-profile lookup needs a resource key that grants the
/// hardware-profile-lookup product (a paid subscription). A key without it
/// returns the standard single-device `device` block with no `hardware.profiles`,
/// so this engine produces an empty [`MultiDeviceData`] rather than failing.
///
/// # Example
///
/// ```no_run
/// use std::sync::Arc;
/// use fiftyone_pipeline_core::{Evidence, Pipeline};
/// use fiftyone_cloud_request_engine::CloudRequestEngine;
/// use fiftyone_device_detection_cloud::{HardwareProfileCloudEngine, HARDWARE_DATA_KEY};
/// use fiftyone_device_detection_shared::DeviceData;
///
/// let request_engine = Arc::new(
///     CloudRequestEngine::builder()
///         .resource_key("my-resource-key")
///         .build()
///         .unwrap(),
/// );
/// let hardware_engine = HardwareProfileCloudEngine::builder()
///     .cloud_request_engine(request_engine.clone())
///     .build();
///
/// let pipeline = Pipeline::builder()
///     .add_element(request_engine)
///     .add_element(Arc::new(hardware_engine))
///     .build()
///     .unwrap();
///
/// // Supply the TAC as query evidence; the cloud returns the matching profiles.
/// let mut data = pipeline.create_flow_data_with(
///     Evidence::builder().add("query.tac", "35925406").build(),
/// );
/// data.process().unwrap();
/// if let Some(hardware) = data.get(HARDWARE_DATA_KEY) {
///     for profile in hardware.profiles() {
///         println!("{:?} {:?}", profile.hardware_vendor().value(), profile.hardware_model().value());
///     }
/// }
/// ```
pub struct HardwareProfileCloudEngine {
    /// The request engine that fetches the cloud JSON and discovers properties.
    request_engine: Arc<CloudRequestEngine>,

    /// This engine reads no evidence directly; the request engine before it
    /// gathers the lookup parameter. An empty whitelist advertises that.
    evidence_filter: EvidenceKeyFilterWhitelist,

    /// The property metadata for the hardware aspect, read from the request
    /// engine's accessible properties once its discovery has completed, then
    /// kept for the life of the engine.
    metadata: OnceLock<DiscoveredMetadata>,
}

impl HardwareProfileCloudEngine {
    /// Start building a hardware-profile cloud engine.
    pub fn builder() -> HardwareProfileCloudEngineBuilder {
        HardwareProfileCloudEngineBuilder::new()
    }

    /// Construct the engine directly from the [`CloudRequestEngine`] that
    /// precedes it in the pipeline.
    ///
    /// The metadata is read from the request engine here, which makes no
    /// cloud request. When the request engine's discovery has not completed it
    /// is read on the first [`FlowElement::process`] after it has. An error is
    /// logged when the request engine has a resource key that is not granted
    /// the hardware product.
    pub fn new(request_engine: Arc<CloudRequestEngine>) -> Self {
        let engine = HardwareProfileCloudEngine {
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

    /// Read the hardware product's properties from the request engine into
    /// this engine's metadata, if they are not there already.
    ///
    /// Makes no cloud request. Returns the number of hardware properties, or
    /// the request engine's [`Error::CloudRequest`] while its discovery has not
    /// completed. A resource key without the hardware-profile-lookup product
    /// leaves the metadata empty rather than failing.
    pub fn refresh_properties(&mut self) -> Result<usize> {
        fill_metadata(
            &self.metadata,
            &self.request_engine,
            HARDWARE_ELEMENT_DATA_KEY,
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
            HARDWARE_ELEMENT_DATA_KEY,
            ENGINE_NAME,
        );
    }

    /// Read the cloud JSON, slice the `hardware.profiles` array, and store the
    /// resulting [`MultiDeviceData`] under [`HARDWARE_DATA_KEY`].
    ///
    /// Returns an [`Error::PipelineConfiguration`] if no cloud request engine ran
    /// before this engine. An empty or absent JSON response, which means the
    /// request engine failed and recorded its own error, gives a result with no
    /// profiles and a flow error that does not fail processing, as .NET does.
    /// Each profile from a response is given the reason for each property the
    /// profile does not hold.
    fn populate(&self, data: &mut FlowData) -> Result<()> {
        let json = match cloud_json(data, ENGINE_NAME)? {
            Some(json) => json,
            None => {
                CloudRequestEngine::record_not_processed(
                    data,
                    ENGINE_NAME,
                    HARDWARE_ELEMENT_DATA_KEY,
                    module_path!(),
                );
                data.get_or_add(HARDWARE_DATA_KEY, MultiDeviceData::new)?;
                return Ok(());
            }
        };

        let result = slice_hardware_profiles(&json, self.metadata.get())?;
        data.get_or_add(HARDWARE_DATA_KEY, || result)?;
        Ok(())
    }
}

/// Parse the cloud response and build a [`MultiDeviceData`] from its
/// `hardware.profiles` array.
///
/// Each profile object is mapped with the shared [`map_device_object`] so a
/// profile reads back through the same typed accessors as a single-device
/// result. A missing `hardware` block or `profiles` array yields an empty
/// result rather than an error, since that is the normal response from a
/// resource key without the hardware-profile-lookup product. Each profile is
/// given the missing property source of `metadata`, as .NET gives each
/// profile the engine's, and logs nothing when a missing property is read,
/// because .NET builds each profile with no logger. Returns an
/// [`Error::CloudRequest`] only when the body is not valid JSON.
fn slice_hardware_profiles(
    json: &str,
    metadata: Option<&DiscoveredMetadata>,
) -> Result<MultiDeviceData> {
    let document: serde_json::Value =
        serde_json::from_str(json).map_err(|e| Error::CloudRequest {
            status_code: 0,
            retry_after_seconds: None,
            message: format!("failed to parse the cloud hardware-profile response: {e}"),
        })?;

    let mut result = MultiDeviceData::new();

    let profiles = document
        .get(HARDWARE_ELEMENT_DATA_KEY)
        .and_then(|hardware| hardware.get("profiles"))
        .and_then(|profiles| profiles.as_array());

    if let Some(profiles) = profiles {
        for profile in profiles {
            let mut device = map_device_object(profile);
            if let Some(metadata) = metadata {
                metadata.add_missing_property_source(&mut device);
            }
            device.base_mut().stop_logging_missing_properties();
            result.push_profile(device);
        }
    }

    Ok(result)
}

impl FlowElement for HardwareProfileCloudEngine {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        self.ensure_metadata();
        self.populate(data)
    }

    fn data_key(&self) -> &str {
        HARDWARE_ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.evidence_filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        self.metadata
            .get()
            .map_or(&[], |metadata| metadata.properties.as_slice())
    }

    /// Reads the cloud's property list first if discovery has completed
    /// since the engine last looked, so the answer is current.
    fn properties_loaded(&self) -> bool {
        self.ensure_metadata();
        self.has_loaded_properties()
    }
}

impl AspectEngine for HardwareProfileCloudEngine {
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

    fn property_list_request_failed(&self) -> bool {
        property_list_request_failed(self.has_loaded_properties(), &self.request_engine)
    }

    fn missing_property_reason(&self, property_name: &str) -> MissingPropertyResult {
        // Read the metadata if discovery has completed since the last
        // process call, so the reason reflects it.
        self.ensure_metadata();
        default_missing_property_reason(self, property_name)
    }
}

/// A fluent builder for [`HardwareProfileCloudEngine`].
///
/// The cloud request
/// engine is required, as this engine reads its output and derives its metadata
/// from it.
pub struct HardwareProfileCloudEngineBuilder {
    request_engine: Option<Arc<CloudRequestEngine>>,
    eager_properties: bool,
}

impl HardwareProfileCloudEngineBuilder {
    /// Create an empty builder.
    pub fn new() -> Self {
        HardwareProfileCloudEngineBuilder {
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

    /// Ask [`HardwareProfileCloudEngineBuilder::try_build`] to fail when the
    /// metadata cannot be read yet, because the request engine's discovery
    /// has not completed, rather than leaving it to the first process call.
    /// [`HardwareProfileCloudEngineBuilder::build`] leaves it to the first
    /// process call either way.
    pub fn eager_properties(mut self, eager: bool) -> Self {
        self.eager_properties = eager;
        self
    }

    /// Build the engine, panicking if no cloud request engine was supplied.
    ///
    /// Use [`HardwareProfileCloudEngineBuilder::try_build`] for a non-panicking
    /// variant. Metadata that cannot be read yet is left to the first process
    /// call, whether or not eager properties were asked for.
    pub fn build(self) -> HardwareProfileCloudEngine {
        let request_engine = self
            .request_engine
            .expect("a CloudRequestEngine is required to build a HardwareProfileCloudEngine");
        HardwareProfileCloudEngine::new(request_engine)
    }

    /// Build the engine, returning an [`Error::PipelineConfiguration`] if no
    /// cloud request engine was supplied, or the request engine's
    /// [`Error::CloudRequest`] when
    /// [`HardwareProfileCloudEngineBuilder::eager_properties`] was set and its
    /// discovery has not completed.
    pub fn try_build(self) -> Result<HardwareProfileCloudEngine> {
        let request_engine = self.request_engine.ok_or_else(|| {
            Error::configuration(
                "a CloudRequestEngine is required to build a HardwareProfileCloudEngine",
            )
        })?;

        let mut engine = HardwareProfileCloudEngine::new(request_engine);
        if self.eager_properties {
            engine.refresh_properties()?;
        }
        Ok(engine)
    }
}

impl Default for HardwareProfileCloudEngineBuilder {
    fn default() -> Self {
        HardwareProfileCloudEngineBuilder::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fiftyone_device_detection_shared::DeviceData;

    #[test]
    fn slice_profiles_reads_each_match() {
        // The cloud returns matched profiles under the `hardware` key, each a
        // flat object of property values.
        let json = r#"{"hardware":{"profiles":[
            {"hardwarevendor":"Apple","hardwarename":["iPhone 11"],"hardwaremodel":"iPhone11,8"},
            {"hardwarevendor":"Apple","hardwarename":["iPhone 11 Pro"],"hardwaremodel":"iPhone12,3"}
        ]}}"#;
        let result = slice_hardware_profiles(json, None).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(
            result.profiles()[0].hardware_vendor().value().unwrap(),
            "Apple"
        );
        assert_eq!(
            result.profiles()[0].hardware_model().value().unwrap(),
            "iPhone11,8"
        );
        assert_eq!(
            result.profiles()[1].hardware_model().value().unwrap(),
            "iPhone12,3"
        );
    }

    #[test]
    fn slice_profiles_no_hardware_block_is_empty() {
        // A standard single-device response (key without the hardware-profile
        // product) has no `hardware` block, so the result is empty, not an error.
        let json = r#"{"device":{"hardwarename":["Desktop"]}}"#;
        let result = slice_hardware_profiles(json, None).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn slice_profiles_invalid_json_is_cloud_error() {
        match slice_hardware_profiles("not json", None) {
            Err(Error::CloudRequest { .. }) => {}
            other => panic!("expected a cloud request error, got {other:?}"),
        }
    }

    #[test]
    fn try_build_requires_request_engine() {
        match HardwareProfileCloudEngine::builder().try_build() {
            Err(Error::PipelineConfiguration { .. }) => {}
            Err(other) => panic!("expected a configuration error, got {other:?}"),
            Ok(_) => panic!("expected a configuration error, but build succeeded"),
        }
    }
}
