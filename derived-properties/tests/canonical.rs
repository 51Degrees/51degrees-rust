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

//! The canonical JSON a script prints, which the element writes to its debug
//! log at build. The text has to match `tools/canonical.mjs` in
//! 51Degrees/derived-properties character for character, and the example here
//! is the one the .NET package tests with.

use fiftyone_derived_properties::{BuiltInScript, Script};

/// A script covering the parts of the form that are easy to get wrong, being
/// the order of the Output fields, named checks, All, Not, an aggregate over
/// every check, an aggregate over a named group of checks, and an Else.
const EXAMPLE_YAML: &str = r#"Format: 1
Name: Example
Version: 1.0.0
Output:
  Name: Example
  Description: An example property.
  ValueType: string
  StoredValueType: string
  DefaultValue: Unknown
  IsList: false
  IsMandatory: true
  IsObsolete: false
  Category: General
  IsPopular: false
  ExportValues: true
  Url: "https://example.com/documentation"
  DisplayOrder: 3
  PropertyId: 42
  VendorIds:
    - device
  Values:
    - { Name: High, Description: High confidence. }
    - { Name: Unknown }
Checks:
  NotCrawler: { Property: device.IsCrawler, Eq: false }
  Seen: { Property: device.IsVisible, Eq: true }
  Fresh:
    All:
      - { Property: device.Year, Gt: 0 }
      - { Not: { Property: device.Year, Gt: 3000 } }
Rules:
  - When: { Passed: Checks, Ge: 2 }
    Then: High
  - When: { Passed: [NotCrawler, Fresh], Ge: 1 }
    Then: High
  - Else: Unknown
"#;

/// The same script written as JSON.
const EXAMPLE_JSON: &str = r#"{
  "Format": 1,
  "Name": "Example",
  "Version": "1.0.0",
  "Output": {
    "Name": "Example",
    "Description": "An example property.",
    "ValueType": "string",
    "StoredValueType": "string",
    "DefaultValue": "Unknown",
    "IsList": false,
    "IsMandatory": true,
    "IsObsolete": false,
    "Category": "General",
    "IsPopular": false,
    "ExportValues": true,
    "Url": "https://example.com/documentation",
    "DisplayOrder": 3,
    "PropertyId": 42,
    "VendorIds": ["device"],
    "Values": [
      { "Name": "High", "Description": "High confidence." },
      { "Name": "Unknown" }
    ]
  },
  "Checks": {
    "NotCrawler": { "Property": "device.IsCrawler", "Eq": false },
    "Seen": { "Property": "device.IsVisible", "Eq": true },
    "Fresh": {
      "All": [
        { "Property": "device.Year", "Gt": 0 },
        { "Not": { "Property": "device.Year", "Gt": 3000 } }
      ]
    }
  },
  "Rules": [
    { "When": { "Passed": "Checks", "Ge": 2 }, "Then": "High" },
    { "When": { "Passed": ["NotCrawler", "Fresh"], "Ge": 1 }, "Then": "High" },
    { "Else": "Unknown" }
  ]
}"#;

/// The canonical JSON the example prints, as the reference printer in
/// derived-properties prints it.
const EXAMPLE_CANONICAL: &str = r#"{
  "Format": 1,
  "Name": "Example",
  "Version": "1.0.0",
  "Output": {
    "Name": "Example",
    "Description": "An example property.",
    "ValueType": "string",
    "StoredValueType": "string",
    "DefaultValue": "Unknown",
    "IsList": false,
    "IsMandatory": true,
    "IsObsolete": false,
    "Category": "General",
    "IsPopular": false,
    "ExportValues": true,
    "Url": "https://example.com/documentation",
    "DisplayOrder": 3,
    "PropertyId": 42,
    "VendorIds": [
      "device"
    ],
    "Dependencies": [
      "device.IsCrawler",
      "device.IsVisible",
      "device.Year"
    ],
    "Values": [
      {
        "Name": "High",
        "Description": "High confidence."
      },
      {
        "Name": "Unknown"
      }
    ]
  },
  "Properties": {
    "device.IsCrawler": {
      "Type": "bool"
    },
    "device.IsVisible": {
      "Type": "bool"
    },
    "device.Year": {
      "Type": "int"
    }
  },
  "Checks": {
    "NotCrawler": {
      "Property": "device.IsCrawler",
      "Eq": false
    },
    "Seen": {
      "Property": "device.IsVisible",
      "Eq": true
    },
    "Fresh": {
      "All": [
        {
          "Property": "device.Year",
          "Gt": 0
        },
        {
          "Not": {
            "Property": "device.Year",
            "Gt": 3000
          }
        }
      ]
    }
  },
  "Rules": [
    {
      "When": {
        "Passed": "Checks",
        "Ge": 2
      },
      "Then": "High"
    },
    {
      "When": {
        "Passed": [
          "NotCrawler",
          "Fresh"
        ],
        "Ge": 1
      },
      "Then": "High"
    },
    {
      "Else": "Unknown"
    }
  ]
}"#;

