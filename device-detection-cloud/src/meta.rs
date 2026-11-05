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

//! Shared property-metadata helpers for the device-detection cloud engines.
//!
//! Both the single-device [`DeviceDetectionCloudEngine`](crate::DeviceDetectionCloudEngine)
//! and the multi-profile [`HardwareProfileCloudEngine`](crate::HardwareProfileCloudEngine)
//! derive their property metadata from the cloud request engine's
//! accessible-properties discovery in exactly the same way, differing only in the
//! element-data key the properties are attributed to. These helpers hold that one
//! shared mapping so the two engines stay consistent.

use std::sync::{Arc, OnceLock};

use fiftyone_cloud_request_engine::CloudPropertyMetaData;
use fiftyone_cloud_request_engine::CloudRequestEngine;
use fiftyone_device_detection_shared::DeviceDataBase;
use fiftyone_pipeline_core::{Error, FlowData, PropertyMetaData, PropertyValueType, Result};
use fiftyone_pipeline_engines::{AspectPropertyMetaData, EngineDeployment, MissingPropertySource};

/// The property metadata a cloud engine derives from the request engine's
/// accessible properties, kept for the life of the engine once derived.
#[derive(Default)]
pub(crate) struct DiscoveredMetadata {
    /// The core property metadata for the product.
    pub(crate) properties: Vec<PropertyMetaData>,
    /// The aspect view of the same metadata.
    pub(crate) aspect_properties: Vec<AspectPropertyMetaData>,
    /// The data tier reported for the product.
    pub(crate) data_source_tier: String,
    /// Why a property is missing from data of a successful request, from
    /// the metadata above. [`None`] when the request engine has no resource
    /// key, because the properties a license key grants are not listed.
    pub(crate) missing: Option<Arc<MissingPropertySource>>,
}

impl DiscoveredMetadata {
    /// Give `device`, mapped from a successful response, the reason for each
    /// property the response did not return, so reading one gives that
    /// reason and logs a warning, as .NET's cloud data does.
    pub(crate) fn add_missing_property_source(&self, device: &mut DeviceDataBase) {
        if let Some(missing) = &self.missing {
            device
                .base_mut()
                .add_missing_property_source(Arc::clone(missing));
        }
    }
}

/// Build the core and aspect property metadata for a set of cloud properties,
/// attributing every property to `element_data_key` and typing it from the
/// cloud's reported value-type name.
pub(crate) fn build_metadata(
    cloud_properties: &[CloudPropertyMetaData],
    element_data_key: &str,
) -> (Vec<PropertyMetaData>, Vec<AspectPropertyMetaData>) {
    let mut core = Vec::with_capacity(cloud_properties.len());
    let mut aspect = Vec::with_capacity(cloud_properties.len());

    for property in cloud_properties {
        let value_type = map_value_type(&property.value_type);
        let mut meta = PropertyMetaData::new(&property.name, element_data_key, value_type);
        if let Some(category) = &property.category {
            meta = meta.with_category(category.clone());
        }
        core.push(meta.clone());
        aspect.push(AspectPropertyMetaData::from_core(meta));
    }

    (core, aspect)
}

/// Fill `slot` with the metadata of the product under `element_data_key`,
/// read from the request engine's accessible properties, unless it is
/// already filled.
///
/// Returns the request engine's [`Error::CloudRequest`] while its discovery
/// has not completed, leaving the slot empty so a later call can fill it. A
/// resource key that grants no such product, or lists it with no properties,
/// fills the slot with empty metadata rather than failing, and the call that
/// fills it logs an error naming `engine_name`, as .NET does, so it is logged
/// once for the life of the engine. With only a license key there are no
/// accessible properties to read, so nothing is logged and the metadata
/// holds no missing property source.
pub(crate) fn fill_metadata(
    slot: &OnceLock<DiscoveredMetadata>,
    request_engine: &CloudRequestEngine,
    element_data_key: &str,
    engine_name: &str,
) -> Result<()> {
    if slot.get().is_some() {
        return Ok(());
    }
    let products = request_engine.public_properties()?;
    let product = products
        .products
        .get(element_data_key)
        .filter(|product| !product.properties.is_empty());
    let mut metadata = product.map_or_else(DiscoveredMetadata::default, |product| {
        let (properties, aspect_properties) = build_metadata(&product.properties, element_data_key);
        DiscoveredMetadata {
            properties,
            aspect_properties,
            data_source_tier: product.data_tier.clone().unwrap_or_default(),
            missing: None,
        }
    });
    if request_engine.resource_key().is_some() {
        metadata.missing = Some(Arc::new(MissingPropertySource::new(
            element_data_key,
            engine_name,
            EngineDeployment::Cloud,
            metadata.data_source_tier.clone(),
            metadata.aspect_properties.clone(),
        )));
    }
    if slot.set(metadata).is_ok() && product.is_none() && request_engine.resource_key().is_some() {
        log::error!(
            "Aspect properties could not be loaded for {engine_name} - '{element_data_key}'"
        );
    }
    Ok(())
}

