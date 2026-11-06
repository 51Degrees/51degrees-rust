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

//! The translators an engine holds, and how the language a request asks for
//! is matched to one of them.

use std::fmt;

use crate::translator::Translator;

/// The language the source values are already in. A request that prefers it
/// gets no translation, even when it would also accept a language the engine
/// has.
pub const BASE_LANGUAGE: &str = "en";

/// An item of an `Accept-Language` value that could not be read, which fails
/// the whole value.
///
/// Each item between commas is a token, optionally followed by `;` and a
/// quality `q=` from 0 to 1, with spaces and tabs allowed around each part. An
/// empty item, such as the one a trailing comma leaves, is refused too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidAcceptLanguage {
    item: String,
}

impl InvalidAcceptLanguage {
    /// The item that could not be read, as it was given.
    pub fn item(&self) -> &str {
        &self.item
    }
}

impl fmt::Display for InvalidAcceptLanguage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "The format of value '{}' is invalid.", self.item)
    }
}

impl std::error::Error for InvalidAcceptLanguage {}

/// The translators an engine holds, one for each locale, such as `fr_FR`, in
/// the order their sources were added.
#[derive(Debug, Clone, Default)]
pub struct Languages {
    translators: Vec<(String, Translator)>,
}

impl Languages {
    /// An empty set of translators.
    pub fn new() -> Self {
        Languages::default()
    }

    /// Add the translator for a locale. A translator already held for the
    /// same locale, ignoring case, is replaced and keeps its place.
    pub(crate) fn add_language(&mut self, locale: impl Into<String>, translator: Translator) {
        let locale = locale.into();
        match self
            .translators
            .iter_mut()
            .find(|(held, _)| held.eq_ignore_ascii_case(&locale))
        {
            Some((_, held)) => *held = translator,
            None => self.translators.push((locale, translator)),
        }
    }

    /// The locales held, in the order their sources were added.
    pub fn locales(&self) -> impl Iterator<Item = &str> {
        self.translators.iter().map(|(locale, _)| locale.as_str())
    }

    /// The translator for a locale held, matched ignoring case.
    pub fn translator(&self, locale: &str) -> Option<&Translator> {
        self.translators
            .iter()
            .find(|(held, _)| held.eq_ignore_ascii_case(locale))
            .map(|(_, translator)| translator)
    }

    /// The translator for the language asked for, with the locale it was
    /// found under, or `None` when no locale held matches.
    ///
    /// `language` is a locale such as `fr_FR` or `fr-FR`, a language such as
    /// `fr`, or a whole `Accept-Language` value such as
    /// `es,de-DE;q=0.8,en;q=0.5`. It is resolved as
    /// [`Languages::try_resolve_locale`] says, with [`BASE_LANGUAGE`].
    pub fn try_get_translator(
        &self,
        language: &str,
    ) -> Result<Option<(&str, &Translator)>, InvalidAcceptLanguage> {
        let resolved = Self::try_resolve_locale(language, self.locales(), Some(BASE_LANGUAGE))?;
        Ok(resolved.and_then(|locale| {
            self.translators
                .iter()
                .find(|(held, _)| held.as_str() == locale)
                .map(|(held, translator)| (held.as_str(), translator))
        }))
    }