/// An int output, so that a rule can supply the whole number zero.
const COUNT_YAML: &str = "Format: 1
Name: Count
Version: 1.0.0
Output:
  Name: Count
  Description: How suspect the request looks.
  ValueType: int
  IsList: false
Checks:
  NotCrawler: { Property: device.IsCrawler, Eq: false }
Rules:
  - When: { Property: device.IsCrawler, Eq: true }
    Then: 0
  - Else: 1
";

/// A double output, where the whole number 2 written for the value prints as
/// 2, the way JavaScript prints it.
const RATIO_YAML: &str = "Format: 1
Name: Ratio
Version: 1.0.0
Output:
  Name: Ratio
  Description: A ratio.
  ValueType: double
  IsList: false
Rules:
  - When: { Property: device.Score, Gt: 0.5 }
    Then: 2
  - Else: 0.25
";

/// Reads a script and prints it, failing with every fault where it does not
/// read.
fn canonical(text: &str, name: &str) -> String {
    Script::compile_named(text, Some(name), "code")
        .unwrap_or_else(|faults| panic!("{faults}"))
        .to_canonical_json()
}

#[test]
fn yaml_and_the_json_that_mirrors_it_print_the_same_canonical_json() {
    assert_eq!(
        canonical(EXAMPLE_YAML, "Example"),
        canonical(EXAMPLE_JSON, "Example")
    );
}

#[test]
fn the_canonical_form_matches_the_expected_text() {
    assert_eq!(canonical(EXAMPLE_YAML, "Example"), EXAMPLE_CANONICAL);
}

#[test]
fn literal_types_survive() {
    let example = canonical(EXAMPLE_YAML, "Example");
    assert!(example.contains("\"Eq\": false"));
    assert!(!example.contains("\"Eq\": \"false\""));
    assert!(example.contains("\"Gt\": 0"));
    assert!(!example.contains("\"Gt\": \"0\""));

    let count = canonical(COUNT_YAML, "Count");
    assert!(count.contains("\"Then\": 0"), "{count}");
    assert!(!count.contains("\"Then\": \"0\""), "{count}");

    let ratio = canonical(RATIO_YAML, "Ratio");
    assert!(ratio.contains("\"Then\": 2\n"), "{ratio}");
    assert!(!ratio.contains("\"Then\": 2.0"), "{ratio}");
    assert!(ratio.contains("\"Else\": 0.25"), "{ratio}");
    assert!(ratio.contains("\"Gt\": 0.5"), "{ratio}");
}

/// A whole number read as `double`, being one too large for `int` or one in a
/// list beside a fraction, prints with no decimal point, as JavaScript prints
/// it.
#[test]
fn a_whole_number_read_as_double_prints_without_a_decimal_point() {
    let text = "Format: 1\nName: Wide\nVersion: 1.0.0\nOutput:\n  Name: Wide\n  \
                Description: A wide count.\n  ValueType: bool\n  IsList: false\n\
                Checks:\n  Big: { Property: device.Count, Eq: 3000000000 }\n  \
                Near: { Property: device.Score, In: [1.5, 2] }\n\
                Rules:\n  - When: { Check: Big }\n    Then: true\n  - Else: false\n";
    let printed = canonical(text, "Wide");
    assert!(printed.contains("\"Eq\": 3000000000\n"), "{printed}");
    assert!(
        printed.contains("\"In\": [\n        1.5,\n        2\n      ]"),
        "{printed}"
    );
    assert!(!printed.contains("3000000000.0"), "{printed}");
    assert!(!printed.contains("2.0\n"), "{printed}");
}

#[test]
fn a_deprecated_script_prints_its_note_between_the_version_and_the_output() {
    let text = EXAMPLE_YAML.replace(
        "Version: 1.0.0",
        "Version: 1.0.0\nDeprecated: true\nDeprecationNote: Use Example2 instead.",
    );
    let printed = canonical(&text, "Example");
    assert!(
        printed.contains(
            "\"Version\": \"1.0.0\",\n  \"Deprecated\": true,\n  \
             \"DeprecationNote\": \"Use Example2 instead.\",\n  \"Output\": {"
        ),
        "{printed}"
    );
}

