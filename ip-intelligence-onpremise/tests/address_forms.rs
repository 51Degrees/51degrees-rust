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

//! The forms of an IP address that the "Address forms" section of the IP
//! Intelligence on-premise specification names, looked up through the engine
//! as a user looks them up.
//!
//! An IPv4-mapped address, such as `::ffff:8.8.8.8`, is its IPv4 address, so
//! it gets the values of that address however it is written, `Ip` gives the
//! IPv4 address and `IpV6` has no value. An IPv4-compatible address
//! (`::8.8.8.8`) and an IPv4-embedded address (`64:ff9b::8.8.8.8`) are IPv6
//! addresses and never get the values of the IPv4 address they hold. An
//! address below the lowest key in a graph has no profile, reads as an
//! address whose profile holds no values, and raises no error.
//!
//! The tests use the free ASN data file, whose lowest key is `1.0.0.0` in the
//! IPv4 graph and `2001:4:112::` in the IPv6 graph. `1.0.0.0` has an `Asn` of
//! its own, so the `Asn` each of its forms reads shows which address was
//! looked up. `8.8.8.8` reads `Asn` as its default in that file, so for its
//! forms only `Ip` and `IpV6` tell the two families apart. The tests run when
//! `_51DEGREES_IPI_PATH` names that file, or when it is in the data repository
//! inside the `cxx` submodule, and skip otherwise.

#[allow(dead_code)]
#[path = "../../deprecated-names/environment.rs"]
mod deprecated_names;

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_ip_intelligence_onpremise::{
    IpIntelligenceDataBase, IpIntelligenceOnPremiseEngineBuilder, IP_DATA_KEY,
};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};
use fiftyone_pipeline_engines::AspectPropertyValue;

/// The free ASN data file's name.
const ASN_FILE_NAME: &str = "51Degrees-IPIV4AsnIpiV41.ipi";

/// The properties the ASN data file holds.
const PROPERTIES: [&str; 2] = ["Asn", "AsnName"];

/// The properties the engine adds, giving the address looked up.
const ADDED: [&str; 2] = ["Ip", "IpV6"];

/// `Asn` is mandatory with this default, so it reads as this for an address
/// that no profile relates to.
const DEFAULT_ASN: &str = "Unknown";

/// The reason a property has no value when no profile relates to the address,
/// as the C library gives it.
const NULL_PROFILE: &str = "The results contained a null profile for the \
    component which the required property belongs to.";

/// Added to a failure that a change in the data file could explain.
const DATA_HINT: &str = "Has the data file gained a new lowest key?";

