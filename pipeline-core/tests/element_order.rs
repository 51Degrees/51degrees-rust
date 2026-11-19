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

//! The order of element data in a flow data and of the names in a property
//! bag, which the JSON builder keeps when it writes them, as .NET does.

use std::any::Any;
use std::sync::Arc;

use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, Result, TypedKey,
};

/// Element data holding one value.
struct ValueData(MapElementData);

impl ElementData for ValueData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        self.0.get(name)
    }
    fn keys(&self) -> Vec<String> {
        self.0.keys()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// An element that adds data under its key, which it gives in mixed case.
struct Writer {
    key: &'static str,
    filter: EvidenceKeyFilterWhitelist,
}

impl FlowElement for Writer {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.get_or_add(TypedKey::<ValueData>::new(self.key), || {
            ValueData(MapElementData::new().set("value", self.key))
        })?;
        Ok(())
    }
    fn data_key(&self) -> &str {
        self.key
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

fn writer(key: &'static str) -> Arc<dyn FlowElement> {
    Arc::new(Writer {
        key,
        filter: EvidenceKeyFilterWhitelist::default(),
    })
}

/// Six keys, so a map that lost the order would give this one by chance once
/// in 720 runs.
const KEYS: [&str; 6] = ["Zulu", "alpha", "mike", "Bravo", "yankee", "charlie"];

fn lowercase(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|key| key.to_lowercase()).collect()
}

#[test]
fn data_keys_are_in_the_order_the_elements_added_their_data() {
    let mut builder = Pipeline::builder();
    for key in KEYS {
        builder = builder.add_element(writer(key));
    }
    let pipeline = builder.build().unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    assert_eq!(data.data_keys(), lowercase(&KEYS));
}

#[test]
fn a_groups_data_comes_after_the_group_in_the_order_its_elements_were_given() {
    let pipeline = Pipeline::builder()
        .add_element(writer(KEYS[0]))
        .add_parallel_elements([writer(KEYS[1]), writer(KEYS[2]), writer(KEYS[3])])
        .add_element(writer(KEYS[4]))
        .add_element(writer(KEYS[5]))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    assert_eq!(data.data_keys(), lowercase(&KEYS));
}

#[test]
fn a_property_bag_lists_names_in_the_order_first_set() {
    let mut bag = MapElementData::new();
    for (position, name) in KEYS.iter().enumerate() {
        bag.insert(name, position as i64);
    }
    // Setting a name again, in another case, keeps its place.
    bag.insert("ALPHA", 100_i64);
    assert_eq!(bag.keys(), lowercase(&KEYS));
    assert_eq!(bag.get("alpha").unwrap(), PropertyValue::Integer(100));
    assert_eq!(bag.len(), KEYS.len());
}