    /// The languages an `Accept-Language` value asks for, most preferred
    /// first, with each `-` written as `_`, so `en-GB` becomes `en_GB`.
    ///
    /// Items are ordered by their quality, highest first, and an item with no
    /// quality counts as 1. Items of equal quality keep their order. A value
    /// that is empty or only white space asks for nothing.
    pub fn parse_accept_language(value: &str) -> Result<Vec<String>, InvalidAcceptLanguage> {
        if value.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut items = value
            .split(',')
            .map(|item| {
                parse_item(item).ok_or_else(|| InvalidAcceptLanguage {
                    item: item.to_owned(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        // A stable sort, so items of equal quality keep their order.
        items.sort_by(|(_, a), (_, b)| b.total_cmp(a));
        Ok(items
            .into_iter()
            .map(|(language, _)| language.replace('-', "_"))
            .collect())
    }

    /// The locale of `available` that best matches an `Accept-Language`
    /// value, or `None` when none does.
    ///
    /// Each language asked for is tried in order of preference.
    ///
    /// 1. A locale equal to it, ignoring case, matches.
    /// 2. Otherwise, if it starts with `base_language`, ignoring case,
    ///    resolution stops with no match, because the source values are
    ///    already in that language. A less preferred language is not tried.
    /// 3. Otherwise a language of two letters, such as `fr`, matches the
    ///    first locale that starts with it, such as `fr_FR`.
    pub fn try_resolve_locale<'a, I>(
        accept_language: &str,
        available: I,
        base_language: Option<&str>,
    ) -> Result<Option<&'a str>, InvalidAcceptLanguage>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let available: Vec<&'a str> = available.into_iter().collect();
        for candidate in Self::parse_accept_language(accept_language)? {
            if let Some(locale) = available
                .iter()
                .copied()
                .find(|locale| locale.eq_ignore_ascii_case(&candidate))
            {
                return Ok(Some(locale));
            }
            if base_language.is_some_and(|base| starts_with_ignore_case(&candidate, base)) {
                return Ok(None);
            }
            if candidate.len() == 2 {
                if let Some(locale) = available
                    .iter()
                    .copied()
                    .find(|locale| starts_with_ignore_case(locale, &candidate))
                {
                    return Ok(Some(locale));
                }
            }
        }
        Ok(None)
    }
}

/// True if `text` starts with `prefix`, ignoring case. Languages and locales
/// are made of token characters, which are all ASCII.
fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len()
        && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// The characters a token may hold, as HTTP defines a token in
/// [RFC 9110 section 5.6.2](https://www.rfc-editor.org/rfc/rfc9110#section-5.6.2).
fn is_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

/// The index of the first byte from `index` that is not a space or a tab.
fn skip_white_space(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && (bytes[index] == b' ' || bytes[index] == b'\t') {
        index += 1;
    }
    index
}

/// Read one item between commas as a language and its quality, the quality
/// being 1 when the item gives none. `None` when the item cannot be read.
///
/// A quality is digits with at most one `.`, not starting with the `.`, so
/// `1.` is read and `.5` is not, and it must be from 0 to 1.
fn parse_item(item: &str) -> Option<(&str, f64)> {
    let bytes = item.as_bytes();
    let start = skip_white_space(bytes, 0);
    let mut index = start;
    while index < bytes.len() && is_token(bytes[index]) {
        index += 1;
    }
    if index == start {
        return None;
    }
    let language = &item[start..index];
    index = skip_white_space(bytes, index);
    let mut quality = 1.0;
    if index < bytes.len() && bytes[index] == b';' {
        index = skip_white_space(bytes, index + 1);
        if index == bytes.len() || !bytes[index].eq_ignore_ascii_case(&b'q') {
            return None;
        }
        index = skip_white_space(bytes, index + 1);
        if index == bytes.len() || bytes[index] != b'=' {
            return None;
        }
        index = skip_white_space(bytes, index + 1);
        let number_start = index;
        if index < bytes.len() && bytes[index] != b'.' {
            let mut dot = false;
            while index < bytes.len()
                && (bytes[index].is_ascii_digit() || (!dot && bytes[index] == b'.'))
            {
                dot |= bytes[index] == b'.';
                index += 1;
            }
        }
        if index == number_start {
            return None;
        }
        quality = item[number_start..index].parse::<f64>().ok()?;
        if !(0.0..=1.0).contains(&quality) {
            return None;
        }
        index = skip_white_space(bytes, index);
    }
    (index == bytes.len()).then_some((language, quality))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::MissingTranslationBehavior;

    fn parse(value: &str) -> Result<Vec<String>, InvalidAcceptLanguage> {
        Languages::parse_accept_language(value)
    }

    fn resolve<'a>(value: &str, locales: &[&'a str]) -> Option<&'a str> {
        Languages::try_resolve_locale(value, locales.iter().copied(), Some(BASE_LANGUAGE)).unwrap()
    }

    #[test]
    fn items_are_ordered_by_quality_keeping_the_order_of_equals() {
        assert_eq!(
            parse("es,de-DE;q=0.8,en;q=0.5").unwrap(),
            ["es", "de_DE", "en"]
        );
        assert_eq!(
            parse("en;q=0.5, fr ; Q = 0.9 ,de,it;q=1.").unwrap(),
            ["de", "it", "fr", "en"]
        );
        assert_eq!(parse("a;q=0.5,b;q=0.5,c").unwrap(), ["c", "a", "b"]);
        assert_eq!(parse("fr_FR").unwrap(), ["fr_FR"]);
        assert_eq!(parse("*;q=0").unwrap(), ["*"]);
    }

