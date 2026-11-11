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

//! A refresh, from the file or from memory, to a data file with other
//! properties or another tier leaves the engine as an engine built on that
//! file would be.
//!
//! The test needs the Lite file `_51DEGREES_DD_PATH` names and a data file
//! with more properties beside it, `Enterprise-HashV41.hash` or
//! `TAC-HashV41.hash`, and skips with a note when either is missing.

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};
use fiftyone_device_detection_shared::DEVICE_DATA_KEY;
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{ElementData, Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::{AspectEngine, OnPremiseAspectEngine};

/// A mobile Safari on iPhone User-Agent.
const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

/// A property the Lite file does not carry and the richer files do.
const RICHER_PROPERTY: &str = "HardwareModel";

/// The Lite file and a file with more properties beside it.
fn data_files() -> Option<(PathBuf, PathBuf)> {
    let lite = PathBuf::from(std::env::var("_51DEGREES_DD_PATH").ok()?);
    let folder = lite.parent()?;
    let richer = ["Enterprise-HashV41.hash", "TAC-HashV41.hash"]
        .into_iter()
        .map(|name| folder.join(name))
        .find(|path| path.is_file())?;
    lite.is_file().then_some((lite, richer))
}

/// An engine that holds the whole data set in memory, so its data file can
/// be replaced on every platform.
fn engine(path: &PathBuf) -> Arc<DeviceDetectionOnPremiseEngine> {
    DeviceDetectionOnPremiseEngineBuilder::new(path)
        .performance_profile(PerformanceProfile::InMemory)
        .build()
        .expect("the data file loads")
}

/// The value of [`RICHER_PROPERTY`] for a mobile User-Agent, or the reason
/// there is none.
fn richer_value(engine: &Arc<DeviceDetectionOnPremiseEngine>) -> Result<String, String> {
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder().add_element(element).build().unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", MOBILE_USER_AGENT)
            .build(),
    );
    data.process().unwrap();
    let device = data.get(DEVICE_DATA_KEY).unwrap();
    device
        .get(RICHER_PROPERTY)
        .map(|value| format!("{value:?}"))
        .map_err(|no_value| no_value.message)
}

#[test]
fn a_refresh_reads_and_reports_the_properties_of_the_new_data_file() {
    let Some((lite, richer)) = data_files() else {
        eprintln!(
            "skipping the refresh test: set _51DEGREES_DD_PATH to the Lite file, with \
             Enterprise-HashV41.hash or TAC-HashV41.hash beside it"
        );
        return;
    };
    let dir = std::env::temp_dir().join(format!("fiftyone-dd-refresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("51Degrees.hash");
    std::fs::copy(&lite, &path).unwrap();

    let refreshed = engine(&path);
    assert!(
        richer_value(&refreshed).is_err(),
        "the Lite file carries {RICHER_PROPERTY}"
    );
    std::fs::copy(&richer, &path).unwrap();
    refreshed.refresh(None).expect("the richer file loads");
    let built = engine(&path);

    assert_eq!(refreshed.data_source_tier(), built.data_source_tier());
    assert_eq!(
        refreshed.available_properties(),
        built.available_properties()
    );
    assert_eq!(refreshed.properties(), built.properties());
    assert_eq!(refreshed.fiftyone_properties(), built.fiftyone_properties());
    assert_eq!(refreshed.components(), built.components());
    assert_eq!(refreshed.aspect_properties(), built.aspect_properties());
    let value = richer_value(&refreshed);
    assert!(
        value.is_ok(),
        "{RICHER_PROPERTY} after the refresh: {value:?}"
    );
    assert_eq!(value, richer_value(&built));

    drop(refreshed);
    drop(built);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A refresh from memory leaves the engine as an engine built on the file
/// holding that data would be, with the date that data was published.
#[test]
fn a_refresh_from_memory_reads_and_reports_the_properties_of_the_new_data() {
    let Some((lite, richer)) = data_files() else {
        eprintln!(
            "skipping the refresh from memory test: set 51DEGREES_DD_PATH to the Lite \
             file, with Enterprise-HashV41.hash or TAC-HashV41.hash beside it"
        );
        return;
    };
    let refreshed = engine(&lite);
    assert!(
        richer_value(&refreshed).is_err(),
        "the Lite file carries {RICHER_PROPERTY}"
    );
    let data = std::fs::read(&richer).unwrap();
    refreshed
        .refresh_from_memory(None, &data)
        .expect("the richer data loads");
    drop(data);
    let built = engine(&richer);

    assert_eq!(refreshed.data_source_tier(), built.data_source_tier());
    assert_eq!(
        refreshed.available_properties(),
        built.available_properties()
    );
    assert_eq!(refreshed.properties(), built.properties());
    assert_eq!(refreshed.fiftyone_properties(), built.fiftyone_properties());
    assert_eq!(refreshed.components(), built.components());
    assert_eq!(refreshed.aspect_properties(), built.aspect_properties());
    let value = richer_value(&refreshed);
    assert!(
        value.is_ok(),
        "{RICHER_PROPERTY} after the refresh: {value:?}"
    );
    assert_eq!(value, richer_value(&built));
    assert_eq!(
        refreshed.data_files()[0].data_published(),
        built.data_files()[0].data_published()
    );
    assert_eq!(
        refreshed.data_files()[0]
            .update_available_time()
            .map(|time| time.date_naive()),
        built.data_files()[0]
            .update_available_time()
            .map(|time| time.date_naive())
    );
}
