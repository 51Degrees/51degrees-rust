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

//! Latitude, Longitude and every point of Areas are given to 3 decimal
//! places, as a number, as text and as a point in a WKT shape.
//!
//! Only the Enterprise data file carries these properties, so the test runs
//! when `51DEGREES_IPI_PATH` names a file that carries Latitude and skips
//! otherwise.

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_ip_intelligence_onpremise::{
    IpIntelligenceData, IpIntelligenceOnPremiseEngineBuilder, IP_DATA_KEY,
};
use fiftyone_native::ipi::Manager;
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};

/// Public addresses in both address families with a location in the
/// Enterprise data file.
const ADDRESSES: &[&str] = &[
    "185.28.167.77",
    "81.2.69.142",
    "102.89.32.1",
    "1.1.1.1",
    "8.8.8.8",
    "2a00:23c5:a1a6:a01::1",
    "2001:4860:4860::8888",
];

/// The separator the C library's text joins the values of a property with.
const SEPARATOR: &str = "|";

/// The decimal places the specification gives a location to.
const PLACES: usize = 3;

/// The data file `51DEGREES_IPI_PATH` names, if it exists.
fn data_file() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var("51DEGREES_IPI_PATH").ok()?.trim());
    path.is_file().then_some(path)
}

/// Fail when a number's text has more than 3 decimal places.
fn assert_places(text: &str, what: &str) {
    let places = text
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    assert!(
        places <= PLACES,
        "{what} is {text}, with {places} decimal places"
    );
}

/// The values in the C library's text for a property, which writes each as
/// `"<value>":<weighting>`, most probable first, joined by [`SEPARATOR`].
fn quoted_values(text: &str) -> Vec<&str> {
    text.split(SEPARATOR)
        .filter_map(|item| {
            let (value, _) = item.strip_prefix('"')?.split_once("\":")?;
            Some(value)
        })
        .collect()
}

#[test]
fn locations_are_given_to_three_decimal_places() {
    let Some(file) = data_file() else {
        eprintln!("51DEGREES_IPI_PATH names no data file; skipping");
        return;
    };
    let manager =
        Manager::open(&file, PerformanceProfile::LowMemory).expect("the data file should load");
    if !manager
        .available_property_names()
        .iter()
        .any(|name| name == "Latitude")
    {
        eprintln!("{} carries no Latitude; skipping", file.display());
        return;
    }
    let engine = IpIntelligenceOnPremiseEngineBuilder::new(&file)
        .performance_profile(PerformanceProfile::LowMemory)
        .properties(["Latitude", "Longitude", "Areas"])
        .auto_update(false)
        .file_system_watcher(false)
        .build()
        .expect("the engine should build");
    let element: Arc<dyn FlowElement> = Arc::new(engine);
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("the pipeline should build");
    let mut results = manager
        .create_results()
        .expect("the results should allocate");

    let mut numbers = 0;
    let mut points = 0;
    for ip in ADDRESSES {
        let mut data = pipeline
            .create_flow_data_with(Evidence::builder().add("server.client-ip", *ip).build());
        data.process().expect("processing should succeed");
        let ip_data = data.get(IP_DATA_KEY).expect("the IP data should be there");
        results.process_ip(ip).expect("the lookup should run");

        for (name, value) in [
            ("Latitude", ip_data.latitude()),
            ("Longitude", ip_data.longitude()),
        ] {
            let Ok(number) = value.value() else {
                continue;
            };
            let what = format!("{name} for {ip}");
            assert_places(&number.to_string(), &what);

            // The C library's text, written with the same places as the text
            // .NET reads the number from.
            let text = results
                .value_as_string(name, SEPARATOR)
                .expect("the text should read")
                .expect("the text should have a value");
            let values = quoted_values(&text);
            assert!(!values.is_empty(), "the text of {what} is {text}");
            for value in &values {
                assert_places(value, &format!("a value in the text of {what}"));
            }
            assert_eq!(
                Some(*number),
                values[0].parse::<f32>().ok(),
                "{what} should be the float nearest the first value in {text}"
            );

            for (candidate, _) in results
                .values_weighted(name)
                .expect("the values should read")
            {
                assert_places(&candidate, &format!("a weighted value of {what}"));
            }
            numbers += 1;
        }

        if let Ok(areas) = ip_data.areas().value() {
            for point in areas
                .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
                .filter(|token| token.chars().any(|c| c.is_ascii_digit()))
            {
                assert_places(point, &format!("a point of Areas for {ip}"));
                points += 1;
            }
        }
    }
    assert!(numbers > 0, "no latitude or longitude was given");
    assert!(points > 0, "no Areas point was given");
    eprintln!("checked {numbers} latitudes and longitudes and {points} Areas numbers");
}
