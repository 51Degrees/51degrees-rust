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

//! The engine that translates the country names and orders every country.

use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use fiftyone_ip_intelligence_shared::IP_DATA_KEY_NAME;
use fiftyone_pipeline_core::{
    Error, Evidence, EvidenceKeyFilter, FlowData, FlowElement, Pipeline, PropertyMetaData,
    PropertyValue, PropertyValueType, Result,
};
use fiftyone_pipeline_engines::AspectPropertyValue;
use fiftyone_pipeline_translation::{
    MissingTranslationBehavior, TranslationData, TranslationEngine, TranslationValue, Translator,
    LANGUAGE_EVIDENCE_KEYS, VALUE_KEY,
};
use icu_collator::options::{CollatorOptions, Strength};
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
use icu_locale_core::Locale;
use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};

use crate::data::{
    countries_translated_properties, COUNTRY_CODES_GEOGRAPHICAL, COUNTRY_CODES_GEOGRAPHICAL_ALL,
    COUNTRY_CODES_POPULATION, COUNTRY_CODES_POPULATION_ALL, COUNTRY_NAMES_GEOGRAPHICAL,
    COUNTRY_NAMES_GEOGRAPHICAL_ALL_TRANSLATED, COUNTRY_NAMES_GEOGRAPHICAL_TRANSLATED,
    COUNTRY_NAMES_KEY_NAME, COUNTRY_NAMES_POPULATION, COUNTRY_NAMES_POPULATION_ALL_TRANSLATED,
    COUNTRY_NAMES_POPULATION_TRANSLATED, COUNTRY_NAMES_TRANSLATED_KEY_NAME, SORTING_CULTURE_USED,
};
use crate::resources::{COUNTRY_CODE_SOURCES, COUNTRY_SOURCES};

/// Translates the English country names
/// [`CountryCodeTranslationEngine`](crate::CountryCodeTranslationEngine)
/// gives into the language the request asks for, and lists every country
/// for that language.
///
/// Add it after the country code engine and read the results under
/// [`COUNTRY_NAMES_TRANSLATED_KEY`](crate::COUNTRY_NAMES_TRANSLATED_KEY)
/// through [`CountriesTranslationData`](crate::CountriesTranslationData).
///
/// `CountryNamesGeographical` and `CountryNamesPopulation` are translated to
/// `CountryNamesGeographicalTranslated` and
/// `CountryNamesPopulationTranslated` as any translation engine translates,
/// reading the language from the first language evidence that is not blank.
///
/// Each list of every country starts with the countries IP Intelligence
/// gives, highest weighting first and then by name, leaving out any code
/// that is not a country, such as `Unknown`. Every other country follows,
/// sorted by name. For these lists each language evidence is tried in turn
/// until one names a language the engine holds. The names are then in that
/// language and sorted as it sorts them. When none does, the names are in
/// English and sorted ignoring case.
///
/// Each language's order comes from the Unicode CLDR collation data in the
/// version of `icu_collator` the build resolves, so a newer version can
/// order a few names differently.
pub struct CountriesTranslationEngine {
    engine: TranslationEngine,
    /// Every country's code and English name, in the order of the country
    /// code source.
    all_countries: Vec<(String, String)>,
    /// The code of every country, in capitals.
    known_codes: HashSet<String>,
    /// The collator and sorted countries for each locale the engine holds.
    locales: Vec<(String, SortedCountries)>,
    /// The English names, sorted ignoring case, for when no locale is found.
    english: SortedCountries,
    properties: Vec<PropertyMetaData>,
}

/// A collator, and every country sorted by name with it.
struct SortedCountries {
    collator: CollatorBorrowed<'static>,
    /// Every country's code and name, in the collator's language, sorted by
    /// name. The sort is stable, so taking some countries out leaves the
    /// rest in the order sorting them alone would give.
    countries: Vec<(String, String)>,
}

impl SortedCountries {
    fn new(
        all_countries: &[(String, String)],
        translator: Option<&Translator>,
        collator: CollatorBorrowed<'static>,
    ) -> Self {
        let mut countries: Vec<(String, String)> = all_countries
            .iter()
            .map(|(code, name)| {
                let name = translator
                    .and_then(|translator| translator.translate(name, &mut Vec::new()))
                    .unwrap_or_else(|| name.clone());
                (code.clone(), name)
            })
            .collect();
        countries.sort_by(|a, b| collator.compare(&a.1, &b.1));
        SortedCountries {
            collator,
            countries,
        }
    }
}

