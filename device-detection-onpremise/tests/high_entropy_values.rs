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

//! The engine turns `51D_GetHighEntropyValues` evidence into client hints
//! itself.
//!
//! The engine passes the base 64 high entropy values to the C library, which
//! turns them into `sec-ch-ua*` headers before it matches. Each case checks
//! the platform name and version the engine alone gives, which come from the
//! high entropy values wherever they can be read, and that a pipeline that
//! still has the deprecated UA-CH high entropy decoder element before the
//! engine gives the same device answers. The evidence is that of the
//! `uach-ghev-*` cases of the comparison with .NET. Skips when the Lite Hash
//! data file is absent.

// The decoder element is deprecated, and these tests use it on purpose.
#![allow(deprecated)]

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};
use fiftyone_device_detection_shared::{UachJsConversionElement, DEVICE_DATA_KEY};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{
    ElementData, Evidence, FlowElement, NoValueError, Pipeline, PropertyValue,
};

/// Chrome 131 on macOS, as the reduced User-Agent gives it.
const MAC_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Chrome 131 on Windows, as the reduced User-Agent gives it.
const WINDOWS_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// The high entropy values of Chrome 131 on macOS 15.1.1, in base 64.
const MAC_VALUES: &str = "eyJhcmNoaXRlY3R1cmUiOiJhcm0iLCJicmFuZHMiOlt7ImJyYW5kIjoiR29vZ2xlIENocm9tZSIsInZlcnNpb24iOiIxMzEifSx7ImJyYW5kIjoiQ2hyb21pdW0iLCJ2ZXJzaW9uIjoiMTMxIn0seyJicmFuZCI6Ik5vdF9BIEJyYW5kIiwidmVyc2lvbiI6IjI0In1dLCJmdWxsVmVyc2lvbkxpc3QiOlt7ImJyYW5kIjoiR29vZ2xlIENocm9tZSIsInZlcnNpb24iOiIxMzEuMC42Nzc4LjE0MCJ9LHsiYnJhbmQiOiJDaHJvbWl1bSIsInZlcnNpb24iOiIxMzEuMC42Nzc4LjE0MCJ9LHsiYnJhbmQiOiJOb3RfQSBCcmFuZCIsInZlcnNpb24iOiIyNC4wLjAuMCJ9XSwibW9iaWxlIjpmYWxzZSwibW9kZWwiOiIiLCJwbGF0Zm9ybSI6Im1hY09TIiwicGxhdGZvcm1WZXJzaW9uIjoiMTUuMS4xIn0=";

/// The high entropy values of Chrome 131 on Android 13, model SM-S911B, in
/// base 64.
const ANDROID_VALUES: &str = "eyJhcmNoaXRlY3R1cmUiOiIiLCJicmFuZHMiOlt7ImJyYW5kIjoiR29vZ2xlIENocm9tZSIsInZlcnNpb24iOiIxMzEifSx7ImJyYW5kIjoiQ2hyb21pdW0iLCJ2ZXJzaW9uIjoiMTMxIn0seyJicmFuZCI6Ik5vdF9BIEJyYW5kIiwidmVyc2lvbiI6IjI0In1dLCJmdWxsVmVyc2lvbkxpc3QiOlt7ImJyYW5kIjoiR29vZ2xlIENocm9tZSIsInZlcnNpb24iOiIxMzEuMC42Nzc4LjgxIn0seyJicmFuZCI6IkNocm9taXVtIiwidmVyc2lvbiI6IjEzMS4wLjY3NzguODEifSx7ImJyYW5kIjoiTm90X0EgQnJhbmQiLCJ2ZXJzaW9uIjoiMjQuMC4wLjAifV0sIm1vYmlsZSI6dHJ1ZSwibW9kZWwiOiJTTS1TOTExQiIsInBsYXRmb3JtIjoiQW5kcm9pZCIsInBsYXRmb3JtVmVyc2lvbiI6IjEzLjAuMCJ9";

/// The query string key of the high entropy values.
const QUERY_KEY: &str = "query.51D_GetHighEntropyValues";

/// The cookie key of the high entropy values.
const COOKIE_KEY: &str = "cookie.51D_GetHighEntropyValues";

/// One case of evidence and the platform the engine gives for it.
struct Case {
    /// The case's name in assertion messages.
    name: &'static str,
    /// The `PlatformName` the engine gives.
    platform_name: &'static str,
    /// The `PlatformVersion` the engine gives.
    platform_version: &'static str,
    /// The evidence, in the order it is added.
    evidence: Vec<(&'static str, &'static str)>,
}

/// A case of `evidence` named `name`, for which the engine gives the
/// platform `platform_name` at `platform_version`.
fn case(
    name: &'static str,
    platform_name: &'static str,
    platform_version: &'static str,
    evidence: Vec<(&'static str, &'static str)>,
) -> Case {
    Case {
        name,
        platform_name,
        platform_version,
        evidence,
    }
}

