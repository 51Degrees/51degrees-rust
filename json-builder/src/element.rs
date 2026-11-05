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

//! The JSON builder flow element.

use std::collections::HashSet;
use std::sync::Arc;

use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
    PropertyValue, PropertyValueType, Result,
};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

use crate::builder::JsonBuilderElementBuilder;
use crate::constants::{JSON_BUILDER_ELEMENT_DATA_KEY, JSON_PROPERTY_KEY, SEQUENCE_EVIDENCE_KEY};
use crate::converter::JsonValueConverter;
use crate::data::{JsonBuilderData, JSON_BUILDER_DATA_KEY};
use crate::steps::{JsonBuilderContext, JsonBuilderSteps, PipelineConfig};
use crate::value::JsonValue;

/// Serializes all element data in a flow data into a single JSON object.
///
/// # Output shape
///
/// The produced document is an object whose keys are the lowercased data keys of
/// the elements that ran, excluding the internal elements in
/// [`crate::DEFAULT_ELEMENT_EXCLUSION_LIST`]. Each element maps to a nested
/// object of its property values, keyed by lowercased property name. For a
/// property the following sibling keys may also appear:
///
/// - `<name>` carries the value. A property that is present in the element data
///   but has no value (its [`fiftyone_pipeline_core::ElementData::get`] returns
///   `Err`) emits JSON `null`. A property the producing element never stored is
///   simply absent from the output, because serialization works from the stored
///   value dictionary rather than the declared metadata.
/// - `<name>nullreason` carries the explanation string when the property is
///   present but has no value.
/// - `<name>delayexecution` is `true` when the property is a JavaScript value
///   whose execution is deferred.
/// - `<name>evidenceproperties` lists the dotted paths of the JavaScript
///   properties whose execution would gather evidence for this property.
///
/// Three top-level keys may be appended:
///
/// - `javascriptProperties` is an array of the dotted paths
///   (`element.property`) of every JavaScript-typed property in the output. It
///   is only emitted while the request sequence number is below
///   [`crate::MAX_JAVASCRIPT_ITERATIONS`]. This key keeps its original casing.
/// - `errors` is an array of the messages of the errors the flow data
///   recorded, in the order they were recorded, present only when there are
///   any. The specification gives it this shape and the client script reads it
///   as one.
/// - `warnings` is an array of the messages of the flow data's warnings, in
///   the order they were recorded, present only when there are any.
///
/// # Changing the output
///
/// [`JsonBuilderSteps`] lets a caller replace any of the steps the builder
/// takes, and [`JsonValueConverter`] lets a caller change how a property value
/// is written. Both are given to the [`JsonBuilderElementBuilder`].
///
/// # Order
///
/// Elements are written in the order their data was added to the flow data,
/// and each element's properties in the order its element data lists them,
/// as .NET writes them. `javascriptProperties` lists paths in the order of
/// the pipeline's flow elements, then of each element's properties. Each
/// order comes from the flow data and its pipeline, so the same flow data
/// always produces byte-identical JSON, which stable ETags downstream rely
/// on.
///
/// # Example
///
/// ```
/// use std::sync::Arc;
/// use fiftyone_pipeline_core::{
///     ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist,
///     FlowData, FlowElement, MapElementData, Pipeline, PropertyMetaData,
///     PropertyValueType, Result, TypedKey,
/// };
/// use fiftyone_json_builder::{JsonBuilderElement, JSON_BUILDER_DATA_KEY};
///
/// // A tiny element that publishes one property.
/// struct DeviceData(MapElementData);
/// impl ElementData for DeviceData {
///     fn get(&self, name: &str) -> std::result::Result<
///         fiftyone_pipeline_core::PropertyValue,
///         fiftyone_pipeline_core::NoValueError,
///     > { self.0.get(name) }
///     fn keys(&self) -> Vec<String> { self.0.keys() }
///     fn as_any(&self) -> &dyn std::any::Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
/// }
/// struct DeviceElement { filter: EvidenceKeyFilterWhitelist, props: Vec<PropertyMetaData> }
/// impl DeviceElement {
///     const KEY: TypedKey<DeviceData> = TypedKey::new("device");
/// }
/// impl FlowElement for DeviceElement {
///     fn process(&self, data: &mut FlowData) -> Result<()> {
///         data.get_or_add(Self::KEY, || {
///             DeviceData(MapElementData::new().set("ismobile", true))
///         })?;
///         Ok(())
///     }
///     fn data_key(&self) -> &str { "device" }
///     fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter { &self.filter }
///     fn properties(&self) -> &[PropertyMetaData] { &self.props }
/// }
///
/// let pipeline = Pipeline::builder()
///     .add_element(Arc::new(DeviceElement {
///         filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
///         props: vec![PropertyMetaData::new("ismobile", "device", PropertyValueType::Bool)],
///     }))
///     .add_element(Arc::new(JsonBuilderElement::new()))
///     .build()
///     .unwrap();
///
/// let mut data = pipeline.create_flow_data_with(
///     Evidence::builder().add("query.sequence", "1").build(),
/// );
/// data.process().unwrap();
///
/// let json = data.get(JSON_BUILDER_DATA_KEY).unwrap().json().to_owned();
/// assert!(json.contains("\"ismobile\": true"));
/// ```
pub struct JsonBuilderElement {
    evidence_key_filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    element_exclusion: HashSet<String>,
    property_exclusion: HashSet<String>,
    steps: Arc<dyn JsonBuilderSteps>,
    converters: Vec<Arc<dyn JsonValueConverter>>,
}

