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
//! ASN file `_51DEGREES_IPI_PATH` names, and skips with a note without it.

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_ip_intelligence_onpremise::{IpIntelligenceOnPremiseEngine, IP_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::DataUpdateService;

/// With update on start-up on, an engine builds from the data file it has when
/// the update cannot be fetched, as the specification says, and looks up with
/// it. The data file stays registered for later updates.
#[test]
fn an_engine_builds_from_its_data_file_when_the_update_on_start_up_fails() {
    let Some(asn) = std::env::var("_51DEGREES_IPI_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    else {
        eprintln!("skipping update on start-up: set _51DEGREES_IPI_PATH to the ASN file");
        return;
    };
    let dir = std::env::temp_dir().join(format!("fiftyone-ipi-startup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("51Degrees.ipi");
    std::fs::copy(&asn, &path).unwrap();
    // Nothing listens on a port once its listener is dropped.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/download", listener.local_addr().unwrap());
    drop(listener);

    let service = Arc::new(DataUpdateService::new());
    let engine = IpIntelligenceOnPremiseEngine::builder(&path)
        .performance_profile(PerformanceProfile::InMemory)
        .properties(["Asn"])
        .auto_update(false)
        .file_system_watcher(false)
        .update_on_startup(true)
        .data_update_url(url)
        .data_update_license_keys(["KEY"])
        .data_update_service(Arc::clone(&service))
        .build_shared()
        .expect("the engine uses the data file it has");

    assert_eq!(service.registered_count(), 1);
    let element: Arc<dyn FlowElement> = engine.clone();
    let pipeline = Pipeline::builder().add_element(element).build().unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("server.client-ip", "1.1.1.1")
            .build(),
    );
    data.process().unwrap();
    let asn_value = data.get(IP_DATA_KEY).unwrap().string("Asn");
    assert!(asn_value.value().is_ok(), "{asn_value:?}");

    drop(data);
    drop(pipeline);
    drop(engine);
    drop(service);
    let _ = std::fs::remove_dir_all(&dir);
}
