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

//! The two country translation engines in a pipeline, following the cases
//! of the .NET IP Intelligence translation tests.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use fiftyone_ip_intelligence_shared::IpIntelligenceDataBase;
use fiftyone_ip_intelligence_translation::{
    CountriesTranslationData, CountriesTranslationEngine, CountryCodeTranslationData,
    CountryCodeTranslationEngine, COUNTRY_NAMES_KEY, COUNTRY_NAMES_TRANSLATED_KEY,
};
use fiftyone_pipeline_core::{
    ElementData, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, Pipeline, PropertyMetaData, PropertyValue, PropertyValueType, Result, TypedKey,
    WeightedValue,
};
use fiftyone_pipeline_translation::TranslationData;
use icu_collator::options::{CollatorOptions, Strength};
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
use icu_locale_core::Locale;

/// Adds a copy of the data it is given under its key.
struct Source<T> {
    key: &'static str,
    data: T,
    filter: EvidenceKeyFilterWhitelist,
}

impl<T: ElementData + Clone> Source<T> {
    fn new(key: &'static str, data: T) -> Self {
        Source {
            key,
            data,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        }
    }
}

impl<T: ElementData + Clone> FlowElement for Source<T> {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let copy = self.data.clone();
        data.get_or_add(TypedKey::<T>::new(self.key), move || copy)?;
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

/// A weighted list as the IP Intelligence data gives it in its property
/// bag, each item a `value` and a `weight` from 0 to 1, in the order given.
fn weighted(items: &[(u16, &str)]) -> PropertyValue {
    PropertyValue::KeyValueList(
        items
            .iter()
            .map(|(raw, code)| {
                BTreeMap::from([
                    ("value".to_owned(), PropertyValue::from(*code)),
                    (
                        "weight".to_owned(),
                        PropertyValue::Double(f64::from(WeightedValue::new(*raw, ()).weighting())),
                    ),
                ])
            })
            .collect(),
    )
}

/// Weightings for codes in order, highest first, adding up to 65,535, as the
/// .NET tests give them.
fn weightify<'a>(codes: &[&'a str]) -> Vec<(u16, &'a str)> {
    if codes.is_empty() {
        return Vec::new();
    }
    let count = codes.len() as u32;
    let mut left = u32::from(u16::MAX);
    let mut buckets: Vec<u32> = (0..count).map(|i| count - i).collect();
    left -= buckets.iter().sum::<u32>();
    let additional = left / count;
    left -= additional * count;
    for bucket in &mut buckets {
        *bucket += additional;
    }
    buckets[0] += left;
    buckets
        .into_iter()
        .zip(codes)
        .map(|(bucket, code)| (bucket as u16, *code))
        .collect()
}

fn ip_weighted(geographical: &[(u16, &str)], population: &[(u16, &str)]) -> MapElementData {
    MapElementData::new()
        .set("CountryCodesGeographical", weighted(geographical))
        .set("CountryCodesPopulation", weighted(population))
}

fn ip(geographical: &[&str], population: &[&str]) -> MapElementData {
    ip_weighted(&weightify(geographical), &weightify(population))
}

fn evidence(entries: &[(&str, &str)]) -> Evidence {
    entries
        .iter()
        .fold(Evidence::builder(), |builder, (key, value)| {
            builder.add(key, *value)
        })
        .build()
}

fn run(elements: Vec<Arc<dyn FlowElement>>, entries: &[(&str, &str)]) -> FlowData {
    let pipeline = elements
        .into_iter()
        .fold(Pipeline::builder(), |builder, element| {
            builder.add_element(element)
        })
        .suppress_process_exceptions(true)
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data_with(evidence(entries));
    data.process().unwrap();
    data
}

fn source<T: ElementData + Clone + 'static>(ip: T) -> Arc<dyn FlowElement> {
    Arc::new(Source::new("ip", ip))
}

fn code_engine() -> Arc<dyn FlowElement> {
    Arc::new(CountryCodeTranslationEngine::builder().build().unwrap())
}

fn countries_engine() -> Arc<dyn FlowElement> {
    Arc::new(CountriesTranslationEngine::builder().build().unwrap())
}

