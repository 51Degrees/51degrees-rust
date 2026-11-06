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

//! The cloud countries translation engine in a pipeline with a cloud
//! request engine whose cloud is a stand-in, following the cases of the
//! .NET engine's tests and what .NET's cloud data helper does with each
//! entry. Each test holds [`SERIAL`], because a logger is installed once for
//! the whole process and the records a test reads must be its own.

use std::sync::{Arc, Mutex, MutexGuard};

use fiftyone_cloud_request_engine::{
    CloudHttpClient, CloudHttpRequest, CloudHttpResponse, CloudRequestEngine,
};
use fiftyone_ip_intelligence_translation::{
    CloudCountriesTranslationEngine, CountriesTranslationData, COUNTRY_NAMES_TRANSLATED_KEY,
};
use fiftyone_pipeline_core::{
    ElementData, Error, Evidence, FlowData, FlowElement, Pipeline, PropertyValue,
    PropertyValueType, WeightedValue,
};
use fiftyone_pipeline_engines::{AspectEngine, EngineDeployment, MissingPropertyReason};

/// The six properties .NET's test lists, as the cloud gives their types.
const PRODUCT: &str = r#""countrynamestranslated":{"DataTier":"CloudV4","Properties":[
    {"Name":"CountryNamesGeographicalTranslated","Type":"WeightedString"},
    {"Name":"CountryNamesPopulationTranslated","Type":"WeightedString"},
    {"Name":"CountryNamesGeographicalAllTranslated","Type":"Array"},
    {"Name":"CountryNamesPopulationAllTranslated","Type":"Array"},
    {"Name":"CountryCodesGeographicalAll","Type":"Array"},
    {"Name":"CountryCodesPopulationAll","Type":"Array"}]}"#;

const IP_PRODUCT: &str = r#""ip":{"DataTier":"CloudV4","Properties":[
    {"Name":"RegisteredName","Type":"String"}]}"#;

const NOT_GRANTED: &str = "ERROR Aspect properties could not be loaded for \
     CloudCountriesTranslationEngine - 'countrynamestranslated'";

/// Records at warning level or above from the translation crates, the cloud
/// request engine and the aspect data, as "LEVEL message".
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held by each test for its whole run.
static SERIAL: Mutex<()> = Mutex::new(());

struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let target = metadata.target();
        metadata.level() <= log::Level::Warn
            && (target.starts_with("fiftyone_ip_intelligence_translation")
                || target.starts_with("fiftyone_pipeline_engines")
                || target.starts_with("fiftyone_pipeline_translation"))
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            LOGGED
                .lock()
                .unwrap()
                .push(format!("{} {}", record.level(), record.args()));
        }
    }

    fn flush(&self) {}
}

static LOGGER: TestLogger = TestLogger;

/// Install the logger, wait for any other test to finish and clear the
/// records it left.
fn start() -> MutexGuard<'static, ()> {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Warn);
    let guard = SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    LOGGED.lock().unwrap().clear();
    guard
}

/// Take the records logged since [`start`] or the last call.
fn take_logged() -> Vec<String> {
    std::mem::take(&mut *LOGGED.lock().unwrap())
}

/// A stand-in cloud granting `products` and answering every data request
/// with `status` and `data`.
struct FakeCloud {
    products: String,
    status: u16,
    data: String,
}

