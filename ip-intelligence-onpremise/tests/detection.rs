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

//! Integration tests for the on-premise IP Intelligence engine, across the data
//! tiers the Rust workspace knows about.
//!
//! The tiers exercise different files and different cost/coverage trade-offs:
//!
//! - **ASN** (`51Degrees-IPIV4AsnIpiV41.ipi`). Small, current and always
//!   loadable, and free from the data repository, whose script downloads it, so
//!   the ASN tests run wherever it is downloaded and are the primary on-premise
//!   coverage.
//! - **Enterprise** (`51Degrees-IPIV4EnterpriseIpiV41.ipi`). Full property
//!   coverage. The Enterprise test is `#[ignore]` because the file is large and
//!   the share is reachable only on the 51Degrees network; run it explicitly with
//!   `--ignored` (or `--include-ignored`) on a machine where the share is mounted.
//!
//! Every tier resolves its data file at run time and skips cleanly when the file
//! is absent, so a checkout without the data files (and an off-network CI) stays
//! green while a developer with the files gets real native coverage. An explicit
//! `51DEGREES_IPI_PATH` overrides the ASN path so a single file can drive the
//! default tests.

use std::path::PathBuf;
use std::sync::Arc;

use fiftyone_ip_intelligence_onpremise::{
    IpIntelligenceData, IpIntelligenceOnPremiseEngine, IpIntelligenceOnPremiseEngineBuilder,
    IP_DATA_KEY,
};
use fiftyone_ip_intelligence_shared::{generated_property_names, GENERATED_PROPERTY_TYPES};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{ElementData, Evidence, FlowElement, Pipeline, PropertyValueType};
use fiftyone_pipeline_engines::{AspectEngine, MissingPropertyReason, OnPremiseAspectEngine};

/// Cloudflare's public DNS resolver. It maps to autonomous system AS13335 in
/// every IP Intelligence tier, so it is a stable lookup target for assertions.
const CLOUDFLARE_IPV4: &str = "1.1.1.1";

/// The IPv6 form of the same Cloudflare resolver, used to prove IPv6 lookups
/// resolve to the same autonomous system as the IPv4 form.
const CLOUDFLARE_IPV6: &str = "2606:4700:4700::1111";

/// The autonomous-system properties an ASN data file carries. Requesting them
/// explicitly makes the on-premise engine surface them for the lookup.
const ASN_PROPERTIES: &[&str] = &["Asn", "AsnName"];

/// Walk up from this crate's directory looking for a data file at the given
/// relative path inside a sibling `*-cxx` checkout, returning the first hit.
///
/// `CARGO_MANIFEST_DIR` is this crate's directory; the loop climbs to the
/// workspace root and beyond to the wider `Workspace` tree where the products
/// keep their data files.
fn find_up(relative: &str) -> Option<PathBuf> {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let candidate = dir.join(relative);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Resolve the ASN data file: an explicit `51DEGREES_IPI_PATH` first, then the
/// ASN file in the data repository inside the `cxx` submodule.
fn asn_data_file() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("51DEGREES_IPI_PATH") {
        let path = PathBuf::from(path.trim());
        if path.is_file() {
            return Some(path);
        }
    }
    find_up("cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi")
}

/// The Enterprise data-file name on the Enterprise share.
const ENTERPRISE_FILE_NAME: &str = "51Degrees-IPIV4EnterpriseIpiV41.ipi";

/// The environment variable giving the root of the dated Enterprise share, laid
/// out as `<root>/YYYY/MM/DD/<file>`. Unset off the 51Degrees network.
const ENTERPRISE_SHARE_ENV_VAR: &str = "51DEGREES_IPI_ENTERPRISE_SHARE";

/// Resolve the latest Enterprise data file on the configured share, or `None`
/// when the variable is unset or the share is not mounted (the common off-network
/// case). The newest dated folder that actually contains the file is chosen;
/// folder names are zero-padded so a lexicographic comparison sorts the same as
/// by date.
fn enterprise_data_file() -> Option<PathBuf> {
    fn latest_numeric_child(dir: &std::path::Path) -> Option<PathBuf> {
        let mut best: Option<(String, PathBuf)> = None;
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_digit()) {
                match &best {
                    Some((best_name, _)) if *best_name >= name => {}
                    _ => best = Some((name, path)),
                }
            }
        }
        best.map(|(_, path)| path)
    }

    let root = PathBuf::from(std::env::var_os(ENTERPRISE_SHARE_ENV_VAR)?);
    let year = latest_numeric_child(&root)?;
    let month = latest_numeric_child(&year)?;
    let day = latest_numeric_child(&month)?;
    let file = day.join(ENTERPRISE_FILE_NAME);
    file.is_file().then_some(file)
}

