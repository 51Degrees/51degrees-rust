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

//! The engine that gives the English name of each country code.

use std::sync::Arc;

use fiftyone_ip_intelligence_shared::IP_DATA_KEY_NAME;
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, FlowData, FlowElement, Pipeline, PropertyMetaData, PropertyValueType, Result,
};
use fiftyone_pipeline_translation::{MissingTranslationBehavior, TranslationEngine};

use crate::data::{
    COUNTRY_CODES_GEOGRAPHICAL, COUNTRY_CODES_POPULATION, COUNTRY_NAMES_GEOGRAPHICAL,
    COUNTRY_NAMES_KEY_NAME, COUNTRY_NAMES_POPULATION,
};
use crate::resources::COUNTRY_CODE_SOURCES;

/// The language the country names are always given in.
const ENGLISH: &str = "en_GB";

/// Gives the English name of each country in IP Intelligence's
/// `CountryCodesGeographical` and `CountryCodesPopulation`, as
/// `CountryNamesGeographical` and `CountryNamesPopulation`, keeping their
/// order and weightings.
///
/// Add it after the IP Intelligence engine and read the names under
/// [`COUNTRY_NAMES_KEY`](crate::COUNTRY_NAMES_KEY) through
/// [`CountryCodeTranslationData`](crate::CountryCodeTranslationData). A code
/// with no name, such as `Unknown`, is given as it is. The names are compiled
/// in, so the engine needs no configuration.
pub struct CountryCodeTranslationEngine {
    engine: TranslationEngine,
}

impl CountryCodeTranslationEngine {
    /// A builder for the engine.
    pub fn builder() -> CountryCodeTranslationEngineBuilder {
        CountryCodeTranslationEngineBuilder::new()
    }

    /// The translation engine this engine is made from.
    pub fn engine(&self) -> &TranslationEngine {
        &self.engine
    }
}

impl FlowElement for CountryCodeTranslationEngine {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        self.engine.process(data)
    }

    fn data_key(&self) -> &str {
        self.engine.data_key()
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        self.engine.evidence_key_filter()
    }

    fn properties(&self) -> &[PropertyMetaData] {
        self.engine.properties()
    }

    fn properties_loaded(&self) -> bool {
        self.engine.properties_loaded()
    }

    fn add_pipeline(&self, pipeline: &Arc<Pipeline>) -> Result<()> {
        self.engine.add_pipeline(pipeline)
    }
}

/// Builds a [`CountryCodeTranslationEngine`]. There is nothing to configure.
#[derive(Debug, Default)]
pub struct CountryCodeTranslationEngineBuilder {}

impl CountryCodeTranslationEngineBuilder {
    /// A builder.
    pub fn new() -> Self {
        CountryCodeTranslationEngineBuilder {}
    }

    /// Build the engine.
    pub fn build(self) -> Result<CountryCodeTranslationEngine> {
        let builder = TranslationEngine::builder()
            .set_element_data_key(COUNTRY_NAMES_KEY_NAME)
            .set_source_element_data_key(IP_DATA_KEY_NAME)
            .set_fixed_language(ENGLISH)
            .set_missing_translation_behavior(MissingTranslationBehavior::Original)
            .add_translation_of_type(
                COUNTRY_CODES_GEOGRAPHICAL,
                COUNTRY_NAMES_GEOGRAPHICAL,
                PropertyValueType::KeyValueList,
            )
            .add_translation_of_type(
                COUNTRY_CODES_POPULATION,
                COUNTRY_NAMES_POPULATION,
                PropertyValueType::KeyValueList,
            );
        let builder = COUNTRY_CODE_SOURCES
            .iter()
            .fold(builder, |builder, (name, text)| {
                builder.add_source_content(*name, *text)
            });
        Ok(CountryCodeTranslationEngine {
            engine: builder.build()?,
        })
    }
}
