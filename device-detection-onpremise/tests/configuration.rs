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

//! The on-premise Device Detection engine made from a configuration,
//! compared with what .NET's FiftyOne.DeviceDetection.Hash.Engine.OnPremise
//! 4.5.180 on pipeline 4.5.129 made from the same configuration with the
//! Lite data file. Each expected answer and message is the one .NET gave,
//! apart from the differences the tests name. The tests need the Lite file
//! `51DEGREES_DD_PATH` names, or the one at `cxx/data/device-detection`, and
//! skip with a note without it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, ThreadId};

use fiftyone_device_detection_onpremise::{configuration, DeviceDetectionOnPremiseEngine};
use fiftyone_device_detection_shared::{DeviceData, DEVICE_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::configuration::{BuilderRegistry, ElementOptions, PipelineOptions};
use fiftyone_pipeline_core::{Error, Evidence, Pipeline, PipelineBuilder, Result};
use fiftyone_pipeline_engines::{AspectEngine, DataUpdateService, OnPremiseAspectEngine};

const BUILDER: &str = "DeviceDetectionHashEngineBuilder";
const BUILDER_TYPE: &str =
    "fiftyone_device_detection_onpremise::configuration::DeviceDetectionHashEngineSettings";
const NO_SUB_ELEMENTS: &str = "No SubElements";
const NO_INNER: &str = "No Inner Exception.";
const DESKTOP: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like \
    Gecko) Chrome/120.0.0.0 Safari/537.36";
const MOBILE: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
    AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
const DISTRIBUTOR: &str = "https://distributor.51degrees.com/api/v2/download";

fn lite_data_file() -> Option<PathBuf> {
    let given = std::env::var("51DEGREES_DD_PATH").ok().map(PathBuf::from);
    let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cxx/data/device-detection/51Degrees-LiteV4.1.hash");
    given
        .into_iter()
        .chain([bundled])
        .find(|path| path.is_file())
}

/// The Lite file, or `None` with a note that the test is skipped.
fn lite() -> Option<String> {
    let found = lite_data_file().map(|path| path.to_string_lossy().into_owned());
    if found.is_none() {
        eprintln!("skipped: no Lite Hash data file at 51DEGREES_DD_PATH or cxx/data");
    }
    found
}

fn registry() -> BuilderRegistry {
    let mut registry = BuilderRegistry::new();
    configuration::add_builders(&mut registry);
    registry
}

/// The engine named `builder` with `parameters`, in the order given.
fn element(builder: &str, parameters: &[(&str, &str)]) -> ElementOptions {
    parameters
        .iter()
        .fold(ElementOptions::new(builder), |element, (name, value)| {
            element.parameter(*name, *value)
        })
}

/// The data file, no copy and no updates, as .NET's base case had them,
/// then `more`.
fn base(lite: &str, more: &[(&str, &str)]) -> ElementOptions {
    let mut parameters = vec![
        ("DataFile", lite),
        ("CreateTempDataCopy", "false"),
        ("AutoUpdate", "false"),
        ("DataFileSystemWatcher", "false"),
    ];
    parameters.extend_from_slice(more);
    element(BUILDER, &parameters)
}

fn build(element: ElementOptions) -> Result<Arc<Pipeline>> {
    PipelineBuilder::new()
        .build_from_configuration(&registry(), &PipelineOptions::new().element(element))
}

/// The pipeline with `service` given as a resource.
fn build_with_service(
    element: ElementOptions,
    service: &Arc<DataUpdateService>,
) -> Result<Arc<Pipeline>> {
    PipelineBuilder::new()
        .add_resource(Arc::clone(service) as _)
        .build_from_configuration(&registry(), &PipelineOptions::new().element(element))
}

fn message(result: Result<Arc<Pipeline>>) -> String {
    match result {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

/// The message .NET gives for the element `builder` that cannot be made.
fn failed_as(builder: &str, message: &str) -> String {
    format!(
        "Configuration failed to build.\nBuilderName: {builder}\nSubElement Builder Names: \
         {NO_SUB_ELEMENTS}\n{message}\n{NO_INNER}\n"
    )
}

fn failed(message: &str) -> String {
    failed_as(BUILDER, message)
}

fn engine(pipeline: &Pipeline) -> Arc<DeviceDetectionOnPremiseEngine> {
    pipeline
        .resource::<DeviceDetectionOnPremiseEngine>()
        .expect("the Device Detection engine")
}

/// IsMobile and PlatformName for `user_agent`, or the message of each that
/// has no value.
fn detect(
    pipeline: &Arc<Pipeline>,
    user_agent: &str,
) -> (
    std::result::Result<bool, String>,
    std::result::Result<String, String>,
) {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", user_agent)
            .build(),
    );
    data.process().expect("processing");
    let device = data.get(DEVICE_DATA_KEY).expect("the device data");
    (
        device
            .is_mobile()
            .value()
            .copied()
            .map_err(|e| e.to_string()),
        device
            .platform_name()
            .value()
            .cloned()
            .map_err(|e| e.to_string()),
    )
}

/// The update service message, naming `features`.
fn needs_service(features: &str) -> String {
    format!(
        "Some enabled features ({features}) require an IDataUpdateService but one has not been \
         supplied. This can be corrected by passing an IDataUpdateService instance to the \
         engine builder constructor. If building from configuration, an IServiceProvider \
         instance, able to resolve an IDataUpdateService, must be passed to the pipeline \
         builder."
    )
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A new, empty folder for one test.
fn empty_folder(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "fiftyone-dd-configuration-{test}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct CapturingLogger {
    entries: Mutex<Vec<(ThreadId, log::Level, String)>>,
}

impl log::Log for CapturingLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            self.entries.lock().unwrap().push((
                thread::current().id(),
                record.level(),
                record.args().to_string(),
            ));
        }
    }

    fn flush(&self) {}
}

