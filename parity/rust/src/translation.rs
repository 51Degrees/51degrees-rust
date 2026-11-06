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

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use fiftyone_ip_intelligence_shared::IP_DATA_KEY_NAME;
use fiftyone_ip_intelligence_translation::{
    CountriesTranslationEngine, CountryCodeTranslationEngine,
};
use fiftyone_json_builder::JsonBuilderElementBuilder;
use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    NoValueError, Pipeline, PropertyMetaData, PropertyValue, TypedKey,
};
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

const IP_KEY: TypedKey<CountriesData> = TypedKey::new(IP_DATA_KEY_NAME);

/// A property's value, or the reason it has none.
type Entry = std::result::Result<PropertyValue, &'static str>;

/// The country codes, in the order they were set.
struct CountriesData {
    entries: Vec<(&'static str, Entry)>,
}

impl CountriesData {
    fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, entry)| entry)
    }
}

impl ElementData for CountriesData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        match self.entry(name) {
            Some(Ok(value)) => Ok(value.clone()),
            Some(Err(reason)) => Err(NoValueError::new(*reason)),
            None => Err(NoValueError::new(format!(
                "No value for property '{name}'."
            ))),
        }
    }

    fn has_property(&self, name: &str) -> bool {
        self.entry(name).is_some()
    }

    fn keys(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Read one property's evidence as a weighted list, each item a record of
/// `rawweighting` and `value` as the IP Intelligence engines give it.
fn weighted_codes(text: &str) -> Result<Entry> {
    if text.is_empty() {
        return Ok(Err(NO_CODES_REASON));
    }
    let mut records = Vec::new();
    for item in text.split(',') {
        let Some((code, weighting)) = item.split_once(':') else {
            bail!("{item} is not code:raw weighting");
        };
        let weighting: u16 = weighting
            .parse()
            .with_context(|| format!("{item} has no raw weighting from 0 to 65535"))?;
        records.push(BTreeMap::from([
            (
                "rawweighting".to_owned(),
                PropertyValue::Integer(i64::from(weighting)),
            ),
            ("value".to_owned(), PropertyValue::String(code.to_owned())),
        ]));
    }
    Ok(Ok(PropertyValue::KeyValueList(records)))
}

/// Writes the country codes the evidence gives under IP Intelligence's data
/// key, and nothing when the evidence gives none. It lists no properties,
/// so the line leaves it out and only the JSON shows its data.
struct CountriesElement {
    filter: EvidenceKeyFilterWhitelist,
}

impl FlowElement for CountriesElement {
    fn process(&self, data: &mut FlowData) -> fiftyone_pipeline_core::Result<()> {
        let mut entries = Vec::new();
        for (name, key) in PROPERTIES {
            if let Some(text) = data.evidence().get(key) {
                let entry = weighted_codes(text).map_err(|error| {
                    fiftyone_pipeline_core::Error::Pipeline {
                        message: format!("{key}: {error:#}"),
                    }
                })?;
                entries.push((name, entry));
            }
        }
        if !entries.is_empty() {
            data.get_or_add(IP_KEY, || CountriesData { entries })?;
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
    let countries = CountriesElement {
        filter: EvidenceKeyFilterWhitelist::new(PROPERTIES.map(|(_, key)| key)),
    };
    Ok(Pipeline::builder()
        .add_element(sequence)
        .add_element(Arc::new(countries))
        .add_element(Arc::new(CountryCodeTranslationEngine::builder().build()?))
        .add_element(Arc::new(CountriesTranslationEngine::builder().build()?))
        .add_element(Arc::new(json))
        .suppress_process_exceptions(true)
        .build()?)
}
