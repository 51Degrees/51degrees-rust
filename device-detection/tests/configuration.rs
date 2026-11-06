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
//! with what .NET's FiftyOne.DeviceDetection 4.5.180 found and made with
//! `FiftyOnePipelineBuilder` once every assembly of the package was loaded.
//! The on-premise tests need the Lite file `51DEGREES_DD_PATH` names, or the
//! one at `cxx/data/device-detection`, and skip with a note without it.

use std::sync::Arc;

use fiftyone_device_detection::configuration::{self, ElementOptions, PipelineOptions};
use fiftyone_device_detection::{Error, Pipeline, PipelineBuilder, Result};

const NO_INNER: &str = "No Inner Exception.";

/// The builders .NET's package gave that make an element, in the order
/// this crate adds them. .NET's also gave `AppleProfileEngineBuilder`, whose
/// engine the crates do not have, and its abstract builder classes, which
/// make nothing.
fn expected_builders() -> Vec<&'static str> {
    let mut names = vec![
        "SequenceElementBuilder",
        "SetHeadersElementBuilder",
        "ShareUsageBuilder",
    ];
    if cfg!(feature = "cloud") {
        names.extend([
            "CloudRequestEngineBuilder",
            "DeviceDetectionCloudEngineBuilder",
            "HardwareProfileCloudEngineBuilder",
        ]);
    }
    if cfg!(feature = "on-premise") {
        names.push("DeviceDetectionHashEngineBuilder");
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
fn a_builder_dotnets_package_lacks_is_not_found() {
    // .NET's sample configuration names the first, which no .NET 4.5
    // package has, and the JSON builder is in another package.
    for name in [
        "UachJsConversionElementBuilder",
        "JsonBuilderElementBuilder",
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
mod on_premise {
    use std::path::{Path, PathBuf};

    use super::*;

    /// The file `variable` names, or the one at `bundled` under `cxx/data`,
    /// or `None` with a note that the test is skipped.
    fn data_file(variable: &str, bundled: &str) -> Option<String> {
        let given = std::env::var(variable).ok().map(PathBuf::from);
        let bundled = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../cxx/data")
            .join(bundled);
        let found = given
            .into_iter()
            .chain([bundled.clone()])
            .find(|path| path.is_file())
            .map(|path| path.to_string_lossy().into_owned());
        if found.is_none() {
            eprintln!(
                "skipped: no data file at {variable} or {}",
                bundled.display()
            );
        }
        found
    }

    /// An on-premise engine as .NET's run had it, with the data file, no
    /// copy and no updates.
    fn engine(builder: &str, data_file: &str) -> ElementOptions {
        ElementOptions::new(builder)
            .parameter("DataFile", data_file)
            .parameter("CreateTempDataCopy", "false")
            .parameter("AutoUpdate", "false")
            .parameter("DataFileSystemWatcher", "false")
    }

    #[test]
    fn the_hash_engine_runs_between_the_sequence_and_usage_sharing() {
        let Some(lite) = data_file(
            "51DEGREES_DD_PATH",
            "device-detection/51Degrees-LiteV4.1.hash",
        ) else {
            return;
        };
        let hash =
            || PipelineOptions::new().element(engine("DeviceDetectionHashEngineBuilder", &lite));
        let on = hash().parameter("SetShareUsage", "true");
        assert_eq!(
            data_keys(&build(&on).unwrap()),
            ["sequence", "device", "shareusage"]
        );
        let off = hash().parameter("SetShareUsage", "false");
        assert_eq!(data_keys(&build(&off).unwrap()), ["sequence", "device"]);
    }

    #[test]
    fn the_ip_intelligence_builders_can_join_the_registry() {
        // The builders both crates add are found once.
        let mut registry = configuration::registry();
        fiftyone_ip_intelligence::configuration::add_builders(&mut registry);
        let mut expected = expected_builders();
        expected.extend(["IpiCloudEngineBuilder", "IpiOnPremiseEngineBuilder"]);
        assert_eq!(registry.builder_names(), expected);

        let Some(lite) = data_file(
            "51DEGREES_DD_PATH",
            "device-detection/51Degrees-LiteV4.1.hash",
        ) else {
            return;
        };
        let Some(asn) = data_file(
            "51DEGREES_IPI_PATH",
            "ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi",
        ) else {
            return;
        };
        let options = PipelineOptions::new()
            .element(engine("DeviceDetectionHashEngineBuilder", &lite))
            .element(engine("IpiOnPremiseEngineBuilder", &asn))
            // A Sequence element the configuration names on its own stays
            // where it is, as in .NET.
            .element(ElementOptions::new("SequenceElementBuilder"))
            .parameter("SetShareUsage", "true");
        let pipeline = PipelineBuilder::new()
            .build_from_configuration(&registry, &options)
            .unwrap();
        assert_eq!(
            data_keys(&pipeline),
            ["device", "ip", "sequence", "shareusage"]
        );
    }
}

#[cfg(feature = "cloud")]
mod cloud {
    use fiftyone_cloud_request_engine::{CloudHttpClient, CloudHttpRequest, CloudHttpResponse};

    use super::*;

    /// Answers for the cloud service, granting both engines a product.
    struct StandIn;

    impl CloudHttpClient for StandIn {
        fn send(
            &self,
            request: &CloudHttpRequest,
        ) -> std::result::Result<CloudHttpResponse, String> {
            let body = if request.url.contains("evidencekeys") {
                r#"["header.user-agent"]"#
            } else if request.url.contains("accessibleproperties") {
                r#"{"Products":{
                    "device":{"DataTier":"CloudV4TAC","Properties":[
                        {"Name":"IsMobile","Type":"Boolean","Category":"Device"}]},
                    "hardware":{"DataTier":"CloudV4TAC","Properties":[
                        {"Name":"Profiles","Type":"Array","Category":"Unspecified",
                        "ItemProperties":[
                            {"Name":"HardwareVendor","Type":"String","Category":"Device"}]}]}}}"#
            } else {
                r#"{"device":{"ismobile":false},"hardware":{"profiles":[]}}"#
            };
            Ok(CloudHttpResponse {
                status: 200,
                body: body.to_string(),
                retry_after: None,
            })
        }
    }

    #[test]
    fn the_cloud_engines_take_the_request_engine_made_before_them() {
        let client: Arc<dyn CloudHttpClient> = Arc::new(StandIn);
        let options = PipelineOptions::new()
            .element(
                ElementOptions::new("CloudRequestEngineBuilder").parameter("ResourceKey", "AQkey"),
            )
            .element(ElementOptions::new("DeviceDetectionCloudEngineBuilder"))
            .element(ElementOptions::new("HardwareProfileCloudEngineBuilder"))
            .parameter("SetShareUsage", "false");
        let pipeline = PipelineBuilder::new()
            .add_resource(Arc::new(client))
            .build_from_configuration(&configuration::registry(), &options)
            .unwrap();
        assert_eq!(
            data_keys(&pipeline),
            ["sequence", "cloud-response", "device", "hardware"]
        );
    }
}
