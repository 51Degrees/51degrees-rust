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

//! Making a pipeline from a configuration. The cases follow .NET's
//! `PipelineBuilder_BuildFromConfiguration_*` tests in
//! `pipeline/Tests/Core.Tests/FlowElements/PipelineBuilderTests.cs`, with
//! .NET's messages, and add the JSON file reader's cases.

use std::any::{type_name, Any};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use fiftyone_pipeline_core::configuration::{
    parse_enum, BuildContext, BuilderDefinition, BuilderRegistry, ConfigType, ConfigValue,
    ElementOptions, Parameter, ParseFailure, PipelineDefinition, PipelineElements, PipelineOptions,
};
use fiftyone_pipeline_core::{
    ElementData, Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PipelineBuilder, PropertyMetaData, PropertyValue,
    Result, TypedKey,
};

/// Element data holding one string value.
struct DoneData(MapElementData);

impl ElementData for DoneData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        self.0.get(name)
    }
    fn keys(&self) -> Vec<String> {
        self.0.keys()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Counts the test elements dropped, for the tests that check elements
/// made before a failure are released.
static DROPPED: AtomicUsize = AtomicUsize::new(0);

/// A test element that adds data saying it ran, and holds the settings it
/// was made with so a test can read them back.
struct Element<S> {
    key: &'static str,
    settings: S,
    filter: EvidenceKeyFilterWhitelist,
    counts_drops: bool,
}

impl<S> Element<S> {
    fn new(key: &'static str, settings: S) -> Self {
        Element {
            key,
            settings,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            counts_drops: false,
        }
    }
}

impl<S> Drop for Element<S> {
    fn drop(&mut self) {
        if self.counts_drops {
            DROPPED.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl<S: Send + Sync + 'static> FlowElement for Element<S> {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.get_or_add(TypedKey::<DoneData>::new(self.key), || {
            DoneData(MapElementData::new().set("done", "yes"))
        })?;
        Ok(())
    }
    fn data_key(&self) -> &str {
        self.key
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

/// Keep the element as a resource so a test can read its settings, and
/// give it to the pipeline.
fn finish<S: Send + Sync + 'static>(
    element: Element<S>,
    context: &mut BuildContext,
) -> Result<Arc<dyn FlowElement>> {
    let element = Arc::new(element);
    context.add_resource(element.clone());
    Ok(element)
}

/// .NET's `MultiplyByElementBuilder`, whose build method takes the value.
struct MultiplyBy {
    multiple: i32,
}

#[derive(Debug, Clone, PartialEq)]
struct Splitter {
    delimiters: Vec<String>,
    max_length: i32,
}

/// .NET's `ListSplitterElementBuilder`, whose settings are optional.
struct SplitterBuilder(Splitter);

impl SplitterBuilder {
    fn new() -> Self {
        SplitterBuilder(Splitter {
            delimiters: vec![",".to_string()],
            max_length: i32::MAX,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Profile {
    LowMemory,
    Balanced,
    HighPerformance,
    MaxPerformance,
}

impl ConfigType for Profile {
    const TYPE_NAME: &'static str = "PerformanceProfiles";

    fn parse_text(text: &str) -> std::result::Result<Self, ParseFailure> {
        parse_enum(
            text,
            &[
                ("LowMemory", 0, Profile::LowMemory),
                ("Balanced", 3, Profile::Balanced),
                ("HighPerformance", 4, Profile::HighPerformance),
                ("MaxPerformance", 5, Profile::MaxPerformance),
            ],
        )
    }
}

/// A builder with two build methods, as a .NET on-premise engine builder
/// has `Build(string datafile, bool createTempDataCopy)` and others.
#[derive(Debug, Clone, PartialEq)]
struct DataFile {
    path: String,
    copy: bool,
}

/// A service one builder adds and a later builder needs, as the cloud
/// engines need the cloud request engine.
struct Service(&'static str);

fn multiply_definition() -> BuilderDefinition<MultiplyBy> {
    BuilderDefinition::new("MultiplyByElementBuilder", |b: MultiplyBy, context| {
        finish(Element::new("multiplyby", b.multiple), context)
    })
    .alternate_name("Multiply")
    .build_method(vec![Parameter::new::<i32>("multiple")], |args, _| {
        Ok(MultiplyBy {
            multiple: *args.get::<i32>("multiple"),
        })
    })
}

/// A second builder named `MultiplyByElementBuilder`, with state of
/// another type, as two .NET types of one name in different assemblies.
struct OtherMultiplyBy;

fn other_multiply_definition() -> BuilderDefinition<OtherMultiplyBy> {
    BuilderDefinition::new("MultiplyByElementBuilder", |_: OtherMultiplyBy, context| {
        finish(Element::new("multiplyby", 1), context)
    })
    .build_method(vec![Parameter::new::<i32>("multiple")], |_, _| {
        Ok(OtherMultiplyBy)
    })
}

fn splitter_definition() -> BuilderDefinition<SplitterBuilder> {
    BuilderDefinition::new(
        "ListSplitterElementBuilder",
        |b: SplitterBuilder, context| finish(Element::new("listsplitter", b.0), context),
    )
    .setting("SetDelimiter", |mut b: SplitterBuilder, d: String| {
        b.0.delimiters = vec![d];
        b
    })
    .setting_alternate_name("SetDelimiter", "delim")
    .setting("SetDelimiters", |mut b: SplitterBuilder, d: Vec<String>| {
        b.0.delimiters = d;
        b
    })
    .setting("SetMaxLength", |mut b: SplitterBuilder, length: i32| {
        b.0.max_length = length;
        b
    })
    .build_method(vec![], |_, _| Ok(SplitterBuilder::new()))
}

fn profile_definition() -> BuilderDefinition<Profile> {
    BuilderDefinition::new("EnumPerfProfileBuilder", |p: Profile, context| {
        finish(Element::new("enumperfprofile", p), context)
    })
    .setting("SetPerformanceProfile", |_, p: Profile| p)
    .build_method(vec![], |_, _| Ok(Profile::Balanced))
}

fn data_file_definition() -> BuilderDefinition<DataFile> {
    BuilderDefinition::new("DataFileEngineBuilder", |d: DataFile, context| {
        finish(Element::new("datafile", d), context)
    })
    .build_method(vec![], |_, _| {
        Ok(DataFile {
            path: "default".into(),
            copy: false,
        })
    })
    .build_method(
        vec![
            Parameter::new::<String>("datafile"),
            Parameter::new::<bool>("createTempDataCopy"),
        ],
        |args, _| {
            Ok(DataFile {
                path: args.get::<String>("DATAFILE").clone(),
                copy: *args.get::<bool>("createtempdatacopy"),
            })
        },
    )
}

fn service_definitions() -> [BuilderDefinition<()>; 2] {
    [
        BuilderDefinition::new("ServiceElementBuilder", |_, context: &mut BuildContext| {
            context.add_resource(Arc::new(Service("made by the configuration")));
            finish(Element::new("service", ()), context)
        })
        .build_method(vec![], |_, _| Ok(())),
        BuilderDefinition::new(
            "UsesServiceElementBuilder",
            |_, context: &mut BuildContext| {
                let service = context.latest_resource::<Service>().ok_or_else(|| {
                    Error::configuration("No service was made before this element.")
                })?;
                finish(Element::new("usesservice", service.0), context)
            },
        )
        .build_method(vec![], |_, _| Ok(())),
    ]
}

/// A builder whose element counts its drops, and one whose element always
/// fails to build.
fn dropping_definitions() -> [BuilderDefinition<()>; 2] {
    [
        BuilderDefinition::new("CountedElementBuilder", |_, context: &mut BuildContext| {
            let mut element = Element::new("counted", ());
            element.counts_drops = true;
            let element: Arc<dyn FlowElement> = Arc::new(element);
            let _ = context;
            Ok(element)
        })
        .build_method(vec![], |_, _| Ok(())),
        BuilderDefinition::new("FailingElementBuilder", |_, _: &mut BuildContext| {
            Err(Error::configuration("The failing element always fails."))
        })
        .build_method(vec![], |_, _| Ok(())),
    ]
}

fn registry() -> BuilderRegistry {
    let [service, uses_service] = service_definitions();
    let [counted, failing] = dropping_definitions();
    BuilderRegistry::new()
        .with(multiply_definition())
        .with(splitter_definition())
        .with(profile_definition())
        .with(data_file_definition())
        .with(service)
        .with(uses_service)
        .with(counted)
        .with(failing)
}

fn build(options: &PipelineOptions) -> Result<Arc<Pipeline>> {
    PipelineBuilder::new().build_from_configuration(&registry(), options)
}

fn one(element: ElementOptions) -> PipelineOptions {
    PipelineOptions::new().element(element)
}

fn message(result: Result<Arc<Pipeline>>) -> String {
    match result {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

/// The message .NET gives for an element that cannot be made.
fn failed(builder: &str, subs: &str, message: &str, inner: &str) -> String {
    format!(
        "Configuration failed to build.\nBuilderName: {builder}\nSubElement Builder Names: \
         {subs}\n{message}\n{inner}\n"
    )
}

const NO_INNER: &str = "No Inner Exception.";

fn splitter(pipeline: &Pipeline) -> Splitter {
    pipeline
        .resource::<Element<Splitter>>()
        .expect("a list splitter")
        .settings
        .clone()
}

fn keys(pipeline: &Pipeline) -> Vec<&str> {
    pipeline
        .flow_elements()
        .iter()
        .map(|e| e.data_key())
        .collect()
}

#[test]
fn a_mandatory_build_parameter_is_given_to_the_build_method() {
    for name in [
        "MultiplyByElementBuilder",
        "MultiplyByElement",
        "Multiply",
        "multiplybyelementbuilder",
        "MULTIPLYBYELEMENT",
    ] {
        let pipeline = build(&one(ElementOptions::new(name).parameter("multiple", "8"))).unwrap();
        let element = pipeline.resource::<Element<i32>>().unwrap();
        assert_eq!(element.settings, 8, "{name}");
        assert_eq!(keys(&pipeline), vec!["multiplyby"]);
    }
}

#[test]
fn a_missing_mandatory_parameter_fails() {
    let found = message(build(&one(ElementOptions::new("MultiplyByElementBuilder"))));
    let builder = type_name::<MultiplyBy>();
    assert_eq!(
        found,
        failed(
            "MultiplyByElementBuilder",
            "No SubElements",
            &format!(
                "Builder '{builder}' for element 0 has no 'Set' methods or 'Build' methods \
                 that match (case-insensitively) the following parameters: . The available \
                 configuration methods on this builder are: \nThe available 'Build' methods \
                 have the following signatures: \nmultiple (Int32)\n"
            ),
            NO_INNER
        )
    );
}

#[test]
fn a_mandatory_parameter_of_the_wrong_type_fails() {
    let found = message(build(&one(
        ElementOptions::new("MultiplyByElementBuilder").parameter("multiple", "WrongType")
    )));
    let builder = type_name::<MultiplyBy>();
    assert_eq!(
        found,
        failed(
            "MultiplyByElementBuilder",
            "No SubElements",
            &format!(
                "Method 'Build' on builder '{builder}' for element 0 expects a parameter of \
                 type 'Int32'. Failed to parse value 'WrongType' using the static 'TryParse' \
                 method."
            ),
            NO_INNER
        )
    );
}

#[test]
fn an_unknown_builder_fails_and_lists_the_builders() {
    let expected = |name: &str| {
        failed(
            name,
            "No SubElements",
            &format!(
                "Could not find builder matching '{name}' for element 0. Available builders \
                 are: MultiplyByElementBuilder,ListSplitterElementBuilder,\
                 EnumPerfProfileBuilder,DataFileEngineBuilder,ServiceElementBuilder,\
                 UsesServiceElementBuilder,CountedElementBuilder,FailingElementBuilder"
            ),
            NO_INNER,
        )
    };
    for name in [
        "NoBuilder",
        "PipelineBuilder",
        "Builder",
        "MultiplyByElementBuilderBuilder",
    ] {
        assert_eq!(
            message(build(&one(ElementOptions::new(name)))),
            expected(name)
        );
    }
}

#[test]
fn an_element_without_a_builder_name_fails() {
    let unset = message(build(&one(ElementOptions::default())));
    assert_eq!(
        unset,
        failed(
            "BuilderName not set",
            "No SubElements",
            "A BuilderName must be specified for element 0.",
            NO_INNER
        )
    );
    let empty = message(build(&one(ElementOptions::new(""))));
    assert_eq!(
        empty,
        failed(
            "",
            "No SubElements",
            "A BuilderName must be specified for element 0.",
            NO_INNER
        )
    );
}

#[test]
fn an_optional_setting_is_found_by_its_name_without_set_or_its_alternate() {
    for name in [
        "SetDelimiter",
        "Delimiter",
        "delim",
        "setdelimiter",
        "DELIM",
    ] {
        let pipeline = build(&one(
            ElementOptions::new("ListSplitterElement").parameter(name, "|")
        ))
        .unwrap();
        assert_eq!(splitter(&pipeline).delimiters, vec!["|"], "{name}");
    }
}

#[test]
fn an_optional_setting_left_out_keeps_its_default() {
    let pipeline = build(&one(ElementOptions::new("ListSplitterElement"))).unwrap();
    assert_eq!(
        splitter(&pipeline),
        Splitter {
            delimiters: vec![",".into()],
            max_length: i32::MAX
        }
    );
}

#[test]
fn an_unknown_setting_fails_and_lists_the_settings() {
    let found = message(build(&one(ElementOptions::new("ListSplitterElement")
        .parameter("NoMethod", "|")
        .parameter("delim", ";"))));
    let builder = type_name::<SplitterBuilder>();
    assert_eq!(
        found,
        failed(
            "ListSplitterElement",
            "No SubElements",
            &format!(
                "Builder '{builder}' for element 0 has no 'Set' methods or 'Build' methods \
                 that match (case-insensitively) the following parameters: NOMETHOD. The \
                 available configuration methods on this builder are: \nSetDelimiter \
                 (String)\nSetDelimiters (List`1)\nSetMaxLength (Int32)\nThe available \
                 'Build' methods have the following signatures: \n\n"
            ),
            NO_INNER
        )
    );
}

#[test]
fn an_integer_setting_is_read_from_text() {
    let pipeline = build(&one(
        ElementOptions::new("ListSplitterElement").parameter("MaxLength", " 3 ")
    ))
    .unwrap();
    assert_eq!(splitter(&pipeline).max_length, 3);
}

#[test]
fn a_setting_of_the_wrong_type_fails() {
    let builder = type_name::<SplitterBuilder>();
    for value in ["WrongType", "3.5", "2147483648", ""] {
        let found = message(build(&one(
            ElementOptions::new("ListSplitterElement").parameter("MaxLength", value)
        )));
        assert_eq!(
            found,
            failed(
                "ListSplitterElement",
                "No SubElements",
                &format!(
                    "Method 'SetMaxLength' on builder '{builder}' for element 0 expects a \
                     parameter of type 'Int32'. Failed to parse value '{value}' using the \
                     static 'TryParse' method."
                ),
                NO_INNER
            )
        );
    }
}

#[test]
fn an_enumeration_is_read_by_name_in_any_case_or_by_number() {
    for (value, expected) in [
        ("HighPerformance", Profile::HighPerformance),
        ("balanced", Profile::Balanced),
        ("MaxPerformance", Profile::MaxPerformance),
        ("0", Profile::LowMemory),
    ] {
        let pipeline = build(&one(
            ElementOptions::new("EnumPerfProfile").parameter("SetPerformanceProfile", value)
        ))
        .unwrap();
        let element = pipeline.resource::<Element<Profile>>().unwrap();
        assert_eq!(element.settings, expected, "{value}");
    }
}

#[test]
fn an_enumeration_value_that_is_not_known_fails_with_the_valid_values() {
    let found = message(build(&one(
        ElementOptions::new("EnumPerfProfile").parameter("PerformanceProfile", "Fastest")
    )));
    let builder = type_name::<Profile>();
    assert_eq!(
        found,
        failed(
            "EnumPerfProfile",
            "No SubElements",
            &format!(
                "Method 'SetPerformanceProfile' on builder '{builder}' for element 0 expects \
                 a parameter of type 'PerformanceProfiles'. Failed to parse enum value \
                 'Fastest'. Valid values are: LowMemory, Balanced, HighPerformance, \
                 MaxPerformance"
            ),
            "Requested value 'Fastest' was not found."
        )
    );
}

#[test]
fn a_list_is_read_from_a_list_or_from_comma_separated_text() {
    for (value, expected) in [
        (ConfigValue::from(vec!["|", ","]), vec!["|", ","]),
        (ConfigValue::from("A,|"), vec!["A", "|"]),
        (ConfigValue::from("|,\",\""), vec!["|", ","]),
        (ConfigValue::from("|,\"\"\"\""), vec!["|", "\""]),
        (ConfigValue::from("|"), vec!["|"]),
    ] {
        let pipeline = build(&one(
            ElementOptions::new("ListSplitterElement").parameter("Delimiters", value.clone())
        ))
        .unwrap();
        assert_eq!(splitter(&pipeline).delimiters, expected, "{value:?}");
    }
}

#[test]
fn a_list_is_refused_where_one_value_is_taken() {
    let found = message(build(&one(
        ElementOptions::new("ListSplitterElement").parameter("MaxLength", vec!["1", "2"])
    )));
    let builder = type_name::<SplitterBuilder>();
    assert_eq!(
        found,
        failed(
            "ListSplitterElement",
            "No SubElements",
            &format!(
                "Method 'SetMaxLength' on builder '{builder}' for element 0 expects a \
                 parameter of type 'Int32' but supplied object is of type 'String[]'."
            ),
            NO_INNER
        )
    );
}

/// .NET passes a list to a method taking text as it is, so the call gives
/// the message, as it did for the JavaScript builder's `SetHost` in .NET
/// 4.5.129. A build method's text parameter is passed the same way.
#[test]
fn a_list_given_for_text_cannot_be_converted() {
    let cannot = "Object of type 'System.String[]' cannot be converted to type 'System.String'.";
    assert_eq!(
        message(build(&one(
            ElementOptions::new("ListSplitterElement").parameter("Delimiter", vec!["|"])
        ))),
        failed("ListSplitterElement", "No SubElements", cannot, NO_INNER)
    );
    assert_eq!(
        message(build(&one(ElementOptions::new("DataFileEngine")
            .parameter("DataFile", vec!["51Degrees.hash"])
            .parameter("CreateTempDataCopy", "true")))),
        failed("DataFileEngine", "No SubElements", cannot, NO_INNER)
    );
}

/// A setting such as .NET's `JsonBuilderElementBuilder.SetJsonConverters`,
/// whose value only code can give. The messages are those .NET 4.5.129 gave
/// for that setting.
#[test]
fn a_setting_taken_in_code_only_refuses_every_value() {
    struct Converters;
    let registry = BuilderRegistry::new().with(
        BuilderDefinition::new("ConvertingElementBuilder", |_: Converters, context| {
            finish(Element::new("converting", ()), context)
        })
        .code_only_setting("SetConverters", "IEnumerable`1")
        .build_method(vec![], |_, _| Ok(Converters)),
    );
    let make = |element: ElementOptions| {
        PipelineBuilder::new().build_from_configuration(&registry, &one(element))
    };
    let named = || ElementOptions::new("ConvertingElementBuilder");
    let builder = type_name::<Converters>();
    let refused = |but: &str| {
        failed(
            "ConvertingElementBuilder",
            "No SubElements",
            &format!(
                "Method 'SetConverters' on builder '{builder}' for element 0 expects a \
                 parameter of type 'IEnumerable`1' but {but}"
            ),
            NO_INNER,
        )
    };

    for (name, value) in [
        ("Converters", "x"),
        ("SetConverters", "x"),
        ("converters", ""),
    ] {
        assert_eq!(
            message(make(named().parameter(name, value))),
            refused(
                "this type does not have the expected 'TryParse' method (or has multiples \
                 that cannot be resolved)."
            ),
            "{name}"
        );
    }
    assert_eq!(
        message(make(named().parameter("Converters", vec!["a"]))),
        refused("supplied object is of type 'String[]'.")
    );

    // Left out, the element is made, and the setting is listed with its
    // .NET type.
    assert_eq!(keys(&make(named()).unwrap()), ["converting"]);
    assert_eq!(
        message(make(named().parameter("Unknown", "1"))),
        failed(
            "ConvertingElementBuilder",
            "No SubElements",
            &format!(
                "Builder '{builder}' for element 0 has no 'Set' methods or 'Build' methods \
                 that match (case-insensitively) the following parameters: UNKNOWN. The \
                 available configuration methods on this builder are: \nSetConverters \
                 (IEnumerable`1)\nThe available 'Build' methods have the following \
                 signatures: \n\n"
            ),
            NO_INNER,
        )
    );
}

/// Methods such as .NET's `DerivedPropertyElementBuilder.AddScript(string,
/// string)`, which .NET finds by name and refuses because they do not take
/// one value. The messages are those .NET 4.5.129 gave for `AddScript` and
/// `Build` on that builder, where an `AddScript` taking one value comes
/// first and is tried first.
#[test]
fn a_method_taking_no_value_or_several_is_refused_when_reached() {
    struct Names(Vec<i32>);
    let registry = BuilderRegistry::new().with(
        BuilderDefinition::new("NamingElementBuilder", |b: Names, context| {
            finish(Element::new("naming", b.0), context)
        })
        .setting("AddName", |mut b: Names, name: i32| {
            b.0.push(name);
            b
        })
        .method_with_parameters("AddName", 2)
        .method_with_parameters("Reset", 0)
        .method_with_parameters("SetLimit", 2)
        .setting("SetLimit", |b: Names, _: i32| b)
        .build_method(vec![], |_, _| Ok(Names(Vec::new()))),
    );
    let make = |element: ElementOptions| {
        PipelineBuilder::new().build_from_configuration(&registry, &one(element))
    };
    let named = || ElementOptions::new("NamingElementBuilder");
    let builder = type_name::<Names>();
    let refused = |method: &str, takes: &str| {
        failed(
            "NamingElementBuilder",
            "No SubElements",
            &format!(
                "Method '{method}' on builder '{builder}' for element 0 takes {takes}This is not \
                 supported."
            ),
            NO_INNER,
        )
    };

    // The setting taking one value comes first and reads the value.
    let pipeline = make(named().parameter("AddName", "7")).unwrap();
    let element = pipeline.resource::<Element<Vec<i32>>>().unwrap();
    assert_eq!(element.settings, [7]);

    // A value it cannot read reaches the method taking two.
    for value in [ConfigValue::from("x"), ConfigValue::from(vec!["7"])] {
        assert_eq!(
            message(make(named().parameter("AddName", value.clone()))),
            refused("AddName", "more than one parameter. "),
            "{value:?}"
        );
    }
    assert_eq!(
        message(make(named().parameter("Reset", "x"))),
        refused("Reset", "no parameters ")
    );
    // A setting added after such a method is never reached.
    assert_eq!(
        message(make(named().parameter("Limit", "1"))),
        refused("SetLimit", "more than one parameter. ")
    );

    // Only the settings taking one value are listed.
    assert_eq!(
        message(make(named().parameter("Unknown", "1"))),
        failed(
            "NamingElementBuilder",
            "No SubElements",
            &format!(
                "Builder '{builder}' for element 0 has no 'Set' methods or 'Build' methods \
                 that match (case-insensitively) the following parameters: UNKNOWN. The \
                 available configuration methods on this builder are: \nAddName (Int32)\n\
                 SetLimit (Int32)\nThe available 'Build' methods have the following \
                 signatures: \n\n"
            ),
            NO_INNER,
        )
    );
}

#[test]
#[should_panic(expected = "a method taking one value is a setting")]
fn a_method_taking_one_value_is_not_added_as_one_taking_several() {
    let _ = BuilderDefinition::new("NamingElementBuilder", |_: (), _: &mut BuildContext| {
        Err(Error::configuration("not made"))
    })
    .method_with_parameters("AddName", 1);
}

#[test]
fn elements_are_made_in_the_order_given() {
    let pipeline = build(
        &PipelineOptions::new()
            .element(ElementOptions::new("EnumPerfProfile"))
            .element(ElementOptions::new("ListSplitterElement"))
            .element(ElementOptions::new("MultiplyByElement").parameter("Multiple", "3"))
            .element(ElementOptions::new("DataFileEngine")),
    )
    .unwrap();
    assert_eq!(
        keys(&pipeline),
        vec!["enumperfprofile", "listsplitter", "multiplyby", "datafile"]
    );
    assert!(!pipeline.is_concurrent());
}

#[test]
fn the_build_method_is_chosen_by_the_names_of_its_parameters() {
    let pipeline = build(&one(ElementOptions::new("DataFileEngine")
        .parameter("DataFile", "51Degrees.hash")
        .parameter("createtempdatacopy", "true")))
    .unwrap();
    let element = pipeline.resource::<Element<DataFile>>().unwrap();
    assert_eq!(
        element.settings,
        DataFile {
            path: "51Degrees.hash".into(),
            copy: true
        }
    );

    let found = message(build(&one(
        ElementOptions::new("DataFileEngine").parameter("DataFile", "51Degrees.hash")
    )));
    let builder = type_name::<DataFile>();
    assert_eq!(
        found,
        failed(
            "DataFileEngine",
            "No SubElements",
            &format!(
                "Builder '{builder}' for element 0 has no 'Set' methods or 'Build' methods \
                 that match (case-insensitively) the following parameters: DATAFILE. The \
                 available configuration methods on this builder are: \nThe available 'Build' \
                 methods have the following signatures: \n\ndatafile (String),\
                 createTempDataCopy (Boolean)\n"
            ),
            NO_INNER
        )
    );

    // Two names, as the build method takes, but one is not its parameter.
    let found = message(build(&one(ElementOptions::new("DataFileEngine")
        .parameter("DataFile", "51Degrees.hash")
        .parameter("CreateTempCopy", "true"))));
    assert!(
        found.contains("the following parameters: DATAFILE,CREATETEMPCOPY. The available"),
        "{found}"
    );
}

#[test]
fn two_build_methods_taking_the_same_names_fail() {
    let registry = BuilderRegistry::new().with(
        BuilderDefinition::new("TwiceBuilder", |_: (), context: &mut BuildContext| {
            finish(Element::new("twice", ()), context)
        })
        .build_method(vec![Parameter::new::<i32>("value")], |_, _| Ok(()))
        .build_method(vec![Parameter::new::<String>("value")], |_, _| Ok(())),
    );
    let found = match PipelineBuilder::new().build_from_configuration(
        &registry,
        &one(ElementOptions::new("Twice").parameter("value", "1")),
    ) {
        Err(Error::PipelineConfiguration { message }) => message,
        _ => panic!("expected a configuration error"),
    };
    assert_eq!(
        found,
        failed(
            "Twice",
            "No SubElements",
            "Builder '()' for element 0 has multiple 'Build' methods that match the following \
             parameters: VALUE. Matching method signatures are: \nvalue (Int32)\nvalue \
             (String)\n",
            NO_INNER
        )
    );
}

#[test]
fn a_builder_with_no_build_method_fails() {
    let registry = BuilderRegistry::new().with(BuilderDefinition::new(
        "NoBuildBuilder",
        |_: (), context: &mut BuildContext| finish(Element::new("nobuild", ()), context),
    ));
    let found = match PipelineBuilder::new()
        .build_from_configuration(&registry, &one(ElementOptions::new("NoBuild")))
    {
        Err(Error::PipelineConfiguration { message }) => message,
        _ => panic!("expected a configuration error"),
    };
    assert_eq!(
        found,
        failed(
            "NoBuild",
            "No SubElements",
            "Builder '()' for element 0 has no 'Build' methods.",
            NO_INNER
        )
    );
}

#[test]
fn a_name_matching_two_builders_fails() {
    let registry = BuilderRegistry::new()
        .with(multiply_definition())
        .with(
            other_multiply_definition()
                .alternate_name("Multiply")
                .alternate_name("Splitter"),
        )
        .with(splitter_definition().alternate_name("Splitter"));
    let run = |name: &str| match PipelineBuilder::new().build_from_configuration(
        &registry,
        &one(ElementOptions::new(name).parameter("multiple", "2")),
    ) {
        Err(Error::PipelineConfiguration { message }) => message,
        _ => panic!("expected a configuration error"),
    };
    assert_eq!(
        run("Multiply"),
        failed(
            "Multiply",
            "No SubElements",
            &format!(
                "The flow element builder name 'Multiply' matches multiple builders: [{},{}].",
                type_name::<MultiplyBy>(),
                type_name::<OtherMultiplyBy>()
            ),
            NO_INNER
        )
    );
    assert!(run("Splitter").contains(&format!(
        "matches multiple builders: [{},{}].",
        type_name::<OtherMultiplyBy>(),
        type_name::<SplitterBuilder>()
    )));
    assert!(run("MultiplyByElementBuilder").contains(&format!(
        "matches multiple builders: [{},{}].",
        type_name::<MultiplyBy>(),
        type_name::<OtherMultiplyBy>()
    )));
}

#[test]
fn a_builder_added_twice_is_found_once() {
    let mut registry = BuilderRegistry::new();
    registry
        .add(multiply_definition())
        .add(splitter_definition())
        .add(multiply_definition());
    let registry = registry.with(splitter_definition());
    assert_eq!(
        registry.builder_names(),
        ["MultiplyByElementBuilder", "ListSplitterElementBuilder"]
    );
    let pipeline = PipelineBuilder::new()
        .build_from_configuration(
            &registry,
            &PipelineOptions::new()
                .element(ElementOptions::new("Multiply").parameter("multiple", "2"))
                .element(ElementOptions::new("ListSplitterElement")),
        )
        .expect("each builder is found once");
    assert_eq!(keys(&pipeline), ["multiplyby", "listsplitter"]);
}

#[test]
fn a_builder_of_one_name_and_another_type_is_added() {
    let registry = BuilderRegistry::new()
        .with(multiply_definition())
        .with(other_multiply_definition());
    assert_eq!(
        registry.builder_names(),
        ["MultiplyByElementBuilder", "MultiplyByElementBuilder"]
    );
}

#[test]
fn a_parameter_given_twice_fails() {
    let found = message(build(&one(ElementOptions::new("ListSplitterElement")
        .parameter("MaxLength", "1")
        .parameter("maxlength", "2"))));
    assert_eq!(
        found,
        failed(
            "ListSplitterElement",
            "No SubElements",
            "The build parameter 'MaxLength' is given more than once for element 0.",
            NO_INNER
        )
    );
}

#[test]
fn sub_elements_make_a_parallel_group() {
    let pipeline = build(
        &PipelineOptions::new()
            .element(ElementOptions::new("EnumPerfProfile"))
            .element(ElementOptions::group([
                ElementOptions::new("ListSplitterElement"),
                ElementOptions::new("MultiplyByElement").parameter("Multiple", "3"),
            ])),
    )
    .unwrap();
    assert!(pipeline.is_concurrent());
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    for key in ["enumperfprofile", "listsplitter", "multiplyby"] {
        assert!(data.get_str(key).is_some(), "{key}");
    }
    let mut listed = keys(&pipeline);
    listed.sort_unstable();
    assert_eq!(
        listed,
        vec!["enumperfprofile", "listsplitter", "multiplyby"]
    );
    assert_eq!(pipeline.resource::<Element<i32>>().unwrap().settings, 3);
}

#[test]
fn a_group_with_a_builder_name_or_settings_fails() {
    let group = || ElementOptions::group([ElementOptions::new("ListSplitterElement")]);
    let mut named = group();
    named.builder_name = Some("ListSplitterElement".into());
    let with_settings = group().parameter("MaxLength", "1");
    for (options, builder) in [
        (named, "ListSplitterElement"),
        (with_settings, "BuilderName not set"),
    ] {
        let found = message(build(
            &PipelineOptions::new()
                .element(ElementOptions::new("EnumPerfProfile"))
                .element(options),
        ));
        assert_eq!(
            found,
            failed(
                builder,
                "ListSplitterElement",
                "ElementOptions 1 contains both SubElements and other settings values. This \
                 is invalid",
                NO_INNER
            )
        );
    }
}

#[test]
fn a_group_inside_a_group_fails() {
    let found = message(build(&one(ElementOptions::group([
        ElementOptions::new("ListSplitterElement"),
        ElementOptions::group([ElementOptions::new("EnumPerfProfile")]),
    ]))));
    assert_eq!(
        found,
        failed(
            "BuilderName not set",
            "ListSplitterElement,",
            "ElementOptions 0 contains nested sub elements. This is not supported.",
            NO_INNER
        )
    );
}

#[test]
fn an_element_of_a_group_that_fails_is_named_by_its_place_in_the_group() {
    let found = message(build(&one(ElementOptions::group([
        ElementOptions::new("ListSplitterElement"),
        ElementOptions::new("MultiplyByElement").parameter("Multiple", "x"),
    ]))));
    let builder = type_name::<MultiplyBy>();
    assert_eq!(
        found,
        failed(
            "BuilderName not set",
            "ListSplitterElement,MultiplyByElement",
            &format!(
                "Method 'Build' on builder '{builder}' for element 1 in element 0 expects a \
                 parameter of type 'Int32'. Failed to parse value 'x' using the static \
                 'TryParse' method."
            ),
            NO_INNER
        )
    );
}

#[test]
fn elements_made_before_a_failure_are_released() {
    for options in [
        PipelineOptions::new()
            .element(ElementOptions::new("CountedElement"))
            .element(ElementOptions::new("CountedElement"))
            .element(ElementOptions::new("FailingElement")),
        PipelineOptions::new()
            .element(ElementOptions::new("CountedElement"))
            .element(ElementOptions::group([
                ElementOptions::new("CountedElement"),
                ElementOptions::new("FailingElement"),
            ])),
    ] {
        let before = DROPPED.load(Ordering::SeqCst);
        let found = message(build(&options));
        assert!(
            found.contains("\nThe failing element always fails.\n"),
            "{found}"
        );
        assert_eq!(DROPPED.load(Ordering::SeqCst) - before, 2);
    }
}

#[test]
fn a_builder_finds_what_an_earlier_builder_added() {
    let pipeline = build(
        &PipelineOptions::new()
            .element(ElementOptions::new("ServiceElement"))
            .element(ElementOptions::new("UsesServiceElement")),
    )
    .unwrap();
    let element = pipeline.resource::<Element<&'static str>>().unwrap();
    assert_eq!(element.settings, "made by the configuration");
    assert!(pipeline.resource::<Service>().is_some());

    let found = message(build(&one(ElementOptions::new("UsesServiceElement"))));
    assert!(
        found.contains("\nNo service was made before this element.\n"),
        "{found}"
    );
}

#[test]
fn a_builder_finds_a_resource_given_to_the_pipeline_builder() {
    let pipeline = PipelineBuilder::new()
        .add_resource(Arc::new(Service("given in code")))
        .build_from_configuration(&registry(), &one(ElementOptions::new("UsesServiceElement")))
        .unwrap();
    let element = pipeline.resource::<Element<&'static str>>().unwrap();
    assert_eq!(element.settings, "given in code");
}

#[test]
fn a_builder_finds_the_resource_added_most_recently() {
    let pipeline = PipelineBuilder::new()
        .add_resource(Arc::new(Service("given in code")))
        .build_from_configuration(
            &registry(),
            &PipelineOptions::new()
                .element(ElementOptions::new("ServiceElement"))
                .element(ElementOptions::new("UsesServiceElement")),
        )
        .unwrap();
    let element = pipeline.resource::<Element<&'static str>>().unwrap();
    assert_eq!(element.settings, "made by the configuration");
    // The pipeline keeps both, the one given in code first.
    assert_eq!(pipeline.resource::<Service>().unwrap().0, "given in code");
}

#[test]
fn a_builder_finds_every_resource_of_a_type_oldest_first() {
    let mut registry = registry();
    registry.add(
        BuilderDefinition::new(
            "ListsServicesElementBuilder",
            |_, context: &mut BuildContext| {
                let found: Vec<&'static str> = context
                    .resources::<Service>()
                    .iter()
                    .map(|service| service.0)
                    .collect();
                finish(Element::new("listsservices", found), context)
            },
        )
        .build_method(vec![], |_, _| Ok(())),
    );
    let pipeline = PipelineBuilder::new()
        .add_resource(Arc::new(Service("given in code")))
        .build_from_configuration(
            &registry,
            &PipelineOptions::new()
                .element(ElementOptions::new("ServiceElement"))
                .element(ElementOptions::new("ListsServicesElement")),
        )
        .unwrap();
    let element = pipeline.resource::<Element<Vec<&'static str>>>().unwrap();
    assert_eq!(
        element.settings,
        ["given in code", "made by the configuration"]
    );

    let pipeline = PipelineBuilder::new()
        .build_from_configuration(&registry, &one(ElementOptions::new("ListsServicesElement")))
        .unwrap();
    let element = pipeline.resource::<Element<Vec<&'static str>>>().unwrap();
    assert!(element.settings.is_empty());
}

#[test]
fn elements_added_in_code_are_replaced_by_the_configuration() {
    let pipeline = PipelineBuilder::new()
        .add_element(Arc::new(Element::new("incode", ())))
        .build_from_configuration(
            &registry(),
            &one(ElementOptions::new("ListSplitterElement")),
        )
        .unwrap();
    assert_eq!(keys(&pipeline), vec!["listsplitter"]);
}

#[test]
fn the_pipeline_settings_are_applied() {
    // .NET 4.5.129 has both forms of the suppress setting.
    for name in [
        "SetSuppressProcessException",
        "SuppressProcessException",
        "suppressprocessexception",
        "SetSuppressProcessExceptions",
        "SuppressProcessExceptions",
    ] {
        let pipeline = build(
            &one(ElementOptions::new("ListSplitterElement"))
                .parameter(name, "True")
                .parameter("SetAutoDisposeElements", "false"),
        )
        .unwrap();
        assert!(pipeline.is_suppress_process_exceptions(), "{name}");
    }
    let pipeline = build(
        &one(ElementOptions::new("ListSplitterElement"))
            .parameter("SetSuppressProcessException", "false"),
    )
    .unwrap();
    assert!(!pipeline.is_suppress_process_exceptions());
}

#[test]
fn an_unknown_pipeline_setting_is_ignored_as_dotnet_ignores_it() {
    let pipeline =
        build(&one(ElementOptions::new("ListSplitterElement")).parameter("SetNoSuchSetting", "1"))
            .unwrap();
    assert_eq!(keys(&pipeline), vec!["listsplitter"]);
}

#[test]
fn a_pipeline_setting_of_the_wrong_type_fails() {
    let found = message(build(
        &one(ElementOptions::new("ListSplitterElement"))
            .parameter("SetSuppressProcessException", "maybe"),
    ));
    assert_eq!(
        found,
        format!(
            "Method 'SetSuppressProcessException' on builder '{}' for pipeline expects a \
             parameter of type 'Boolean'. Failed to parse value 'maybe' using the static \
             'TryParse' method.",
            type_name::<PipelineBuilder>()
        )
    );
    let found = message(build(
        &one(ElementOptions::new("ListSplitterElement"))
            .parameter("SuppressProcessExceptions", "maybe"),
    ));
    assert_eq!(
        found,
        format!(
            "Method 'SetSuppressProcessExceptions' on builder '{}' for pipeline expects a \
             parameter of type 'Boolean'. Failed to parse value 'maybe' using the static \
             'TryParse' method.",
            type_name::<PipelineBuilder>()
        )
    );
}

/// .NET's `PipelineBuilder` builds a pipeline with no elements from a
/// configuration with none, where `PipelineBuilder::build` refuses one.
#[test]
fn a_configuration_with_no_elements_fails() {
    assert_eq!(
        message(build(&PipelineOptions::new())),
        "A pipeline must contain at least one flow element."
    );
}

/// The settings of a pipeline builder that adds an element called `first`
/// to run first when `SetFirst` is true, and one called `last` to run last
/// unless `SetLast` is false, as .NET's `FiftyOnePipelineBuilder` adds the
/// Sequence and usage sharing elements.
struct Wrapper {
    first: bool,
    last: bool,
}

/// Whether the wrapper's step found a list splitter on its own.
struct SplitterFound(bool);

fn wrapper_definition() -> PipelineDefinition<Wrapper> {
    PipelineDefinition::new(
        || Wrapper {
            first: false,
            last: true,
        },
        |wrapper: Wrapper, elements: &mut PipelineElements, context: &mut BuildContext| {
            let found = elements.contains::<Element<Splitter>>();
            context.add_resource(Arc::new(SplitterFound(found)));
            if wrapper.first {
                elements.insert_first(Arc::new(Element::new("first", ())));
            }
            if wrapper.last {
                elements.push(Arc::new(Element::new("last", ())));
            }
            Ok(())
        },
    )
    .setting("SetFirst", |mut wrapper: Wrapper, first: bool| {
        wrapper.first = first;
        wrapper
    })
    .setting("SetLast", |mut wrapper: Wrapper, last: bool| {
        wrapper.last = last;
        wrapper
    })
}

fn build_wrapped(options: &PipelineOptions) -> Result<Arc<Pipeline>> {
    let mut registry = registry();
    registry.set_pipeline(wrapper_definition());
    PipelineBuilder::new().build_from_configuration(&registry, options)
}

#[test]
fn a_pipeline_definition_adds_elements_before_the_pipeline_is_built() {
    let splitter = || one(ElementOptions::new("ListSplitterElement"));
    let built = |options: &PipelineOptions| build_wrapped(options).unwrap();
    assert_eq!(keys(&built(&splitter())), ["listsplitter", "last"]);
    for (first, last) in [
        ("SetFirst", "SetLast"),
        ("First", "Last"),
        ("first", "LAST"),
    ] {
        let options = splitter().parameter(first, "True").parameter(last, "false");
        assert_eq!(keys(&built(&options)), ["first", "listsplitter"], "{first}");
    }
    let options = splitter().parameter("SetFirst", " true ");
    assert_eq!(keys(&built(&options)), ["first", "listsplitter", "last"]);

    // A configuration with no elements builds when the step adds one.
    assert_eq!(keys(&built(&PipelineOptions::new())), ["last"]);

    // The registry without the definition has neither the settings nor the
    // step, so the setting is ignored.
    let options = splitter().parameter("SetFirst", "true");
    assert_eq!(keys(&build(&options).unwrap()), ["listsplitter"]);
}

#[test]
fn a_pipeline_definition_finds_an_element_only_on_its_own() {
    let splitter = || ElementOptions::new("ListSplitterElement");
    let multiply = || ElementOptions::new("MultiplyByElementBuilder").parameter("Multiple", "3");
    for (options, expected) in [
        (PipelineOptions::new().element(splitter()), true),
        (PipelineOptions::new().element(multiply()), false),
        (
            PipelineOptions::new()
                .element(multiply())
                .element(splitter()),
            true,
        ),
        (
            PipelineOptions::new().element(ElementOptions::group([multiply(), splitter()])),
            false,
        ),
        // A group of one element is that element on its own.
        (
            PipelineOptions::new().element(ElementOptions::group([splitter()])),
            true,
        ),
    ] {
        let pipeline = build_wrapped(&options).unwrap();
        let found = pipeline.resource::<SplitterFound>().unwrap();
        assert_eq!(found.0, expected, "{options:?}");
    }
}

#[test]
fn a_pipeline_definition_names_itself_for_every_pipeline_setting() {
    let splitter = || one(ElementOptions::new("ListSplitterElement"));
    for (name, method) in [
        ("Last", "SetLast"),
        ("SuppressProcessException", "SetSuppressProcessException"),
        ("SuppressProcessExceptions", "SetSuppressProcessExceptions"),
        ("AutoDisposeElements", "SetAutoDisposeElements"),
    ] {
        let found = message(build_wrapped(&splitter().parameter(name, "maybe")));
        assert_eq!(
            found,
            format!(
                "Method '{method}' on builder '{}' for pipeline expects a parameter of type \
                 'Boolean'. Failed to parse value 'maybe' using the static 'TryParse' method.",
                type_name::<Wrapper>()
            ),
            "{name}"
        );
    }

    // Every pipeline's own settings are still found, and an unknown one is
    // ignored.
    let pipeline = build_wrapped(
        &splitter()
            .parameter("SuppressProcessExceptions", "true")
            .parameter("SetNoSuchSetting", "1"),
    )
    .unwrap();
    assert!(pipeline.is_suppress_process_exceptions());
    assert_eq!(keys(&pipeline), ["listsplitter", "last"]);
}

/// How many times the refusing definition's step has run.
static REFUSING_STEPS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn a_pipeline_definition_step_runs_last_and_can_fail_the_build() {
    let mut registry = registry();
    registry.set_pipeline(wrapper_definition());
    // A second definition replaces the first, so `SetLast` is unknown.
    registry.set_pipeline(
        PipelineDefinition::new(
            || false,
            |refuse: bool, _: &mut PipelineElements, _: &mut BuildContext| {
                REFUSING_STEPS.fetch_add(1, Ordering::SeqCst);
                if refuse {
                    return Err(Error::configuration("The step refused."));
                }
                Ok(())
            },
        )
        .setting("SetRefuse", |_: bool, refuse: bool| refuse),
    );
    let build = |options: &PipelineOptions| {
        PipelineBuilder::new().build_from_configuration(&registry, options)
    };
    let splitter = || one(ElementOptions::new("ListSplitterElement"));

    let pipeline = build(&splitter().parameter("SetLast", "true")).unwrap();
    assert_eq!(keys(&pipeline), ["listsplitter"]);
    assert_eq!(REFUSING_STEPS.load(Ordering::SeqCst), 1);

    assert_eq!(
        message(build(&splitter().parameter("Refuse", "true"))),
        "The step refused."
    );
    assert_eq!(REFUSING_STEPS.load(Ordering::SeqCst), 2);

    // A setting that cannot be read fails before the step runs, and an
    // element that cannot be made fails before the settings are read.
    assert!(message(build(&splitter().parameter("Refuse", "maybe")))
        .starts_with("Method 'SetRefuse' on builder 'bool' for pipeline"));
    let options = one(ElementOptions::new("NoSuchBuilder")).parameter("Refuse", "maybe");
    assert!(message(build(&options)).starts_with("Configuration failed to build."));
    assert_eq!(REFUSING_STEPS.load(Ordering::SeqCst), 2);
}

/// A configuration file in the shape .NET reads, with comments, every value
/// written as .NET's sample writes it, and a parallel group.
const SAMPLE: &str = r#"{
  /* Settings for the whole application, which the pipeline ignores. */
  "Logging": { "LogLevel": { "Default": "Information" } },
  "PipelineOptions": {
    "Elements": [
      {
        "BuilderName": "EnumPerfProfileBuilder",
        "BuildParameters": {
          // A value is matched ignoring case.
          "SetPerformanceProfile": "maxperformance"
        }
      },
      {
        "SubElements": [
          {
            "BuilderName": "ListSplitterElement",
            "BuildParameters": {
              "SetDelimiters": "|,#",
              "SetMaxLength": 3,
            }
          },
          {
            "BuilderName": "Multiply",
            "BuildParameters": { "multiple": 7 }
          }
        ]
      },
      {
        "BuilderName": "DataFileEngine",
        "BuildParameters": {
          "DataFile": "51Degrees.hash",
          "CreateTempDataCopy": false
        }
      }
    ],
    "BuildParameters": {
      "SetSuppressProcessException": true
    },
    "ClientSideEvidenceEnabled": true,
    "UseAsyncScript": true
  }
}"#;

#[test]
fn a_json_file_written_for_dotnet_makes_the_same_pipeline_as_code() {
    let options = PipelineOptions::from_json(SAMPLE).unwrap();
    let in_code = PipelineOptions::new()
        .element(
            ElementOptions::new("EnumPerfProfileBuilder")
                .parameter("SetPerformanceProfile", "maxperformance"),
        )
        .element(ElementOptions::group([
            ElementOptions::new("ListSplitterElement")
                .parameter("SetDelimiters", "|,#")
                .parameter("SetMaxLength", "3"),
            ElementOptions::new("Multiply").parameter("multiple", "7"),
        ]))
        .element(
            ElementOptions::new("DataFileEngine")
                .parameter("CreateTempDataCopy", "False")
                .parameter("DataFile", "51Degrees.hash"),
        )
        .parameter("SetSuppressProcessException", "True");
    assert_eq!(options, in_code);

    let pipeline = build(&options).unwrap();
    assert!(pipeline.is_suppress_process_exceptions());
    assert!(pipeline.is_concurrent());
    assert_eq!(
        splitter(&pipeline),
        Splitter {
            delimiters: vec!["|".into(), "#".into()],
            max_length: 3
        }
    );
    assert_eq!(pipeline.resource::<Element<i32>>().unwrap().settings, 7);
    assert_eq!(
        pipeline.resource::<Element<Profile>>().unwrap().settings,
        Profile::MaxPerformance
    );
    assert_eq!(
        pipeline.resource::<Element<DataFile>>().unwrap().settings,
        DataFile {
            path: "51Degrees.hash".into(),
            copy: false
        }
    );
}

#[test]
fn json_build_parameters_are_applied_in_dotnet_order_not_file_order() {
    // .NET's configuration orders names ignoring case, so SetDelimiter is
    // applied before SetDelimiters whichever comes first in the file.
    let options = PipelineOptions::from_json(
        r#"{ "pipelineoptions": { "elements": [ { "buildername": "ListSplitterElement",
            "buildparameters": { "SetDelimiters": "|,#", "SetDelimiter": ";" } } ] } }"#,
    )
    .unwrap();
    let names: Vec<&str> = options.elements[0]
        .build_parameters
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(names, vec!["SetDelimiter", "SetDelimiters"]);
    let pipeline = build(&options).unwrap();
    assert_eq!(splitter(&pipeline).delimiters, vec!["|", "#"]);
}

#[test]
fn the_obsolete_pipeline_parameters_name_is_read() {
    let options = PipelineOptions::from_json(
        r#"{ "PipelineOptions": { "Elements": [ { "BuilderName": "ListSplitterElement" } ],
            "PipelineBuilderParameters": { "SetSuppressProcessException": true } } }"#,
    )
    .unwrap();
    assert!(build(&options).unwrap().is_suppress_process_exceptions());
}

/// .NET binds `PipelineBuilderParameters` first, into the dictionary that
/// `BuildParameters` then fills, each in key order. A name in both in the
/// same case keeps its first place with the second value, and one in
/// another case is a second entry.
#[test]
fn the_pipeline_parameters_of_both_sections_are_read_in_dotnet_order() {
    let options = PipelineOptions::from_json(
        r#"{ "PipelineOptions": {
            "BuildParameters": { "SetSuppressProcessException": "false", "B": "2", "a": "3" },
            "PipelineBuilderParameters": {
                "SetSuppressProcessException": "true", "C": "1", "A": "4" } } }"#,
    )
    .unwrap();
    let expected: Vec<(String, ConfigValue)> = [
        ("A", "4"),
        ("C", "1"),
        ("SetSuppressProcessException", "false"),
        ("a", "3"),
        ("B", "2"),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value.into()))
    .collect();
    assert_eq!(options.build_parameters, expected);
}

