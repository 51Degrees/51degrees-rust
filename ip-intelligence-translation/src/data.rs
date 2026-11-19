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

//! The keys the two engines store their data under, and typed reads of that
//! data.

use fiftyone_pipeline_core::{PropertyMetaData, PropertyValueType, TypedKey, WeightedValue};
use fiftyone_pipeline_engines::AspectPropertyValue;
use fiftyone_pipeline_translation::{TranslationData, TranslationValue};

/// The data key of [`CountryCodeTranslationEngine`](crate::CountryCodeTranslationEngine).
pub const COUNTRY_NAMES_KEY_NAME: &str = "countrynames";

/// The typed key of the English country names.
pub const COUNTRY_NAMES_KEY: TypedKey<TranslationData> = TypedKey::new(COUNTRY_NAMES_KEY_NAME);

/// The data key of [`CountriesTranslationEngine`](crate::CountriesTranslationEngine).
pub const COUNTRY_NAMES_TRANSLATED_KEY_NAME: &str = "countrynamestranslated";

/// The typed key of the translated country names and the lists of every
/// country.
pub const COUNTRY_NAMES_TRANSLATED_KEY: TypedKey<TranslationData> =
    TypedKey::new(COUNTRY_NAMES_TRANSLATED_KEY_NAME);

pub(crate) const COUNTRY_CODES_GEOGRAPHICAL: &str = "CountryCodesGeographical";
pub(crate) const COUNTRY_CODES_POPULATION: &str = "CountryCodesPopulation";
pub(crate) const COUNTRY_NAMES_GEOGRAPHICAL: &str = "CountryNamesGeographical";
pub(crate) const COUNTRY_NAMES_POPULATION: &str = "CountryNamesPopulation";
pub(crate) const COUNTRY_NAMES_GEOGRAPHICAL_TRANSLATED: &str = "CountryNamesGeographicalTranslated";
pub(crate) const COUNTRY_NAMES_POPULATION_TRANSLATED: &str = "CountryNamesPopulationTranslated";
pub(crate) const COUNTRY_NAMES_GEOGRAPHICAL_ALL_TRANSLATED: &str =
    "CountryNamesGeographicalAllTranslated";
pub(crate) const COUNTRY_NAMES_POPULATION_ALL_TRANSLATED: &str =
    "CountryNamesPopulationAllTranslated";
pub(crate) const COUNTRY_CODES_GEOGRAPHICAL_ALL: &str = "CountryCodesGeographicalAll";
pub(crate) const COUNTRY_CODES_POPULATION_ALL: &str = "CountryCodesPopulationAll";
pub(crate) const SORTING_CULTURE_USED: &str = "SortingCultureUsed";

/// The six properties under [`COUNTRY_NAMES_TRANSLATED_KEY`], with their
/// value types. [`CountriesTranslationEngine`](crate::CountriesTranslationEngine)
/// gives these, and
/// [`CloudCountriesTranslationEngine`](crate::CloudCountriesTranslationEngine)
/// lists them until the cloud's own list is known.
pub(crate) fn countries_translated_properties() -> Vec<PropertyMetaData> {
    [
        (
            COUNTRY_NAMES_GEOGRAPHICAL_TRANSLATED,
            PropertyValueType::KeyValueList,
        ),
        (
            COUNTRY_NAMES_POPULATION_TRANSLATED,
            PropertyValueType::KeyValueList,
        ),
        (
            COUNTRY_NAMES_GEOGRAPHICAL_ALL_TRANSLATED,
            PropertyValueType::StringList,
        ),
        (
            COUNTRY_NAMES_POPULATION_ALL_TRANSLATED,
            PropertyValueType::StringList,
        ),
        (
            COUNTRY_CODES_GEOGRAPHICAL_ALL,
            PropertyValueType::StringList,
        ),
        (COUNTRY_CODES_POPULATION_ALL, PropertyValueType::StringList),
    ]
    .into_iter()
    .map(|(name, value_type)| {
        PropertyMetaData::new(name, COUNTRY_NAMES_TRANSLATED_KEY_NAME, value_type)
    })
    .collect()
}

