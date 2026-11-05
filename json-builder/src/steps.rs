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

//! The steps the JSON builder takes, which a caller can replace one at a
//! time.

use std::collections::{HashMap, HashSet};
use std::panic::RefUnwindSafe;

use fiftyone_pipeline_core::{
    ElementData, FlowData, NoValueError, Pipeline, PropertyMetaData, PropertyValue,
    PropertyValueType,
};

use crate::default_steps;
use crate::element::JsonBuilderElement;
use crate::value::JsonObject;

/// The property metadata of one pipeline that the builder needs to add the
/// `delayexecution` and `evidenceproperties` keys, and to keep JavaScript
/// properties when only some properties are selected.
///
/// Each collection holds complete names, being the lowercased element data
/// key and property name joined by a dot, for example `location.javascript`.
#[derive(Debug, Clone, Default)]
pub struct PipelineConfig {
    delayed_execution: HashSet<String>,
    delayed_evidence: HashMap<String, Vec<String>>,
    javascript: HashSet<String>,
}

impl PipelineConfig {
    /// The complete names of the JavaScript properties whose delay execution
    /// flag is set.
    pub fn delayed_execution_properties(&self) -> &HashSet<String> {
        &self.delayed_execution
    }

    /// For each property with evidence properties, the complete names of
    /// those that are delayed execution JavaScript properties. A property is
    /// listed only when at least one is.
    pub fn delayed_evidence_properties(&self) -> &HashMap<String, Vec<String>> {
        &self.delayed_evidence
    }

    /// The complete names of the JavaScript properties, item properties of a
    /// list of records included.
    pub fn javascript_properties(&self) -> &HashSet<String> {
        &self.javascript
    }

    /// Read the available property metadata of every element whose data the
    /// output can hold.
    pub(crate) fn from_pipeline(pipeline: &Pipeline, element: &JsonBuilderElement) -> Self {
        let mut config = PipelineConfig::default();
        for flow_element in pipeline.flow_elements() {
            let element_key = flow_element.data_key().to_lowercase();
            if element.is_element_excluded(&element_key) {
                continue;
            }
            for property in flow_element.properties().iter().filter(|p| p.available) {
                config.record_property(&element_key, property);
            }
        }
        // An evidence property may be declared by an element later in the
        // pipeline than the property naming it, so the lists are filtered
        // once every element has been read.
        config.retain_delayed_evidence();
        config
    }

    /// Record one property, and its item properties, under a data path.
    fn record_property(&mut self, data_path: &str, property: &PropertyMetaData) {
        let name = property.name.to_lowercase();
        let path = format!("{data_path}.{name}");

        if property.value_type == PropertyValueType::JavaScript {
            self.javascript.insert(path.clone());
            if property.delay_execution {
                self.delayed_execution.insert(path.clone());
            }
        }
        if !property.evidence_properties.is_empty() {
            // Evidence properties are named within the same element, so they
            // take the element data key as their prefix.
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

    /// Keep only the evidence properties that are delayed execution
    /// JavaScript, and drop a property left with none, as the specification
    /// lists only those the client has to run.
    fn retain_delayed_evidence(&mut self) {
        let delayed = &self.delayed_execution;
        self.delayed_evidence.retain(|_, evidence| {
            evidence.retain(|name| delayed.contains(name));
            !evidence.is_empty()
        });
    }
}

/// What every step is given: the element, the flow data being written and
/// the pipeline's property metadata.
#[derive(Clone, Copy)]
pub struct JsonBuilderContext<'a> {
    element: &'a JsonBuilderElement,
    data: &'a FlowData,
    config: &'a PipelineConfig,
}

impl<'a> JsonBuilderContext<'a> {
    /// Bundle the element, the flow data and the pipeline's property
    /// metadata.
    pub fn new(
        element: &'a JsonBuilderElement,
        data: &'a FlowData,
        config: &'a PipelineConfig,
    ) -> Self {
        JsonBuilderContext {
            element,
            data,
            config,
        }
    }

    /// The JSON builder doing the writing, for its exclusion lists and its
    /// value conversion.
    pub fn element(&self) -> &'a JsonBuilderElement {
        self.element
    }

    /// The flow data being written.
    pub fn data(&self) -> &'a FlowData {
        self.data
    }

    /// The pipeline's delayed execution and evidence property names.
    pub fn config(&self) -> &'a PipelineConfig {
        self.config
    }
}