/// The IP data, then both engines.
fn process<T: ElementData + Clone + 'static>(ip: T, entries: &[(&str, &str)]) -> FlowData {
    run(vec![source(ip), code_engine(), countries_engine()], entries)
}

fn result(data: &FlowData) -> &TranslationData {
    data.get(COUNTRY_NAMES_TRANSLATED_KEY)
        .expect("the countries engine adds its data")
}

fn values(list: &[WeightedValue<String>]) -> Vec<&str> {
    list.iter().map(|item| item.value.as_str()).collect()
}

fn translated_geographical(data: &FlowData) -> Vec<WeightedValue<String>> {
    result(data)
        .country_names_geographical_translated()
        .into_value()
        .unwrap()
}

fn names_all(data: &FlowData) -> Vec<String> {
    result(data)
        .country_names_geographical_all_translated()
        .into_value()
        .unwrap()
}

fn codes_all(data: &FlowData) -> Vec<String> {
    result(data)
        .country_codes_geographical_all()
        .into_value()
        .unwrap()
}

fn error_messages(data: &FlowData) -> Vec<String> {
    data.errors()
        .iter()
        .map(|error| error.source.message().into_owned())
        .collect()
}

/// The collator the engine says it sorted with.
fn collator_used(data: &FlowData) -> CollatorBorrowed<'static> {
    let culture = result(data)
        .sorting_culture_used()
        .expect("the engine has run");
    if culture.is_empty() {
        let mut options = CollatorOptions::default();
        options.strength = Some(Strength::Secondary);
        return Collator::try_new(CollatorPreferences::default(), options).unwrap();
    }
    let locale = Locale::try_from_str(culture).unwrap();
    Collator::try_new((&locale).into(), CollatorOptions::default()).unwrap()
}

fn assert_sorted(names: &[String], collator: &CollatorBorrowed<'static>) {
    for pair in names.windows(2) {
        assert_ne!(
            collator.compare(&pair[0], &pair[1]),
            Ordering::Greater,
            "'{}' comes before '{}'",
            pair[0],
            pair[1]
        );
    }
}

#[test]
fn each_country_code_is_given_its_english_name() {
    let data = run(
        vec![source(ip(&["GB", "FR"], &["GB", "FR"])), code_engine()],
        &[],
    );
    let names = data
        .get(COUNTRY_NAMES_KEY)
        .expect("the engine adds its data");
    let geographical = names.country_names_geographical().into_value().unwrap();
    let population = names.country_names_population().into_value().unwrap();
    assert_eq!(values(&geographical), ["United Kingdom", "France"]);
    assert_eq!(values(&population), ["United Kingdom", "France"]);
    assert_eq!(
        geographical
            .iter()
            .map(|item| item.raw_weighting)
            .collect::<Vec<_>>(),
        [32768, 32767]
    );
}

#[test]
fn the_country_names_are_translated() {
    let data = process(
        ip(&["GB", "FR"], &["GB", "FR"]),
        &[("header.accept-language", "fr_FR")],
    );
    assert_eq!(
        values(&translated_geographical(&data)),
        ["Royaume-Uni", "France"]
    );
    let population = result(&data)
        .country_names_population_translated()
        .into_value()
        .unwrap();
    assert_eq!(values(&population), ["Royaume-Uni", "France"]);
}

#[test]
fn the_all_lists_lead_with_the_weighted_countries_and_sort_the_rest() {
    for ip_data in [
        ip_weighted(
            &[(30000, "FR"), (35535, "GB")],
            &[(30000, "FR"), (35535, "GB")],
        ),
        ip(&["GB", "FR"], &["GB", "FR"]),
    ] {
        let data = process(ip_data, &[("header.accept-language", "fr_FR")]);
        let names = names_all(&data);
        let codes = codes_all(&data);
        assert_eq!(names.len(), codes.len());
        assert_eq!(names.len(), 250);
        assert_eq!(names[..2], ["Royaume-Uni", "France"]);
        assert_eq!(codes[..2], ["GB", "FR"]);
        assert!(!codes[2..].iter().any(|code| code == "GB" || code == "FR"));
        assert_eq!(result(&data).sorting_culture_used(), Some("fr-FR"));
        assert_sorted(&names[2..], &collator_used(&data));
        for list in [
            result(&data).country_names_population_all_translated(),
            result(&data).country_codes_population_all(),
        ] {
            assert_eq!(list.into_value().unwrap().len(), 250);
        }
    }
}

