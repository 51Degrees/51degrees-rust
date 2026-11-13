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

//! The parts of format 1 the shipped script does not happen to use.
//!
//! The shared cases prove the script 51Degrees ships. These prove the rest of
//! the format, being every operator, every shape of condition, the conversion
//! table, the wording of the no value message and the metadata that is carried
//! through.

use fiftyone_derived_properties::{
    Faults, Lookup, MapSource, Outcome, Script, SourceValue, WeightedSourceValue, USUAL_CAUSES,
};

/// Wrap the rules given in the smallest script that will hold them.
fn script_of(body: &str) -> String {
    format!(
        "Format: 1\n\
         Name: Demo\n\
         Version: \"1.0.0\"\n\
         Output:\n\
         \x20 Name: Demo\n\
         \x20 Description: A demonstration.\n\
         \x20 ValueType: string\n\
         \x20 IsList: false\n\
         {body}"
    )
}

fn compile(body: &str) -> Script {
    Script::compile_named(&script_of(body), Some("Demo"), "a test")
        .unwrap_or_else(|faults| panic!("the script did not validate:\n{faults}"))
}

fn refuse(body: &str) -> Faults {
    match Script::compile_named(&script_of(body), Some("Demo"), "a test") {
        Ok(_) => panic!("the script was expected to be refused"),
        Err(faults) => faults,
    }
}

fn text(value: &str) -> Lookup {
    Lookup::Value(SourceValue::Text(value.to_owned()))
}

fn value_of(script: &Script, source: &MapSource) -> String {
    match script.evaluate(source) {
        Outcome::Value(value) => value.to_text(),
        Outcome::NoValue { message, .. } => panic!("no value: {message}"),
    }
}

// ---------------------------------------------------------------------
// The operators.
// ---------------------------------------------------------------------

#[test]
fn every_operator_on_a_number() {
    let script = compile(
        "Checks:\n\
         \x20 GreaterThan:  { Property: a.N, Gt: 5 }\n\
         \x20 AtLeast:      { Property: a.N, Ge: 5 }\n\
         \x20 LessThan:     { Property: a.N, Lt: 5 }\n\
         \x20 AtMost:       { Property: a.N, Le: 5 }\n\
         \x20 Equals:       { Property: a.N, Eq: 5 }\n\
         \x20 NotEquals:    { Property: a.N, Ne: 5 }\n\
         \x20 OneOf:        { Property: a.N, In: [4, 5, 6] }\n\
         \x20 NoneOf:       { Property: a.N, NotIn: [4, 5, 6] }\n\
         Rules:\n\
         \x20 - When: { Passed: Checks, Eq: 4 }\n\
         \x20   Then: four\n\
         \x20 - Else: other\n",
    );
    // 5 passes AtLeast, AtMost, Equals and OneOf, and nothing else.
    let source = MapSource::new().with("a.N", Lookup::Value(SourceValue::Int(5)));
    assert_eq!(value_of(&script, &source), "four");

    let source = MapSource::new().with("a.N", Lookup::Value(SourceValue::Int(9)));
    // 9 passes GreaterThan, AtLeast, NotEquals and NoneOf, which is four again,
    // so the counting is checked with a different set of the same size.
    assert_eq!(value_of(&script, &source), "four");

    let source = MapSource::new().with("a.N", Lookup::Value(SourceValue::Int(4)));
    // 4 passes LessThan, AtMost, NotEquals and OneOf.
    assert_eq!(value_of(&script, &source), "four");
}

#[test]
fn every_operator_on_text() {
    let script = compile(
        "Rules:\n\
         \x20 - When: { Property: a.T, StartsWith: \"Mozilla\" }\n\
         \x20   Then: starts\n\
         \x20 - When: { Property: a.T, EndsWith: \"Safari\" }\n\
         \x20   Then: ends\n\
         \x20 - When: { Property: a.T, Contains: \"Chrome\" }\n\
         \x20   Then: holds\n\
         \x20 - When: { Property: a.T, In: [\"one\", \"two\"] }\n\
         \x20   Then: listed\n\
         \x20 - When: { Property: a.T, Ne: \"other\" }\n\
         \x20   Then: differs\n\
         \x20 - Else: other\n",
    );
    assert_eq!(
        value_of(&script, &MapSource::new().with("a.T", text("Mozilla/5.0"))),
        "starts"
    );
    assert_eq!(
        value_of(&script, &MapSource::new().with("a.T", text("like Safari"))),
        "ends"
    );
    assert_eq!(
        value_of(
            &script,
            &MapSource::new().with("a.T", text("a Chrome build"))
        ),
        "holds"
    );
    assert_eq!(
        value_of(&script, &MapSource::new().with("a.T", text("two"))),
        "listed"
    );
    assert_eq!(
        value_of(&script, &MapSource::new().with("a.T", text("else"))),
        "differs"
    );
    assert_eq!(
        value_of(&script, &MapSource::new().with("a.T", text("other"))),
        "other"
    );
    // Text compares with regard to letter case, so a different case is a
    // different value.
    assert_eq!(
        value_of(&script, &MapSource::new().with("a.T", text("mozilla/5.0"))),
        "differs"
    );
}