fn logger() -> &'static CapturingLogger {
    static LOGGER: OnceLock<&'static CapturingLogger> = OnceLock::new();
    LOGGER.get_or_init(|| {
        let logger: &'static CapturingLogger = Box::leak(Box::new(CapturingLogger {
            entries: Mutex::new(Vec::new()),
        }));
        log::set_logger(logger).expect("no other logger is installed in this binary");
        log::set_max_level(log::LevelFilter::Warn);
        logger
    })
}

/// The warnings and errors logged on the current thread while `run` runs.
/// The tests in this binary run in parallel, so each reads only its own.
fn logged_by<T>(run: impl FnOnce() -> T) -> (T, Vec<(log::Level, String)>) {
    let me = thread::current().id();
    let mine = || {
        logger()
            .entries
            .lock()
            .unwrap()
            .iter()
            .filter(|(id, _, _)| *id == me)
            .map(|(_, level, message)| (*level, message.clone()))
            .collect::<Vec<_>>()
    };
    let before = mine().len();
    let result = run();
    (result, mine().split_off(before))
}

/// .NET found the builder by its name, by its name without `Builder` and by
/// its name in lower case, and the engine gave these answers.
#[test]
fn the_engine_is_found_by_dotnets_names_and_detects() {
    let Some(lite) = lite() else { return };
    for name in [
        BUILDER,
        "DeviceDetectionHashEngine",
        "devicedetectionhashenginebuilder",
    ] {
        let pipeline = build(element(
            name,
            &[
                ("DataFile", &lite),
                ("CreateTempDataCopy", "false"),
                ("AutoUpdate", "false"),
                ("DataFileSystemWatcher", "false"),
            ],
        ))
        .expect(name);
        let keys: Vec<&str> = pipeline
            .flow_elements()
            .iter()
            .map(|e| e.data_key())
            .collect();
        assert_eq!(keys, ["device"], "{name}");
        assert_eq!(
            detect(&pipeline, DESKTOP),
            (Ok(false), Ok("Windows".to_string()))
        );
        assert_eq!(detect(&pipeline, MOBILE), (Ok(true), Ok("iOS".to_string())));
        let engine = engine(&pipeline);
        assert_eq!(engine.performance_profile(), PerformanceProfile::Default);
        assert_eq!(engine.data_source_tier(), "Lite");
    }
}

/// Automatic updates and the file system watcher are on unless the
/// configuration turns them off, and .NET refused each update that was on
/// without an update service, naming them in this order.
#[test]
fn updates_need_an_update_service() {
    let Some(lite) = lite() else { return };
    let file = ("DataFile", lite.as_str());
    let no_copy = ("CreateTempDataCopy", "false");
    for (parameters, features) in [
        (vec![file, no_copy], "auto update, file system watcher"),
        (
            vec![file, no_copy, ("AutoUpdate", "false")],
            "file system watcher",
        ),
        (
            vec![file, no_copy, ("DataFileSystemWatcher", "false")],
            "auto update",
        ),
        (
            vec![
                file,
                no_copy,
                ("AutoUpdate", "false"),
                ("DataFileSystemWatcher", "false"),
                ("DataUpdateOnStartup", "true"),
            ],
            "update on startup",
        ),
        (
            vec![file, no_copy, ("DataUpdateOnStartup", "true")],
            "auto update, update on startup, file system watcher",
        ),
    ] {
        assert_eq!(
            message(build(element(BUILDER, &parameters))),
            failed(&needs_service(features)),
            "{features}"
        );
    }
}