impl CloudHttpClient for FakeCloud {
    fn send(&self, request: &CloudHttpRequest) -> Result<CloudHttpResponse, String> {
        let (status, body) = if request.url.contains("accessibleproperties") {
            (200, format!(r#"{{"Products":{{{}}}}}"#, self.products))
        } else if request.url.contains("evidencekeys") {
            (
                200,
                r#"["query.client-ip","header.accept-language"]"#.to_owned(),
            )
        } else {
            (self.status, self.data.clone())
        };
        Ok(CloudHttpResponse {
            status,
            body,
            retry_after: None,
        })
    }
}

fn fake(products: &[&str], status: u16, data: &str) -> Arc<FakeCloud> {
    Arc::new(FakeCloud {
        products: products.join(","),
        status,
        data: data.to_owned(),
    })
}

/// A request engine with a resource key, on `cloud`.
fn request_engine(cloud: Arc<FakeCloud>) -> Arc<CloudRequestEngine> {
    Arc::new(
        CloudRequestEngine::builder()
            .resource_key("test-resource-key")
            .endpoint("https://cloud.example.test/api/v4/")
            .http_client(cloud)
            .build()
            .unwrap(),
    )
}

fn countries_engine(request: &Arc<CloudRequestEngine>) -> Arc<CloudCountriesTranslationEngine> {
    Arc::new(
        CloudCountriesTranslationEngine::builder()
            .cloud_request_engine(Arc::clone(request))
            .build()
            .unwrap(),
    )
}

/// A pipeline of the request engine and the countries engine, recording
/// errors rather than failing on them.
fn pipeline(request: Arc<CloudRequestEngine>) -> Arc<Pipeline> {
    let countries = countries_engine(&request);
    Pipeline::builder()
        .add_element(request)
        .add_element(countries)
        .suppress_process_exceptions(true)
        .build()
        .unwrap()
}

/// A pipeline on a cloud granting the six properties and answering `data`.
fn granted(data: &str) -> Arc<Pipeline> {
    pipeline(request_engine(fake(&[PRODUCT], 200, data)))
}

fn process(pipeline: &Arc<Pipeline>) -> FlowData {
    let mut data = pipeline.create_flow_data_with(
        Evidence::builder()
            .add("query.client-ip", "185.28.167.78")
            .add("header.accept-language", "de-DE")
            .build(),
    );
    data.process().unwrap();
    data
}

/// The messages of the errors recorded by the countries engine.
fn countries_errors(data: &FlowData) -> Vec<String> {
    data.errors()
        .iter()
        .filter(|error| error.element_data_key == "countrynamestranslated")
        .map(|error| match &error.source {
            Error::Pipeline { message } => message.clone(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn the_properties_are_those_the_metadata_lists_with_their_forms() {
    let _serial = start();
    let request = request_engine(fake(&[PRODUCT], 200, "{}"));
    let engine = countries_engine(&request);

    assert_eq!(engine.data_key(), "countrynamestranslated");
    assert_eq!(engine.deployment(), EngineDeployment::Cloud);
    assert_eq!(engine.engine_type_name(), "CloudCountriesTranslationEngine");
    assert!(!engine
        .evidence_key_filter()
        .include("header.accept-language"));
    assert!(engine.properties_loaded());

    let properties: Vec<(&str, PropertyValueType)> = engine
        .properties()
        .iter()
        .map(|property| (property.name.as_str(), property.value_type))
        .collect();
    assert_eq!(
        properties,
        [
            (
                "CountryNamesGeographicalTranslated",
                PropertyValueType::KeyValueList
            ),
            (
                "CountryNamesPopulationTranslated",
                PropertyValueType::KeyValueList
            ),
            (
                "CountryNamesGeographicalAllTranslated",
                PropertyValueType::StringList
            ),
            (
                "CountryNamesPopulationAllTranslated",
                PropertyValueType::StringList
            ),
            ("CountryCodesGeographicalAll", PropertyValueType::StringList),
            ("CountryCodesPopulationAll", PropertyValueType::StringList),
        ]
    );
    assert_eq!(engine.aspect_properties().len(), 6);
    assert_eq!(take_logged(), Vec::<String>::new());
}

/// .NET's `ProcessEngine_PopulatesWeightedAndPlainProperties`, with the
/// weighted item's fields in capitals, and a second weighted list in lower
/// case kept in the response's order.
#[test]
fn weighted_and_plain_lists_are_read_with_fields_in_any_case() {
    let _serial = start();
    let pipeline = granted(
        r#"{"countrynamestranslated":{
            "CountryNamesGeographicalTranslated":[
                {"Value":"Deutschland","RawWeighting":65535}],
            "countrynamespopulationtranslated":[
                {"rawweighting":20000,"value":"Frankreich"},
                {"rawweighting":45535,"value":"Deutschland"}],
            "CountryCodesGeographicalAll":["DE","FR"],
            "CountryNamesGeographicalAllTranslated":["Deutschland","Frankreich"]}}"#,
    );
    let data = process(&pipeline);
    assert_eq!(countries_errors(&data), Vec::<String>::new());
    let translated = data.get(COUNTRY_NAMES_TRANSLATED_KEY).unwrap();

    assert_eq!(
        translated.country_names_geographical_translated().value(),
        Ok(&vec![WeightedValue::new(65535, "Deutschland".to_owned())])
    );
    assert_eq!(
        translated.country_names_population_translated().value(),
        Ok(&vec![
            WeightedValue::new(20000, "Frankreich".to_owned()),
            WeightedValue::new(45535, "Deutschland".to_owned()),
        ])
    );
    assert_eq!(
        translated.country_codes_geographical_all().value(),
        Ok(&vec!["DE".to_owned(), "FR".to_owned()])
    );
    assert_eq!(
        translated
            .country_names_geographical_all_translated()
            .value(),
        Ok(&vec!["Deutschland".to_owned(), "Frankreich".to_owned()])
    );
    // The names are those the response gives, in its order.
    assert_eq!(
        translated.keys(),
        [
            "CountryNamesGeographicalTranslated",
            "countrynamespopulationtranslated",
            "CountryCodesGeographicalAll",
            "CountryNamesGeographicalAllTranslated",
        ]
    );
    assert_eq!(take_logged(), Vec::<String>::new());
}

#[test]
fn a_null_value_gives_its_reason_or_unknown() {
    let _serial = start();
    let pipeline = granted(
        r#"{"countrynamestranslated":{
            "CountryNamesGeographicalTranslated":null,
            "CountryNamesGeographicalTranslatednullreason":"No location for this range.",
            "CountryCodesGeographicalAll":null}}"#,
    );
    let data = process(&pipeline);
    assert_eq!(countries_errors(&data), Vec::<String>::new());
    let translated = data.get(COUNTRY_NAMES_TRANSLATED_KEY).unwrap();

    assert_eq!(
        translated
            .country_names_geographical_translated()
            .no_value_message(),
        Some("No location for this range.")
    );
    assert_eq!(
        translated
            .country_codes_geographical_all()
            .no_value_message(),
        Some("Unknown")
    );
    // The reason entry is not a property of its own.
    assert_eq!(
        translated.keys(),
        [
            "CountryNamesGeographicalTranslated",
            "CountryCodesGeographicalAll"
        ]
    );
    assert_eq!(take_logged(), Vec::<String>::new());
}

/// A property the metadata lists and the response did not return gives the
/// reason, as .NET's cloud data does, and logs it.
#[test]
fn a_property_the_response_did_not_return_gives_the_reason() {
    let _serial = start();
    let pipeline = granted(r#"{"countrynamestranslated":{"CountryCodesGeographicalAll":["DE"]}}"#);
    let data = process(&pipeline);
    let translated = data.get(COUNTRY_NAMES_TRANSLATED_KEY).unwrap();

    let message = translated.country_codes_population_all();
    let message = message.no_value_message().unwrap();
    assert!(
        message.starts_with(
            "Property 'CountryCodesPopulationAll' not found in data for element \
             'countrynamestranslated'. "
        ),
        "{message}"
    );
    assert_eq!(
        translated
            .get("CountryCodesPopulationAll")
            .unwrap_err()
            .message,
        message
    );
    assert!(!translated.has_property("CountryCodesPopulationAll"));
    assert_eq!(
        take_logged(),
        vec![
            "WARN Property 'CountryCodesPopulationAll' missing from aspect data \
             'TranslationData'. PropertyNotAccessibleWithResourceKey"
                .to_owned();
            2
        ]
    );
}

/// `SortingCultureUsed` and any other entry the metadata does not list is
/// kept, and logs .NET's warning.
#[test]
fn an_entry_the_metadata_does_not_list_is_kept_and_logged() {
    let _serial = start();
    let pipeline = granted(
        r#"{"countrynamestranslated":{
            "SortingCultureUsed":"de-DE",
            "CountryCodesGeographicalAll":["DE"],
            "Unlisted":["a","b"],
            "UnlistedObject":{"a":1}}}"#,
    );
    let data = process(&pipeline);
    assert_eq!(countries_errors(&data), Vec::<String>::new());
    let translated = data.get(COUNTRY_NAMES_TRANSLATED_KEY).unwrap();

    assert_eq!(translated.sorting_culture_used(), Some("de-DE"));
    assert_eq!(
        translated.get("Unlisted").unwrap(),
        PropertyValue::StringList(vec!["a".to_owned(), "b".to_owned()])
    );
    // An object cannot be held, so it is left out.
    assert_eq!(
        translated.keys(),
        [
            "SortingCultureUsed",
            "CountryCodesGeographicalAll",
            "Unlisted"
        ]
    );
    assert_eq!(
        take_logged(),
        [
            "WARN No meta-data entry for property 'SortingCultureUsed' in \
             'CloudCountriesTranslationEngine'",
            "WARN No meta-data entry for property 'Unlisted' in \
             'CloudCountriesTranslationEngine'",
            "WARN No meta-data entry for property 'UnlistedObject' in \
             'CloudCountriesTranslationEngine'",
        ]
    );
}

/// .NET reads the section with a dictionary's indexer, so a response
/// without it fails with that indexer's message, leaving the data empty.
#[test]
fn a_response_without_the_section_is_an_error_leaving_the_data_empty() {
    let _serial = start();
    let pipeline = granted(r#"{"ip":{"RegisteredName":"Example"}}"#);
    let data = process(&pipeline);

    assert_eq!(
        countries_errors(&data),
        ["The given key 'countrynamestranslated' was not present in the dictionary."]
    );
    let translated = data.get(COUNTRY_NAMES_TRANSLATED_KEY).unwrap();
    assert_eq!(translated.keys(), Vec::<String>::new());
    assert!(!translated.country_codes_geographical_all().has_value());
}

#[test]
fn a_section_that_is_not_an_object_is_an_error() {
    let _serial = start();
    let pipeline = granted(r#"{"countrynamestranslated":["DE"]}"#);
    let data = process(&pipeline);
    assert_eq!(
        countries_errors(&data),
        ["The 'countrynamestranslated' member of the cloud response is not a JSON object."]
    );
    assert_eq!(
        data.get(COUNTRY_NAMES_TRANSLATED_KEY).unwrap().keys(),
        Vec::<String>::new()
    );
}

#[test]
fn a_value_not_in_the_form_its_type_says_is_an_error() {
    let _serial = start();
    for (entry, form) in [
        (
            r#""CountryCodesGeographicalAll":"DE""#,
            "CountryCodesGeographicalAll' in the 'countrynamestranslated' member of the \
             cloud response is not a list of strings.",
        ),
        (
            r#""CountryCodesGeographicalAll":[{"a":1}]"#,
            "CountryCodesGeographicalAll' in the 'countrynamestranslated' member of the \
             cloud response is not a list of strings.",
        ),
        (
            r#""CountryNamesGeographicalTranslated":["Deutschland"]"#,
            "CountryNamesGeographicalTranslated' in the 'countrynamestranslated' member of \
             the cloud response is not a list of weighted strings.",
        ),
        (
            r#""CountryNamesGeographicalTranslated":[{"rawweighting":70000,"value":"Deutschland"}]"#,
            "CountryNamesGeographicalTranslated' in the 'countrynamestranslated' member of \
             the cloud response is not a list of weighted strings.",
        ),
    ] {
        let pipeline = granted(&format!(r#"{{"countrynamestranslated":{{{entry}}}}}"#));
        let data = process(&pipeline);
        assert_eq!(
            countries_errors(&data),
            [format!("The value of '{form}")],
            "{entry}"
        );
        assert_eq!(
            data.get(COUNTRY_NAMES_TRANSLATED_KEY).unwrap().keys(),
            Vec::<String>::new(),
            "{entry}"
        );
    }
}

/// When the cloud request fails the engine records an error that does not
/// fail processing, and each property read gives `CloudRequestFailed`, as
/// .NET's cloud engines do.
#[test]
fn a_failed_cloud_request_marks_the_data() {
    let _serial = start();
    let pipeline = pipeline(request_engine(fake(
        &[PRODUCT],
        500,
        r#"{"errors":["The cloud service is not available."]}"#,
    )));
    let data = process(&pipeline);

    let ours: Vec<_> = data
        .errors()
        .iter()
        .filter(|error| error.element_data_key == "countrynamestranslated")
        .collect();
    assert_eq!(ours.len(), 1);
    assert!(!ours[0].should_throw);
    assert!(matches!(ours[0].source, Error::Pipeline { .. }));

    let translated = data.get(COUNTRY_NAMES_TRANSLATED_KEY).unwrap();
    let codes = translated.country_codes_geographical_all();
    let message = codes.no_value_message().unwrap();
    assert!(
        message.starts_with(
            "Property 'CountryCodesGeographicalAll' not found in data for element \
             'countrynamestranslated'. "
        ),
        "{message}"
    );
    // A failed request logs each read at information level, so nothing
    // here is recorded.
    assert_eq!(take_logged(), Vec::<String>::new());
}

#[test]
fn a_key_without_the_product_has_no_properties_and_logs_once() {
    let _serial = start();
    let request = request_engine(fake(
        &[IP_PRODUCT],
        200,
        r#"{"ip":{"RegisteredName":"Example"}}"#,
    ));
    let engine = countries_engine(&request);
    assert!(engine.has_loaded_properties());
    assert!(engine.properties().is_empty());
    assert_eq!(
        engine
            .missing_property_reason("CountryCodesGeographicalAll")
            .reason,
        MissingPropertyReason::ProductNotAccessibleWithResourceKey
    );
    assert_eq!(take_logged(), [NOT_GRANTED]);

    // Processing logs nothing more.
    let pipeline = Pipeline::builder()
        .add_element(request)
        .add_element(engine)
        .suppress_process_exceptions(true)
        .build()
        .unwrap();
    process(&pipeline);
    assert_eq!(take_logged(), Vec::<String>::new());
}

/// With only a license key the granted properties are not listed, so each
/// value's form is read from the JSON, nothing is logged, and a property
/// the response did not return has no reason.
#[test]
fn a_license_key_alone_reads_each_form_from_the_json() {
    let _serial = start();
    let request = Arc::new(
        CloudRequestEngine::builder()
            .license_key("test-license-key")
            .values(["countrynamestranslated.countrycodesgeographicalall"])
            .endpoint("https://cloud.example.test/api/v4/")
            .http_client(fake(
                &[],
                200,
                r#"{"countrynamestranslated":{
                    "CountryNamesGeographicalTranslated":[
                        {"rawweighting":65535,"value":"Deutschland"}],
                    "CountryCodesGeographicalAll":["DE","FR"],
                    "SortingCultureUsed":"de-DE"}}"#,
            ))
            .build()
            .unwrap(),
    );
    let pipeline = pipeline(request);
    let data = process(&pipeline);
    assert_eq!(countries_errors(&data), Vec::<String>::new());
    let translated = data.get(COUNTRY_NAMES_TRANSLATED_KEY).unwrap();

    assert_eq!(
        translated.country_names_geographical_translated().value(),
        Ok(&vec![WeightedValue::new(65535, "Deutschland".to_owned())])
    );
    assert_eq!(
        translated.country_codes_geographical_all().value(),
        Ok(&vec!["DE".to_owned(), "FR".to_owned()])
    );
    assert_eq!(translated.sorting_culture_used(), Some("de-DE"));
    assert_eq!(
        translated.country_codes_population_all().no_value_message(),
        Some("No value for property 'CountryCodesPopulationAll'.")
    );
    assert!(pipeline.flow_elements()[1].properties().is_empty());
    assert_eq!(take_logged(), Vec::<String>::new());
}

#[test]
fn the_engine_needs_a_request_engine() {
    let _serial = start();
    assert!(matches!(
        CloudCountriesTranslationEngine::builder().build(),
        Err(Error::PipelineConfiguration { .. })
    ));

    // An engine with no request engine before it in the pipeline.
    let request = request_engine(fake(&[PRODUCT], 200, "{}"));
    let alone = Pipeline::builder()
        .add_element(countries_engine(&request))
        .suppress_process_exceptions(false)
        .build()
        .unwrap();
    let mut data = alone.create_flow_data_with(Evidence::builder().build());
    assert!(data.process().is_err());
}
