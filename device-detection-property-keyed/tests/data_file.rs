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

//! The TAC and Native engines after the on-premise Device Detection engine.
//!
//! The tests run on the Hash data file `51DEGREES_DD_PATH` names, or the Lite
//! file in the `cxx` submodule, and skip when there is neither. The Lite file
//! has no `TAC` or `NativeModel` property, so over it the engines accept no
//! evidence and write nothing. The tests that find profiles need a file with
//! both properties, such as the TAC file, and skip over any other. When
//! `51DEGREES_REQUIRE_TAC` is set, as the comparison workflow sets it, they
//! fail rather than skip.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder, PROFILE_ID_PROPERTY_NAME,
};
use fiftyone_device_detection_property_keyed::constants::{
    NATIVE_MODEL_PROPERTY_NAME, TAC_PROPERTY_NAME,
};
use fiftyone_device_detection_property_keyed::{
    NativeEngineBuilder, PropertyKeyedDeviceEngine, TacEngineBuilder,
};
use fiftyone_device_detection_shared::{
    DeviceDataBase, MultiDeviceData, HARDWARE_DATA_KEY, HARDWARE_ELEMENT_DATA_KEY,
    PROFILES_PROPERTY_NAME,
};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{
    ElementData, Evidence, FlowData, FlowElement, Pipeline, PropertyValue,
};

const TAC_KEY: &str = "query.tac";
const NATIVE_MODEL_KEY: &str = "query.nativemodel";
const USER_AGENT_KEY: &str = "header.user-agent";
const USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
                          AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 \
                          Mobile/15E148 Safari/604.1";

/// A TAC and a native model name that pass the engines' checks.
const VALID_TAC: &str = "35332509";
const VALID_NATIVE_MODEL: &str = "SM-G960F";

/// Set where the tests that find profiles must run, so they fail rather than
/// skip.
const REQUIRE_TAC: &str = "51DEGREES_REQUIRE_TAC";

/// How many of a property's values, in the data file's order, are searched
/// for one that passes the engine's check and that a profile holds.
const VALUES_SEARCHED: usize = 1000;

/// The Hash data file `51DEGREES_DD_PATH` names, or the Lite file in the `cxx`
/// submodule.
fn data_file() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("51DEGREES_DD_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .to_path_buf();
    let roots = [workspace.clone(), workspace.parent()?.to_path_buf()];
    roots
        .into_iter()
        .map(|root| {
            root.join("cxx")
                .join("data")
                .join("device-detection")
                .join("51Degrees-LiteV4.1.hash")
        })
        .find(|path| path.is_file())
}

/// The Device Detection engine over the data file, indexing `TAC` and
/// `NativeModel`, or [`None`] when there is no data file. Loaded once and
/// shared by the tests, as the TAC file is large.
fn device_engine() -> Option<Arc<DeviceDetectionOnPremiseEngine>> {
    static DEVICE: OnceLock<Option<Arc<DeviceDetectionOnPremiseEngine>>> = OnceLock::new();
    DEVICE
        .get_or_init(|| {
            Some(
                DeviceDetectionOnPremiseEngineBuilder::new(data_file()?)
                    .performance_profile(PerformanceProfile::LowMemory)
                    .indexed_properties([TAC_PROPERTY_NAME, NATIVE_MODEL_PROPERTY_NAME])
                    .build()
                    .expect("the data file loads"),
            )
        })
        .clone()
}

fn tac() -> Arc<PropertyKeyedDeviceEngine> {
    TacEngineBuilder::new()
        .build()
        .expect("the TAC engine builds")
}

fn native() -> Arc<PropertyKeyedDeviceEngine> {
    NativeEngineBuilder::new()
        .build()
        .expect("the Native engine builds")
}

/// The Device Detection engine, then `engines` in order.
fn pipeline(
    device: &Arc<DeviceDetectionOnPremiseEngine>,
    engines: &[&Arc<PropertyKeyedDeviceEngine>],
) -> Arc<Pipeline> {
    engines
        .iter()
        .fold(
            Pipeline::builder().add_element(Arc::clone(device) as Arc<dyn FlowElement>),
            |builder, engine| builder.add_element(Arc::clone(engine) as Arc<dyn FlowElement>),
        )
        .build()
        .expect("the pipeline builds")
}

