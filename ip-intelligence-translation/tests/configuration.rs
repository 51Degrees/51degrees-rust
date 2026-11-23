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

//! The country translation engines made from a configuration, compared with
//! what .NET's FiftyOne.IpIntelligence.Translation 4.5.155 made from the
//! same configuration with `FiftyOnePipelineBuilder`, each builder given by
//! the service provider. .NET's pipeline also held its Sequence element,
//! which these tests' registry does not add.

use std::sync::Arc;

use fiftyone_ip_intelligence_translation::configuration;
use fiftyone_pipeline_core::configuration::{
    BuilderRegistry, ConfigValue, ElementOptions, PipelineOptions,
};
use fiftyone_pipeline_core::{Error, Pipeline, PipelineBuilder, Result};

const NO_SUB_ELEMENTS: &str = "No SubElements";
const NO_INNER: &str = "No Inner Exception.";

fn registry() -> BuilderRegistry {
    let mut registry = BuilderRegistry::new();
    configuration::add_builders(&mut registry);
    registry
}

fn build(options: &PipelineOptions) -> Result<Arc<Pipeline>> {
    PipelineBuilder::new().build_from_configuration(&registry(), options)
}

fn elements(names: &[&str]) -> PipelineOptions {
    names.iter().fold(PipelineOptions::new(), |options, name| {
        options.element(ElementOptions::new(*name))
    })
}

fn message(result: Result<Arc<Pipeline>>) -> String {
    match result {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

/// Each element's data key and the names of its properties.
fn described(pipeline: &Pipeline) -> Vec<(String, Vec<String>)> {
    pipeline
        .flow_elements()
        .iter()
        .map(|element| {
            (
                element.data_key().to_owned(),
                element
                    .properties()
                    .iter()
                    .map(|p| p.name.clone())
                    .collect(),
            )
        })
        .collect()
}

/// The two engines as .NET described them.
fn country_engines() -> Vec<(String, Vec<String>)> {
    let engine = |key: &str, properties: &[&str]| {
        (
            key.to_owned(),
            properties.iter().map(|p| p.to_string()).collect(),
        )
    };
    vec![
        engine(
            "countrynames",
            &["CountryNamesGeographical", "CountryNamesPopulation"],
        ),
        engine(
            "countrynamestranslated",
            &[
                "CountryNamesGeographicalTranslated",
                "CountryNamesPopulationTranslated",
                "CountryNamesGeographicalAllTranslated",
                "CountryNamesPopulationAllTranslated",
                "CountryCodesGeographicalAll",
                "CountryCodesPopulationAll",
            ],
        ),
    ]
}

#[test]
fn the_builders_are_added_in_the_order_dotnet_listed_them() {
    assert_eq!(
        registry().builder_names(),
        [
            "CountriesTranslationEngineBuilder",
            "CountryCodeTranslationEngineBuilder",
            "TranslationEngineBuilder",
            "CloudCountriesTranslationEngineBuilder",
        ]
    );
}

/// .NET made both engines by their builders' names, by the names without
/// `Builder` in any case, and as a parallel group.
#[test]
fn the_country_engines_are_made_by_dotnets_names() {
    for names in [
        [
            "CountryCodeTranslationEngineBuilder",
            "CountriesTranslationEngineBuilder",
        ],
        ["CountryCodeTranslationEngine", "countriestranslationengine"],
    ] {
        let pipeline = build(&elements(&names)).expect("the pipeline");
        assert_eq!(described(&pipeline), country_engines(), "{names:?}");
    }
    let group = PipelineOptions::new().element(ElementOptions::group([
        ElementOptions::new("CountryCodeTranslationEngineBuilder"),
        ElementOptions::new("CountriesTranslationEngineBuilder"),
    ]));
    let pipeline = build(&group).expect("the pipeline");
    assert_eq!(described(&pipeline), country_engines());
}

/// Neither of .NET's builders has a method, so .NET took each parameter
/// for a build parameter and found no build method taking it.
#[test]
fn a_country_engine_takes_no_setting() {
    for (builder, builder_type, parameter) in [
        (
            "CountryCodeTranslationEngineBuilder",
            "fiftyone_ip_intelligence_translation::country_code::\
             CountryCodeTranslationEngineBuilder",
            "FixedLanguage",
        ),
        (
            "CountriesTranslationEngineBuilder",
            "fiftyone_ip_intelligence_translation::countries::CountriesTranslationEngineBuilder",
            "SourceElementDataKey",
        ),
    ] {
        let options = PipelineOptions::new()
            .element(ElementOptions::new(builder).parameter(parameter, ConfigValue::from("x")));
        assert_eq!(
            message(build(&options)),
            format!(
                "Configuration failed to build.\nBuilderName: {builder}\nSubElement Builder \
                 Names: {NO_SUB_ELEMENTS}\nBuilder '{builder_type}' for element 0 has no 'Set' \
                 methods or 'Build' methods that match (case-insensitively) the following \
                 parameters: {}. The available configuration methods on this builder are: \n\
                 The available 'Build' methods have the following signatures: \n\n\n{NO_INNER}\n",
                parameter.to_uppercase()
            ),
            "{builder}"
        );
    }
}
