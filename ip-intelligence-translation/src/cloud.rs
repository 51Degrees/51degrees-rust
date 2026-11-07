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

//! The engine that reads the translated country names from the cloud
//! response, and its builder.

use std::collections::HashMap;
use std::sync::Arc;

use fiftyone_cloud_request_engine::{CloudRequestEngine, LicensedProducts};
use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
    PropertyValueType, Result, WeightedValue,
};
use fiftyone_pipeline_engines::{
    default_missing_property_reason, AspectEngine, AspectPropertyMetaData, AspectPropertyValue,
    EngineDeployment, MissingPropertyResult, MissingPropertySource,
};
use fiftyone_pipeline_translation::{TranslationData, TranslationValue};
use once_cell::sync::OnceCell;
use serde_json::{Map, Value};

use crate::data::{
    countries_translated_properties, COUNTRY_NAMES_TRANSLATED_KEY,
    COUNTRY_NAMES_TRANSLATED_KEY_NAME,
};

/// The engine's name in log records and messages.
const ENGINE_NAME: &str = "CloudCountriesTranslationEngine";

/// The end of a key giving the reason the property it starts with is null.
const NULL_REASON_SUFFIX: &str = "nullreason";

/// The reason given for a null value the response gives no reason for.
const UNKNOWN_REASON: &str = "Unknown";

/// The field of a weighted item holding its weighting, matched ignoring
/// case.
const RAW_WEIGHTING_FIELD: &str = "rawweighting";

/// The field of a weighted item holding its value, matched ignoring case.
const VALUE_FIELD: &str = "value";

/// The start of the cloud type of a weighted property, matched ignoring
/// case.
const WEIGHTED_TYPE_PREFIX: &str = "weighted";

/// The cloud type of a list of strings, matched ignoring case.
const ARRAY_TYPE: &str = "array";

/// The form a property's value takes, from the type the cloud gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    /// A list of strings, each with its weighting, from a `Weighted` type.
    Weighted,
    /// A list of strings, from the `Array` type.
    List,
    /// A single value, from any other type, held as text.
    Text,
}

impl Form {
    fn from_cloud_type(value_type: &str) -> Self {
        let value_type = value_type.to_ascii_lowercase();
        if value_type.starts_with(WEIGHTED_TYPE_PREFIX) {
            Form::Weighted
        } else if value_type == ARRAY_TYPE {
            Form::List
        } else {
            Form::Text
        }
    }

    fn value_type(self) -> PropertyValueType {
        match self {
            Form::Weighted => PropertyValueType::KeyValueList,
            Form::List => PropertyValueType::StringList,
            Form::Text => PropertyValueType::String,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Form::Weighted => "a list of weighted strings",
            Form::List => "a list of strings",
            Form::Text => "a single value",
        }
    }
}

/// Each entry of the response's section, as the name the response gives it
/// and its value.
type Entries = Vec<(String, AspectPropertyValue<TranslationValue>)>;

/// The metadata derived from the request engine's accessible properties,
/// kept together so the pieces are derived and cached as one.
struct DerivedMetadata {
    core: Vec<PropertyMetaData>,
    aspect: Vec<AspectPropertyMetaData>,
    /// Each property's lowercase name and the form of its value.
    forms: HashMap<String, Form>,
}

