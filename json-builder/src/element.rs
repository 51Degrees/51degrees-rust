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

use std::collections::{HashMap, HashSet};

use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData, PropertyValue, PropertyValueType, Result,
};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;
use serde_json::{Map, Value};

use crate::builder::JsonBuilderElementBuilder;
use crate::constants::{
    DELAY_EXECUTION_SUFFIX, ERRORS_KEY, EVIDENCE_PROPERTIES_SUFFIX, JAVASCRIPT_PROPERTIES_KEY,
    JSON_BUILDER_ELEMENT_DATA_KEY, JSON_PROPERTY_KEY, MAX_JAVASCRIPT_ITERATIONS,
    NULL_REASON_SUFFIX, SEQUENCE_EVIDENCE_KEY,
};
use crate::data::{JsonBuilderData, JSON_BUILDER_DATA_KEY};

/// The pipeline-derived metadata the builder needs to add the `delayexecution`
/// and `evidenceproperties` sibling keys.
///
/// It is computed once per pipeline from the elements' property metadata and is
/// keyed by the lowercased dotted path `elementkey.propertyname`, exactly the
/// path the value is emitted under. Both maps are stable to build (they iterate
/// the pipeline elements in order) and read-only afterwards.
#[derive(Debug, Default)]
struct PipelineConfig {
    /// Dotted paths whose property is a JavaScript value that should not run
    /// automatically on the client.
    delayed_execution: HashSet<String>,
    /// Dotted path to the list of delayed-execution JavaScript property paths
    /// whose execution would gather evidence for that property. Filtered to only
    /// delayed-execution entries by [`PipelineConfig::retain_delayed_evidence`].
    delayed_evidence: HashMap<String, Vec<String>>,
}

impl PipelineConfig {
    /// Walk every element's available property metadata and record which
    /// dotted paths are delayed-execution JavaScript and which carry evidence
    /// properties. The JSON builder, JavaScript builder and other excluded
    /// elements are skipped because their data never reaches the output, and
    /// so are properties marked unavailable, as .NET reads only the pipeline's
    /// available properties.
    fn from_pipeline(pipeline: &Pipeline, element_exclusion: &HashSet<String>) -> Self {
        let mut config = PipelineConfig::default();
        for element in pipeline.flow_elements() {
            let element_key = element.data_key().to_lowercase();
            if element_exclusion.contains(&element_key) {
                continue;
            }
            for property in element.properties().iter().filter(|p| p.available) {
                config.record_property(&element_key, property);
            }
        }
        // The evidence lists are filtered against the delayed-execution set only
        // after every element has been walked, because an evidence property may
        // be declared by an element later in the pipeline than the property that
        // references it.
        config.retain_delayed_evidence();
        config
    }

    /// Record one property (and recurse into its item properties) under the
    /// given dotted path prefix.
    fn record_property(&mut self, data_path: &str, property: &PropertyMetaData) {
        let name = property.name.to_lowercase();
        let path = format!("{data_path}.{name}");

        if property.delay_execution && property.value_type == PropertyValueType::JavaScript {
            self.delayed_execution.insert(path.clone());
        }
        if !property.evidence_properties.is_empty() {
            // Evidence property names are stored against the dotted path of the
            // property they help determine, prefixed with the same element key.
            // This is the raw list. retain_delayed_evidence later keeps only the
            // entries that are themselves delayed-execution JavaScript.
            let prefix = data_path
                .split_once('.')
                .map_or(data_path, |(element, _)| element);
            let evidence: Vec<String> = property
                .evidence_properties
                .iter()
                .map(|p| format!("{prefix}.{}", p.to_lowercase()))
                .collect();
            self.delayed_evidence.insert(path.clone(), evidence);
        }
        for item in &property.item_properties {
            self.record_property(&path, item);
        }
    }