/// The English country names
/// [`CountryCodeTranslationEngine`](crate::CountryCodeTranslationEngine)
/// writes under [`COUNTRY_NAMES_KEY`].
pub trait CountryCodeTranslationData {
    /// The names of the countries in IP Intelligence's
    /// `CountryCodesGeographical`, in the same order and with the same
    /// weightings.
    fn country_names_geographical(&self) -> AspectPropertyValue<Vec<WeightedValue<String>>>;

    /// The names of the countries in IP Intelligence's
    /// `CountryCodesPopulation`, in the same order and with the same
    /// weightings.
    fn country_names_population(&self) -> AspectPropertyValue<Vec<WeightedValue<String>>>;
}

impl CountryCodeTranslationData for TranslationData {
    fn country_names_geographical(&self) -> AspectPropertyValue<Vec<WeightedValue<String>>> {
        weighted_list(self, COUNTRY_NAMES_GEOGRAPHICAL)
    }

    fn country_names_population(&self) -> AspectPropertyValue<Vec<WeightedValue<String>>> {
        weighted_list(self, COUNTRY_NAMES_POPULATION)
    }
}

/// The country names
/// [`CountriesTranslationEngine`](crate::CountriesTranslationEngine) writes
/// under [`COUNTRY_NAMES_TRANSLATED_KEY`], in the language the request asks
/// for.
pub trait CountriesTranslationData {
    /// The geographical country names, translated, in the same order and
    /// with the same weightings.
    fn country_names_geographical_translated(
        &self,
    ) -> AspectPropertyValue<Vec<WeightedValue<String>>>;

    /// The population country names, translated, in the same order and with
    /// the same weightings.
    fn country_names_population_translated(
        &self,
    ) -> AspectPropertyValue<Vec<WeightedValue<String>>>;

    /// The name of every country, translated. The geographical countries
    /// come first, highest weighting first, and the rest follow in the order
    /// the language sorts their names.
    fn country_names_geographical_all_translated(&self) -> AspectPropertyValue<Vec<String>>;

    /// The name of every country, translated. The population countries come
    /// first, highest weighting first, and the rest follow in the order the
    /// language sorts their names.
    fn country_names_population_all_translated(&self) -> AspectPropertyValue<Vec<String>>;

    /// The code of every country, in the same order as
    /// [`CountriesTranslationData::country_names_geographical_all_translated`].
    fn country_codes_geographical_all(&self) -> AspectPropertyValue<Vec<String>>;

    /// The code of every country, in the same order as
    /// [`CountriesTranslationData::country_names_population_all_translated`].
    fn country_codes_population_all(&self) -> AspectPropertyValue<Vec<String>>;

    /// The locale the lists of every country were sorted for, such as
    /// `fr-FR`, or an empty string when the request asked for no language
    /// the engine holds and the names were sorted ignoring case. `None`
    /// before the engine has run, or when the cloud service did not return
    /// it. This is for tests, and not part of the data a consumer should
    /// rely on.
    fn sorting_culture_used(&self) -> Option<&str>;
}

impl CountriesTranslationData for TranslationData {
    fn country_names_geographical_translated(
        &self,
    ) -> AspectPropertyValue<Vec<WeightedValue<String>>> {
        weighted_list(self, COUNTRY_NAMES_GEOGRAPHICAL_TRANSLATED)
    }

    fn country_names_population_translated(
        &self,
    ) -> AspectPropertyValue<Vec<WeightedValue<String>>> {
        weighted_list(self, COUNTRY_NAMES_POPULATION_TRANSLATED)
    }

    fn country_names_geographical_all_translated(&self) -> AspectPropertyValue<Vec<String>> {
        string_list(self, COUNTRY_NAMES_GEOGRAPHICAL_ALL_TRANSLATED)
    }

    fn country_names_population_all_translated(&self) -> AspectPropertyValue<Vec<String>> {
        string_list(self, COUNTRY_NAMES_POPULATION_ALL_TRANSLATED)
    }

    fn country_codes_geographical_all(&self) -> AspectPropertyValue<Vec<String>> {
        string_list(self, COUNTRY_CODES_GEOGRAPHICAL_ALL)
    }

    fn country_codes_population_all(&self) -> AspectPropertyValue<Vec<String>> {
        string_list(self, COUNTRY_CODES_POPULATION_ALL)
    }

