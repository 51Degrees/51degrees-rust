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

//! @page ipi-cloud-mixed-getting-started-example Mixed Getting Started (IP Intelligence, Cloud)
//!
//! Mixed cloud example: Device Detection and IP Intelligence in one pipeline.
//!
//! Builds a single pipeline that runs both the cloud Device Detection engine and
//! the cloud IP Intelligence engine over one shared cloud request engine, so a
//! User-Agent and a client IP supplied together are resolved in one cloud call.
//! The descriptive block the documentation tooling renders lives at the bottom of
//! the file.
//!
//! @snippet ipi-cloud-mixed-getting-started.rs example

use std::io::Write;
use std::sync::Arc;

use anyhow::Context;
use fiftyone_cloud_request_engine::CloudRequestEngine;
use fiftyone_device_detection_cloud::{DeviceData, DeviceDetectionCloudEngine, DEVICE_DATA_KEY};
use fiftyone_ip_intelligence_cloud::{IpIntelligenceCloudEngine, IpIntelligenceData, IP_DATA_KEY};
use fiftyone_pipeline_core::{Evidence, Pipeline};
use fiftyone_pipeline_engines::AspectPropertyValue;

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-mixed-getting-started.rs&utm_term=find-out-more"),
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-mixed-getting-started.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-mixed-getting-started.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-mixed-getting-started.rs&utm_term=find-out-more"),
];

/// The evidence key under which the User-Agent is supplied. The cloud Device
/// Detection engine reads it after the cloud request engine forwards it.
const USER_AGENT_EVIDENCE_KEY: &str = "header.user-agent";

/// The evidence key under which the client IP is supplied. This is the
/// query-prefixed 51Degrees client-IP key the cloud IP Intelligence engine
/// expects (confirmed from the ip-intelligence-cloud engine's live test).
const CLIENT_IP_EVIDENCE_KEY: &str = "query.client-ip-51d";

/// One sample request: a User-Agent and a client IP, with a short label that
/// describes the device and location pairing so the output reads clearly.
struct Sample {
    /// A human-readable label for the pairing, printed before the values.
    label: &'static str,
    /// The User-Agent string supplied as device evidence.
    user_agent: &'static str,
    /// The client IP supplied as IP Intelligence evidence.
    client_ip: &'static str,
}

