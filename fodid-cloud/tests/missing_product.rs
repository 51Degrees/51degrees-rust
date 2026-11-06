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

//! What the 51Did cloud engine logs and reports when the resource key grants
//! no 51Did product, or the request for its property list fails. Kept in its
//! own test binary because a logger is installed once for the whole process.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
    CloudRequestEngineBuilder,
};
use fiftyone_fodid_cloud::FodIdCloudEngine;
use fiftyone_pipeline_core::{Evidence, FlowElement, MissingPropertyReason, Pipeline};
use fiftyone_pipeline_engines::AspectEngine;

/// Records from this crate at warning level or above, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

/// Keeps this crate's records at warning level or above in [`LOGGED`].
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
            && metadata.target().starts_with("fiftyone_fodid_cloud")
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            LOGGED
                .lock()
                .unwrap()
                .push(format!("{} {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}

static LOGGER: TestLogger = TestLogger;

const MISSING_PRODUCT_ERROR: &str =
    "ERROR Aspect properties could not be loaded for FodIdCloudEngine - 'fodid'";

/// Install the logger, wait for any other test to finish and clear the
/// records it left.
fn start() -> MutexGuard<'static, ()> {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    LOGGED.lock().unwrap().clear();
    guard
}

/// Take the records logged since [`start`] or the last call.
fn take_logged() -> Vec<String> {
    std::mem::take(&mut *LOGGED.lock().unwrap())
}

/// A cloud whose accessible properties grant only the device product.
struct DeviceOnlyCloud;

impl CloudHttpClient for DeviceOnlyCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let body = if request.url.contains("accessibleproperties") {
            r#"{"Products":{"device":{"DataTier":"CloudV4Free","Properties":[{"Name":"IsMobile","Type":"Bool","Category":"Device"}]}}}"#
        } else if request.url.contains("evidencekeys") {
            r#"["header.user-agent"]"#
        } else {
            r#"{"device":{"ismobile":true}}"#
        };
        Ok(CloudHttpResponse {
            status: 200,
            body: body.to_owned(),
            retry_after: None,
        })
    }
}

/// A pipeline of the request engine and the 51Did engine, and the 51Did
/// engine itself.
fn pipeline(request_engine: CloudRequestEngineBuilder) -> (Arc<Pipeline>, Arc<FodIdCloudEngine>) {
    let request_engine = Arc::new(
        request_engine
            .http_client(Arc::new(DeviceOnlyCloud))
            .build()
            .unwrap(),
    );
    let fodid = Arc::new(
        FodIdCloudEngine::builder()
            .cloud_request_engine(Arc::clone(&request_engine))
            .build()
            .unwrap(),
    );
    let pipeline = Pipeline::builder()
        .add_element(request_engine)
        .add_element(Arc::clone(&fodid) as Arc<dyn FlowElement>)
        .build()
        .unwrap();
    (pipeline, fodid)
}

fn process(pipeline: &Arc<Pipeline>) {
    let evidence = Evidence::builder()
        .add("header.user-agent", "Mozilla/5.0")
        .build();
    let _ = pipeline.create_flow_data_with(evidence).process();
}

/// A resource key without the 51Did product gives empty metadata, and the
/// error is logged once, however many requests follow.
#[test]
fn a_resource_key_without_the_product_logs_one_error_and_gives_empty_metadata() {
    let _serial = start();
    let (pipeline, fodid) = pipeline(CloudRequestEngine::builder().resource_key("rk"));
    assert_eq!(take_logged(), Vec::<String>::new());

    for _ in 0..3 {
        process(&pipeline);
    }
    assert_eq!(take_logged(), vec![MISSING_PRODUCT_ERROR]);
    assert!(fodid.has_loaded_properties());
    assert!(fodid.properties_loaded());
    assert!(fodid.properties().is_empty());
    assert!(fodid.aspect_properties().is_empty());
    assert!(!fodid.property_list_request_failed());
    assert_eq!(
        fodid.missing_property_reason("IdProbGlobal").reason,
        MissingPropertyReason::ProductNotAccessibleWithResourceKey
    );
}

/// A cloud that refuses the property list request and answers the rest.
struct PropertiesDownCloud;