    fn sorting_culture_used(&self) -> Option<&str> {
        match self.value(SORTING_CULTURE_USED) {
            Some(AspectPropertyValue::Value(TranslationValue::String(value))) => value.as_deref(),
            _ => None,
        }
    }
}

/// A property held as a weighted list, leaving out any item with no
/// translation.
fn weighted_list(
    data: &TranslationData,
    name: &str,
) -> AspectPropertyValue<Vec<WeightedValue<String>>> {
    match data.value(name) {
        None => AspectPropertyValue::no_value(data.missing_property_message(name)),
        Some(AspectPropertyValue::NoValue { message }) => {
            AspectPropertyValue::no_value(message.clone())
        }
        Some(AspectPropertyValue::Value(TranslationValue::WeightedList(items))) => {
            AspectPropertyValue::new(
                items
                    .iter()
                    .filter_map(|item| {
                        item.value
                            .clone()
                            .map(|value| WeightedValue::new(item.raw_weighting, value))
                    })
                    .collect(),
            )
        }
        Some(AspectPropertyValue::Value(_)) => AspectPropertyValue::no_value(format!(
            "The property '{name}' does not hold a weighted list."
        )),
    }
}

/// A property held as a list of strings, leaving out any item with no
/// translation.
fn string_list(data: &TranslationData, name: &str) -> AspectPropertyValue<Vec<String>> {
    match data.value(name) {
        None => AspectPropertyValue::no_value(data.missing_property_message(name)),
        Some(AspectPropertyValue::NoValue { message }) => {
            AspectPropertyValue::no_value(message.clone())
        }
        Some(AspectPropertyValue::Value(TranslationValue::StringList(items))) => {
            AspectPropertyValue::new(items.iter().flatten().cloned().collect())
        }
        Some(AspectPropertyValue::Value(_)) => {
            AspectPropertyValue::no_value(format!("The property '{name}' does not hold a list."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_form_is_read_and_a_wrong_form_has_no_value() {
        let mut data = TranslationData::new();
        data.set(
            COUNTRY_NAMES_GEOGRAPHICAL,
            AspectPropertyValue::new(TranslationValue::WeightedList(vec![
                WeightedValue::new(40000, Some("France".to_owned())),
                WeightedValue::new(25535, None),
            ])),
        );
        data.set(
            COUNTRY_CODES_GEOGRAPHICAL_ALL,
            AspectPropertyValue::new(TranslationValue::StringList(vec![
                Some("FR".to_owned()),
                None,
            ])),
        );
        data.set(
            COUNTRY_NAMES_POPULATION,
            AspectPropertyValue::no_value("None."),
        );
        data.set(
            SORTING_CULTURE_USED,
            AspectPropertyValue::new(TranslationValue::String(Some("fr-FR".to_owned()))),
        );

        assert_eq!(
            data.country_names_geographical(),
            AspectPropertyValue::new(vec![WeightedValue::new(40000, "France".to_owned())])
        );
        assert_eq!(
            data.country_codes_geographical_all(),
            AspectPropertyValue::new(vec!["FR".to_owned()])
        );
        assert_eq!(
            data.country_names_population().no_value_message(),
            Some("None.")
        );
        assert_eq!(
            data.country_codes_population_all().no_value_message(),
            Some("No value for property 'CountryCodesPopulationAll'.")
        );
        assert!(!data.country_names_geographical_translated().has_value());
        assert_eq!(data.sorting_culture_used(), Some("fr-FR"));

        // A list read as a weighted list, and the reverse, has no value.
        let mut wrong = TranslationData::new();
        wrong.set(
            COUNTRY_NAMES_GEOGRAPHICAL,
            AspectPropertyValue::new(TranslationValue::StringList(Vec::new())),
        );
        wrong.set(
            COUNTRY_CODES_GEOGRAPHICAL_ALL,
            AspectPropertyValue::new(TranslationValue::WeightedList(Vec::new())),
        );
        wrong.set(
            SORTING_CULTURE_USED,
            AspectPropertyValue::no_value("Not sorted."),
        );
        assert!(!wrong.country_names_geographical().has_value());
        assert!(!wrong.country_codes_geographical_all().has_value());
        assert_eq!(wrong.sorting_culture_used(), None);
    }
}
