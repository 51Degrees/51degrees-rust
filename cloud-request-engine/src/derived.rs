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

//! The `derived` Properties of the cloud response as Element Data, given by
//! [`DerivedCloudEngine`], among them HumanConfidence and
//! HumanConfidenceSource.
//!
//! On a request with no sealed result, each Property is the Value the
//! response holds, or no value with the message that says why: the remote
//! server's null reason where the response holds it as null, as the
//! [Cloud Aspect Engine](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/cloud-aspect-engine.md#processing)
//! describes, and the reason from
//! [Missing Properties](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/properties.md#missing-properties)
//! where it leaves the Property out, as
//! [Human Confidence](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/human-confidence.md)
//! asks. On a request that carries a sealed result, the two Properties are
//! those [`human_confidence`](crate::human_confidence) gives.

use std::any::Any;
use std::sync::{Arc, OnceLock};

use serde_json::{Map, Value};

use fiftyone_pipeline_core::{
    ElementData, Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, PropertyMetaData, PropertyValue, PropertyValueType, Result,
    TypedKey,
};
use fiftyone_pipeline_engines::{
    default_missing_property_reason, AspectData, AspectDataBase, AspectEngine,
    AspectPropertyMetaData, AspectPropertyValue, EngineDeployment, MissingPropertyResult,
    MissingPropertySource,
};

use crate::aspect::{aspect_response, aspect_values, AspectResponse, AspectValue};
use crate::human_confidence::{human_confidence, SealedHumanConfidence};
use crate::CloudRequestEngine;

/// The data key of the derived Properties, and the member of the cloud
/// response they are read from.
pub const DERIVED_ELEMENT_DATA_KEY: &str = "derived";

/// The key the [`DerivedData`] is stored under in the flow data.
pub const DERIVED_DATA_KEY: TypedKey<DerivedData> = TypedKey::new(DERIVED_ELEMENT_DATA_KEY);

/// The engine's name in log records and messages.
const ENGINE_NAME: &str = "DerivedCloudEngine";

/// The `HumanConfidence` Property.
const HUMAN_CONFIDENCE: &str = "HumanConfidence";

/// The `HumanConfidenceSource` Property.
const HUMAN_CONFIDENCE_SOURCE: &str = "HumanConfidenceSource";

/// The derived Properties of the cloud response, with the message that
/// says why for each one that has no value.
#[derive(Debug, Clone)]
pub struct DerivedData {
    base: AspectDataBase,
    /// The Properties held with no value, each with its message.
    no_values: MapElementData,
}

impl DerivedData {
    fn new() -> Self {
        Self {
            base: AspectDataBase::new(DERIVED_ELEMENT_DATA_KEY),
            no_values: MapElementData::new(),
        }
    }

    /// The `HumanConfidence` Property: `High`, `Medium` or `Low`, or no value
    /// with the message that says why.
    pub fn human_confidence(&self) -> AspectPropertyValue<String> {
        self.string(HUMAN_CONFIDENCE)
    }

    /// The `HumanConfidenceSource` Property: `Passive` or `Active`, or no
    /// value with the message that says why.
    pub fn human_confidence_source(&self) -> AspectPropertyValue<String> {
        self.string(HUMAN_CONFIDENCE_SOURCE)
    }

    /// Borrow the [`AspectDataBase`] that holds the Values.
    pub fn base(&self) -> &AspectDataBase {
        &self.base
    }

    fn string(&self, name: &str) -> AspectPropertyValue<String> {
        match self.get(name) {
            Ok(value) => match value.as_str() {
                Some(text) => AspectPropertyValue::new(text.to_owned()),
                None => {
                    AspectPropertyValue::no_value(format!("The value of '{name}' is not text."))
                }
            },
            Err(no_value) => AspectPropertyValue::no_value(no_value.message),
        }
    }

    fn insert(&mut self, name: &str, value: impl Into<PropertyValue>) {
        self.base.insert(name, value);
    }