#[test]
fn with_no_language_the_all_lists_are_in_english_sorted_ignoring_case() {
    let data = process(ip(&["GB"], &["GB"]), &[]);
    let names = names_all(&data);
    assert_eq!(names[0], "United Kingdom");
    assert_eq!(codes_all(&data)[0], "GB");
    assert_eq!(names.len(), 250);
    assert_eq!(result(&data).sorting_culture_used(), Some(""));
    assert_sorted(&names[1..], &collator_used(&data));
}

#[test]
fn with_no_weighted_codes_every_country_is_sorted() {
    let data = process(ip(&[], &[]), &[("header.accept-language", "de_DE")]);
    let names = names_all(&data);
    assert_eq!(names.len(), 250);
    assert_eq!(result(&data).sorting_culture_used(), Some("de-DE"));
    assert_sorted(&names, &collator_used(&data));
    assert!(translated_geographical(&data).is_empty());
}

#[test]
fn ip_data_without_the_codes_gives_every_country() {
    let mut empty = IpIntelligenceDataBase::new("ip");
    empty.set_weighted_string_no_value("CountryCodesGeographical", "Not in the key.");
    for data in [
        process(
            MapElementData::new(),
            &[("header.accept-language", "de_DE")],
        ),
        process(empty, &[("header.accept-language", "de_DE")]),
    ] {
        assert_eq!(names_all(&data).len(), 250);
        assert_eq!(codes_all(&data).len(), 250);
        assert_eq!(
            result(&data)
                .country_codes_population_all()
                .into_value()
                .unwrap()
                .len(),
            250
        );
        assert_sorted(&names_all(&data), &collator_used(&data));
    }
}

#[test]
fn with_no_ip_data_every_country_is_given_and_the_missing_data_is_an_error() {
    let data = run(vec![code_engine(), countries_engine()], &[]);
    assert_eq!(names_all(&data).len(), 250);
    assert_eq!(codes_all(&data).len(), 250);
    assert_eq!(
        error_messages(&data),
        ["The source data 'ip' could not be found in the FlowData."]
    );
}

#[test]
fn a_code_that_is_not_a_country_is_left_out() {
    let data = process(
        ip(&["Unknown"], &["Unknown"]),
        &[("header.accept-language", "de_DE")],
    );
    let collator = collator_used(&data);
    for (codes, names) in [
        (
            result(&data).country_codes_geographical_all(),
            result(&data).country_names_geographical_all_translated(),
        ),
        (
            result(&data).country_codes_population_all(),
            result(&data).country_names_population_all_translated(),
        ),
    ] {
        let codes = codes.into_value().unwrap();
        let names = names.into_value().unwrap();
        assert_eq!(codes.len(), 250);
        assert_eq!(names.len(), 250);
        assert!(!codes.iter().any(|code| code == "Unknown"));
        assert!(!names.iter().any(|name| name == "Unknown"));
        assert_sorted(&names, &collator);
    }
    // The weighted names still carry the code as it was given.
    assert_eq!(values(&translated_geographical(&data)), ["Unknown"]);

    let data = process(
        ip(&["DE", "Unknown"], &["DE", "Unknown"]),
        &[("header.accept-language", "de_DE")],
    );
    let names = names_all(&data);
    let codes = codes_all(&data);
    assert_eq!(
        (codes[0].as_str(), names[0].as_str()),
        ("DE", "Deutschland")
    );
    assert!(!codes.iter().any(|code| code == "Unknown"));
    assert!(!names.iter().any(|name| name == "Unknown"));
    assert_eq!(codes.len(), 250);
    assert_sorted(&names[1..], &collator_used(&data));
}

#[test]
fn a_code_is_known_ignoring_case_but_only_the_same_code_leaves_the_rest() {
    let data = process(ip(&["gb"], &["gb"]), &[]);
    let codes = codes_all(&data);
    let names = names_all(&data);
    assert_eq!(
        (codes[0].as_str(), names[0].as_str()),
        ("gb", "United Kingdom")
    );
    assert_eq!(codes.len(), 251);
    assert!(codes[1..].iter().any(|code| code == "GB"));
}