/// The five device-and-IP pairings this example processes: a UK desktop, a
/// Chinese iPhone, a Chilean desktop, an iPad on an IPv6 address and a US
/// Android device.
const SAMPLES: &[Sample] = &[
    Sample {
        label: "Desktop from the UK",
        user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                     (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36",
        client_ip: "82.12.34.23",
    },
    Sample {
        label: "iPhone from China",
        user_agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 14_6 like Mac OS X) \
                     AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0.3 \
                     Mobile/15E148 Safari/604.1",
        client_ip: "1.3.32.31",
    },
    Sample {
        label: "Desktop from Chile",
        user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                     (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36",
        client_ip: "45.236.48.61",
    },
    Sample {
        label: "iPad on an IPv6 address",
        user_agent: "Mozilla/5.0 (iPad; CPU OS 14_6 like Mac OS X) AppleWebKit/605.1.15 \
                     (KHTML, like Gecko) CriOS/91.0.4472.80 Mobile/15E148 Safari/604.1",
        client_ip: "2001:0db8:085a:0000:0000:8a2e:0370:7334",
    },
    Sample {
        label: "Android device from the USA",
        user_agent: "Mozilla/5.0 (Linux; Android 11; SM-G973F) AppleWebKit/537.36 \
                     (KHTML, like Gecko) Chrome/91.0.4472.120 Mobile Safari/537.36",
        client_ip: "8.8.8.8",
    },
];

/// The options that drive a run of this example.
pub struct Options {
    /// The cloud resource key that authenticates the request and selects the
    /// properties returned. The key must grant both the device and IP
    /// intelligence products for this example to print both result sets.
    pub resource_key: String,
    /// An optional override for the cloud endpoint. When `None` the public
    /// 51Degrees cloud endpoint is used.
    pub endpoint: Option<String>,
}

// [example]
/// Run the mixed example against the supplied options, writing all output to
/// `out`.
///
/// One cloud request engine is created from the resource key. A Device Detection
/// cloud engine and an IP Intelligence cloud engine are then both built to read
/// from that one request engine, and all three elements are added to a single
/// pipeline. Each sample User-Agent and client IP is processed together, so the
/// single cloud call behind the pipeline returns both products at once. This is a
/// console example, so it does not add the ShareUsageElement. A production
/// deployment, and every web example, should enable usage sharing so the data
/// behind these results keeps improving.
pub fn run(options: &Options, out: &mut dyn Write) -> anyhow::Result<()> {
    // The single cloud request engine. Both aspect engines read its one JSON
    // response and its accessible properties, so the whole pipeline makes one
    // cloud call per flow data. The single-product facades each build their own
    // pipeline, so the engines are composed directly here to share the request
    // engine.
    let mut request_builder = CloudRequestEngine::builder().resource_key(&options.resource_key);
    if let Some(endpoint) = &options.endpoint {
        request_builder = request_builder.endpoint(endpoint.clone());
    }
    let request_engine = Arc::new(
        request_builder
            .build()
            .context("failed to build the cloud request engine")?,
    );

    // The Device Detection cloud engine, reading from the shared request engine.
    let device_engine = DeviceDetectionCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .build();

    // The IP Intelligence cloud engine, reading from the same request engine.
    let ip_engine = IpIntelligenceCloudEngine::builder()
        .cloud_request_engine(request_engine.clone())
        .build()
        .context("failed to build the IP Intelligence cloud engine")?;

    // One pipeline with the request engine first, then the two aspect engines.
    let pipeline: Arc<Pipeline> = Pipeline::builder()
        .add_element(request_engine)
        .add_element(Arc::new(device_engine))
        .add_element(Arc::new(ip_engine))
        .build()
        .context("failed to build the mixed cloud pipeline")?;

    writeln!(
        out,
        "Processing {} device-and-IP pairing(s) through one cloud pipeline.",
        SAMPLES.len()
    )?;

    for sample in SAMPLES {
        analyse_sample(&pipeline, sample, out)?;
    }

    Ok(())
}
// [example]

/// Process one sample through the pipeline and print both the device and the IP
/// intelligence results.
fn analyse_sample(
    pipeline: &Arc<Pipeline>,
    sample: &Sample,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    writeln!(out)?;
    writeln!(out, "{}", sample.label)?;
    writeln!(out, "Input values:")?;
    writeln!(out, "\t{USER_AGENT_EVIDENCE_KEY}: {}", sample.user_agent)?;
    writeln!(out, "\t{CLIENT_IP_EVIDENCE_KEY}: {}", sample.client_ip)?;

    // FlowData carries both evidence values in, the request engine forwards them
    // in one cloud call, and both aspect engines read their slice of the result.
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add(USER_AGENT_EVIDENCE_KEY, sample.user_agent)
            .add(CLIENT_IP_EVIDENCE_KEY, sample.client_ip)
            .build(),
    );
    data.process()
        .with_context(|| format!("processing the sample {} failed", sample.label))?;

    // The Device Detection result is read through the shared device data key.
    // Device properties are single-valued, so they print without a weighting.
    let device = data
        .get(DEVICE_DATA_KEY)
        .context("the pipeline produced no Device Detection data")?;
    writeln!(out, "Device Detection results:")?;
    output_device_bool("IsMobile", &device.is_mobile(), out)?;
    output_device_string("PlatformName", &device.platform_name(), out)?;
    output_device_string("BrowserName", &device.browser_name(), out)?;
    output_device_string("HardwareVendor", &device.hardware_vendor(), out)?;

    // The IP Intelligence result is read through the shared IP data key. These
    // properties are plain strings, so they print without a weighting.
    let ip_data = data
        .get(IP_DATA_KEY)
        .context("the pipeline produced no IP Intelligence data")?;
    writeln!(out, "IP Intelligence results:")?;
    output_ip_string("Country", &ip_data.country(), out)?;
    output_ip_string("CountryCode", &ip_data.country_code(), out)?;
    output_ip_string("RegisteredCountry", &ip_data.registered_country(), out)?;
    output_ip_string("RegisteredName", &ip_data.registered_name(), out)?;
    output_ip_string("RegisteredOwner", &ip_data.registered_owner(), out)?;

    Ok(())
}

/// Print a single-valued boolean device property, or its no-value message.
fn output_device_bool(
    name: &str,
    property: &AspectPropertyValue<bool>,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    match property.value() {
        Ok(value) => writeln!(out, "\t{name}: {value}")?,
        Err(no_value) => writeln!(out, "\t{name}: {no_value}")?,
    }
    Ok(())
}

/// Print a single-valued string device property, or its no-value message.
fn output_device_string(
    name: &str,
    property: &AspectPropertyValue<String>,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    match property.value() {
        Ok(value) => writeln!(out, "\t{name}: {value}")?,
        Err(no_value) => writeln!(out, "\t{name}: {no_value}")?,
    }
    Ok(())
}

