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

//! The on-premise IP Intelligence engine made from a configuration,
//! compared with what .NET's FiftyOne.IpIntelligence.Engine.OnPremise
//! 4.5.155 on pipeline 4.5.129 made from the same configuration with the
//! free ASN data file. Each expected answer and message is the one .NET
//! gave, apart from the differences the tests name. The tests need the ASN
//! file `51DEGREES_IPI_PATH` names, or the one at `cxx/data/ip-intelligence`,
//! and skip with a note without it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, ThreadId};

use fiftyone_ip_intelligence_onpremise::{
    configuration, IpIntelligenceData, IpIntelligenceOnPremiseEngine, IP_DATA_KEY,
};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::configuration::{BuilderRegistry, ElementOptions, PipelineOptions};
use fiftyone_pipeline_core::{Error, Evidence, Pipeline, PipelineBuilder, Result};
use fiftyone_pipeline_engines::{DataUpdateService, OnPremiseAspectEngine};

const BUILDER: &str = "IpiOnPremiseEngineBuilder";
const BUILDER_TYPE: &str =
    "fiftyone_ip_intelligence_onpremise::configuration::IpiOnPremiseEngineSettings";
const NO_SUB_ELEMENTS: &str = "No SubElements";
const NO_INNER: &str = "No Inner Exception.";
const IPV4: &str = "1.1.1.1";
const IPV6: &str = "2606:4700:4700::1111";
const DISTRIBUTOR: &str = "https://distributor.51degrees.com/api/v2/download";

fn asn_data_file() -> Option<PathBuf> {
    let given = std::env::var("51DEGREES_IPI_PATH").ok().map(PathBuf::from);
    let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi");
    given
        .into_iter()
        .chain([bundled])
        .find(|path| path.is_file())
}

