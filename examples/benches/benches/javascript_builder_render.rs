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

//! JavaScript-builder template-render benchmark. See the descriptive block at the
//! bottom of this file for the full write-up.

use std::sync::Arc;

use criterion::{black_box, criterion_group, criterion_main, Criterion};

use fiftyone_javascript_builder::{
    JavaScriptBuilderElement, JavaScriptBuilderElementBuilder, JAVASCRIPT_BUILDER_DATA_KEY,
};
use fiftyone_json_builder::JsonBuilderElement;
use fiftyone_pipeline_core::{Evidence, Pipeline};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

/// Build a pipeline of the Sequence element, the JSON builder and the
/// JavaScript builder, configured by `configure`.
///
/// The JSON builder produces the JSON payload the JavaScript-builder template
/// embeds, so the two run together as they do in a real client-side pipeline.
/// The JSON builder needs the sequence number the Sequence element gives.
/// This mirrors the JavaScript-builder integration test's pipeline assembly.
fn build_pipeline(
    configure: impl FnOnce(JavaScriptBuilderElementBuilder) -> JavaScriptBuilderElement,
) -> Arc<Pipeline> {
    let js_element = configure(JavaScriptBuilderElement::builder());
    Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::new(js_element))
        .build()
        .expect("the JSON + JavaScript builder pipeline should build")
}

/// Render the JavaScript once: process a flow data carrying a host header (so the
/// callback URL and the update mechanism are emitted, the fuller template path)
/// and return the rendered length, read behind `black_box` so the render cannot
/// be optimized away.
fn render_once(pipeline: &Arc<Pipeline>) -> usize {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.host", "example.com")
            .build(),
    );
    data.process()
        .expect("the JavaScript-builder pipeline should process");
    let javascript = data
        .get(JAVASCRIPT_BUILDER_DATA_KEY)
        .expect("the JavaScript-builder element should have produced data")
        .javascript()
        .to_owned();
    black_box(javascript.len())
}

/// Register the template-render benchmarks. The non-minified render is always
/// measured. The minified render is added only when the `minify` feature is
/// compiled in, so the budget for each path is guarded by the corresponding
/// build.
fn javascript_builder_render(c: &mut Criterion) {
    let mut group = c.benchmark_group("javascript_builder");

    // The plain template render: minification off, so the element emits the
    // unminified Mustache output. This is the default behavior of the crate.
    let plain = build_pipeline(|b| b.set_minify(false).build());
    // Warm the lazily-compiled template so the first measured iteration is steady
    // state rather than the one-off compile.
    let _ = render_once(&plain);
    group.bench_function("render", |b| {
        b.iter(|| {
            let len = render_once(&plain);
            black_box(len);
        });
    });

    // The minify path is only present when the crate is built with the `minify`
    // feature, so the benchmark for it is compiled in under the same gate. When
    // minification is off the element falls back to the unminified output, so
    // benchmarking the minify arm without the feature would measure the same
    // work twice under two names, which the gate avoids.
    #[cfg(feature = "minify")]
    {
        let minified = build_pipeline(|b| b.set_minify(true).build());
        let _ = render_once(&minified);
        group.bench_function("render_minified", |b| {
            b.iter(|| {
                let len = render_once(&minified);
                black_box(len);
            });
        });
    }

    group.finish();
}

criterion_group!(benches, javascript_builder_render);
criterion_main!(benches);

/*
 * @example javascript_builder_render.rs
 *
 * The JavaScript-builder template-render benchmark. It guards the template-render
 * budget the pipeline specification calls for, so a regression in the Mustache
 * render (or, under the `minify` feature, the minify path) shows up as a measured
 * slowdown rather than slipping by unnoticed.
 *
 * What it measures
 *
 * It assembles the same pipeline a real client-side integration uses, the JSON
 * builder feeding the JavaScript builder, then times one full render: processing
 * a flow data and reading back the generated JavaScript. The flow data carries a
 * host header so the callback URL and the update mechanism are emitted, which
 * exercises the fuller template path rather than the trivial no-URL case.
 *
 * Two benchmarks, gated by feature
 *
 * - `render`. The default, unminified render. Always measured.
 * - `render_minified`. The minify path, compiled in and measured only when the
 *   crate's `minify` feature is enabled (run `cargo bench -p examples-benches
 *   --features minify`). Without the feature the element falls back to the
 *   unminified output, so benchmarking the minify arm then would just measure the
 *   same render twice, which the `#[cfg(feature = "minify")]` gate avoids.
 *
 * The rendered length is read behind `black_box` on every iteration so the
 * compiler cannot elide the render being measured.
 *
 * This benchmark needs no data files, so it always runs. It depends on the
 * JavaScript builder directly because that element is not surfaced through either
 * detection facade.
 */