    #[test]
    fn a_value_of_only_white_space_asks_for_nothing() {
        assert!(parse("").unwrap().is_empty());
        assert!(parse(" \t").unwrap().is_empty());
        assert!(parse("\u{3000}").unwrap().is_empty());
    }

    #[test]
    fn an_item_that_cannot_be_read_fails_the_value() {
        for (value, item) in [
            ("fr,", ""),
            ("fr,,de", ""),
            ("fr, ", " "),
            ("fr-FR fr", "fr-FR fr"),
            ("fr;", "fr;"),
            ("fr;q", "fr;q"),
            ("fr;q=", "fr;q="),
            ("fr;q=.5", "fr;q=.5"),
            ("fr;q=1.5", "fr;q=1.5"),
            ("fr;q=0.5.1", "fr;q=0.5.1"),
            ("fr;level=1", "fr;level=1"),
            ("fr;q=-1", "fr;q=-1"),
            ("fran\u{e7}ais", "fran\u{e7}ais"),
            ("fr,\u{3000}", "\u{3000}"),
        ] {
            let error = parse(value).unwrap_err();
            assert_eq!(error.item(), item, "{value}");
            assert_eq!(
                error.to_string(),
                format!("The format of value '{item}' is invalid.")
            );
        }
    }

    #[test]
    fn a_quality_may_end_with_its_point_and_have_many_digits() {
        assert_eq!(parse("fr;q=1.,de;q=0.0001").unwrap(), ["fr", "de"]);
        assert_eq!(parse("fr;q=00000000000000000001").unwrap(), ["fr"]);
        assert_eq!(parse("fr;q=1.0000000000000000001").unwrap(), ["fr"]);
    }

    #[test]
    fn an_exact_locale_matches_ignoring_case() {
        let locales = ["fr_FR", "de_DE"];
        assert_eq!(resolve("de_DE", &locales), Some("de_DE"));
        assert_eq!(resolve("DE-de", &locales), Some("de_DE"));
        assert_eq!(resolve("ja_JP", &locales), None);
    }

    #[test]
    fn a_two_letter_language_matches_the_first_locale_it_starts() {
        let locales = ["pt_BR", "fr_FR", "pt_PT"];
        assert_eq!(resolve("pt", &locales), Some("pt_BR"));
        assert_eq!(resolve("FR", &locales), Some("fr_FR"));
        // Three letters are not a language to fall back on.
        assert_eq!(resolve("fra", &locales), None);
    }

    #[test]
    fn the_base_language_preferred_stops_resolution() {
        let locales = ["fr_FR", "de_DE"];
        assert_eq!(resolve("en-US,en;q=0.9,fr;q=0.5", &locales), None);
        assert_eq!(resolve("EN,fr", &locales), None);
        assert_eq!(resolve("fr,en;q=0.5", &locales), Some("fr_FR"));
        // An unknown language before English does not stop it.
        assert_eq!(resolve("xx,de", &locales), Some("de_DE"));
        // With no base language, English falls through.
        assert_eq!(
            Languages::try_resolve_locale("en,fr", locales.iter().copied(), None).unwrap(),
            Some("fr_FR")
        );
        // A locale held for English matches before the stop.
        assert_eq!(resolve("en-GB,fr", &["en_GB", "fr_FR"]), Some("en_GB"));
    }

    #[test]
    fn a_higher_preference_wins_over_an_exact_locale() {
        let locales = ["fr_FR", "de_DE"];
        assert_eq!(resolve("fr,de-DE;q=0.5", &locales), Some("fr_FR"));
        assert_eq!(resolve("fr;q=0.4,de-DE;q=0.5", &locales), Some("de_DE"));
    }

    #[test]
    fn a_translator_added_again_replaces_the_first_in_its_place() {
        let mut languages = Languages::new();
        let behavior = MissingTranslationBehavior::Original;
        languages.add_language("fr_FR", Translator::new(behavior));
        languages.add_language("de_DE", Translator::new(behavior));
        let chat = Translator::with_translations([("cat", "chat")], behavior).unwrap();
        languages.add_language("fr_FR", chat);
        assert_eq!(languages.locales().collect::<Vec<_>>(), ["fr_FR", "de_DE"]);
        let (locale, translator) = languages.try_get_translator("fr").unwrap().unwrap();
        assert_eq!(locale, "fr_FR");
        assert_eq!(
            translator.translate("cat", &mut Vec::new()),
            Some("chat".to_owned())
        );
    }
}