/// The setting applied last wins, so with the order above the
/// `BuildParameters` value is the one the pipeline takes when each section
/// gives one form of the name, whether the same name, the name in another
/// case or another form of the same setting.
#[test]
fn a_pipeline_setting_in_both_sections_takes_the_build_parameters_value() {
    for (obsolete, current) in [
        ("SetSuppressProcessException", "SetSuppressProcessException"),
        ("SetSuppressProcessException", "setsuppressprocessexception"),
        ("SuppressProcessException", "SetSuppressProcessException"),
        ("SetSuppressProcessException", "SuppressProcessException"),
    ] {
        for value in [true, false] {
            let options = PipelineOptions::from_json(&format!(
                r#"{{ "PipelineOptions": {{
                    "Elements": [ {{ "BuilderName": "ListSplitterElement" }} ],
                    "BuildParameters": {{ "{current}": {value} }},
                    "PipelineBuilderParameters": {{ "{obsolete}": {} }} }} }}"#,
                !value
            ))
            .unwrap();
            assert_eq!(
                build(&options).unwrap().is_suppress_process_exceptions(),
                value,
                "{obsolete} then {current} given {value}"
            );
        }
    }
}

fn json_message(text: &str) -> String {
    match PipelineOptions::from_json(text) {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(options) => panic!("read {options:?}"),
    }
}

