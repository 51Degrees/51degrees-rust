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

//! Property metadata read at the pipeline level, as the
//! [properties specification](https://github.com/51Degrees/specifications/blob/706de1f0cb3edd560eb7fd7ef3bb19b93a4883dc/pipeline-specification/features/properties.md#L70)
//! says: "In addition, it MUST be possible to get Property metadata at the
//! Pipeline level for all Properties that can be populated by all Flow
//! Elements in the Pipeline."

use std::sync::Arc;

use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData, PropertyValueType, Result,
};

/// An element that populates the properties it is given and does nothing.
struct Populates {
    key: &'static str,
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl FlowElement for Populates {
    fn process(&self, _data: &mut FlowData) -> Result<()> {
        Ok(())
    }
    fn data_key(&self) -> &str {
        self.key
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

fn element(key: &'static str, names: &[&str]) -> Arc<dyn FlowElement> {
    Arc::new(Populates {
        key,
        filter: EvidenceKeyFilterWhitelist::default(),
        properties: names
            .iter()
            .map(|name| PropertyMetaData::new(*name, key, PropertyValueType::String))
            .collect(),
    })
}

/// The element data key and name of each property the pipeline gives.
fn named(properties: &[PropertyMetaData]) -> Vec<(String, String)> {
    properties
        .iter()
        .map(|p| (p.element_data_key.clone(), p.name.clone()))
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(key, name)| (key.to_string(), name.to_string()))
        .collect()
}

#[test]
fn the_pipeline_gives_the_metadata_of_every_property_of_every_element() {
    let pipeline = Pipeline::builder()
        .add_element(element("first", &["one", "two"]))
        .add_element(element("empty", &[]))
        .add_parallel_elements([element("left", &["three"]), element("right", &["four"])])
        .add_element(element("last", &["five"]))
        .build()
        .unwrap();

    assert_eq!(
        named(&pipeline.properties()),
        pairs(&[
            ("first", "one"),
            ("first", "two"),
            ("last", "five"),
            ("left", "three"),
            ("right", "four"),
        ])
    );
}

#[test]
fn the_pipeline_finds_a_property_by_its_element_and_name_ignoring_case() {
    let pipeline = Pipeline::builder()
        .add_element(element("device", &["IsMobile"]))
        .add_element(element("ip", &["IsMobile"]))
        .build()
        .unwrap();

    let found = pipeline.property("Device", "ismobile").unwrap();
    assert_eq!(found.element_data_key, "device");
    assert_eq!(found.name, "IsMobile");
    assert_eq!(pipeline.property("ip", "ISMOBILE").unwrap().element_data_key, "ip");
    assert!(pipeline.property("device", "browsername").is_none());
    assert!(pipeline.property("location", "ismobile").is_none());
}