    fn insert_no_value(&mut self, name: &str, message: impl Into<String>) {
        self.no_values
            .insert(name, PropertyValue::String(message.into()));
    }

    /// Set the two Properties of a request that carries a sealed result.
    fn set_sealed(&mut self, sealed: SealedHumanConfidence) {
        self.insert(HUMAN_CONFIDENCE, sealed.human_confidence().as_str());
        self.insert(
            HUMAN_CONFIDENCE_SOURCE,
            sealed.human_confidence_source().as_str(),
        );
    }
}

impl ElementData for DerivedData {
    /// The Value, else the message of a Property held with no value, else
    /// the reason the Property is missing.
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        if let Some(value) = self.base.values().get_value(name) {
            return Ok(value.clone());
        }
        if let Some(message) = self.no_values.get_value(name) {
            return Err(NoValueError::new(
                message.as_str().unwrap_or_default().to_owned(),
            ));
        }
        self.base.get(name)
    }

    fn has_property(&self, name: &str) -> bool {
        self.base.values().get_value(name).is_some()
            || self.no_values.get_value(name).is_some()
            || self.base.has_property(name)
    }

    fn keys(&self) -> Vec<String> {
        let mut keys = self.base.keys();
        for key in self.no_values.keys() {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl AspectData for DerivedData {
    fn engine_keys(&self) -> &[String] {
        self.base.engine_keys()
    }

    fn cache_hit(&self) -> bool {
        self.base.cache_hit()
    }
}

/// The metadata of the `derived` product the request engine lists.
struct Metadata {
    properties: Vec<PropertyMetaData>,
    aspect_properties: Vec<AspectPropertyMetaData>,
    /// Why a Property is missing from a response, given with a Resource Key.
    missing: Option<Arc<MissingPropertySource>>,
}

/// Gives the `derived` member of the cloud response as [`DerivedData`],
/// under [`DERIVED_DATA_KEY`].
///
/// It reads the response of the [`CloudRequestEngine`] before it in the
/// Pipeline, as each cloud aspect engine does. On a request that carries a
/// sealed result, HumanConfidence and HumanConfidenceSource are those
/// [`human_confidence`](crate::human_confidence) gives, so a read never
/// fails for them and adds no error.
///
/// ```
/// use std::sync::Arc;
///
/// use fiftyone_cloud_request_engine::{
///     CloudEngineState, CloudHttpClient, CloudHttpRequest, CloudHttpResponse,
///     CloudRequestEngine, DerivedCloudEngine, DERIVED_DATA_KEY,
/// };
/// use fiftyone_pipeline_core::{Evidence, Pipeline};
///
/// // Answers the json call in place of the cloud service.
/// struct StandIn;
///
/// impl CloudHttpClient for StandIn {
///     fn send(&self, _: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
///         Ok(CloudHttpResponse {
///             status: 200,
///             body: r#"{"derived":{"humanconfidence":"High","humanconfidencesource":"Passive"}}"#
///                 .to_owned(),
///             retry_after: None,
///         })
///     }
/// }
///
/// let request_engine = Arc::new(
///     CloudRequestEngine::builder()
///         .resource_key("my-resource-key")
///         .http_client(Arc::new(StandIn))
///         .set_state(CloudEngineState::default())
///         .build()?,
/// );
/// let derived = DerivedCloudEngine::builder()
///     .cloud_request_engine(request_engine.clone())
///     .build()?;
/// let pipeline = Pipeline::builder()
///     .add_element(request_engine)
///     .add_element(Arc::new(derived))
///     .build()?;
///
/// let mut data = pipeline.create_flow_data_with(Evidence::builder().build());
/// data.process()?;
/// let derived = data.get(DERIVED_DATA_KEY).expect("the derived data");
/// assert_eq!(derived.human_confidence().value()?, "High");
/// assert_eq!(derived.human_confidence_source().value()?, "Passive");
/// # Ok::<(), fiftyone_pipeline_core::Error>(())
/// ```
pub struct DerivedCloudEngine {
    request_engine: Option<Arc<CloudRequestEngine>>,
    evidence_filter: EvidenceKeyFilterWhitelist,
    metadata: OnceLock<Metadata>,
    default_properties: Vec<PropertyMetaData>,
    default_aspect_properties: Vec<AspectPropertyMetaData>,
}

impl DerivedCloudEngine {
    /// Start building a derived cloud engine.
    pub fn builder() -> DerivedCloudEngineBuilder {
        DerivedCloudEngineBuilder::new()
    }

    /// Read the `derived` product's metadata from the request engine, once
    /// its discovery has completed. Makes no cloud request.
    fn ensure_metadata(&self) {
        if self.metadata.get().is_some() {
            return;
        }
        let Some(request_engine) = &self.request_engine else {
            return;
        };
        let Ok(products) = request_engine.public_properties() else {
            return;
        };
        let product = products
            .products
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(DERIVED_ELEMENT_DATA_KEY))
            .map(|(_, product)| product)
            .filter(|product| !product.properties.is_empty());
        let (properties, aspect_properties): (Vec<_>, Vec<_>) =
            product.map_or_else(Default::default, |product| {
                product
                    .properties
                    .iter()
                    .map(|property| {
                        let mut core = PropertyMetaData::new(
                            &property.name,
                            DERIVED_ELEMENT_DATA_KEY,
                            PropertyValueType::String,
                        );
                        if let Some(category) = &property.category {
                            core = core.with_category(category.clone());
                        }
                        (core.clone(), AspectPropertyMetaData::from_core(core))
                    })
                    .unzip()
            });
        // The reasons a source gives name a Resource Key, so a License Key
        // alone gives none, as for the other cloud engines.
        let missing = request_engine.resource_key().map(|_| {
            Arc::new(MissingPropertySource::new(
                DERIVED_ELEMENT_DATA_KEY,
                ENGINE_NAME,
                EngineDeployment::Cloud,
                "",
                aspect_properties.clone(),
            ))
        });
        let _ = self.metadata.set(Metadata {
            properties,
            aspect_properties,
            missing,
        });
    }

    /// The derived data for a request: the response's `derived` member, and
    /// then the two Properties of a sealed result where the request carries
    /// one.
    fn derived_data(&self, data: &mut FlowData) -> Result<DerivedData> {
        let sealed = human_confidence(data);
        let mut derived = match aspect_response(data, DERIVED_ELEMENT_DATA_KEY, ENGINE_NAME) {
            Ok(AspectResponse::Portion(portion)) => self.portion_data(&portion),
            Ok(AspectResponse::NoResponse) => {
                // A sealed result gives the two Properties whatever happened
                // to the request, so the engine has nothing to record then.
                if sealed.is_none() {
                    CloudRequestEngine::record_not_processed(
                        data,
                        ENGINE_NAME,
                        DERIVED_ELEMENT_DATA_KEY,
                        module_path!(),
                    );
                }
                let mut derived = DerivedData::new();
                derived.base.mark_cloud_request_failed();
                derived
            }
            Err(error) if sealed.is_none() => return Err(error),
            Err(_) => DerivedData::new(),
        };
        if let Some(sealed) = sealed {
            derived.set_sealed(sealed);
        }
        Ok(derived)
    }

    /// The derived data of a response's `derived` member, each Property
    /// with its Value or with its null reason, and the reason for each
    /// Property the member leaves out.
    fn portion_data(&self, portion: &Map<String, Value>) -> DerivedData {
        let mut derived = DerivedData::new();
        for (name, value) in aspect_values(portion) {
            match value {
                AspectValue::NoValue(reason) => derived.insert_no_value(name, reason),
                AspectValue::Value(value) => {
                    if let Some(value) = property_value(value) {
                        derived.insert(name, value);
                    }
                }
            }
        }
        if let Some(missing) = self.metadata.get().and_then(|m| m.missing.as_ref()) {
            derived
                .base
                .add_missing_property_source(Arc::clone(missing));
        }
        derived
    }
}

/// A Value of the response as a [`PropertyValue`], or `None` for an object.
fn property_value(value: &Value) -> Option<PropertyValue> {
    match value {
        Value::Bool(value) => Some(PropertyValue::Bool(*value)),
        Value::Number(number) => number
            .as_i64()
            .map(PropertyValue::Integer)
            .or_else(|| number.as_f64().map(PropertyValue::Double)),
        Value::String(text) => Some(PropertyValue::String(text.clone())),
        Value::Array(items) => Some(PropertyValue::StringList(
            items
                .iter()
                .map(|item| match item {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .collect(),
        )),
        Value::Object(_) | Value::Null => None,
    }
}

impl FlowElement for DerivedCloudEngine {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        self.ensure_metadata();
        let derived = self.derived_data(data)?;
        data.get_or_add(DERIVED_DATA_KEY, || derived)?;
        Ok(())
    }

    fn data_key(&self) -> &str {
        DERIVED_ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        // The engine reads the request engine's response, not evidence.
        &self.evidence_filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        match self.metadata.get() {
            Some(metadata) => &metadata.properties,
            None => &self.default_properties,
        }
    }

    fn properties_loaded(&self) -> bool {
        self.ensure_metadata();
        self.has_loaded_properties()
    }
}

impl AspectEngine for DerivedCloudEngine {
    fn engine_type_name(&self) -> &str {
        ENGINE_NAME
    }

    fn data_source_tier(&self) -> &str {
        ""
    }

    fn deployment(&self) -> EngineDeployment {
        EngineDeployment::Cloud
    }

    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        match self.metadata.get() {
            Some(metadata) => &metadata.aspect_properties,
            None => &self.default_aspect_properties,
        }
    }

    fn has_loaded_properties(&self) -> bool {
        self.metadata.get().is_some()
    }

    fn property_list_request_failed(&self) -> bool {
        !self.has_loaded_properties()
            && self
                .request_engine
                .as_ref()
                .is_some_and(|engine| engine.public_properties().is_err())
    }

    fn missing_property_reason(&self, property_name: &str) -> MissingPropertyResult {
        self.ensure_metadata();
        default_missing_property_reason(self, property_name)
    }
}

/// A fluent builder for [`DerivedCloudEngine`].
#[derive(Default)]
pub struct DerivedCloudEngineBuilder {
    request_engine: Option<Arc<CloudRequestEngine>>,
}

impl DerivedCloudEngineBuilder {
    /// A builder with no request engine set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the [`CloudRequestEngine`] whose response and property list the
    /// engine reads. Add the same request engine to the Pipeline before this
    /// engine.
    pub fn cloud_request_engine(mut self, engine: Arc<CloudRequestEngine>) -> Self {
        self.request_engine = Some(engine);
        self
    }

    /// Build the engine.
    pub fn build(self) -> Result<DerivedCloudEngine> {
        let request_engine = self.request_engine.ok_or_else(|| {
            Error::configuration(
                "a DerivedCloudEngine requires a CloudRequestEngine; set one with \
                 .cloud_request_engine(..)",
            )
        })?;
        let default_properties: Vec<PropertyMetaData> = [HUMAN_CONFIDENCE, HUMAN_CONFIDENCE_SOURCE]
            .into_iter()
            .map(|name| {
                PropertyMetaData::new(name, DERIVED_ELEMENT_DATA_KEY, PropertyValueType::String)
            })
            .collect();
        let engine = DerivedCloudEngine {
            request_engine: Some(request_engine),
            evidence_filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            metadata: OnceLock::new(),
            default_aspect_properties: default_properties
                .iter()
                .cloned()
                .map(AspectPropertyMetaData::from_core)
                .collect(),
            default_properties,
        };
        engine.ensure_metadata();
        Ok(engine)
    }
}
