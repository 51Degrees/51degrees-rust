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

//! @page ipi-cloud-metadata-example Metadata (IP Intelligence, Cloud)
//!
//! Cloud IP-intelligence metadata console example.
//!
//! See the descriptive block at the bottom of this file for the full narrative.
//!
//! @snippet ipi-cloud-metadata.rs example

use anyhow::{Context, Result};
use fiftyone_ip_intelligence::IpIntelligencePipelineBuilder;

/// The links the example's output ends with.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-metadata.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-metadata.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-metadata.rs&utm_term=find-out-more"),
];

/// A representative set of evidence keys to probe against the pipeline's
/// evidence filter. The cloud request engine advertises its accepted keys as a
/// whitelist, but the public `FlowElement` view only exposes
/// `EvidenceKeyFilter::include`, so we test each candidate rather than
/// enumerate. These cover the IP-intelligence client-IP keys plus a couple of
/// keys the broader cloud accepts, so the output shows both accepted and
/// rejected examples.
const CANDIDATE_EVIDENCE_KEYS: &[&str] = &[
    "query.client-ip-51d",
    "server.client-ip",
    "query.client-ip",
    "51d.client-ip",
    "header.user-agent",
    "query.user-agent",
];

/// Options the example runs with, gathered so `main` and the test call [`run`]
/// the same way.
pub struct Options {
    /// The cloud resource key whose property set is described.
    pub resource_key: String,
    /// An optional override for the cloud endpoint. When `None` the public
    /// 51Degrees cloud endpoint is used.
    pub endpoint: Option<String>,
}

// [example]
/// The example logic: build a cloud IP-intelligence pipeline, then print the
/// properties the resource key exposes (with category and value type) and which
/// evidence keys the pipeline accepts.
pub fn run(options: Options) -> Result<()> {
    // Build the cloud pipeline. Metadata is read from the assembled pipeline, so
    // no flow data needs to be processed. A console example never adds the
    // ShareUsageElement; production should enable usage sharing.
    let mut builder = IpIntelligencePipelineBuilder::cloud(options.resource_key);
    if let Some(endpoint) = options.endpoint {
        builder = builder.endpoint(endpoint);
    }
    let pipeline = builder
        .build()
        .context("failed to build the cloud IP-intelligence pipeline")?;

    output_properties(&pipeline);
    output_evidence_keys(&pipeline);

    Ok(())
}
// [example]

/// Print the properties every element in the pipeline can populate.
///
/// The properties available depend on the resource key, not on the full cloud
/// catalogue, so this is the list this key returns. Each line shows the property
/// name, its category, and its value type. IP-intelligence properties are
/// weighted, so they surface as a `KeyValueList` of rawweighting/value
/// records.
fn output_properties(pipeline: &fiftyone_pipeline_core::Pipeline) {
    println!("Available properties:");
    let mut any = false;
    for element in pipeline.flow_elements() {
        for property in element.properties() {
            any = true;
            let category = if property.category.is_empty() {
                "Uncategorised".to_owned()
            } else {
                property.category.clone()
            };
            // PropertyValueType has no Display impl, so its Debug form names the
            // type (for weighted IP-intelligence properties this is KeyValueList).
            println!(
                "\tProperty - {} [Category: {category}] ({:?})",
                property.name, property.value_type
            );
        }
    }
    if !any {
        println!("\t(the resource key exposes no properties)");
    }
}

/// Print which evidence keys the pipeline accepts.
///
/// The pipeline's evidence filter is the union of every element's filter. The
/// cloud request engine is the element that actually forwards evidence to the
/// service, so its keys dominate here. We probe a representative candidate set
/// because the trait exposes membership testing rather than enumeration.
fn output_evidence_keys(pipeline: &fiftyone_pipeline_core::Pipeline) {
    println!();
    println!("Accepted evidence keys (probed from a representative set):");
    let filter = pipeline.evidence_key_filter();
    for key in CANDIDATE_EVIDENCE_KEYS {
        let accepted = if filter.include(key) {
            "accepted"
        } else {
            "not accepted"
        };
        println!("\t{key}: {accepted}");
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
            "No resource key available. Set the 51DEGREES_RESOURCE_KEY environment \
             variable or pass a key as the first argument. Create a key for free at \
             https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-metadata.rs&utm_term=resource-key-required."
        );
        examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::CLOUD_REPOSITORIES);
        return Ok(());
    };

    let result = run(Options {
        resource_key,
        // The cloud endpoint from 51DEGREES_CLOUD_ENDPOINT, or the public cloud
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
        .expect("the cloud metadata example should complete");
    }
}

/*
 * @example ipi-cloud-metadata.rs
 *
 * The Cloud service exposes metadata about the properties it can return and the
 * evidence it accepts. This example shows how to read that metadata from a built
 * cloud IP-intelligence pipeline and display it.
 *
 * You will learn:
 *
 * 1. How to build a Pipeline that uses 51Degrees Cloud IP Intelligence from a
 *    resource key.
 * 2. How to list the properties the resource key exposes, with each property's
 *    category and value type. IP-intelligence properties are weighted, so they
 *    are reported as a KeyValueList of rawweighting/value records rather than
 *    a plain scalar type.
 * 3. How to determine which evidence keys the pipeline accepts. The pipeline
 *    evidence filter is the union of every element's filter; the cloud request
 *    engine is the element that forwards evidence to the service, so its keys
 *    dominate. The accepted set depends on the products the resource key enables,
 *    so not every probed key will be relevant to IP Intelligence alone.
 *
 * The property list reflects the resource key, not the full cloud catalogue, so
 * it is the set this particular key returns.
 *
 * This is a console example, so it does not add the ShareUsageElement. A
 * production deployment should enable usage sharing.
 *
 * To run this example, create a Resource Key for free at
 * https://configure.51degrees.com?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-cloud-metadata.rs&utm_term=ipi-cloud-metadata and supply it via the 51DEGREES_RESOURCE_KEY
 * environment variable or as the first command-line argument. By default the
 * pipeline talks to the public cloud endpoint; set the 51DEGREES_CLOUD_ENDPOINT
 * environment variable to point at a self-hosted Cloud service instead.
 */
