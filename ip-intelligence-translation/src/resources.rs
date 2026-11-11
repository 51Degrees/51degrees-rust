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

//! The translation sources, compiled into the crate so the engines need no
//! files.
//!
//! They are the IP Intelligence translation files of the 51Degrees data
//! repository, under `ip-intelligence/Translations`, read from the folder
//! `build.rs` chooses.

/// The text of the translation file `file`, a path in the translation folder.
macro_rules! translation {
    ($file:literal) => {
        include_str!(concat!(env!("_51DEGREES_TRANSLATIONS_DIR"), "/", $file))
    };
}

/// The country code sources, as pairs of a file name and its text. The one
/// source, `countrycodes.en_GB.yml`, gives each country's English name for
/// its code, and lists every country the engines know.
///
/// A slice rather than an array, so a source added later does not change
/// its type.
pub const COUNTRY_CODE_SOURCES: &[(&str, &str)] = &[(
    "countrycodes.en_GB.yml",
    translation!("countrycodes.en_GB.yml"),
)];

/// The country name sources, as pairs of a file name and its text, in order
/// of name. Each gives the name of every country in one language for its
/// English name.
///
/// A slice rather than an array, so a language added later does not change
/// its type.
pub const COUNTRY_SOURCES: &[(&str, &str)] = &[
    (
        "countries.cs_CZ.yml",
        translation!("OSM/countries.cs_CZ.yml"),
    ),
    (
        "countries.de_DE.yml",
        translation!("OSM/countries.de_DE.yml"),
    ),
    (
        "countries.es_ES.yml",
        translation!("OSM/countries.es_ES.yml"),
    ),
    (
        "countries.fr_FR.yml",
        translation!("OSM/countries.fr_FR.yml"),
    ),
    (
        "countries.hu_HU.yml",
        translation!("OSM/countries.hu_HU.yml"),
    ),
    (
        "countries.it_IT.yml",
        translation!("OSM/countries.it_IT.yml"),
    ),
    (
        "countries.ja_JP.yml",
        translation!("OSM/countries.ja_JP.yml"),
    ),
    (
        "countries.nl_NL.yml",
        translation!("OSM/countries.nl_NL.yml"),
    ),
    (
        "countries.pl_PL.yml",
        translation!("OSM/countries.pl_PL.yml"),
    ),
    (
        "countries.pt_PT.yml",
        translation!("OSM/countries.pt_PT.yml"),
    ),
    (
        "countries.ro_RO.yml",
        translation!("OSM/countries.ro_RO.yml"),
    ),
    (
        "countries.sk_SK.yml",
        translation!("OSM/countries.sk_SK.yml"),
    ),
    (
        "countries.sv_SE.yml",
        translation!("OSM/countries.sv_SE.yml"),
    ),
    (
        "countries.tr_TR.yml",
        translation!("OSM/countries.tr_TR.yml"),
    ),
    (
        "countries.uk_UA.yml",
        translation!("OSM/countries.uk_UA.yml"),
    ),
    (
        "countries.vi_VN.yml",
        translation!("OSM/countries.vi_VN.yml"),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_country_sources_are_in_order_of_name() {
        let names: Vec<&str> = COUNTRY_SOURCES.iter().map(|(name, _)| *name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }
}
