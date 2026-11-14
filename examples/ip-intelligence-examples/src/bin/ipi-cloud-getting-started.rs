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

//! @page ipi-cloud-getting-started-example Getting Started (IP Intelligence, Cloud)
//!
//! Cloud IP-intelligence getting-started console example.
//!
//! See the descriptive block at the bottom of this file for the full narrative.
//!
//! @snippet ipi-cloud-getting-started.rs example

use std::sync::Arc;

use anyhow::{Context, Result};
use fiftyone_ip_intelligence::{
    AspectPropertyValue, IpIntelligenceData, IpIntelligencePipelineBuilder, WeightedValue,
    IP_DATA_KEY,
};
use fiftyone_pipeline_core::{Evidence, Pipeline};

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-getting-started.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-getting-started.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-getting-started.rs&utm_term=find-out-more"),
];

/// The evidence key under which a client IP address is supplied to the cloud
/// pipeline. The cloud service accepts the query-prefixed 51Degrees client-IP
/// key, so both the IPv4 and IPv6 sample below are added under this one key.
const CLIENT_IP_EVIDENCE_KEY: &str = "query.client-ip-51d";

/// Options the example runs with, gathered so `main` and the test can call
/// [`run`] the same way through a single entry point.
pub struct Options {
    /// The cloud resource key that authenticates the request and selects the
    /// properties returned.
    pub resource_key: String,
    /// An optional override for the cloud endpoint. When `None` the public
    /// 51Degrees cloud endpoint is used.
    pub endpoint: Option<String>,
}

// [example]
/// The example logic: build a cloud IP-intelligence pipeline from a resource
/// key, then analyse an IPv4 and an IPv6 address, printing the network and
/// location properties for each.
pub fn run(options: Options) -> Result<()> {
    // Build the cloud pipeline. This is the only crate an application needs to
    // depend on to talk to the 51Degrees cloud service. A console example never
    // adds the ShareUsageElement; a production deployment should enable usage
    // sharing so the data behind these results keeps improving.
    let mut builder = IpIntelligencePipelineBuilder::cloud(options.resource_key);
    if let Some(endpoint) = options.endpoint {
        builder = builder.endpoint(endpoint);
    }
    let pipeline = builder
        .build()
        .context("failed to build the cloud IP-intelligence pipeline")?;

    // Run a representative IPv4 and IPv6 address through the pipeline. IP
    // Intelligence is location and network aware, so a single address maps to a
    // set of network and location properties.
    analyse_ip(&pipeline, "185.28.167.77")?;
    analyse_ip(&pipeline, "2001:4860:4860::8888")?;

    Ok(())
}
// [example]

/// Process a single IP address and print the IP-intelligence results.
fn analyse_ip(pipeline: &Arc<Pipeline>, ip: &str) -> Result<()> {
    println!("Input values:");
    println!("\t{CLIENT_IP_EVIDENCE_KEY}: {ip}");
    println!();

    // FlowData carries the evidence into the pipeline and the results out of it.
    // We add the client IP as evidence, then process.
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add(CLIENT_IP_EVIDENCE_KEY, ip).build());
    data.process()
        .with_context(|| format!("failed to process the IP address {ip}"))?;

    // The result is read through the shared IpIntelligenceData accessors. Each
    // accessor returns an AspectPropertyValue wrapping a single plain value, so
    // we go through small helpers that print the value or its no-value message.
    let ip_data = data
        .get(IP_DATA_KEY)
        .context("the pipeline did not populate any IP-intelligence data")?;

    println!("Results:");
    // Network properties: who the IP range is registered to and its bounds.
    print_string_property("RegisteredName", &ip_data.registered_name());
    print_string_property("RegisteredOwner", &ip_data.registered_owner());
    print_string_property("RegisteredCountry", &ip_data.registered_country());
    print_string_property("IpRangeStart", &ip_data.ip_range_start());
    print_string_property("IpRangeEnd", &ip_data.ip_range_end());
    // Location properties: where the address most probably is.
    print_string_property("Country", &ip_data.country());
    print_string_property("CountryCode", &ip_data.country_code());
    print_string_property("CountryCode3", &ip_data.country_code3());
    print_string_property("Region", &ip_data.region());
    print_string_property("State", &ip_data.state());
    print_string_property("Town", &ip_data.town());
    print_double_property("Latitude", &ip_data.latitude());
    print_double_property("Longitude", &ip_data.longitude());
    print_integer_property("AccuracyRadiusMin", &ip_data.accuracy_radius_min());
    print_integer_property("TimeZoneOffset", &ip_data.time_zone_offset());
    // Network flags, read as typed booleans.
    print_bool_property("IsHosted", &ip_data.is_hosted());
    print_bool_property("IsVPN", &ip_data.is_vpn());
    // The weighted country-code distributions: a single IP can overlap several
    // countries, so each resolves to a weighted list ordered most probable first.
    print_weighted_property(
        "CountryCodesGeographical",
        &ip_data.country_codes_geographical(),
    );
    print_weighted_property(
        "CountryCodesPopulation",
        &ip_data.country_codes_population(),
    );
    print_weighted_property("Mcc", &ip_data.mcc());
    println!();

    Ok(())
}

