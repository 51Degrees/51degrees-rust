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

//! The property keyed engine and its three builders.

use std::any::Any;
use std::sync::{Arc, OnceLock};

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, PROFILE_ID_PROPERTY_NAME,
};
use fiftyone_device_detection_shared::{
    MultiDeviceData, HARDWARE_ELEMENT_DATA_KEY, PROFILES_PROPERTY_NAME,
};
use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData, PropertyValueType, Result, TypedKey,
};
use fiftyone_pipeline_engines::{AspectEngine, AspectPropertyMetaData};

use crate::constants::{
    DEFAULT_DATA_KEY_PREFIX, EVIDENCE_KEY_PREFIX, NATIVE_MODEL_PROPERTY_NAME, TAC_PROPERTY_NAME,
};
use crate::number::parse_int;

/// The number of UTF-16 code units a valid TAC has, as .NET counts a
/// string's length.
const TAC_LENGTH: usize = 8;

/// The fewest UTF-16 code units a valid native model name has.
const NATIVE_MODEL_MIN_LENGTH: usize = 2;

/// .NET's message for a generic builder given no key property.
const NO_KEY_PROPERTY: &str = "KeyProperty must be set before building";

/// .NET's message for an engine given an empty key property, with the
/// parameter name its `ArgumentException` adds.
const EMPTY_KEY_PROPERTY: &str =
    "keyProperty is required and cannot be null or empty. (Parameter 'keyProperty')";

/// .NET's message for an engine given an empty data key, with the parameter
/// name its `ArgumentException` adds.
const EMPTY_DATA_KEY: &str =
    "elementDataKey is required and cannot be null or empty. (Parameter 'elementDataKey')";

/// Which values of the key property an engine looks up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Validation {
    /// Exactly eight UTF-16 code units that .NET's `int.TryParse` reads as
    /// a number, as .NET's `TacEngine` checks.
    Tac,
    /// At least two UTF-16 code units, as .NET's `NativeEngine` checks.
    NativeModel,
    /// Every value.
    Any,
}

impl Validation {
    /// The warning for a value the rule refuses, or [`None`] when the value
    /// is looked up. The texts are .NET's.
    fn check(self, value: &str) -> Option<String> {
        let length = value.encode_utf16().count();
        match self {
            Validation::Tac if length != TAC_LENGTH || parse_int(value).is_none() => Some(format!(
                "The TAC value '{value}' is not valid. A TAC must be exactly 8 numeric digits."
            )),
            Validation::NativeModel if length < NATIVE_MODEL_MIN_LENGTH => Some(format!(
                "The NativeModel value '{value}' is not valid. A NativeModel must be at least \
                 two characters."
            )),
            _ => None,
        }
    }
}

/// Gives every hardware profile in the Device Detection data file that holds
/// the value of one property the request gives, as .NET's property keyed
/// engines do.
///
/// [`TacEngineBuilder`] builds the engine for `TAC`, the first eight digits
/// of a mobile device's IMEI, and [`NativeEngineBuilder`] the engine for
/// `NativeModel`, the model name a device's own operating system reports.
/// Both write a [`MultiDeviceData`] under
/// [`HARDWARE_DATA_KEY`](fiftyone_device_detection_shared::HARDWARE_DATA_KEY),
/// so when both are in one pipeline the second adds its profiles after the
/// first's. [`PropertyKeyedDeviceEngineBuilder`] builds one for any other
/// property.
///
/// Add the engine after a [`DeviceDetectionOnPremiseEngine`]. When the
/// pipeline is built the engine finds the key property in that engine's
/// data file, ignoring case, and then accepts the evidence `query.` and the
/// key property's name in lower case, such as `query.tac`. A data file
/// without the property, such as the free Lite file for `TAC` and
/// `NativeModel`, gives an engine that accepts no evidence and writes
/// nothing.
///
/// A request carrying the evidence always gets the data, holding no
/// profiles when the value matches none. A TAC that is not eight digits, or
/// a native model name shorter than two characters, gives a warning in the
/// flow data, never an error, and no profiles. The value is matched exactly,
/// including case.
///
/// The lookup is fast when the Device Detection engine indexes the key
/// property, which
/// [`indexed_property`](fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngineBuilder::indexed_property)
/// asks for. Without the index each request reads every profile, and the
/// engine logs a warning when it is added to the pipeline.
pub struct PropertyKeyedDeviceEngine {
    key_property: String,
    data_key: &'static str,
    validation: Validation,
    state: OnceLock<State>,
    /// The filter until the pipeline is known, which accepts nothing.
    no_evidence: EvidenceKeyFilterWhitelist,
}

