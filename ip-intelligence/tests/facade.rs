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

//! Facade-level integration tests for [`IpIntelligencePipelineBuilder`].
//!
//! These prove that both fluent entry points build a working pipeline that
//! exposes its result under the shared [`IP_DATA_KEY`], so an application can
//! switch deployment without changing how it reads the data:
//!
//! - [`IpIntelligencePipelineBuilder::on_premise`] over the loadable ASN data
//!   file (gated on the `on-premise` feature; skips when the file is absent).
//! - [`IpIntelligencePipelineBuilder::cloud`] against the live service (gated on
//!   the default `cloud` feature; `#[ignore]` and gated on a resource key in the
//!   environment, honouring the same key chain as the rest of the workspace).
//!
//! Run the full set, including the feature-gated and live tests, with:
//! `cargo test -p fiftyone-ip-intelligence --all-features -- --include-ignored`.

// The typed `IpIntelligenceData` accessors (for example `country_code`) are
// used only by the live cloud test below, which is gated on `reqwest-client`.
// Gate the import to match, so the default (cloud + on-premise) build sees no
// unused import. The on-premise test reads `string(..)`, an inherent method on
// the data type, so it needs no trait import.
#[cfg(feature = "reqwest-client")]
use fiftyone_ip_intelligence::IpIntelligenceData;
use fiftyone_ip_intelligence::{IpIntelligencePipelineBuilder, IP_DATA_KEY};
use fiftyone_pipeline_core::Evidence;

/// Cloudflare's public IPv4 resolver, autonomous system AS13335 in every tier.
/// Used only by the on-premise test, so gated to avoid an unused-const warning
/// when the crate is built with the default (cloud-only) features.
#[cfg(feature = "on-premise")]
const CLOUDFLARE_IPV4: &str = "1.1.1.1";

/// Walk up from this crate's directory looking for a data file at the given
/// relative path inside a sibling `*-cxx` checkout, returning the first hit.
#[cfg(feature = "on-premise")]
fn find_up(relative: &str) -> Option<std::path::PathBuf> {
    let mut dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
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

/// The on-premise facade builds a pipeline over the ASN data file and resolves
/// the autonomous system through the shared `IP_DATA_KEY`. Skips cleanly when no
/// ASN data file is present.
#[cfg(feature = "on-premise")]
#[test]
fn on_premise_facade_resolves_asn() {
    use fiftyone_ip_intelligence::PerformanceProfile;

    let data_file = std::env::var("_51DEGREES_IPI_PATH")
        .ok()
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| find_up("cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi"));

    let Some(data_file) = data_file else {
        eprintln!("no ASN data file found; skipping on-premise facade test");
        return;
    };

    let pipeline = IpIntelligencePipelineBuilder::on_premise(&data_file)
        .performance_profile(PerformanceProfile::HighPerformance)
        .property("Asn")
        // Off so the test sends nothing to 51Degrees.
        .share_usage(false)
        // Off so the build needs no licence key.
        .auto_update(false)
        .build()
        .expect("the on-premise facade pipeline builds");

    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("query.client-ip", CLOUDFLARE_IPV4)
            .build(),
    );
    data.process().expect("on-premise processing succeeds");

    let ip = data.get(IP_DATA_KEY).expect("the facade produced ip data");
    // Asn is a plain string property.
    let asn = ip.string("Asn");
    let value = asn.value().expect("Asn resolves to a value");
    assert!(
        value.contains("AS13335"),
        "the on-premise facade should resolve {CLOUDFLARE_IPV4} to AS13335, got {value}"
    );
}

/// Resolve a cloud resource key from the environment, honouring the aligned name
/// first and then the CI-exported bespoke and basic names, mirroring
/// `examples-shared::keys::resource_key_from_env`. Used only by the live cloud
/// test, which is gated on `reqwest-client`, so gate the helper to match and
/// avoid a dead-code warning in the default (no reqwest-client) build.
#[cfg(feature = "reqwest-client")]
fn live_resource_key() -> Option<String> {
    [
        "_51DEGREES_RESOURCE_KEY",
        "_51DEGREES_RESOURCE_KEY_BESPOKE",
        "_51DEGREES_RESOURCE_KEY_BASIC",
    ]
    .into_iter()
    .find_map(|name| match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_owned()),
        _ => None,
    })
}

/// The cloud facade builds a pipeline against the live service and resolves a
/// country code for a public IP through the same `IP_DATA_KEY`. Ignored by
/// default; runs when a resource key is present in the environment.
// Builds a cloud pipeline relying on the built-in reqwest client, so it is gated
// on reqwest-client (which implies cloud). The workspace build unifies it on.
#[cfg(feature = "reqwest-client")]
#[test]
#[ignore = "requires network and a resource key (_51DEGREES_RESOURCE_KEY or the _51DEGREES_RESOURCE_KEY_BESPOKE/_BASIC tiered names)"]
fn cloud_facade_resolves_country_code() {
    let Some(resource_key) = live_resource_key() else {
        eprintln!("no resource key in the environment; skipping live cloud facade test");
        return;
    };

    let pipeline = IpIntelligencePipelineBuilder::cloud(resource_key)
        // Off so the test sends nothing to 51Degrees.
        .share_usage(false)
        .build()
        .expect("the cloud facade pipeline builds");

    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("query.client-ip-51d", "185.28.167.78")
            .build(),
    );
    // Processing must succeed regardless of the key's product tier.
    data.process().expect("live cloud processing succeeds");

    let ip = data.get(IP_DATA_KEY).expect("the facade produced ip data");
    // The country code is only returned when the resource key grants the IP
    // intelligence (location) product. A device-only key processes cleanly but
    // yields no IPI values, so the content assertion is skipped in that case
    // rather than failing.
    match ip.country_code().value() {
        Ok(code) => assert!(
            !code.is_empty(),
            "a country code returned for a public IP should be non-empty"
        ),
        Err(_) => eprintln!(
            "the resource key returned no IP intelligence data (it may not grant the \
             location product); skipping the live country-code assertion"
        ),
    }
}
