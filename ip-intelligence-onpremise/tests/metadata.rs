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

//! The engine's property and component metadata, read from the data file as
//! the .NET engine reads it. The tests run against the ASN file in the data
//! repository, or the file `51DEGREES_IPI_PATH` names, and skip when neither
//! is present. A test that needs an Enterprise file skips on any other, and
//! fails instead when `51DEGREES_REQUIRE_ENTERPRISE` is set. The failed
//! refresh test always uses the ASN file, because it holds two copies of the
//! file in memory and writes a third to disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fiftyone_ip_intelligence_onpremise::{
    IpIntelligenceData, IpIntelligenceOnPremiseEngine, IP_DATA_KEY,
};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{ElementData, Evidence, FlowElement, Pipeline, PropertyValue};
use fiftyone_pipeline_engines::{AspectEngine, MissingPropertyReason, OnPremiseAspectEngine};
use fiftyone_pipeline_engines_fiftyone::ValueMetaData;

/// The properties the .NET engine lists after the data file's.
const ADDED_PROPERTIES: [&str; 2] = ["Ip", "IpV6"];

/// Set where the Enterprise file must be given, as in the comparison with
/// .NET, so a test that needs it fails rather than skips.
const REQUIRE_ENTERPRISE: &str = "51DEGREES_REQUIRE_ENTERPRISE";

/// The ASN file in the data repository.
const ASN_FILE_NAME: &str = "51Degrees-IPIV4AsnIpiV41.ipi";

/// Skip `what` for `reason`, or fail when [`REQUIRE_ENTERPRISE`] is set.
fn skip_without_enterprise(reason: &str, what: &str) {
    assert!(
        std::env::var_os(REQUIRE_ENTERPRISE).is_none(),
        "{REQUIRE_ENTERPRISE} is set, but {reason}"
    );
    eprintln!("{reason}; skipping {what}");
}

/// The data file `51DEGREES_IPI_PATH` names, else [`asn_file`].
fn data_file() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("51DEGREES_IPI_PATH") {
        let path = PathBuf::from(path.trim());
        if path.is_file() {
            return Some(path);
        }
    }
    asn_file()
}

