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

//! Translation engines for IP Intelligence's country codes, built on the
//! [`TranslationEngine`](fiftyone_pipeline_translation::TranslationEngine).
//!
//! Add them to a pipeline in this order.
//!
//! 1. The IP Intelligence engine, which gives `CountryCodesGeographical` and
//!    `CountryCodesPopulation`, each a weighted list of country codes.
//! 2. [`CountryCodeTranslationEngine`], which gives the English name of each
//!    country under [`COUNTRY_NAMES_KEY`], read through
//!    [`CountryCodeTranslationData`].
//! 3. [`CountriesTranslationEngine`], which translates those names into the
//!    language the request asks for and lists every country for that
//!    language, under [`COUNTRY_NAMES_TRANSLATED_KEY`], read through
//!    [`CountriesTranslationData`].
//!
//! The country names come from the 51Degrees data repository and are
//! compiled in, in English and 16 other languages, so neither engine needs
//! any configuration or files.
//!
//! ```
//! use fiftyone_ip_intelligence_translation::{
//!     CountriesTranslationEngine, CountryCodeTranslationEngine,
//! };
//!
//! let names = CountryCodeTranslationEngine::builder().build()?;
//! assert_eq!(names.engine().fixed_language(), Some("en_GB"));
//!
//! let countries = CountriesTranslationEngine::builder().build()?;
//! assert_eq!(countries.all_countries().len(), 250);
//!
//! // The engine reads the language from the request's evidence. Here the
//! // same resolution is asked for directly.
//! let (locale, translator) = countries
//!     .engine()
//!     .languages()
//!     .try_get_translator("de-CH,fr;q=0.8,en;q=0.5")?
//!     .expect("French is held");
//! assert_eq!(locale, "fr_FR");
//! assert_eq!(
//!     translator.translate("United Kingdom", &mut Vec::new()).as_deref(),
//!     Some("Royaume-Uni")
//! );
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![warn(missing_docs)]

mod countries;
mod country_code;
mod data;
mod resources;

pub use countries::{CountriesTranslationEngine, CountriesTranslationEngineBuilder};
pub use country_code::{CountryCodeTranslationEngine, CountryCodeTranslationEngineBuilder};
pub use data::{
    CountriesTranslationData, CountryCodeTranslationData, COUNTRY_NAMES_KEY,
    COUNTRY_NAMES_KEY_NAME, COUNTRY_NAMES_TRANSLATED_KEY, COUNTRY_NAMES_TRANSLATED_KEY_NAME,
};
pub use resources::{COUNTRY_CODE_SOURCES, COUNTRY_SOURCES};