/// With an update service, .NET registered the data file with it. .NET
/// built with automatic updates on and no license key too, where the crates
/// refuse, because the specification's defaults require a key before an
/// engine builds with automatic updates or update on startup on. The
/// message is .NET's for that check, which its configuration never makes.
#[test]
fn updates_with_an_update_service_register_the_data_file() {
    let Some(lite) = lite() else { return };
    let service = Arc::new(DataUpdateService::new());
    let pipeline = build_with_service(
        element(
            BUILDER,
            &[
                ("DataFile", &lite),
                ("CreateTempDataCopy", "false"),
                ("DataUpdateLicenseKey", "KEY1"),
                ("DataFileSystemWatcher", "false"),
            ],
        ),
        &service,
    )
    .unwrap();
    let made = engine(&pipeline);
    let file = &made.data_files()[0];
    assert!(file.is_registered());
    assert!(file.configuration().automatic_updates_enabled);
    assert_eq!(file.configuration().data_update_license_keys, ["KEY1"]);
    assert_eq!(
        file.formatted_update_url().as_deref(),
        Some(&*format!(
            "{DISTRIBUTOR}?LicenseKeys=KEY1&Download=True&Type=HashV41"
        ))
    );
    assert_eq!(service.registered_count(), 1);
    drop(made);
    drop(pipeline);

    // Dropping the pipeline, or failing to build it after the engine was
    // made, releases the engine and ends its registration, as a .NET engine
    // unregisters its data file when it is disposed.
    assert_eq!(service.registered_count(), 0);
    let updated = element(
        BUILDER,
        &[
            ("DataFile", &lite),
            ("CreateTempDataCopy", "false"),
            ("DataUpdateLicenseKey", "KEY1"),
            ("DataFileSystemWatcher", "false"),
        ],
    );
    let missing_file = base("no-such-51Degrees.hash", &[]);
    for second in [ElementOptions::new("NoSuchBuilder"), missing_file] {
        let options = PipelineOptions::new()
            .element(updated.clone())
            .element(second);
        let result = PipelineBuilder::new()
            .add_resource(Arc::clone(&service) as _)
            .build_from_configuration(&registry(), &options);
        assert!(result.is_err());
        assert_eq!(service.registered_count(), 0);
    }

    for key in [None, Some(""), Some(" ")] {
        let mut parameters = vec![("DataFile", lite.as_str()), ("CreateTempDataCopy", "false")];
        if let Some(key) = key {
            parameters.push(("DataUpdateLicenseKey", key));
        }
        assert_eq!(
            message(build_with_service(element(BUILDER, &parameters), &service)),
            failed(
                "Some enabled features (auto update) require a license key but one has not \
                 been supplied. This can be corrected by passing a license key to the engine \
                 builder. If building from configuration, check the associated builder \
                 implementation to determine the correct parameter name to use."
            ),
            "{key:?}"
        );
    }

    // Updates off, the data file is not registered, as in .NET.
    let pipeline = build_with_service(base(&lite, &[]), &service).unwrap();
    assert!(!engine(&pipeline).data_files()[0].is_registered());
}

/// A configuration may say that the update source needs no license key, as
/// the specification allows, so automatic updates from a URL requested as
/// it is configured run with no key. .NET's engine builder has no such
/// setting and never asks for a key here.
#[test]
fn a_configuration_may_say_no_license_key_is_required() {
    let Some(lite) = lite() else { return };
    const SOURCE: &str = "https://updates.example.test/51Degrees-LiteV4.1.hash";
    let service = Arc::new(DataUpdateService::new());
    let updated = |required: Option<&str>| {
        let mut parameters = vec![
            ("DataFile", lite.as_str()),
            ("CreateTempDataCopy", "false"),
            ("DataFileSystemWatcher", "false"),
            ("DataUpdateUrl", SOURCE),
            ("DataUpdateUseUrlFormatter", "false"),
        ];
        if let Some(required) = required {
            parameters.push(("LicenseKeyRequired", required));
        }
        element(BUILDER, &parameters)
    };

    let pipeline = build_with_service(updated(Some("false")), &service).unwrap();
    let made = engine(&pipeline);
    let file = &made.data_files()[0];
    assert!(file.is_registered());
    assert!(file.configuration().automatic_updates_enabled);
    assert!(file.configuration().data_update_license_keys.is_empty());
    assert!(!file.configuration().license_key_required_for_updates);
    assert_eq!(file.formatted_update_url().as_deref(), Some(SOURCE));
    assert_eq!(service.registered_count(), 1);
    drop(made);
    drop(pipeline);

    for required in [None, Some("true")] {
        assert_eq!(
            message(build_with_service(updated(required), &service)),
            failed(
                "Some enabled features (auto update) require a license key but one has not \
                 been supplied. This can be corrected by passing a license key to the engine \
                 builder. If building from configuration, check the associated builder \
                 implementation to determine the correct parameter name to use."
            ),
            "{required:?}"
        );
    }
    assert_eq!(service.registered_count(), 0);
}