/// Every case, with the platform both the Lite and the Enterprise file give.
fn cases() -> Vec<Case> {
    vec![
        case(
            "query",
            "macOS",
            "15.1.1",
            vec![
                ("header.user-agent", MAC_USER_AGENT),
                (QUERY_KEY, MAC_VALUES),
            ],
        ),
        case(
            "cookie",
            "macOS",
            "15.1.1",
            vec![
                ("header.user-agent", MAC_USER_AGENT),
                (COOKIE_KEY, MAC_VALUES),
            ],
        ),
        case(
            "query then cookie",
            "macOS",
            "15.1.1",
            vec![(QUERY_KEY, MAC_VALUES), (COOKIE_KEY, ANDROID_VALUES)],
        ),
        // The query string's value is used before the cookie's whatever
        // order they were added in, as evidence.md and the decoder
        // specification give it precedence.
        case(
            "cookie then query",
            "macOS",
            "15.1.1",
            vec![(COOKIE_KEY, ANDROID_VALUES), (QUERY_KEY, MAC_VALUES)],
        ),
        case(
            "over headers",
            "Android",
            "13.0",
            vec![
                (
                    "header.sec-ch-ua",
                    "\"Google Chrome\";v=\"131\", \"Chromium\";v=\"131\", \
                     \"Not_A Brand\";v=\"24\"",
                ),
                ("header.sec-ch-ua-mobile", "?0"),
                ("header.sec-ch-ua-platform", "\"Windows\""),
                ("header.sec-ch-ua-platform-version", "\"15.0.0\""),
                (
                    "header.sec-ch-ua-full-version-list",
                    "\"Google Chrome\";v=\"131.0.6778.86\", \
                     \"Chromium\";v=\"131.0.6778.86\", \
                     \"Not_A Brand\";v=\"24.0.0.0\"",
                ),
                ("header.sec-ch-ua-arch", "\"x86\""),
                ("header.sec-ch-ua-bitness", "\"64\""),
                ("header.sec-ch-ua-model", "\"\""),
                ("header.user-agent", WINDOWS_USER_AGENT),
                (QUERY_KEY, ANDROID_VALUES),
            ],
        ),
        case(
            "User-Agent in the query",
            "macOS",
            "15.1.1",
            vec![
                ("query.user-agent", MAC_USER_AGENT),
                (QUERY_KEY, MAC_VALUES),
            ],
        ),
        case(
            "no User-Agent",
            "Android",
            "13.0",
            vec![(QUERY_KEY, ANDROID_VALUES)],
        ),
        // The User-Agent alone gives the platform.
        case(
            "not base 64",
            "macOS",
            "10.15.7",
            vec![
                ("header.user-agent", MAC_USER_AGENT),
                (QUERY_KEY, "This is not base 64!"),
            ],
        ),
    ]
}

/// The Lite Hash data file `_51DEGREES_DD_PATH` names, or else the one in the
/// `cxx` submodule found from this crate's directory upwards.
fn lite_data_file() -> Option<PathBuf> {
    if let Some(path) = std::env::var("_51DEGREES_DD_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Some(path);
    }
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join("cxx/data/device-detection/51Degrees-LiteV4.1.hash");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// An engine over the Lite file, or `None` when it is absent.
fn build_engine() -> Option<Arc<DeviceDetectionOnPremiseEngine>> {
    let data_file = lite_data_file()?;
    Some(
        DeviceDetectionOnPremiseEngineBuilder::new(data_file)
            .performance_profile(PerformanceProfile::HighPerformance)
            .build()
            .expect("the engine builds over the Lite file"),
    )
}

/// Every device property and its value or no value reason, in the order the
/// device data lists them.
type Answers = Vec<(String, Result<PropertyValue, NoValueError>)>;

/// The device answers for `evidence`, and how many client hints the decoder
/// element wrote, which is `None` when the pipeline holds no such element.
fn answers(pipeline: &Arc<Pipeline>, evidence: &[(&str, &str)]) -> (Answers, Option<usize>) {
    let mut builder = Evidence::builder();
    for (key, value) in evidence {
        builder = builder.add(key, *value);
    }
    let mut data = pipeline.create_flow_data_with(builder.build());
    data.process().expect("processing succeeds");
    let hints = data
        .get_str(UachJsConversionElement::DATA_KEY.name())
        .map(|uach| ElementData::keys(uach).len());
    let device = data.get(DEVICE_DATA_KEY).expect("device data was produced");
    let answers = ElementData::keys(device)
        .into_iter()
        .map(|name| {
            let value = ElementData::get(device, &name);
            (name, value)
        })
        .collect();
    (answers, hints)
}

/// The text of the property `name` in `answers`, or `None` when it has no
/// text value there.
fn text<'a>(answers: &'a Answers, name: &str) -> Option<&'a str> {
    answers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .and_then(|(_, value)| value.as_ref().ok())
        .and_then(PropertyValue::as_str)
}

#[test]
fn the_decoder_element_changes_no_device_answer() {
    let Some(engine) = build_engine() else {
        eprintln!("no Lite Hash data file found; skipping the high entropy values test");
        return;
    };
    let engine: Arc<dyn FlowElement> = engine;
    let alone = Pipeline::builder()
        .add_element(Arc::clone(&engine))
        .build()
        .expect("the pipeline of the engine alone builds");
    let decoded = Pipeline::builder()
        .add_element(Arc::new(UachJsConversionElement::new()))
        .add_element(engine)
        .build()
        .expect("the pipeline with the decoder element builds");

    for case in cases() {
        let name = case.name;
        let (expected, none) = answers(&alone, &case.evidence);
        let (actual, hints) = answers(&decoded, &case.evidence);
        assert!(none.is_none(), "{name}: the engine alone has no decoder");
        assert!(!expected.is_empty(), "{name}: the engine lists properties");
        assert_eq!(
            (
                text(&expected, "PlatformName"),
                text(&expected, "PlatformVersion"),
            ),
            (Some(case.platform_name), Some(case.platform_version)),
            "{name}: the platform the engine alone gives"
        );
        if name != "not base 64" {
            assert!(
                hints.is_some_and(|count| count > 0),
                "{name}: the decoder element wrote client hints, so the \
                 second pipeline's Flow Data holds data the first does not"
            );
        }
        assert_eq!(actual, expected, "{name}");
    }
}