fn process(pipeline: &Arc<Pipeline>, evidence: &[(&str, &str)]) -> FlowData {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add_all(evidence.iter().copied())
            .build(),
    );
    data.process().expect("the request is processed");
    assert!(data.errors().is_empty(), "{evidence:?} gave errors");
    data
}

fn warnings(data: &FlowData) -> Vec<&str> {
    data.warnings()
        .iter()
        .map(|warning| warning.message.as_str())
        .collect()
}

/// Note why a test that finds profiles is skipped, or fail when
/// `51DEGREES_REQUIRE_TAC` is set.
fn skip_tac(reason: &str) {
    assert!(
        std::env::var_os(REQUIRE_TAC).is_none(),
        "{REQUIRE_TAC} is set, but {reason}"
    );
    eprintln!("{reason}; skipping");
}

/// The Device Detection engine over a data file whose `TAC` and
/// `NativeModel` the engines find, or [`None`] when there is none.
fn device_with_key_properties() -> Option<Arc<DeviceDetectionOnPremiseEngine>> {
    let Some(device) = device_engine() else {
        skip_tac("no Hash data file found");
        return None;
    };
    // The engines find their key properties when added to a pipeline.
    let (tac, native) = (tac(), native());
    pipeline(&device, &[&tac, &native]);
    if tac.evidence_key().is_none() || native.evidence_key().is_none() {
        skip_tac("the data file has no TAC or no NativeModel property");
        return None;
    }
    Some(device)
}

