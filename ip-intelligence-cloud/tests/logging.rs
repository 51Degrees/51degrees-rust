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

//! What the IP Intelligence cloud engine logs at warning level or above, for
//! the cases where .NET's engine logs. Kept in its own test binary because a
//! logger is installed once for the whole process, and each test holds
//! [`SERIAL`] so the records it reads are its own.

use std::sync::{Arc, Mutex, MutexGuard};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_ip_intelligence_cloud::IpIntelligenceCloudEngine;
use fiftyone_pipeline_core::{Evidence, Pipeline};

const DEVICE_PRODUCT: &str = r#""device":{"DataTier":"CloudV4TAC","Properties":[
    {"Name":"IsMobile","Type":"Boolean","Category":"Device"}]}"#;
const IP_PRODUCT: &str = r#""ip":{"DataTier":"CloudV4","Properties":[
    {"Name":"RegisteredName","Type":"String","Category":"Network"}]}"#;

/// Records at warning level or above, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

/// Keeps the cloud engine's records at warning level or above in [`LOGGED`].
/// The checks are made in the cloud request engine crate, so its records are
/// the ones kept. The pipeline's own record of an element's error is left
/// out.
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
            && metadata
                .target()
                .starts_with("fiftyone_cloud_request_engine")
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

/// A fake cloud granting `product` and answering every data request with
/// `data`.
struct FakeCloud {
    product: &'static str,
    data: &'static str,
}

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let body = if request.url.contains("accessibleproperties") {
            format!(r#"{{"Products":{{{}}}}}"#, self.product)
        } else if request.url.contains("evidencekeys") {
            r#"["query.client-ip"]"#.to_owned()
        } else {
            self.data.to_owned()
        };
        Ok(CloudHttpResponse {
            status: 200,
            body,
            retry_after: None,
        })
    }
}

/// The request engine and the IP Intelligence engine, built with the logger
/// recording, and a pipeline holding both.
fn build(product: &'static str, data: &'static str) -> Arc<Pipeline> {
    let request = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .endpoint("https://cloud.example.test/api/v4/")
            .http_client(Arc::new(FakeCloud { product, data }))
            .build()
            .unwrap(),
    );
    let engine = IpIntelligenceCloudEngine::builder()
        .cloud_request_engine(Arc::clone(&request))
        .build()
        .unwrap();
    Pipeline::builder()
        .add_element(request)
        .add_element(Arc::new(engine))
        .build()
        .unwrap()
}

fn process(pipeline: &Arc<Pipeline>) {
    let evidence = Evidence::builder()
        .add("query.client-ip", "1.1.1.1")
        .build();
    pipeline.create_flow_data_with(evidence).process().unwrap();
}

#[test]
fn a_product_that_is_not_granted_logs_one_error_when_built() {
    let _serial = start();
    let pipeline = build(DEVICE_PRODUCT, r#"{"ip":{"registeredname":"Example"}}"#);
    assert_eq!(
        take_logged(),
        vec!["ERROR Aspect properties could not be loaded for IpIntelligenceCloudEngine - 'ip'"]
    );
    for request in 1..=2 {
        process(&pipeline);
        assert_eq!(take_logged(), Vec::<String>::new(), "request {request}");
    }
}

#[test]
fn a_property_with_no_metadata_logs_a_warning_on_every_request() {
    let _serial = start();
    let pipeline = build(
        IP_PRODUCT,
        r#"{"ip":{"registeredname":"Example","unlisted":"x",
            "registerednamenullreason":"why"}}"#,
    );
    assert_eq!(take_logged(), Vec::<String>::new());
    for request in 1..=2 {
        process(&pipeline);
        assert_eq!(
            take_logged(),
            vec!["WARN No meta-data entry for property 'unlisted' in 'IpIntelligenceCloudEngine'"],
            "request {request}"
        );
    }
}