impl CountriesTranslationEngine {
    /// A builder for the engine.
    pub fn builder() -> CountriesTranslationEngineBuilder {
        CountriesTranslationEngineBuilder::new()
    }

    /// The translation engine that translates the weighted country names.
    pub fn engine(&self) -> &TranslationEngine {
        &self.engine
    }

    /// Every country's code and English name, in the order of the country
    /// code source.
    pub fn all_countries(&self) -> &[(String, String)] {
        &self.all_countries
    }

    /// The translator and locale for the lists of every country, from the
    /// first language evidence that names a language the engine holds.
    fn resolve(&self, evidence: &Evidence) -> Result<Option<(&str, &Translator)>> {
        for key in LANGUAGE_EVIDENCE_KEYS {
            let Some(value) = evidence.get(key).filter(|value| !value.trim().is_empty()) else {
                continue;
            };
            let found = self
                .engine
                .languages()
                .try_get_translator(value)
                .map_err(|error| Error::Pipeline {
                    message: error.to_string(),
                })?;
            if found.is_some() {
                return Ok(found);
            }
        }
        Ok(None)
    }

    fn sorted(&self, locale: &str) -> &SortedCountries {
        self.locales
            .iter()
            .find(|(held, _)| held == locale)
            .map_or(&self.english, |(_, sorted)| sorted)
    }

    /// The codes and names of every country, led by the weighted countries.
    fn all_countries_for(
        &self,
        names: &[(u16, String)],
        codes: &[String],
        sorted: &SortedCountries,
    ) -> (Vec<String>, Vec<String>) {
        // Each name is paired with the code at the same place, as far as the
        // shorter list goes. The sort is stable, so countries that compare
        // equal keep the order they had.
        let mut weighted: Vec<(u16, &str, &str)> = names
            .iter()
            .zip(codes)
            .map(|((weighting, name), code)| (*weighting, name.as_str(), code.as_str()))
            .collect();
        weighted.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| sorted.collator.compare(a.1, b.1))
        });
        weighted.retain(|(_, _, code)| self.known_codes.contains(&code.to_ascii_uppercase()));

        let rest = sorted
            .countries
            .iter()
            .filter(|(code, _)| !weighted.iter().any(|&(_, _, held)| held == code.as_str()));
        weighted
            .iter()
            .map(|&(_, name, code)| (code.to_owned(), name.to_owned()))
            .chain(rest.cloned())
            .unzip()
    }
}

impl fmt::Debug for CountriesTranslationEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CountriesTranslationEngine")
            .field("data_key", &self.engine.data_key())
            .field("countries", &self.all_countries.len())
            .finish_non_exhaustive()
    }
}

impl FlowElement for CountriesTranslationEngine {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        self.engine.process(data)?;

        let (sorted, culture) = match self.resolve(data.evidence())? {
            Some((locale, _)) => (self.sorted(locale), locale.replace('_', "-")),
            None => (&self.english, String::new()),
        };

        let own = data.get(self.engine.typed_key());
        let geographical = self.all_countries_for(
            &weighted_names(own, COUNTRY_NAMES_GEOGRAPHICAL_TRANSLATED),
            &weighted_codes(data, COUNTRY_CODES_GEOGRAPHICAL),
            sorted,
        );
        let population = self.all_countries_for(
            &weighted_names(own, COUNTRY_NAMES_POPULATION_TRANSLATED),
            &weighted_codes(data, COUNTRY_CODES_POPULATION),
            sorted,
        );

