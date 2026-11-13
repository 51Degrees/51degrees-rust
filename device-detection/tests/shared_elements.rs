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

//! One element instance in several pipelines.
//!
//! The on-premise Device Detection and IP Intelligence engines keep no state
//! for a pipeline, so one instance gives the same answers in every pipeline it
//! is in, and Set Headers keeps each pipeline's Accept-CH properties apart. The
//! same holds through an element that wraps another and passes every call on.
//!
//! The data files are found as the coexistence tests find them, and each test
//! skips when one is absent.

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use fiftyone_device_detection::{DeviceDetectionPipelineBuilder, PerformanceProfile};
use fiftyone_ip_intelligence::IpIntelligencePipelineBuilder;
use fiftyone_pipeline_core::{
    Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, MapElementData,
    Pipeline, PropertyMetaData, PropertyValueType, Result, TypedKey,
};
use fiftyone_pipeline_engines_fiftyone::SetHeadersElement;

/// The threads that use both pipelines at once.
const THREADS: usize = 16;

/// How many times each thread sends every request through the pipelines.
const ROUNDS: usize = 20;

const CHROME: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                      (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
const SAFARI: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) \
                      AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 \
                      Mobile/15E148 Safari/604.1";
const FIREFOX: &str = "Mozilla/5.0 (Android 14; Mobile; rv:131.0) Gecko/131.0 \
                       Firefox/131.0";
const CRAWLER: &str = "Mozilla/5.0 (compatible; Googlebot/2.1; \
                       +http://www.google.com/bot.html)";

/// User-Agent and client IP pairs sent through the pipelines.
const REQUESTS: [(&str, &str); 4] = [
    (CHROME, "1.1.1.1"),
    (SAFARI, "8.8.8.8"),
    (FIREFOX, "2001:4860:4860::8888"),
    (CRAWLER, "81.2.69.142"),
];

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

/// The file named by `variable`, or else the one found at `relative`.
fn data_file(variable: &str, relative: &str) -> Option<PathBuf> {
    crate::deprecated_names::var(variable)
        .ok()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| find_up(relative))
}

/// The Device Detection facade's elements, with usage sharing and updates off
/// so the test sends nothing and needs no licence key.
fn device_detection() -> Option<Vec<Arc<dyn FlowElement>>> {
    let Some(file) = data_file(
        "_51DEGREES_DD_PATH",
        "cxx/data/device-detection/51Degrees-LiteV4.1.hash",
    ) else {
        eprintln!("skipping: no Device Detection Hash data file found");
        return None;
    };
    let pipeline = DeviceDetectionPipelineBuilder::on_premise(file)
        .performance_profile(PerformanceProfile::LowMemory)
        .share_usage(false)
        .auto_update(false)
        .build()
        .expect("the Device Detection pipeline builds");
    Some(pipeline.flow_elements().to_vec())
}

/// The IP Intelligence facade's elements, built as [`device_detection`] builds
/// its own.
fn ip_intelligence() -> Option<Vec<Arc<dyn FlowElement>>> {
    let Some(file) = data_file(
        "_51DEGREES_IPI_PATH",
        "cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi",
    ) else {
        eprintln!("skipping: no IP Intelligence data file found");
        return None;
    };
    let pipeline = IpIntelligencePipelineBuilder::on_premise(file)
        .performance_profile(PerformanceProfile::LowMemory)
        .share_usage(false)
        .auto_update(false)
        .build()
        .expect("the IP Intelligence pipeline builds");
    Some(pipeline.flow_elements().to_vec())
}

fn pipeline_of(elements: &[Arc<dyn FlowElement>]) -> Arc<Pipeline> {
    let mut builder = Pipeline::builder();
    for element in elements {
        builder = builder.add_element(Arc::clone(element));
    }
    builder.build().expect("the pipeline builds")
}

/// An element that wraps another and passes every call to it.
struct Wrapped(Arc<dyn FlowElement>);

impl FlowElement for Wrapped {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        self.0.process(data)
    }
    fn data_key(&self) -> &str {
        self.0.data_key()
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.0.evidence_key_filter()
    }
    fn properties(&self) -> &[PropertyMetaData] {
        self.0.properties()
    }
    fn is_concurrent(&self) -> bool {
        self.0.is_concurrent()
    }
    fn type_name(&self) -> &'static str {
        self.0.type_name()
    }
    fn add_pipeline(&self, pipeline: &Arc<Pipeline>) -> Result<()> {
        self.0.add_pipeline(pipeline)
    }
}

fn wrapped(elements: &[Arc<dyn FlowElement>]) -> Vec<Arc<dyn FlowElement>> {
    elements
        .iter()
        .map(|element| Arc::new(Wrapped(Arc::clone(element))) as Arc<dyn FlowElement>)
        .collect()
}

