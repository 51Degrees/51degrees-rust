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

//! Cross-facade on-premise coexistence.
//!
//! This integration test links the Device Detection on-premise Hash engine and
//! the IP Intelligence on-premise engine into a single binary, then runs a real
//! detection and a real lookup. It is the facade-level proof of the native
//! coexistence fact: IP Intelligence's common C library symbols are namespaced
//! to `ipi_fiftyoneDegrees*`, so the two native engines link side by side with
//! no collision.
//!
//! The Device Detection facade pulls `fiftyone-native` with the `dd` feature and
//! the IP Intelligence facade pulls it with the `ipi` feature. When both facades
//! are in one dependency graph Cargo unifies those into `["dd", "ipi"]`, which
//! links both namespaced builds. If the namespacing regressed, this test binary
//! would fail to link (duplicate-symbol) rather than fail an assertion.
//!
//! Each data file is resolved at run time. When a file is genuinely absent the
//! test skips that half rather than failing, so a checkout without the data
//! files is still green, while a developer (and CI) with them gets real coverage.

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::path::PathBuf;

use fiftyone_device_detection::{
    DeviceData, DeviceDetectionPipelineBuilder, PerformanceProfile, DEVICE_DATA_KEY,
};
use fiftyone_ip_intelligence::{IpIntelligencePipelineBuilder, IP_DATA_KEY};
use fiftyone_pipeline_core::Evidence;