#[test]
fn json_that_dotnet_would_refuse_or_not_read_is_refused() {
    for (text, expected) in [
        (
            r#"{ "PipelineOptions": { "Elements": [ { "BuilderName": "A",
                "BuildParameters": { "SetMaxLength": 1, "setmaxlength": 2 } } ] } }"#,
            "A duplicate key 'PipelineOptions:Elements:0:BuildParameters:setmaxlength' was \
             found.",
        ),
        (
            r#"{ "PipelineOptions": { "Elements": [ { "BuilderName": "A",
                "BuildParameters": { "SetDelimiters": ["|", ","] } } ] } }"#,
            "'PipelineOptions:Elements:0:BuildParameters:SetDelimiters' is a JSON array or \
             object. Give each build parameter as text, and a list as comma-separated text.",
        ),
        (
            r#"{ "Logging": {} }"#,
            "The configuration has no 'PipelineOptions' section.",
        ),
        (
            r#"{ "PipelineOptions": [] }"#,
            "The configuration's 'PipelineOptions' section must be a JSON object.",
        ),
        (
            r#"{ "PipelineOptions": { "Elements": { "0": {} } } }"#,
            "'PipelineOptions:Elements' must be a JSON array of elements.",
        ),
        (
            r#"{ "PipelineOptions": { "Elements": [ "A" ] } }"#,
            "'PipelineOptions:Elements:0' must be a JSON object describing an element.",
        ),
        (
            r#"{ "PipelineOptions": { "Elements": [ { "BuilderName": ["A"] } ] } }"#,
            "'PipelineOptions:Elements:0:BuilderName' must be text.",
        ),
        (
            r#"{ "PipelineOptions": { "Elements": [ { "BuildParameters": "A" } ] } }"#,
            "'PipelineOptions:Elements:0:BuildParameters' must be a JSON object of names and \
             values.",
        ),
        (
            r#"[]"#,
            "The configuration's top level must be a JSON object.",
        ),
        (
            "{ \"PipelineOptions\": { } ",
            "The configuration is not valid JSON, a missing ',' or '}' in an object at line \
             1 column 26.",
        ),
    ] {
        assert_eq!(json_message(text), expected, "{text}");
    }
}