/// What the engine reads from the Device Detection engine once it is added
/// to a pipeline.
struct State {
    /// The Device Detection engine whose data set the profiles are read from.
    source: Arc<DeviceDetectionOnPremiseEngine>,
    /// How the key property is looked up, or [`None`] when the data file has
    /// no such property.
    lookup: Option<Lookup>,
    tier: String,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    aspect_properties: Vec<AspectPropertyMetaData>,
}

/// The key property as the data file names it, and the evidence key that
/// gives its value.
struct Lookup {
    /// The name as the data file gives it, which the lookup matches exactly.
    property_name: String,
    /// `query.` and the name in lower case.
    evidence_key: String,
}

impl PropertyKeyedDeviceEngine {
    /// The property the engine looks profiles up by, as its builder was
    /// given it.
    pub fn key_property(&self) -> &str {
        &self.key_property
    }

    /// The evidence key the engine accepts, or [`None`] until the engine is
    /// added to a pipeline and when the data file has no key property.
    pub fn evidence_key(&self) -> Option<&str> {
        self.state
            .get()
            .and_then(|state| state.lookup.as_ref())
            .map(|lookup| lookup.evidence_key.as_str())
    }
}

impl State {
    fn new(
        source: &Arc<DeviceDetectionOnPremiseEngine>,
        key_property: &str,
        data_key: &str,
    ) -> Self {
        let tier = AspectEngine::data_source_tier(source.as_ref()).to_owned();
        let source_properties = source.fiftyone_properties();
        let key = source_properties
            .iter()
            .find(|property| property.name().eq_ignore_ascii_case(key_property));
        let lookup = match key {
            Some(key) => {
                let property_name = key.name().to_owned();
                if !source.indexed_properties().contains(&property_name) {
                    log::warn!(
                        "Property '{property_name}' is not indexed by the Device Detection \
                         engine, so each lookup reads every profile. Name it to \
                         DeviceDetectionOnPremiseEngineBuilder::indexed_property."
                    );
                }
                Some(Lookup {
                    evidence_key: format!("{EVIDENCE_KEY_PREFIX}{}", property_name.to_lowercase()),
                    property_name,
                })
            }
            None => {
                log::warn!(
                    "Property '{key_property}' not found in '{}'. It will not be indexed.",
                    source.data_key()
                );
                None
            }
        };
        // Each profile lists its id, then the properties of the key
        // property's component that are not JavaScript, in the Device
        // Detection engine's order.
        let component = key.and_then(|key| key.component_id());
        let mut items = vec![PropertyMetaData::new(
            PROFILE_ID_PROPERTY_NAME,
            data_key,
            PropertyValueType::String,
        )];
        if let Some(component) = component {
            items.extend(
                source_properties
                    .iter()
                    .filter(|property| property.component_id() == Some(component))
                    .filter(|property| property.core().value_type != PropertyValueType::JavaScript)
                    .map(|property| {
                        let mut item = property.core().clone();
                        item.element_data_key = data_key.to_owned();
                        item
                    }),
            );
        }
        let profiles = AspectPropertyMetaData::new(
            PROFILES_PROPERTY_NAME,
            data_key,
            PropertyValueType::KeyValueList,
        )
        .map_core(|core| core.with_item_properties(items));
        let filter = EvidenceKeyFilterWhitelist::new(
            lookup.iter().map(|lookup| lookup.evidence_key.as_str()),
        );
        State {
            source: Arc::clone(source),
            lookup,
            tier,
            filter,
            properties: vec![profiles.core().clone()],
            aspect_properties: vec![profiles],
        }
    }
}

/// The [`DeviceDetectionOnPremiseEngine`] among the pipeline's elements.
fn device_detection_engine(pipeline: &Pipeline) -> Result<Arc<DeviceDetectionOnPremiseEngine>> {
    pipeline
        .flow_elements()
        .iter()
        .find_map(|element| {
            Arc::clone(element)
                .into_any()?
                .downcast::<DeviceDetectionOnPremiseEngine>()
                .ok()
        })
        .ok_or_else(|| {
            Error::configuration(
                "Ensure DeviceDetectionOnPremiseEngine is added to the pipeline before the \
                 property keyed engine.",
            )
        })
}