        let output = data.get_or_add(self.engine.typed_key(), TranslationData::new)?;
        output.set(
            SORTING_CULTURE_USED,
            AspectPropertyValue::new(TranslationValue::String(Some(culture))),
        );
        for ((codes, names), codes_name, names_name) in [
            (
                geographical,
                COUNTRY_CODES_GEOGRAPHICAL_ALL,
                COUNTRY_NAMES_GEOGRAPHICAL_ALL_TRANSLATED,
            ),
            (
                population,
                COUNTRY_CODES_POPULATION_ALL,
                COUNTRY_NAMES_POPULATION_ALL_TRANSLATED,
            ),
        ] {
            output.set(codes_name, AspectPropertyValue::new(codes.into()));
            output.set(names_name, AspectPropertyValue::new(names.into()));
        }
        Ok(())
    }

    fn data_key(&self) -> &str {
        self.engine.data_key()
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.engine.evidence_key_filter()
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }

    fn properties_loaded(&self) -> bool {
        self.engine.properties_loaded()
    }

    fn add_pipeline(&self, pipeline: &Arc<Pipeline>) -> Result<()> {
        self.engine.add_pipeline(pipeline)
    }
}

/// The names and weightings of a weighted list the engine wrote, or none
/// when it holds no weighted list under that name.
fn weighted_names(own: Option<&TranslationData>, name: &str) -> Vec<(u16, String)> {
    match own.and_then(|data| data.value(name)) {
        Some(AspectPropertyValue::Value(TranslationValue::WeightedList(items))) => items
            .iter()
            .map(|item| (item.raw_weighting, item.value.clone().unwrap_or_default()))
            .collect(),
        _ => Vec::new(),
    }
}