/// The ASN data file, being the file `_51DEGREES_IPI_PATH` names when it is
/// that file, then the file in the data repository inside the `cxx`
/// submodule.
fn asn_data_file() -> Option<PathBuf> {
    if let Ok(path) = crate::deprecated_names::var("_51DEGREES_IPI_PATH") {
        let path = PathBuf::from(path.trim());
        if path.file_name().is_some_and(|name| name == ASN_FILE_NAME) && path.is_file() {
            return Some(path);
        }
    }
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join("cxx/data/ip-intelligence").join(ASN_FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// A pipeline holding the on-premise engine over the ASN data file, or `None`
/// with a note when there is no such file.
fn pipeline() -> Option<Arc<Pipeline>> {
    let Some(file) = asn_data_file() else {
        eprintln!("no ASN data file found; skipping");
        return None;
    };
    let engine = IpIntelligenceOnPremiseEngineBuilder::new(&file)
        .performance_profile(PerformanceProfile::HighPerformance)
        .properties(PROPERTIES)
        .auto_update(false)
        .file_system_watcher(false)
        .build()
        .expect("the engine should build");
    let element: Arc<dyn FlowElement> = Arc::new(engine);
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("the pipeline should build");
    Some(pipeline)
}

/// The IP data for `address`, given as the client IP. The lookup must raise
/// no error and record none, whatever the address.
fn lookup(pipeline: &Arc<Pipeline>, address: &str) -> IpIntelligenceDataBase {
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.client-ip", address).build());
    data.process()
        .unwrap_or_else(|error| panic!("looking up {address} raised {error:?}"));
    assert!(
        data.errors().is_empty(),
        "looking up {address} recorded {:?}",
        data.errors()
    );
    data.get(IP_DATA_KEY)
        .expect("the IP data should be there")
        .clone()
}

/// What a user reads of each property in `names` for `address`.
fn readings(
    pipeline: &Arc<Pipeline>,
    address: &str,
    names: &[&'static str],
) -> Vec<(&'static str, AspectPropertyValue<String>)> {
    let ip = lookup(pipeline, address);
    names.iter().map(|name| (*name, ip.string(name))).collect()
}

/// Fail unless `address` has an `Asn` of its own, rather than the default,
/// because agreeing only on the default would prove nothing.
fn assert_has_an_asn(pipeline: &Arc<Pipeline>, address: &str) {
    let asn = lookup(pipeline, address).string("Asn");
    assert!(
        asn.as_option().is_some_and(|asn| asn != DEFAULT_ASN),
        "{address} should have an Asn of its own, got {asn:?}. {DATA_HINT}"
    );
}

/// Every way of writing an IPv4-mapped address gets exactly what its IPv4
/// address gets, for the data file's properties and for `Ip` and `IpV6`.
/// `Ip` gives the IPv4 address and `IpV6` has no value.
#[test]
fn a_mapped_address_is_its_ipv4_address_however_it_is_written() {
    let Some(pipeline) = pipeline() else {
        return;
    };
    assert_has_an_asn(&pipeline, "1.0.0.0");
    let every = [PROPERTIES, ADDED].concat();
    for (ipv4, forms) in [
        (
            "1.0.0.0",
            &[
                "::ffff:1.0.0.0",
                "::FFFF:1.0.0.0",
                "::ffff:100:0",
                "0:0:0:0:0:ffff:100:0",
                "0000:0000:0000:0000:0000:ffff:0100:0000",
                "[::ffff:1.0.0.0]:443",
            ][..],
        ),
        (
            "8.8.8.8",
            &["::ffff:8.8.8.8", "::ffff:808:808", "0:0:0:0:0:ffff:808:808"][..],
        ),
    ] {
        let ip = lookup(&pipeline, ipv4);
        assert_eq!(
            ip.string("Ip").as_option().map(String::as_str),
            Some(ipv4),
            "Ip for {ipv4}"
        );
        assert!(!ip.string("IpV6").has_value(), "IpV6 for {ipv4}");
        let expected = readings(&pipeline, ipv4, &every);
        for form in forms {
            assert_eq!(
                readings(&pipeline, form, &every),
                expected,
                "{form} against {ipv4}"
            );
        }
    }
}

/// An IPv4-compatible or IPv4-embedded address is an IPv6 address. It never
/// gets the values of the IPv4 address it holds, `Ip` has no value and `IpV6`
/// gives the address. In the ASN data file each is below the lowest key in the
/// IPv6 graph, so `Asn` reads as its default, where `1.0.0.0` has its own.
#[test]
fn compatible_and_embedded_addresses_are_ipv6_addresses() {
    let Some(pipeline) = pipeline() else {
        return;
    };
    assert_has_an_asn(&pipeline, "1.0.0.0");
    for form in [
        "::1.0.0.0",
        "::100:0",
        "64:ff9b::1.0.0.0",
        "64:ff9b::100:0",
        "::8.8.8.8",
        "::808:808",
        "64:ff9b::8.8.8.8",
        "64:ff9b::808:808",
        "0064:ff9b:0000:0000:0000:0000:0808:0808",
    ] {
        let ip = lookup(&pipeline, form);
        assert_eq!(
            ip.string("Asn").as_option().map(String::as_str),
            Some(DEFAULT_ASN),
            "Asn for {form}. {DATA_HINT}"
        );
        assert!(!ip.string("Ip").has_value(), "Ip for {form}");
        assert!(ip.string("IpV6").has_value(), "IpV6 for {form}");
    }
}

/// An address below the lowest key in a graph has no profile and raises no
/// error. `Asn`, mandatory with a default, reads as its default, and a property
/// with no value has the null profile reason. Each address reads as one whose
/// profile holds no values, so a data file that adds a key at the bottom of
/// each graph changes nothing. The lowest keys keep their own values.
#[test]
fn an_address_below_the_lowest_key_has_no_profile() {
    let Some(pipeline) = pipeline() else {
        return;
    };
    for (lowest_key, empty_profile, below) in [
        (
            "1.0.0.0",
            "127.0.0.1",
            &[
                "0.0.0.0",
                "0.255.255.255",
                "::ffff:0.0.0.0",
                "::ffff:0:0",
                "::ffff:0.255.255.255",
            ][..],
        ),
        (
            "2001:4:112::",
            "2001:4:113::",
            &["::", "::1", "2001:4:111:ffff:ffff:ffff:ffff:ffff"][..],
        ),
    ] {
        assert_has_an_asn(&pipeline, lowest_key);
        let empty = readings(&pipeline, empty_profile, &PROPERTIES);
        for address in below {
            let read = readings(&pipeline, address, &PROPERTIES);
            for (name, value) in &read {
                if let Some(message) = value.no_value_message() {
                    assert_eq!(message, NULL_PROFILE, "{name} for {address}");
                }
            }
            assert_eq!(
                read[0].1.as_option().map(String::as_str),
                Some(DEFAULT_ASN),
                "Asn for {address}. {DATA_HINT}"
            );
            assert_eq!(read, empty, "{address} against {empty_profile}");
        }
    }
}