/// Reads the country names the cloud service translated, and the lists of
/// every country, from the cloud response, as
/// [`CountriesTranslationEngine`](crate::CountriesTranslationEngine) gives
/// them on premise.
///
/// Add it after a [`CloudRequestEngine`] and read the results under
/// [`COUNTRY_NAMES_TRANSLATED_KEY`] through
/// [`CountriesTranslationData`](crate::CountriesTranslationData), as for the
/// on-premise engine, so one can be swapped for the other.
///
/// # Evidence
///
/// The engine reads no evidence. The cloud request engine sends the
/// language evidence and the cloud service translates.
///
/// # Metadata
///
/// The properties are those the request engine's accessible properties list
/// for the product `countrynamestranslated`. A `Weighted` type gives a list
/// of weighted strings, `Array` a list of strings, and any other type a
/// single value held as text. Until the request engine's discovery has
/// completed, the engine lists the six properties the on-premise engine
/// gives, as the IP Intelligence cloud engine lists its defaults, and
/// [`FlowElement::properties_loaded`] is `false`. A resource key without the
/// product gives empty metadata and logs an error once, as the IP
/// Intelligence cloud engine does.
///
/// # Processing
///
/// Each entry of the response's `countrynamestranslated` section becomes a
/// property under the name the response gives, in the response's order,
/// leaving out each `<name>nullreason` entry. A null value has no value,
/// with the reason in the `<name>nullreason` entry or `Unknown`. An entry
/// the metadata does not list, such as `SortingCultureUsed`, is kept as
/// text or a list and logs a warning. With no metadata at all, as with a
/// license key alone, each value's form is read from the JSON with nothing
/// logged.
///
/// A response without the section, or a value not in the form its type
/// says, is an error, leaving the data empty. When the cloud request engine
/// left no response, the engine records an error that does not fail
/// processing and marks the data, so each property read gives
/// [`MissingPropertyReason::CloudRequestFailed`](fiftyone_pipeline_engines::MissingPropertyReason::CloudRequestFailed).
/// With a resource key, a property the response did not return gives the
/// reason it is missing.
///
/// # Example
///
/// ```no_run
/// use std::sync::Arc;
/// use fiftyone_cloud_request_engine::CloudRequestEngine;
/// use fiftyone_ip_intelligence_translation::{
///     CloudCountriesTranslationEngine, CountriesTranslationData,
///     COUNTRY_NAMES_TRANSLATED_KEY,
/// };
/// use fiftyone_pipeline_core::{Evidence, Pipeline};
///
/// let request_engine = Arc::new(
///     CloudRequestEngine::builder()
///         .resource_key("my-resource-key")
///         .build()?,
/// );
/// let countries = CloudCountriesTranslationEngine::builder()
///     .cloud_request_engine(Arc::clone(&request_engine))
///     .build()?;
/// let pipeline = Pipeline::builder()
///     .add_element(request_engine)
///     .add_element(Arc::new(countries))
///     .build()?;
///
/// let mut data = pipeline.create_flow_data_with(
///     Evidence::builder()
///         .add("query.client-ip-51d", "185.28.167.78")
///         .add("header.accept-language", "fr-FR")
///         .build(),
/// );
/// data.process()?;
/// if let Some(translated) = data.get(COUNTRY_NAMES_TRANSLATED_KEY) {
///     if let Ok(names) = translated.country_names_geographical_translated().value() {
///         for name in names {
///             println!("{} ({})", name.value, name.weighting());
///         }
///     }
/// }
/// # Ok::<(), fiftyone_pipeline_core::Error>(())
/// ```
pub struct CloudCountriesTranslationEngine {
    request_engine: Arc<CloudRequestEngine>,
    /// Empty, as the engine reads no evidence.
    evidence_filter: EvidenceKeyFilterWhitelist,
    properties: OnceCell<Vec<PropertyMetaData>>,
    aspect_properties: OnceCell<Vec<AspectPropertyMetaData>>,
    /// The six properties the on-premise engine gives, listed until the
    /// request engine's discovery has completed.
    default_properties: Vec<PropertyMetaData>,
    /// The aspect metadata of the default properties.
    default_aspect_properties: Vec<AspectPropertyMetaData>,
    /// Why a property is missing from data of a successful request. `None`
    /// with a license key alone, because the properties it grants are not
    /// listed.
    missing: OnceCell<Option<Arc<MissingPropertySource>>>,
    forms: OnceCell<HashMap<String, Form>>,
}

impl CloudCountriesTranslationEngine {
    /// A builder for the engine.
    pub fn builder() -> CloudCountriesTranslationEngineBuilder {
        CloudCountriesTranslationEngineBuilder::new()
    }

    /// The request engine whose response and accessible properties this
    /// engine reads.
    pub fn cloud_request_engine(&self) -> &Arc<CloudRequestEngine> {
        &self.request_engine
    }

    /// Derive the metadata from the request engine's accessible properties,
    /// taking the product keyed by `countrynamestranslated`. Each property
    /// keeps the category the list gives it. Returns `None` when there is no
    /// such product or it lists no properties.
    fn derive_metadata(products: &LicensedProducts) -> Option<DerivedMetadata> {
        let product = products.products.get(COUNTRY_NAMES_TRANSLATED_KEY_NAME)?;
        if product.properties.is_empty() {
            return None;
        }
        let mut derived = DerivedMetadata {
            core: Vec::with_capacity(product.properties.len()),
            aspect: Vec::with_capacity(product.properties.len()),
            forms: HashMap::with_capacity(product.properties.len()),
        };
        for property in &product.properties {
            let form = Form::from_cloud_type(&property.value_type);
            let mut core = PropertyMetaData::new(
                &property.name,
                COUNTRY_NAMES_TRANSLATED_KEY_NAME,
                form.value_type(),
            );
            if let Some(category) = &property.category {
                core = core.with_category(category.clone());
            }
            let mut aspect = AspectPropertyMetaData::from_core(core.clone());
            if let Some(tier) = &product.data_tier {
                aspect = aspect.with_data_tiers([tier.clone()]);
            }
            derived.core.push(core);
            derived.aspect.push(aspect);
            derived.forms.insert(property.name.to_lowercase(), form);
        }
        Some(derived)
    }