#[test]
fn not_all_and_any_nest() {
    let script = compile(
        "Checks:\n\
         \x20 Both:\n\
         \x20   All:\n\
         \x20     - { Property: a.One, Eq: true }\n\
         \x20     - Any:\n\
         \x20         - { Property: a.Two, Eq: true }\n\
         \x20         - Not: { Property: a.Three, Eq: true }\n\
         Rules:\n\
         \x20 - When: { Not: { Check: Both } }\n\
         \x20   Then: no\n\
         \x20 - Else: yes\n",
    );
    let request = |one: bool, two: bool, three: bool| {
        MapSource::new()
            .with("a.One", Lookup::Value(SourceValue::Bool(one)))
            .with("a.Two", Lookup::Value(SourceValue::Bool(two)))
            .with("a.Three", Lookup::Value(SourceValue::Bool(three)))
    };
    assert_eq!(value_of(&script, &request(true, true, true)), "yes");
    assert_eq!(value_of(&script, &request(true, false, false)), "yes");
    assert_eq!(value_of(&script, &request(true, false, true)), "no");
    assert_eq!(value_of(&script, &request(false, true, false)), "no");
}

#[test]
fn an_aggregate_counts_a_named_group_and_every_check() {
    let script = compile(
        "Checks:\n\
         \x20 First:  { Property: a.One, Eq: true }\n\
         \x20 Second: { Property: a.Two, Eq: true }\n\
         \x20 Third:  { Property: a.Three, Eq: true }\n\
         Rules:\n\
         \x20 - When: { Failed: Checks, Eq: 0 }\n\
         \x20   Then: none\n\
         \x20 - When: { Passed: [First, Second], Ge: 1 }\n\
         \x20   Then: some\n\
         \x20 - When: { Failed: [First, Second], Gt: 1 }\n\
         \x20   Then: most\n\
         \x20 - Else: other\n",
    );
    let request = |one: bool, two: bool, three: bool| {
        MapSource::new()
            .with("a.One", Lookup::Value(SourceValue::Bool(one)))
            .with("a.Two", Lookup::Value(SourceValue::Bool(two)))
            .with("a.Three", Lookup::Value(SourceValue::Bool(three)))
    };
    assert_eq!(value_of(&script, &request(true, true, true)), "none");
    assert_eq!(value_of(&script, &request(true, false, false)), "some");
    assert_eq!(value_of(&script, &request(false, false, true)), "most");
}

#[test]
fn passed_and_failed_add_up_to_the_size_of_the_group() {
    let script = compile(
        "Checks:\n\
         \x20 First:  { Property: a.One, Eq: true }\n\
         \x20 Second: { Property: a.Two, Eq: true }\n\
         Rules:\n\
         \x20 - When: { Passed: Checks, Ne: 1 }\n\
         \x20   Then: both or neither\n\
         \x20 - When: { Failed: Checks, Lt: 2 }\n\
         \x20   Then: one\n\
         \x20 - Else: other\n",
    );
    let request = |one: bool, two: bool| {
        MapSource::new()
            .with("a.One", Lookup::Value(SourceValue::Bool(one)))
            .with("a.Two", Lookup::Value(SourceValue::Bool(two)))
    };
    assert_eq!(value_of(&script, &request(true, true)), "both or neither");
    assert_eq!(value_of(&script, &request(false, false)), "both or neither");
    assert_eq!(value_of(&script, &request(true, false)), "one");
}

// ---------------------------------------------------------------------
// Types and conversion.
// ---------------------------------------------------------------------

