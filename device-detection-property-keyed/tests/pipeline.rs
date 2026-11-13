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

//! A property keyed engine needs an on-premise Device Detection engine in
//! its pipeline. These tests need no data file.

use std::sync::Arc;

use fiftyone_device_detection_property_keyed::{
    NativeEngineBuilder, PropertyKeyedDeviceEngineBuilder, TacEngineBuilder,
};
use fiftyone_pipeline_core::{FlowElement, Pipeline};

fn build_error(engine: Arc<dyn FlowElement>) -> String {
    match Pipeline::builder().add_element(engine).build() {
        Ok(_) => panic!("a pipeline without a Device Detection engine was built"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn a_pipeline_without_device_detection_is_refused() {
    let engines: [Arc<dyn FlowElement>; 3] = [
        TacEngineBuilder::new().build().unwrap(),
        NativeEngineBuilder::new().build().unwrap(),
        PropertyKeyedDeviceEngineBuilder::new()
            .set_key_property("HardwareName")
            .build()
            .unwrap(),
    ];
    for engine in engines {
        let message = build_error(engine);
        assert!(
            message.contains("Ensure DeviceDetectionOnPremiseEngine is added to the pipeline"),
            "{message}"
        );
    }
}