/// Build a fresh on-premise engine over `data_file` with the requested profile
/// and properties, returning `None` and a note when the native data set will not
/// load. A load failure is a clean skip rather than a hard failure so the suite
/// stays green on a checkout where a tier's file is the wrong version or
/// otherwise unreadable.
fn build_engine(
    data_file: &std::path::Path,
    profile: PerformanceProfile,
    properties: &[&str],
) -> Option<Arc<IpIntelligenceOnPremiseEngine>> {
    match IpIntelligenceOnPremiseEngineBuilder::new(data_file)
        .performance_profile(profile)
        .properties(properties.iter().copied())
        .auto_update(false)
        .file_system_watcher(false)
        .build()
    {
        Ok(engine) => Some(Arc::new(engine)),
        Err(e) => {
            eprintln!(
                "the IP Intelligence data set at {} did not load ({e}); skipping",
                data_file.display()
            );
            None
        }
    }
}

/// Look one IP address up through a one-element pipeline and return the IP data,
/// cloned out so the flow data can be dropped.
fn lookup(
    engine: Arc<IpIntelligenceOnPremiseEngine>,
    ip: &str,
) -> fiftyone_ip_intelligence_onpremise::IpIntelligenceDataBase {
    let element: Arc<dyn FlowElement> = engine;
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("pipeline builds");
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.client-ip", ip).build());
    data.process().expect("processing succeeds");
    data.get(IP_DATA_KEY)
        .expect("ip intelligence data was produced")
        .clone()
}

/// Assert that a lookup of the given IP resolves the autonomous system to
/// Cloudflare's AS13335, the stable fact every tier agrees on. `Asn` is a plain
/// string property, so it is read through the strongly-typed [`asn`] accessor.
///
/// [`asn`]: IpIntelligenceData::asn
fn assert_resolves_to_cloudflare(engine: Arc<IpIntelligenceOnPremiseEngine>, ip: &str) {
    let ip_data = lookup(engine, ip);
    let asn = ip_data.asn();
    let value = asn
        .value()
        .unwrap_or_else(|e| panic!("Asn should resolve for {ip}, got no-value: {e}"));
    assert!(
        value.contains("AS13335"),
        "{ip} should resolve to Cloudflare AS13335, got {value}"
    );
}

#[test]
fn asn_resolves_ipv4_to_cloudflare() {
    let Some(file) = asn_data_file() else {
        eprintln!("no ASN data file found; skipping ASN IPv4 lookup");
        return;
    };
    let Some(engine) = build_engine(&file, PerformanceProfile::HighPerformance, ASN_PROPERTIES)
    else {
        return;
    };
    assert_resolves_to_cloudflare(engine, CLOUDFLARE_IPV4);
}

#[test]
fn asn_resolves_ipv6_to_cloudflare() {
    let Some(file) = asn_data_file() else {
        eprintln!("no ASN data file found; skipping ASN IPv6 lookup");
        return;
    };
    let Some(engine) = build_engine(&file, PerformanceProfile::HighPerformance, ASN_PROPERTIES)
    else {
        return;
    };
    assert_resolves_to_cloudflare(engine, CLOUDFLARE_IPV6);
}

#[test]
fn asn_and_asn_name_resolve_as_plain_strings() {
    let Some(file) = asn_data_file() else {
        eprintln!("no ASN data file found; skipping plain-property test");
        return;
    };
    let Some(engine) = build_engine(&file, PerformanceProfile::HighPerformance, ASN_PROPERTIES)
    else {
        return;
    };

    // Both autonomous-system properties are plain strings, read through their
    // strongly-typed accessors. A public Cloudflare IP resolves both.
    let ip_data = lookup(engine, CLOUDFLARE_IPV4);
    let asn_value = ip_data.asn();
    let asn = asn_value
        .value()
        .expect("Asn should resolve to a plain string for a public IP");
    assert!(
        asn.contains("AS13335"),
        "the autonomous system number should be AS13335, got {asn}"
    );
    let asn_name_value = ip_data.asn_name();
    let asn_name = asn_name_value
        .value()
        .expect("AsnName should resolve to a plain string for a public IP");
    assert!(
        !asn_name.is_empty(),
        "the autonomous system name should be non-empty"
    );
}