#[test]
fn a_whole_number_written_either_way_infers_the_same_type() {
    // The companion positive case to the rejection case that 2 and 2.5
    // conflict. A number infers its type from its value rather than from the
    // way the value was written, so 2 and 2.0 are one type.
    let script = compile(
        "Checks:\n\
         \x20 Written:  { Property: a.N, Lt: 2.0 }\n\
         Rules:\n\
         \x20 - When: { Property: a.N, Lt: 2 }\n\
         \x20   Then: small\n\
         \x20 - Else: large\n",
    );
    assert_eq!(script.source_properties()[0].value_type.as_str(), "int");
    // A value with a fractional part then cannot be read at all, which is the
    // trap the format warns about.
    let source = MapSource::new().with("a.N", Lookup::Value(SourceValue::Double(1.5)));
    match script.evaluate(&source) {
        Outcome::NoValue { missing, .. } => assert_eq!(missing, vec!["a.N".to_owned()]),
        Outcome::Value(value) => panic!("expected no value, got {}", value.to_text()),
    }
}

#[test]
fn int_is_a_signed_32_bit_whole_number() {
    let script = compile(
        "Rules:\n\
         \x20 - When: { Property: a.N, Gt: 0 }\n\
         \x20   Then: positive\n\
         \x20 - Else: other\n",
    );
    let at_the_limit = MapSource::new().with("a.N", Lookup::Value(SourceValue::Int(2147483647)));
    assert_eq!(value_of(&script, &at_the_limit), "positive");
    let past_the_limit = MapSource::new().with("a.N", Lookup::Value(SourceValue::Int(2147483648)));
    assert!(matches!(
        script.evaluate(&past_the_limit),
        Outcome::NoValue { .. }
    ));
    // The same holds for the text form of a value.
    let past_as_text = MapSource::new().with("a.N", text("3000000000"));
    assert!(matches!(
        script.evaluate(&past_as_text),
        Outcome::NoValue { .. }
    ));
    // A whole number written in a script outside the range infers double
    // instead, so it can still be compared.
    let wide = compile(
        "Rules:\n\
         \x20 - When: { Property: a.N, Gt: 3000000000 }\n\
         \x20   Then: huge\n\
         \x20 - Else: other\n",
    );
    assert_eq!(wide.source_properties()[0].value_type.as_str(), "double");
    let source = MapSource::new().with("a.N", text("3000000001"));
    assert_eq!(value_of(&wide, &source), "huge");
}

#[test]
fn values_are_never_coerced_loosely() {
    let script = compile(
        "Rules:\n\
         \x20 - When: { Property: a.Flag, Eq: true }\n\
         \x20   Then: on\n\
         \x20 - Else: off\n",
    );
    // A boolean read from text, in any letter case and with whitespace around
    // it.
    assert_eq!(
        value_of(&script, &MapSource::new().with("a.Flag", text(" TRUE "))),
        "on"
    );
    assert_eq!(
        value_of(&script, &MapSource::new().with("a.Flag", text("False"))),
        "off"
    );
    for never in ["N/A", "Unknown", "", "1", "yes"] {
        let source = MapSource::new().with("a.Flag", text(never));
        assert!(
            matches!(script.evaluate(&source), Outcome::NoValue { .. }),
            "'{never}' must not become a boolean"
        );
    }
}

#[test]
fn a_number_and_a_boolean_can_be_read_as_text() {
    let script = compile(
        "Rules:\n\
         \x20 - When: { Property: a.T, Eq: \"True\" }\n\
         \x20   Then: flag\n\
         \x20 - When: { Property: a.T, Eq: \"8\" }\n\
         \x20   Then: number\n\
         \x20 - Else: other\n",
    );
    // A native boolean read as text becomes True or False with a capital first
    // letter, and a native number becomes its plain printed form.
    let source = MapSource::new().with("a.T", Lookup::Value(SourceValue::Bool(true)));
    assert_eq!(value_of(&script, &source), "flag");
    let source = MapSource::new().with("a.T", Lookup::Value(SourceValue::Int(8)));
    assert_eq!(value_of(&script, &source), "number");
    let source = MapSource::new().with("a.T", Lookup::Value(SourceValue::Double(8.0)));
    assert_eq!(value_of(&script, &source), "number");
}