impl FlowElement for PropertyKeyedDeviceEngine {
    /// Add the profiles holding the request's value of the key property, if
    /// the request gives one.
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let Some(state) = self.state.get() else {
            return Err(Error::configuration(
                "The property keyed engine must be added to a pipeline before processing data.",
            ));
        };
        let Some(lookup) = &state.lookup else {
            return Ok(());
        };
        let Some(value) = data.evidence().get(&lookup.evidence_key).map(str::to_owned) else {
            return Ok(());
        };
        let warning = self.validation.check(&value);
        let profiles = match warning {
            Some(_) => Vec::new(),
            None => state
                .source
                .profiles_for_property_value(&lookup.property_name, &value)?,
        };
        let hardware = data.get_or_add(TypedKey::new(self.data_key), MultiDeviceData::new)?;
        for profile in profiles {
            hardware.push_profile(profile);
        }
        if let Some(message) = warning {
            data.add_warning(message, Some(self as &dyn FlowElement));
        }
        Ok(())
    }

    fn data_key(&self) -> &str {
        self.data_key
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        match self.state.get() {
            Some(state) => &state.filter,
            None => &self.no_evidence,
        }
    }

    fn properties(&self) -> &[PropertyMetaData] {
        match self.state.get() {
            Some(state) => &state.properties,
            None => &[],
        }
    }

    fn properties_loaded(&self) -> bool {
        self.state.get().is_some()
    }

    /// Find the key property in the pipeline's
    /// [`DeviceDetectionOnPremiseEngine`]. A pipeline without one is refused.
    ///
    /// The engine keeps what it read from the first pipeline, so it can be
    /// added to another pipeline only when that pipeline has the same Device
    /// Detection engine.
    fn add_pipeline(&self, pipeline: &Arc<Pipeline>) -> Result<()> {
        let source = device_detection_engine(pipeline)?;
        if self.state.get().is_none() {
            let state = State::new(&source, &self.key_property, self.data_key);
            // Another pipeline may have set it first, which the check below
            // compares with.
            let _ = self.state.set(state);
        }
        let same = self
            .state
            .get()
            .is_some_and(|state| Arc::ptr_eq(&state.source, &source));
        if same {
            Ok(())
        } else {
            Err(Error::configuration(
                "The property keyed engine is already in a pipeline with another Device \
                 Detection engine. Make another property keyed engine for this pipeline.",
            ))
        }
    }

    fn into_any(self: Arc<Self>) -> Option<Arc<dyn Any + Send + Sync>> {
        Some(self)
    }
}

impl AspectEngine for PropertyKeyedDeviceEngine {
    fn data_source_tier(&self) -> &str {
        self.state.get().map_or("", |state| state.tier.as_str())
    }

    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        match self.state.get() {
            Some(state) => &state.aspect_properties,
            None => &[],
        }
    }

    fn has_loaded_properties(&self) -> bool {
        self.state.get().is_some()
    }

    fn has_typed_property(&self, property_name: &str) -> bool {
        property_name.eq_ignore_ascii_case(PROFILES_PROPERTY_NAME)
    }
}

/// Make the engine, refusing an empty key property or data key as .NET's
/// engine does.
fn build_engine(
    key_property: String,
    data_key: &'static str,
    validation: Validation,
) -> Result<Arc<PropertyKeyedDeviceEngine>> {
    if key_property.is_empty() {
        return Err(Error::configuration(EMPTY_KEY_PROPERTY));
    }
    if data_key.is_empty() {
        return Err(Error::configuration(EMPTY_DATA_KEY));
    }
    Ok(Arc::new(PropertyKeyedDeviceEngine {
        key_property,
        data_key,
        validation,
        state: OnceLock::new(),
        no_evidence: EvidenceKeyFilterWhitelist::default(),
    }))
}

/// Builds the engine that looks hardware profiles up by `TAC`, as .NET's
/// `TacEngineBuilder` does. It writes under
/// [`HARDWARE_DATA_KEY`](fiftyone_device_detection_shared::HARDWARE_DATA_KEY)
/// and gives a warning for a TAC that is not exactly eight digits.
#[derive(Debug)]
pub struct TacEngineBuilder {
    key_property: String,
    data_key: &'static str,
}

impl Default for TacEngineBuilder {
    fn default() -> Self {
        TacEngineBuilder::new()
    }
}