/// Each data file setting reads back as .NET read it back.
#[test]
fn the_data_file_settings_read_back_as_dotnet_read_them() {
    let Some(lite) = lite() else { return };
    let made = |more: &[(&str, &str)]| {
        let pipeline = build(base(&lite, more)).expect("built");
        let configuration = engine(&pipeline).data_files()[0].configuration().clone();
        let url = engine(&pipeline).data_files()[0].formatted_update_url();
        (configuration, url)
    };

    let (defaults, url) = made(&[]);
    assert_eq!(defaults.data_update_url.as_deref(), Some(DISTRIBUTOR));
    assert_eq!(
        url.as_deref(),
        Some(&*format!(
            "{DISTRIBUTOR}?LicenseKeys=&Download=True&Type=HashV41"
        ))
    );
    assert!(defaults.verify_md5 && defaults.decompress_content);
    assert!(defaults.verify_modified_since && !defaults.update_on_startup);
    assert_eq!(defaults.polling_interval_seconds, 1800);
    assert_eq!(defaults.max_randomisation_seconds, 600);
    assert!(defaults.data_update_license_keys.is_empty());
    assert_eq!(defaults.data_file_path.as_deref(), Some(Path::new(&lite)));

    let (configuration, url) = made(&[("DataUpdateUrl", "https://example.com/data")]);
    assert_eq!(
        configuration.data_update_url.as_deref(),
        Some("https://example.com/data")
    );
    assert_eq!(
        url.as_deref(),
        Some("https://example.com/data?LicenseKeys=&Download=True&Type=HashV41")
    );
    let (_, url) = made(&[("DataUpdateUseUrlFormatter", "false")]);
    assert_eq!(url.as_deref(), Some(DISTRIBUTOR));
    assert!(!made(&[("DataUpdateVerifyMd5", "false")]).0.verify_md5);
    assert!(
        !made(&[("DataUpdateDecompress", "false")])
            .0
            .decompress_content
    );
    assert!(
        !made(&[("VerifyIfModifiedSince", "false")])
            .0
            .verify_modified_since
    );
    assert!(
        !made(&[("DataUpdateOnStartup", "false")])
            .0
            .update_on_startup
    );
    for (value, seconds) in [
        ("60", 60),
        ("00:02:00", 120),
        ("24855.03:14:07", 2147483647),
    ] {
        assert_eq!(
            made(&[("UpdatePollingInterval", value)])
                .0
                .polling_interval_seconds,
            seconds,
            "{value}"
        );
    }
    for (value, seconds) in [("30", 30), ("1.00:00:00", 86400)] {
        assert_eq!(
            made(&[("UpdateRandomisationMax", value)])
                .0
                .max_randomisation_seconds,
            seconds,
            "{value}"
        );
    }

    // The crates' URL formatter joins the keys with a plain `|`, where .NET
    // gives the URL with it escaped as `%7C`.
    let (configuration, url) = made(&[
        ("DataUpdateLicenseKey", "KEY1"),
        ("SetDataUpdateLicenseKey", "KEY2"),
    ]);
    assert_eq!(configuration.data_update_license_keys, ["KEY1", "KEY2"]);
    assert_eq!(
        url.as_deref(),
        Some(&*format!(
            "{DISTRIBUTOR}?LicenseKeys=KEY1|KEY2&Download=True&Type=HashV41"
        ))
    );
    assert_eq!(
        made(&[("DataUpdateLicenseKeys", "A,B")])
            .0
            .data_update_license_keys,
        ["A", "B"]
    );
    let pipeline =
        build(base(&lite, &[]).parameter("DataUpdateLicenseKeys", vec!["A", "B"])).unwrap();
    assert_eq!(
        engine(&pipeline).data_files()[0]
            .configuration()
            .data_update_license_keys,
        ["A", "B"]
    );
}

/// .NET read each performance profile by its exact name or number, refused
/// any other with these messages, and gave a list given in code to the text
/// overload, whose call refused it.
#[test]
fn performance_profiles_are_read_as_dotnet_reads_them() {
    let Some(lite) = lite() else { return };
    for (name, profile) in [
        ("LowMemory", PerformanceProfile::LowMemory),
        ("MaxPerformance", PerformanceProfile::InMemory),
        ("HighPerformance", PerformanceProfile::HighPerformance),
        ("Balanced", PerformanceProfile::Balanced),
        ("BalancedTemp", PerformanceProfile::Balanced),
        ("0", PerformanceProfile::LowMemory),
    ] {
        let pipeline = build(base(&lite, &[("PerformanceProfile", name)])).expect(name);
        assert_eq!(engine(&pipeline).performance_profile(), profile, "{name}");
        assert_eq!(
            detect(&pipeline, MOBILE),
            (Ok(true), Ok("iOS".to_string())),
            "{name}"
        );
    }
    assert_eq!(
        message(build(base(
            &lite,
            &[("PerformanceProfile", "maxperformance")]
        ))),
        failed(
            "'maxperformance' is not a valid performance profile. Available profiles are \
             'LowMemory', 'MaxPerformance', 'HighPerformance', 'Balanced', 'BalancedTemp'."
        )
    );
    assert_eq!(
        message(build(base(&lite, &[("PerformanceProfile", "5")]))),
        failed(
            "The performance profile '5' is not valid for a DeviceDetectionHashEngine. \
             (Parameter 'profile')"
        )
    );
    assert_eq!(
        message(build(
            base(&lite, &[]).parameter("PerformanceProfile", vec!["LowMemory"])
        )),
        failed("Object of type 'System.String[]' cannot be converted to type 'System.String'.")
    );
}