/// A backslash before a character outside ASCII, as in a Windows path
/// written with single backslashes, is a bad escape refused with its place.
#[test]
fn a_bad_escape_before_a_character_outside_ascii_is_refused() {
    for (text, place) in [
        (r#"{"a": "\é"}"#, "line 1 column 9"),
        (
            "{ \"PipelineOptions\": {\n  \"DataFile\": \"C:\\Ärzte\\51Degrees.hash\" } }",
            "line 2 column 19",
        ),
    ] {
        assert_eq!(
            json_message(text),
            format!("The configuration is not valid JSON, a bad escape in a string at {place}."),
            "{text}"
        );
    }
}

/// A configuration that builds, using each form the JSON reader takes.
const EVERY_FORM: &str = "\u{feff}// A comment.
{ \"pipelineoptions\": { /* Names in any case. */
    \"Elements\": [
      { \"SubElements\": [
        { \"BuilderName\": \"ListSplitterElement\", \"BuildParameters\": { \"delim\": null,
          \"SetDelimiters\": \"\\u007c,\\t,\\\"é😀\\ud83d\\ude00,\\b\\f\\n\\r\", \"SetMaxLength\": 3, },
        },
        { \"BuilderName\": \"DataFileEngine\", \"BuildParameters\": {
          \"DataFile\": \"C:\\\\Data\\/51Degrees.hash\", \"CreateTempDataCopy\": true } }, ], },
      { \"BuilderName\": \"EnumPerfProfile\",
        \"BuildParameters\": { \"PerformanceProfile\": \"highperformance\" } },
      { \"BuilderName\": \"Multiply\", \"BuildParameters\": { \"multiple\": -20 } },
    ],
    \"PipelineBuilderParameters\": { \"SetSuppressProcessException\": true },
    \"BuildParameters\": { \"SuppressProcessException\": false, \"Precision\": -1.5e-3 } } }
";

/// Characters that make up JSON, and some wider than one byte.
const JSON_CHARACTERS: &[char] = &[
    '{', '}', '[', ']', '"', ':', ',', '\\', '/', '*', '-', '+', '.', '0', '1', '9', 'e', 'E', 'u',
    'a', 'd', 't', 'r', 'n', 'l', 'f', 's', ' ', '\n', '\t', '\u{0}', 'é', 'Ä', '😀', '\u{feff}',
];

/// A repeatable source of numbers for the texts below, xorshift64*.
struct Random(u64);

impl Random {
    fn below(&mut self, limit: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 33) as usize % limit.max(1)
    }

    fn character(&mut self) -> char {
        JSON_CHARACTERS[self.below(JSON_CHARACTERS.len())]
    }
}

/// Every beginning and end of each sample, and thousands of samples with a
/// few characters changed and of random texts, are read and, when they
/// read, built, without a panic. Only that each call returns is checked.
#[test]
fn no_configuration_text_panics() {
    const CHANGED: usize = 4000;
    const RANDOM: usize = 4000;
    const MOST_CHANGES: usize = 4;
    const LONGEST_RANDOM: usize = 48;
    let samples = [SAMPLE, EVERY_FORM];
    for sample in samples {
        build(&PipelineOptions::from_json(sample).unwrap()).unwrap();
    }
    let mut texts: Vec<String> = Vec::new();
    for sample in samples {
        for end in (0..=sample.len()).filter(|end| sample.is_char_boundary(*end)) {
            texts.push(sample[..end].to_string());
            texts.push(sample[end..].to_string());
        }
    }
    let mut random = Random(0x51de_c0de_51de_c0de);
    for _ in 0..CHANGED {
        let mut text: Vec<char> = samples[random.below(samples.len())].chars().collect();
        for _ in 0..=random.below(MOST_CHANGES) {
            let at = random.below(text.len());
            match random.below(3) {
                0 => text.insert(at, random.character()),
                1 => {
                    text.remove(at);
                }
                _ => text[at] = random.character(),
            }
        }
        texts.push(text.into_iter().collect());
    }
    for _ in 0..RANDOM {
        let length = random.below(LONGEST_RANDOM + 1);
        texts.push((0..length).map(|_| random.character()).collect());
    }
    for text in &texts {
        let returned = std::panic::catch_unwind(|| {
            if let Ok(options) = PipelineOptions::from_json(text) {
                let _ = build(&options);
            }
        });
        assert!(returned.is_ok(), "{text:?} panicked");
    }
}

/// .NET 4.5.129 bound a JSON `null` as empty text, reading the file with
/// .NET's configuration reader 8.0.1 and binder 8.0.2.
#[test]
fn json_values_are_read_as_dotnet_reads_them() {
    let options = PipelineOptions::from_json(
        r#"{ "PipelineOptions": { "Elements": [ { "BuilderName": 7, "SubElements": [],
            "BuildParameters": { "a": 1.50, "b": true, "c": false, "d": "", "e": "x",
            "f": null } },
            { "BuilderName": null, "BuildParameters": null, "SubElements": null },
            { "BuildParameters": { "a": "x" } } ],
            "BuildParameters": { "SetSuppressProcessException": null } } }"#,
    )
    .unwrap();
    assert_eq!(
        options,
        PipelineOptions::new()
            .element(
                ElementOptions::new("7")
                    .parameter("a", "1.50")
                    .parameter("b", "True")
                    .parameter("c", "False")
                    .parameter("d", "")
                    .parameter("e", "x")
                    .parameter("f", "")
            )
            .element(ElementOptions::new(""))
            .element(ElementOptions::default().parameter("a", "x"))
            .parameter("SetSuppressProcessException", "")
    );
}