/// The ASN file, or `None` with a note that the test is skipped.
fn asn() -> Option<String> {
    let found = asn_data_file().map(|path| path.to_string_lossy().into_owned());
    if found.is_none() {
        eprintln!("skipped: no ASN data file at 51DEGREES_IPI_PATH or cxx/data");
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
fn base(asn: &str, more: &[(&str, &str)]) -> ElementOptions {
    let mut parameters = vec![
        ("DataFile", asn),
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

/// The message .NET gives for the element that cannot be made.
fn failed(message: &str) -> String {
    format!(
        "Configuration failed to build.\nBuilderName: {BUILDER}\nSubElement Builder Names: \
         {NO_SUB_ELEMENTS}\n{message}\n{NO_INNER}\n"
    )
}

fn engine(pipeline: &Pipeline) -> Arc<IpIntelligenceOnPremiseEngine> {
    pipeline
        .resource::<IpIntelligenceOnPremiseEngine>()
        .expect("the IP Intelligence engine")
}

/// Asn and AsnName for `ip`, or the message of each that has no value.
fn look_up(
    pipeline: &Arc<Pipeline>,
    ip: &str,
) -> (
    std::result::Result<String, String>,
    std::result::Result<String, String>,
) {
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.client-ip-51d", ip).build());
    data.process().expect("processing");
    let found = data.get(IP_DATA_KEY).expect("the IP Intelligence data");
    (
        found.asn().value().cloned().map_err(|e| e.to_string()),
        found.asn_name().value().cloned().map_err(|e| e.to_string()),
    )
}

/// The answer .NET gave for both of Cloudflare's addresses.
fn cloudflare() -> (
    std::result::Result<String, String>,
    std::result::Result<String, String>,
) {
    (Ok("AS13335".to_string()), Ok("CLOUDFLARENET".to_string()))
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
        "fiftyone-ipi-configuration-{test}-{}",
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
fn the_engine_is_found_by_dotnets_names_and_looks_up() {
    let Some(asn) = asn() else { return };
    for name in [BUILDER, "IpiOnPremiseEngine", "ipionpremiseenginebuilder"] {
        let pipeline = build(element(
            name,
            &[
                ("DataFile", &asn),
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
        assert_eq!(keys, ["ip"], "{name}");
        assert_eq!(look_up(&pipeline, IPV4), cloudflare(), "{name}");
        assert_eq!(look_up(&pipeline, IPV6), cloudflare(), "{name}");
        let engine = engine(&pipeline);
        assert_eq!(engine.performance_profile(), PerformanceProfile::Default);
        assert_eq!(engine.concurrency(), None);
    }
}

/// Automatic updates and the file system watcher are on unless the
/// configuration turns them off, and .NET refused each update that was on
/// without an update service, naming them in this order.
#[test]
fn updates_need_an_update_service() {
    let Some(asn) = asn() else { return };
    let file = ("DataFile", asn.as_str());
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
    let Some(asn) = asn() else { return };
    let service = Arc::new(DataUpdateService::new());
    let pipeline = build_with_service(
        element(
            BUILDER,
            &[
                ("DataFile", &asn),
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
            "{DISTRIBUTOR}?LicenseKeys=KEY1&Download=True&Type=IPIV41"
        ))
    );
    assert_eq!(look_up(&pipeline, IPV4), cloudflare());
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
            ("DataFile", &asn),
            ("CreateTempDataCopy", "false"),
            ("DataUpdateLicenseKey", "KEY1"),
            ("DataFileSystemWatcher", "false"),
        ],
    );
    let missing_file = base("no-such-51Degrees.ipi", &[]);
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
        let mut parameters = vec![("DataFile", asn.as_str()), ("CreateTempDataCopy", "false")];
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
    let pipeline = build_with_service(base(&asn, &[]), &service).unwrap();
    assert!(!engine(&pipeline).data_files()[0].is_registered());
}

/// A configuration may say that the update source needs no license key, as
/// the specification allows, so automatic updates from a URL requested as
/// it is configured run with no key. .NET's engine builder has no such
/// setting and never asks for a key here.
#[test]
fn a_configuration_may_say_no_license_key_is_required() {
    let Some(asn) = asn() else { return };
    const SOURCE: &str = "https://updates.example.test/51Degrees-IPIV4AsnIpiV41.ipi";
    let service = Arc::new(DataUpdateService::new());
    let updated = |required: Option<&str>| {
        let mut parameters = vec![
            ("DataFile", asn.as_str()),
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
    let Some(asn) = asn() else { return };
    let made = |more: &[(&str, &str)]| {
        let pipeline = build(base(&asn, more)).expect("built");
        let configuration = engine(&pipeline).data_files()[0].configuration().clone();
        let url = engine(&pipeline).data_files()[0].formatted_update_url();
        (configuration, url)
    };

    let (defaults, url) = made(&[]);
    assert_eq!(defaults.data_update_url.as_deref(), Some(DISTRIBUTOR));
    assert_eq!(
        url.as_deref(),
        Some(&*format!(
            "{DISTRIBUTOR}?LicenseKeys=&Download=True&Type=IPIV41"
        ))
    );
    assert!(!defaults.automatic_updates_enabled && !defaults.file_system_watcher_enabled);
    assert!(defaults.verify_md5 && defaults.decompress_content);
    assert!(defaults.verify_modified_since && !defaults.update_on_startup);
    assert_eq!(defaults.polling_interval_seconds, 1800);
    assert_eq!(defaults.max_randomisation_seconds, 600);
    assert!(defaults.data_update_license_keys.is_empty());
    assert_eq!(defaults.data_file_path.as_deref(), Some(Path::new(&asn)));

    let (configuration, url) = made(&[("DataUpdateUrl", "https://example.com/data")]);
    assert_eq!(
        configuration.data_update_url.as_deref(),
        Some("https://example.com/data")
    );
    assert_eq!(
        url.as_deref(),
        Some("https://example.com/data?LicenseKeys=&Download=True&Type=IPIV41")
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
    for (value, seconds) in [("60", 60), ("00:02:00", 120)] {
        assert_eq!(
            made(&[("UpdatePollingInterval", value)])
                .0
                .polling_interval_seconds,
            seconds,
            "{value}"
        );
    }
    assert_eq!(
        made(&[("UpdateRandomisationMax", "30")])
            .0
            .max_randomisation_seconds,
        30
    );

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
            "{DISTRIBUTOR}?LicenseKeys=KEY1|KEY2&Download=True&Type=IPIV41"
        ))
    );
}

/// .NET read each performance profile by its exact name or number, refused
/// any other with these messages, and gave a list given in code to the text
/// overload, whose call refused it.
#[test]
fn performance_profiles_are_read_as_dotnet_reads_them() {
    let Some(asn) = asn() else { return };
    for (name, profile) in [
        ("LowMemory", PerformanceProfile::LowMemory),
        ("MaxPerformance", PerformanceProfile::InMemory),
        ("HighPerformance", PerformanceProfile::HighPerformance),
        ("Balanced", PerformanceProfile::Balanced),
        ("BalancedTemp", PerformanceProfile::Balanced),
        (" HighPerformance ", PerformanceProfile::HighPerformance),
        ("0", PerformanceProfile::LowMemory),
        ("LowMemory,Balanced", PerformanceProfile::Balanced),
    ] {
        let pipeline = build(base(&asn, &[("PerformanceProfile", name)])).expect(name);
        assert_eq!(engine(&pipeline).performance_profile(), profile, "{name}");
        assert_eq!(look_up(&pipeline, IPV4), cloudflare(), "{name}");
    }
    assert_eq!(
        message(build(base(
            &asn,
            &[("PerformanceProfile", "maxperformance")]
        ))),
        failed(
            "'maxperformance' is not a valid performance profile. Available profiles are \
             'LowMemory', 'MaxPerformance', 'HighPerformance', 'Balanced', 'BalancedTemp'."
        )
    );
    for (text, number) in [("5", "5"), ("-1", "-1"), ("Balanced,BalancedTemp", "7")] {
        assert_eq!(
            message(build(base(&asn, &[("PerformanceProfile", text)]))),
            failed(&format!(
                "The performance profile '{number}' is not valid for a IpiOnPremiseEngine. \
                 (Parameter 'profile')"
            )),
            "{text}"
        );
    }
    assert_eq!(
        message(build(
            base(&asn, &[]).parameter("PerformanceProfile", vec!["LowMemory"])
        )),
        failed("Object of type 'System.String[]' cannot be converted to type 'System.String'.")
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
    let Some(asn) = asn() else { return };
    for parameters in [
        vec![("Concurrency", "4")],
        vec![("Concurrency", "4"), ("PerformanceProfile", "LowMemory")],
        vec![("PerformanceProfile", "LowMemory"), ("Concurrency", "4")],
    ] {
        let pipeline = build(base(&asn, &parameters)).expect("built");
        assert_eq!(engine(&pipeline).concurrency(), Some(4), "{parameters:?}");
        assert_eq!(look_up(&pipeline, IPV4), cloudflare(), "{parameters:?}");
    }
    let pipeline = build(base(&asn, &[("Concurrency", "0")])).unwrap();
    assert_eq!(engine(&pipeline).concurrency(), Some(0));
}

/// .NET restricted the engine to the properties named, ignoring case, so
/// another property has no value, and refused a property the data file
/// does not hold. Both give the property left out the data upgrade reason,
/// where the Hash engine gives the reason for a property the configuration
/// left out.
#[test]
fn properties_restrict_the_engine() {
    let Some(asn) = asn() else { return };
    for parameters in [
        vec![("Property", "AsnName")],
        vec![("Properties", "AsnName")],
        vec![("Property", "asnname")],
    ] {
        let pipeline = build(base(&asn, &parameters)).unwrap();
        let (number, name) = look_up(&pipeline, IPV4);
        assert_eq!(name, Ok("CLOUDFLARENET".to_string()), "{parameters:?}");
        assert!(number.is_err(), "{parameters:?}");
    }
    let pipeline = build(base(&asn, &[]).parameter("Properties", vec!["AsnName"])).unwrap();
    assert!(look_up(&pipeline, IPV4).0.is_err());
    for parameters in [
        vec![("Properties", "Asn,AsnName")],
        vec![("Property", "Asn"), ("SetProperty", "AsnName")],
    ] {
        let pipeline = build(base(&asn, &parameters)).unwrap();
        assert_eq!(look_up(&pipeline, IPV4), cloudflare(), "{parameters:?}");
    }
    assert_eq!(
        message(build(base(&asn, &[("Property", "Bogus")]))),
        failed(&format!(
            "None of the properties requested could be found in the data file ('{asn}'), so \
             no properties can be initialised. To initialise all available properties, set \
             the field to null."
        ))
    );
}

/// .NET read a copy of the data file made in the temporary folder when
/// `CreateTempDataCopy` was true, under every profile, and removed it with
/// the engine. It made none with an empty temporary folder, or for
/// `BalancedTemp` alone. A refresh here reads a new copy in place of the
/// old.
#[test]
fn a_copy_of_the_data_file_is_read_when_asked() {
    let Some(asn) = asn() else { return };
    let folder = empty_folder("copy");
    let temp = folder.to_string_lossy().into_owned();
    for profile in ["LowMemory", "MaxPerformance", "BalancedTemp"] {
        let pipeline = build(element(
            BUILDER,
            &[
                ("DataFile", &asn),
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
        assert_eq!(look_up(&pipeline, IPV4), cloudflare(), "{profile}");
        engine(&pipeline).refresh(None).expect(profile);
        let refreshed = files_in(&folder);
        assert_eq!(refreshed.len(), 1, "{profile}");
        assert_ne!(refreshed, copied, "{profile}");
        assert_eq!(look_up(&pipeline, IPV4), cloudflare(), "{profile}");
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
        let pipeline = build(base(&asn, &parameters)).unwrap();
        assert!(files_in(&folder).is_empty(), "{parameters:?}");
        drop(pipeline);
    }
    let pipeline = build(element(
        BUILDER,
        &[
            ("DataFile", &asn),
            ("CreateTempDataCopy", "true"),
            ("AutoUpdate", "false"),
            ("DataFileSystemWatcher", "false"),
            ("TempDirPath", ""),
        ],
    ))
    .unwrap();
    assert_eq!(look_up(&pipeline, IPV4), cloudflare());
    // No copy was made in the current folder, which an empty folder name
    // would give.
    let current = files_in(&std::env::current_dir().unwrap());
    assert!(
        !current.iter().any(|name| name.ends_with(".operational")),
        "{current:?}"
    );
    drop(pipeline);
    std::fs::remove_dir_all(&folder).unwrap();
}

/// .NET's two build methods, and the builder's own `Build()`, which .NET
/// calls when no build parameter is given.
#[test]
fn the_build_methods_are_dotnets() {
    let Some(asn) = asn() else { return };
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
            element(BUILDER, &[("CreateTempDataCopy", "false")])
                .parameter("DataFile", vec![asn.as_str()])
        )),
        failed("Object of type 'System.String[]' cannot be converted to type 'System.String'.")
    );
    assert_eq!(
        message(build(element(
            BUILDER,
            &[("DataFile", &asn), ("CreateTempDataCopy", "x")]
        ))),
        failed(&format!(
            "Method 'Build' on builder '{BUILDER_TYPE}' for element 0 expects a parameter of \
             type 'Boolean'. Failed to parse value 'x' using the static 'TryParse' method."
        ))
    );
}

/// .NET lists every setting and build method of the builder, giving
/// `RuntimeParameterInfo` as each type, where the crates give the type, and
/// lists `Build (Stream)` among the settings too, where the crates list it
/// as a build method only. The crates also list `SetLicenseKeyRequired`,
/// which .NET's builder lacks.
#[test]
fn an_unknown_setting_lists_the_settings_and_build_methods() {
    let Some(asn) = asn() else { return };
    let listed = [
        "SetReuseTempFile (Boolean)",
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
            vec![("DataFile", asn.as_str()), ("AutoUpdate", "false")],
            "DATAFILE",
        ),
        (
            vec![
                ("DataFile", asn.as_str()),
                ("CreateTempDataCopy", "false"),
                ("Unknown", "1"),
            ],
            "DATAFILE,CREATETEMPDATACOPY,UNKNOWN",
        ),
        (
            vec![("Data", "x"), ("DataFile", asn.as_str())],
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

/// The values .NET could not read, with .NET's messages.
#[test]
fn values_that_do_not_parse_are_refused() {
    let Some(asn) = asn() else { return };
    for (name, value, method, type_name) in [
        ("Concurrency", "-1", "SetConcurrency", "UInt16"),
        ("Concurrency", "65536", "SetConcurrency", "UInt16"),
        ("Concurrency", "x", "SetConcurrency", "UInt16"),
        ("ReuseTempFile", "x", "SetReuseTempFile", "Boolean"),
        ("CacheSize", "x", "SetCacheSize", "Int32"),
    ] {
        assert_eq!(
            message(build(base(&asn, &[(name, value)]))),
            failed(&format!(
                "Method '{method}' on builder '{BUILDER_TYPE}' for element 0 expects a \
                 parameter of type '{type_name}'. Failed to parse value '{value}' using the \
                 static 'TryParse' method."
            )),
            "{name} {value}"
        );
    }
    for (name, method, type_name) in [
        ("ReuseTempFile", "SetReuseTempFile", "Boolean"),
        ("Concurrency", "SetConcurrency", "UInt16"),
    ] {
        assert_eq!(
            message(build(base(&asn, &[]).parameter(name, vec!["4"]))),
            failed(&format!(
                "Method '{method}' on builder '{BUILDER_TYPE}' for element 0 expects a \
                 parameter of type '{type_name}' but supplied object is of type 'String[]'."
            )),
            "{name}"
        );
    }
}

/// Settings whose values only code can give, with .NET's messages.
#[test]
fn settings_taken_in_code_only_are_refused() {
    let Some(asn) = asn() else { return };
    for (name, method, type_name) in [
        ("LazyLoading", "SetLazyLoading", "LazyLoadingConfiguration"),
        ("Cache", "SetCache", "CacheConfiguration"),
        ("AddDataFile", "AddDataFile", "IDataFileConfiguration"),
    ] {
        assert_eq!(
            message(build(base(&asn, &[(name, "x")]))),
            failed(&format!(
                "Method '{method}' on builder '{BUILDER_TYPE}' for element 0 expects a \
                 parameter of type '{type_name}' but this type does not have the expected \
                 'TryParse' method (or has multiples that cannot be resolved)."
            )),
            "{name}"
        );
    }
}

/// .NET refused a results cache of any size, and update URLs its `Uri`
/// could not read, with these messages.
#[test]
fn values_the_settings_refuse() {
    let Some(asn) = asn() else { return };
    assert_eq!(
        message(build(base(&asn, &[("CacheSize", "10")]))),
        failed(
            "A results cache cannot be configured in the IP Intelligence engine. Its results \
             refer to native memory, which is not safe to share between requests."
        )
    );
    assert_eq!(
        message(build(base(&asn, &[("DataUpdateUrl", "")]))),
        failed("Invalid URI: The URI is empty.")
    );
    assert_eq!(
        message(build(base(&asn, &[("DataUpdateUrl", "not a url")]))),
        failed("Invalid URI: The format of the URI could not be determined.")
    );
}

/// The settings that have no effect here are read, and logged.
#[test]
fn settings_with_no_effect_are_logged() {
    let Some(asn) = asn() else { return };
    let warning = |setting: &str, why: &str| {
        (
            log::Level::Warn,
            format!(
                "The IpiOnPremiseEngine setting '{setting}' has no effect here and is ignored, \
                 {why}."
            ),
        )
    };
    for (name, value, logged) in [
        (
            "ReuseTempFile",
            "true",
            vec![warning(
                "SetReuseTempFile",
                "as the engine makes a new copy of the data file each time it needs one",
            )],
        ),
        ("ReuseTempFile", "false", vec![]),
        (
            "DataDownloadType",
            "IPIV41Other",
            vec![warning(
                "SetDataDownloadType",
                "as the download type is the IP Intelligence one",
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
    ] {
        let (pipeline, found) = logged_by(|| build(base(&asn, &[(name, value)])));
        let pipeline = pipeline.expect(name);
        assert_eq!(found, logged, "{name} {value}");
        // .NET's download type stays the IP Intelligence one.
        assert_eq!(
            engine(&pipeline).data_files()[0]
                .formatted_update_url()
                .as_deref(),
            Some(&*format!(
                "{DISTRIBUTOR}?LicenseKeys=&Download=True&Type=IPIV41"
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
        .join("51Degrees-IPIV4AsnIpiV41-missing.ipi");
    let found = message(build(element(
        BUILDER,
        &[
            ("DataFile", "51Degrees-IPIV4AsnIpiV41-missing.ipi"),
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
