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

//! @page ipi-onprem-performance-example Performance (IP Intelligence, On-premise)
//!
//! On-premise IP Intelligence performance (throughput) console example.
//!
//! Benchmarks how many IP lookups per second the on-premise engine sustains,
//! single- and multi-threaded, reading the evidence from the data submodule.
//! The descriptive block the documentation tooling renders lives at the bottom
//! of the file.
//!
//! @snippet ipi-onprem-performance.rs example

use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use fiftyone_ip_intelligence::{IpIntelligencePipelineBuilder, PerformanceProfile, IP_DATA_KEY};
use fiftyone_pipeline_core::{Evidence, Pipeline};

/// The relative path to the IP-intelligence evidence file in the data submodule.
const IPI_EVIDENCE_RELATIVE_PATH: &str = "cxx/data/ip-intelligence/evidence.yml";

/// The options that drive a run of this example.
pub struct ExampleOptions {
    /// The full path to the `.ipi` data file to open.
    pub data_file: std::path::PathBuf,
    /// The full path to the YAML evidence file to read.
    pub evidence_file: std::path::PathBuf,
    /// How many worker threads to run the multi-threaded pass with.
    pub thread_count: usize,
    /// The maximum number of distinct IP addresses to load from the evidence
    /// file. Each thread loops over the loaded set.
    pub max_ips: usize,
    /// How many times each thread loops over the loaded IP set.
    pub iterations: usize,
    /// Where to write the results JSON the nightly performance graphs read, if
    /// anywhere. CI passes `--json-output <path>`; an interactive run leaves it
    /// unset and only reads the printed figures.
    pub json_output: Option<std::path::PathBuf>,
}

impl ExampleOptions {
    /// Default options, using the best-available loadable data-file tier and a
    /// thread count taken from the available parallelism.
    pub fn from_env() -> Option<Self> {
        let options = Self::for_tier(examples_shared::IpiTier::BestAvailable, 500, 4)?;
        Some(ExampleOptions {
            json_output: examples_shared::json_output_path(std::env::args().skip(1)),
            ..options
        })
    }

    /// Options pinned to a specific data-file tier, IP cap and iteration count.
    /// The test pins the ASN tier and a tiny workload so it runs quickly and
    /// deterministically against the always-loadable ASN file.
    pub fn for_tier(
        tier: examples_shared::IpiTier,
        max_ips: usize,
        iterations: usize,
    ) -> Option<Self> {
        Some(ExampleOptions {
            data_file: examples_shared::ipi_data_path(tier)?,
            evidence_file: examples_shared::find_file(IPI_EVIDENCE_RELATIVE_PATH)?,
            thread_count: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4),
            max_ips,
            iterations,
            json_output: None,
        })
    }
}

/// The result of one benchmark pass.
struct BenchmarkResult {
    /// Total lookups performed across all threads.
    lookups: u64,
    /// Wall-clock time the pass took.
    elapsed_ms: f64,
}

impl BenchmarkResult {
    /// Lookups per second across the whole pass.
    fn lookups_per_second(&self) -> f64 {
        if self.elapsed_ms <= 0.0 {
            return 0.0;
        }
        (self.lookups as f64) / (self.elapsed_ms / 1000.0)
    }

    /// Mean milliseconds per lookup.
    fn ms_per_lookup(&self) -> f64 {
        if self.lookups == 0 {
            return 0.0;
        }
        self.elapsed_ms / (self.lookups as f64)
    }
}