/// Print a plain IP-intelligence string property, or its no-value message.
fn output_ip_string(
    name: &str,
    property: &AspectPropertyValue<String>,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    match property.value() {
        Ok(value) => writeln!(out, "\t{name}: {value}")?,
        Err(no_value) => writeln!(out, "\t{name}: {no_value}")?,
    }
    Ok(())
}

/// Resolve the resource key from the first command-line argument or the shared
/// environment resolver and run the example. Without a key the example prints how
/// to obtain one and exits successfully, so it is safe to run offline.
fn main() -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let resource_key = std::env::args()
        .nth(1)
        .or_else(examples_shared::resource_key_from_env);

    let Some(resource_key) = resource_key else {
        writeln!(
            out,
            "No resource key available. Set the _51DEGREES_RESOURCE_KEY environment \
             variable or pass a key as the first argument. Create a key for free at \
             https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-mixed-getting-started.rs&utm_term=resource-key-required. \
             The key must grant both the device and IP intelligence products."
        )?;
        examples_shared::write_find_out_more(
            &mut out,
            FIND_OUT_MORE,
            examples_shared::CLOUD_REPOSITORIES,
        )?;
        return Ok(());
    };

    let options = Options {
        resource_key,
        // The cloud endpoint from _51DEGREES_CLOUD_ENDPOINT, or the public cloud
        // when unset. A host other than cloud.51degrees.com is an on premise web
        // server or a privately hosted 51Degrees cloud (see
        // examples_shared::CLOUD_ENDPOINT_ENV_VAR).
        endpoint: examples_shared::cloud_endpoint_from_env(),
    };

    let result = run(&options, &mut out);
    examples_shared::write_find_out_more(
        &mut out,
        FIND_OUT_MORE,
        examples_shared::CLOUD_REPOSITORIES,
    )?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example runs end to end when a resource key is available. It is
    /// ignored by default so a plain `cargo test` stays green offline; supply a
    /// key that grants both products and run with `--ignored` to exercise the
    /// live cloud path.
    #[test]
    #[ignore = "requires a live 51Degrees cloud resource key granting device and IP intelligence"]
    fn runs_against_the_cloud() {
        let resource_key =
            examples_shared::resource_key_from_env().expect("a resource key must be set");
        let options = Options {
            resource_key,
            endpoint: examples_shared::cloud_endpoint_from_env(),
        };

        let mut buffer = Vec::new();
        run(&options, &mut buffer).expect("the mixed cloud example should complete");

        let printed = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(printed.contains("Device Detection results:"));
        assert!(printed.contains("IP Intelligence results:"));
    }
}

/*
 * @example ipi-cloud-mixed-getting-started.rs
 *
 * This example demonstrates using both Device Detection and IP Intelligence from
 * the 51Degrees cloud service in a single pipeline.
 *
 * You will learn:
 *
 * 1. How to build one Pipeline that runs both 51Degrees Cloud Device Detection
 *    and Cloud IP Intelligence over a single shared CloudRequestEngine, so a
 *    User-Agent and a client IP supplied together are resolved in one cloud call.
 * 2. How to pass both kinds of evidence (a User-Agent under `header.user-agent`
 *    and a client IP under `query.client-ip-51d`) to the pipeline as flow data.
 * 3. How to read the device result (IsMobile, PlatformName, BrowserName,
 *    HardwareVendor) and the IP intelligence result (Country, CountryCode,
 *    RegisteredCountry, RegisteredName, RegisteredOwner) from the one flow data.
 *
 * The single-product cloud facades each build their own pipeline, so this example
 * composes the engines directly: it creates one CloudRequestEngine from the
 * resource key, then builds a DeviceDetectionCloudEngine and an
 * IpIntelligenceCloudEngine that both read from it, and adds all three elements to
 * one Pipeline. Both the device properties and these IP intelligence properties
 * are single-valued, so each is printed as a single plain value (or a "no value"
 * explanation when it is absent).
 *
 * The example processes five device-and-IP pairings: a UK desktop, a Chinese
 * iPhone, a Chilean desktop, an iPad on an IPv6 address and a US Android device.
 *
 * This is a console example, so it does not add the ShareUsageElement. A
 * production deployment, and every web example, should enable usage sharing so the
 * data behind these results keeps improving.
 *
 * To run this example, create a Resource Key for free at
 * https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-mixed-getting-started.rs&utm_term=ipi-cloud-mixed-getting-started and supply it via the _51DEGREES_RESOURCE_KEY
 * environment variable or as the first command-line argument. The key must grant
 * both the device and IP intelligence products. By default the pipeline talks to
 * the public cloud endpoint; set the _51DEGREES_CLOUD_ENDPOINT environment variable to
 * point at a self-hosted Cloud service instead.
 */