/// .NET gave each matching setting to its native configuration, negative
/// values included, and its engine detected with them. With neither graph,
/// the C library refused the configuration in both.
#[test]
fn the_matching_settings_reach_the_engine() {
    let Some(lite) = lite() else { return };
    let pipeline = build(base(
        &lite,
        &[
            ("Difference", "10"),
            ("Drift", "10"),
            ("AllowUnmatched", "true"),
            ("UpdateMatchedUserAgent", "false"),
            ("UsePerformanceGraph", "true"),
            ("UsePredictiveGraph", "false"),
        ],
    ))
    .unwrap();
    let options = engine(&pipeline).hash_options().clone();
    assert_eq!(
        (
            options.difference,
            options.drift,
            options.allow_unmatched,
            options.update_matched_user_agent,
            options.use_performance_graph,
            options.use_predictive_graph,
        ),
        (
            Some(10),
            Some(10),
            Some(true),
            Some(false),
            Some(true),
            Some(false)
        )
    );
    assert_eq!(detect(&pipeline, MOBILE), (Ok(true), Ok("iOS".to_string())));
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", DESKTOP)
            .build(),
    );
    data.process().unwrap();
    let device = data.get(DEVICE_DATA_KEY).unwrap();
    assert_eq!(device.method().value().unwrap(), "PERFORMANCE");
    assert_eq!(device.user_agents().value().unwrap(), &Vec::<String>::new());

    let pipeline = build(base(
        &lite,
        &[("Drift", "-3"), ("Difference", "-2147483648")],
    ))
    .unwrap();
    let options = engine(&pipeline).hash_options().clone();
    assert_eq!(
        (options.drift, options.difference),
        (Some(-3), Some(i32::MIN))
    );

    let refused = message(build(base(&lite, &[("UsePredictiveGraph", "false")])));
    assert!(
        refused.contains("The configuration provided was not valid"),
        "{refused}"
    );
}

/// The concurrency reaches the engine. .NET loses it when a performance
/// profile is set too, in either order in a configuration file, because the
/// keys are read in name order and the profile replaces the native
/// configuration's concurrency, so the engine opens with the profile's own.
/// The crates keep the concurrency the configuration names, as the
/// specification's separate Concurrency option describes.
#[test]
fn the_concurrency_reaches_the_engine_whatever_the_profile() {
    let Some(lite) = lite() else { return };
    for parameters in [
        vec![("Concurrency", "4")],
        vec![("Concurrency", "4"), ("PerformanceProfile", "LowMemory")],
        vec![("PerformanceProfile", "LowMemory"), ("Concurrency", "4")],
    ] {
        let pipeline = build(base(&lite, &parameters)).expect("built");
        assert_eq!(engine(&pipeline).concurrency(), 4, "{parameters:?}");
        assert_eq!(
            detect(&pipeline, MOBILE),
            (Ok(true), Ok("iOS".to_string())),
            "{parameters:?}"
        );
    }
}

/// .NET restricted the engine to the properties named, so another property
/// is missing, and refused a property the data file does not hold.
#[test]
fn properties_restrict_the_engine() {
    let Some(lite) = lite() else { return };
    for parameters in [
        vec![("Property", "IsMobile")],
        vec![("Properties", "IsMobile")],
    ] {
        let pipeline = build(base(&lite, &parameters)).unwrap();
        let (is_mobile, platform) = detect(&pipeline, MOBILE);
        assert_eq!(is_mobile, Ok(true));
        let missing = platform.unwrap_err();
        assert!(
            missing.contains("excluded when configuring the engine"),
            "{missing}"
        );
    }
    let pipeline = build(base(&lite, &[]).parameter("Properties", vec!["IsMobile"])).unwrap();
    assert!(detect(&pipeline, MOBILE).1.is_err());
    for parameters in [
        vec![("Properties", "IsMobile,PlatformName")],
        vec![("Property", "IsMobile"), ("SetProperty", "PlatformName")],
    ] {
        let pipeline = build(base(&lite, &parameters)).unwrap();
        assert_eq!(detect(&pipeline, MOBILE), (Ok(true), Ok("iOS".to_string())));
    }
    assert_eq!(
        message(build(base(&lite, &[("Property", "Bogus")]))),
        failed(&format!(
            "None of the properties requested could be found in the data file ('{lite}'), so \
             no properties can be initialised. To initialise all available properties, set \
             the field to null."
        ))
    );
}