/// Every value `elements` gave for one request through `pipeline`, one line
/// per property.
fn answer(
    pipeline: &Arc<Pipeline>,
    elements: &[Arc<dyn FlowElement>],
    (user_agent, client_ip): (&str, &str),
) -> String {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", user_agent)
            .add("server.client-ip", client_ip)
            .build(),
    );
    data.process().expect("the request is processed");
    let mut lines = Vec::new();
    for element in elements {
        let key = element.data_key();
        let Some(values) = data.get_str(key) else {
            continue;
        };
        let mut names = values.keys();
        names.sort();
        for name in names {
            lines.push(format!("{key}.{name} = {:?}", values.get(&name)));
        }
    }
    lines.join("\n")
}

#[test]
fn engines_in_two_pipelines_answer_as_in_one() {
    let (Some(device), Some(ip)) = (device_detection(), ip_intelligence()) else {
        return;
    };
    let engines: Vec<Arc<dyn FlowElement>> = device.iter().chain(&ip).cloned().collect();
    let alone = pipeline_of(&engines);
    let expected: Vec<String> = REQUESTS
        .iter()
        .map(|request| answer(&alone, &engines, *request))
        .collect();
    assert!(expected
        .iter()
        .all(|lines| lines.contains("device.ismobile")));
    assert!(expected.iter().all(|lines| lines.contains("ip.")));

    for wrap in [false, true] {
        let (device, ip) = if wrap {
            (wrapped(&device), wrapped(&ip))
        } else {
            (device.clone(), ip.clone())
        };
        // The same instances in two pipelines, in a different order in each.
        let first: Vec<_> = device.iter().chain(&ip).cloned().collect();
        let second: Vec<_> = ip.iter().chain(&device).cloned().collect();
        let pipelines = [pipeline_of(&first), pipeline_of(&second)];

        thread::scope(|scope| {
            for thread in 0..THREADS {
                let (pipelines, engines, expected) = (&pipelines, &engines, &expected);
                scope.spawn(move || {
                    for round in 0..ROUNDS {
                        for (index, request) in REQUESTS.iter().enumerate() {
                            let pipeline = &pipelines[(thread + round + index) % 2];
                            assert_eq!(
                                answer(pipeline, engines, *request),
                                expected[index],
                                "wrapped {wrap}, {request:?}"
                            );
                        }
                    }
                });
            }
        });
    }
}

/// An element that publishes `SetHeaderBrowserAccept-CH` as `X-Test`.
struct TestHeaderElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl TestHeaderElement {
    const KEY: TypedKey<MapElementData> = TypedKey::new("test");

    fn new() -> Self {
        TestHeaderElement {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: vec![PropertyMetaData::new(
                "SetHeaderBrowserAccept-CH",
                Self::KEY.name(),
                PropertyValueType::String,
            )],
        }
    }
}

impl FlowElement for TestHeaderElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.get_or_add(Self::KEY, || {
            MapElementData::new().set("SetHeaderBrowserAccept-CH", "X-Test")
        })?;
        Ok(())
    }
    fn data_key(&self) -> &str {
        Self::KEY.name()
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// The Accept-CH value of one request with `user_agent` through `pipeline`.
fn accept_ch(pipeline: &Arc<Pipeline>, user_agent: &str) -> Option<String> {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.user-agent", user_agent)
            .build(),
    );
    data.process().expect("the request is processed");
    data.get(SetHeadersElement::KEY)
        .expect("Set Headers ran")
        .response_headers()
        .get("Accept-CH")
        .cloned()
}

#[test]
fn set_headers_in_two_pipelines_answers_each_from_its_own() {
    let Some(device) = device_detection() else {
        return;
    };
    for wrap in [false, true] {
        let shared: Arc<dyn FlowElement> = Arc::new(SetHeadersElement::new());
        let (device, shared) = if wrap {
            (
                wrapped(&device),
                Arc::new(Wrapped(shared)) as Arc<dyn FlowElement>,
            )
        } else {
            (device.clone(), shared)
        };
        let test: Arc<dyn FlowElement> = Arc::new(TestHeaderElement::new());
        let alone: Arc<dyn FlowElement> = Arc::new(SetHeadersElement::new());
        let a = pipeline_of(&[device.as_slice(), &[Arc::clone(&shared)]].concat());
        let b = pipeline_of(&[test, shared]);
        let alone = pipeline_of(&[device.as_slice(), &[alone]].concat());

        for user_agent in [CHROME, SAFARI] {
            // B first, so a single kept scan would be B's.
            assert_eq!(accept_ch(&b, user_agent).as_deref(), Some("X-Test"));
            let from_a = accept_ch(&a, user_agent);
            eprintln!("wrapped {wrap}, Accept-CH {from_a:?} for {user_agent}");
            assert!(!from_a.as_deref().unwrap_or_default().contains("X-Test"));
            assert_eq!(from_a, accept_ch(&alone, user_agent), "wrapped {wrap}");
        }
        // Chrome sends client hints when asked, so its answer asks for some.
        assert!(accept_ch(&a, CHROME).is_some_and(|value| value.contains("Sec-CH-UA")));
    }
}
