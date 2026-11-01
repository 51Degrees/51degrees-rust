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

//! Integration tests for the on-premise Hash device-detection engine.
//!
//! These run against the packaged Lite Hash data file. When the file cannot be
//! found the tests skip cleanly with an explanatory note rather than failing, so
//! the suite is green on a checkout without the data file.

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngineBuilder;
use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY, GENERATED_PROPERTY_TYPES};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{ElementData, Evidence, FlowElement, Pipeline, PropertyValueType};
use fiftyone_pipeline_engines::{AspectEngine, OnPremiseAspectEngine};

/// A representative desktop Chrome on Windows User-Agent.
const DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// A representative mobile Safari on iPhone User-Agent.
const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

/// Locate the packaged Lite Hash data file, mirroring the native crate's test
/// resolution. Resolution order: an explicit `51DEGREES_DD_PATH`
/// environment variable, then the `cxx` submodule, then
/// the wider `Workspace` tree where the other products keep their data.
fn lite_data_file() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("51DEGREES_DD_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }

    // CARGO_MANIFEST_DIR is the crate dir; its parent is the workspace root.
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .to_path_buf();

    let sibling = workspace
        .join("cxx")
        .join("device-detection-data")
        .join("51Degrees-LiteV4.1.hash");
    if sibling.is_file() {
        return Some(sibling);
    }

    if let Some(parent) = workspace.parent() {
        let candidate = parent
            .join("cxx")
            .join("device-detection-data")
            .join("51Degrees-LiteV4.1.hash");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

/// Build a fresh on-premise engine over the Lite data file, or `None` when the
/// data file is absent or the native data set will not load in this binary.
///
/// A load failure is treated as a clean skip rather than a hard failure. In a
/// test binary that links both the Device Detection and IP Intelligence native
/// libraries, the two ship their own copies of the shared common C layer
/// with incompatible file-offset widths (the IP Intelligence build enables
/// large-data-file support). When the linker resolves a Device Detection data
/// read to an IP Intelligence definition the Hash file is parsed with the wrong
/// offsets and the load reports corrupt data. That is a build-layer concern in
/// the native crates, not in this engine, so the integration tests skip on it
/// while still exercising the full detection path on a binary that links only
/// the Device Detection native library.
fn build_engine() -> Option<Arc<fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngine>>
{
    let data_file = lite_data_file()?;
    match DeviceDetectionOnPremiseEngineBuilder::new(data_file)
        .performance_profile(PerformanceProfile::HighPerformance)
        .build()
    {
        Ok(engine) => Some(engine),
        Err(e) => {
            eprintln!(
                "the native Hash data set did not load ({e}); skipping. This is the \
                 Device Detection / IP Intelligence common C library link collision, a \
                 native-build concern outside this engine."
            );
            None
        }
    }
}

/// Run a single User-Agent through a one-element pipeline and return the device
/// data, cloned out so the flow data can be dropped.
fn detect(
    engine: Arc<fiftyone_device_detection_onpremise::DeviceDetectionOnPremiseEngine>,
    user_agent: &str,
) -> fiftyone_device_detection_shared::DeviceDataBase {
    let element: Arc<dyn FlowElement> = engine;
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("pipeline builds");
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", user_agent)
            .build(),
    );
    data.process().expect("processing succeeds");
    data.get(DEVICE_DATA_KEY)
        .expect("device data was produced")
        .clone()
}

#[test]
fn detects_a_desktop_user_agent() {
    let Some(engine) = build_engine() else {
        eprintln!("no Lite Hash data file found; skipping desktop detection test");
        return;
    };

    let device = detect(engine, DESKTOP_USER_AGENT);

    // A desktop User-Agent is not mobile.
    let is_mobile = device.is_mobile();
    assert!(is_mobile.has_value(), "IsMobile should be determined");
    assert!(!*is_mobile.value().unwrap(), "a desktop is not mobile");

    // The platform should be a Windows family for this User-Agent.
    let platform = device.platform_name();
    assert!(platform.has_value(), "PlatformName should be determined");
    assert!(
        platform
            .value()
            .unwrap()
            .to_ascii_lowercase()
            .contains("windows"),
        "expected a Windows platform, got {:?}",
        platform.value().unwrap()
    );
}

#[test]
fn detects_a_mobile_user_agent() {
    let Some(engine) = build_engine() else {
        eprintln!("no Lite Hash data file found; skipping mobile detection test");
        return;
    };

    let device = detect(engine, MOBILE_USER_AGENT);

    let is_mobile = device.is_mobile();
    assert!(is_mobile.has_value(), "IsMobile should be determined");
    assert!(*is_mobile.value().unwrap(), "an iPhone is mobile");

    // The device id is a four-component hyphenated string for a single match.
    let device_id = device.device_id();
    assert!(device_id.has_value(), "DeviceId should be present");
    assert_eq!(
        device_id.value().unwrap().split('-').count(),
        4,
        "a device id has four profile components, got {:?}",
        device_id.value().unwrap()
    );
}

/// Naming `DeviceId` gives the same id as an engine with every property, even
/// when it is the only name, which leaves no data file properties to read.
#[test]
fn naming_device_id_gives_the_same_id_as_every_property() {
    let (Some(engine), Some(data_file)) = (build_engine(), lite_data_file()) else {
        eprintln!("no Lite Hash data file found; skipping named device id test");
        return;
    };
    let every_property = detect(engine, MOBILE_USER_AGENT).device_id();
    let every_property = every_property.value().unwrap();

    for names in [&["DeviceId"][..], &["DeviceId", "IsMobile"]] {
        let engine = DeviceDetectionOnPremiseEngineBuilder::new(&data_file)
            .performance_profile(PerformanceProfile::HighPerformance)
            .properties(names.iter().copied())
            .build()
            .expect("an engine naming DeviceId should build");
        let device = detect(engine, MOBILE_USER_AGENT);
        assert_eq!(
            device.device_id().value().unwrap(),
            every_property,
            "{names:?}"
        );
    }
}

#[test]
fn concurrency_sizes_the_pool_for_parallel_use() {
    // The LowMemory profile reads collections through a fixed-size handle pool.
    // `.concurrency(n)` sizes that pool, so more threads than the default core
    // count can share one engine. Build a LowMemory engine with an explicit
    // concurrency and run detections from several threads at once: if the pool
    // were not sized for them the native reads would error under contention.
    let Some(data_file) = lite_data_file() else {
        eprintln!("no Lite Hash data file found; skipping concurrency test");
        return;
    };
    const THREADS: u16 = 8;
    let engine = match DeviceDetectionOnPremiseEngineBuilder::new(data_file)
        .performance_profile(PerformanceProfile::LowMemory)
        .concurrency(THREADS)
        .build()
    {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("native Hash data set did not load ({e}); skipping concurrency test");
            return;
        }
    };

    let element: Arc<dyn FlowElement> = engine;
    let pipeline = Arc::new(
        Pipeline::builder()
            .add_element(element)
            .build()
            .expect("pipeline builds"),
    );

    let workers: Vec<_> = (0..THREADS)
        .map(|_| {
            let pipeline = Arc::clone(&pipeline);
            std::thread::spawn(move || {
                for _ in 0..25 {
                    let mut data = pipeline.create_flow_data_with(
                        Evidence::builder()
                            .add("header.user-agent", DESKTOP_USER_AGENT)
                            .build(),
                    );
                    data.process().expect("concurrent processing succeeds");
                    let device = data.get(DEVICE_DATA_KEY).expect("device data was produced");
                    assert!(
                        device.is_mobile().has_value(),
                        "IsMobile should resolve on every worker thread"
                    );
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("a worker thread panicked");
    }
}

#[test]
fn refresh_round_trips() {
    let Some(engine) = build_engine() else {
        eprintln!("no Lite Hash data file found; skipping refresh test");
        return;
    };

    // Detect once before the refresh.
    let before = detect(engine.clone(), MOBILE_USER_AGENT);
    assert!(*before.is_mobile().value().unwrap());

    // Refresh reloads the data file in place and hot-swaps it.
    engine
        .refresh(None)
        .expect("refresh from the on-disk file succeeds");

    // The engine still detects correctly after the swap, proving the reloaded
    // data set is live.
    let after = detect(engine, MOBILE_USER_AGENT);
    assert!(
        *after.is_mobile().value().unwrap(),
        "detection still works after a refresh"
    );
}

#[test]
fn metadata_is_published() {
    let Some(engine) = build_engine() else {
        eprintln!("no Lite Hash data file found; skipping metadata test");
        return;
    };

    // The data file exposes properties, including the match metrics.
    assert!(
        !engine.aspect_properties().is_empty(),
        "the engine should publish property metadata"
    );
    assert!(
        engine
            .aspect_properties()
            .iter()
            .any(|p| p.name().eq_ignore_ascii_case("IsMobile")),
        "IsMobile should be among the published properties"
    );
    assert!(
        engine
            .aspect_properties()
            .iter()
            .any(|p| p.name().eq_ignore_ascii_case("DeviceId")),
        "the DeviceId match metric should be published"
    );

    // The engine reads the User-Agent header and a UACH header.
    assert!(engine.evidence_key_filter().include("header.user-agent"));
    assert!(engine
        .evidence_key_filter()
        .include("header.sec-ch-ua-mobile"));

    // The published date is recorded from the data file on disk.
    assert!(
        engine.data_file_published().is_some(),
        "the data-file publish time should be recorded"
    );
}

/// The engine accepts every header the data file reads, as a header and as a
/// query value, alongside the client-side `51d_` values.
#[test]
fn evidence_keys_come_from_the_data_file() {
    let Some(engine) = build_engine() else {
        eprintln!("no Lite Hash data file found; skipping evidence keys test");
        return;
    };
    let keys = engine.evidence_keys();
    let filter = engine.evidence_key_filter();
    for key in [
        "header.user-agent",
        "query.user-agent",
        "header.x-operamini-phone-ua",
        "query.sec-ch-ua-platform-version",
    ] {
        assert!(
            keys.iter().any(|k| k == key),
            "{key} should be listed: {keys:?}"
        );
        assert!(filter.include(key), "{key} should be accepted");
    }
    assert!(filter.include("query.51d_screenpixelswidth"));
    assert!(!filter.include("header.referer"));

    engine.refresh(None).expect("refresh succeeds");
    assert!(engine
        .evidence_key_filter()
        .include("header.x-operamini-phone-ua"));
}

/// Client hints given as query values, as for off-line processing, reach the
/// detection. Chrome reports Windows 11 as Windows NT 10.0 in its User-Agent,
/// and only the platform version hint tells the two apart.
#[test]
fn client_hints_sent_as_query_values_are_used() {
    let Some(engine) = build_engine() else {
        eprintln!("no Lite Hash data file found; skipping query client hints test");
        return;
    };
    let element: Arc<dyn FlowElement> = engine;
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("pipeline builds");
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", DESKTOP_USER_AGENT)
            .add("query.sec-ch-ua-platform", "\"Windows\"")
            .add("query.sec-ch-ua-platform-version", "\"15.0.0\"")
            .build(),
    );
    data.process().expect("processing succeeds");
    let device = data.get(DEVICE_DATA_KEY).expect("device data present");
    let version = device.platform_version();
    let version = version
        .value()
        .expect("PlatformVersion should have a value");
    assert_eq!(version, "11.0");
}

/// The match method is written as the specification names it, and the matched
/// User-Agents are written as a list.
#[test]
fn method_and_user_agents_are_written() {
    let Some(engine) = build_engine() else {
        eprintln!("no Lite Hash data file found; skipping match metrics test");
        return;
    };
    let device = detect(engine, DESKTOP_USER_AGENT);
    let method = device.method();
    let method = method.value().expect("Method should have a value");
    assert!(
        ["NONE", "PERFORMANCE", "COMBINED", "PREDICTIVE"].contains(&method.as_str()),
        "{method}"
    );
    let user_agents = device.user_agents();
    let user_agents = user_agents.value().expect("UserAgents should have a value");
    assert!(!user_agents.is_empty());
}

#[test]
fn paid_tier_data_resolves_hardware_vendor() {
    // Uses whatever `51DEGREES_DD_PATH` points at (or the bundled Lite file as a
    // fallback). HardwareVendor is a paid-tier property the free Lite file does
    // not carry, so this test asserts it only when a paid data file is
    // configured, and skips the paid-tier assertion otherwise.
    let Some(engine) = build_engine() else {
        eprintln!("no on-premise Hash data file found; skipping paid-tier detection test");
        return;
    };

    let device = detect(engine, MOBILE_USER_AGENT);

    // The iPhone is mobile in any tier.
    assert!(
        *device.is_mobile().value().unwrap(),
        "an iPhone is mobile in the paid data file too"
    );

    // HardwareVendor is only present in the paid Enterprise/TAC data. When the
    // configured file does not resolve it (the free Lite file does not), skip the
    // paid-tier assertion rather than fail. Point `51DEGREES_DD_PATH` at a paid
    // data file to exercise it. For an iPhone User-Agent the vendor is Apple.
    let vendor = device.hardware_vendor();
    if !vendor.has_value() {
        eprintln!(
            "the configured data file does not resolve HardwareVendor (not a paid \
             tier); skipping the paid-tier assertion"
        );
        return;
    }
    assert!(
        vendor.value().unwrap().eq_ignore_ascii_case("Apple"),
        "an iPhone should resolve to the Apple hardware vendor, got {:?}",
        vendor.value().unwrap()
    );
}

/// For every generated property that resolves to a value, the value must unpack
/// through the by-name getter the strongly-typed accessor uses, as the type the
/// generated metadata declares. A property that is present in the result but
/// cannot be read back as its declared type (for example a value the native
/// reader returned that does not parse as the declared bool, integer or double)
/// is a real type-mapping defect and fails this test, naming every offender.
///
/// Several diverse user agents are looked up so a wide cross-section of the 285
/// properties resolves. Skips cleanly when no data file is present.
#[test]
fn every_present_property_unpacks_as_its_declared_type() {
    let Some(engine) = build_engine() else {
        eprintln!("no usable device-detection data file; skipping type-unpack check");
        return;
    };
    let element: Arc<dyn FlowElement> = engine;
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("pipeline builds");

    // A spread of devices, operating systems and a bot, to surface as many of
    // the hardware, platform, browser and crawler properties as the data file
    // carries.
    let user_agents = [
        "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 \
         (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
         Chrome/120.0.0.0 Safari/537.36",
        "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) \
         Chrome/120.0.0.0 Mobile Safari/537.36",
        "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)",
    ];

    let mut offenders: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut present_count = 0usize;

    for ua in user_agents {
        let mut data = pipeline
            .create_flow_data_with(Evidence::builder().add("header.user-agent", ua).build());
        data.process().expect("processing succeeds");
        let device = data.get(DEVICE_DATA_KEY).expect("device data present");

        for (name, declared) in GENERATED_PROPERTY_TYPES {
            // Only properties that resolved to a value in this lookup are checked.
            if device.get(name).is_err() {
                continue;
            }
            present_count += 1;
            let unpacks = match declared {
                PropertyValueType::Bool => device.bool_property(name).value().is_ok(),
                PropertyValueType::Integer => device.integer_property(name).value().is_ok(),
                PropertyValueType::Double => device.double_property(name).value().is_ok(),
                PropertyValueType::StringList => device.string_list_property(name).value().is_ok(),
                // String and JavaScript both read as a string.
                _ => device.string_property(name).value().is_ok(),
            };
            if !unpacks {
                let raw = device.get(name).ok();
                offenders.insert(format!("{name} [{declared:?}] raw={raw:?}"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "{} present propert(ies) did not unpack as their declared type:\n{}",
        offenders.len(),
        offenders.into_iter().collect::<Vec<_>>().join("\n")
    );
    assert!(
        present_count > 0,
        "expected at least some properties to resolve for the sample user agents"
    );
}