/// .NET read a copy of the data file made in the temporary folder when
/// `CreateTempDataCopy` was true, under every profile, and removed it with
/// the engine. It made none with an empty temporary folder, or for
/// `BalancedTemp`. A refresh here reads a new copy in place of the old.
#[test]
fn a_copy_of_the_data_file_is_read_when_asked() {
    let Some(lite) = lite() else { return };
    let folder = empty_folder("copy");
    let temp = folder.to_string_lossy().into_owned();
    for profile in ["LowMemory", "MaxPerformance"] {
        let pipeline = build(element(
            BUILDER,
            &[
                ("DataFile", &lite),
                ("CreateTempDataCopy", " TRUE "),
                ("AutoUpdate", "false"),
                ("DataFileSystemWatcher", "false"),
                ("TempDirPath", &temp),
                ("PerformanceProfile", profile),
            ],
        ))
        .expect(profile);
        let copied = files_in(&folder);
        assert_eq!(copied.len(), 1, "{profile}");
        assert_eq!(detect(&pipeline, MOBILE).0, Ok(true));
        engine(&pipeline).refresh(None).expect(profile);
        let refreshed = files_in(&folder);
        assert_eq!(refreshed.len(), 1, "{profile}");
        assert_ne!(refreshed, copied, "{profile}");
        assert_eq!(detect(&pipeline, MOBILE).0, Ok(true));
        drop(pipeline);
        assert!(files_in(&folder).is_empty(), "{profile}");
    }
    for parameters in [
        vec![("TempDirPath", temp.as_str())],
        vec![
            ("TempDirPath", temp.as_str()),
            ("PerformanceProfile", "BalancedTemp"),
        ],
    ] {
        let pipeline = build(base(&lite, &parameters)).unwrap();
        assert!(files_in(&folder).is_empty(), "{parameters:?}");
        drop(pipeline);
    }
    let pipeline = build(element(
        BUILDER,
        &[
            ("DataFile", &lite),
            ("CreateTempDataCopy", "true"),
            ("AutoUpdate", "false"),
            ("DataFileSystemWatcher", "false"),
            ("TempDirPath", ""),
        ],
    ))
    .unwrap();
    assert_eq!(detect(&pipeline, MOBILE).0, Ok(true));
    drop(pipeline);
    std::fs::remove_dir_all(&folder).unwrap();
}

/// .NET's two build methods, and the builder's own `Build()`, which .NET
/// calls when no build parameter is given.
#[test]
fn the_build_methods_are_dotnets() {
    let Some(lite) = lite() else { return };
    assert_eq!(
        message(build(element(BUILDER, &[]))),
        failed(
            "This builder requires one and only one data file to be configured but 0 data file \
             configurations that have been supplied."
        )
    );
    assert_eq!(
        message(build(element(BUILDER, &[("Data", "x")]))),
        failed(&format!(
            "Method 'Build' on builder '{BUILDER_TYPE}' for element 0 expects a parameter of \
             type 'Stream' but this type does not have the expected 'TryParse' method (or has \
             multiples that cannot be resolved)."
        ))
    );
    assert_eq!(
        message(build(
            element(BUILDER, &[("DataFile", &lite)]).parameter("CreateTempDataCopy", vec!["true"])
        )),
        failed("Unable to cast object of type 'System.String[]' to type 'System.String'.")
    );
    assert_eq!(
        message(build(element(
            BUILDER,
            &[("DataFile", &lite), ("CreateTempDataCopy", "x")]
        ))),
        failed(&format!(
            "Method 'Build' on builder '{BUILDER_TYPE}' for element 0 expects a parameter of \
             type 'Boolean'. Failed to parse value 'x' using the static 'TryParse' method."
        ))
    );
}