#[test]
fn a_weighted_list_takes_the_highest_weight_and_an_empty_list_cannot_be_read() {
    let script = compile(
        "Rules:\n\
         \x20 - When: { Property: a.N, Ge: 5 }\n\
         \x20   Then: high\n\
         \x20 - Else: low\n",
    );
    let weighted = |candidates: Vec<(f64, i64)>| {
        Lookup::Weighted(
            candidates
                .into_iter()
                .map(|(weight, value)| WeightedSourceValue {
                    weight,
                    value: SourceValue::Int(value),
                })
                .collect(),
        )
    };
    let source = MapSource::new().with("a.N", weighted(vec![(0.3, 2), (0.7, 9)]));
    assert_eq!(value_of(&script, &source), "high");
    // The first of two equal weights wins.
    let source = MapSource::new().with("a.N", weighted(vec![(0.5, 2), (0.5, 9)]));
    assert_eq!(value_of(&script, &source), "low");
    let source = MapSource::new().with("a.N", weighted(vec![]));
    assert!(matches!(script.evaluate(&source), Outcome::NoValue { .. }));
    let source = MapSource::new().with("a.N", Lookup::List);
    match script.evaluate(&source) {
        Outcome::NoValue { message, .. } => {
            assert!(
                message.contains("held a list where a single value is needed"),
                "{message}"
            )
        }
        Outcome::Value(value) => panic!("expected no value, got {}", value.to_text()),
    }
}

// ---------------------------------------------------------------------
// The no value message.
// ---------------------------------------------------------------------

#[test]
fn the_no_value_message_names_every_missing_property() {
    let script = compile(
        "Checks:\n\
         \x20 First: { Property: device.First, Eq: true }\n\
         Rules:\n\
         \x20 - When: { Property: ip.Second, Gt: 0 }\n\
         \x20   Then: yes\n\
         \x20 - Else: no\n",
    );
    match script.evaluate(&MapSource::new()) {
        Outcome::NoValue { missing, message } => {
            // The properties appear in the order the script first named them,
            // with the checks read before the rules.
            assert_eq!(
                missing,
                vec!["device.First".to_owned(), "ip.Second".to_owned()]
            );
            assert_eq!(
                message,
                format!(
                    "Derived property 'Demo' has no value because 2 source properties were not \
                     available. 'device.First' (element 'device' has no value for 'First': \
                     property not present on this request). 'ip.Second' (element 'ip' has no \
                     value for 'Second': property not present on this request). {USUAL_CAUSES}"
                )
            );
        }
        Outcome::Value(value) => panic!("expected no value, got {}", value.to_text()),
    }
}

#[test]
fn the_no_value_message_takes_each_of_the_four_shapes() {
    let script = compile(
        "Rules:\n\
         \x20 - When: { Property: device.Age, Lt: 2 }\n\
         \x20   Then: yes\n\
         \x20 - Else: no\n",
    );
    let message_for =
        |lookup: Lookup| match script.evaluate(&MapSource::new().with("device.Age", lookup)) {
            Outcome::NoValue { message, .. } => message,
            Outcome::Value(value) => panic!("expected no value, got {}", value.to_text()),
        };

    // One source property, so the count reads in the singular.
    assert!(message_for(Lookup::Absent).contains(
        "has no value because 1 source property was not available. 'device.Age' (element \
         'device' has no value for 'Age': property not present on this request)."
    ));
    // The source's own no value message is carried up.
    assert!(
        message_for(Lookup::NoValue("the JavaScript has not run yet".to_owned()))
            .contains("(element 'device' has no value for 'Age': the JavaScript has not run yet).")
    );
    // A value that is there but does not convert does not repeat the name.
    assert!(message_for(text("Unknown"))
        .contains("'device.Age' (held 'Unknown' which cannot be read as int)."));
    assert!(message_for(Lookup::List)
        .contains("'device.Age' (held a list where a single value is needed)."));
}

// ---------------------------------------------------------------------
// The document.
// ---------------------------------------------------------------------