fn is_tac(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_native_model(value: &str) -> bool {
    value.encode_utf16().count() >= 2
}

/// The first value of `property` in the data file's order that `accept`
/// passes and that a profile holds, with the ids of the profiles holding it.
fn held_value(
    device: &DeviceDetectionOnPremiseEngine,
    property: &str,
    accept: fn(&str) -> bool,
) -> (String, Vec<u32>) {
    device
        .property_value_range(property, 0..VALUES_SEARCHED)
        .expect("the values read")
        .unwrap_or_else(|| panic!("the data file has no {property}"))
        .into_iter()
        .map(|value| value.name)
        .filter(|value| accept(value))
        .find_map(|value| {
            let ids = device
                .profile_ids_for_property_value(property, &value)
                .expect("the value is looked up");
            (!ids.is_empty()).then_some((value, ids))
        })
        .unwrap_or_else(|| {
            panic!("no profile holds any of the first {VALUES_SEARCHED} {property} values")
        })
}

/// The id each profile holds, in order.
fn profile_ids(hardware: &MultiDeviceData) -> Vec<u32> {
    hardware
        .profiles()
        .iter()
        .map(|profile| {
            profile
                .string_property(PROFILE_ID_PROPERTY_NAME)
                .value()
                .expect("each profile has its id")
                .parse()
                .expect("the id is a number")
        })
        .collect()
}

/// Assert each profile holds `value` of `property`.
fn assert_each_holds(profiles: &[DeviceDataBase], property: &str, value: &str) {
    for profile in profiles {
        let held = profile
            .string_list_property(property)
            .into_value()
            .unwrap_or_default();
        assert!(
            held.iter().any(|held| held == value),
            "a profile holds {property} {held:?}, not {value:?}"
        );
    }
}

/// Over a data file without `TAC` and `NativeModel`, such as the Lite file,
/// the engines accept no evidence and list a profile as its id alone, and a
/// request giving either value, valid or not, gets no data and no warning,
/// as from .NET's engines.
#[test]
fn over_a_file_without_the_key_properties_nothing_is_written() {
    let Some(device) = device_engine() else {
        eprintln!("no Hash data file found; skipping");
        return;
    };
    let (tac, native) = (tac(), native());
    let pipeline = pipeline(&device, &[&tac, &native]);
    if tac.evidence_key().is_some() || native.evidence_key().is_some() {
        eprintln!("the data file has TAC or NativeModel; skipping");
        return;
    }
    for engine in [&tac, &native] {
        assert!(!engine.evidence_key_filter().include(TAC_KEY));
        assert!(!engine.evidence_key_filter().include(NATIVE_MODEL_KEY));
        let [profiles] = engine.properties() else {
            panic!("the engine lists one property");
        };
        assert_eq!(profiles.name, PROFILES_PROPERTY_NAME);
        let items: Vec<&str> = profiles
            .item_properties
            .iter()
            .map(|item| item.name.as_str())
            .collect();
        assert_eq!(items, [PROFILE_ID_PROPERTY_NAME]);
    }
    let requests: [&[(&str, &str)]; 3] = [
        &[(TAC_KEY, VALID_TAC), (NATIVE_MODEL_KEY, VALID_NATIVE_MODEL)],
        &[(TAC_KEY, "1234567"), (NATIVE_MODEL_KEY, "a")],
        &[(USER_AGENT_KEY, USER_AGENT)],
    ];
    for evidence in requests {
        let data = process(&pipeline, evidence);
        assert!(data.get(HARDWARE_DATA_KEY).is_none(), "{evidence:?}");
        assert!(warnings(&data).is_empty(), "{evidence:?}");
    }
}

/// Each engine alone gives, for a value the data file holds, the profiles
/// holding it in ascending id order, each with its id, and lists a profile
/// as its id then properties of the key property's component.
#[test]
fn a_value_gives_the_profiles_holding_it() {
    let Some(device) = device_with_key_properties() else {
        return;
    };
    let engines = [
        (
            tac(),
            TAC_KEY,
            TAC_PROPERTY_NAME,
            is_tac as fn(&str) -> bool,
        ),
        (
            native(),
            NATIVE_MODEL_KEY,
            NATIVE_MODEL_PROPERTY_NAME,
            is_native_model as fn(&str) -> bool,
        ),
    ];
    for (engine, key, property, accept) in engines {
        let (value, ids) = held_value(&device, property, accept);
        let pipeline = pipeline(&device, &[&engine]);
        assert_eq!(engine.evidence_key(), Some(key));
        let data = process(&pipeline, &[(key, value.as_str())]);
        let hardware = data
            .get(HARDWARE_DATA_KEY)
            .unwrap_or_else(|| panic!("no data for {property} {value:?}"));
        assert_eq!(profile_ids(hardware), ids, "{property} {value:?}");
        assert!(
            ids.windows(2).all(|pair| pair[0] < pair[1]),
            "{property} {value:?}: {ids:?}"
        );
        assert_each_holds(hardware.profiles(), property, &value);
        assert!(warnings(&data).is_empty(), "{property} {value:?}");
        println!("{property} {value:?} gives {} profiles", ids.len());

        let [profiles] = engine.properties() else {
            panic!("the engine lists one property");
        };
        let items = &profiles.item_properties;
        assert_eq!(items[0].name, PROFILE_ID_PROPERTY_NAME);
        assert!(
            items.iter().any(|item| item.name == property),
            "{property} is not among the listed properties of a profile"
        );
    }
}

/// The engines keep the ids of the profiles they find and write none of the
/// profiles to the JSON, as .NET's do, so the JSON builder writes
/// `hardware.profiles` empty however many profiles hold the value, whilst
/// `profiles()` still gives every one of them.
#[test]
fn the_json_writes_no_on_premise_profiles() {
    use fiftyone_json_builder::{JsonBuilderElement, JSON_BUILDER_DATA_KEY};
    use fiftyone_pipeline_engines_fiftyone::SequenceElement;

    let Some(device) = device_with_key_properties() else {
        return;
    };
    let engines = [
        (
            tac(),
            TAC_KEY,
            TAC_PROPERTY_NAME,
            is_tac as fn(&str) -> bool,
        ),
        (
            native(),
            NATIVE_MODEL_KEY,
            NATIVE_MODEL_PROPERTY_NAME,
            is_native_model as fn(&str) -> bool,
        ),
    ];
    for (engine, key, property, accept) in engines {
        let (value, ids) = held_value(&device, property, accept);
        let json_builder = JsonBuilderElement::builder()
            .exclude_element(SequenceElement::DEFAULT_ELEMENT_DATA_KEY)
            .build();
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(SequenceElement::new()))
            .add_element(Arc::clone(&device) as Arc<dyn FlowElement>)
            .add_element(Arc::clone(&engine) as Arc<dyn FlowElement>)
            .add_element(Arc::new(json_builder))
            .build()
            .expect("the pipeline builds");
        let data = process(
            &pipeline,
            &[(key, value.as_str()), (USER_AGENT_KEY, USER_AGENT)],
        );
        let hardware = data
            .get(HARDWARE_DATA_KEY)
            .unwrap_or_else(|| panic!("no data for {property} {value:?}"));
        assert_eq!(profile_ids(hardware), ids, "{property} {value:?}");
        let Ok(PropertyValue::ElementDataList(written)) = hardware.get(PROFILES_PROPERTY_NAME)
        else {
            panic!("{PROFILES_PROPERTY_NAME} is not a list of element data");
        };
        assert!(
            written.is_empty(),
            "{property} {value:?} writes {} profiles",
            written.len()
        );

        let text = data
            .get(JSON_BUILDER_DATA_KEY)
            .expect("the JSON builder's data")
            .json()
            .to_owned();
        let json: serde_json::Value = serde_json::from_str(&text).expect("the output is JSON");
        assert_eq!(
            json["hardware"],
            serde_json::json!({ "profiles": [] }),
            "{property} {value:?}"
        );
        println!(
            "{property} {value:?} gives {} profiles and writes none",
            ids.len()
        );
    }
}