impl CloudHttpClient for PropertiesDownCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        if request.url.contains("accessibleproperties") {
            return Err("connection refused".to_owned());
        }
        DeviceOnlyCloud.send(request)
    }
}

/// While the property list request has failed, a missing property's reason is
/// the failed request, whatever its name.
#[test]
fn a_failed_property_list_request_is_the_reason_a_property_is_missing() {
    let _serial = start();
    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("rk")
            .http_client(Arc::new(PropertiesDownCloud))
            .build()
            .unwrap(),
    );
    let fodid = FodIdCloudEngine::builder()
        .cloud_request_engine(request_engine)
        .build()
        .unwrap();

    assert!(!fodid.has_loaded_properties());
    assert!(!fodid.properties_loaded());
    assert!(fodid.property_list_request_failed());
    let missing = fodid.missing_property_reason("IdProbGlobal");
    assert_eq!(missing.reason, MissingPropertyReason::CloudRequestFailed);
    assert_eq!(
        missing.description,
        "Property 'IdProbGlobal' not found in data for element 'fodid'. This is because \
         an upstream cloud request failed, so no data was available to populate the \
         property."
    );
    assert_eq!(
        fodid.missing_property_reason("NotAProperty").reason,
        MissingPropertyReason::CloudRequestFailed
    );
    assert_eq!(take_logged(), Vec::<String>::new());
}

/// With only a license key there are no accessible properties to read, so the
/// engine gives empty metadata, as the specification says, and logs nothing.
#[test]
fn a_license_key_alone_gives_empty_metadata_and_logs_nothing() {
    let _serial = start();
    let (pipeline, fodid) = pipeline(
        CloudRequestEngine::builder()
            .license_key("lk")
            .values(["device.ismobile"]),
    );

    for _ in 0..3 {
        process(&pipeline);
    }
    assert_eq!(take_logged(), Vec::<String>::new());
    assert!(fodid.has_loaded_properties());
    assert!(fodid.properties().is_empty());
    assert!(fodid.aspect_properties().is_empty());
}

/// A cloud granting the 51Did product that refuses the property list request
/// while `properties_down` is set and answers the rest.
struct RecoveringCloud {
    properties_down: Arc<AtomicBool>,
}

impl CloudHttpClient for RecoveringCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        if !request.url.contains("accessibleproperties") {
            return DeviceOnlyCloud.send(request);
        }
        if self.properties_down.load(Ordering::Relaxed) {
            return Err("connection refused".to_owned());
        }
        Ok(CloudHttpResponse {
            status: 200,
            body: r#"{"Products":{"fodid":{"DataTier":"CloudV4","Properties":[
                {"Name":"IdProbGlobal","Type":"String","Category":"Identifier"}]}}}"#
                .to_owned(),
            retry_after: None,
        })
    }
}

/// An engine that has not processed since discovery completed reads the
/// property list when asked whether its properties are loaded.
#[test]
fn an_engine_that_has_not_processed_reads_the_property_list_when_asked() {
    let _serial = start();
    let down = Arc::new(AtomicBool::new(true));
    let request_engine = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("rk")
            .http_client(Arc::new(RecoveringCloud {
                properties_down: Arc::clone(&down),
            }))
            .build()
            .unwrap(),
    );
    let fodid = FodIdCloudEngine::builder()
        .cloud_request_engine(Arc::clone(&request_engine))
        .build()
        .unwrap();
    assert!(!fodid.properties_loaded());

    // Only the request engine processes, so discovery completes without the
    // 51Did engine reading the list.
    down.store(false, Ordering::Relaxed);
    let pipeline = Pipeline::builder()
        .add_element(Arc::clone(&request_engine) as Arc<dyn FlowElement>)
        .build()
        .unwrap();
    let evidence = Evidence::builder()
        .add("header.user-agent", "Mozilla/5.0")
        .build();
    let mut data = pipeline.create_flow_data_with(evidence);
    data.process().unwrap();
    assert!(request_engine.public_properties().is_ok());
    assert!(!fodid.has_loaded_properties());

    assert!(fodid.properties_loaded());
    let names: Vec<&str> = fodid
        .properties()
        .iter()
        .map(|property| property.name.as_str())
        .collect();
    assert_eq!(names, ["IdProbGlobal"]);
    assert_eq!(take_logged(), Vec::<String>::new());
}
