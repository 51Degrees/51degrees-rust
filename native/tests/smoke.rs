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

//! End to end smoke tests of the safe API against real data files.
//!
//! Each product's smoke tests are compiled only when that product's feature is
//! enabled. The Device Detection block compiles under `dd` and the IP
//! Intelligence block under `ipi`. This crate's own dev build enables both (its
//! default features), so this binary links both engines at once. That used to
//! hit a common C library symbol collision, but IP Intelligence now compiles
//! its copy of the common C library into an `ipi_` prefixed namespace, so both
//! engines load real data files in the same binary. A data load failure is
//! therefore a real fault here, not a soft-skip.
//!
//! The [`both_engines_coexist`] test is the proof of that fix. It loads a Device
//! Detection Hash file and an IP Intelligence file in one process, runs a real
//! detection and a real lookup, and asserts a value from each. It panics if
//! either file is missing or either step errors, so a regression cannot pass
//! silently.
//!
//! The per-product tests below resolve their data file at run time and skip
//! cleanly only when the file is absent, so the suite still builds and runs on a
//! machine without the data files. When a file is present they load it for real
//! and assert on the result.

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

#[cfg(any(feature = "dd", feature = "ipi"))]
use std::path::PathBuf;

/// Search well known locations for a Lite data file with the given extension and
/// optional name fragment, returning the first that exists.
///
/// Resolution order, all relative to the Rust workspace root:
/// the matching `_51DEGREES_*` environment variable, then the
/// `cxx` submodule, then the parent
/// `Workspace` tree where the other 51Degrees products keep their data.
#[cfg(any(feature = "dd", feature = "ipi"))]
fn find_data_file(
    env_var: &str,
    cxx_dir: &str,
    data_dir: &str,
    file_name: &str,
) -> Option<PathBuf> {
    // 1. An explicit path from the environment wins.
    if let Ok(path) = crate::deprecated_names::var(env_var) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }

    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(PathBuf::from)?;

    // 2. A sibling cxx checkout, the layout the -sys build scripts expect.
    let sibling = workspace.join(cxx_dir).join(data_dir).join(file_name);
    if sibling.is_file() {
        return Some(sibling);
    }

    // 3. The wider Workspace tree, where a sibling cxx checkout sits next to the
    // Rust workspace rather than inside it (the usual local layout).
    if let Some(parent) = workspace.parent() {
        let candidate = parent.join(cxx_dir).join(data_dir).join(file_name);
        if candidate.is_file() {
            return Some(candidate);
        }
        // Some layouts keep the data directory directly under the Workspace tree.
        let flat = parent.join(data_dir).join(file_name);
        if flat.is_file() {
            return Some(flat);
        }
    }

    None
}

/// Locate a Lite Hash data file for the Device Detection smoke test.
#[cfg(feature = "dd")]
fn dd_lite_data_file() -> Option<PathBuf> {
    find_data_file(
        "_51DEGREES_DD_PATH",
        "cxx",
        "data/device-detection",
        "51Degrees-LiteV4.1.hash",
    )
}

/// Locate an IP Intelligence data file for the IP Intelligence smoke test.
///
/// Resolves the ASN file (`51Degrees-IPIV4AsnIpiV41.ipi`) from the data
/// repository, which loads against this source revision.
#[cfg(feature = "ipi")]
fn ipi_data_file() -> Option<PathBuf> {
    find_data_file(
        "_51DEGREES_IPI_PATH",
        "cxx",
        "data/ip-intelligence",
        "51Degrees-IPIV4AsnIpiV41.ipi",
    )
}

#[cfg(feature = "dd")]
mod device_detection {
    use super::dd_lite_data_file;
    use fiftyone_native::{dd, PerformanceProfile};
    use fiftyone_pipeline_core::{Error, Evidence};
    use std::sync::Arc;

    /// A representative desktop Chrome on Windows user agent.
    const DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
        AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