impl TacEngineBuilder {
    /// A builder for `TAC` under the data key `hardware`.
    pub fn new() -> Self {
        TacEngineBuilder {
            key_property: TAC_PROPERTY_NAME.to_owned(),
            data_key: HARDWARE_ELEMENT_DATA_KEY,
        }
    }

    /// Look profiles up by another property, still checking each value as a
    /// TAC, as .NET's builder allows.
    pub fn set_key_property(mut self, key_property: impl Into<String>) -> Self {
        self.key_property = key_property.into();
        self
    }

    /// Write under another data key. Engines that share a key add to the
    /// same data.
    pub fn set_element_data_key(mut self, data_key: &'static str) -> Self {
        self.data_key = data_key;
        self
    }

    /// Build the engine. It finds the key property when it is added to a
    /// pipeline. Fails when the key property or data key is empty.
    pub fn build(self) -> Result<Arc<PropertyKeyedDeviceEngine>> {
        build_engine(self.key_property, self.data_key, Validation::Tac)
    }
}

/// Builds the engine that looks hardware profiles up by `NativeModel`, as
/// .NET's `NativeEngineBuilder` does. It writes under
/// [`HARDWARE_DATA_KEY`](fiftyone_device_detection_shared::HARDWARE_DATA_KEY)
/// and gives a warning for a native model name shorter than two characters.
#[derive(Debug)]
pub struct NativeEngineBuilder {
    key_property: String,
    data_key: &'static str,
}

impl Default for NativeEngineBuilder {
    fn default() -> Self {
        NativeEngineBuilder::new()
    }
}

impl NativeEngineBuilder {
    /// A builder for `NativeModel` under the data key `hardware`.
    pub fn new() -> Self {
        NativeEngineBuilder {
            key_property: NATIVE_MODEL_PROPERTY_NAME.to_owned(),
            data_key: HARDWARE_ELEMENT_DATA_KEY,
        }
    }

    /// Look profiles up by another property, still checking each value as a
    /// native model name, as .NET's builder allows.
    pub fn set_key_property(mut self, key_property: impl Into<String>) -> Self {
        self.key_property = key_property.into();
        self
    }

    /// Write under another data key. Engines that share a key add to the
    /// same data.
    pub fn set_element_data_key(mut self, data_key: &'static str) -> Self {
        self.data_key = data_key;
        self
    }

    /// Build the engine. It finds the key property when it is added to a
    /// pipeline. Fails when the key property or data key is empty.
    pub fn build(self) -> Result<Arc<PropertyKeyedDeviceEngine>> {
        build_engine(self.key_property, self.data_key, Validation::NativeModel)
    }
}

/// Builds an engine that looks hardware profiles up by any property and
/// looks up every value, as .NET's `PropertyKeyedDeviceEngineBuilder` does.
#[derive(Debug, Default)]
pub struct PropertyKeyedDeviceEngineBuilder {
    key_property: Option<String>,
    data_key: Option<&'static str>,
}

impl PropertyKeyedDeviceEngineBuilder {
    /// A builder with no key property, which must be set before building.
    pub fn new() -> Self {
        PropertyKeyedDeviceEngineBuilder::default()
    }

    /// Look profiles up by the property named, such as `TAC`.
    pub fn set_key_property(mut self, key_property: impl Into<String>) -> Self {
        self.key_property = Some(key_property.into());
        self
    }

    /// Write under this data key. The default is
    /// [`DEFAULT_DATA_KEY_PREFIX`] and the key property's name in lower
    /// case, such as `profiles-tac`.
    pub fn set_element_data_key(mut self, data_key: &'static str) -> Self {
        self.data_key = Some(data_key);
        self
    }

