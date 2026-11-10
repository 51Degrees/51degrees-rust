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

//! A refresh to a data file of another tier leaves the engine reporting what
//! an engine built on that file reports.
//!
//! The test needs the ASN file `_51DEGREES_IPI_PATH` names and the Lite file
//! `51Degrees-LiteV41.ipi` beside it, and skips with a note when either is
//! missing.

use std::path::PathBuf;

use fiftyone_ip_intelligence_onpremise::IpIntelligenceOnPremiseEngine;
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_engines::{AspectEngine, OnPremiseAspectEngine};

/// The ASN file and the Lite file beside it.
fn data_files() -> Option<(PathBuf, PathBuf)> {
    let asn = PathBuf::from(std::env::var("_51DEGREES_IPI_PATH").ok()?);
    let lite = asn.parent()?.join("51Degrees-LiteV41.ipi");
    (asn.is_file() && lite.is_file()).then_some((asn, lite))
}

/// An engine that holds the whole data set in memory, so its data file can
/// be replaced on every platform.
fn engine(path: &PathBuf) -> IpIntelligenceOnPremiseEngine {
    IpIntelligenceOnPremiseEngine::builder(path)
        .performance_profile(PerformanceProfile::InMemory)
        .auto_update(false)
        .build()
        .expect("the data file loads")
}

#[test]
fn a_refresh_reports_the_tier_and_publish_date_of_the_new_data_file() {
    let Some((asn, lite)) = data_files() else {
        eprintln!(
            "skipping the refresh test: set _51DEGREES_IPI_PATH to the ASN file, with \
             51Degrees-LiteV41.ipi beside it"
        );
        return;
    };
    let dir = std::env::temp_dir().join(format!("fiftyone-ipi-refresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("51Degrees.ipi");
    std::fs::copy(&asn, &path).unwrap();

    let refreshed = engine(&path);
    let asn_tier = refreshed.data_source_tier().to_owned();
    std::fs::copy(&lite, &path).unwrap();
    refreshed.refresh(None).expect("the Lite file loads");
    let built = engine(&path);

    assert_ne!(built.data_source_tier(), asn_tier, "the files share a tier");
    assert_eq!(refreshed.data_source_tier(), built.data_source_tier());
    assert_eq!(refreshed.aspect_properties(), built.aspect_properties());
    assert_eq!(refreshed.data_file_published(), built.data_file_published());

    drop(refreshed);
    drop(built);
    let _ = std::fs::remove_dir_all(&dir);
}