/// A `null` builder name is empty, so .NET's message gives no name, where a
/// missing one is "not set". A `null` setting is given empty text, which
/// .NET then reads as the setting's type reads it.
#[test]
fn a_json_null_is_given_as_empty_text() {
    let from = |text: &str| build(&PipelineOptions::from_json(text).unwrap());
    let builder = type_name::<SplitterBuilder>();
    for (text, name) in [
        (
            r#"{ "PipelineOptions": { "Elements": [ { "BuilderName": null } ] } }"#,
            "",
        ),
        (
            r#"{ "PipelineOptions": { "Elements": [ { "BuildParameters": {} } ] } }"#,
            "BuilderName not set",
        ),
    ] {
        assert_eq!(
            message(from(text)),
            failed(
                name,
                "No SubElements",
                "A BuilderName must be specified for element 0.",
                NO_INNER
            ),
            "{text}"
        );
    }
    assert_eq!(
        message(from(
            r#"{ "PipelineOptions": { "Elements": [ { "BuilderName": "ListSplitterElement",
                "BuildParameters": { "MaxLength": null } } ] } }"#
        )),
        failed(
            "ListSplitterElement",
            "No SubElements",
            &format!(
                "Method 'SetMaxLength' on builder '{builder}' for element 0 expects a \
                 parameter of type 'Int32'. Failed to parse value '' using the static \
                 'TryParse' method."
            ),
            NO_INNER
        )
    );
}

