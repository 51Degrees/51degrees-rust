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

//! Pipelines made from a configuration with this crate's registry, compared
//! with what .NET's FiftyOne.IpIntelligence 4.5.155 found and made with
//! `FiftyOnePipelineBuilder` once every assembly of the package was loaded.
//! The on-premise test needs the ASN file `51DEGREES_IPI_PATH` names, or the
//! one at `cxx/data/ip-intelligence`, and skips with a note without it.

use std::sync::Arc;

use fiftyone_ip_intelligence::configuration::{self, ElementOptions, PipelineOptions};
use fiftyone_pipeline_core::{Error, Pipeline, PipelineBuilder, Result};

const NO_INNER: &str = "No Inner Exception.";

/// The builders .NET's package gave that make an element, in the order
/// this crate adds them. .NET's also gave four translation engine builders,
/// whose engines the crates do not have, and its abstract builder classes,
/// which make nothing.
fn expected_builders() -> Vec<&'static str> {
    let mut names = vec![
        "SequenceElementBuilder",
        "SetHeadersElementBuilder",
        "ShareUsageBuilder",
    ];
    if cfg!(feature = "cloud") {
        names.extend(["CloudRequestEngineBuilder", "IpiCloudEngineBuilder"]);
    }
    if cfg!(feature = "on-premise") {
        names.push("IpiOnPremiseEngineBuilder");
    }
    names
}

fn build(options: &PipelineOptions) -> Result<Arc<Pipeline>> {
    PipelineBuilder::new().build_from_configuration(&configuration::registry(), options)
}

fn message(result: Result<Arc<Pipeline>>) -> String {
    match result {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

fn data_keys(pipeline: &Pipeline) -> Vec<&str> {
    pipeline
        .flow_elements()
        .iter()
        .map(|e| e.data_key())
        .collect()
}

fn set_headers() -> PipelineOptions {
    PipelineOptions::new().element(ElementOptions::new("SetHeadersElementBuilder"))
}

#[test]
fn the_registry_gives_the_builders_dotnets_package_gives() {
    assert_eq!(
        configuration::registry().builder_names(),
        expected_builders()
    );
}

#[test]
fn a_builder_dotnets_package_has_and_the_crates_lack_is_not_found() {
    for name in [
        "CloudCountriesTranslationEngineBuilder",
        "CountriesTranslationEngineBuilder",
        "CountryCodeTranslationEngineBuilder",
        "TranslationEngineBuilder",
    ] {
        let options = PipelineOptions::new().element(ElementOptions::new(name));
        assert_eq!(
            message(build(&options)),
            format!(
                "Configuration failed to build.\nBuilderName: {name}\nSubElement Builder Names: \
                 No SubElements\nCould not find builder matching '{name}' for element 0. \
                 Available builders are: {}\n{NO_INNER}\n",
                expected_builders().join(",")
            )
        );
    }
}

#[test]
fn usage_sharing_follows_the_feature_unless_the_configuration_sets_it() {
    let by_default: &[&str] = if cfg!(feature = "share-usage") {
        &["sequence", "set-headers", "shareusage"]
    } else {
        &["sequence", "set-headers"]
    };
    assert_eq!(data_keys(&build(&set_headers()).unwrap()), by_default);
    let on = set_headers().parameter("SetShareUsage", "true");
    assert_eq!(
        data_keys(&build(&on).unwrap()),
        ["sequence", "set-headers", "shareusage"]
    );
    let off = set_headers().parameter("SetShareUsage", "false");
    assert_eq!(
        data_keys(&build(&off).unwrap()),
        ["sequence", "set-headers"]
    );
}

/// The ASN file `51DEGREES_IPI_PATH` names, or the one under `cxx/data`, or
/// `None` with a note that the test is skipped.
#[cfg(feature = "on-premise")]
fn asn_file() -> Option<String> {
    use std::path::{Path, PathBuf};

    let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi");
    let found = std::env::var("51DEGREES_IPI_PATH")
        .ok()
        .map(PathBuf::from)
        .into_iter()
        .chain([bundled])
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned());
    if found.is_none() {
        eprintln!("skipped: no ASN data file at 51DEGREES_IPI_PATH or cxx/data");
    }
    found
}

#[cfg(feature = "on-premise")]
#[test]
fn the_on_premise_engine_runs_between_the_sequence_and_usage_sharing() {
    let Some(asn) = asn_file() else {
        return;
    };
    let engine = || {
        PipelineOptions::new().element(
            ElementOptions::new("IpiOnPremiseEngineBuilder")
                .parameter("DataFile", asn.as_str())
                .parameter("CreateTempDataCopy", "false")
                .parameter("AutoUpdate", "false")
                .parameter("DataFileSystemWatcher", "false"),
        )
    };
    let on = engine().parameter("SetShareUsage", "true");
    assert_eq!(
        data_keys(&build(&on).unwrap()),
        ["sequence", "ip", "shareusage"]
    );
    let off = engine().parameter("SetShareUsage", "false");
    assert_eq!(data_keys(&build(&off).unwrap()), ["sequence", "ip"]);
}

/// The configuration in the README, with `path` where it names the data
/// file `file_name`.
#[cfg(feature = "on-premise")]
fn readme_configuration(file_name: &str, path: &str) -> PipelineOptions {
    let readme = include_str!("../README.md");
    let (_, rest) = readme
        .split_once("```jsonc")
        .expect("a configuration in the README");
    let (json, _) = rest.split_once("```").expect("the configuration's end");
    let named = format!("\"{file_name}\"");
    assert_eq!(json.matches(&named).count(), 1, "{json}");
    let given = format!("\"{}\"", path.replace('\\', "\\\\"));
    PipelineOptions::from_json(&json.replace(&named, &given)).unwrap()
}

#[cfg(feature = "on-premise")]
#[test]
fn the_readme_configuration_makes_the_pipeline_it_describes() {
    use fiftyone_ip_intelligence::{
        IpIntelligenceData, IpIntelligenceOnPremiseEngine, PerformanceProfile, IP_DATA_KEY,
    };
    use fiftyone_pipeline_core::Evidence;

    let Some(asn) = asn_file() else {
        return;
    };
    let options = readme_configuration("51Degrees-IPIV4AsnIpiV41.ipi", &asn);
    let pipeline = build(&options).unwrap();
    let expected: &[&str] = if cfg!(feature = "share-usage") {
        &["sequence", "ip", "shareusage"]
    } else {
        &["sequence", "ip"]
    };
    assert_eq!(data_keys(&pipeline), expected);
    let engine = pipeline
        .resource::<IpIntelligenceOnPremiseEngine>()
        .expect("the IP Intelligence engine");
    assert_eq!(engine.performance_profile(), PerformanceProfile::LowMemory);

    // Processed without usage sharing, so the test sends nothing.
    let pipeline = build(&options.parameter("SetShareUsage", "false")).unwrap();
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("server.client-ip", "1.1.1.1")
            .build(),
    );
    data.process().unwrap();
    let ip = data.get(IP_DATA_KEY).expect("the IP data");
    assert_eq!(
        ip.asn_name().value().ok().map(String::as_str),
        Some("CLOUDFLARENET")
    );
}

