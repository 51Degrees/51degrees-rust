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

//! On-premise IP Intelligence lookup-throughput benchmark. See the descriptive
//! block at the bottom of this file for the full write-up.

use std::path::Path;
use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use fiftyone_ip_intelligence::{IpIntelligencePipelineBuilder, PerformanceProfile, IP_DATA_KEY};
use fiftyone_pipeline_core::{Evidence, Pipeline};

/// The relative path to the IP-intelligence evidence file in the data submodule.
/// `examples_shared::find_file` walks up the tree to locate it.
const IPI_EVIDENCE_RELATIVE_PATH: &str = "cxx/data/ip-intelligence/evidence.yml";

/// How many distinct IP addresses to load and replay per benchmark iteration.
/// A few hundred is enough to amortise per-iteration overhead while keeping each
/// iteration short.
const IP_COUNT: usize = 500;

/// The evidence key the lookups are supplied under. The on-premise IP engine
/// accepts a client IP under this key.
const CLIENT_IP_KEY: &str = "query.client-ip";

/// Build the on-premise IP Intelligence pipeline the benchmark looks up through.
///
/// `HighPerformance` favours lookup speed at the cost of memory, which a
/// throughput measurement wants, and requesting a single property (`Asn`) keeps
/// the lookup minimal. The file-system watcher and auto-update are switched off
/// so no background work perturbs the timing. No `ShareUsageElement` is added.
fn build_pipeline(data_file: &Path) -> Arc<Pipeline> {
    IpIntelligencePipelineBuilder::on_premise(data_file)
        .performance_profile(PerformanceProfile::InMemory)
        .properties(["Asn"])
        .auto_update(false)
        .file_system_watcher(false)
        .build()
        .expect("the benchmark on-premise IP Intelligence pipeline should build from the ASN file")
}

/// Load up to `max` IP-address strings from the multi-document YAML evidence file.
///
/// The file is a `---`-separated stream of single-key IP maps (the key is
/// `server.client-ip` in the bundled file). Each chunk is parsed as a generic
/// mapping and the first scalar value taken as the address, so the loader works
/// regardless of the exact evidence-key spelling. This mirrors the IP
/// performance console example's loader.
fn load_ips(path: &Path, max: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut ips = Vec::new();
    for chunk in text.split("\n---") {
        if ips.len() >= max {
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
    ips
}

/// Look every IP up once, reading the result so the optimizer cannot elide the
/// work, and return how many lookups succeeded.
///
/// A failed lookup (for example a malformed address) is not fatal, so only
/// successful lookups are counted.
fn look_up_all(pipeline: &Arc<Pipeline>, ips: &[String]) -> u64 {
    let mut lookups = 0u64;
    for ip in ips {
        let mut data = pipeline
            .create_flow_data_with(Evidence::builder().add(CLIENT_IP_KEY, ip.clone()).build());
        if data.process().is_ok() {
            if let Some(ipi) = data.get(IP_DATA_KEY) {
                // Observe the result so the lookup is not optimized away.
                black_box(&ipi);
                lookups += 1;
            }
        }
    }
    lookups
}

/// Register the IP Intelligence throughput benchmark, or skip cleanly when the
/// ASN data file or evidence file is not present in this checkout.
fn ipi_onprem_throughput(c: &mut Criterion) {
    // Force the ASN tier: it is the small, current, always-loadable file from
    // the data repository, downloaded when it is missing, and the Enterprise
    // share is not reachable off-network, so ASN is the right target for a
    // reproducible bench. With it absent the bench registers nothing and
    // `cargo bench` stays green offline.
    let Some(data_file) = examples_shared::ipi_data_path(examples_shared::IpiTier::Asn) else {
        eprintln!(
            "skipping ipi_onprem_throughput: no ASN IP Intelligence data file found \
             (set {})",
            examples_shared::IPI_PATH_ENV_VAR
        );
        return;
    };
    let Some(evidence_file) = examples_shared::find_file(IPI_EVIDENCE_RELATIVE_PATH) else {
        eprintln!(
            "skipping ipi_onprem_throughput: '{IPI_EVIDENCE_RELATIVE_PATH}' not found \
             (run git submodule update --init --recursive)"
        );
        return;
    };

    let ips = load_ips(&evidence_file, IP_COUNT);
    if ips.is_empty() {
        eprintln!("skipping ipi_onprem_throughput: no IP addresses were loaded");
        return;
    }

    let pipeline = build_pipeline(&data_file);

    // Warm the data file's caches with one untimed pass so the first measured
    // iteration reflects steady state rather than one-off initialization.
    let _ = look_up_all(&pipeline, &ips);

    let mut group = c.benchmark_group("ipi_onprem");
    // Report throughput as lookups per second.
    group.throughput(Throughput::Elements(ips.len() as u64));
    group.bench_with_input(BenchmarkId::new("lookup", ips.len()), &ips, |b, ips| {
        b.iter(|| {
            let lookups = look_up_all(&pipeline, ips);
            black_box(lookups);
        });
    });
    group.finish();
}

criterion_group!(benches, ipi_onprem_throughput);
criterion_main!(benches);

/*
 * @example ipi_onprem_throughput.rs
 *
 * The on-premise IP Intelligence lookup-throughput benchmark. It is the Criterion
 * counterpart of the `ipi-onprem-performance` console example, expressed as a
 * repeatable benchmark so a throughput regression is caught as a measured
 * slowdown.
 *
 * What it measures
 *
 * It loads a few hundred client-IP addresses from `evidence.yml`,
 * builds one on-premise pipeline tuned for speed against the 4.5 ASN `.ipi` file,
 * then times looking every IP up. Criterion is told the element count per
 * iteration via `Throughput::Elements`, so it reports lookups per second.
 *
 * Configuration choices that maximise and stabilise the figure:
 *
 * - The `HighPerformance` profile holds the data set in memory for the fastest
 *   lookups. The other profiles (InMemory, LowMemory, Balanced, Default) trade
 *   memory for speed. Swap the profile in `build_pipeline` to compare them.
 * - A single requested property (`Asn`) keeps the lookup minimal. The result is
 *   read behind `black_box` so the optimizer cannot remove the work.
 * - Auto-update and the file-system watcher are disabled so no background thread
 *   perturbs the timing.
 *
 * Why the ASN tier
 *
 * The bench forces `IpiTier::Asn`: the ASN file is the small, current,
 * always-loadable file from the data repository, and the Enterprise file
 * lives on a network share reachable only inside 51Degrees, so ASN is the
 * reproducible target.
 *
 * Usage sharing is intentionally not enabled, as this is a benchmark tool.
 *
 * Data is resolved through `examples_shared` (the ASN file via `ipi_data_path`,
 * which downloads it when it is missing, and the evidence file via `find_file`).
 * When either cannot be found the benchmark registers nothing and exits cleanly,
 * so `cargo bench` is safe to run without the data submodules.
 */
