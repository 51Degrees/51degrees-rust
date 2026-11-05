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
    ElementOptions, Parameter, ParseFailure, PipelineOptions,
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
                let service = context.resource::<Service>().ok_or_else(|| {
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
        .with(multiply_definition().alternate_name("Splitter"))
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
                "The flow element builder name 'Multiply' matches multiple builders: [{0},{0}].",
                type_name::<MultiplyBy>()
            ),
            NO_INNER
        )
    );
    assert!(run("Splitter").contains(&format!(
        "matches multiple builders: [{},{}].",
        type_name::<MultiplyBy>(),
        type_name::<SplitterBuilder>()
    )));
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
    for name in [
        "SetSuppressProcessException",
        "SuppressProcessException",
        "suppressprocessexception",
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
}

#[test]
fn a_configuration_with_no_elements_fails() {
    assert_eq!(
        message(build(&PipelineOptions::new())),
        "A pipeline must contain at least one flow element."
    );
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
            r#"{ "PipelineOptions": { "Elements": [ { "BuilderName": "A",
                "BuildParameters": { "SetMaxLength": null } } ] } }"#,
            "'PipelineOptions:Elements:0:BuildParameters:SetMaxLength' has no value.",
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

#[test]
fn json_values_are_read_as_dotnet_reads_them() {
    let options = PipelineOptions::from_json(
        r#"{ "PipelineOptions": { "Elements": [ { "BuilderName": 7, "SubElements": [],
            "BuildParameters": { "a": 1.50, "b": true, "c": false, "d": "", "e": "x" } },
            { "BuilderName": null, "BuildParameters": null, "SubElements": null } ],
            "BuildParameters": {} } }"#,
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
            )
            .element(ElementOptions::default())
    );
}