    /// Derive the metadata from the request engine's accessible properties
    /// if that is not done yet. Makes no cloud request, and leaves the
    /// metadata unset while discovery has not completed. The properties are
    /// set last, because a reader that finds them set reads the rest.
    fn ensure_metadata(&self) {
        if self.properties.get().is_some() {
            return;
        }
        let Ok(products) = self.request_engine.public_properties() else {
            return;
        };
        match Self::derive_metadata(products) {
            Some(derived) => {
                let _ = self
                    .missing
                    .set(self.missing_property_source(&derived.aspect));
                let _ = self.forms.set(derived.forms);
                let _ = self.aspect_properties.set(derived.aspect);
                let _ = self.properties.set(derived.core);
            }
            None => {
                let _ = self.missing.set(self.missing_property_source(&[]));
                let _ = self.forms.set(HashMap::new());
                let _ = self.aspect_properties.set(Vec::new());
                if self.properties.set(Vec::new()).is_ok()
                    && self.request_engine.resource_key().is_some()
                {
                    log::error!(
                        "Aspect properties could not be loaded for {ENGINE_NAME} - \
                         '{COUNTRY_NAMES_TRANSLATED_KEY_NAME}'"
                    );
                }
            }
        }
    }

    fn missing_property_source(
        &self,
        properties: &[AspectPropertyMetaData],
    ) -> Option<Arc<MissingPropertySource>> {
        self.request_engine.resource_key().is_some().then(|| {
            Arc::new(MissingPropertySource::new(
                COUNTRY_NAMES_TRANSLATED_KEY_NAME,
                ENGINE_NAME,
                EngineDeployment::Cloud,
                self.data_source_tier(),
                properties.to_vec(),
            ))
        })
    }

    /// Each entry of the response's section, as the name the response
    /// gives it and its value, in the response's order.
    fn read_response(&self, json: &str) -> Result<Entries> {
        let root: Value = serde_json::from_str(json).map_err(|error| Error::Pipeline {
            message: format!("The cloud response could not be read as JSON. {error}"),
        })?;
        let section = match root.get(COUNTRY_NAMES_TRANSLATED_KEY_NAME) {
            Some(Value::Object(section)) => section,
            Some(_) => {
                return Err(Error::Pipeline {
                    message: format!(
                        "The '{COUNTRY_NAMES_TRANSLATED_KEY_NAME}' member of the cloud \
                         response is not a JSON object."
                    ),
                })
            }
            // .NET's message, as it reads the member with a dictionary's
            // indexer.
            None => {
                return Err(Error::Pipeline {
                    message: format!(
                        "The given key '{COUNTRY_NAMES_TRANSLATED_KEY_NAME}' was not present \
                         in the dictionary."
                    ),
                })
            }
        };

        let empty = HashMap::new();
        let forms = self.forms.get().unwrap_or(&empty);
        let mut entries = Vec::with_capacity(section.len());
        for (name, value) in section {
            if is_null_reason(name) {
                continue;
            }
            let form = forms.get(&name.to_lowercase()).copied();
            if form.is_none() && !forms.is_empty() {
                log::warn!("No meta-data entry for property '{name}' in '{ENGINE_NAME}'");
            }
            let value = if value.is_null() {
                AspectPropertyValue::no_value(null_reason(section, name))
            } else {
                match form {
                    Some(form) => AspectPropertyValue::new(
                        read_as(value, form).ok_or_else(|| not_the_form(name, form))?,
                    ),
                    None => match read_unlisted(value) {
                        Some(value) => AspectPropertyValue::new(value),
                        None => {
                            log::debug!(
                                "'{name}' in '{COUNTRY_NAMES_TRANSLATED_KEY_NAME}' is left \
                                 out, as it is neither text nor a list."
                            );
                            continue;
                        }
                    },
                }
            };
            entries.push((name.clone(), value));
        }
        Ok(entries)
    }
}

/// Whether `name` ends `nullreason`, ignoring case.
fn is_null_reason(name: &str) -> bool {
    name.get(name.len().saturating_sub(NULL_REASON_SUFFIX.len())..)
        .is_some_and(|end| end.eq_ignore_ascii_case(NULL_REASON_SUFFIX))
}