    /// Drop every recorded evidence name that is not itself a delayed-execution
    /// JavaScript property, and drop any property left with an empty list. The
    /// full delayed-execution set is known only after the whole pipeline has been
    /// walked, so this runs as a second pass. As the specification requires, an
    /// evidence property is listed only when the client has to run it to gather
    /// the evidence.
    fn retain_delayed_evidence(&mut self) {
        let delayed = &self.delayed_execution;
        self.delayed_evidence.retain(|_, evidence| {
            evidence.retain(|name| delayed.contains(name));
            !evidence.is_empty()
        });
    }
}

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
/// Two top-level keys may be appended:
///
/// - `javascriptProperties` is an array of the dotted paths
///   (`element.property`) of every JavaScript-typed property present. It is
///   only emitted while the request sequence number is below
///   [`crate::MAX_JAVASCRIPT_ITERATIONS`]. This key keeps its original casing.
/// - `errors` is an array of the messages of the errors the flow data
///   recorded, in the order they were recorded, present only when there are
///   any. The specification gives it this shape and the client script reads it
///   as one.
///
/// # Determinism
///
/// Element keys and property names are sorted before serialization and the
/// `serde_json` map preserves insertion order, so the same flow data always
/// produces byte-identical JSON. This stability is required for stable ETags
/// downstream.
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
        // The pipeline config (delay-execution and evidence-property paths) is
        // derived from the element metadata of the pipeline that owns this flow
        // data. It is cheap to compute and avoids holding a Sync cache of
        // pipeline-keyed state on this Send + Sync element.
        let config = PipelineConfig::from_pipeline(data.pipeline(), &self.element_exclusion);
        self.build_json(data, &config)
    }

    /// Internal constructor used by the builder.
    pub(crate) fn from_parts(
        element_exclusion: HashSet<String>,
        property_exclusion: HashSet<String>,
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
        }
    }

    /// True if the element with this lowercased data key is excluded from the
    /// output.
    fn is_element_excluded(&self, lowercased_key: &str) -> bool {
        self.element_exclusion.contains(lowercased_key)
    }

    /// True if the property with this lowercased name is excluded from the
    /// output.
    fn is_property_excluded(&self, lowercased_name: &str) -> bool {
        self.property_exclusion.contains(lowercased_name)
    }

    /// The request sequence number that the cap is compared against.
    ///
    /// Taken from the sequence element's data when that element ran, because
    /// it has already added one to any `query.sequence` evidence. The other
    /// 51Degrees pipelines write that new value back into the evidence before
    /// their JSON builder reads it, whilst evidence is immutable here, so
    /// reading the evidence alone kept the list for one request more than they
    /// do. Without a sequence element the evidence is read instead.
    ///
    /// Returns `None` when neither holds a whole number. The caller treats a
    /// missing sequence number as "do not cap" by defaulting to `0`, which
    /// keeps the JavaScript-properties list on for direct, non-web callers that
    /// never set the sequence.
    fn sequence_number(data: &FlowData) -> Option<i64> {
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

    /// Build the full JSON document for the flow data.
    fn build_json(&self, data: &FlowData, config: &PipelineConfig) -> String {
        let mut root = Map::new();

        // Element data, in sorted key order for determinism.
        let mut element_keys: Vec<String> = data
            .data_keys()
            .into_iter()
            .map(|k| k.to_lowercase())
            .filter(|k| !self.is_element_excluded(k))
            .collect();
        element_keys.sort();
        element_keys.dedup();

        for element_key in &element_keys {
            if let Some(element_data) = data.get_str(element_key) {
                let values = self.element_values(element_data, element_key, config);
                root.insert(element_key.clone(), Value::Object(values));
            }
        }

        // The JavaScript-properties list, capped by the sequence number.
        let sequence = Self::sequence_number(data).unwrap_or(0);
        if sequence < MAX_JAVASCRIPT_ITERATIONS {
            let javascript_properties = self.javascript_property_paths(data);
            if !javascript_properties.is_empty() {
                let array = javascript_properties
                    .into_iter()
                    .map(Value::String)
                    .collect();
                root.insert(JAVASCRIPT_PROPERTIES_KEY.to_owned(), Value::Array(array));
            }
        }

        // Flow errors, as the list of their messages.
        let errors = Self::collect_errors(data);
        if !errors.is_empty() {
            root.insert(ERRORS_KEY.to_owned(), Value::Array(errors));
        }

        // `serde_json::to_string_pretty` preserves the insertion order because
        // the crate is built with the `preserve_order` feature.
        serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Serialize one element's data into a JSON object, adding the sibling keys.
    fn element_values(
        &self,
        element_data: &dyn ElementData,
        element_key: &str,
        config: &PipelineConfig,
    ) -> Map<String, Value> {
        let mut values = Map::new();

        let mut names: Vec<String> = element_data
            .keys()
            .into_iter()
            .map(|n| n.to_lowercase())
            .filter(|n| !self.is_property_excluded(n))
            .collect();
        names.sort();
        names.dedup();

        for name in &names {
            let data_path = format!("{element_key}.{name}");
            match element_data.get(name) {
                Ok(value) => {
                    values.insert(name.clone(), property_value_to_json(&value));
                    if config.delayed_execution.contains(&data_path) {
                        values.insert(format!("{name}{DELAY_EXECUTION_SUFFIX}"), Value::Bool(true));
                    }
                }
                Err(no_value) => {
                    // A null-valued property emits the value as JSON null and a
                    // sibling explaining why, following the nullreason rule.
                    values.insert(name.clone(), Value::Null);
                    values.insert(
                        format!("{name}{NULL_REASON_SUFFIX}"),
                        Value::String(no_value.message),
                    );
                }
            }
            if let Some(evidence) = config.delayed_evidence.get(&data_path) {
                let array = evidence.iter().cloned().map(Value::String).collect();
                values.insert(
                    format!("{name}{EVIDENCE_PROPERTIES_SUFFIX}"),
                    Value::Array(array),
                );
            }
        }

        values
    }

    /// Collect the dotted paths of every JavaScript-typed property present in
    /// the output, in sorted order.
    ///
    /// A property counts as JavaScript when its metadata declares the JavaScript
    /// value type; the path is `elementkey.propertyname`. Excluded elements,
    /// excluded properties and properties marked unavailable never contribute.
    fn javascript_property_paths(&self, data: &FlowData) -> Vec<String> {
        let mut paths = Vec::new();
        for element in data.pipeline().flow_elements() {
            let element_key = element.data_key().to_lowercase();
            if self.is_element_excluded(&element_key) {
                continue;
            }
            // Only report properties whose data is actually present for this
            // request, so the client is not asked to run JavaScript for an
            // element that did not run.
            let present = data.get_str(&element_key).is_some();
            if !present {
                continue;
            }
            for property in element.properties().iter().filter(|p| p.available) {
                let name = property.name.to_lowercase();
                if property.value_type == PropertyValueType::JavaScript
                    && !self.is_property_excluded(&name)
                {
                    paths.push(format!("{element_key}.{name}"));
                }
            }
        }
        paths.sort();
        paths.dedup();
        paths
    }

    /// The messages of the flow data's errors, in the order they were
    /// recorded.
    fn collect_errors(data: &FlowData) -> Vec<Value> {
        data.errors()
            .iter()
            .map(|error| Value::String(error.source.to_string()))
            .collect()
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

/// Convert a core [`PropertyValue`] to a `serde_json` value.
///
/// A [`PropertyValue::JavaScript`] is serialized as a plain string (the snippet)
/// because the client treats it as code, and a [`PropertyValue::KeyValueList`]
/// becomes an array of objects so nested records survive the round trip.
fn property_value_to_json(value: &PropertyValue) -> Value {
    match value {
        PropertyValue::String(s) | PropertyValue::JavaScript(s) => Value::String(s.clone()),
        PropertyValue::Bool(b) => Value::Bool(*b),
        PropertyValue::Integer(i) => Value::Number((*i).into()),
        PropertyValue::Double(d) => serde_json::Number::from_f64(*d)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        PropertyValue::StringList(list) => {
            Value::Array(list.iter().cloned().map(Value::String).collect())
        }
        PropertyValue::KeyValueList(records) => Value::Array(
            records
                .iter()
                .map(|record| {
                    let mut object = Map::new();
                    for (key, inner) in record {
                        object.insert(key.to_lowercase(), property_value_to_json(inner));
                    }
                    Value::Object(object)
                })
                .collect(),
        ),
        // PropertyValue is non_exhaustive; emit null for any future variant
        // rather than failing to compile or panicking.
        _ => Value::Null,
    }
}