/// A builder whose settings share names, as .NET's on-premise engine
/// builders take `SetUpdatePollingInterval` as an `Int32` and as a
/// `TimeSpan`, and `SetPerformanceProfile` as text and as an enumeration.
#[derive(Debug, Clone, PartialEq, Default)]
struct Overloaded {
    number: Option<i32>,
    fraction: Option<f64>,
    text: Option<String>,
}

fn overloaded_registry() -> BuilderRegistry {
    BuilderRegistry::new().with(
        BuilderDefinition::new("OverloadedElementBuilder", |o: Overloaded, context| {
            finish(Element::new("overloaded", o), context)
        })
        .setting("SetValue", |mut o: Overloaded, number: i32| {
            o.number = Some(number);
            o
        })
        .setting("SetValue", |mut o: Overloaded, fraction: f64| {
            o.fraction = Some(fraction);
            o
        })
        .setting("SetName", |mut o: Overloaded, text: String| {
            o.text = Some(text);
            o
        })
        .code_only_setting("SetName", "PerformanceProfiles")
        .build_method(vec![], |_, _| Ok(Overloaded::default())),
    )
}

fn overloaded(element: ElementOptions) -> Result<Overloaded> {
    let pipeline =
        PipelineBuilder::new().build_from_configuration(&overloaded_registry(), &one(element))?;
    Ok(pipeline
        .resource::<Element<Overloaded>>()
        .expect("the element")
        .settings
        .clone())
}

