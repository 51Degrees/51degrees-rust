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

//! @page ipi-cloud-get-all-properties-example Get All Properties (IP Intelligence, Cloud)
//!
//! Cloud IP-intelligence get-all-properties console example.
//!
//! See the descriptive block at the bottom of this file for the full narrative.
//!
//! @snippet ipi-cloud-get-all-properties.rs example

use anyhow::{Context, Result};
use fiftyone_ip_intelligence::{IpIntelligencePipelineBuilder, IP_DATA_KEY};
use fiftyone_pipeline_core::{ElementData, Evidence, PropertyValue};

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-get-all-properties.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-get-all-properties.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-get-all-properties.rs&utm_term=find-out-more"),
];

/// The evidence key under which the client IP address is supplied to the cloud
/// pipeline.
const CLIENT_IP_EVIDENCE_KEY: &str = "query.client-ip-51d";

/// A sample IP address to look up. Any of the example addresses returns a full
/// spread of weighted properties.
const SAMPLE_IP: &str = "8.8.8.8";

/// Maximum length to print for any single rendered value, so a long weighted
/// distribution stays readable. Values are truncated at 200 characters.
const MAX_VALUE_LENGTH: usize = 200;

/// Options the example runs with, gathered so `main` and the test call [`run`]
/// the same way.
pub struct Options {
    /// The cloud resource key whose every returned property is printed.
    pub resource_key: String,
    /// An optional override for the cloud endpoint. When `None` the public
    /// 51Degrees cloud endpoint is used.
    pub endpoint: Option<String>,
}

// [example]
/// The example logic: build a cloud IP-intelligence pipeline, process one IP
/// address, then enumerate every property the result holds, printing each with
/// its weighted values.
pub fn run(options: Options) -> Result<()> {
    // Build the cloud pipeline. A console example never adds the
    // ShareUsageElement; a production deployment should enable usage sharing.
    let mut builder = IpIntelligencePipelineBuilder::cloud(options.resource_key);
    if let Some(endpoint) = options.endpoint {
        builder = builder.endpoint(endpoint);
    }
    let pipeline = builder
        .build()
        .context("failed to build the cloud IP-intelligence pipeline")?;

    // Add the client IP as evidence and process it.
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add(CLIENT_IP_EVIDENCE_KEY, SAMPLE_IP)
            .build(),
    );
    data.process()
        .with_context(|| format!("failed to process the IP address {SAMPLE_IP}"))?;

    let ip_data = data
        .get(IP_DATA_KEY)
        .context("the pipeline did not populate any IP-intelligence data")?;

    println!("What property values are associated with the IP '{SAMPLE_IP}'?");

    // Iterate every property in the dynamic property bag rather than the fixed
    // set of typed accessors, so this prints whatever the resource key returns.
    // The keys are sorted for a stable, readable ordering.
    let mut names = ip_data.keys();
    names.sort();
    for name in names {
        match ip_data.get(&name) {
            Ok(value) => println!("{name} = {}", render_value(&value)),
            // A property present in the key set but with no value still prints,
            // so the full returned shape is visible.
            Err(error) => println!("{name} = NO VALUE ({error})"),
        }
    }

    Ok(())
}
// [example]

/// Render any property value for display.
///
/// IP-intelligence properties are weighted, so in the dynamic bag they appear as
/// a `KeyValueList` of `{rawweighting, value}` records. This renders each
/// record as `value (weighting w)` so the weighting is visible, and falls back
/// to the shared string rendering for any non-weighted property the key might
/// also return. Long results are truncated.
fn render_value(value: &PropertyValue) -> String {
    let rendered = match value {
        PropertyValue::KeyValueList(records) => {
            let parts: Vec<String> = records
                .iter()
                .map(|record| {
                    // Each record carries a `value` candidate and its raw
                    // weighting, from 0 to 65,535, under `rawweighting`. The
                    // crates up to 4.5.18 give a weighting from 0 to 1 under
                    // `weight` instead, so that is read too.
                    let candidate = record
                        .get("value")
                        .map(examples_shared::property_value_to_string)
                        .unwrap_or_else(|| "Unknown".to_owned());
                    let weighting = match record.get("rawweighting") {
                        Some(raw) => raw.as_double().map(|raw| raw / f64::from(u16::MAX)),
                        None => record.get("weight").and_then(PropertyValue::as_double),
                    };
                    match weighting {
                        Some(weighting) => format!("{candidate} (weighting {weighting:.3})"),
                        None => candidate,
                    }
                })
                .collect();
            parts.join(", ")
        }
        // Any non-weighted property renders through the shared string helper,
        // which understands every PropertyValue variant.
        other => examples_shared::property_value_to_string(other),
    };

    if rendered.chars().count() > MAX_VALUE_LENGTH {
        // Truncate on a character boundary so multi-byte location names do not
        // panic the slice.
        let truncated: String = rendered.chars().take(MAX_VALUE_LENGTH).collect();
        format!("{truncated}...")
    } else {
        rendered
    }
}