impl JsonBuilderElement {
    /// Create a JSON builder with the default element and property exclusion
    /// lists (see [`crate::DEFAULT_ELEMENT_EXCLUSION_LIST`] and
    /// [`crate::DEFAULT_PROPERTY_EXCLUSION_LIST`]).
    pub fn new() -> Self {
        JsonBuilderElementBuilder::new().build()
    }

    /// Start configuring a JSON builder.
    pub fn builder() -> JsonBuilderElementBuilder {
        JsonBuilderElementBuilder::new()
    }

    /// The JSON this element produces for the flow data, without adding it to
    /// the flow data.
    ///
    /// The client-side endpoints serve this when the element left no data for
    /// a request.
    pub fn json_for(&self, data: &FlowData) -> String {
        // The property metadata is read from the pipeline that owns this
        // flow data each time, so the element holds no state per pipeline.
        let config = PipelineConfig::from_pipeline(data.pipeline(), self);
        let context = JsonBuilderContext::new(self, data, &config);
        self.steps.build_json(&context)
    }

    /// True if the element data with this key is left out of the output. The
    /// key is matched without regard to case.
    pub fn is_element_excluded(&self, data_key: &str) -> bool {
        self.element_exclusion.contains(&data_key.to_lowercase())
    }

    /// True if properties with this name are left out of the output. The name
    /// is matched without regard to case.
    pub fn is_property_excluded(&self, name: &str) -> bool {
        self.property_exclusion.contains(&name.to_lowercase())
    }

    /// The request sequence number that the `javascriptProperties` cap is
    /// compared against, or `None` when there is none.
    ///
    /// Taken from the sequence element's data when that element ran, because
    /// it has already added one to any `query.sequence` evidence. The other
    /// 51Degrees pipelines write that new value back into the evidence before
    /// their JSON builder reads it, whilst evidence is immutable here, so
    /// reading the evidence alone would keep the list for one request more
    /// than they do. Without a sequence element the evidence is read instead.
    pub fn sequence_number(data: &FlowData) -> Option<i64> {
        if let Some(sequence) = data
            .get(SequenceElement::KEY)
            .and_then(|sequence| sequence.sequence())
        {
            return Some(sequence);
        }
        data.evidence()
            .get(SEQUENCE_EVIDENCE_KEY)
            .and_then(|s| s.trim().parse::<i64>().ok())
    }

    /// Convert a property value to JSON, asking the converters added to the
    /// builder first, in order, and then using the builder's own conversion.
    ///
    /// The builder's own conversion writes a string or JavaScript as a string,
    /// a list of strings as an array, and a list of records as an array of
    /// objects with lowercased keys.
    pub fn to_json_value(&self, value: &PropertyValue) -> JsonValue {
        self.convert_by_caller(value)
            .unwrap_or_else(|| self.built_in_json_value(value))
    }

    /// The answer of the first converter that gives one.
    pub(crate) fn convert_by_caller(&self, value: &PropertyValue) -> Option<JsonValue> {
        self.converters
            .iter()
            .find_map(|converter| converter.convert(self, value))
    }

    /// The builder's own conversion. Values held inside this one go through
    /// [`JsonBuilderElement::to_json_value`], so the converters see them.
    pub(crate) fn built_in_json_value(&self, value: &PropertyValue) -> JsonValue {
        match value {
            PropertyValue::String(s) | PropertyValue::JavaScript(s) => JsonValue::String(s.clone()),
            PropertyValue::Bool(b) => JsonValue::Bool(*b),
            PropertyValue::Integer(i) => JsonValue::Integer(*i),
            PropertyValue::Double(d) => JsonValue::Double(*d),
            PropertyValue::StringList(list) => {
                JsonValue::Array(list.iter().cloned().map(JsonValue::String).collect())
            }
            PropertyValue::KeyValueList(records) => JsonValue::Array(
                records
                    .iter()
                    .map(|record| {
                        JsonValue::Object(
                            record
                                .iter()
                                .map(|(key, inner)| (key.to_lowercase(), self.to_json_value(inner)))
                                .collect(),
                        )
                    })
                    .collect(),
            ),
            // PropertyValue is non_exhaustive, so a variant added later is
            // written as null until it is given a form here.
            _ => JsonValue::Null,
        }
    }

    /// Internal constructor used by the builder.
    pub(crate) fn from_parts(
        element_exclusion: HashSet<String>,
        property_exclusion: HashSet<String>,
        steps: Arc<dyn JsonBuilderSteps>,
        converters: Vec<Arc<dyn JsonValueConverter>>,
    ) -> Self {
        JsonBuilderElement {
            // The JSON builder reads no evidence directly to build its output;
            // the sequence number it consults is added by the pipeline's
            // sequence element, so the public filter is empty.
            evidence_key_filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: vec![PropertyMetaData::new(
                JSON_PROPERTY_KEY,
                JSON_BUILDER_ELEMENT_DATA_KEY,
                PropertyValueType::String,
            )],
            element_exclusion,
            property_exclusion,
            steps,
            converters,
        }
    }
}

impl Default for JsonBuilderElement {
    fn default() -> Self {
        JsonBuilderElement::new()
    }
}

impl FlowElement for JsonBuilderElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let json = self.json_for(data);

        let result = data.get_or_add(JSON_BUILDER_DATA_KEY, JsonBuilderData::new);
        match result {
            Ok(element_data) => {
                element_data.set_json(json);
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn data_key(&self) -> &str {
        JSON_BUILDER_ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.evidence_key_filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}