/// Walk up from this crate's directory looking for a data file at the given
/// relative path inside a sibling `*-cxx` checkout, returning the first hit.
fn find_up(relative: &str) -> Option<PathBuf> {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join(relative);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[test]
fn dd_and_ipi_on_premise_coexist_in_one_binary() {
    // Resolve both data files. Honour explicit overrides first, then fall back to
    // the sibling cxx checkouts beside the workspace.
    let dd_file = crate::deprecated_names::var("_51DEGREES_DD_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| find_up("cxx/data/device-detection/51Degrees-LiteV4.1.hash"));
    let ipi_file = crate::deprecated_names::var("_51DEGREES_IPI_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| find_up("cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi"));

    // Build both on-premise pipelines in this one binary. The mere fact that this
    // links and both builders run is the coexistence proof.
    let mut ran_dd = false;
    let mut ran_ipi = false;

    if let Some(dd_file) = dd_file.as_ref() {
        let dd_pipeline = DeviceDetectionPipelineBuilder::on_premise(dd_file)
            .performance_profile(PerformanceProfile::HighPerformance)
            // Off so the test sends nothing to 51Degrees.
            .share_usage(false)
            // Off so the build needs no licence key.
            .auto_update(false)
            .build()
            .expect("device-detection on-premise pipeline builds");

        let user_agent = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
                          AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 \
                          Mobile/15E148 Safari/604.1";
        let mut data = dd_pipeline.create_flow_data_with(
            Evidence::builder()
                .add("header.user-agent", user_agent)
                .build(),
        );
        data.process()
            .expect("device detection processing succeeds");
        let device = data
            .get(DEVICE_DATA_KEY)
            .expect("the Hash engine produced device data");
        let is_mobile = device.is_mobile();
        let is_mobile = is_mobile
            .value()
            .expect("IsMobile resolves for an iPhone user agent");
        assert!(*is_mobile, "an iPhone user agent is detected as mobile");
        ran_dd = true;
    } else {
        eprintln!("skipping the DD half: no Lite Hash data file found");
    }

    if let Some(ipi_file) = ipi_file.as_ref() {
        let ipi_pipeline = IpIntelligencePipelineBuilder::on_premise(ipi_file)
            .performance_profile(PerformanceProfile::HighPerformance)
            .property("Asn")
            // Off so the test sends nothing to 51Degrees.
            .share_usage(false)
            // Off so the build needs no licence key.
            .auto_update(false)
            .build()
            .expect("ip-intelligence on-premise pipeline builds");

        let mut data = ipi_pipeline.create_flow_data_with(
            // Cloudflare's public resolver, mapped to autonomous system AS13335.
            Evidence::builder()
                .add("server.client-ip", "1.1.1.1")
                .build(),
        );
        data.process().expect("ip intelligence processing succeeds");
        let ip = data.get(IP_DATA_KEY).expect("the engine produced ip data");
        // Asn is a plain string property.
        let asn = ip.string("Asn");
        let value = asn.value().expect("Asn resolves to a value");
        assert!(
            value.contains("AS13335"),
            "the Cloudflare IPv4 resolves to AS13335, got {value}"
        );
        ran_ipi = true;
    } else {
        eprintln!("skipping the IPI half: no ASN data file found");
    }

    // When both files are present (the developer and CI case) both halves must
    // have run real native work in this single binary.
    if dd_file.is_some() && ipi_file.is_some() {
        assert!(
            ran_dd && ran_ipi,
            "both the DD detection and the IPI lookup must run in one binary"
        );
    }
}

#[test]
fn dd_and_ipi_combined_in_one_pipeline() {
    // The coexistence test above runs two separate pipelines. This goes further
    // and composes both on-premise engines into ONE pipeline so a single flow
    // data carries both the device and the IP results, which is the "mixed"
    // scenario a real application uses. The engines are taken from each facade
    // pipeline and added to a fresh pipeline, the same composition the web
    // examples use. Skips unless both data files are present.
    use std::sync::Arc;

    use fiftyone_pipeline_core::{FlowElement, Pipeline};

    let dd_file = crate::deprecated_names::var("_51DEGREES_DD_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| find_up("cxx/data/device-detection/51Degrees-LiteV4.1.hash"));
    let ipi_file = crate::deprecated_names::var("_51DEGREES_IPI_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| find_up("cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi"));
    let (Some(dd_file), Some(ipi_file)) = (dd_file, ipi_file) else {
        eprintln!("skipping the combined-pipeline test: both data files are required");
        return;
    };

    // Build each facade pipeline, then hand their engines to one shared pipeline.
    let dd_pipeline = DeviceDetectionPipelineBuilder::on_premise(&dd_file)
        .performance_profile(PerformanceProfile::HighPerformance)
        // Off so the test sends nothing to 51Degrees.
        .share_usage(false)
        // Off so the build needs no licence key.
        .auto_update(false)
        .build()
        .expect("device-detection on-premise pipeline builds");
    let ipi_pipeline = IpIntelligencePipelineBuilder::on_premise(&ipi_file)
        .performance_profile(PerformanceProfile::HighPerformance)
        .property("Asn")
        // Off so the test sends nothing to 51Degrees.
        .share_usage(false)
        // Off so the build needs no licence key.
        .auto_update(false)
        .build()
        .expect("ip-intelligence on-premise pipeline builds");

    let mut elements: Vec<Arc<dyn FlowElement>> = dd_pipeline.flow_elements().to_vec();
    elements.extend(ipi_pipeline.flow_elements().iter().cloned());
    let mut builder = Pipeline::builder();
    for element in elements {
        builder = builder.add_element(element);
    }
    let combined = builder.build().expect("the combined pipeline builds");

    // One flow data, both kinds of evidence.
    let mut data = combined.create_flow_data_with(
        Evidence::builder()
            .add(
                "header.user-agent",
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
                 AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1",
            )
            .add("server.client-ip", "1.1.1.1")
            .build(),
    );
    data.process().expect("combined processing succeeds");

    // Both results are present in the one flow data.
    let device = data
        .get(DEVICE_DATA_KEY)
        .expect("device data present in the combined flow");
    assert!(
        *device.is_mobile().value().expect("IsMobile resolves"),
        "the iPhone user agent is mobile"
    );
    let ip = data
        .get(IP_DATA_KEY)
        .expect("ip data present in the combined flow");
    // Asn is a plain string property.
    let asn = ip.string("Asn");
    let value = asn.value().expect("Asn resolves").clone();
    assert!(
        value.contains("AS13335"),
        "the Cloudflare IPv4 resolves to AS13335, got {value}"
    );
}

/// Device Detection, IP Intelligence and an element that always fails, run as
/// a parallel group, give the values and errors they give in sequence.
mod parallel_group {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use fiftyone_pipeline_core::{
        Error, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
        Pipeline, PropertyMetaData, Result,
    };

    use super::{
        find_up, DeviceData, DeviceDetectionPipelineBuilder, IpIntelligencePipelineBuilder,
        PerformanceProfile, DEVICE_DATA_KEY, IP_DATA_KEY,
    };

    /// An element that always fails.
    #[derive(Default)]
    struct AlwaysFails {
        filter: EvidenceKeyFilterWhitelist,
    }

    impl FlowElement for AlwaysFails {
        fn process(&self, _data: &mut FlowData) -> Result<()> {
            Err(Error::configuration("deliberate failure"))
        }
        fn data_key(&self) -> &str {
            "always-fails"
        }
        fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
            &self.filter
        }
        fn properties(&self) -> &[PropertyMetaData] {
            &[]
        }
    }

    /// What processing gave: its returned error, every value by data key and
    /// property name, and every error as its data key and message.
    #[derive(Debug, PartialEq)]
    struct Answers {
        returned: Option<String>,
        values: BTreeMap<String, BTreeMap<String, String>>,
        errors: Vec<(String, String)>,
    }

    fn answers(pipeline: &Arc<Pipeline>, evidence: Evidence) -> Answers {
        let mut data = pipeline.create_flow_data_with(evidence);
        let returned = data.process().err().map(|error| error.to_string());
        let values = data
            .data_keys()
            .into_iter()
            .map(|key| {
                let found = data.get_str(&key).unwrap();
                let properties = found
                    .keys()
                    .into_iter()
                    .map(|name| {
                        let value = format!("{:?}", found.get(&name));
                        (name, value)
                    })
                    .collect();
                (key, properties)
            })
            .collect();
        let errors = data
            .errors()
            .iter()
            .map(|error| (error.element_data_key.clone(), error.source.to_string()))
            .collect();
        Answers {
            returned,
            values,
            errors,
        }
    }

    /// The data file a variable names, or else the Lite file at `relative`.
    fn data_file(variable: &str, relative: &str) -> Option<PathBuf> {
        crate::deprecated_names::var(variable)
            .ok()
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .or_else(|| find_up(relative))
    }

    /// The element under `key` in a facade's pipeline.
    fn engine(pipeline: &Pipeline, key: &str) -> Arc<dyn FlowElement> {
        pipeline
            .flow_elements()
            .iter()
            .find(|element| element.data_key() == key)
            .cloned()
            .unwrap_or_else(|| panic!("the pipeline has an element under '{key}'"))
    }

    fn cases() -> Vec<(&'static str, Evidence)> {
        vec![
            (
                "an iPhone and an IPv4 address",
                Evidence::builder()
                    .add(
                        "header.user-agent",
                        "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
                         AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 \
                         Mobile/15E148 Safari/604.1",
                    )
                    .add("server.client-ip", "1.1.1.1")
                    .build(),
            ),
            (
                "Chrome with client hints and an IPv6 address",
                Evidence::builder()
                    .add(
                        "header.user-agent",
                        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
                         AppleWebKit/537.36 (KHTML, like Gecko) \
                         Chrome/120.0.0.0 Safari/537.36",
                    )
                    .add("header.sec-ch-ua-platform", "\"Windows\"")
                    .add("header.sec-ch-ua-platform-version", "\"15.0.0\"")
                    .add("server.client-ip", "2606:4700:4700::1111")
                    .build(),
            ),
            (
                "an address that does not parse",
                Evidence::builder()
                    .add("server.client-ip", "not an address")
                    .build(),
            ),
            ("no evidence", Evidence::default()),
        ]
    }

    #[test]
    fn dd_ipi_and_a_failing_element_answer_alike_in_a_group_and_in_sequence() {
        let dd_file = data_file(
            "_51DEGREES_DD_PATH",
            "cxx/data/device-detection/51Degrees-LiteV4.1.hash",
        );
        let ipi_file = data_file(
            "_51DEGREES_IPI_PATH",
            "cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi",
        );
        let (Some(dd_file), Some(ipi_file)) = (dd_file, ipi_file) else {
            eprintln!("skipping the parallel group test: both data files are required");
            return;
        };

        let dd_pipeline = DeviceDetectionPipelineBuilder::on_premise(&dd_file)
            .performance_profile(PerformanceProfile::HighPerformance)
            // Off so the test sends nothing to 51Degrees.
            .share_usage(false)
            // Off so the build needs no licence key.
            .auto_update(false)
            .build()
            .expect("device-detection on-premise pipeline builds");
        let ipi_pipeline = IpIntelligencePipelineBuilder::on_premise(&ipi_file)
            .performance_profile(PerformanceProfile::HighPerformance)
            // Off so the test sends nothing to 51Degrees.
            .share_usage(false)
            // Off so the build needs no licence key.
            .auto_update(false)
            .build()
            .expect("ip-intelligence on-premise pipeline builds");
        let elements: Vec<Arc<dyn FlowElement>> = vec![
            engine(&dd_pipeline, DEVICE_DATA_KEY.name()),
            engine(&ipi_pipeline, IP_DATA_KEY.name()),
            Arc::new(AlwaysFails::default()),
        ];

        for suppress in [false, true] {
            let mut in_sequence = Pipeline::builder().suppress_process_exceptions(suppress);
            for element in &elements {
                in_sequence = in_sequence.add_element(Arc::clone(element));
            }
            let in_sequence = in_sequence.build().unwrap();
            let in_group = Pipeline::builder()
                .suppress_process_exceptions(suppress)
                .add_parallel_elements(elements.iter().cloned())
                .build()
                .unwrap();
            assert!(in_group.is_concurrent());

            for (case, evidence) in cases() {
                let sequence = answers(&in_sequence, evidence.clone());
                let group = answers(&in_group, evidence);
                assert_eq!(group, sequence, "{case}, suppress {suppress}");
                assert!(
                    group.errors.contains(&(
                        "always-fails".to_owned(),
                        "pipeline configuration error: deliberate failure".to_owned()
                    )),
                    "{case}: {:?}",
                    group.errors
                );
                assert_eq!(group.returned.is_some(), !suppress, "{case}");
            }
        }

        let in_group = Pipeline::builder()
            .suppress_process_exceptions(true)
            .add_parallel_elements(elements)
            .build()
            .unwrap();
        let mut data = in_group.create_flow_data_with(cases().remove(0).1);
        data.process().unwrap();
        let device = data
            .get(DEVICE_DATA_KEY)
            .expect("the group ran Device Detection");
        assert!(
            *device.is_mobile().value().expect("IsMobile resolves"),
            "the iPhone user agent is mobile"
        );
        let ip = data
            .get(IP_DATA_KEY)
            .expect("the group ran IP Intelligence");
        let asn = ip.string("Asn");
        let value = asn.value().expect("Asn resolves");
        assert!(value.contains("AS13335"), "got {value}");
    }
}
