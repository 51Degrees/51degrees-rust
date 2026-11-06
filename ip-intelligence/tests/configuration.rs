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

#[cfg(feature = "on-premise")]
#[test]
fn the_on_premise_engine_runs_between_the_sequence_and_usage_sharing() {
    use std::path::{Path, PathBuf};

    let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi");
    let Some(asn) = std::env::var("51DEGREES_IPI_PATH")
        .ok()
        .map(PathBuf::from)
        .into_iter()
        .chain([bundled])
        .find(|path| path.is_file())
    else {
        eprintln!("skipped: no ASN data file at 51DEGREES_IPI_PATH or cxx/data");
        return;
    };
    let engine = || {
        PipelineOptions::new().element(
            ElementOptions::new("IpiOnPremiseEngineBuilder")
                .parameter("DataFile", asn.to_string_lossy().into_owned())
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
}
