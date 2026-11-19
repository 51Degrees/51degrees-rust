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

//! The performance test of minification.
//!
//! The specification says "Performance tests MUST be in place to verify the
//! performance impact of this feature", the feature being minification
//! (pipeline-specification/pipeline-elements/javascript-builder.md line 109).
//! [`minification_stays_within_its_budget`] renders the script with
//! minification off and on and fails when minifying adds more than
//! [`MINIFY_BUDGET`] to a render. It measures, so it is ignored, and the
//! nightly performance workflow runs it in release as its last step.
//! [`the_performance_run_runs_this_test`] checks that the workflow does. The
//! pull request build runs the ignored tests in debug, where a time says
//! nothing about a release, so there the test returns before it measures.
//!
//! The tests measure time or read the workflow from disk, so they are
//! compiled only off WebAssembly, where a test has no file system.
#![cfg(not(target_family = "wasm"))]

use std::path::Path;
#[cfg(feature = "minify")]
use std::sync::Arc;
#[cfg(feature = "minify")]
use std::time::{Duration, Instant};

#[cfg(feature = "minify")]
use fiftyone_javascript_builder::{
    JavaScriptBuilderElement, JavaScriptBuilderElementBuilder, JAVASCRIPT_BUILDER_DATA_KEY,
};
#[cfg(feature = "minify")]
use fiftyone_json_builder::JsonBuilderElement;
#[cfg(feature = "minify")]
use fiftyone_pipeline_core::{Evidence, Pipeline};
#[cfg(feature = "minify")]
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

/// The most minification may add to the median render, in release.
///
/// Minifying the script of about 49,200 bytes to 15,524 bytes added, in
/// release:
///
/// - 0.84 ms to 0.85 ms over five runs on `ubuntu-latest` with 4 processors,
///   the nightly performance runner and the only runner that times it.
/// - 2.34 ms on a Windows 11 developer machine.
/// - 3.63 ms to 3.91 ms over five runs on a shared Windows 11 machine with 4
///   processors, busy with other work.
///
/// The limit is over six times the highest of these, leaving room for a busy
/// runner while still failing on a change that makes minifying many times
/// slower. `minify::tests::minifying_uses_its_threads_again` keeps the
/// minifier from starting a thread for each request, which cost more than the
/// minifying.
#[cfg(feature = "minify")]
const MINIFY_BUDGET: Duration = Duration::from_millis(25);

/// Renders timed for each configuration, after one untimed render.
#[cfg(feature = "minify")]
const RENDERS: usize = 201;

/// A pipeline of the Sequence element, the JSON builder and the JavaScript
/// builder, as a client-side integration has.
#[cfg(feature = "minify")]
fn pipeline(
    configure: impl FnOnce(JavaScriptBuilderElementBuilder) -> JavaScriptBuilderElement,
) -> Arc<Pipeline> {
    Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::new(configure(JavaScriptBuilderElement::builder())))
        .build()
        .expect("the JavaScript builder pipeline builds")
}

/// Process one request with a host, so the script has its callback URL, and
/// return the script.
#[cfg(feature = "minify")]
fn render(pipeline: &Arc<Pipeline>) -> String {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.host", "example.com")
            .build(),
    );
    data.process().expect("the pipeline processes");
    data.get(JAVASCRIPT_BUILDER_DATA_KEY)
        .expect("the JavaScript builder gives its data")
        .javascript()
        .to_owned()
}

/// The median time of [`RENDERS`] renders, after one untimed render that
/// compiles the template. The median leaves out the odd render slowed by
/// other work on the machine.
#[cfg(feature = "minify")]
fn median_render(pipeline: &Arc<Pipeline>) -> (Duration, usize) {
    let length = render(pipeline).len();
    let mut times: Vec<Duration> = (0..RENDERS)
        .map(|_| {
            let start = Instant::now();
            let script = render(pipeline);
            let elapsed = start.elapsed();
            std::hint::black_box(script);
            elapsed
        })
        .collect();
    times.sort();
    (times[times.len() / 2], length)
}

/// Minification makes the script smaller and adds no more than
/// [`MINIFY_BUDGET`] to a render.
#[cfg(feature = "minify")]
#[test]
#[ignore = "measures time, so the nightly performance workflow runs it in release"]
fn minification_stays_within_its_budget() {
    if cfg!(debug_assertions) {
        println!("Not measured, as the budget is for a release build.");
        return;
    }
    let plain = pipeline(|builder| builder.set_minify(false).build());
    let minified = pipeline(|builder| builder.set_minify(true).build());

    let (plain_time, plain_length) = median_render(&plain);
    let (minified_time, minified_length) = median_render(&minified);
    let impact = minified_time.saturating_sub(plain_time);
    println!(
        "Median render: {plain_time:?} for {plain_length} bytes unminified, \
         {minified_time:?} for {minified_length} bytes minified. Minifying adds \
         {impact:?}, against a budget of {MINIFY_BUDGET:?}."
    );

    assert!(
        minified_length < plain_length,
        "the minified script ({minified_length} bytes) is smaller than the \
         unminified one ({plain_length} bytes)"
    );
    assert!(
        impact <= MINIFY_BUDGET,
        "minifying adds {impact:?} to the median render, more than the \
         budget of {MINIFY_BUDGET:?}"
    );
}

/// The nightly performance workflow runs
/// [`minification_stays_within_its_budget`] in release with the ignored tests,
/// in a step after the last upload of the night's figures, so a failure here
/// costs none of them. `ci/run-performance-tests.ps1`, whose failure stops the
/// uploads, does not run it.
#[test]
fn the_performance_run_runs_this_test() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let read = |relative: &str| {
        let path = root.join(relative);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{} reads: {error}", path.display()))
    };
    let runs_it = |line: &str| {
        line.contains("cargo test")
            && line.contains("--release")
            && line.contains("-p fiftyone-javascript-builder")
            && line.contains("--test performance")
            && line.contains("--ignored")
    };
    let workflow = read(".github/workflows/nightly-performance.yml");
    let steps: Vec<&str> = workflow
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .collect();
    let test = steps.iter().position(|line| runs_it(line));
    let last_upload = steps
        .iter()
        .rposition(|line| line.contains("actions/upload-artifact"));
    assert!(
        matches!((test, last_upload), (Some(test), Some(upload)) if test > upload),
        "nightly-performance.yml runs `cargo test --release -p \
         fiftyone-javascript-builder --test performance -- --ignored` after \
         its last upload"
    );

    let script = read("ci/run-performance-tests.ps1");
    assert!(
        !script
            .lines()
            .map(str::trim)
            .any(|line| !line.starts_with('#') && runs_it(line)),
        "ci/run-performance-tests.ps1 leaves the minification test to its own \
         step"
    );
}