#[test]
fn population_and_geographical_are_independent() {
    let data = process(ip(&["DE"], &["US", "CN"]), &[]);
    let geographical = codes_all(&data);
    let population = result(&data)
        .country_codes_population_all()
        .into_value()
        .unwrap();
    assert_eq!(geographical[0], "DE");
    assert_eq!(population[..2], ["US", "CN"]);
}

#[test]
fn german_names_lead_the_all_list() {
    let data = process(ip(&["DE"], &["DE"]), &[("header.accept-language", "de_DE")]);
    assert_eq!(values(&translated_geographical(&data)), ["Deutschland"]);
    assert_eq!(names_all(&data)[0], "Deutschland");
    assert_eq!(codes_all(&data)[0], "DE");
}

#[test]
fn a_language_with_a_dash_is_found() {
    let data = process(ip(&["GB"], &["GB"]), &[("header.accept-language", "fr-FR")]);
    assert_eq!(values(&translated_geographical(&data)), ["Royaume-Uni"]);
}

#[test]
fn english_gives_the_english_names() {
    let data = process(
        ip(&["FR", "DE"], &["FR", "DE"]),
        &[("header.accept-language", "en-US,en;q=0.9")],
    );
    assert_eq!(
        values(&translated_geographical(&data)),
        ["France", "Germany"]
    );
    assert_eq!(names_all(&data)[..2], ["France", "Germany"]);
    assert_eq!(result(&data).sorting_culture_used(), Some(""));
}

#[test]
fn english_first_stops_a_less_preferred_language_being_used() {
    let data = process(
        ip(&["DE"], &["DE"]),
        &[(
            "header.accept-language",
            "en-US,en;q=0.9,de-DE;q=0.5,fr;q=0.3",
        )],
    );
    assert_eq!(values(&translated_geographical(&data)), ["Germany"]);
}

#[test]
fn the_most_preferred_language_held_is_used() {
    let data = process(
        ip(&["DE"], &["DE"]),
        &[("header.accept-language", "es,de-DE;q=0.8,fr;q=0.5")],
    );
    assert_eq!(values(&translated_geographical(&data)), ["Alemania"]);
    assert_eq!(result(&data).sorting_culture_used(), Some("es-ES"));
}

#[test]
fn the_translation_query_names_the_language() {
    let data = process(ip(&["GB"], &["GB"]), &[("query.translation", "fr_FR")]);
    assert_eq!(values(&translated_geographical(&data)), ["Royaume-Uni"]);

    let data = process(
        ip(&["GB"], &["GB"]),
        &[
            ("header.accept-language", "de_DE"),
            ("query.translation", "fr_FR"),
        ],
    );
    assert_eq!(values(&translated_geographical(&data)), ["Royaume-Uni"]);
    assert_eq!(names_all(&data)[0], "Royaume-Uni");
}

#[test]
fn the_accept_language_query_comes_before_the_header() {
    let data = process(
        ip(&["GB"], &["GB"]),
        &[
            ("header.accept-language", "de_DE"),
            ("query.accept-language", "es"),
        ],
    );
    assert_eq!(values(&translated_geographical(&data)), ["Reino Unido"]);
    assert_eq!(result(&data).sorting_culture_used(), Some("es-ES"));
}

#[test]
fn translation_keeps_the_order_and_each_weighting() {
    let data = process(
        ip_weighted(
            &[(30000, "FR"), (35535, "GB")],
            &[(30000, "FR"), (35535, "GB")],
        ),
        &[("header.accept-language", "fr_FR")],
    );
    let translated = translated_geographical(&data);
    assert_eq!(
        translated,
        [
            WeightedValue::new(30000, "France".to_owned()),
            WeightedValue::new(35535, "Royaume-Uni".to_owned()),
        ]
    );
}

