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

//! Inspect an on-premise engine's data file and report standard warnings.
//!
//! The example caller passes the on-premise engine it built and prints any
//! returned warnings, so an out-of-date or limited-accuracy data file is
//! surfaced rather than silently degrading results.

use chrono::{Duration, Utc};

use fiftyone_pipeline_engines::OnPremiseAspectEngine;

/// A data file older than this many days triggers the age warning.
///
/// The threshold is 30 days, which stays well within the 28-day bound the
/// warning must honour.
pub const DATA_FILE_AGE_WARNING_DAYS: i64 = 30;

/// The pricing page, as the out-of-date warning links it.
const PRICING_URL_AGE: &str = "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-examples-shared-src-data_file_check.rs&utm_term=data-file-age";

/// The pricing page, as the free data file warning links it.
const PRICING_URL_FREE: &str = "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-examples-shared-src-data_file_check.rs&utm_term=free-data-file";

/// Inspect `engine`'s data file and return the warnings an example should print.
///
/// The returned vector is empty when nothing is wrong. It contains a warning
/// when:
///
/// - the data file's publish date is more than [`DATA_FILE_AGE_WARNING_DAYS`]
///   days in the past (a newer file may be needed to detect the latest devices
///   or networks), or
/// - the engine's data-source tier is `"Lite"` (illustration-only data with
///   limited accuracy).
///
/// When the publish date is unknown (the engine reports none) the age check is
/// skipped rather than guessed. The messages are returned for the caller to
/// print instead of writing to a logger, so the helper stays logging-framework
/// agnostic.
pub fn check_data_file(engine: &dyn OnPremiseAspectEngine) -> Vec<String> {
    let mut warnings = Vec::new();

    if let Some(published) = engine.data_file_published() {
        let threshold = Utc::now() - Duration::days(DATA_FILE_AGE_WARNING_DAYS);
        if published < threshold {
            warnings.push(format!(
                "This example is using a data file that is more than \
                 {DATA_FILE_AGE_WARNING_DAYS} days old (published {published}). A more recent \
                 data file may be needed to correctly detect the latest devices, browsers and \
                 networks. The get-data-files script in cxx/data replaces a free file with a \
                 newer one when run with -u, or -Update in PowerShell. The Enterprise file, \
                 with automatic daily updates, is described at {PRICING_URL_AGE}"
            ));
        }
    }

    // The tier comes from the AspectEngine super-trait, so a free, limited file
    // is flagged regardless of its age.
    let tier = engine.data_source_tier();
    if is_limited_data_tier(tier) {
        warnings.push(format!(
            "This example is using a '{tier}' data file. This is free data for illustration \
             only, with limited accuracy and capabilities. The Enterprise file is described \
             at {PRICING_URL_FREE}"
        ));
    }

    warnings
}

/// Whether `tier` names a free, limited-accuracy data file that the examples
/// should flag.
///
/// This covers the Device Detection `Lite` tier and the IP Intelligence free
/// `Asn` file (whose data-set name carries `Asn`, for example `IPIV4Asn`). Both
/// are illustration-only, in contrast to the paid Enterprise and TAC files.
fn is_limited_data_tier(tier: &str) -> bool {
    let tier = tier.to_ascii_lowercase();
    tier == "lite" || tier.contains("asn")
}

/// A short, single-line summary of the data file `engine` is using, suitable for
/// an informational log line before any warnings.
///
/// Reports the tier and, when known, the publish date.
pub fn data_file_info(engine: &dyn OnPremiseAspectEngine) -> String {
    match engine.data_file_published() {
        Some(published) => format!(
            "Using a '{}' data file published {published}.",
            engine.data_source_tier()
        ),
        None => format!(
            "Using a '{}' data file (publish date unknown).",
            engine.data_source_tier()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use chrono::DateTime;
    use fiftyone_pipeline_core::{
        EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, PropertyMetaData,
        Result,
    };
    use fiftyone_pipeline_engines::{
        AspectEngine, AspectEngineDataFile, AspectPropertyMetaData, DataFileConfiguration,
    };

    // A minimal on-premise engine stand-in whose tier and publish date the tests
    // control directly. It does no real processing.
    struct FakeOnPremiseEngine {
        tier: String,
        filter: EvidenceKeyFilterWhitelist,
        properties: Vec<PropertyMetaData>,
        aspect_properties: Vec<AspectPropertyMetaData>,
        files: Vec<Arc<AspectEngineDataFile>>,
    }

    impl FakeOnPremiseEngine {
        fn new(tier: &str, published: Option<DateTime<Utc>>) -> Self {
            let file = Arc::new(AspectEngineDataFile::new(
                DataFileConfiguration::builder("fake.dat").build(),
            ));
            if let Some(when) = published {
                file.set_data_published(when);
            }
            FakeOnPremiseEngine {
                tier: tier.to_owned(),
                filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
                properties: Vec::new(),
                aspect_properties: Vec::new(),
                files: vec![file],
            }
        }
    }

    impl FlowElement for FakeOnPremiseEngine {
        fn process(&self, _data: &mut FlowData) -> Result<()> {
            Ok(())
        }
        fn data_key(&self) -> &str {
            "fake"
        }
        fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
            &self.filter
        }
        fn properties(&self) -> &[PropertyMetaData] {
            &self.properties
        }
    }

    impl AspectEngine for FakeOnPremiseEngine {
        fn data_source_tier(&self) -> &str {
            &self.tier
        }
        fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
            &self.aspect_properties
        }
    }

    impl OnPremiseAspectEngine for FakeOnPremiseEngine {
        fn data_files(&self) -> &[Arc<AspectEngineDataFile>] {
            &self.files
        }
        fn refresh(&self, _data_file_identifier: Option<&str>) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn recent_enterprise_file_has_no_warnings() {
        let engine = FakeOnPremiseEngine::new("Enterprise", Some(Utc::now()));
        assert!(check_data_file(&engine).is_empty());
    }

    #[test]
    fn old_file_warns_about_age() {
        let old = Utc::now() - Duration::days(DATA_FILE_AGE_WARNING_DAYS + 5);
        let engine = FakeOnPremiseEngine::new("Premium", Some(old));
        let warnings = check_data_file(&engine);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("more than"));
    }

    #[test]
    fn lite_tier_warns_even_when_recent() {
        let engine = FakeOnPremiseEngine::new("Lite", Some(Utc::now()));
        let warnings = check_data_file(&engine);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Lite"));
    }

    #[test]
    fn asn_tier_warns_even_when_recent() {
        // The IP Intelligence free file reports its data-set name, for example
        // `IPIV4Asn`, which must be flagged as free, limited data.
        let engine = FakeOnPremiseEngine::new("IPIV4Asn", Some(Utc::now()));
        let warnings = check_data_file(&engine);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("IPIV4Asn"));
        assert!(warnings[0].contains("free data"));
    }

    #[test]
    fn old_lite_file_warns_twice() {
        let old = Utc::now() - Duration::days(DATA_FILE_AGE_WARNING_DAYS + 1);
        let engine = FakeOnPremiseEngine::new("Lite", Some(old));
        assert_eq!(check_data_file(&engine).len(), 2);
    }

    #[test]
    fn unknown_publish_date_skips_age_check() {
        let engine = FakeOnPremiseEngine::new("Enterprise", None);
        assert!(check_data_file(&engine).is_empty());
        assert!(data_file_info(&engine).contains("publish date unknown"));
    }
}