    /// Build the engine. It finds the key property when it is added to a
    /// pipeline. Fails when no key property is set, or the data key is
    /// empty.
    pub fn build(self) -> Result<Arc<PropertyKeyedDeviceEngine>> {
        let key_property = self
            .key_property
            .filter(|key_property| !key_property.is_empty())
            .ok_or_else(|| Error::configuration(NO_KEY_PROPERTY))?;
        // The pipeline's typed keys hold their name for the life of the
        // process, so the default name is kept for as long. An engine is
        // built once for a pipeline, so this is one short string each time.
        let data_key: &'static str = match self.data_key {
            Some(data_key) => data_key,
            None => Box::leak(
                format!("{DEFAULT_DATA_KEY_PREFIX}{}", key_property.to_lowercase())
                    .into_boxed_str(),
            ),
        };
        build_engine(key_property, data_key, Validation::Any)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tac_is_eight_characters_dotnet_reads_as_a_number() {
        for (value, valid) in [
            ("12345678", true),
            (" 1234567", true),
            ("+1234567", true),
            ("-1234567", true),
            ("1234567 ", true),
            ("1234567\0", true),
            ("00000000", true),
            ("1234567", false),
            ("123456789", false),
            ("abcdefgh", false),
            ("1234 678", false),
            ("", false),
        ] {
            assert_eq!(Validation::Tac.check(value).is_none(), valid, "{value:?}");
        }
    }

    #[test]
    fn a_native_model_is_two_utf16_code_units_or_more() {
        for (value, valid) in [
            ("", false),
            ("a", false),
            ("\u{e9}", false),
            ("\u{1f600}", true),
            ("ab", true),
            ("SM-G960F", true),
        ] {
            assert_eq!(
                Validation::NativeModel.check(value).is_none(),
                valid,
                "{value:?}"
            );
        }
    }

    #[test]
    fn the_warnings_are_dotnets_text() {
        assert_eq!(
            Validation::Tac.check("1234567").as_deref(),
            Some("The TAC value '1234567' is not valid. A TAC must be exactly 8 numeric digits.")
        );
        assert_eq!(
            Validation::Tac.check("").as_deref(),
            Some("The TAC value '' is not valid. A TAC must be exactly 8 numeric digits.")
        );
        assert_eq!(
            Validation::NativeModel.check("a").as_deref(),
            Some(
                "The NativeModel value 'a' is not valid. A NativeModel must be at least two \
                 characters."
            )
        );
    }

    #[test]
    fn the_generic_engine_looks_up_every_value() {
        for value in ["", "a", "1234567", "abcdefgh"] {
            assert_eq!(Validation::Any.check(value), None, "{value:?}");
        }
    }

    #[test]
    fn the_tac_and_native_builders_write_under_hardware() {
        let tac = TacEngineBuilder::new().build().unwrap();
        assert_eq!(tac.key_property(), "TAC");
        assert_eq!(tac.data_key(), "hardware");
        assert_eq!(tac.validation, Validation::Tac);
        let native = NativeEngineBuilder::new().build().unwrap();
        assert_eq!(native.key_property(), "NativeModel");
        assert_eq!(native.data_key(), "hardware");
        assert_eq!(native.validation, Validation::NativeModel);
    }

    #[test]
    fn the_generic_builder_needs_a_key_and_names_its_data_after_it() {
        assert!(PropertyKeyedDeviceEngineBuilder::new().build().is_err());
        assert!(PropertyKeyedDeviceEngineBuilder::new()
            .set_key_property("")
            .build()
            .is_err());
        let engine = PropertyKeyedDeviceEngineBuilder::new()
            .set_key_property("NativeModel")
            .build()
            .unwrap();
        assert_eq!(engine.data_key(), "profiles-nativemodel");
        assert_eq!(engine.validation, Validation::Any);
        let engine = PropertyKeyedDeviceEngineBuilder::new()
            .set_key_property("TAC")
            .set_element_data_key("devices")
            .build()
            .unwrap();
        assert_eq!(engine.data_key(), "devices");
    }

    #[test]
    fn an_empty_key_property_or_data_key_is_refused() {
        assert!(TacEngineBuilder::new()
            .set_key_property("")
            .build()
            .is_err());
        assert!(NativeEngineBuilder::new()
            .set_element_data_key("")
            .build()
            .is_err());
        assert!(PropertyKeyedDeviceEngineBuilder::new()
            .set_key_property("TAC")
            .set_element_data_key("")
            .build()
            .is_err());
    }

    #[test]
    fn an_engine_outside_a_pipeline_accepts_nothing_and_lists_nothing() {
        let engine = TacEngineBuilder::new().build().unwrap();
        assert!(!engine.properties_loaded());
        assert!(engine.properties().is_empty());
        assert!(engine.aspect_properties().is_empty());
        assert!(!engine.evidence_key_filter().include("query.tac"));
        assert_eq!(engine.evidence_key(), None);
        assert_eq!(engine.data_source_tier(), "");
        assert!(engine.has_typed_property("profiles"));
    }
}