/// The steps the JSON builder takes to write a flow data, each of which a
/// caller can replace.
///
/// Every method has a default, so an implementation replaces only the steps
/// it changes. The defaults call one another through `self`, so a replaced
/// step is used wherever the builder takes it. To keep the default for part
/// of a step, call the function of the same name in
/// [`default_steps`](crate::default_steps). Pass the implementation to
/// [`JsonBuilderElementBuilder::set_steps`](crate::JsonBuilderElementBuilder::set_steps).
///
/// The steps match the protected virtual methods of .NET's
/// `JsonBuilderElement`, as follows.
///
/// | Method                           | .NET                          |
/// |----------------------------------|-------------------------------|
/// | [`build_json`](Self::build_json) | `BuildJson(data, config, ...)` |
/// | [`write_json`](Self::write_json) | `BuildJson(allProperties)`    |
/// | [`get_all_properties`](Self::get_all_properties) | `GetAllProperties` |
/// | [`get_values`](Self::get_values) | `GetValues`                   |
/// | [`add_json_values_for_property`](Self::add_json_values_for_property) | `AddJsonValuesForProperty` |
/// | [`get_javascript_properties`](Self::get_javascript_properties) | `GetJavaScriptProperties` |
/// | [`add_errors`](Self::add_errors) | `AddErrors`                   |
/// | [`add_warnings`](Self::add_warnings) | `AddWarnings`             |
///
/// An implementation must be `RefUnwindSafe` so the builder and element
/// holding it stay `UnwindSafe` and `RefUnwindSafe`. Keep any state a step
/// changes in a `Mutex` or an atomic, which are both.
///
/// # Example
///
/// Writing only the `ismobile` property of each element, so no other
/// property is read.
///
/// ```
/// use fiftyone_json_builder::{
///     JsonBuilderContext, JsonBuilderElement, JsonBuilderSteps, JsonObject,
/// };
/// use fiftyone_pipeline_core::ElementData;
///
/// struct IsMobileOnly;
///
/// impl JsonBuilderSteps for IsMobileOnly {
///     fn get_values(
///         &self,
///         context: &JsonBuilderContext<'_>,
///         data_path: &str,
///         source: &dyn ElementData,
///     ) -> JsonObject {
///         let mut values = JsonObject::new();
///         if source.has_property("ismobile") {
///             self.add_json_values_for_property(
///                 context,
///                 &mut values,
///                 data_path,
///                 "ismobile",
///                 source.get("ismobile"),
///                 false,
///             );
///         }
///         values
///     }
/// }
///
/// let element = JsonBuilderElement::builder()
///     .set_steps(IsMobileOnly)
///     .build();
/// ```
pub trait JsonBuilderSteps: Send + Sync + RefUnwindSafe {
    /// Write the whole flow data as JSON text.
    ///
    /// The default collects the element data with
    /// [`get_all_properties`](Self::get_all_properties), adds the
    /// `javascriptProperties` list while the sequence number is below
    /// [`MAX_JAVASCRIPT_ITERATIONS`](crate::MAX_JAVASCRIPT_ITERATIONS), adds
    /// the errors and then the warnings, and writes the result with
    /// [`write_json`](Self::write_json).
    fn build_json(&self, context: &JsonBuilderContext<'_>) -> String {
        default_steps::build_json(self, context)
    }

    /// Write the collected values as JSON text.
    ///
    /// The default writes them as .NET's builder does, indented by two
    /// spaces with keys in the order they were added, ending lines with the
    /// platform's line ending. See [`write_indented`](crate::write_indented).
    fn write_json(&self, context: &JsonBuilderContext<'_>, all_properties: &JsonObject) -> String {
        default_steps::write_json(self, context, all_properties)
    }

    /// Collect the values of every element data in the output, keyed by the
    /// lowercased element data key.
    ///
    /// The default leaves out the excluded elements and collects each other
    /// element's values with [`get_values`](Self::get_values).
    fn get_all_properties(&self, context: &JsonBuilderContext<'_>) -> JsonObject {
        default_steps::get_all_properties(self, context)
    }

    /// Collect the values of one element data, or of one record in a list
    /// of records.
    ///
    /// `data_path` is the dotted path of the object the values go in, for
    /// example `device` or `hardware.profiles`. The default leaves out the
    /// excluded property names, writes only the meta properties of a
    /// property that is not selected (see
    /// [`JsonBuilderElement::is_property_selected`]) unless it is a
    /// JavaScript property, and adds each other property with
    /// [`add_json_values_for_property`](Self::add_json_values_for_property).
    fn get_values(
        &self,
        context: &JsonBuilderContext<'_>,
        data_path: &str,
        source: &dyn ElementData,
    ) -> JsonObject {
        default_steps::get_values(self, context, data_path, source)
    }

    /// Add the keys for one property to `json_values`.
    ///
    /// The default adds the value under the lowercased name, or `null` and a
    /// `nullreason` key when the property has no value. Unless
    /// `include_value_data_only` is set, it then adds `delayexecution` and
    /// `evidenceproperties` where the pipeline's metadata asks for them. A
    /// list of records is written as an array of objects, each collected
    /// with [`get_values`](Self::get_values). Any other value is converted by
    /// [`JsonBuilderElement::to_json_value`].
    fn add_json_values_for_property(
        &self,
        context: &JsonBuilderContext<'_>,
        json_values: &mut JsonObject,
        data_path: &str,
        name: &str,
        value: Result<PropertyValue, NoValueError>,
        include_value_data_only: bool,
    ) {
        default_steps::add_json_values_for_property(
            self,
            context,
            json_values,
            data_path,
            name,
            value,
            include_value_data_only,
        );
    }

    /// The complete names of the JavaScript properties for the
    /// `javascriptProperties` list.
    ///
    /// `available_properties` holds the complete names of the keys collected
    /// for the output. The default gives every available JavaScript property
    /// that is also in `available_properties`.
    fn get_javascript_properties(
        &self,
        context: &JsonBuilderContext<'_>,
        available_properties: &[String],
    ) -> Vec<String> {
        default_steps::get_javascript_properties(self, context, available_properties)
    }

    /// Add the flow data's errors.
    ///
    /// The default adds an `errors` array of their messages, in the order
    /// they were recorded, when there are any.
    fn add_errors(&self, context: &JsonBuilderContext<'_>, all_properties: &mut JsonObject) {
        default_steps::add_errors(self, context, all_properties);
    }

    /// Add the flow data's warnings.
    ///
    /// The default adds a `warnings` array of their messages, in the order
    /// they were recorded, when there are any. When `all_properties` already
    /// holds a `warnings` array of strings, the messages are added after
    /// them.
    fn add_warnings(&self, context: &JsonBuilderContext<'_>, all_properties: &mut JsonObject) {
        default_steps::add_warnings(self, context, all_properties);
    }
}

/// The builder's own steps, each one the default.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultJsonBuilderSteps;

impl JsonBuilderSteps for DefaultJsonBuilderSteps {}