#[cfg(feature = "cloud")]
mod cloud {
    use fiftyone_cloud_request_engine::{CloudHttpClient, CloudHttpRequest, CloudHttpResponse};

    use super::*;

    /// Answers for the cloud service, granting the IP Intelligence product.
    struct StandIn;

    impl CloudHttpClient for StandIn {
        fn send(
            &self,
            request: &CloudHttpRequest,
        ) -> std::result::Result<CloudHttpResponse, String> {
            let body = if request.url.contains("evidencekeys") {
                r#"["query.client-ip"]"#
            } else if request.url.contains("accessibleproperties") {
                r#"{"Products":{"ip":{"DataTier":"CloudV4","Properties":[
                    {"Name":"RegisteredName","Type":"String","Category":"Network"}]}}}"#
            } else {
                r#"{"ip":{"registeredname":"Example Networks"}}"#
            };
            Ok(CloudHttpResponse {
                status: 200,
                body: body.to_string(),
                retry_after: None,
            })
        }
    }

    #[test]
    fn the_cloud_engine_takes_the_request_engine_made_before_it() {
        let client: Arc<dyn CloudHttpClient> = Arc::new(StandIn);
        let options = PipelineOptions::new()
            .element(
                ElementOptions::new("CloudRequestEngineBuilder").parameter("ResourceKey", "AQkey"),
            )
            .element(ElementOptions::new("IpiCloudEngineBuilder"))
            .parameter("SetShareUsage", "false");
        let pipeline = PipelineBuilder::new()
            .add_resource(Arc::new(client))
            .build_from_configuration(&configuration::registry(), &options)
            .unwrap();
        assert_eq!(data_keys(&pipeline), ["sequence", "cloud-response", "ip"]);
    }

    /// The configuration of .NET's IP Intelligence cloud
    /// `GettingStarted-Console` example at 51degrees-dotnet 4d7152bf, with its
    /// comments shortened. It names the builders without `Builder` and turns
    /// usage sharing off under the older `PipelineBuilderParameters`.
    const DOTNET_CLOUD_EXAMPLE: &str = r#"{
  // For a sample configuration file demonstrating all available options, see
  // the sample configuration in .NET's examples.
  "PipelineOptions": {
    "Elements": [
      {
        "BuilderName": "CloudRequestEngine",
        // A resource key created with the Configurator.
        "BuildParameters": {
          "ResourceKey": "!!ENTER_YOUR_RESOURCE_KEY_HERE!!",
          "CloudRequestOrigin": "51Degrees.example.com",
          // The default is 2 seconds, which a cold connection can exceed.
          "TimeOutSeconds": 10
        }
      },
      {
        "BuilderName": "IpiCloudEngine"
      }
    ],
    "PipelineBuilderParameters": {
      // Set this to true in production in order to share your usage data
      // with 51Degrees.
      "ShareUsage": false
    }
  }
}"#;

    #[test]
    fn dotnets_cloud_example_configuration_is_read() {
        use fiftyone_ip_intelligence::CloudRequestEngine;

        let client: Arc<dyn CloudHttpClient> = Arc::new(StandIn);
        let options = PipelineOptions::from_json(DOTNET_CLOUD_EXAMPLE).unwrap();
        let pipeline = PipelineBuilder::new()
            .add_resource(Arc::new(client))
            .build_from_configuration(&configuration::registry(), &options)
            .unwrap();
        assert_eq!(data_keys(&pipeline), ["sequence", "cloud-response", "ip"]);
        let engine = pipeline
            .resource::<CloudRequestEngine>()
            .expect("the cloud request engine");
        assert_eq!(engine.timeout(), std::time::Duration::from_secs(10));
        assert_eq!(engine.cloud_request_origin(), Some("51Degrees.example.com"));
    }
}