/// .NET lists every setting and build method of the builder, giving
/// `RuntimeParameterInfo` as each type, where the crates give the type. The
/// crates leave out .NET's `Build (Stream)`, which is a build method. The
/// crates also list `SetLicenseKeyRequired`, which .NET's builder lacks.
#[test]
fn an_unknown_setting_lists_the_settings_and_build_methods() {
    let Some(lite) = lite() else { return };
    let listed = [
        "SetDifference (Int32)",
        "SetDrift (Int32)",
        "SetAllowUnmatched (Boolean)",
        "SetReuseTempFile (Boolean)",
        "SetUpdateMatchedUserAgent (Boolean)",
        "SetUsePerformanceGraph (Boolean)",
        "SetUsePredictiveGraph (Boolean)",
        "SetPerformanceProfile (String)",
        "SetPerformanceProfile (PerformanceProfiles)",
        "SetConcurrency (UInt16)",
        "SetCache (CacheConfiguration)",
        "SetCacheSize (Int32)",
        "SetDataDownloadType (String)",
        "SetDataUpdateUrl (String)",
        "SetDataUpdateUrl (Uri)",
        "SetDataUpdateUrlFormatter (IDataUpdateUrlFormatter)",
        "SetDataUpdateUseUrlFormatter (IDataUpdateUrlFormatter)",
        "SetDataUpdateUseUrlFormatter (Boolean)",
        "SetDataUpdateVerifyMd5 (Boolean)",
        "SetDataUpdateDecompress (Boolean)",
        "SetAutoUpdate (Boolean)",
        "SetDataFileSystemWatcher (Boolean)",
        "SetUpdatePollingInterval (Int32)",
        "SetUpdatePollingInterval (TimeSpan)",
        "SetUpdateRandomisationMax (Int32)",
        "SetUpdateRandomisationMax (TimeSpan)",
        "SetVerifyIfModifiedSince (Boolean)",
        "SetDataUpdateLicenseKey (String)",
        "SetDataUpdateLicenseKeys (String[])",
        "SetLicenseKeyRequired (Boolean)",
        "SetDataUpdateOnStartup (Boolean)",
        "AddDataFile (IDataFileConfiguration)",
        "SetTempDirPath (String)",
        "SetLazyLoading (LazyLoadingConfiguration)",
        "SetLazyLoadingTimeout (Int32)",
        "SetCacheHitOrMiss (Boolean)",
        "SetProperties (List`1)",
        "SetProperty (String)",
    ]
    .iter()
    .map(|s| format!("{s}\n"))
    .collect::<String>();
    for (parameters, names) in [
        (
            vec![("DataFile", lite.as_str()), ("AutoUpdate", "false")],
            "DATAFILE",
        ),
        (
            vec![
                ("DataFile", lite.as_str()),
                ("CreateTempDataCopy", "false"),
                ("Unknown", "1"),
            ],
            "DATAFILE,CREATETEMPDATACOPY,UNKNOWN",
        ),
        (
            vec![
                ("DataFile", lite.as_str()),
                ("CreateTempDataCopy", "false"),
                ("UpdateOnStartup", "false"),
            ],
            "DATAFILE,CREATETEMPDATACOPY,UPDATEONSTARTUP",
        ),
        (
            vec![("Data", "x"), ("DataFile", lite.as_str())],
            "DATA,DATAFILE",
        ),
    ] {
        assert_eq!(
            message(build(element(BUILDER, &parameters))),
            failed(&format!(
                "Builder '{BUILDER_TYPE}' for element 0 has no 'Set' methods or 'Build' methods \
                 that match (case-insensitively) the following parameters: {names}. The \
                 available configuration methods on this builder are: \n{listed}The available \
                 'Build' methods have the following signatures: \ndatafile \
                 (String),createTempDataCopy (Boolean)\ndata (Stream)\n\n"
            )),
            "{names}"
        );
    }
}

/// The values .NET could not read, with .NET's messages, the last overload
/// tried naming its type.
#[test]
fn values_that_do_not_parse_are_refused() {
    let Some(lite) = lite() else { return };
    for (name, value, method, type_name) in [
        ("Concurrency", "-1", "SetConcurrency", "UInt16"),
        ("Concurrency", "65536", "SetConcurrency", "UInt16"),
        ("Concurrency", "x", "SetConcurrency", "UInt16"),
        ("CacheSize", "x", "SetCacheSize", "Int32"),
        ("Difference", "x", "SetDifference", "Int32"),
        ("Difference", "2147483648", "SetDifference", "Int32"),
        ("AllowUnmatched", "x", "SetAllowUnmatched", "Boolean"),
        (
            "DataUpdateUseUrlFormatter",
            "x",
            "SetDataUpdateUseUrlFormatter",
            "Boolean",
        ),
        (
            "UpdatePollingInterval",
            "x",
            "SetUpdatePollingInterval",
            "TimeSpan",
        ),
        (
            "UpdatePollingInterval",
            "2147483648",
            "SetUpdatePollingInterval",
            "TimeSpan",
        ),
    ] {
        assert_eq!(
            message(build(base(&lite, &[(name, value)]))),
            failed(&format!(
                "Method '{method}' on builder '{BUILDER_TYPE}' for element 0 expects a \
                 parameter of type '{type_name}'. Failed to parse value '{value}' using the \
                 static 'TryParse' method."
            )),
            "{name} {value}"
        );
    }
    assert_eq!(
        message(build(
            base(&lite, &[]).parameter("SetAutoUpdate", vec!["false"])
        )),
        failed(&format!(
            "Method 'SetAutoUpdate' on builder '{BUILDER_TYPE}' for element 0 expects a \
             parameter of type 'Boolean' but supplied object is of type 'String[]'."
        ))
    );
    for name in ["TempDirPath", "DataUpdateUrl"] {
        assert_eq!(
            message(build(base(&lite, &[]).parameter(name, vec!["x"]))),
            failed("Object of type 'System.String[]' cannot be converted to type 'System.String'."),
            "{name}"
        );
    }
}

/// Settings whose values only code can give, with .NET's messages.
#[test]
fn settings_taken_in_code_only_are_refused() {
    let Some(lite) = lite() else { return };
    for (name, method, type_name) in [
        ("LazyLoading", "SetLazyLoading", "LazyLoadingConfiguration"),
        ("Cache", "SetCache", "CacheConfiguration"),
        ("AddDataFile", "AddDataFile", "IDataFileConfiguration"),
        (
            "DataUpdateUrlFormatter",
            "SetDataUpdateUrlFormatter",
            "IDataUpdateUrlFormatter",
        ),
    ] {
        assert_eq!(
            message(build(base(&lite, &[(name, "x")]))),
            failed(&format!(
                "Method '{method}' on builder '{BUILDER_TYPE}' for element 0 expects a \
                 parameter of type '{type_name}' but this type does not have the expected \
                 'TryParse' method (or has multiples that cannot be resolved)."
            )),
            "{name}"
        );
    }
}

