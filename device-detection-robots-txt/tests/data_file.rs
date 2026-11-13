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

//! The engine in a pipeline with the on-premise Device Detection engine.
//!
//! These run against the Lite Hash data file, or the file `_51DEGREES_DD_PATH`
//! names, and skip with a note when there is none. The Lite file holds no
//! crawler properties, so against it the text allows every crawler, and the
//! checks of each crawler run against a file that holds them, such as the
//! Enterprise file.

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_device_detection_onpremise::{
    DeviceDetectionOnPremiseEngine, DeviceDetectionOnPremiseEngineBuilder,
};
use fiftyone_device_detection_robots_txt::{
    RobotsTxtEngine, TdlSourceResolver, ROBOTS_TXT_DATA_KEY,
};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::AspectEngine;
use robotstxt::DefaultMatcher;
use url::Url;

const SITE: &str = "https://example.com/";

const TDL: &str = "https://m4ow.uk/socw/1.txt";

/// The Hash data file `_51DEGREES_DD_PATH` names, or the Lite file in the `cxx`
/// submodule.
fn data_file() -> Option<PathBuf> {
    if let Ok(path) = crate::deprecated_names::var("_51DEGREES_DD_PATH") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .to_path_buf();
    let roots = [workspace.clone(), workspace.parent()?.to_path_buf()];
    roots
        .into_iter()
        .map(|root| {
            root.join("cxx")
                .join("data")
                .join("device-detection")
                .join("51Degrees-LiteV4.1.hash")
        })
        .find(|path| path.is_file())
}

fn device_engine() -> Option<Arc<DeviceDetectionOnPremiseEngine>> {
    let Some(path) = data_file() else {
        eprintln!("no Hash data file found; skipping");
        return None;
    };
    Some(
        DeviceDetectionOnPremiseEngineBuilder::new(path)
            .performance_profile(PerformanceProfile::LowMemory)
            .build()
            .expect("the data file loads"),
    )
}

fn pipeline(
    device: &Arc<DeviceDetectionOnPremiseEngine>,
    robots: &Arc<RobotsTxtEngine>,
) -> Arc<Pipeline> {
    Pipeline::builder()
        .add_element(Arc::clone(device) as Arc<dyn FlowElement>)
        .add_element(Arc::clone(robots) as Arc<dyn FlowElement>)
        .build()
        .expect("the pipeline builds")
}

/// A robots.txt engine that resolves no TDL source ids, so no test needs the
/// network.
fn robots() -> Arc<RobotsTxtEngine> {
    RobotsTxtEngine::builder()
        .set_tdl_source_resolver(None)
        .build()
        .expect("the engine builds")
}

/// The plain and annotated text for the evidence given.
fn texts(pipeline: &Arc<Pipeline>, evidence: &[(&str, &str)]) -> Option<(String, String)> {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add_all(evidence.iter().map(|(key, value)| (*key, *value)))
            .build(),
    );
    data.process().expect("the request is processed");
    let robots = data.get(ROBOTS_TXT_DATA_KEY)?;
    Some((
        robots.plain_text().into_value().expect("plain text"),
        robots
            .annotated_text()
            .into_value()
            .expect("annotated text"),
    ))
}

/// True if a reader following RFC 9309 matches the token as the whole of a
/// product token, being letters, `_` and `-` only.
fn is_product_token(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == '_' || c == '-')
}

#[test]
fn the_crawlers_and_usages_are_read_from_the_data_file() {
    let Some(device) = device_engine() else {
        return;
    };
    let robots = robots();
    assert!(robots.model().is_none());
    assert!(!robots.properties_loaded());
    let _pipeline = pipeline(&device, &robots);
    let model = robots.model().expect("the model is read");
    let keys = robots.usage_evidence_keys();
    if device.property_value_count("CrawlerUsage").is_none() {
        eprintln!("the data file holds no crawler properties");
        assert!(model.crawlers().is_empty());
        assert!(model.usages().is_empty());
        assert!(keys.is_empty());
        return;
    }
    assert!(!model.crawlers().is_empty());
    assert!(!model.usages().is_empty());
    let expected: Vec<String> = model
        .usages()
        .iter()
        .filter(|usage| !usage.name.is_empty())
        .map(|usage| format!("query.robotstxt.{}", usage.name.to_lowercase()))
        .collect();
    assert_eq!(keys, expected);
    let filter = robots.evidence_key_filter();
    for key in &keys {
        assert!(filter.include(key), "{key}");
    }
    assert!(filter.include("query.robotstxt.tdl"));
    assert!(!filter.include("header.user-agent"));
}

#[test]
fn the_properties_have_the_device_detection_tier() {
    let Some(device) = device_engine() else {
        return;
    };
    let robots = robots();
    let _pipeline = pipeline(&device, &robots);
    assert!(robots.properties_loaded());
    assert_eq!(robots.data_source_tier(), device.data_source_tier());
    let names: Vec<&str> = robots
        .aspect_properties()
        .iter()
        .map(|p| p.name())
        .collect();
    assert_eq!(names, ["AnnotatedText", "PlainText"]);
    for property in robots.aspect_properties() {
        assert_eq!(property.core().category, "Bots");
        assert_eq!(property.element_data_key(), "robotstxt");
        assert_eq!(
            property.data_tiers_where_present(),
            [device.data_source_tier().to_owned()]
        );
        assert!(property.available());
    }
}