/// Print a plain string property, or its no-value message when absent.
fn print_string_property(name: &str, property: &AspectPropertyValue<String>) {
    match property.value() {
        Ok(value) => println!("\t{name}: {value}"),
        Err(error) => println!("\t{name}: {error}"),
    }
}

/// Print a plain floating-point property (latitude or longitude).
fn print_double_property(name: &str, property: &AspectPropertyValue<f32>) {
    match property.value() {
        Ok(value) => println!("\t{name}: {value:.4}"),
        Err(error) => println!("\t{name}: {error}"),
    }
}

/// Print a plain integer property (accuracy radius or time-zone offset).
fn print_integer_property(name: &str, property: &AspectPropertyValue<i32>) {
    match property.value() {
        Ok(value) => println!("\t{name}: {value}"),
        Err(error) => println!("\t{name}: {error}"),
    }
}

/// Print a plain boolean property (a network flag).
fn print_bool_property(name: &str, property: &AspectPropertyValue<bool>) {
    match property.value() {
        Ok(value) => println!("\t{name}: {value}"),
        Err(error) => println!("\t{name}: {error}"),
    }
}

/// Print a weighted property (the country-code distributions, `Mcc`) as a
/// comma-separated list of `value (weight)` candidates ordered most probable
/// first, or its no-value message when absent.
fn print_weighted_property(name: &str, property: &AspectPropertyValue<Vec<WeightedValue<String>>>) {
    match property.value() {
        Ok(list) => {
            let rendered = list
                .iter()
                .map(|candidate| format!("{} ({:.2})", candidate.value, candidate.weighting()))
                .collect::<Vec<_>>()
                .join(", ");
            println!("\t{name}: {rendered}");
        }
        Err(error) => println!("\t{name}: {error}"),
    }
}

/// Read the resource key and optional endpoint from the environment (or the
/// first command-line argument) and run the example. Without a key the example
/// prints how to obtain one and exits successfully, so it is safe to run offline.
fn main() -> Result<()> {
    let resource_key = std::env::args()
        .nth(1)
        .or_else(examples_shared::resource_key_from_env);

    let Some(resource_key) = resource_key else {
        println!(
            "No resource key available. Set the _51DEGREES_RESOURCE_KEY environment \
             variable or pass a key as the first argument. Create a key for free at \
             https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-getting-started.rs&utm_term=resource-key-required."
        );

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

    /// The example runs end to end when a resource key is available. It is
    /// ignored by default so a plain `cargo test` stays green offline; supply a
    /// key and run with `--ignored` to exercise the live cloud path.
    #[test]
    #[ignore = "requires a live 51Degrees cloud resource key"]
    fn runs_against_the_cloud() {
        let resource_key =
            examples_shared::resource_key_from_env().expect("a resource key must be set");
        run(Options {
            resource_key,
            endpoint: examples_shared::cloud_endpoint_from_env(),
        })
        .expect("the cloud getting-started example should complete");
    }
}

/*
 * @example ipi-cloud-getting-started.rs
 *
 * This example shows how to use the 51Degrees Cloud IP Intelligence service to
 * determine location and network details from an IP address.
 *
 * You will learn:
 *
 * 1. How to build a Pipeline that uses 51Degrees Cloud IP Intelligence from a
 *    resource key.
 * 2. How to pass an IP address (evidence) to the Pipeline as flow data.
 * 3. How to read the results, where each property resolves to a single plain
 *    typed value (a string, a float or an integer).
 *
 * Most IP Intelligence properties resolve to one value for a given IP address.
 * This example prints each value directly, or a "no value" explanation when the
 * service does not return it for the address.
 *
 * The example processes one IPv4 and one IPv6 address and prints the registered
 * network properties (RegisteredName, RegisteredOwner, RegisteredCountry, the IP
 * range bounds) and the location properties (Country, CountryCode, CountryCode3,
 * Region, State, Town, Latitude, Longitude, AccuracyRadiusMin, TimeZoneOffset).
 *
 * This is a console example, so it does not add the ShareUsageElement. A
 * production deployment should enable usage sharing so the data behind these
 * results keeps improving.
 *
 * To run this example, create a Resource Key for free at
 * https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-getting-started.rs&utm_term=ipi-cloud-getting-started and supply it via the _51DEGREES_RESOURCE_KEY
 * environment variable or as the first command-line argument. By default the
 * pipeline talks to the public cloud endpoint; set the _51DEGREES_CLOUD_ENDPOINT
 * environment variable to point at a self-hosted Cloud service instead.
 */