/// The TAC and Native engines in one pipeline add to one `hardware` data,
/// the TAC engine's profiles first. A request giving one key gets that
/// engine's profiles alone, and one giving neither gets no data.
#[test]
fn both_engines_add_to_one_hardware_data() {
    let Some(device) = device_with_key_properties() else {
        return;
    };
    let (tac_value, tac_ids) = held_value(&device, TAC_PROPERTY_NAME, is_tac);
    let (model, model_ids) = held_value(&device, NATIVE_MODEL_PROPERTY_NAME, is_native_model);
    let (tac, native) = (tac(), native());
    let pipeline = pipeline(&device, &[&tac, &native]);

    let data = process(
        &pipeline,
        &[
            (TAC_KEY, tac_value.as_str()),
            (NATIVE_MODEL_KEY, model.as_str()),
            (USER_AGENT_KEY, USER_AGENT),
        ],
    );
    let hardware = data.get(HARDWARE_DATA_KEY).expect("the engines' data");
    let both: Vec<u32> = tac_ids.iter().chain(&model_ids).copied().collect();
    assert_eq!(profile_ids(hardware), both);
    let (from_tac, from_model) = hardware.profiles().split_at(tac_ids.len());
    assert_each_holds(from_tac, TAC_PROPERTY_NAME, &tac_value);
    assert_each_holds(from_model, NATIVE_MODEL_PROPERTY_NAME, &model);
    assert!(warnings(&data).is_empty());

    let data = process(&pipeline, &[(NATIVE_MODEL_KEY, model.as_str())]);
    let hardware = data
        .get(HARDWARE_DATA_KEY)
        .expect("the Native engine's data");
    assert_eq!(profile_ids(hardware), model_ids);

    let data = process(&pipeline, &[(USER_AGENT_KEY, USER_AGENT)]);
    assert!(data.get(HARDWARE_DATA_KEY).is_none());
    assert!(warnings(&data).is_empty());
}

/// A TAC that is not eight digits, or a native model name shorter than two
/// characters, gives .NET's warning from the engine and data holding no
/// profiles, as .NET's engines create the data before checking the value.
#[test]
fn a_value_the_engine_refuses_gives_a_warning_and_no_profiles() {
    let Some(device) = device_with_key_properties() else {
        return;
    };
    let (tac, native) = (tac(), native());
    let pipeline = pipeline(&device, &[&tac, &native]);
    let tac_warning = |value: &str| {
        format!("The TAC value '{value}' is not valid. A TAC must be exactly 8 numeric digits.")
    };
    let native_warning = |value: &str| {
        format!(
            "The NativeModel value '{value}' is not valid. A NativeModel must be at least two \
             characters."
        )
    };
    let cases = [
        (TAC_KEY, "1234567", tac_warning("1234567")),
        (TAC_KEY, "abcdefgh", tac_warning("abcdefgh")),
        (TAC_KEY, "", tac_warning("")),
        (NATIVE_MODEL_KEY, "a", native_warning("a")),
        (NATIVE_MODEL_KEY, "", native_warning("")),
    ];
    for (key, value, warning) in cases {
        let data = process(&pipeline, &[(key, value)]);
        let hardware = data
            .get(HARDWARE_DATA_KEY)
            .unwrap_or_else(|| panic!("no data for {key} {value:?}"));
        assert!(hardware.is_empty(), "{key} {value:?}");
        assert_eq!(warnings(&data), [warning.as_str()], "{key} {value:?}");
        assert_eq!(
            data.warnings()[0].element_data_key.as_deref(),
            Some(HARDWARE_ELEMENT_DATA_KEY),
            "{key} {value:?}"
        );
    }
}
