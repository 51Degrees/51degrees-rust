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

//! Getting an element's data from the flow data by giving the element
//! itself, mechanism 3 of the
//! [access-to-results specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/access-to-results.md#accessing-results).

use std::any::Any;
use std::sync::Arc;

use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, Pipeline, PropertyMetaData, PropertyValue, Result, TypedKey,
};

/// Element data holding the number its element was given.
struct NumberData(MapElementData);

impl ElementData for NumberData {
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

/// An element that adds its number under its data key.
struct NumberElement {
    key: &'static str,
    number: i64,
    filter: EvidenceKeyFilterWhitelist,
}

impl NumberElement {
    fn new(key: &'static str, number: i64) -> Self {
        NumberElement {
            key,
            number,
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
        }
    }
}

impl FlowElement for NumberElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let number = self.number;
        data.get_or_add(TypedKey::<NumberData>::new(self.key), || {
            NumberData(MapElementData::new().set("number", number))
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

/// A processed flow data from a pipeline of the two elements.
fn processed(first: &Arc<NumberElement>, second: &Arc<NumberElement>) -> FlowData {
    let pipeline = Pipeline::builder()
        .add_element(first.clone())
        .add_element(second.clone())
        .build()
        .expect("the pipeline builds");
    let mut data = pipeline.create_flow_data();
    data.process().expect("processing succeeds");
    data
}

#[test]
fn each_elements_data_is_found_by_giving_the_element() {
    let first = Arc::new(NumberElement::new("first", 1));
    let second = Arc::new(NumberElement::new("second", 2));
    let data = processed(&first, &second);

    for (element, number) in [(&first, 1), (&second, 2)] {
        let found = data
            .get_from_element(element.as_ref())
            .expect("the element's data is found");
        assert_eq!(
            found.get("number").ok(),
            Some(PropertyValue::Integer(number))
        );
        assert!(found.as_any().downcast_ref::<NumberData>().is_some());
    }
}

#[test]
fn an_element_given_as_a_trait_object_finds_its_data() {
    let first = Arc::new(NumberElement::new("first", 1));
    let second = Arc::new(NumberElement::new("second", 2));
    let data = processed(&first, &second);

    let elements: Vec<Arc<dyn FlowElement>> = vec![first, second];
    for element in &elements {
        let found = data
            .get_from_element(element.as_ref())
            .expect("the element's data is found");
        let by_key = data
            .get_str(element.data_key())
            .expect("the data is there by key");
        assert!(std::ptr::eq(
            found.as_any() as *const dyn Any as *const (),
            by_key.as_any() as *const dyn Any as *const ()
        ));
    }
}

#[test]
fn an_element_that_added_no_data_finds_none() {
    let first = Arc::new(NumberElement::new("first", 1));
    let second = Arc::new(NumberElement::new("second", 2));
    let data = processed(&first, &second);

    let other = NumberElement::new("other", 3);
    assert!(data.get_from_element(&other).is_none());
}