/// The reason in the `<name>nullreason` entry, or `Unknown`.
fn null_reason(section: &Map<String, Value>, name: &str) -> String {
    match section.get(&format!("{name}{NULL_REASON_SUFFIX}")) {
        Some(Value::String(reason)) => reason.clone(),
        _ => UNKNOWN_REASON.to_owned(),
    }
}

fn not_the_form(name: &str, form: Form) -> Error {
    Error::Pipeline {
        message: format!(
            "The value of '{name}' in the '{COUNTRY_NAMES_TRANSLATED_KEY_NAME}' member of the \
             cloud response is not {}.",
            form.description()
        ),
    }
}

/// `value` in `form`, or `None` when it is not in that form.
fn read_as(value: &Value, form: Form) -> Option<TranslationValue> {
    match form {
        Form::Weighted => {
            let items = value.as_array()?;
            items
                .iter()
                .map(weighted_item)
                .collect::<Option<Vec<_>>>()
                .map(TranslationValue::WeightedList)
        }
        Form::List => {
            let items = value.as_array()?;
            items
                .iter()
                .map(|item| text(item).map(Some))
                .collect::<Option<Vec<_>>>()
                .map(TranslationValue::StringList)
        }
        Form::Text => text(value).map(|text| TranslationValue::String(Some(text))),
    }
}

/// A value the metadata does not list, in the form its JSON has, or `None`
/// for an object or a list of mixed items.
fn read_unlisted(value: &Value) -> Option<TranslationValue> {
    match value {
        Value::Array(items) if !items.is_empty() && items.iter().all(Value::is_object) => {
            read_as(value, Form::Weighted)
        }
        Value::Array(_) => read_as(value, Form::List),
        _ => read_as(value, Form::Text),
    }
}

/// A weighted item, being an object with a weighting from 0 to 65535 and a
/// string value, its fields matched ignoring case.
fn weighted_item(item: &Value) -> Option<WeightedValue<Option<String>>> {
    let fields = item.as_object()?;
    let field = |wanted: &str| {
        fields
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
            .map(|(_, value)| value)
    };
    // A weighting with a fraction rounds half to even, as .NET converts it.
    let weighting = field(RAW_WEIGHTING_FIELD)?.as_f64()?.round_ties_even();
    if !(0.0..=f64::from(u16::MAX)).contains(&weighting) {
        return None;
    }
    let value = field(VALUE_FIELD)?.as_str()?;
    Some(WeightedValue::new(weighting as u16, Some(value.to_owned())))
}

/// A string, number or boolean as text, a boolean written as .NET writes
/// it, or `None` for anything else.
fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(true) => Some("True".to_owned()),
        Value::Bool(false) => Some("False".to_owned()),
        _ => None,
    }
}

impl FlowElement for CloudCountriesTranslationEngine {
    /// Stores the translated country names the cloud returned. The data is
    /// added before the response is read, so a response that cannot be
    /// read leaves it empty, as .NET does.
    fn process(&self, data: &mut FlowData) -> Result<()> {
        self.ensure_metadata();
        let entries = {
            let cloud = data.get(CloudRequestEngine::DATA_KEY).ok_or_else(|| {
                Error::configuration(format!(
                    "No cloud request data is present. This is probably because there is no \
                     'CloudRequestEngine' before the '{ENGINE_NAME}' in the pipeline. This \
                     engine cannot produce results until that is corrected."
                ))
            })?;
            match cloud.json_response() {
                Some(json) if !json.is_empty() => Some(self.read_response(json)),
                _ => None,
            }
        };
        let Some(entries) = entries else {
            CloudRequestEngine::record_not_processed(
                data,
                ENGINE_NAME,
                COUNTRY_NAMES_TRANSLATED_KEY_NAME,
                module_path!(),
            );
            data.get_or_add(COUNTRY_NAMES_TRANSLATED_KEY, TranslationData::new)?
                .mark_cloud_request_failed(COUNTRY_NAMES_TRANSLATED_KEY_NAME);
            return Ok(());
        };
        let output = data.get_or_add(COUNTRY_NAMES_TRANSLATED_KEY, TranslationData::new)?;
        if let Some(Some(missing)) = self.missing.get() {
            output.add_missing_property_source(
                COUNTRY_NAMES_TRANSLATED_KEY_NAME,
                Arc::clone(missing),
            );
        }
        for (name, value) in entries? {
            output.set(name, value);
        }
        Ok(())
    }

    fn data_key(&self) -> &str {
        COUNTRY_NAMES_TRANSLATED_KEY_NAME
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.evidence_filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        match self.properties.get() {
            Some(properties) => properties,
            None => &self.default_properties,
        }
    }

