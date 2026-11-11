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

//! The time one read of a property takes through the IP intelligence data,
//! by name and through its typed accessor, once its value has been read, so
//! the figures time finding the property, and the time to build an engine
//! and to look one address up with no names and with every name. They mean
//! something only in an optimised build, so the tests time nothing in a
//! debug build, and `cargo test --release -p
//! fiftyone-ip-intelligence-onpremise --test read_timing -- --nocapture`
//! prints them.

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use fiftyone_ip_intelligence_onpremise::{
    IpIntelligenceData, IpIntelligenceOnPremiseEngine, IpIntelligenceOnPremiseEngineBuilder,
    IP_DATA_KEY,
};
use fiftyone_native::PerformanceProfile;
use fiftyone_pipeline_core::{Evidence, FlowElement, Pipeline};

/// The reads timed for each figure.
const READS: u32 = 1_000_000;

/// The lookups timed for each figure, after as many again untimed.
const LOOKUPS: u32 = 20_000;

/// A Cloudflare public IPv4, in autonomous system AS13335.
const CLIENT_IP: &str = "1.1.1.1";

/// The addresses the timed lookups take in turn.
const CLIENT_IPS: [&str; 3] = [CLIENT_IP, "8.8.8.8", "2606:4700:4700::1111"];

/// The IP Intelligence data file `51DEGREES_IPI_PATH` names, or the free
/// ASN file in the `cxx` submodule.
fn data_file() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("51DEGREES_IPI_PATH") {
        let path = PathBuf::from(path.trim());
        if path.is_file() {
            return Some(path);
        }
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .to_path_buf();
    Some(workspace.join("cxx/data/ip-intelligence/51Degrees-IPIV4AsnIpiV41.ipi"))
        .filter(|path| path.is_file())
}

/// An engine over `path` built with `names`, or with none when it is empty,
/// and the time the build took in milliseconds.
fn build(path: &Path, names: &[String]) -> (Arc<IpIntelligenceOnPremiseEngine>, f64) {
    let start = Instant::now();
    let engine = IpIntelligenceOnPremiseEngineBuilder::new(path)
        .performance_profile(PerformanceProfile::HighPerformance)
        .properties(names.iter().cloned())
        .auto_update(false)
        .file_system_watcher(false)
        .build()
        .expect("the data file loads");
    (Arc::new(engine), start.elapsed().as_secs_f64() * 1000.0)
}

/// The mean time of one lookup through `engine`, making the flow data and
/// processing it, in microseconds.
fn mean_lookup_microseconds(engine: Arc<IpIntelligenceOnPremiseEngine>) -> f64 {
    let element: Arc<dyn FlowElement> = engine;
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("the pipeline builds");
    let look_up = |lookup: u32| {
        let ip = CLIENT_IPS[lookup as usize % CLIENT_IPS.len()];
        let mut data =
            pipeline.create_flow_data_with(Evidence::builder().add("query.client-ip", ip).build());
        data.process().expect("processing succeeds");
        black_box(&data);
    };
    (0..LOOKUPS).for_each(look_up);
    let start = Instant::now();
    (0..LOOKUPS).for_each(look_up);
    start.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(LOOKUPS)
}

#[test]
fn building_and_looking_up_with_no_names_and_with_every_name() {
    if cfg!(debug_assertions) {
        eprintln!("the lookups are timed only in an optimised build; skipping");
        return;
    }
    let Some(path) = data_file() else {
        eprintln!("no IP Intelligence data file found; skipping the lookup timing");
        return;
    };
    let (none, none_build) = build(&path, &[]);
    let every: Vec<String> = none.populated_property_names().to_vec();
    let (named, every_build) = build(&path, &every);
    let none_lookup = mean_lookup_microseconds(none);
    let every_lookup = mean_lookup_microseconds(named);
    println!(
        "No names: built in {none_build:.0} ms, {none_lookup:.2} us a lookup. \
         Every name ({} names): built in {every_build:.0} ms, {every_lookup:.2} us a \
         lookup. Each lookup figure is the mean of {LOOKUPS}.",
        every.len()
    );
}

/// The mean time of one call of `read`, in nanoseconds.
fn mean_nanoseconds(mut read: impl FnMut()) -> f64 {
    let start = Instant::now();
    for _ in 0..READS {
        read();
    }
    start.elapsed().as_nanos() as f64 / f64::from(READS)
}

#[test]
fn a_read_by_name_and_a_typed_read() {
    if cfg!(debug_assertions) {
        eprintln!("the reads are timed only in an optimised build; skipping");
        return;
    }
    let Some(path) = data_file() else {
        eprintln!("no IP Intelligence data file found; skipping the read timing");
        return;
    };
    let (engine, _) = build(&path, &[]);
    let element: Arc<dyn FlowElement> = engine;
    let pipeline = Pipeline::builder()
        .add_element(element)
        .build()
        .expect("the pipeline builds");
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("query.client-ip", CLIENT_IP)
            .build(),
    );
    data.process().expect("processing succeeds");
    let ip = data.get(IP_DATA_KEY).expect("ip data was produced");
    assert_eq!(format!("{:?}", ip.asn()), format!("{:?}", ip.string("Asn")));
    let by_name = mean_nanoseconds(|| {
        black_box(ip.string(black_box("Asn")));
    });
    let typed = mean_nanoseconds(|| {
        black_box(ip.asn());
    });
    println!(
        "Asn read by name {by_name:.1} ns, typed {typed:.1} ns, the mean of {READS} reads each"
    );
}
