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

//! The specification says "Minification SHOULD be performed using an existing
//! third-party library" (pipeline-specification/pipeline-elements/
//! javascript-builder.md line 106). The minified script is the one the oxc
//! parser, minifier and code generator give for the unminified script, so no
//! minifier of this crate's own does the work.

#![cfg(all(feature = "minify", not(target_family = "wasm")))]

use std::sync::Arc;

use fiftyone_javascript_builder::{JavaScriptBuilderElement, JAVASCRIPT_BUILDER_DATA_KEY};
use fiftyone_json_builder::JsonBuilderElement;
use fiftyone_pipeline_core::{Evidence, Pipeline};
use fiftyone_pipeline_engines_fiftyone::constants::{EVIDENCE_SEQUENCE, EVIDENCE_SESSIONID};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

/// The script for one request, with minification on or off. The session and
/// sequence are given, so both scripts are for the same evidence.
fn script(minify: bool) -> String {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::new(
            JavaScriptBuilderElement::builder()
                .set_minify(minify)
                .build(),
        ))
        .build()
        .expect("the pipeline builds");
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("header.host", "example.com")
            .add(EVIDENCE_SESSIONID, "abc")
            .add(EVIDENCE_SEQUENCE, "1")
            .build(),
    );
    data.process().expect("the pipeline processes");
    data.get(JAVASCRIPT_BUILDER_DATA_KEY)
        .expect("the JavaScript builder gives its data")
        .javascript()
        .to_owned()
}

/// `source` minified by the third-party oxc library alone, read as a classic
/// script as a page loads it.
fn minified_by_oxc(source: &str) -> String {
    use oxc::allocator::Allocator;
    use oxc::codegen::{Codegen, CodegenOptions};
    use oxc::minifier::{Minifier, MinifierOptions};
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::script()).parse();
    assert!(
        !parsed.panicked && parsed.diagnostics.is_empty(),
        "the unminified script parses: {:?}",
        parsed.diagnostics
    );
    let mut program = parsed.program;
    let minified = Minifier::new(MinifierOptions::default()).minify(&allocator, &mut program);
    Codegen::new()
        .with_options(CodegenOptions {
            minify: true,
            ..CodegenOptions::default()
        })
        .with_scoping(minified.scoping)
        .build(&program)
        .code
}

#[test]
fn the_script_is_minified_by_a_third_party_library() {
    let plain = script(false);
    let minified = script(true);

    assert!(
        minified.len() < plain.len(),
        "the minified script ({} bytes) is smaller than the unminified one \
         ({} bytes)",
        minified.len(),
        plain.len()
    );
    assert_eq!(
        minified,
        minified_by_oxc(&plain),
        "the minified script is what oxc gives for the unminified one"
    );
}