#[test]
fn a_json_script_and_the_yaml_that_mirrors_it_are_the_same_script() {
    let yaml = compile(
        "Checks:\n\
         \x20 Current: { Property: a.N, Lt: 2 }\n\
         Rules:\n\
         \x20 - When: { Check: Current }\n\
         \x20   Then: yes\n\
         \x20 - Else: no\n",
    );
    let json = r#"{
      "Format": 1,
      "Name": "Demo",
      "Version": "1.0.0",
      "Output": {
        "Name": "Demo",
        "Description": "A demonstration.",
        "ValueType": "string",
        "IsList": false
      },
      "Checks": { "Current": { "Property": "a.N", "Lt": 2 } },
      "Rules": [
        { "When": { "Check": "Current" }, "Then": "yes" },
        { "Else": "no" }
      ]
    }"#;
    let json = Script::compile_named(json, Some("Demo"), "a test").expect("the JSON script");
    assert_eq!(yaml.output(), json.output());
    assert_eq!(yaml.checks(), json.checks());
    assert_eq!(yaml.rules(), json.rules());
    assert_eq!(yaml.source_properties(), json.source_properties());
}

#[test]
fn keys_are_matched_without_regard_to_case_and_written_once() {
    let script = Script::compile(
        "format: 1\n\
         NAME: Demo\n\
         version: \"1.0.0\"\n\
         output:\n\
         \x20 name: Demo\n\
         \x20 description: A demonstration.\n\
         \x20 valuetype: STRING\n\
         \x20 islist: false\n\
         rules:\n\
         \x20 - else: only\n",
    )
    .expect("keys are matched without regard to case");
    assert_eq!(script.output().value_type.as_str(), "string");

    // Two keys in one mapping that differ only in case are one key written
    // twice, and a reader would have to drop one of the two, so both are
    // refused.
    let faults = Script::compile(
        "Format: 1\n\
         Name: Demo\n\
         name: Demo\n\
         Version: \"1.0.0\"\n\
         Rules:\n\
         \x20 - Else: only\n",
    )
    .expect_err("two keys differing only in case are refused");
    assert!(
        faults.to_string().contains("differ only in case"),
        "{faults}"
    );

    // An exact duplicate is refused too, by the reader before any rule of the
    // format is read, so the fault is about the document as a whole and has no
    // path inside it.
    let faults = Script::compile(
        "Format: 1\n\
         Name: Demo\n\
         Name: Other\n\
         Version: \"1.0.0\"\n\
         Rules:\n\
         \x20 - Else: only\n",
    )
    .expect_err("a key written twice is refused");
    assert_eq!(faults.len(), 1, "{faults}");
    assert_eq!(faults.faults()[0].path, "");
}

#[test]
fn a_check_name_is_matched_exactly_as_written() {
    let faults = refuse(
        "Checks:\n\
         \x20 NotCrawler: { Property: a.Flag, Eq: false }\n\
         Rules:\n\
         \x20 - When: { Check: notcrawler }\n\
         \x20   Then: yes\n\
         \x20 - Else: no\n",
    );
    assert!(
        faults
            .faults()
            .iter()
            .any(|fault| fault.message.contains("check 'notcrawler' is not defined")),
        "{faults}"
    );
}

#[test]
fn every_fault_in_a_script_is_reported_together() {
    let faults = refuse(
        "Checks:\n\
         \x20 First: { Property: notqualified, Eq: true }\n\
         Rules:\n\
         \x20 - When: { Property: a.N, Wrong: 1 }\n\
         \x20   Then: yes\n\
         \x20 - Else: no\n",
    );
    assert_eq!(faults.len(), 2, "{faults}");
    assert_eq!(faults.faults()[0].path, "Checks.First.Property");
    assert_eq!(faults.faults()[1].path, "Rules[0].When");
}

#[test]
fn an_element_key_may_carry_single_hyphens() {
    let script = compile(
        "Checks:\n\
         \x20 Signed: { Property: 'agent-signature.AgentSignature', Eq: Verified }\n\
         Rules:\n\
         \x20 - When: { Check: Signed }\n\
         \x20   Then: signed\n\
         \x20 - Else: unsigned\n",
    );
    let source = MapSource::new().with("agent-signature.AgentSignature", text("Verified"));
    assert_eq!(value_of(&script, &source), "signed");
    let source = MapSource::new().with("agent-signature.AgentSignature", text("Absent"));
    assert_eq!(value_of(&script, &source), "unsigned");
}

