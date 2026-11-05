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

//! The steps a caller can replace, and a template the caller gives.

use std::sync::Arc;

use fiftyone_javascript_builder::{
    default_steps, DefaultJavaScriptBuilderSteps, JavaScriptBuilderContext,
    JavaScriptBuilderElement, JavaScriptBuilderElementBuilder, JavaScriptBuilderSteps,
    JAVASCRIPT_BUILDER_DATA_KEY,
};
use fiftyone_json_builder::JsonBuilderElement;
use fiftyone_pipeline_core::{Error, Evidence, Pipeline};

/// A statement found only inside the user prompt section of the template.
const USER_PROMPT_MARKER: &str = "var answerKeys = [\"id.usage\", \"tcstring\"];";

/// The evidence every test here sends, giving a callback URL so the update
/// section, and the user prompt section inside it, can be rendered.
const EVIDENCE: &[(&str, &str)] = &[
    ("header.host", "example.com"),
    ("query.x", "1"),
    ("query.session-id", "from-evidence"),
    ("query.sequence", "3"),
];

/// Run a pipeline of the JSON builder and the JavaScript builder, built by
/// `configure` with minification off, and return the script.
fn script(
    configure: impl FnOnce(JavaScriptBuilderElementBuilder) -> JavaScriptBuilderElementBuilder,
) -> String {
    let element = configure(JavaScriptBuilderElement::builder().set_minify(false)).build();
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(JsonBuilderElement::new()))
        .add_element(Arc::new(element))
        .build()
        .expect("pipeline builds");
    let mut evidence = Evidence::builder();
    for (key, value) in EVIDENCE {
        evidence = evidence.add(*key, *value);
    }
    let mut data = pipeline.create_flow_data_with(evidence.build());
    data.process().expect("processing succeeds");
    data.get(JAVASCRIPT_BUILDER_DATA_KEY)
        .expect("javascript builder data present")
        .javascript()
        .to_owned()
}

/// Steps giving fixed values, so each one can be seen in the script.
struct Fixed {
    session_id: &'static str,
    sequence: i32,
}

impl JavaScriptBuilderSteps for Fixed {
    fn get_parameters(&self, _: &JavaScriptBuilderContext<'_>) -> Vec<(String, String)> {
        vec![("a".to_owned(), "1".to_owned())]
    }

    fn get_sequence(&self, _: &JavaScriptBuilderContext<'_>) -> i32 {
        self.sequence
    }

    fn get_session_id(&self, _: &JavaScriptBuilderContext<'_>) -> String {
        self.session_id.to_owned()
    }

    fn render_user_prompt(&self, _: &JavaScriptBuilderContext<'_>) -> bool {
        true
    }
}

#[test]
fn each_replaced_step_reaches_the_script() {
    let js = script(|b| {
        b.set_steps(Fixed {
            session_id: "abc-123",
            sequence: 5,
        })
    });
    assert!(js.contains("return {\"a\":\"1\"};"), "the parameters step");
    assert!(
        js.contains("var sessionId = \"abc-123\";"),
        "the session id step"
    );
    assert!(js.contains("var sequence = 5;"), "the sequence step");
    assert!(js.contains(USER_PROMPT_MARKER), "the user prompt step");
}

#[test]
fn the_default_steps_give_the_builders_own_script() {
    let own = script(|b| b);
    let defaults = script(|b| b.set_steps(DefaultJavaScriptBuilderSteps));
    assert_eq!(own, defaults);
    assert!(own.contains("return {\"x\":\"1\"};"));
    assert!(own.contains("var sessionId = \"from-evidence\";"));
    assert!(own.contains("var sequence = 3;"));
    assert!(!own.contains(USER_PROMPT_MARKER));
}

/// Adds a parameter after the ones the default step gives.
struct WithRegion;

impl JavaScriptBuilderSteps for WithRegion {
    fn get_parameters(&self, context: &JavaScriptBuilderContext<'_>) -> Vec<(String, String)> {
        let mut parameters = default_steps::get_parameters(context);
        parameters.push(("region".to_owned(), "eu".to_owned()));
        parameters
    }
}

#[test]
fn a_step_keeps_the_default_for_part_of_its_work() {
    let js = script(|b| b.set_steps(WithRegion));
    assert!(js.contains("return {\"x\":\"1\",\"region\":\"eu\"};"));
    // The steps it does not replace keep their defaults.
    assert!(js.contains("var sessionId = \"from-evidence\";"));
    assert!(js.contains("var sequence = 3;"));
}

/// Builds the session id from the element's configured object name and the
/// request's evidence.
struct FromContext;

impl JavaScriptBuilderSteps for FromContext {
    fn get_session_id(&self, context: &JavaScriptBuilderContext<'_>) -> String {
        let x = context.data().evidence().get("query.x").unwrap_or("none");
        format!("{}-{x}", context.element().object_name())
    }
}

#[test]
fn a_step_reads_the_element_and_the_flow_data() {
    let js = script(|b| b.set_object_name("named").unwrap().set_steps(FromContext));
    assert!(js.contains("var sessionId = \"named-1\";"));
}

#[test]
fn a_session_id_or_sequence_a_step_gives_is_made_safe() {
    for (session_id, sequence) in [("a\"b", 0), ("</script>", -5), ("", i32::MIN)] {
        let js = script(|b| {
            b.set_steps(Fixed {
                session_id,
                sequence,
            })
        });
        assert!(
            js.contains("var sessionId = \"\";"),
            "{session_id:?} was not emptied"
        );
        assert!(js.contains("var sequence = 1;"), "{sequence} was kept");
    }
}

/// A template using a variable, a section and an inverted section.
const TEMPLATE: &str = "var {{_objName}} = {{&_sequence}};\
                        {{#_userPrompt}} prompt{{/_userPrompt}}\
                        {{^_userPrompt}} no prompt{{/_userPrompt}}";

#[test]
fn a_template_the_caller_gives_is_rendered() {
    let js = script(|b| b.set_template(TEMPLATE).expect("the template is valid"));
    assert_eq!(js, "var fod = 3; no prompt");

    let js = script(|b| {
        b.set_template(TEMPLATE)
            .expect("the template is valid")
            .set_steps(Fixed {
                session_id: "abc-123",
                sequence: 5,
            })
    });
    assert_eq!(js, "var fod = 5; prompt");
}

#[test]
fn a_template_that_does_not_parse_is_a_configuration_error() {
    for source in ["{{#a}}", "{{a", "{{#a}}{{/b}}", "{{/a}}"] {
        let result = JavaScriptBuilderElement::builder().set_template(source);
        assert!(
            matches!(result, Err(Error::PipelineConfiguration { .. })),
            "{source:?} was accepted"
        );
    }
}