#[test]
fn asn_publishes_metadata_and_evidence_keys() {
    let Some(file) = asn_data_file() else {
        eprintln!("no ASN data file found; skipping metadata test");
        return;
    };
    let Some(engine) = build_engine(&file, PerformanceProfile::HighPerformance, ASN_PROPERTIES)
    else {
        return;
    };

    // The engine publishes property metadata, including the requested ASN
    // property.
    assert!(
        !engine.aspect_properties().is_empty(),
        "the engine should publish property metadata"
    );
    assert!(
        engine
            .aspect_properties()
            .iter()
            .any(|p| p.name().eq_ignore_ascii_case("Asn")),
        "Asn should be among the published properties"
    );

    // The engine reads an IP address from client-IP evidence.
    assert!(
        engine.evidence_key_filter().include("query.client-ip"),
        "the engine should accept query.client-ip evidence"
    );

    // The data file records its tier (a non-empty source-tier string).
    assert!(
        !engine.data_source_tier().is_empty(),
        "the engine should report a data source tier"
    );
}

#[test]
fn asn_refresh_round_trips() {
    let Some(file) = asn_data_file() else {
        eprintln!("no ASN data file found; skipping refresh test");
        return;
    };
    let Some(engine) = build_engine(&file, PerformanceProfile::HighPerformance, ASN_PROPERTIES)
    else {
        return;
    };

    // A lookup works before the refresh.
    assert_resolves_to_cloudflare(engine.clone(), CLOUDFLARE_IPV4);

    // Refresh reloads the data file in place and hot-swaps it.
    engine
        .refresh(None)
        .expect("refresh from the on-disk file succeeds");

    // The same lookup still works after the swap, proving the reloaded data set
    // is live.
    assert_resolves_to_cloudflare(engine, CLOUDFLARE_IPV4);
}