// [example]
/// Run the performance example, writing all output to `out`.
///
/// Usage sharing is not enabled, as this is a console example.
pub fn run(options: &ExampleOptions, out: &mut dyn Write) -> anyhow::Result<()> {
    // InMemory loads the whole data set into memory for the fastest lookups,
    // which is what a throughput benchmark wants. The other profiles
    // (HighPerformance, LowMemory, Balanced, Default) trade memory, load time and
    // lookup speed differently. The performance options documentation explains
    // each one (the compare example shows them side by side):
    // https://51degrees.com/documentation/_device_detection__features__performance_options.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-performance.rs&utm_term=performance-options
    let pipeline: Arc<Pipeline> = IpIntelligencePipelineBuilder::on_premise(&options.data_file)
        .performance_profile(PerformanceProfile::InMemory)
        .properties(["Asn"])
        .auto_update(false)
        .file_system_watcher(false)
        .build()
        .context("failed to build the on-premise IP Intelligence pipeline")?;

    let ips = Arc::new(
        load_ips(&options.evidence_file, options.max_ips).with_context(|| {
            format!(
                "failed to read evidence from {}",
                options.evidence_file.display()
            )
        })?,
    );
    anyhow::ensure!(
        !ips.is_empty(),
        "no IP addresses were loaded from the evidence file"
    );

    writeln!(
        out,
        "Loaded {} IP address(es); running {} iteration(s) per thread.",
        ips.len(),
        options.iterations
    )?;

    // Warm the data file's caches with a single-threaded pass that is not timed
    // into the headline numbers, so the benchmark measures steady state.
    writeln!(out)?;
    writeln!(out, "Warming up ...")?;
    let _ = benchmark(&pipeline, &ips, 1, options.iterations);

    writeln!(out, "Single-threaded pass ...")?;
    let single = benchmark(&pipeline, &ips, 1, options.iterations);
    report(out, "Single-threaded", &single, 1)?;

    writeln!(out)?;
    writeln!(
        out,
        "Multi-threaded pass ({} threads) ...",
        options.thread_count
    )?;
    let multi = benchmark(&pipeline, &ips, options.thread_count, options.iterations);
    report(out, "Multi-threaded", &multi, options.thread_count)?;

    // The nightly performance graphs read this file. The example writes it
    // itself so the figure does not depend on the wording or number formatting
    // of the printed report above, which is free to change. The multi-threaded
    // pass is the headline figure for the product, so it is the one published.
    if let Some(json_output) = &options.json_output {
        let results = examples_shared::PerformanceResults::new()
            .higher_is_better("LookupsPerSecond", multi.lookups_per_second())
            .lower_is_better("AvgMillisecsPerLookup", multi.ms_per_lookup());
        results.write_to(json_output).with_context(|| {
            format!(
                "failed to write the performance results to '{}'",
                json_output.display()
            )
        })?;
        writeln!(
            out,
            "Wrote performance results to '{}'.",
            json_output.display()
        )?;
    }

    Ok(())
}
// [example]

/// Run a benchmark pass: `thread_count` threads, each looping `iterations`
/// times over the shared IP set, looking each IP up through the pipeline.
fn benchmark(
    pipeline: &Arc<Pipeline>,
    ips: &Arc<Vec<String>>,
    thread_count: usize,
    iterations: usize,
) -> BenchmarkResult {
    let total = Arc::new(AtomicU64::new(0));
    let started = Instant::now();

    std::thread::scope(|scope| {
        for _ in 0..thread_count {
            let pipeline = Arc::clone(pipeline);
            let ips = Arc::clone(ips);
            let total = Arc::clone(&total);
            scope.spawn(move || {
                let mut local: u64 = 0;
                for _ in 0..iterations {
                    for ip in ips.iter() {
                        let mut data = pipeline.create_flow_data_with(
                            Evidence::builder().add("query.client-ip", ip).build(),
                        );
                        // A failed lookup (for example a malformed address) is
                        // not fatal to the benchmark; only successful lookups
                        // are counted, and the result is read to ensure the
                        // work is not optimized away.
                        if data.process().is_ok() && data.get(IP_DATA_KEY).is_some() {
                            local += 1;
                        }
                    }
                }
                total.fetch_add(local, Ordering::Relaxed);
            });
        }
    });

    BenchmarkResult {
        lookups: total.load(Ordering::Relaxed),
        elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
    }
}

/// Write a benchmark result as lookups, time, throughput and per-lookup cost.
fn report(
    out: &mut dyn Write,
    label: &str,
    result: &BenchmarkResult,
    thread_count: usize,
) -> anyhow::Result<()> {
    writeln!(out, "{label} result:")?;
    writeln!(out, "\tThreads: {thread_count}")?;
    writeln!(out, "\tLookups: {}", result.lookups)?;
    writeln!(out, "\tElapsed: {:.1} ms", result.elapsed_ms)?;
    writeln!(
        out,
        "\tThroughput: {:.0} lookups/sec",
        result.lookups_per_second()
    )?;
    writeln!(out, "\tMean: {:.4} ms/lookup", result.ms_per_lookup())?;
    Ok(())
}