/// The values of a weighted list in the IP Intelligence data, or none when
/// there is no such data, the property has no value, or it is not a
/// weighted list.
fn weighted_codes(data: &FlowData, name: &str) -> Vec<String> {
    let Some(ip) = data.get_str(IP_DATA_KEY_NAME) else {
        return Vec::new();
    };
    match ip.get(name) {
        Ok(PropertyValue::KeyValueList(records)) => records
            .iter()
            .map(|record| match record.get(VALUE_KEY) {
                Some(PropertyValue::String(code)) => Some(code.clone()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Builds a [`CountriesTranslationEngine`]. There is nothing to configure.
#[derive(Debug, Default)]
pub struct CountriesTranslationEngineBuilder {}

impl CountriesTranslationEngineBuilder {
    /// A builder.
    pub fn new() -> Self {
        CountriesTranslationEngineBuilder {}
    }

    /// Build the engine.
    pub fn build(self) -> Result<CountriesTranslationEngine> {
        let builder = TranslationEngine::builder()
            .set_element_data_key(COUNTRY_NAMES_TRANSLATED_KEY_NAME)
            .set_source_element_data_key(COUNTRY_NAMES_KEY_NAME)
            .set_missing_translation_behavior(MissingTranslationBehavior::Original)
            .add_translation_of_type(
                COUNTRY_NAMES_GEOGRAPHICAL,
                COUNTRY_NAMES_GEOGRAPHICAL_TRANSLATED,
                PropertyValueType::KeyValueList,
            )
            .add_translation_of_type(
                COUNTRY_NAMES_POPULATION,
                COUNTRY_NAMES_POPULATION_TRANSLATED,
                PropertyValueType::KeyValueList,
            );
        let engine = COUNTRY_SOURCES
            .iter()
            .fold(builder, |builder, (name, text)| {
                builder.add_source_content(*name, *text)
            })
            .build()?;

        let (name, text) = COUNTRY_CODE_SOURCES[0];
        let all_countries = read_countries(name, text)?;
        let known_codes = all_countries
            .iter()
            .map(|(code, _)| code.to_ascii_uppercase())
            .collect();

        // Which countries lead a list changes with each request, but the
        // order of the rest depends only on the locale, so each locale's
        // order is found once here.
        let languages = engine.languages();
        let mut locales = Vec::new();
        for locale in languages.locales() {
            let sorted = SortedCountries::new(
                &all_countries,
                languages.translator(locale),
                locale_collator(locale)?,
            );
            locales.push((locale.to_owned(), sorted));
        }
        let mut options = CollatorOptions::default();
        options.strength = Some(Strength::Secondary);
        let ignoring_case = Collator::try_new(CollatorPreferences::default(), options)
            .map_err(|error| collator_error("the root locale", error))?;
        let english = SortedCountries::new(&all_countries, None, ignoring_case);

        let properties = countries_translated_properties();

        Ok(CountriesTranslationEngine {
            engine,
            all_countries,
            known_codes,
            locales,
            english,
            properties,
        })
    }
}

/// The collator for a locale such as `fr_FR`, at the default strength,
/// which tells apart case and accents.
fn locale_collator(locale: &str) -> Result<CollatorBorrowed<'static>> {
    let tag = locale.replace('_', "-");
    let parsed = Locale::try_from_str(&tag).map_err(|error| {
        Error::configuration(format!("The locale '{tag}' could not be read: {error}"))
    })?;
    Collator::try_new((&parsed).into(), CollatorOptions::default())
        .map_err(|error| collator_error(&format!("the locale '{tag}'"), error))
}

fn collator_error(what: &str, error: impl fmt::Display) -> Error {
    Error::configuration(format!("No collator could be made for {what}: {error}"))
}

/// Read the country code source as pairs of a code and its English name, in
/// the order of the source.
fn read_countries(name: &str, text: &str) -> Result<Vec<(String, String)>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let pairs = match serde_norway::from_str::<Option<Pairs>>(text) {
        Ok(Some(Pairs(pairs))) => pairs,
        Ok(None) => Vec::new(),
        Err(error) => {
            return Err(Error::configuration(format!(
                "The source for {name} could not be parsed into a list of \
                 countries: {error}"
            )))
        }
    };
    let mut seen = HashSet::new();
    let mut countries = Vec::with_capacity(pairs.len());
    for (code, country) in pairs {
        if !seen.insert(code.clone()) {
            return Err(Error::configuration(format!(
                "The source for {name} gives the country '{code}' twice."
            )));
        }
        countries.push((code, country.unwrap_or_default()));
    }
    Ok(countries)
}

/// A YAML mapping read as its entries in order.
struct Pairs(Vec<(String, Option<String>)>);

impl<'de> Deserialize<'de> for Pairs {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct PairsVisitor;

        impl<'de> Visitor<'de> for PairsVisitor {
            type Value = Pairs;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a mapping of country codes to names")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Pairs, A::Error> {
                let mut pairs = Vec::new();
                while let Some(entry) = map.next_entry::<String, Option<String>>()? {
                    pairs.push(entry);
                }
                Ok(Pairs(pairs))
            }
        }

        deserializer.deserialize_map(PairsVisitor)
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::*;

    #[test]
    fn the_countries_are_read_in_the_order_of_the_source() {
        let countries =
            read_countries("codes.en_GB.yml", "\u{feff}NO: Norway\nAD: Andorra\nON:\n").unwrap();
        assert_eq!(
            countries,
            [
                ("NO".to_owned(), "Norway".to_owned()),
                ("AD".to_owned(), "Andorra".to_owned()),
                ("ON".to_owned(), String::new()),
            ]
        );
        assert!(read_countries("codes.en_GB.yml", "").unwrap().is_empty());
        for text in ["NO: Norway\nNO: Norge\n", "- NO\n- AD\n"] {
            assert!(read_countries("codes.en_GB.yml", text).is_err(), "{text}");
        }
    }

    #[test]
    fn the_shipped_country_list_has_every_country_once() {
        let (name, text) = COUNTRY_CODE_SOURCES[0];
        let countries = read_countries(name, text).unwrap();
        assert_eq!(countries.len(), 250);
        assert_eq!(
            countries.first(),
            Some(&("AD".to_owned(), "Andorra".to_owned()))
        );
        assert!(countries
            .iter()
            .all(|(code, country)| code.len() == 2 && !country.is_empty()));
    }

    #[test]
    fn a_locale_collator_tells_case_apart_and_the_other_does_not() {
        let french = locale_collator("fr_FR").unwrap();
        assert_eq!(french.compare("a", "A"), Ordering::Less);
        assert_eq!(french.compare("Égypte", "Equateur"), Ordering::Less);
        let mut options = CollatorOptions::default();
        options.strength = Some(Strength::Secondary);
        let ignoring_case = Collator::try_new(CollatorPreferences::default(), options).unwrap();
        assert_eq!(ignoring_case.compare("a", "A"), Ordering::Equal);
        assert_eq!(ignoring_case.compare("a", "á"), Ordering::Less);
    }
}
