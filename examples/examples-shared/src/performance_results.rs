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

//! The performance-results model the performance examples emit.
//!
//! The nightly performance graphs read a JSON file per configuration, in a
//! schema shared across every 51Degrees language repository:
//!
//! ```json
//! {
//!   "HigherIsBetter": { "DetectionsPerSecond": 1234567.0 },
//!   "LowerIsBetter":  { "AvgMillisecsPerDetection": 0.00081 }
//! }
//! ```
//!
//! The example writes this file itself, and CI only copies it into place. CI
//! deliberately does not recover the figure by parsing an example's printed
//! output: a scraped figure is tied to the exact wording and number formatting
//! of that output, so renaming a label or changing a number format would
//! silently stop the graph updating.
//!
//! Both the Device Detection and the IP Intelligence performance examples build
//! their results through this one type, so they emit an identical structure and
//! there is a single definition of the schema to maintain.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// The command-line flag the performance examples take the results path from.
pub const JSON_OUTPUT_FLAG: &str = "--json-output";

/// A set of performance metrics in the shared results schema.
///
/// Metric names are the series keys on the graph, so they must stay stable
/// across runs of the same configuration. Metrics are held in a [`BTreeMap`] so
/// the serialised order is deterministic whatever order they were added in.
///
/// # Examples
///
/// ```
/// use examples_shared::PerformanceResults;
///
/// let results = PerformanceResults::new()
///     .higher_is_better("DetectionsPerSecond", 1_234_567.0)
///     .lower_is_better("AvgMillisecsPerDetection", 0.00081);
/// assert!(results.to_json().contains("\"DetectionsPerSecond\""));
/// ```
#[derive(Debug, Default, Clone, Serialize)]
pub struct PerformanceResults {
    /// Metrics where a higher value is a better result, such as a throughput.
    #[serde(rename = "HigherIsBetter", skip_serializing_if = "BTreeMap::is_empty")]
    higher_is_better: BTreeMap<String, f64>,
    /// Metrics where a lower value is a better result, such as a per-item cost.
    #[serde(rename = "LowerIsBetter", skip_serializing_if = "BTreeMap::is_empty")]
    lower_is_better: BTreeMap<String, f64>,
}

impl PerformanceResults {
    /// An empty result set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a metric where a higher value is a better result.
    #[must_use]
    pub fn higher_is_better(mut self, metric: &str, value: f64) -> Self {
        self.higher_is_better.insert(metric.to_owned(), value);
        self
    }

    /// Add a metric where a lower value is a better result.
    #[must_use]
    pub fn lower_is_better(mut self, metric: &str, value: f64) -> Self {
        self.lower_is_better.insert(metric.to_owned(), value);
        self
    }

    /// Whether any metric has been added. A results file with no metrics carries
    /// no figure, so callers can treat this as an error rather than write one.
    pub fn is_empty(&self) -> bool {
        self.higher_is_better.is_empty() && self.lower_is_better.is_empty()
    }

    /// Render the results as pretty-printed JSON in the shared schema.
    ///
    /// Serialisation of a map of `f64` cannot fail, so this returns the string
    /// directly rather than a `Result`.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("performance results are always serialisable")
    }

    /// Write the results as JSON to `path`, creating the parent directory if it
    /// does not already exist.
    pub fn write_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, format!("{}\n", self.to_json()))
    }
}

/// Take the results path from a `--json-output <path>` argument, if present.
///
/// The performance examples share this so the flag behaves identically in each
/// one, which is what lets a single CI adapter run any of them.
pub fn json_output_path<Arguments, Argument>(arguments: Arguments) -> Option<PathBuf>
where
    Arguments: IntoIterator<Item = Argument>,
    Argument: AsRef<str>,
{
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        let argument = argument.as_ref();
        if argument == JSON_OUTPUT_FLAG {
            return arguments.next().map(|path| PathBuf::from(path.as_ref()));
        }
        // Also accept the `--json-output=<path>` spelling, which is what a shell
        // user is most likely to type.
        if let Some(path) = argument.strip_prefix(&format!("{JSON_OUTPUT_FLAG}=")) {
            return Some(PathBuf::from(path));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialises_both_sections_in_the_shared_schema() {
        let json = PerformanceResults::new()
            .higher_is_better("DetectionsPerSecond", 1000.0)
            .lower_is_better("AvgMillisecsPerDetection", 0.5)
            .to_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["HigherIsBetter"]["DetectionsPerSecond"], 1000.0);
        assert_eq!(parsed["LowerIsBetter"]["AvgMillisecsPerDetection"], 0.5);
    }

    #[test]
    fn omits_an_empty_section() {
        let json = PerformanceResults::new()
            .higher_is_better("LookupsPerSecond", 42.0)
            .to_json();
        assert!(json.contains("HigherIsBetter"));
        assert!(!json.contains("LowerIsBetter"));
    }

    #[test]
    fn an_empty_result_set_is_reported_as_empty() {
        assert!(PerformanceResults::new().is_empty());
        assert!(!PerformanceResults::new().higher_is_better("Any", 1.0).is_empty());
    }

    #[test]
    fn reads_the_json_output_flag_in_both_spellings() {
        assert_eq!(
            json_output_path(["--json-output", "results.json"]),
            Some(PathBuf::from("results.json"))
        );
        assert_eq!(
            json_output_path(["--json-output=results.json"]),
            Some(PathBuf::from("results.json"))
        );
        assert_eq!(json_output_path(["data.hash"]), None);
        // A trailing flag with no value is not a path.
        assert_eq!(json_output_path(["--json-output"]), None);
    }

    #[test]
    fn writes_the_file_and_creates_its_parent_directory() {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let path = directory.path().join("nested").join("results.json");
        PerformanceResults::new()
            .higher_is_better("DetectionsPerSecond", 7.0)
            .write_to(&path)
            .expect("the results file should be written");
        let written = std::fs::read_to_string(&path).expect("the results file should be readable");
        assert!(written.contains("\"DetectionsPerSecond\": 7.0"));
    }
}