/// The ASN file in the data repository inside the `cxx` submodule.
fn asn_file() -> Option<PathBuf> {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join("cxx/data/ip-intelligence").join(ASN_FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// An engine on the data file, opened with `properties` named, or every
/// property when none is.
fn engine(properties: &[&str]) -> Option<IpIntelligenceOnPremiseEngine> {
    let file = data_file()?;
    let engine = IpIntelligenceOnPremiseEngine::builder(&file)
        .performance_profile(PerformanceProfile::LowMemory)
        .properties(properties.iter().copied())
        .auto_update(false)
        .file_system_watcher(false)
        .build()
        .unwrap_or_else(|err| panic!("{} should build an engine: {err}", file.display()));
    Some(engine)
}

/// The names of the properties the engine lists, in order.
fn listed(engine: &IpIntelligenceOnPremiseEngine) -> Vec<String> {
    engine
        .aspect_properties()
        .iter()
        .map(|property| property.name().to_owned())
        .collect()
}

/// Every property the data file defines is listed in the data file's order,
/// then `Ip` and `IpV6`, and the core, aspect and 51Degrees lists agree.
#[test]
fn every_data_file_property_is_listed_then_ip_and_ipv6() {
    let Some(engine) = engine(&[]) else {
        eprintln!("no IP Intelligence data file found; skipping the property list");
        return;
    };
    let names = listed(&engine);
    let populated = engine.populated_property_names();
    assert!(names.starts_with(populated), "{names:?}");
    assert_eq!(names[populated.len()..], ADDED_PROPERTIES);
    assert!(populated.iter().any(|name| name == "Asn"), "{populated:?}");

    let fiftyone = engine.fiftyone_properties();
    let core = engine.properties();
    let aspect = engine.aspect_properties();
    assert_eq!(fiftyone.len(), names.len());
    assert_eq!(core.len(), names.len());
    for ((property, core), aspect) in fiftyone.iter().zip(core).zip(aspect) {
        assert_eq!(property.aspect(), aspect);
        assert_eq!(&core.name, property.name());
        assert_eq!(core.value_type, aspect.core().value_type);
        assert!(core.available, "{}", core.name);
    }
}

/// An engine opened with some properties lists the same properties, marking
/// those it was not opened with unavailable, and reads only those it was.
#[test]
fn a_restricted_engine_lists_every_property_and_marks_the_others_unavailable() {
    let (Some(every), Some(engine)) = (engine(&[]), engine(&["asnname", "NotAProperty"])) else {
        eprintln!("no IP Intelligence data file found; skipping the restricted engine");
        return;
    };
    assert_eq!(listed(&engine), listed(&every));
    assert_eq!(engine.populated_property_names(), ["AsnName"]);
    for property in engine.aspect_properties() {
        let available = matches!(property.name(), "AsnName" | "Ip" | "IpV6");
        assert_eq!(property.available(), available, "{}", property.name());
    }
    let data = lookup(engine, "1.1.1.1");
    assert!(data.asn_name().has_value());
    assert!(!data.asn().has_value());
}

/// `Ip` and `IpV6` have the metadata .NET gives them.
#[test]
fn ip_and_ipv6_have_the_metadata_dotnet_gives_them() {
    let Some(engine) = engine(&[]) else {
        eprintln!("no IP Intelligence data file found; skipping Ip and IpV6");
        return;
    };
    let properties = engine.fiftyone_properties();
    for (name, version) in [("Ip", "IPv4"), ("IpV6", "IPv6")] {
        let property = properties
            .iter()
            .find(|property| property.name() == name)
            .unwrap_or_else(|| panic!("{name} is listed"));
        let aspect = property.aspect();
        assert_eq!(
            aspect.description(),
            format!("The {version} address of the request as a string.")
        );
        assert_eq!(aspect.core().category, "Network");
        assert!(aspect.data_tiers_where_present().is_empty());
        assert!(aspect.available());
        assert_eq!(property.component_id(), Some(255));
        assert_eq!(property.component_name(), "Network");
        assert_eq!(property.display_order(), 255);
        assert!(!property.mandatory() && !property.list() && !property.obsolete());
        assert!(property.show() && property.show_values());
        assert_eq!(property.url(), "");
        assert_eq!(property.default_value(), Some(&ValueMetaData::new("N/A")));
        assert_eq!(property.values(), [ValueMetaData::new("N/A")]);
    }
}

/// The components are the data file's, each holding exactly its own
/// properties, then an empty `Metrics` component. `Ip` and `IpV6` are in no
/// listed component, as in .NET.
#[test]
fn components_hold_their_properties_then_an_empty_metrics_component() {
    let Some(engine) = engine(&[]) else {
        eprintln!("no IP Intelligence data file found; skipping components");
        return;
    };
    let components = engine.components();
    let (metrics, file_components) = components.split_last().expect("components are listed");
    assert_eq!(
        (metrics.component_id, metrics.name.as_str()),
        (255, "Metrics")
    );
    assert!(metrics.properties().is_empty());
    assert_eq!(metrics.default_profile_id, None);
    assert!(file_components
        .iter()
        .any(|component| component.name == "Asn"));
    let properties = engine.fiftyone_properties();
    for component in file_components {
        let own: Vec<&str> = properties
            .iter()
            .filter(|property| property.component_id() == Some(component.component_id))
            .map(|property| property.name())
            .collect();
        let held: Vec<&str> = component
            .properties()
            .iter()
            .map(|property| property.name())
            .collect();
        assert_eq!(held, own, "{}", component.name);
        assert!(!own.is_empty(), "{}", component.name);
        assert!(component.default_profile_id.is_some(), "{}", component.name);
    }
    for name in ADDED_PROPERTIES {
        assert!(
            components
                .iter()
                .all(|component| component.property(name).is_none()),
            "{name}"
        );
    }
}

/// A property's values are read from the data file when asked for, whether
/// or not the engine was opened with it, and a value is found by its exact
/// text.
#[test]
fn property_values_are_read_from_the_data_file() {
    let Some(engine) = engine(&["Asn"]) else {
        eprintln!("no IP Intelligence data file found; skipping values");
        return;
    };
    for name in ["Asn", "AsnName"] {
        let count = engine.property_value_count(name).expect("listed");
        let shown = count.min(1000);
        let values = engine
            .property_value_range(name, 0..shown)
            .expect("values are read")
            .expect("listed");
        assert_eq!(values.len(), shown, "{name}");
        let property = engine
            .fiftyone_properties()
            .iter()
            .find(|property| property.name() == name)
            .cloned()
            .expect("listed");
        assert!(
            property.values().is_empty(),
            "{name}: values are read on request"
        );
        let default = property.default_value().expect("a default").clone();
        assert_eq!(
            engine
                .property_value(name, &default.name)
                .expect("searched"),
            Some(default),
            "{name}"
        );
    }
    let first: Vec<String> = engine
        .property_value_range("Asn", 0..3)
        .expect("values are read")
        .expect("listed")
        .into_iter()
        .map(|value| value.name)
        .collect();
    let all: Vec<String> = engine
        .property_values("Asn")
        .expect("values are read")
        .expect("listed")
        .into_iter()
        .map(|value| value.name)
        .collect();
    assert_eq!(first, all[..3]);
    // The ASN file in the data repository changes only when the submodule
    // moves, so its first values are known. Other files gain values.
    let file = data_file().expect("the engine's data file");
    if file.file_name().is_some_and(|name| name == ASN_FILE_NAME) {
        assert_eq!(first, ["AS0", "AS1", "AS1000"]);
    }
    assert_eq!(
        engine
            .property_value("Asn", "AS13335")
            .expect("searched")
            .map(|value| value.name),
        Some(String::from("AS13335"))
    );
    assert_eq!(
        engine.property_value("Asn", "as13335").expect("searched"),
        None
    );
    assert_eq!(
        engine.property_values("Ip").expect("listed values"),
        Some(vec![ValueMetaData::new("N/A")])
    );
    assert_eq!(engine.property_value_count("Ip"), Some(1));
    assert_eq!(
        engine.property_value("Ip", "N/A").expect("searched"),
        Some(ValueMetaData::new("N/A"))
    );
    assert_eq!(engine.property_value("Ip", "n/a").expect("searched"), None);
    assert_eq!(engine.property_values("asn").expect("no error"), None);
    assert_eq!(
        engine.property_values("NotAProperty").expect("no error"),
        None
    );
    assert_eq!(engine.property_value_count("NotAProperty"), None);
}

/// A property the data file defines but the engine was not opened with is
/// excluded by configuration, through the engine and through IP data, with
/// .NET's text.
#[test]
fn a_property_left_out_by_configuration_is_excluded() {
    let Some(engine) = engine(&["Asn"]) else {
        eprintln!("no IP Intelligence data file found; skipping the excluded property");
        return;
    };
    let expected = "Property 'AsnName' not found in data for element 'ip'. This is because \
                    the property has been excluded when configuring the engine.";
    let result = engine.missing_property_reason("AsnName");
    assert_eq!(
        result.reason,
        MissingPropertyReason::PropertyExcludedFromConfig
    );
    assert_eq!(result.description, expected);
    let data = lookup(engine, "1.1.1.1");
    assert_eq!(data.asn_name().no_value_message(), Some(expected));
    assert_eq!(data.get("AsnName").unwrap_err().message, expected);
}

/// On an Enterprise file the GeoNames identifiers read through their typed
/// accessors, `StateGeoNameIdsGeographical` as a weighted list and
/// `ContinentGeoNameId` as a string, and the property bag holds the same.
#[test]
fn the_geonames_identifiers_read_through_their_typed_accessors() {
    let Some(engine) = engine(&[]) else {
        skip_without_enterprise(
            "no IP Intelligence data file found",
            "undeclared properties",
        );
        return;
    };
    if !engine
        .populated_property_names()
        .iter()
        .any(|name| name == "StateGeoNameIdsGeographical")
    {
        let file = data_file().expect("the engine's data file");
        let reason = format!("{} is not an Enterprise file", file.display());
        skip_without_enterprise(&reason, "undeclared properties");
        return;
    }
    let data = lookup(engine, "1.1.1.1");
    assert!(
        matches!(
            data.get("StateGeoNameIdsGeographical"),
            Ok(PropertyValue::KeyValueList(_))
        ),
        "{:?}",
        data.get("StateGeoNameIdsGeographical")
    );
    assert!(
        matches!(data.get("ContinentGeoNameId"), Ok(PropertyValue::String(_))),
        "{:?}",
        data.get("ContinentGeoNameId")
    );
    let states = data.state_geo_name_ids_geographical();
    let states = states.value().expect("the weighted state identifiers");
    assert!(!states.is_empty());
    let mut previous = u16::MAX;
    for state in states {
        assert!(state.raw_weighting <= previous, "highest weighting first");
        previous = state.raw_weighting;
        assert!(!state.value.is_empty());
    }
    let continent = data.continent_geo_name_id();
    let continent = continent.value().expect("the continent identifier");
    assert!(!continent.is_empty());
}

/// A refresh reads the metadata of the data file again into new lists.
#[test]
fn a_refresh_reads_the_metadata_again() {
    let Some(engine) = engine(&[]) else {
        eprintln!("no IP Intelligence data file found; skipping refresh");
        return;
    };
    let before = engine.fiftyone_properties();
    let components = engine.components();
    engine.refresh(None).expect("the refresh succeeds");
    let after = engine.fiftyone_properties();
    assert!(!Arc::ptr_eq(&before, &after));
    assert_eq!(*before, *after);
    assert_eq!(*components, *engine.components());
    assert_eq!(listed(&engine).len(), after.len());
}

/// Where the data file's header gives its publish date and the place of its
/// properties, and where a property's description offset sits in its record,
/// from `ipi.h`, `collection.h` and `property.h` in 51degrees-cxx with the
/// 64 bit file offsets the IP Intelligence format uses.
const PUBLISHED_HEADER: usize = 66;
const PROPERTIES_HEADER: usize = 122;
const PROPERTY_SIZE: usize = 44;
const DESCRIPTION_OFFSET: usize = 16;

/// `file` published a year later, with its first property's description
/// past the end of the strings. The data set loads, because loading reads no
/// description, and reading the engine's metadata then fails.
fn later_with_unreadable_metadata(file: &Path) -> Vec<u8> {
    let mut bytes = std::fs::read(file).unwrap();
    let year_at = PUBLISHED_HEADER..PUBLISHED_HEADER + 2;
    let year = i16::from_le_bytes(bytes[year_at.clone()].try_into().unwrap());
    bytes[year_at].copy_from_slice(&(year + 1).to_le_bytes());
    let read_u32 = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let start = u64::from_le_bytes(
        bytes[PROPERTIES_HEADER..PROPERTIES_HEADER + 8]
            .try_into()
            .unwrap(),
    ) as usize;
    let length = read_u32(PROPERTIES_HEADER + 8) as usize;
    let count = read_u32(PROPERTIES_HEADER + 12) as usize;
    assert_eq!(length, count * PROPERTY_SIZE, "the properties are found");
    let at = start + DESCRIPTION_OFFSET;
    bytes[at..at + 4].copy_from_slice(&(i32::MAX as u32).to_le_bytes());
    bytes
}

/// A refresh to a data file that loads but whose metadata cannot be read
/// fails and changes nothing. The engine keeps the old file's publish date,
/// so the update service's `If-Modified-Since` still asks for the new file.
/// The test needs no particular file, so it uses the small ASN file whatever
/// `51DEGREES_IPI_PATH` names.
#[test]
fn a_refresh_whose_metadata_cannot_be_read_changes_nothing() {
    let Some(file) = asn_file() else {
        eprintln!("no ASN data file found; skipping the failed refresh");
        return;
    };
    let dir = std::env::temp_dir().join(format!(
        "fiftyone-ipi-unreadable-metadata-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("51Degrees.ipi");
    std::fs::copy(&file, &path).unwrap();
    // In memory, so the data file can be replaced on every platform.
    let engine = IpIntelligenceOnPremiseEngine::builder(&path)
        .performance_profile(PerformanceProfile::InMemory)
        .auto_update(false)
        .file_system_watcher(false)
        .build()
        .expect("the data file loads");
    let published = engine.data_file_published().expect("a publish date");
    let properties = engine.fiftyone_properties();

    std::fs::write(&path, later_with_unreadable_metadata(&file)).unwrap();
    let error = engine
        .refresh(None)
        .expect_err("the new metadata cannot be read");

    assert!(
        error
            .to_string()
            .contains("the data file's metadata could not be read (property 0)"),
        "{error}"
    );
    assert_eq!(engine.data_file_published(), Some(published));
    assert!(Arc::ptr_eq(&properties, &engine.fiftyone_properties()));
    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Look `ip` up through a one-element pipeline and return the IP data.
fn lookup(
    engine: IpIntelligenceOnPremiseEngine,
    ip: &str,
) -> fiftyone_ip_intelligence_onpremise::IpIntelligenceDataBase {
    let element: Arc<dyn FlowElement> = Arc::new(engine);
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("pipeline builds");
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.client-ip", ip).build());
    data.process().expect("processing succeeds");
    data.get(IP_DATA_KEY)
        .expect("ip intelligence data was produced")
        .clone()
}