/// Read the cloud request engine's stored JSON for an engine to map, returning
/// `Ok(None)` when there is nothing to map.
///
/// Returns an [`Error::PipelineConfiguration`], naming `engine_name`, when no
/// cloud request engine ran before the calling engine. An empty or absent JSON
/// response means the request engine failed and already recorded its own error,
/// which yields `Ok(None)`, so the caller stores empty data and records that
/// it did not process, rather than returning a second error.
pub(crate) fn cloud_json(data: &FlowData, engine_name: &str) -> Result<Option<String>> {
    // The request engine writes its data under the `cloud` key. If it is absent
    // or never started processing, the calling engine has been placed without a
    // cloud request engine before it.
    let cloud = data.get(CloudRequestEngine::DATA_KEY).ok_or_else(|| {
        Error::configuration(format!(
            "{engine_name} requires a CloudRequestEngine before it in the pipeline; no cloud \
             data was found."
        ))
    })?;

    if !cloud.process_started() {
        return Err(Error::configuration(format!(
            "{engine_name} requires a CloudRequestEngine before it in the pipeline; the cloud \
             request engine did not start."
        )));
    }

    Ok(match cloud.json_response() {
        Some(json) if !json.is_empty() => Some(json.to_owned()),
        _ => None,
    })
}

/// Map a cloud value-type name onto the nearest core [`PropertyValueType`].
///
/// The cloud reports JSON-flavored type names (for example `String`, `Bool`,
/// `Array`). Anything unrecognised is treated as a string, which is the safest
/// fallback because every value can be read back as text.
pub(crate) fn map_value_type(cloud_type: &str) -> PropertyValueType {
    match cloud_type {
        "Bool" | "Boolean" => PropertyValueType::Bool,
        "Int32" | "Integer" | "Int" | "Long" | "Int64" => PropertyValueType::Integer,
        "Double" | "Single" | "Float" => PropertyValueType::Double,
        "Array" => PropertyValueType::StringList,
        "JavaScript" => PropertyValueType::JavaScript,
        _ => PropertyValueType::String,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_value_type_covers_the_common_cloud_types() {
        assert_eq!(map_value_type("Bool"), PropertyValueType::Bool);
        assert_eq!(map_value_type("Int32"), PropertyValueType::Integer);
        assert_eq!(map_value_type("Double"), PropertyValueType::Double);
        assert_eq!(map_value_type("Array"), PropertyValueType::StringList);
        assert_eq!(map_value_type("JavaScript"), PropertyValueType::JavaScript);
        // Unknown types fall back to a string.
        assert_eq!(map_value_type("Something"), PropertyValueType::String);
    }

    #[test]
    fn build_metadata_owns_the_given_key_and_types() {
        let cloud = vec![
            CloudPropertyMetaData {
                name: "IsMobile".to_owned(),
                value_type: "Bool".to_owned(),
                category: Some("Device".to_owned()),
                ..Default::default()
            },
            CloudPropertyMetaData {
                name: "HardwareName".to_owned(),
                value_type: "Array".to_owned(),
                ..Default::default()
            },
        ];
        let (core, aspect) = build_metadata(&cloud, "hardware");
        assert_eq!(core.len(), 2);
        assert_eq!(aspect.len(), 2);
        assert_eq!(core[0].name, "IsMobile");
        assert_eq!(core[0].element_data_key, "hardware");
        assert_eq!(core[0].value_type, PropertyValueType::Bool);
        assert_eq!(core[0].category, "Device");
        assert_eq!(aspect[1].name(), "HardwareName");
    }
}