/// .NET tries a method's overloads in turn until one reads the value, and
/// gives the last one's message when none does, as .NET 4.5.180's Hash
/// engine builder did for `UpdatePollingInterval`, read as an `Int32` and
/// then as a `TimeSpan`.
#[test]
fn settings_sharing_a_name_are_tried_in_turn() {
    let named = || ElementOptions::new("OverloadedElementBuilder");
    assert_eq!(
        overloaded(named().parameter("Value", "2")).unwrap(),
        Overloaded {
            number: Some(2),
            ..Overloaded::default()
        }
    );
    assert_eq!(
        overloaded(named().parameter("Value", "2.5")).unwrap(),
        Overloaded {
            fraction: Some(2.5),
            ..Overloaded::default()
        }
    );
    let builder = type_name::<Overloaded>();
    assert_eq!(
        message(overloaded(named().parameter("Value", "x")).map(|_| unreachable_pipeline())),
        failed(
            "OverloadedElementBuilder",
            "No SubElements",
            &format!(
                "Method 'SetValue' on builder '{builder}' for element 0 expects a parameter of \
                 type 'Double'. Failed to parse value 'x' using the static 'TryParse' method."
            ),
            NO_INNER
        )
    );
}

/// .NET passes a list to a method taking text as it is, and the call's
/// refusal ends the search, so the overload after it is not tried, as .NET
/// 4.5.180's Hash engine builder showed for `PerformanceProfile`, taken as
/// text and as an enumeration.
#[test]
fn a_list_refused_by_a_text_setting_ends_the_search() {
    let named = || ElementOptions::new("OverloadedElementBuilder");
    assert_eq!(
        overloaded(named().parameter("Name", "a")).unwrap().text,
        Some("a".to_string())
    );
    assert_eq!(
        message(overloaded(named().parameter("Name", vec!["a"])).map(|_| unreachable_pipeline())),
        failed(
            "OverloadedElementBuilder",
            "No SubElements",
            "Object of type 'System.String[]' cannot be converted to type 'System.String'.",
            NO_INNER
        )
    );
}