    /// Reads the cloud's property list first if discovery has completed
    /// since the engine last looked, so the answer is current.
    fn properties_loaded(&self) -> bool {
        self.ensure_metadata();
        self.has_loaded_properties()
    }
}

impl AspectEngine for CloudCountriesTranslationEngine {
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
        match self.aspect_properties.get() {
            Some(properties) => properties,
            None => &self.default_aspect_properties,
        }
    }

    fn has_loaded_properties(&self) -> bool {
        self.properties.get().is_some()
    }

    /// The request engine asks for the property list when it is built, or
    /// is given it, so a list it still lacks is one whose request failed.
    fn property_list_request_failed(&self) -> bool {
        !self.has_loaded_properties() && self.request_engine.public_properties().is_err()
    }

    fn missing_property_reason(&self, property_name: &str) -> MissingPropertyResult {
        self.ensure_metadata();
        default_missing_property_reason(self, property_name)
    }
}

/// Builds a [`CloudCountriesTranslationEngine`].
#[derive(Default)]
pub struct CloudCountriesTranslationEngineBuilder {
    request_engine: Option<Arc<CloudRequestEngine>>,
}

impl CloudCountriesTranslationEngineBuilder {
    /// A builder with no request engine set.
    pub fn new() -> Self {
        CloudCountriesTranslationEngineBuilder::default()
    }

    /// The [`CloudRequestEngine`] this engine reads from, which must be
    /// added to the pipeline before this engine. Required.
    pub fn cloud_request_engine(mut self, engine: Arc<CloudRequestEngine>) -> Self {
        self.request_engine = Some(engine);
        self
    }

    /// Build the engine, or give
    /// [`Error::PipelineConfiguration`](fiftyone_pipeline_core::Error::PipelineConfiguration)
    /// when no request engine was set.
    ///
    /// The metadata is read from the request engine here, which makes no
    /// cloud request. When the request engine's discovery has not
    /// completed, the metadata is read the first time the engine needs it
    /// after discovery completes.
    pub fn build(self) -> Result<CloudCountriesTranslationEngine> {
        let request_engine = self.request_engine.ok_or_else(|| {
            Error::configuration(format!(
                "A {ENGINE_NAME} needs a CloudRequestEngine. Set one with \
                 .cloud_request_engine(..)."
            ))
        })?;
        let default_properties = countries_translated_properties();
        let default_aspect_properties = default_properties
            .iter()
            .cloned()
            .map(AspectPropertyMetaData::from_core)
            .collect();
        let engine = CloudCountriesTranslationEngine {
            request_engine,
            evidence_filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: OnceCell::new(),
            aspect_properties: OnceCell::new(),
            default_properties,
            default_aspect_properties,
            missing: OnceCell::new(),
            forms: OnceCell::new(),
        };
        engine.ensure_metadata();
        Ok(engine)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_ending_nullreason_in_any_case_is_a_reason() {
        assert!(is_null_reason("CountryCodesGeographicalAllnullreason"));
        assert!(is_null_reason("CountryCodesGeographicalAllNullReason"));
        assert!(!is_null_reason("nullreasons"));
        assert!(!is_null_reason("reason"));
        // A name whose last ten bytes start inside a character is not one.
        assert!(!is_null_reason("é123456789"));
    }

    #[test]
    fn the_cloud_type_decides_the_form() {
        assert_eq!(Form::from_cloud_type("WeightedString"), Form::Weighted);
        assert_eq!(Form::from_cloud_type("weightedstring"), Form::Weighted);
        assert_eq!(Form::from_cloud_type("Array"), Form::List);
        assert_eq!(Form::from_cloud_type("ARRAY"), Form::List);
        assert_eq!(Form::from_cloud_type("String"), Form::Text);
        assert_eq!(Form::from_cloud_type("Int32"), Form::Text);
    }

    #[test]
    fn a_weighting_must_be_a_number_from_0_to_65535() {
        let item = |weighting: Value| {
            weighted_item(&serde_json::json!({ "RawWeighting": weighting, "Value": "France" }))
                .map(|item| item.raw_weighting)
        };
        assert_eq!(item(serde_json::json!(0)), Some(0));
        assert_eq!(item(serde_json::json!(65535)), Some(65535));
        assert_eq!(item(serde_json::json!(2.5)), Some(2));
        assert_eq!(item(serde_json::json!(3.5)), Some(4));
        assert_eq!(item(serde_json::json!(65536)), None);
        assert_eq!(item(serde_json::json!(-1)), None);
        assert_eq!(item(serde_json::json!("1")), None);
        assert_eq!(
            weighted_item(&serde_json::json!({ "rawweighting": 1, "value": null })),
            None
        );
    }
}
