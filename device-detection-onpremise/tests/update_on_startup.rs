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

//! Update on start-up when the update cannot be fetched. The test needs the
//! Lite file `_51DEGREES_DD_PATH` names, and skips with a note without it.

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngineBuilder;
use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::{DataFileConfiguration, DataUpdateService};

/// A mobile Safari on iPhone User-Agent.
const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

/// With update on start-up on, an engine builds from the data file it has when
/// the update cannot be fetched, as the specification says, and detects with
/// it. The data file stays registered for later updates.
#[test]
fn an_engine_builds_from_its_data_file_when_the_update_on_start_up_fails() {
    let Some(lite) = std::env::var("_51DEGREES_DD_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    else {
        eprintln!("skipping update on start-up: set _51DEGREES_DD_PATH to the Lite file");
        return;
    };
    let dir = std::env::temp_dir().join(format!("fiftyone-dd-startup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("51Degrees.hash");
    std::fs::copy(&lite, &path).unwrap();
    // Nothing listens on a port once its listener is dropped.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/download", listener.local_addr().unwrap());
    drop(listener);

    let service = Arc::new(DataUpdateService::new());
    let config = DataFileConfiguration::builder(&path)
        .data_update_url(url)
        .data_update_license_keys(["KEY"])
        .update_on_startup(true)
        .automatic_updates_enabled(false)
        .file_system_watcher_enabled(false)
        .build();
    let engine = DeviceDetectionOnPremiseEngineBuilder::new(&path)
        .performance_profile(PerformanceProfile::InMemory)
        .auto_update(config, Some(Arc::clone(&service)))
        .build()
        .expect("the engine uses the data file it has");

    assert_eq!(service.registered_count(), 1);
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder().add_element(element).build().unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", MOBILE_USER_AGENT)
            .build(),
    );
    data.process().unwrap();
    assert_eq!(
        data.get(DEVICE_DATA_KEY)
            .unwrap()
            .is_mobile()
            .value()
            .ok()
            .copied(),
        Some(true)
    );

    drop(data);
    drop(pipeline);
    drop(engine);
    drop(service);
    let _ = std::fs::remove_dir_all(&dir);
}