/// Load up to `max_ips` distinct client-IP values from the evidence file.
///
/// The evidence file is a `---`-separated stream of single-key IP maps. Each
/// document's value is taken as an IP address. Splitting on the delimiter and
/// reading each chunk as a generic value keeps the loader simple.
fn load_ips(path: &std::path::Path, max_ips: usize) -> anyhow::Result<Vec<String>> {
    let text = std::fs::read_to_string(path)?;
    let mut ips = Vec::new();
    for chunk in text.split("\n---") {
        if ips.len() >= max_ips {
            break;
        }
        let chunk = chunk.trim();
        if chunk.is_empty() || chunk == "---" {
            continue;
        }
        let value: serde_norway::Value = match serde_norway::from_str(chunk) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if let serde_norway::Value::Mapping(mapping) = value {
            for (_key, val) in mapping {
                if let Some(ip) = val.as_str() {
                    ips.push(ip.to_owned());
                    break;
                }
            }
        }
    }
    Ok(ips)
}

/// Read the data file and evidence file, then run the example.
fn main() -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let options = match ExampleOptions::from_env() {
        Some(options) => options,
        None => anyhow::bail!(
            "Could not locate both an IP Intelligence data file and the \
             evidence.yml file. Set {} to an .ipi file, and run git submodule \
             update --init --recursive so cxx/data/ip-intelligence/evidence.yml \
             is present.",
            examples_shared::IPI_PATH_ENV_VAR
        ),
    };

    run(&options, &mut out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn performance_benchmark_reports_throughput() {
        // A tiny workload (50 IPs, 1 iteration) so the test is quick.
        let Some(options) = ExampleOptions::for_tier(examples_shared::IpiTier::Asn, 50, 1) else {
            eprintln!("no usable IP Intelligence data file or evidence file; skipping perf run");
            return;
        };

        let mut buffer = Vec::new();
        run(&options, &mut buffer).expect("the performance example should run");

        let printed = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(printed.contains("Single-threaded result"));
        assert!(printed.contains("Multi-threaded result"));
        assert!(printed.contains("lookups/sec"));
    }

    /// The results file CI publishes must be written in the shared schema, with
    /// the throughput metric the performance graph is keyed on.
    #[test]
    fn writes_the_results_json_when_asked() {
        let Some(options) = ExampleOptions::for_tier(examples_shared::IpiTier::Asn, 50, 1) else {
            eprintln!("no usable IP Intelligence data file or evidence file; skipping perf run");
            return;
        };
        let json_output = std::env::temp_dir()
            .join("ipi-onprem-performance-results")
            .join("results.json");
        let _ = std::fs::remove_file(&json_output);

        let options = ExampleOptions {
            json_output: Some(json_output.clone()),
            ..options
        };
        run(&options, &mut Vec::new()).expect("the performance example should run");

        let written =
            std::fs::read_to_string(&json_output).expect("the results file should be written");
        let parsed: serde_json::Value =
            serde_json::from_str(&written).expect("the results file should be valid JSON");
        assert!(
            parsed["HigherIsBetter"]["LookupsPerSecond"].is_number(),
            "the results file should carry the graph metric, got: {written}"
        );
    }
}

/*
 * @example ipi-onprem-performance.rs
 *
 * The on-premise IP Intelligence performance example.
 *
 * It shows how to:
 *
 * 1. Build an on-premise IP Intelligence pipeline tuned for throughput with the
 *    `InMemory` profile. The other profiles (HighPerformance, LowMemory,
 *    Balanced, Default) are documented inline so they can be swapped in to
 *    compare their memory/speed trade-offs.
 * 2. Load a set of IP addresses from the evidence file in the data submodule.
 * 3. Run a warm-up pass, then a single-threaded and a multi-threaded timed
 *    pass, looking every IP up through the pipeline. The pipeline is shared
 *    across threads via an `Arc`, demonstrating that the on-premise engine is
 *    safe to use concurrently.
 * 4. Report the headline metrics: total lookups, elapsed time, throughput
 *    (lookups per second) and the mean time per lookup.
 *
 * # Data file
 *
 * Resolved by `examples_shared::ipi_data_path`. Contact 51Degrees for an
 * Enterprise file: <https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-bin-ipi-onprem-performance.rs&utm_term=ipi-onprem-performance>.
 *
 * # Usage sharing
 *
 * Not enabled, because this is a console example. Production deployments are
 * encouraged to enable usage sharing.
 */