#[test]
fn a_malformed_element_key_is_not_a_source_property() {
    for property in [
        "-agent.Signature",
        "agent-.Signature",
        "agent--signature.Signature",
        "agent.Sig-nature",
        "agent-signature",
    ] {
        let faults = refuse(&format!(
            "Checks:\n\
             \x20 First: {{ Property: '{property}', Eq: true }}\n\
             Rules:\n\
             \x20 - When: {{ Check: First }}\n\
             \x20   Then: yes\n\
             \x20 - Else: no\n"
        ));
        assert!(
            faults
                .faults()
                .iter()
                .any(|fault| fault.path == "Checks.First.Property"
                    && fault.message.contains("is not a source property")),
            "'{property}' was not refused as a source property:\n{faults}"
        );
    }
}

// ---------------------------------------------------------------------
// Output, the property definition.
// ---------------------------------------------------------------------

#[test]
fn the_metadata_is_carried_through_unchanged() {
    let script = Script::compile(
        "Format: 1\n\
         Name: Demo\n\
         Version: \"2.1.0-beta.3+build5\"\n\
         Output:\n\
         \x20 Name: Demo\n\
         \x20 Description: A demonstration.\n\
         \x20 ValueType: int\n\
         \x20 StoredValueType: int\n\
         \x20 DefaultValue: \"1\"\n\
         \x20 IsList: false\n\
         \x20 IsMandatory: true\n\
         \x20 IsObsolete: false\n\
         \x20 Category: Demonstrations\n\
         \x20 IsPopular: true\n\
         \x20 ExportValues: true\n\
         \x20 Url: https://51degrees.com\n\
         \x20 DisplayOrder: 3\n\
         \x20 PropertyId: 12345\n\
         \x20 VendorIds: [\"51D\"]\n\
         \x20 Values:\n\
         \x20   - { Name: 1, Description: One. }\n\
         \x20   - { Name: 2, Description: Two. }\n\
         Rules:\n\
         \x20 - When: { Property: a.N, Gt: 0 }\n\
         \x20   Then: 2\n\
         \x20 - Else: 1\n",
    )
    .expect("the script");
    let output = script.output();
    assert_eq!(output.value_type.as_str(), "int");
    assert_eq!(output.stored_value_type.as_deref(), Some("int"));
    assert_eq!(output.default_value.as_deref(), Some("1"));
    assert_eq!(output.is_mandatory, Some(true));
    assert_eq!(output.is_obsolete, Some(false));
    assert_eq!(output.category.as_deref(), Some("Demonstrations"));
    assert_eq!(output.is_popular, Some(true));
    assert_eq!(output.export_values, Some(true));
    assert_eq!(output.display_order, Some(3));
    assert_eq!(output.property_id, Some(12345));
    assert_eq!(output.vendor_ids, vec!["51D".to_owned()]);
    // Dependencies are computed where the script does not give them.
    assert_eq!(output.dependencies, vec!["a.N".to_owned()]);
    // A value name written as a whole number is held as text, because that is
    // how it is compared.
    let values = output.values.as_ref().expect("the value list");
    assert_eq!(values[0].name, "1");
    assert_eq!(values[1].description.as_deref(), Some("Two."));

    let source = MapSource::new().with("a.N", Lookup::Value(SourceValue::Int(1)));
    assert_eq!(value_of(&script, &source), "2");
}

#[test]
fn a_whole_number_is_accepted_where_the_value_type_is_double() {
    let script = Script::compile(
        "Format: 1\n\
         Name: Demo\n\
         Version: \"1.0.0\"\n\
         Output:\n\
         \x20 Name: Demo\n\
         \x20 Description: A demonstration.\n\
         \x20 ValueType: double\n\
         \x20 IsList: false\n\
         Rules:\n\
         \x20 - When: { Property: a.N, Gt: 0 }\n\
         \x20   Then: 1\n\
         \x20 - Else: 0.5\n",
    )
    .expect("a whole number reads as a double");
    let source = MapSource::new().with("a.N", Lookup::Value(SourceValue::Int(1)));
    assert_eq!(value_of(&script, &source), "1");
    let source = MapSource::new().with("a.N", Lookup::Value(SourceValue::Int(-1)));
    assert_eq!(value_of(&script, &source), "0.5");
}