/// Read the resource key and optional endpoint and run the example. Without a
/// key the example prints how to obtain one and exits successfully.
fn main() -> Result<()> {
    let resource_key = std::env::args()
        .nth(1)
        .or_else(examples_shared::resource_key_from_env);

    let Some(resource_key) = resource_key else {
        println!(
            "No resource key available. Set the _51DEGREES_RESOURCE_KEY environment \
             variable or pass a key as the first argument. Create a key for free at \
             https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-get-all-properties.rs&utm_term=resource-key-required."
        );
        examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::CLOUD_REPOSITORIES);
        return Ok(());
    };

    let result = run(Options {
        resource_key,
        // The cloud endpoint from _51DEGREES_CLOUD_ENDPOINT, or the public cloud
        // when unset. A host other than cloud.51degrees.com is an on premise web
        // server or a privately hosted 51Degrees cloud (see
        // examples_shared::CLOUD_ENDPOINT_ENV_VAR).
        endpoint: examples_shared::cloud_endpoint_from_env(),
    });
    examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::CLOUD_REPOSITORIES);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example runs end to end when a resource key is available. Ignored by
    /// default so a plain `cargo test` stays green offline.
    #[test]
    #[ignore = "requires a live 51Degrees cloud resource key"]
    fn runs_against_the_cloud() {
        let resource_key =
            examples_shared::resource_key_from_env().expect("a resource key must be set");
        run(Options {
            resource_key,
            endpoint: examples_shared::cloud_endpoint_from_env(),
        })
        .expect("the cloud get-all-properties example should complete");
    }

    /// A weighted KeyValueList renders each candidate with its weighting. This
    /// exercises the renderer offline, without any cloud call.
    #[test]
    fn renders_weighted_records() {
        use std::collections::BTreeMap;

        let mut record = BTreeMap::new();
        record.insert("value".to_owned(), PropertyValue::String("US".to_owned()));
        record.insert("rawweighting".to_owned(), PropertyValue::Integer(59_965));
        let value = PropertyValue::KeyValueList(vec![record]);
        assert_eq!(render_value(&value), "US (weighting 0.915)");

        let mut record = BTreeMap::new();
        record.insert("value".to_owned(), PropertyValue::String("US".to_owned()));
        record.insert("weight".to_owned(), PropertyValue::Double(0.915));
        let value = PropertyValue::KeyValueList(vec![record]);
        assert_eq!(render_value(&value), "US (weighting 0.915)");
    }
}

/*
 * @example ipi-cloud-get-all-properties.rs
 *
 * This example shows how to retrieve every IP-intelligence property a resource
 * key returns from the 51Degrees Cloud service, with the weighting on each
 * value.
 *
 * You will learn:
 *
 * 1. How to build a Pipeline that uses 51Degrees Cloud IP Intelligence from a
 *    resource key.
 * 2. How to process an IP address and read the result.
 * 3. How to enumerate every property the result holds through the dynamic
 *    property bag, rather than the fixed set of typed accessors, so the example
 *    prints whatever the resource key returns.
 *
 * IP Intelligence properties are weighted, so each property can carry several
 * candidate values, each with a weighting that says how strongly the data
 * supports it. In the dynamic bag each weighted property appears as a list of
 * rawweighting/value records, and this example renders each candidate as
 * `value (weighting w)` so the distribution is visible. Long results are
 * truncated for readability.
 *
 * This is a console example, so it does not add the ShareUsageElement. A
 * production deployment should enable usage sharing.
 *
 * To run this example, create a Resource Key for free at
 * https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-get-all-properties.rs&utm_term=ipi-cloud-get-all-properties and supply it via the _51DEGREES_RESOURCE_KEY
 * environment variable or as the first command-line argument. By default the
 * pipeline talks to the public cloud endpoint; set the _51DEGREES_CLOUD_ENDPOINT
 * environment variable to point at a self-hosted Cloud service instead.
 */
