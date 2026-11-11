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

//! The translation pipeline: the Sequence element, an element that writes IP
//! Intelligence country codes taken from the case's evidence, the two country
//! translation engines and the JSON builder, as the .NET producer builds it.
//!
//! The codes come from the evidence rather than a data file, so every case
//! gives both languages the same codes and weightings whatever the file.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use fiftyone_ip_intelligence_shared::{IpIntelligenceDataBase, IP_DATA_KEY, IP_DATA_KEY_NAME};
use fiftyone_ip_intelligence_translation::{
    CountriesTranslationEngine, CountryCodeTranslationEngine,
};
use fiftyone_json_builder::JsonBuilderElementBuilder;
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData, WeightedValue,
};
use fiftyone_pipeline_engines::{EngineDeployment, MissingPropertySource};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

/// The `pipeline` of a case that runs through this pipeline.
pub const PIPELINE: &str = "translation";

/// The evidence holding each property's codes, as `code:raw weighting`
/// items separated by commas. A property whose evidence is absent is not
/// set, and one whose evidence is empty has no value.
const PROPERTIES: [(&str, &str); 2] = [
    ("CountryCodesGeographical", "query.parity-geographical"),
    ("CountryCodesPopulation", "query.parity-population"),
];

/// The reason a property with empty evidence gives.
const NO_CODES_REASON: &str = "The evidence gave no country codes.";

/// The type the reason for a missing property names.
const ELEMENT_TYPE_NAME: &str = "CountriesElement";

/// Read one property's evidence as weighted codes, or `None` when it is
/// empty.
fn weighted_codes(text: &str) -> Result<Option<Vec<WeightedValue<String>>>> {
    if text.is_empty() {
        return Ok(None);
    }
    text.split(',')
        .map(|item| -> Result<WeightedValue<String>> {
            let Some((code, weighting)) = item.split_once(':') else {
                bail!("{item} is not code:raw weighting");
            };
            let weighting: u16 = weighting
                .parse()
                .with_context(|| format!("{item} has no raw weighting from 0 to 65535"))?;
            Ok(WeightedValue::new(weighting, code.to_owned()))
        })
        .collect::<Result<_>>()
        .map(Some)
}

/// Writes the country codes the evidence gives as the crates' IP
/// Intelligence data, with a missing property source as the on-premise IP
/// Intelligence engine adds one, so the translation engines read each code as
/// a record of `rawweighting` and `value` and find a property the evidence
/// did not give missing, as in a deployment. It writes no data when neither
/// evidence key is present. It lists no properties, so the line leaves it out
/// and only the JSON shows its data.
struct CountriesElement {
    filter: EvidenceKeyFilterWhitelist,
    missing: Arc<MissingPropertySource>,
}

impl CountriesElement {
    fn new() -> Self {
        CountriesElement {
            filter: EvidenceKeyFilterWhitelist::new(PROPERTIES.map(|(_, key)| key)),
            missing: Arc::new(MissingPropertySource::new(
                IP_DATA_KEY_NAME,
                ELEMENT_TYPE_NAME,
                EngineDeployment::OnPremise,
                "",
                Vec::new(),
            )),
        }
    }

    /// Empty IP Intelligence data that knows its missing properties.
    fn empty_data(&self) -> IpIntelligenceDataBase {
        let mut ip = IpIntelligenceDataBase::new(IP_DATA_KEY_NAME);
        ip.base_mut()
            .add_missing_property_source(Arc::clone(&self.missing));
        ip
    }
}

impl FlowElement for CountriesElement {
    fn process(&self, data: &mut FlowData) -> fiftyone_pipeline_core::Result<()> {
        let mut written: Option<IpIntelligenceDataBase> = None;
        for (name, key) in PROPERTIES {
            let Some(text) = data.evidence().get(key) else {
                continue;
            };
            let codes =
                weighted_codes(text).map_err(|error| fiftyone_pipeline_core::Error::Pipeline {
                    message: format!("{key}: {error:#}"),
                })?;
            let ip = written.get_or_insert_with(|| self.empty_data());
            match codes {
                Some(codes) => ip.set_weighted_string(name, codes),
                None => ip.set_weighted_string_no_value(name, NO_CODES_REASON),
            }
        }
        if let Some(ip) = written {
            data.get_or_add(IP_DATA_KEY, || ip)?;
        }
        Ok(())
    }

    fn data_key(&self) -> &str {
        IP_DATA_KEY_NAME
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

/// Sequence, the country codes element, the two translation engines and the
/// JSON builder, with process errors collected rather than returned. The
/// crates' Sequence element has data of its own, which .NET's does not, so
/// the JSON builder leaves that data out.
pub fn build_pipeline() -> Result<Arc<Pipeline>> {
    let sequence = Arc::new(SequenceElement::new());
    let json = JsonBuilderElementBuilder::new()
        .exclude_element(sequence.data_key())
        .build();
    let countries = CountriesElement::new();
    Ok(Pipeline::builder()
        .add_element(sequence)
        .add_element(Arc::new(countries))
        .add_element(Arc::new(CountryCodeTranslationEngine::builder().build()?))
        .add_element(Arc::new(CountriesTranslationEngine::builder().build()?))
        .add_element(Arc::new(json))
        .suppress_process_exceptions(true)
        .build()?)
}