#[test]
fn a_deprecated_script_says_what_to_use_instead() {
    let script = Script::compile(
        "Format: 1\n\
         Name: Demo\n\
         Version: \"1.0.0\"\n\
         Deprecated: true\n\
         DeprecationNote: Use Demonstration instead.\n\
         Output:\n\
         \x20 Name: Demo\n\
         \x20 Description: A demonstration.\n\
         \x20 ValueType: string\n\
         \x20 IsList: false\n\
         Rules:\n\
         \x20 - Else: only\n",
    )
    .expect("a deprecated script still works");
    assert!(script.is_deprecated());
    assert_eq!(
        script.deprecation_note(),
        Some("Use Demonstration instead.")
    );

    // A deprecated script without a note is a fault, and so is a note on a
    // script that is not deprecated.
    let faults = Script::compile(
        "Format: 1\n\
         Name: Demo\n\
         Version: \"1.0.0\"\n\
         Deprecated: true\n\
         Output:\n\
         \x20 Name: Demo\n\
         \x20 Description: A demonstration.\n\
         \x20 ValueType: string\n\
         \x20 IsList: false\n\
         Rules:\n\
         \x20 - Else: only\n",
    )
    .expect_err("a deprecated script must say what to use instead");
    assert!(faults.to_string().contains("DeprecationNote"), "{faults}");
}

// ---------------------------------------------------------------------
// Conditions the format refuses.
// ---------------------------------------------------------------------

#[test]
fn an_empty_or_nonsense_condition_is_refused() {
    for (body, expected) in [
        (
            "Rules:\n\x20 - When: {}\n\x20   Then: yes\n\x20 - Else: no\n",
            "a condition is empty",
        ),
        (
            "Rules:\n\x20 - When: a string\n\x20   Then: yes\n\x20 - Else: no\n",
            "a condition expected a mapping",
        ),
        (
            "Rules:\n\x20 - When: { All: [] }\n\x20   Then: yes\n\x20 - Else: no\n",
            "All must list at least one condition",
        ),
        (
            "Rules:\n\x20 - When: { Property: a.T, In: [] }\n\x20   Then: yes\n\x20 - Else: no\n",
            "In expects a non empty list",
        ),
        (
            "Rules:\n\x20 - When: { Property: a.T, In: [1, \"two\"] }\n\x20   Then: yes\n\
             \x20 - Else: no\n",
            "every member of a list must be of the same type",
        ),
        (
            "Rules:\n\x20 - When: { Property: a.T, In: [1, null] }\n\x20   Then: yes\n\
             \x20 - Else: no\n",
            "a null literal is not allowed in a list",
        ),
        (
            "Checks:\n\x20 One: { Property: a.T, Eq: \"x\" }\n\
             Rules:\n\x20 - When: { Failed: Rules, Eq: 0 }\n\x20   Then: yes\n\x20 - Else: no\n",
            "a group is the word Checks",
        ),
        (
            "Rules:\n\x20 - When: { Passed: Checks }\n\x20   Then: yes\n\x20 - Else: no\n",
            "an aggregate condition has no operator",
        ),
        (
            "Rules:\n\x20 - When: { Passed: Checks, Contains: \"x\" }\n\x20   Then: yes\n\
             \x20 - Else: no\n",
            "is not allowed on a count",
        ),
        (
            "Rules:\n\x20 - When: { Passed: Checks, Failed: Checks, Eq: 0 }\n\x20   Then: yes\n\
             \x20 - Else: no\n",
            "an aggregate condition takes one of Passed, Failed",
        ),
        (
            "Rules:\n\x20 - When: { Not: { Property: a.T, Eq: \"x\" }, Any: [] }\n\
             \x20   Then: yes\n\x20 - Else: no\n",
            "must be the only key of its condition",
        ),
    ] {
        let faults = refuse(body);
        assert!(
            faults
                .faults()
                .iter()
                .any(|fault| fault.message.contains(expected)),
            "expected a fault mentioning '{expected}', got:\n{faults}"
        );
    }
}

#[test]
fn a_mixed_number_list_reads_as_double() {
    let script = compile(
        "Rules:\n\
         \x20 - When: { Property: a.N, In: [1, 2.5] }\n\
         \x20   Then: listed\n\
         \x20 - Else: other\n",
    );
    assert_eq!(script.source_properties()[0].value_type.as_str(), "double");
    let source = MapSource::new().with("a.N", Lookup::Value(SourceValue::Double(2.5)));
    assert_eq!(value_of(&script, &source), "listed");
    let source = MapSource::new().with("a.N", Lookup::Value(SourceValue::Int(1)));
    assert_eq!(value_of(&script, &source), "listed");
    let source = MapSource::new().with("a.N", Lookup::Value(SourceValue::Int(3)));
    assert_eq!(value_of(&script, &source), "other");
}