#[test]
fn the_ip_data_type_gives_the_weightings_exactly() {
    let mut ip_data = IpIntelligenceDataBase::new("ip");
    ip_data.set_weighted_string(
        "CountryCodesGeographical",
        vec![
            WeightedValue::new(20001, "GB".to_owned()),
            WeightedValue::new(45534, "FR".to_owned()),
        ],
    );
    let data = process(ip_data, &[("header.accept-language", "de")]);
    let names = data
        .get(COUNTRY_NAMES_KEY)
        .unwrap()
        .country_names_geographical()
        .into_value()
        .unwrap();
    assert_eq!(
        names,
        [
            WeightedValue::new(45534, "France".to_owned()),
            WeightedValue::new(20001, "United Kingdom".to_owned()),
        ]
    );
    assert_eq!(
        translated_geographical(&data),
        [
            WeightedValue::new(45534, "Frankreich".to_owned()),
            WeightedValue::new(20001, "Vereinigtes Königreich".to_owned()),
        ]
    );
    assert_eq!(codes_all(&data)[..2], ["FR", "GB"]);
}

#[test]
fn equal_weightings_are_ordered_by_translated_name() {
    let data = process(
        ip_weighted(
            &[(30000, "FR"), (30000, "DE")],
            &[(30000, "FR"), (30000, "DE")],
        ),
        &[("header.accept-language", "fr_FR")],
    );
    assert_eq!(codes_all(&data)[..2], ["DE", "FR"]);
    assert_eq!(names_all(&data)[..2], ["Allemagne", "France"]);
}

#[test]
fn an_unknown_language_gives_the_english_names() {
    let data = process(
        ip(&["FR", "DE"], &["FR", "DE"]),
        &[("header.accept-language", "zz-ZZ")],
    );
    assert_eq!(
        values(&translated_geographical(&data)),
        ["France", "Germany"]
    );
    assert_eq!(names_all(&data).len(), 250);
    assert_eq!(codes_all(&data).len(), 250);
    assert!(error_messages(&data).is_empty());
}

#[test]
fn the_all_lists_try_each_language_evidence_in_turn() {
    // The weighted names use the first language evidence only, which names
    // no language held. The lists of every country go on to the next.
    for first in ["zz", "en"] {
        let data = process(
            ip(&["GB"], &["GB"]),
            &[
                ("query.translation", first),
                ("header.accept-language", "fr"),
            ],
        );
        assert_eq!(values(&translated_geographical(&data)), ["United Kingdom"]);
        assert_eq!(names_all(&data)[0], "United Kingdom");
        assert!(names_all(&data).iter().any(|name| name == "Allemagne"));
        assert_eq!(result(&data).sorting_culture_used(), Some("fr-FR"));
        assert_sorted(&names_all(&data)[1..], &collator_used(&data));
    }
}

#[test]
fn an_accept_language_that_cannot_be_read_is_an_error() {
    let data = process(
        ip(&["GB"], &["GB"]),
        &[("header.accept-language", "fr;q=2")],
    );
    assert_eq!(
        error_messages(&data),
        ["The format of value 'fr;q=2' is invalid."]
    );
    assert!(!result(&data).country_codes_geographical_all().has_value());

    // A later value that cannot be read fails the lists of every country,
    // after the weighted names are translated from the first.
    let data = process(
        ip(&["GB"], &["GB"]),
        &[
            ("query.translation", "zz"),
            ("header.accept-language", "fr;q=2"),
        ],
    );
    assert_eq!(
        error_messages(&data),
        ["The format of value 'fr;q=2' is invalid."]
    );
    assert_eq!(values(&translated_geographical(&data)), ["United Kingdom"]);
    assert!(!result(&data).country_codes_geographical_all().has_value());
}

#[test]
fn codes_and_names_line_up_across_the_whole_list() {
    let data = process(ip(&["GB"], &["GB"]), &[("header.accept-language", "fr_FR")]);
    let codes = codes_all(&data);
    let names = names_all(&data);
    assert_eq!(codes.len(), names.len());
    assert_eq!(codes.iter().collect::<HashSet<_>>().len(), codes.len());
    for (code, name) in [("AO", "Angola"), ("AW", "Aruba"), ("AU", "Australie")] {
        let index = codes
            .iter()
            .position(|held| held == code)
            .unwrap_or_else(|| panic!("'{code}' is listed"));
        assert_eq!(names[index], name, "the name at '{code}'");
    }
}

#[test]
fn the_sorting_culture_is_the_locale_found() {
    let data = process(ip(&["GB"], &["GB"]), &[("header.accept-language", "fr_FR")]);
    assert_eq!(result(&data).sorting_culture_used(), Some("fr-FR"));
    let data = process(ip(&["GB"], &["GB"]), &[]);
    assert_eq!(result(&data).sorting_culture_used(), Some(""));
}

