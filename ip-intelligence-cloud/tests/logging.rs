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

use std::sync::atomic::{AtomicBool, Ordering};
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

const NOT_GRANTED: &str = "WARN Aspect properties could not be loaded for \
     IpIntelligenceCloudEngine - 'ip'. The resource key does not include access to any \
     properties under 'ip'.";

/// Records at warning level or above, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

/// Keeps the records of this crate and the cloud request engine at warning
/// level or above in [`LOGGED`]. The pipeline's own record of an element's
/// error is left out.
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
            && (metadata
                .target()
                .starts_with("fiftyone_cloud_request_engine")
                || metadata
                    .target()
                    .starts_with("fiftyone_ip_intelligence_cloud"))
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
/// `data`. While `properties_down` is set the accessible properties
/// request cannot reach it.
struct FakeCloud {
    product: &'static str,
    data: &'static str,
    properties_down: Arc<AtomicBool>,
}

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        if request.url.contains("accessibleproperties")
            && self.properties_down.load(Ordering::Relaxed)
        {
            return Err("connection refused".to_owned());
        }
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
    build_on(product, data, Arc::default())
}

/// As [`build`], on a cloud whose accessible properties cannot be reached
/// while `properties_down` is set.
fn build_on(
    product: &'static str,
    data: &'static str,
    properties_down: Arc<AtomicBool>,
) -> Arc<Pipeline> {
    let request = Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .endpoint("https://cloud.example.test/api/v4/")
            .http_client(Arc::new(FakeCloud {
                product,
                data,
                properties_down,
            }))
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
fn a_product_that_is_not_granted_logs_one_warning_when_built() {
    let _serial = start();
    let pipeline = build(DEVICE_PRODUCT, r#"{"ip":{"registeredname":"Example"}}"#);
    assert_eq!(take_logged(), vec![NOT_GRANTED]);
    for request in 1..=2 {
        process(&pipeline);
        assert_eq!(take_logged(), Vec::<String>::new(), "request {request}");
    }
}

#[test]
fn a_product_listed_with_no_properties_logs_one_warning_when_built() {
    let _serial = start();
    let pipeline = build(
        r#""ip":{"DataTier":"CloudV4","Properties":[]}"#,
        r#"{"ip":{"registeredname":"Example"}}"#,
    );
    assert_eq!(take_logged(), vec![NOT_GRANTED]);
    for request in 1..=2 {
        process(&pipeline);
        assert_eq!(take_logged(), Vec::<String>::new(), "request {request}");
    }
}

#[test]
fn a_product_that_is_not_granted_is_logged_once_discovery_completes() {
    let _serial = start();
    let down = Arc::new(AtomicBool::new(true));
    // The cloud gives no `ip` member for a key without the product.
    let pipeline = build_on(DEVICE_PRODUCT, r#"{"device":{}}"#, Arc::clone(&down));
    let engine = &pipeline.flow_elements()[1];
    assert!(
        !take_logged()
            .iter()
            .any(|record| record.contains("Aspect properties")),
        "not known yet"
    );
    assert!(!engine.properties().is_empty(), "the defaults until then");

    down.store(false, Ordering::Relaxed);
    process(&pipeline);
    assert_eq!(take_logged(), vec![NOT_GRANTED]);
    assert!(engine.properties().is_empty(), "none once it is known");
    process(&pipeline);
    assert_eq!(take_logged(), Vec::<String>::new());
}

#[test]
fn a_property_with_no_metadata_logs_nothing() {
    let _serial = start();
    let pipeline = build(
        IP_PRODUCT,
        r#"{"ip":{"registeredname":"Example","unlisted":"x",
            "registerednamenullreason":"why"}}"#,
    );
    assert_eq!(take_logged(), Vec::<String>::new());
    for request in 1..=2 {
        process(&pipeline);
        assert_eq!(take_logged(), Vec::<String>::new(), "request {request}");
    }
}

/// With only a license key there are no accessible properties to read, so the
/// engine gives empty metadata, as the specification says, and logs nothing.
#[test]
fn a_license_key_alone_gives_empty_metadata_and_logs_nothing() {
    let _serial = start();
    let request = Arc::new(
        CloudRequestEngine::builder()
            .license_key("test-license-key")
            .values(["ip.registeredname"])
            .endpoint("https://cloud.example.test/api/v4/")
            .http_client(Arc::new(FakeCloud {
                product: IP_PRODUCT,
                data: r#"{"ip":{"registeredname":"Example"}}"#,
                properties_down: Arc::default(),
            }))
            .build()
            .unwrap(),
    );
    let engine = IpIntelligenceCloudEngine::builder()
        .cloud_request_engine(Arc::clone(&request))
        .build()
        .unwrap();
    let pipeline = Pipeline::builder()
        .add_element(request)
        .add_element(Arc::new(engine))
        .build()
        .unwrap();

    for request in 1..=3 {
        process(&pipeline);
        assert_eq!(take_logged(), Vec::<String>::new(), "request {request}");
    }
    assert!(pipeline.flow_elements()[1].properties().is_empty());
}