    /// A representative iPhone Safari user agent.
    const MOBILE_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)         AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";

    /// Open the Lite Hash data file through the safe API. Both products are
    /// linked in this binary, and they now coexist, so a load failure is a real
    /// fault rather than a skip. The data file presence is environmental, so the
    /// caller skips only when the file is absent. Once the file is found it must
    /// load, hence the hard `expect`.
    fn open_loaded(data_file: &std::path::Path) -> Arc<dd::Manager> {
        dd::Manager::open(data_file, PerformanceProfile::Default).unwrap_or_else(|err| {
            panic!("Device Detection Lite data file should load through the safe API: {err}")
        })
    }

    /// Open a Lite Hash data file, run a detection from a desktop user agent and
    /// confirm IsMobile reads back as the expected value through the safe API.
    #[test]
    fn detect_is_mobile_via_safe_api() {
        let Some(data_file) = dd_lite_data_file() else {
            eprintln!("no Lite Hash data file found; skipping Device Detection smoke test");
            return;
        };
        let manager = open_loaded(&data_file);

        assert!(
            manager.property_count() > 0,
            "the Lite data set should expose properties"
        );
        assert!(
            manager.required_property_index("IsMobile").is_some(),
            "IsMobile should be an available property"
        );

        let mut results = manager.create_results().expect("results should allocate");
        results
            .process_user_agent(DESKTOP_USER_AGENT)
            .expect("processing a user agent should not raise an exception");

        assert!(
            results
                .has_values("IsMobile")
                .expect("has_values should not error"),
            "IsMobile should have a value for a desktop user agent"
        );
        let value = results
            .value_as_string("IsMobile", ",")
            .expect("reading IsMobile should not error")
            .expect("IsMobile should produce a value");
        assert_eq!(value, "False", "a desktop user agent is not mobile");
    }

    /// The same detection driven from pipeline evidence rather than a bare
    /// string, exercising the evidence marshalling helper.
    #[test]
    fn detect_from_evidence_via_safe_api() {
        let Some(data_file) = dd_lite_data_file() else {
            eprintln!("no Lite Hash data file found; skipping Device Detection evidence test");
            return;
        };
        let manager = open_loaded(&data_file);
        let mut results = manager.create_results().expect("results should allocate");

        let evidence = Evidence::builder()
            .add("header.user-agent", DESKTOP_USER_AGENT)
            .build();
        results
            .process_evidence(&evidence)
            .expect("processing evidence should not raise an exception");

        let value = results
            .value_as_string("IsMobile", ",")
            .expect("reading IsMobile should not error");
        assert_eq!(value.as_deref(), Some("False"));
    }

    /// A query User-Agent is matched in preference to the header, which is how
    /// a caller supplies a User-Agent for off-line processing.
    #[test]
    fn query_user_agent_takes_precedence_over_the_header() {
        let Some(data_file) = dd_lite_data_file() else {
            eprintln!("no Lite Hash data file found; skipping query precedence test");
            return;
        };
        let manager = open_loaded(&data_file);
        let mut results = manager.create_results().expect("results should allocate");

        let evidence = Evidence::builder()
            .add("header.user-agent", DESKTOP_USER_AGENT)
            .add("query.user-agent", MOBILE_USER_AGENT)
            .build();
        results
            .process_evidence(&evidence)
            .expect("processing evidence should not raise an exception");

        let value = results
            .value_as_string("IsMobile", ",")
            .expect("reading IsMobile should not error");
        assert_eq!(value.as_deref(), Some("True"));
    }

    /// The longest header the default configuration uses, in bytes.
    const HEADER_LIMIT: usize = 500;

    /// The mobile User-Agent padded to `length` bytes. The padding does not
    /// change the match, so only the length can change the result.
    fn padded_user_agent(length: usize) -> String {
        format!("{MOBILE_USER_AGENT:x<length$}")
    }

    /// A header over the limit is ignored with no error, through the direct
    /// User-Agent call and through evidence alike.
    #[test]
    fn a_header_over_the_limit_is_ignored() {
        let Some(data_file) = dd_lite_data_file() else {
            eprintln!("no Lite Hash data file found; skipping header limit test");
            return;
        };
        let manager = open_loaded(&data_file);

        for (length, matched) in [(HEADER_LIMIT, true), (HEADER_LIMIT + 1, false)] {
            let user_agent = padded_user_agent(length);

            let mut direct = manager.create_results().expect("results should allocate");
            direct
                .process_user_agent(&user_agent)
                .expect("a long user agent should not raise an exception");

            let mut through_evidence = manager.create_results().expect("results should allocate");
            let evidence = Evidence::builder()
                .add("header.user-agent", user_agent.as_str())
                .build();
            through_evidence
                .process_evidence(&evidence)
                .expect("a long header should not raise an exception");

            for results in [&direct, &through_evidence] {
                let has_values = results
                    .has_values("IsMobile")
                    .expect("has_values should not error");
                assert_eq!(has_values, matched, "a header of {length} bytes");
            }
        }
    }

    /// Detection moves on from a header over the limit to the next evidence,
    /// so the header is used here even though a query value takes precedence.
    #[test]
    fn detection_moves_on_from_a_header_over_the_limit() {
        let Some(data_file) = dd_lite_data_file() else {
            eprintln!("no Lite Hash data file found; skipping next header test");
            return;
        };
        let manager = open_loaded(&data_file);
        let mut results = manager.create_results().expect("results should allocate");

        let over_limit = padded_user_agent(HEADER_LIMIT + 1);
        let evidence = Evidence::builder()
            .add("query.user-agent", over_limit.as_str())
            .add("header.user-agent", DESKTOP_USER_AGENT)
            .build();
        results
            .process_evidence(&evidence)
            .expect("processing evidence should not raise an exception");

        let value = results
            .value_as_string("IsMobile", ",")
            .expect("reading IsMobile should not error");
        assert_eq!(value.as_deref(), Some("False"));
    }

    /// The limit is on headers. A high-entropy values blob is not a header, so
    /// a longer blob is still turned into client hint headers.
    #[test]
    fn a_high_entropy_values_blob_over_the_limit_is_used() {
        let Some(data_file) = dd_lite_data_file() else {
            eprintln!("no Lite Hash data file found; skipping long blob test");
            return;
        };
        let manager = open_loaded(&data_file);
        // Base64 of the high-entropy values of Chrome on macOS, with brands
        // and a full version list.
        let ghev = "eyJhcmNoaXRlY3R1cmUiOiJhcm0iLCJicmFuZHMiOlt7ImJyYW5kIjoiR29vZ2xlIENocm9t\
            ZSIsInZlcnNpb24iOiIxMzEifSx7ImJyYW5kIjoiQ2hyb21pdW0iLCJ2ZXJzaW9uIjoiMTMx\
            In0seyJicmFuZCI6Ik5vdF9BIEJyYW5kIiwidmVyc2lvbiI6IjI0In1dLCJmdWxsVmVyc2lv\
            bkxpc3QiOlt7ImJyYW5kIjoiR29vZ2xlIENocm9tZSIsInZlcnNpb24iOiIxMzEuMC42Nzc4\
            LjE0MCJ9LHsiYnJhbmQiOiJDaHJvbWl1bSIsInZlcnNpb24iOiIxMzEuMC42Nzc4LjE0MCJ9\
            LHsiYnJhbmQiOiJOb3RfQSBCcmFuZCIsInZlcnNpb24iOiIyNC4wLjAuMCJ9XSwibW9iaWxl\
            IjpmYWxzZSwibW9kZWwiOiIiLCJwbGF0Zm9ybSI6Im1hY09TIiwicGxhdGZvcm1WZXJzaW9u\
            IjoiMTUuMS4xIn0=";
        assert!(ghev.len() > HEADER_LIMIT);

        for key in [
            "query.51d_gethighentropyvalues",
            "cookie.51d_gethighentropyvalues",
        ] {
            let mut results = manager.create_results().expect("results should allocate");
            let evidence = Evidence::builder().add(key, ghev).build();
            results
                .process_evidence(&evidence)
                .expect("processing evidence should not raise an exception");

            let value = results
                .value_as_string("PlatformName", ",")
                .expect("reading PlatformName should not error");
            assert_eq!(value.as_deref(), Some("macOS"), "{key}");
        }
    }

    /// With the default settings a User-Agent over the limit, as the only
    /// evidence, gives the device id `0-0-0-0`, which shows it was ignored,
    /// and the same User-Agent cut to the limit gives the device id of the
    /// User-Agent itself. With unmatched results allowed, or with other
    /// evidence that still matches, the device id is not all zeros.
    #[test]
    fn a_user_agent_over_the_limit_alone_gives_a_zero_device_id() {
        let Some(data_file) = dd_lite_data_file() else {
            eprintln!("no Lite Hash data file found; skipping zero device id test");
            return;
        };
        const ZERO: &str = "0-0-0-0";
        let over = padded_user_agent(HEADER_LIMIT + 1);
        let cut = &over[..HEADER_LIMIT];

        // The device ids from the direct User-Agent call and from evidence.
        let device_ids = |manager: &Arc<dd::Manager>, user_agent: &str| {
            let mut direct = manager.create_results().expect("results should allocate");
            direct
                .process_user_agent(user_agent)
                .expect("a user agent should not raise an exception");
            let mut through_evidence = manager.create_results().expect("results should allocate");
            let evidence = Evidence::builder()
                .add("header.user-agent", user_agent)
                .build();
            through_evidence
                .process_evidence(&evidence)
                .expect("a header should not raise an exception");
            [direct, through_evidence].map(|results| {
                results
                    .device_id()
                    .expect("reading the device id should not error")
                    .expect("a detection should give a device id")
            })
        };

        let defaults = open_loaded(&data_file);
        assert_eq!(device_ids(&defaults, &over), [ZERO, ZERO], "over the limit");
        let whole = device_ids(&defaults, MOBILE_USER_AGENT);
        assert_ne!(whole[0], ZERO);
        assert_eq!(device_ids(&defaults, cut), whole, "cut to the limit");

        let mut options = dd::HashOptions::default();
        options.allow_unmatched = Some(true);
        let unmatched = dd::Manager::open_with_hash_options(
            &data_file,
            PerformanceProfile::Default,
            None,
            &options,
        )
        .expect("the Lite file should load with unmatched results allowed");
        for id in device_ids(&unmatched, &over) {
            assert_ne!(id, ZERO, "over the limit with unmatched results allowed");
        }

        let mut results = defaults.create_results().expect("results should allocate");
        let evidence = Evidence::builder()
            .add("query.user-agent", over.as_str())
            .add("header.user-agent", DESKTOP_USER_AGENT)
            .build();
        results
            .process_evidence(&evidence)
            .expect("processing evidence should not raise an exception");
        let id = results
            .device_id()
            .expect("reading the device id should not error")
            .expect("a detection should give a device id");
        assert_ne!(id, ZERO, "over the limit with other evidence that matches");
    }

    /// Naming `DeviceId` resolves every component, whatever else is named, so
    /// the id is the one a data set with every property gives. Without it, a
    /// component with no named property reads as zero.
    #[test]
    fn naming_device_id_resolves_every_component() {
        let Some(data_file) = dd_lite_data_file() else {
            eprintln!("no Lite Hash data file found; skipping named device id test");
            return;
        };
        let device_id = |names: Option<&[&str]>| {
            let manager =
                dd::Manager::open_with_properties(&data_file, PerformanceProfile::Default, names)
                    .unwrap_or_else(|err| panic!("{names:?} should load: {err}"));
            let mut results = manager.create_results().expect("results should allocate");
            results
                .process_user_agent(MOBILE_USER_AGENT)
                .expect("processing a user agent should not raise an exception");
            results
                .device_id()
                .expect("reading the device id should not error")
                .expect("a match should produce a device id")
        };

        let every_property = device_id(None);
        for names in [
            &["DeviceId"][..],
            &["deviceid"],
            &["DeviceId", "IsMobile"],
            &["IsMobile", "DeviceId"],
        ] {
            assert_eq!(device_id(Some(names)), every_property, "{names:?}");
        }
        assert_ne!(device_id(Some(&["IsMobile"])), every_property);
    }

    /// A name the data set has no property for, such as `DeviceId`, keeps its
    /// place among the names given, so a property named after it is still
    /// read, by its name and by its index.
    #[test]
    fn a_name_without_a_property_leaves_the_others_readable() {
        let Some(data_file) = dd_lite_data_file() else {
            eprintln!("no Lite Hash data file found; skipping a name without a property");
            return;
        };
        for names in [&["DeviceId", "IsMobile"][..], &["IsMobile", "DeviceId"]] {
            let manager = dd::Manager::open_with_properties(
                &data_file,
                PerformanceProfile::Default,
                Some(names),
            )
            .unwrap_or_else(|err| panic!("{names:?} should load: {err}"));
            let mut results = manager.create_results().expect("results should allocate");
            results
                .process_user_agent(MOBILE_USER_AGENT)
                .expect("processing a user agent should not raise an exception");
            let index = manager
                .required_property_index("IsMobile")
                .unwrap_or_else(|| panic!("{names:?} should give IsMobile an index"));
            assert_eq!(
                results
                    .value_as_string("IsMobile", ",")
                    .expect("reading IsMobile should not error")
                    .as_deref(),
                Some("True"),
                "{names:?}"
            );
            assert_eq!(
                results
                    .value_as_string_by_index(index, c",")
                    .expect("reading IsMobile by index should not error")
                    .as_deref(),
                Some("True"),
                "{names:?}"
            );
        }
    }

    /// A data file that can't be used is named in the error, so the caller can
    /// see which one.
    #[test]
    fn a_failed_load_names_the_data_file() {
        let directory = std::env::temp_dir();
        let missing = directory.join("fiftyone-native-smoke-dd-missing.hash");
        let not_data = directory.join("fiftyone-native-smoke-dd-not-data.hash");
        std::fs::write(&not_data, vec![7u8; 64 * 1024]).expect("a temporary file");

        for file in [&missing, &not_data] {
            let name = file.file_name().unwrap().to_string_lossy();
            match dd::Manager::open(file, PerformanceProfile::Default) {
                Err(Error::Native { message, .. }) => {
                    assert!(message.contains(&*name), "{name} is not in {message:?}");
                }
                Err(other) => panic!("{name} gave another kind of error: {other}"),
                Ok(_) => panic!("{name} should not load"),
            }
        }
        std::fs::remove_file(&not_data).ok();
    }

    /// Evidence the engine expands during processing (a high-entropy values
    /// blob, which it turns into client hint headers) and client-side
    /// override values are accepted, and the pooled evidence array can be
    /// reused afterwards for evidence of a different shape.
    #[test]
    fn client_side_evidence_is_accepted_and_the_array_reused() {
        let Some(data_file) = dd_lite_data_file() else {
            eprintln!("no Lite Hash data file found; skipping client-side evidence test");
            return;
        };
        let manager = open_loaded(&data_file);
        // Base64 of {"brands":[{"brand":"Google Chrome","version":"124"}],
        // "fullVersionList":[{"brand":"Google Chrome","version":"124.0.6367.91"}],
        // "mobile":false,"model":"","platform":"Windows",
        // "platformVersion":"15.0.0","architecture":"x86","bitness":"64"}
        let ghev = "eyJicmFuZHMiOlt7ImJyYW5kIjoiR29vZ2xlIENocm9tZSIsInZlcnNpb24iOiIxMjQifV0s\
            ImZ1bGxWZXJzaW9uTGlzdCI6W3siYnJhbmQiOiJHb29nbGUgQ2hyb21lIiwidmVyc2lvbiI6IjEy\
            NC4wLjYzNjcuOTEifV0sIm1vYmlsZSI6ZmFsc2UsIm1vZGVsIjoiIiwicGxhdGZvcm0iOiJXaW5k\
            b3dzIiwicGxhdGZvcm1WZXJzaW9uIjoiMTUuMC4wIiwiYXJjaGl0ZWN0dXJlIjoieDg2IiwiYml0\
            bmVzcyI6IjY0In0=";
        for _ in 0..3 {
            let mut results = manager.create_results().expect("results should allocate");
            let evidence = Evidence::builder()
                .add("header.user-agent", DESKTOP_USER_AGENT)
                .add("query.51d_gethighentropyvalues", ghev)
                .add("query.51d_screenpixelswidth", "1234")
                .add("cookie.51d_screenpixelsheight", "567")
                .build();
            results
                .process_evidence(&evidence)
                .expect("client-side evidence should not raise an exception");
            let value = results
                .value_as_string("IsMobile", ",")
                .expect("reading IsMobile should not error");
            assert_eq!(value.as_deref(), Some("False"));

            let mut results = manager.create_results().expect("results should allocate");
            let evidence = Evidence::builder()
                .add("header.user-agent", MOBILE_USER_AGENT)
                .build();
            results
                .process_evidence(&evidence)
                .expect("processing evidence should not raise an exception");
            let value = results
                .value_as_string("IsMobile", ",")
                .expect("reading IsMobile should not error");
            assert_eq!(value.as_deref(), Some("True"));
        }
    }

    /// A screen size the client-side JavaScript measured replaces the one
    /// matched from the headers. That needs results created with room for
    /// overrides. A property is only overridable when the data file has the
    /// JavaScript property that measures it, which the Lite file does not, so
    /// this runs with a licensed file such as Enterprise.
    #[test]
    fn browser_measured_values_override_the_data_file() {
        let Some(data_file) = dd_lite_data_file() else {
            eprintln!("no Hash data file found; skipping override test");
            return;
        };
        let manager = open_loaded(&data_file);
        for name in [
            "ScreenPixelsWidth",
            "ScreenPixelsHeight",
            "ScreenPixelsWidthJavaScript",
            "ScreenPixelsHeightJavaScript",
        ] {
            if manager.required_property_index(name).is_none() {
                eprintln!("data file has no {name}; skipping override test");
                return;
            }
        }
        let mut results = manager.create_results().expect("results should allocate");
        let evidence = Evidence::builder()
            .add("header.user-agent", MOBILE_USER_AGENT)
            .add("query.51d_screenpixelswidth", "1234")
            .add("cookie.51d_screenpixelsheight", "567")
            .build();
        results
            .process_evidence(&evidence)
            .expect("processing evidence should not raise an exception");
        for (name, measured) in [("ScreenPixelsWidth", "1234"), ("ScreenPixelsHeight", "567")] {
            assert_eq!(
                results
                    .value_as_string(name, ",")
                    .expect("reading a screen size should not error")
                    .as_deref(),
                Some(measured),
                "{name} should be the value the browser measured"
            );
        }
    }

    /// With the property value index built (the in-memory profiles build it),
    /// a property the matched profile holds no value for reads back empty,
    /// not as the values of the properties stored before it. The index used
    /// to be left uninitialised for such entries, which gave an iPhone a
    /// high-entropy values script of "Apple|Mobile Safari|17.0|...".
    #[test]
    fn missing_profile_value_is_empty_with_the_value_index() {
        let Some(data_file) = dd_lite_data_file() else {
            eprintln!("no Lite Hash data file found; skipping value index test");
            return;
        };
        let manager = dd::Manager::open(&data_file, PerformanceProfile::HighPerformance)
            .expect("the Lite data file loads in memory");
        if manager
            .required_property_index("JavascriptGetHighEntropyValues")
            .is_none()
        {
            eprintln!("data file has no JavascriptGetHighEntropyValues; skipping");
            return;
        }
        let mut results = manager.create_results().expect("results should allocate");
        results
            .process_user_agent(MOBILE_USER_AGENT)
            .expect("processing a user agent should not raise an exception");
        let value = results
            .value_as_string("JavascriptGetHighEntropyValues", "|")
            .expect("reading the script should not error")
            .unwrap_or_default();
        assert!(
            !value.contains('|'),
            "Safari needs no high-entropy values script, got '{value}'"
        );
    }

    /// Every property's value for each User-Agent, in property order.
    fn read_every_property(
        manager: &Arc<dd::Manager>,
        names: &[String],
        user_agents: &[&str],
    ) -> Vec<Vec<Option<String>>> {
        user_agents
            .iter()
            .map(|user_agent| {
                let mut results = manager.create_results().expect("results should allocate");
                results
                    .process_user_agent(user_agent)
                    .expect("processing a user agent should not raise an exception");
                names
                    .iter()
                    .map(|name| {
                        results
                            .value_as_string(name, ",")
                            .expect("reading a listed property should not error")
                    })
                    .collect()
            })
            .collect()
    }

    /// Threads that read every property at once, starting together before
    /// the manager has built its property index, get exactly the values a
    /// single thread gets.
    #[test]
    fn concurrent_reads_match_a_single_thread() {
        const THREADS: usize = 8;
        const ROUNDS: usize = 25;
        let Some(data_file) = dd_lite_data_file() else {
            eprintln!("no Lite Hash data file found; skipping concurrent read test");
            return;
        };
        let user_agents = [DESKTOP_USER_AGENT, MOBILE_USER_AGENT];
        let expected = {
            let manager = open_loaded(&data_file);
            let names = manager.property_names();
            read_every_property(&manager, &names, &user_agents)
        };
        assert!(
            expected.iter().flatten().any(Option::is_some),
            "the single-threaded reads should find values"
        );

        let manager = open_loaded(&data_file);
        let names = Arc::new(manager.property_names());
        let start = Arc::new(std::sync::Barrier::new(THREADS));
        let readers: Vec<_> = (0..THREADS)
            .map(|_| {
                let (manager, names, start) =
                    (Arc::clone(&manager), Arc::clone(&names), Arc::clone(&start));
                std::thread::spawn(move || {
                    start.wait();
                    (0..ROUNDS)
                        .map(|_| read_every_property(&manager, &names, &user_agents))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for reader in readers {
            for values in reader.join().expect("a reader thread should not panic") {
                assert_eq!(values, expected);
            }
        }
    }

    /// An unknown property reads back as no value rather than an error, the
    /// contract the safe API promises.
    #[test]
    fn unknown_property_is_none() {
        let Some(data_file) = dd_lite_data_file() else {
            eprintln!("no Lite Hash data file found; skipping unknown property test");
            return;
        };
        let manager = open_loaded(&data_file);
        let results = manager.create_results().expect("results should allocate");
        assert_eq!(manager.required_property_index("NotAProperty"), None);
        assert_eq!(
            results
                .value_as_string("NotAProperty", ",")
                .expect("reading an unknown property should not error"),
            None
        );
    }
}

#[cfg(feature = "ipi")]
mod ip_intelligence {
    use super::ipi_data_file;
    use fiftyone_native::ipi::PropertyRead;
    use fiftyone_native::{ipi, PerformanceProfile};
    use fiftyone_pipeline_core::{Error, Evidence};

    /// Open the IP Intelligence data file through the safe API. Both products are
    /// linked in this binary and now coexist, and the resolved ASN file matches
    /// this source revision, so a load failure is a real fault rather than a
    /// skip. The file presence is environmental, so the caller skips only when
    /// the file is absent. Once found it must load, hence the hard `expect`.
    fn open_loaded(data_file: &std::path::Path) -> std::sync::Arc<ipi::Manager> {
        ipi::Manager::open(data_file, PerformanceProfile::Default).unwrap_or_else(|err| {
            panic!("IP Intelligence data file should load through the safe API: {err}")
        })
    }

    /// Open the IP Intelligence data file and look up a public IP address
    /// through the safe API, reading at least one network property back.
    #[test]
    fn lookup_public_ip_via_safe_api() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping IPI smoke test");
            return;
        };
        let manager = open_loaded(&data_file);

        let mut results = manager.create_results().expect("results should allocate");
        // A Cloudflare public IP, which the ASN data set maps to AS13335.
        results
            .process_ip("1.1.1.1")
            .expect("looking up a public IP should not raise an exception");

        // The ASN data set populates the autonomous system number for a public
        // IP. A non-empty value proves a real lookup resolved.
        let asn = results
            .value_as_string("Asn", "|")
            .expect("reading the Asn property should not error")
            .expect("the Asn property should be present in the ASN data set");
        eprintln!("Asn = {asn}");
        assert!(
            asn.contains("AS13335"),
            "the Cloudflare IP should resolve to AS13335, got {asn:?}"
        );
    }

    /// The same lookup driven from pipeline evidence carrying the client IP,
    /// exercising the evidence-to-IP extraction.
    #[test]
    fn lookup_from_evidence_via_safe_api() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping IPI evidence test");
            return;
        };
        let manager = open_loaded(&data_file);

        let mut results = manager.create_results().expect("results should allocate");
        let evidence = Evidence::builder()
            .add("server.client-ip", "185.28.167.77")
            .build();
        results
            .process_evidence(&evidence)
            .expect("processing evidence should not raise an exception");
        // Reading any value should not error even if the property set is small.
        let _ = results
            .value_as_string("RegisteredCountry", "|")
            .expect("reading a property should not error");
    }

    /// Looking up evidence with no client IP gives results with no values, as
    /// .NET gives, rather than an error.
    #[test]
    fn evidence_without_ip_gives_no_values() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping no-IP test");
            return;
        };
        let manager = open_loaded(&data_file);
        let mut results = manager.create_results().expect("results should allocate");
        let evidence = Evidence::builder().add("header.user-agent", "x").build();
        results
            .process_evidence(&evidence)
            .expect("evidence with no client IP should be looked up");
        let index = manager
            .required_property_index("Asn")
            .expect("the ASN data set has Asn");
        assert!(!results
            .has_values_by_index(index)
            .expect("asking for values should not error"));
    }

    /// An IPv4-mapped IPv6 address gives the answers of its IPv4 address, in
    /// either form, looked up as an address or from evidence.
    #[test]
    fn a_mapped_address_gives_the_answers_of_its_ipv4_address() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping mapped address test");
            return;
        };
        let manager = open_loaded(&data_file);
        let names = manager.available_property_names();
        let read = |address: &str, from_evidence: bool| {
            let mut results = manager.create_results().expect("results should allocate");
            let processed = if from_evidence {
                let evidence = Evidence::builder().add("server.client-ip", address).build();
                results.process_evidence(&evidence)
            } else {
                results.process_ip(address)
            };
            processed.expect("looking up an address should not raise an exception");
            names
                .iter()
                .map(|name| {
                    (
                        results
                            .value_as_string(name, "|")
                            .expect("reading a value should not error"),
                        results
                            .read_property(name)
                            .expect("reading a property should not error"),
                    )
                })
                .collect::<Vec<_>>()
        };

        // Agreeing only because nothing has a value would prove nothing.
        let mut results = manager.create_results().expect("results should allocate");
        results
            .process_ip("::ffff:1.1.1.1")
            .expect("looking up a mapped address should not raise an exception");
        let asn = results
            .value_as_string("Asn", "|")
            .expect("reading the Asn property should not error")
            .expect("the Asn property should be present in the ASN data set");
        assert!(
            asn.contains("AS13335"),
            "::ffff:1.1.1.1 should resolve to AS13335 as 1.1.1.1 does, got {asn:?}"
        );

        for (ipv4, mapped) in [
            ("1.1.1.1", "::ffff:1.1.1.1"),
            ("185.28.167.77", "::ffff:b91c:a74d"),
        ] {
            for from_evidence in [false, true] {
                assert_eq!(
                    read(mapped, from_evidence),
                    read(ipv4, from_evidence),
                    "{mapped} against {ipv4}, from evidence {from_evidence}"
                );
            }
        }
    }

    /// A data file that can't be used is named in the error, so the caller can
    /// see which one.
    #[test]
    fn a_failed_load_names_the_data_file() {
        let directory = std::env::temp_dir();
        let missing = directory.join("fiftyone-native-smoke-ipi-missing.ipi");
        let not_data = directory.join("fiftyone-native-smoke-ipi-not-data.ipi");
        std::fs::write(&not_data, vec![7u8; 64 * 1024]).expect("a temporary file");

        for file in [&missing, &not_data] {
            let name = file.file_name().unwrap().to_string_lossy();
            match ipi::Manager::open(file, PerformanceProfile::Default) {
                Err(Error::Native { message, .. }) => {
                    assert!(message.contains(&*name), "{name} is not in {message:?}");
                }
                Err(other) => panic!("{name} gave another kind of error: {other}"),
                Ok(_) => panic!("{name} should not load"),
            }
        }
        std::fs::remove_file(&not_data).ok();
    }

    /// The addresses whose answers are compared, in both families, including
    /// one that no public range holds and an IPv4 address in its mapped form.
    const COMPARED_ADDRESSES: [&str; 6] = [
        "1.1.1.1",
        "8.8.8.8",
        "185.28.167.77",
        "2001:4860:4860::8888",
        "10.0.0.1",
        "::ffff:185.28.167.77",
    ];

    /// What a caller can read of a property for each compared address.
    fn answers(
        manager: &std::sync::Arc<ipi::Manager>,
        property: &str,
    ) -> Vec<(Option<String>, PropertyRead)> {
        COMPARED_ADDRESSES
            .iter()
            .map(|address| {
                let mut results = manager.create_results().expect("results should allocate");
                results
                    .process_ip(address)
                    .expect("looking up an address should not raise an exception");
                (
                    results
                        .value_as_string(property, "|")
                        .expect("reading a value should not error"),
                    results
                        .read_property(property)
                        .expect("reading a property should not error"),
                )
            })
            .collect()
    }

    /// Naming properties can change how a profile reads the data file, and
    /// never an answer. Each list is read under each profile and compared with
    /// a data set holding every property.
    #[test]
    fn naming_properties_keeps_the_answers_under_every_profile() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping named properties test");
            return;
        };
        let every_property = ipi::Manager::open(&data_file, PerformanceProfile::LowMemory)
            .expect("the data file should load");
        let names = every_property.available_property_names();
        // Lists agreeing only because nothing has a value would prove nothing.
        let answered = names
            .iter()
            .filter(|name| {
                answers(&every_property, name)
                    .iter()
                    .any(|(value, _)| value.as_deref().is_some_and(|value| !value.is_empty()))
            })
            .count();
        eprintln!(
            "{answered} of {} properties have an answer to compare",
            names.len()
        );
        assert!(answered > 0, "the compared addresses should have answers");

        let mut lists: Vec<Vec<&str>> = names.iter().map(|name| vec![name.as_str()]).collect();
        lists.push(vec![names[0].as_str(), names[names.len() - 1].as_str()]);
        lists.push(names.iter().map(String::as_str).collect());

        for profile in [
            PerformanceProfile::Default,
            PerformanceProfile::Balanced,
            PerformanceProfile::LowMemory,
            PerformanceProfile::HighPerformance,
            PerformanceProfile::InMemory,
        ] {
            for list in &lists {
                let manager =
                    ipi::Manager::open_with_properties(&data_file, profile, Some(list.as_slice()))
                        .unwrap_or_else(|err| {
                            panic!("{profile:?} naming {list:?} should load: {err}")
                        });
                for name in list {
                    assert_eq!(
                        answers(&manager, name),
                        answers(&every_property, name),
                        "{profile:?} naming {list:?}, reading {name}"
                    );
                }
            }
        }
    }

    /// A name the data file has no property for keeps its place among the
    /// names given, so a property named after it still gives the answers of a
    /// data set holding every property.
    #[test]
    fn a_name_without_a_property_leaves_the_others_readable() {
        let Some(data_file) = ipi_data_file() else {
            eprintln!("no IP Intelligence data file found; skipping a name without a property");
            return;
        };
        let every_property = ipi::Manager::open(&data_file, PerformanceProfile::LowMemory)
            .expect("the data file should load");
        for name in every_property.available_property_names() {
            let list = ["NoSuchProperty", name.as_str()];
            let manager = ipi::Manager::open_with_properties(
                &data_file,
                PerformanceProfile::LowMemory,
                Some(list.as_slice()),
            )
            .unwrap_or_else(|err| panic!("naming {list:?} should load: {err}"));
            assert_eq!(
                answers(&manager, &name),
                answers(&every_property, &name),
                "naming {list:?}"
            );
        }
    }
}