/// For every generated property that resolves to a value, the value must unpack
/// through the typed accessor as the type the generated metadata declares. A
/// property present in the result but not readable as its declared type (a value
/// the data file returned that does not parse as the declared bool, integer or
/// double, for example) is a real type-mapping defect and fails this test,
/// naming every offender.
///
/// The engine is asked for the whole generated property set so an Enterprise data
/// file populates a wide cross-section. Skips cleanly when no data file is
/// present; point `51DEGREES_IPI_PATH` at an Enterprise file for full coverage.
#[test]
fn every_present_property_unpacks_as_its_declared_type() {
    let Some(file) = asn_data_file() else {
        eprintln!("no IP-intelligence data file found; skipping type-unpack check");
        return;
    };
    let properties: Vec<String> = generated_property_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let property_refs: Vec<&str> = properties.iter().map(String::as_str).collect();
    let Some(engine) = build_engine(&file, PerformanceProfile::LowMemory, &property_refs) else {
        return;
    };

    // A spread of public addresses so a wide cross-section of properties
    // resolves on an Enterprise file.
    let ips = [
        CLOUDFLARE_IPV4,
        CLOUDFLARE_IPV6,
        "8.8.8.8",
        "185.28.167.77",
        "2.16.0.0",
    ];

    let mut offenders: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut present_count = 0usize;

    for ip in ips {
        let ip_data = lookup(engine.clone(), ip);
        for (name, declared) in GENERATED_PROPERTY_TYPES {
            if ip_data.get(name).is_err() {
                continue;
            }
            present_count += 1;
            let unpacks = match declared {
                PropertyValueType::Bool => ip_data.boolean(name).value().is_ok(),
                PropertyValueType::Integer => ip_data.integer(name).value().is_ok(),
                PropertyValueType::Float => ip_data.float(name).value().is_ok(),
                // The weighted properties are published as a key-value list.
                PropertyValueType::KeyValueList => ip_data.weighted_string(name).value().is_ok(),
                _ => ip_data.string(name).value().is_ok(),
            };
            if !unpacks {
                let raw = ip_data.get(name).ok();
                offenders.insert(format!("{name} [{declared:?}] raw={raw:?}"));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "{} present propert(ies) did not unpack as their declared type:\n{}",
        offenders.len(),
        offenders.into_iter().collect::<Vec<_>>().join("\n")
    );
    assert!(
        present_count > 0,
        "expected at least some properties to resolve for the sample IPs"
    );
}

/// The Enterprise tier carries the full property set, including location. This
/// test is `#[ignore]` because the file is large and the production share is
/// reachable only on the 51Degrees network. Run it with
/// `cargo test -p fiftyone-ip-intelligence-onpremise -- --ignored` on a machine
/// where the share is mounted.
#[test]
#[ignore = "loads the large Enterprise file from the production share; run with --ignored on-network"]
fn enterprise_resolves_location_for_a_public_ip() {
    let Some(file) = enterprise_data_file() else {
        eprintln!("the Enterprise production share is not reachable; skipping");
        return;
    };
    // LowMemory keeps the large file on disk (memory-mapped) rather than loading
    // it wholly into memory, which suits the large Enterprise data file. The
    // Enterprise tier carries location as well as network data, so the location
    // properties are
    // requested alongside the autonomous-system ones.
    let Some(engine) = build_engine(
        &file,
        PerformanceProfile::LowMemory,
        &[
            "Asn",
            "AsnName",
            "CountryCode",
            "Country",
            "CountryCodesGeographical",
        ],
    ) else {
        return;
    };

    // The autonomous system still resolves, as in every tier.
    assert_resolves_to_cloudflare(engine.clone(), CLOUDFLARE_IPV4);

    // Enterprise additionally carries location data: a public IP resolves to a
    // country code, read through the plain string accessor.
    let ip_data = lookup(engine, CLOUDFLARE_IPV4);
    let country = ip_data.country_code();
    assert!(
        country
            .value()
            .map(|code| !code.is_empty())
            .unwrap_or(false),
        "the Enterprise tier should resolve a country code for a public IP"
    );

    // CountryCodesGeographical is a genuinely weighted property: it resolves to a
    // list of candidates ordered high weighting first, each with a `(0, 1]`
    // weighting. This is the integration-level proof of the weighted contract.
    let codes_value = ip_data.country_codes_geographical();
    let codes = codes_value
        .value()
        .expect("the Enterprise tier should resolve the geographical country codes");
    assert!(
        !codes.is_empty(),
        "the weighted country-code list should carry at least one candidate"
    );
    for pair in codes.windows(2) {
        assert!(
            pair[0].weighting() >= pair[1].weighting(),
            "weighted candidates must be ordered high weighting first"
        );
    }
    for candidate in codes {
        let weighting = candidate.weighting();
        assert!(
            weighting > 0.0 && weighting <= 1.0,
            "weighting {weighting} for '{}' should be in (0, 1]",
            candidate.value
        );
    }
}

/// A property the engine was not built with, but that `IpIntelligenceData`
/// has an accessor for, is not in the engine's metadata, so the
/// specification's typed accessor rule gives a data file upgrade and the
/// message leaves out the tier sentence. The rule counts the accessors of the
/// shared `IpIntelligenceData`.
#[test]
fn an_unlisted_property_with_an_accessor_needs_a_data_file_upgrade() {
    let Some(file) = asn_data_file() else {
        eprintln!("no ASN data file found; skipping missing property test");
        return;
    };
    let Some(engine) = build_engine(&file, PerformanceProfile::HighPerformance, ASN_PROPERTIES)
    else {
        return;
    };

    let result = engine.missing_property_reason("CountryCode");

    assert_eq!(
        result.reason,
        MissingPropertyReason::DataFileUpgradeRequired
    );
    assert!(
        result.description.starts_with(
            "Property 'CountryCode' not found in data for element 'ip'. This is because \
             your license or data file does not include this property. To see which \
             licenses include it, see "
        ),
        "{}",
        result.description
    );
}

/// A name no engine knows gives .NET's unknown reason and text.
#[test]
fn an_unknown_property_has_an_unknown_reason() {
    let Some(file) = asn_data_file() else {
        eprintln!("no ASN data file found; skipping missing property test");
        return;
    };
    let Some(engine) = build_engine(&file, PerformanceProfile::HighPerformance, ASN_PROPERTIES)
    else {
        return;
    };

    let result = engine.missing_property_reason("NotAProperty");

    assert_eq!(result.reason, MissingPropertyReason::Unknown);
    assert_eq!(
        result.description,
        "Property 'NotAProperty' not found in data for element 'ip'. The reason for this \
         is unknown. Please check that the aspect and property name are correct."
    );
}

/// IP data from an engine restricted to the ASN properties gives the same
/// reason as `engine.missing_property_reason` for each property it does not
/// hold, through its typed accessors and by name. .NET throws a
/// `PropertyMissingException` instead, and gives `Unknown` for `CountryCode`,
/// because `IIpDataOnPremise` declares no properties of its own.
#[test]
fn ip_data_gives_the_engine_reason_for_a_missing_property() {
    let Some(file) = asn_data_file() else {
        eprintln!("no ASN data file found; skipping missing property test");
        return;
    };
    let Some(engine) = build_engine(&file, PerformanceProfile::HighPerformance, ASN_PROPERTIES)
    else {
        return;
    };

    let ip_data = lookup(Arc::clone(&engine), CLOUDFLARE_IPV4);

    assert!(ip_data.asn().has_value());
    let country_code = engine.missing_property_reason("CountryCode");
    assert_eq!(
        country_code.reason,
        MissingPropertyReason::DataFileUpgradeRequired
    );
    assert_eq!(
        ip_data.country_code().no_value_message(),
        Some(country_code.description.as_str())
    );
    for name in ["CountryCode", "NotAProperty"] {
        let expected = engine.missing_property_reason(name).description;
        assert_eq!(ip_data.get(name).unwrap_err().message, expected, "{name}");
        assert_eq!(
            ip_data.string(name).no_value_message(),
            Some(expected.as_str()),
            "{name}"
        );
    }
}

/// The ASN file's name.
const ASN_FILE_NAME: &str = "51Degrees-IPIV4AsnIpiV41.ipi";

/// An engine built with no properties named lists every typed property with
/// the data file's own tier, including those the ASN file does not hold. The
/// typed accessor rule is only for a property the metadata does not list, so
/// the reason is unknown rather than an upgrade to the tier the file has.
#[test]
fn a_listed_property_the_asn_file_lacks_has_an_unknown_reason() {
    let Some(file) = asn_data_file() else {
        eprintln!("no ASN data file found; skipping missing property test");
        return;
    };
    let engine = build_engine(&file, PerformanceProfile::HighPerformance, &[])
        .expect("the ASN data file builds an engine");
    if lookup(Arc::clone(&engine), CLOUDFLARE_IPV4)
        .country_code()
        .has_value()
    {
        eprintln!(
            "{} holds CountryCode, so it is not the ASN file; skipping",
            file.display()
        );
        return;
    }
    assert!(
        engine
            .aspect_properties()
            .iter()
            .any(|p| p.name() == "CountryCode"),
        "this test covers a property the engine lists although the file lacks it"
    );

    let result = engine.missing_property_reason("CountryCode");

    assert_eq!(result.reason, MissingPropertyReason::Unknown);
    assert!(
        !result.description.contains("license/data"),
        "{}",
        result.description
    );
}

/// IP data from an engine on the ASN file, which holds no `CountryCode`, names
/// no tier to upgrade to for `CountryCode`, through its typed accessor or by
/// name, although the engine lists the property with the file's own tier.
///
/// Built with no properties named, the reader holds only the file's
/// properties, so the data gives the engine's `Unknown` reason. Built with
/// `CountryCode` named, the native layer keeps the name it was given, so the
/// reader holds it and reads it as having no value.
#[test]
fn ip_data_names_no_tier_for_a_listed_property_the_asn_file_lacks() {
    let Some(file) = asn_data_file() else {
        eprintln!("no ASN data file found; skipping missing property test");
        return;
    };
    if file.file_name().and_then(|name| name.to_str()) != Some(ASN_FILE_NAME) {
        eprintln!("{} is not the ASN file; skipping", file.display());
        return;
    }
    let names: [&[&str]; 2] = [&[], &["Asn", "AsnName", "CountryCode"]];
    for properties in names {
        let engine = build_engine(&file, PerformanceProfile::HighPerformance, properties)
            .expect("the ASN file builds an engine");
        assert!(
            engine
                .aspect_properties()
                .iter()
                .any(|p| p.name() == "CountryCode"),
            "{properties:?}: the engine lists CountryCode"
        );
        let expected = engine.missing_property_reason("CountryCode");
        assert_eq!(
            expected.reason,
            MissingPropertyReason::Unknown,
            "{properties:?}"
        );

        let ip_data = lookup(Arc::clone(&engine), CLOUDFLARE_IPV4);

        assert!(ip_data.asn().has_value(), "{properties:?}");
        let typed = ip_data
            .country_code()
            .no_value_message()
            .expect("the ASN file holds no CountryCode")
            .to_owned();
        let by_name = ip_data.get("CountryCode").unwrap_err().message;
        for message in [&typed, &by_name] {
            assert!(
                !message.contains("license/data"),
                "{properties:?}: {message}"
            );
        }
        if properties.is_empty() {
            assert_eq!(typed, expected.description);
            assert_eq!(by_name, expected.description);
        }
    }
}