#[test]
fn no_evidence_writes_no_data() {
    let Some(device) = device_engine() else {
        return;
    };
    let robots = robots();
    let pipeline = pipeline(&device, &robots);
    assert_eq!(
        texts(&pipeline, &[("header.user-agent", "Mozilla/5.0")]),
        None
    );
    assert_eq!(texts(&pipeline, &[]), None);
}

#[test]
fn a_tdl_uri_is_written_and_an_entry_that_is_not_one_is_left_out() {
    let Some(device) = device_engine() else {
        return;
    };
    let robots = robots();
    let pipeline = pipeline(&device, &robots);
    let (plain, annotated) = texts(
        &pipeline,
        &[(
            "query.robotstxt.tdl",
            " https://m4ow.uk/socw/1.txt , not a uri|MOW-SOCW||",
        )],
    )
    .expect("the TDL evidence is enough to write the text");
    for text in [&plain, &annotated] {
        assert!(
            text.contains(&format!("# Terms {TDL}\nTDL: {TDL}\nAllow: /\n")),
            "{text}"
        );
        assert_eq!(text.matches("\nTDL: ").count(), 1, "{text}");
        assert!(text.ends_with("Allow: /\n"));
    }
}

/// Resolves `MOW-SOCW` to a fixed URI.
struct FixedResolver;

impl TdlSourceResolver for FixedResolver {
    fn is_known(&self, id: &str) -> bool {
        id.eq_ignore_ascii_case("MOW-SOCW")
    }

    fn resolve(&self, id: &str) -> Option<Url> {
        self.is_known(id)
            .then(|| Url::parse("https://m4ow.uk/socw/2.txt").unwrap())
    }
}

#[test]
fn a_tdl_source_id_is_given_to_the_resolver() {
    let Some(device) = device_engine() else {
        return;
    };
    let robots = RobotsTxtEngine::builder()
        .set_tdl_source_resolver(Some(Arc::new(FixedResolver)))
        .build()
        .unwrap();
    let pipeline = pipeline(&device, &robots);
    let (plain, _) = texts(
        &pipeline,
        &[("query.robotstxt.tdl", "mow-socw|other|https://example.org")],
    )
    .unwrap();
    assert!(plain.ends_with(
        "User-Agent: *\n\
         # Terms https://m4ow.uk/socw/2.txt\n\
         TDL: https://m4ow.uk/socw/2.txt\n\
         # Terms https://example.org/\n\
         TDL: https://example.org/\n\
         Allow: /\n"
    ));
}

#[test]
fn only_the_properties_asked_for_are_given() {
    let Some(device) = device_engine() else {
        return;
    };
    let robots = RobotsTxtEngine::builder()
        .set_tdl_source_resolver(None)
        .property("plaintext")
        .build()
        .unwrap();
    let pipeline = pipeline(&device, &robots);
    let names: Vec<&str> = robots
        .properties()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(names, ["PlainText"]);
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.robotstxt.tdl", TDL).build());
    data.process().unwrap();
    let robots = data.get(ROBOTS_TXT_DATA_KEY).expect("the data is written");
    assert!(robots.plain_text().has_value());
    assert!(!robots.annotated_text().has_value());
}

#[test]
fn each_crawler_reads_the_answer_the_usages_give() {
    let Some(device) = device_engine() else {
        return;
    };
    let robots = robots();
    let pipeline = pipeline(&device, &robots);
    let model = robots.model().unwrap();
    let keys = robots.usage_evidence_keys();
    for allow in [true, false] {
        let value = if allow { "allow" } else { "disallow" };
        let mut evidence: Vec<(&str, &str)> =
            keys.iter().map(|key| (key.as_str(), value)).collect();
        evidence.push(("query.robotstxt.tdl", TDL));
        let (plain, annotated) = texts(&pipeline, &evidence).expect("the text is written");
        let mut checked = 0;
        let mut skipped = 0;
        for crawler in model.crawlers() {
            for token in &crawler.product_tokens {
                let token = token.trim();
                if token.eq_ignore_ascii_case("N/A") || token.is_empty() {
                    continue;
                }
                if !is_product_token(token) {
                    skipped += 1;
                    continue;
                }
                for text in [&plain, &annotated] {
                    let mut matcher = DefaultMatcher::default();
                    assert_eq!(
                        matcher.one_agent_allowed_by_robots(text, token, SITE),
                        allow,
                        "{} ({token}) with every usage set to {value}",
                        crawler.name
                    );
                }
                checked += 1;
            }
        }
        for text in [&plain, &annotated] {
            let mut matcher = DefaultMatcher::default();
            assert!(matcher.one_agent_allowed_by_robots(text, "NoSuchCrawlerXyz", SITE));
        }
        eprintln!(
            "every usage set to {value}: {checked} product tokens checked, {skipped} that are \
             not RFC 9309 product tokens skipped"
        );
    }
}

#[test]
fn a_pipeline_without_device_detection_is_refused() {
    let robots = robots();
    let result = Pipeline::builder()
        .add_element(robots as Arc<dyn FlowElement>)
        .build();
    let error = result.err().expect("the pipeline is refused").to_string();
    assert!(error.contains("DeviceDetectionOnPremiseEngine"), "{error}");
}

#[test]
fn a_second_pipeline_needs_the_same_device_detection_engine() {
    let Some(device) = device_engine() else {
        return;
    };
    let robots = robots();
    let _first = pipeline(&device, &robots);
    let _second = pipeline(&device, &robots);
    let other = device_engine().unwrap();
    let result = Pipeline::builder()
        .add_element(other as Arc<dyn FlowElement>)
        .add_element(Arc::clone(&robots) as Arc<dyn FlowElement>)
        .build();
    assert!(result.is_err());
}