/// The proof that Device Detection and IP Intelligence coexist in one binary.
///
/// This test compiles only when both products are enabled, which is the default
/// for this crate. It loads a Device Detection Hash file and an IP Intelligence
/// file in the SAME process, runs a real detection and a real lookup, and
/// asserts a real value from each. Before the IP Intelligence common C library
/// copy was namespaced, the two builds shared `fiftyoneDegrees*` symbols, the
/// linker bound each duplicate to one definition, and Device Detection then
/// read its Hash file through the wrong, wide-offset common code and failed
/// with CorruptData. With the `ipi_` prefix in place both engines run correctly
/// side by side, which this test confirms.
///
/// Both data files are resolved the same way as the per-product smoke tests (an
/// explicit env path, else a sibling `*-cxx` checkout). When both are present the
/// proof runs in full and any open or lookup error is a hard failure, so a
/// reintroduced collision cannot pass silently. When either file is absent (for
/// example a checkout where the data files have not been downloaded) the test
/// skips, since the proof needs both real files. It prints the IsMobile value and
/// one IP Intelligence value so the proof is visible in the test output.
#[cfg(all(feature = "dd", feature = "ipi"))]
#[test]
fn both_engines_coexist() {
    use fiftyone_native::{dd, ipi, PerformanceProfile};

    // Resolve both data files the same way the per-product smoke tests do, rather
    // than hard-coding a path. The proof needs both real files, so skip cleanly
    // when either is absent (for example a checkout where the data files have
    // not been downloaded).
    let (Some(dd_path), Some(ipi_path)) = (dd_lite_data_file(), ipi_data_file()) else {
        eprintln!(
            "skipping both_engines_coexist: both a Device Detection Hash file and \
             an IP Intelligence ASN file are required"
        );
        return;
    };

    // A representative desktop Chrome on Windows user agent, which is not mobile.
    const DESKTOP_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
        AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

    // --- Device Detection in this binary ---
    let dd_manager = dd::Manager::open(dd_path, PerformanceProfile::Default)
        .expect("Device Detection Hash file should open in the combined binary");
    let mut dd_results = dd_manager
        .create_results()
        .expect("Device Detection results should allocate");
    dd_results
        .process_user_agent(DESKTOP_USER_AGENT)
        .expect("processing a user agent should not error");
    let is_mobile = dd_results
        .value_as_string("IsMobile", ",")
        .expect("reading IsMobile should not error")
        .expect("IsMobile should produce a value for a desktop user agent");
    println!("both_engines_coexist: Device Detection IsMobile = {is_mobile}");
    assert_eq!(
        is_mobile, "False",
        "a desktop user agent should not be mobile, proving Device Detection read its \
         Hash file through the correct common C library build"
    );

    // --- IP Intelligence in the SAME binary ---
    let ipi_manager = ipi::Manager::open(ipi_path, PerformanceProfile::Default)
        .expect("IP Intelligence ASN file should open in the combined binary");
    let mut ipi_results = ipi_manager
        .create_results()
        .expect("IP Intelligence results should allocate");
    // A Cloudflare public IP, which the ASN data set maps to autonomous system
    // AS13335. Using a known mapping makes the proof deterministic rather than
    // relying on whatever a given address happens to resolve to.
    ipi_results
        .process_ip("1.1.1.1")
        .expect("looking up a public IP should not error");

    // Read back the autonomous system number for the IP. The ASN data file
    // populates the `Asn` property, and a non-empty value here proves the lookup
    // resolved through the correct, prefixed common C library build. The native
    // weighted string getter renders the value as `value:weighting`, so the
    // assertion checks the value content rather than an exact string.
    let asn = ipi_results
        .value_as_string("Asn", "|")
        .expect("reading the Asn property should not error")
        .expect("the Asn property should be present in the ASN data set");
    println!("both_engines_coexist: IP Intelligence Asn = {asn:?}");
    assert!(
        asn.contains("AS13335"),
        "the Cloudflare IP should resolve to AS13335 through the ASN data set, got {asn:?}"
    );
    let ipi_value = asn;

    // Both engines produced a real value in one process. Assert it explicitly so
    // the test reads as the coexistence proof it is.
    assert_eq!(is_mobile, "False");
    assert!(
        !ipi_value.is_empty(),
        "the IP Intelligence value should not be empty"
    );
    println!(
        "both_engines_coexist: PASS - Device Detection (IsMobile={is_mobile}) and IP \
         Intelligence ({ipi_value}) both loaded and resolved in one binary"
    );
}