/// .NET refused a results cache of any size, update periods too long for
/// whole seconds in an `Int32`, and update URLs its `Uri` could not read,
/// with these messages.
#[test]
fn values_the_settings_refuse() {
    let Some(lite) = lite() else { return };
    assert_eq!(
        message(build(base(&lite, &[("CacheSize", "10")]))),
        failed(
            "A results cache cannot be configured in the Device-Detection Hash engine. The \
             overhead of having to manage native object lifetimes when a cache is enabled \
             outweighs the benefit of the cache."
        )
    );
    assert_eq!(
        message(build(base(
            &lite,
            &[("UpdatePollingInterval", "24855.03:14:08")]
        ))),
        failed(
            "Polling interval timespan too large. Must be less than 2147483647 seconds but is \
             2147483648 seconds (Parameter 'pollingInterval')"
        )
    );
    assert_eq!(
        message(build(base(
            &lite,
            &[("UpdateRandomisationMax", "24855.03:14:08")]
        ))),
        failed(
            "Update randomization timespan too large. Must be less than 2147483647 seconds but \
             is 2147483648 seconds (Parameter 'maximumDeviation')"
        )
    );
    assert_eq!(
        message(build(base(&lite, &[("DataUpdateUrl", "")]))),
        failed("Invalid URI: The URI is empty.")
    );
    for url in ["not a url", "example.com/data"] {
        assert_eq!(
            message(build(base(&lite, &[("DataUpdateUrl", url)]))),
            failed("Invalid URI: The format of the URI could not be determined."),
            "{url}"
        );
    }
}

/// .NET kept a negative polling interval or randomisation, and built. The
/// crates refuse one, because the update service counts whole seconds from
/// now.
#[test]
fn negative_update_periods_are_refused() {
    let Some(lite) = lite() else { return };
    for (name, value, seconds) in [
        ("UpdatePollingInterval", "-5", -5),
        ("UpdatePollingInterval", "-00:00:05", -5),
        ("UpdateRandomisationMax", "-5", -5),
    ] {
        assert_eq!(
            message(build(base(&lite, &[(name, value)]))),
            failed(&format!(
                "The setting 'Set{name}' must be 0 or more seconds, but is {seconds}."
            )),
            "{name} {value}"
        );
    }
}

/// The settings that have no effect here are read, and logged.
#[test]
fn settings_with_no_effect_are_logged() {
    let Some(lite) = lite() else { return };
    let warning = |setting: &str, why: &str| {
        (
            log::Level::Warn,
            format!(
                "The DeviceDetectionHashEngine setting '{setting}' has no effect here and is \
                 ignored, {why}."
            ),
        )
    };
    for (name, value, logged) in [
        (
            "DataDownloadType",
            "HashV41Other",
            vec![warning(
                "SetDataDownloadType",
                "as the data file names its own type",
            )],
        ),
        (
            "LazyLoadingTimeout",
            "100",
            vec![warning(
                "SetLazyLoadingTimeout",
                "as the engine has no lazy loading",
            )],
        ),
        (
            "CacheHitOrMiss",
            "true",
            vec![warning("SetCacheHitOrMiss", "as the engine has no cache")],
        ),
        ("CacheHitOrMiss", "false", vec![]),
        (
            "ReuseTempFile",
            "true",
            vec![warning(
                "SetReuseTempFile",
                "as the engine never reuses a copy of the data file it finds",
            )],
        ),
        ("ReuseTempFile", "false", vec![]),
    ] {
        let (pipeline, found) = logged_by(|| build(base(&lite, &[(name, value)])));
        let pipeline = pipeline.expect(name);
        assert_eq!(found, logged, "{name} {value}");
        // .NET's download type stays the data file's.
        assert_eq!(
            engine(&pipeline).data_files()[0]
                .formatted_update_url()
                .as_deref(),
            Some(&*format!(
                "{DISTRIBUTOR}?LicenseKeys=&Download=True&Type=HashV41"
            )),
            "{name}"
        );
    }
}

/// .NET read a relative data file path from the current folder, and named
/// the full path when the file was not there.
#[test]
fn a_relative_data_file_is_read_from_the_current_folder() {
    let missing = std::env::current_dir()
        .unwrap()
        .join("51Degrees-LiteV4.1-missing.hash");
    let found = message(build(element(
        BUILDER,
        &[
            ("DataFile", "51Degrees-LiteV4.1-missing.hash"),
            ("CreateTempDataCopy", "false"),
            ("AutoUpdate", "false"),
            ("DataFileSystemWatcher", "false"),
        ],
    )));
    assert_eq!(
        found,
        failed(&format!(
            "The data file '{}' could not be found. Check the file path and that the program \
             has sufficient read permissions.",
            missing.display()
        ))
    );
}