#[test]
fn a_script_that_is_not_deprecated_prints_neither_key() {
    let printed = canonical(EXAMPLE_YAML, "Example");
    assert!(!printed.contains("Deprecated"), "{printed}");
}

/// The reference prints a list the script wrote even where it is empty, and
/// prints nothing where the script left the list out.
#[test]
fn vendor_ids_print_where_the_script_wrote_them_even_empty() {
    let empty = EXAMPLE_YAML.replace("  VendorIds:\n    - device\n", "  VendorIds: []\n");
    let printed = canonical(&empty, "Example");
    assert!(
        printed.contains("\"PropertyId\": 42,\n    \"VendorIds\": [],\n    \"Dependencies\""),
        "{printed}"
    );

    let absent = EXAMPLE_YAML.replace("  VendorIds:\n    - device\n", "");
    let printed = canonical(&absent, "Example");
    assert!(!printed.contains("VendorIds"), "{printed}");
}

/// A script with no source property still prints its empty Dependencies and
/// Properties, and no Checks.
#[test]
fn a_script_that_reads_nothing_prints_empty_dependencies_and_properties() {
    let text = "Format: 1\nName: Fixed\nVersion: 1.0.0\nOutput:\n  Name: Fixed\n  \
                Description: Always the same.\n  ValueType: bool\n  IsList: false\n\
                Rules:\n  - Else: true\n";
    assert_eq!(
        canonical(text, "Fixed"),
        "{\n  \"Format\": 1,\n  \"Name\": \"Fixed\",\n  \"Version\": \"1.0.0\",\n  \
         \"Output\": {\n    \"Name\": \"Fixed\",\n    \"Description\": \"Always the same.\",\n    \
         \"ValueType\": \"bool\",\n    \"IsList\": false,\n    \"Dependencies\": []\n  },\n  \
         \"Properties\": {},\n  \"Rules\": [\n    {\n      \"Else\": true\n    }\n  ]\n}"
    );
}

/// Lists, Any, a Failed count and text that has to be escaped.
#[test]
fn lists_any_failed_and_escaped_text_print_as_written() {
    let text = "Format: 1\nName: Kind\nVersion: 1.0.0\nOutput:\n  Name: Kind\n  \
                Description: \"A \\\"quoted\\\" word, a back\\\\slash, a tab\\t, a \\u0001 and caf\u{e9}.\"\n  \
                ValueType: string\n  IsList: false\n\
                Checks:\n  Phone: { Property: device.DeviceType, In: [Mobile, SmartPhone] }\n  \
                Old: { Property: device.Year, NotIn: [2000, 2001] }\n\
                Rules:\n  - When: { Any: [ { Check: Phone }, { Failed: [Old], Eq: 0 } ] }\n    \
                Then: a\n  - Else: b\n";
    let printed = canonical(text, "Kind");
    assert!(
        printed.contains(
            "\"Description\": \"A \\\"quoted\\\" word, a back\\\\slash, a tab\\t, a \\u0001 and caf\u{e9}.\""
        ),
        "{printed}"
    );
    assert!(
        printed.contains("\"In\": [\n        \"Mobile\",\n        \"SmartPhone\"\n      ]"),
        "{printed}"
    );
    assert!(
        printed.contains("\"NotIn\": [\n        2000,\n        2001\n      ]"),
        "{printed}"
    );
    assert!(
        printed.contains(
            "\"Any\": [\n          {\n            \"Check\": \"Phone\"\n          },\n          \
             {\n            \"Failed\": [\n              \"Old\"\n            ],\n            \
             \"Eq\": 0\n          }\n        ]"
        ),
        "{printed}"
    );
}

/// The shipped script prints, reads back as JSON, and names every source
/// property it reads with the type the script reads it as.
#[test]
fn the_shipped_script_prints_canonical_json() {
    let script = BuiltInScript::HumanConfidence
        .compile()
        .expect("ships sound");
    let printed = script.to_canonical_json();
    assert!(!printed.ends_with('\n'));
    let value: serde_norway::Value = serde_norway::from_str(&printed).expect("is JSON");
    assert_eq!(value["Name"].as_str(), Some("HumanConfidence"));
    assert_eq!(value["Version"].as_str(), Some(script.version()));
    let properties = value["Properties"].as_mapping().expect("a mapping");
    assert_eq!(properties.len(), script.source_properties().len());
    for property in script.source_properties() {
        assert_eq!(
            properties[property.name.as_str()]["Type"].as_str(),
            Some(property.value_type.as_str()),
            "{}",
            property.name
        );
    }
}
