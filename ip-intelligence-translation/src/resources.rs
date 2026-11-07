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
//! They are copies of the IP Intelligence translation files in the
//! 51Degrees data repository, under `ip-intelligence/Translations`. The
//! crate's tests check each copy against that folder byte for byte.

/// The country code sources, as pairs of a file name and its text. The one
/// source, `countrycodes.en_GB.yml`, gives each country's English name for
/// its code, and lists every country the engines know.
///
/// A slice rather than an array, so a source added later does not change
/// its type.
pub const COUNTRY_CODE_SOURCES: &[(&str, &str)] = &[(
    "countrycodes.en_GB.yml",
    include_str!("../translations/countrycodes.en_GB.yml"),
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
        include_str!("../translations/OSM/countries.cs_CZ.yml"),
    ),
    (
        "countries.de_DE.yml",
        include_str!("../translations/OSM/countries.de_DE.yml"),
    ),
    (
        "countries.es_ES.yml",
        include_str!("../translations/OSM/countries.es_ES.yml"),
    ),
    (
        "countries.fr_FR.yml",
        include_str!("../translations/OSM/countries.fr_FR.yml"),
    ),
    (
        "countries.hu_HU.yml",
        include_str!("../translations/OSM/countries.hu_HU.yml"),
    ),
    (
        "countries.it_IT.yml",
        include_str!("../translations/OSM/countries.it_IT.yml"),
    ),
    (
        "countries.ja_JP.yml",
        include_str!("../translations/OSM/countries.ja_JP.yml"),
    ),
    (
        "countries.nl_NL.yml",
        include_str!("../translations/OSM/countries.nl_NL.yml"),
    ),
    (
        "countries.pl_PL.yml",
        include_str!("../translations/OSM/countries.pl_PL.yml"),
    ),
    (
        "countries.pt_PT.yml",
        include_str!("../translations/OSM/countries.pt_PT.yml"),
    ),
    (
        "countries.ro_RO.yml",
        include_str!("../translations/OSM/countries.ro_RO.yml"),
    ),
    (
        "countries.sk_SK.yml",
        include_str!("../translations/OSM/countries.sk_SK.yml"),
    ),
    (
        "countries.sv_SE.yml",
        include_str!("../translations/OSM/countries.sv_SE.yml"),
    ),
    (
        "countries.tr_TR.yml",
        include_str!("../translations/OSM/countries.tr_TR.yml"),
    ),
    (
        "countries.uk_UA.yml",
        include_str!("../translations/OSM/countries.uk_UA.yml"),
    ),
    (
        "countries.vi_VN.yml",
        include_str!("../translations/OSM/countries.vi_VN.yml"),
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