#[test]
fn the_population_all_list_names_are_translated() {
    let data = process(ip(&["GB"], &["DE"]), &[("header.accept-language", "fr_FR")]);
    assert_eq!(names_all(&data)[0], "Royaume-Uni");
    assert_eq!(codes_all(&data)[0], "GB");
    assert_eq!(
        result(&data)
            .country_names_population_all_translated()
            .into_value()
            .unwrap()[0],
        "Allemagne"
    );
    assert_eq!(
        result(&data)
            .country_codes_population_all()
            .into_value()
            .unwrap()[0],
        "DE"
    );
}

#[test]
fn every_language_is_held_and_sorts_its_own_names() {
    let engine = CountriesTranslationEngine::builder().build().unwrap();
    let locales: Vec<&str> = engine.engine().languages().locales().collect();
    assert_eq!(
        locales,
        [
            "cs_CZ", "de_DE", "es_ES", "fr_FR", "hu_HU", "it_IT", "ja_JP", "nl_NL", "pl_PL",
            "pt_PT", "ro_RO", "sk_SK", "sv_SE", "tr_TR", "uk_UA", "vi_VN",
        ]
    );
    for locale in locales {
        let data = process(ip(&[], &[]), &[("query.translation", locale)]);
        let culture = locale.replace('_', "-");
        assert_eq!(result(&data).sorting_culture_used(), Some(culture.as_str()));
        let names = names_all(&data);
        assert_eq!(names.len(), 250, "{locale}");
        assert_sorted(&names, &collator_used(&data));
    }
}

#[test]
fn each_engine_lists_its_properties() {
    let names = CountryCodeTranslationEngine::builder().build().unwrap();
    assert_eq!(names.data_key(), "countrynames");
    assert_eq!(
        names
            .properties()
            .iter()
            .map(|property| (property.name.as_str(), property.value_type))
            .collect::<Vec<_>>(),
        [
            ("CountryNamesGeographical", PropertyValueType::KeyValueList),
            ("CountryNamesPopulation", PropertyValueType::KeyValueList),
        ]
    );

    let countries = CountriesTranslationEngine::builder().build().unwrap();
    assert_eq!(countries.data_key(), "countrynamestranslated");
    assert_eq!(
        countries
            .properties()
            .iter()
            .map(|property| (
                property.name.as_str(),
                property.element_data_key.as_str(),
                property.value_type
            ))
            .collect::<Vec<_>>(),
        [
            (
                "CountryNamesGeographicalTranslated",
                "countrynamestranslated",
                PropertyValueType::KeyValueList
            ),
            (
                "CountryNamesPopulationTranslated",
                "countrynamestranslated",
                PropertyValueType::KeyValueList
            ),
            (
                "CountryNamesGeographicalAllTranslated",
                "countrynamestranslated",
                PropertyValueType::StringList
            ),
            (
                "CountryNamesPopulationAllTranslated",
                "countrynamestranslated",
                PropertyValueType::StringList
            ),
            (
                "CountryCodesGeographicalAll",
                "countrynamestranslated",
                PropertyValueType::StringList
            ),
            (
                "CountryCodesPopulationAll",
                "countrynamestranslated",
                PropertyValueType::StringList
            ),
        ]
    );
    for key in [
        "query.translation",
        "query.accept-language",
        "header.accept-language",
    ] {
        assert!(countries.evidence_key_filter().include(key), "{key}");
    }
    assert!(!countries.evidence_key_filter().include("header.user-agent"));
}

#[test]
fn the_data_lists_its_properties_in_the_order_written() {
    let data = process(ip(&["GB"], &["GB"]), &[("header.accept-language", "fr_FR")]);
    assert_eq!(
        result(&data).keys(),
        [
            "CountryNamesGeographicalTranslated",
            "CountryNamesPopulationTranslated",
            "SortingCultureUsed",
            "CountryCodesGeographicalAll",
            "CountryNamesGeographicalAllTranslated",
            "CountryCodesPopulationAll",
            "CountryNamesPopulationAllTranslated",
        ]
    );
    assert_eq!(
        result(&data).get("SortingCultureUsed").unwrap(),
        PropertyValue::from("fr-FR")
    );
}
