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

//! Tests of update on start-up when the update cannot be fetched.
#![cfg(feature = "data-update")]

use std::net::TcpListener;
use std::sync::Arc;

use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData, Result,
};
use fiftyone_pipeline_engines::{
    AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, AutoUpdateStatus,
    DataFileConfiguration, DataUpdateService, OnPremiseAspectEngine,
};

/// An engine with data loaded, which a refresh leaves as it is.
struct LoadedEngine {
    filter: EvidenceKeyFilterWhitelist,
    data_files: Vec<Arc<AspectEngineDataFile>>,
}

impl FlowElement for LoadedEngine {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        "loaded"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

impl AspectEngine for LoadedEngine {
    fn data_source_tier(&self) -> &str {
        "Lite"
    }
    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &[]
    }
}

impl OnPremiseAspectEngine for LoadedEngine {
    fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
        &self.data_files
    }
    fn refresh(&self, _identifier: Option<&str>) -> Result<()> {
        Ok(())
    }
}

/// A failed update on start-up leaves the engine on the data it loaded, as the
/// specification says, so the data file is still registered for the updates
/// that follow, and the failure is reported.
#[test]
fn a_failed_update_on_start_up_still_registers_the_data_file() {
    // Nothing listens on a port once its listener is dropped.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/datafile", listener.local_addr().unwrap());
    drop(listener);
    let dir = std::env::temp_dir().join(format!("fiftyone-startup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("device.dat");
    std::fs::write(&path, b"the data loaded").unwrap();
    let config = DataFileConfiguration::builder(&path)
        .data_update_url(url)
        .update_on_startup(true)
        .automatic_updates_enabled(true)
        .file_system_watcher_enabled(false)
        .build();
    let data_file = Arc::new(AspectEngineDataFile::new(config));
    let engine: Arc<dyn OnPremiseAspectEngine> = Arc::new(LoadedEngine {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        data_files: vec![Arc::clone(&data_file)],
    });
    let service = DataUpdateService::new();

    let error = service
        .register(Arc::clone(&engine), data_file)
        .expect_err("nothing is listening");

    assert_eq!(error.status, AutoUpdateStatus::HttpsError);
    assert_eq!(service.registered_count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}