/// Stands in for a pipeline where a test expects an error, so the result
/// keeps the type `message` takes.
fn unreachable_pipeline() -> Arc<Pipeline> {
    panic!("the element was made")
}

/// .NET casts a build parameter's value to text before reading it as any
/// other type, so a list given in code fails the cast, as .NET 4.5.180's
/// Hash engine builder gave for `createTempDataCopy`.
#[test]
fn a_list_given_for_a_build_parameter_that_is_not_text_fails_the_cast() {
    assert_eq!(
        message(build(&one(ElementOptions::new("DataFileEngine")
            .parameter("DataFile", "51Degrees.hash")
            .parameter("CreateTempDataCopy", vec!["true"])))),
        failed(
            "DataFileEngine",
            "No SubElements",
            "Unable to cast object of type 'System.String[]' to type 'System.String'.",
            NO_INNER
        )
    );
}

/// A build method whose parameter only code can give, as .NET's on-premise
/// engine builders have `Build(Stream data)`. The messages are those .NET
/// 4.5.180's Hash engine builder gave.
#[test]
fn a_build_parameter_taken_in_code_only_refuses_every_value() {
    struct Streamed;
    let registry = BuilderRegistry::new().with(
        BuilderDefinition::new("StreamedElementBuilder", |_: Streamed, context| {
            finish(Element::new("streamed", ()), context)
        })
        .build_method(vec![Parameter::new::<String>("datafile")], |_, _| {
            Ok(Streamed)
        })
        .build_method(vec![Parameter::code_only("data", "Stream")], |_, _| {
            panic!("no configuration can give a stream")
        }),
    );
    let make = |element: ElementOptions| {
        PipelineBuilder::new().build_from_configuration(&registry, &one(element))
    };
    let named = || ElementOptions::new("StreamedElementBuilder");
    let builder = type_name::<Streamed>();
    let refused = |text: &str| failed("StreamedElementBuilder", "No SubElements", text, NO_INNER);

    assert_eq!(
        message(make(named().parameter("Data", "x"))),
        refused(&format!(
            "Method 'Build' on builder '{builder}' for element 0 expects a parameter of type \
             'Stream' but this type does not have the expected 'TryParse' method (or has \
             multiples that cannot be resolved)."
        ))
    );
    assert_eq!(
        message(make(named().parameter("Data", vec!["x"]))),
        refused("Unable to cast object of type 'System.String[]' to type 'System.String'.")
    );
    assert_eq!(
        message(make(
            named().parameter("Data", "x").parameter("DataFile", "y")
        )),
        refused(&format!(
            "Builder '{builder}' for element 0 has no 'Set' methods or 'Build' methods that \
             match (case-insensitively) the following parameters: DATA,DATAFILE. The available \
             configuration methods on this builder are: \nThe available 'Build' methods have \
             the following signatures: \ndatafile (String)\ndata (Stream)\n"
        ))
    );
    assert_eq!(
        keys(&make(named().parameter("DataFile", "y")).unwrap()),
        ["streamed"]
    );
}

/// `Box<[String]>` reads as .NET's `String[]`, from comma separated text or
/// from a list given in code.
#[test]
fn a_string_array_setting_reads_text_and_lists() {
    assert_eq!(<Box<[String]> as ConfigType>::TYPE_NAME, "String[]");
    assert_eq!(
        &*<Box<[String]> as ConfigType>::parse_text("a,\"b,c\"").unwrap(),
        ["a", "b,c"]
    );
    assert_eq!(
        &*<Box<[String]> as ConfigType>::parse_value(&ConfigValue::List(vec!["x".into()])).unwrap(),
        ["x"]
    );
}
