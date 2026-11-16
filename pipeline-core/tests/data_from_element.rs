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

//! Element data got by supplying the element, as the
//! [access-to-results specification](https://github.com/51Degrees/specifications/blob/706de1f0cb3edd560eb7fd7ef3bb19b93a4883dc/pipeline-specification/features/access-to-results.md#L33)
//! says: "In strongly typed languages, it SHOULD be possible to get an
//! Element Data instance by supplying an element instance."

use std::any::Any;
use std::sync::Arc;

use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    NoValueError, Pipeline, PropertyMetaData, PropertyValue, Result, TypedFlowElement, TypedKey,
};

/// The data the colour element adds.
struct ColourData {
    colour: String,
}

impl ElementData for ColourData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        if name.eq_ignore_ascii_case("colour") {
            Ok(PropertyValue::String(self.colour.clone()))
        } else {
            Err(NoValueError::new(format!("No property '{name}'.")))
        }
    }
    fn keys(&self) -> Vec<String> {
        vec!["colour".to_string()]
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// An element that adds the colour it is given under its data key.
struct ColourElement {
    key: &'static str,
    colour: &'static str,
    filter: EvidenceKeyFilterWhitelist,
}

impl FlowElement for ColourElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.get_or_add(TypedKey::<ColourData>::new(self.key), || ColourData {
            colour: self.colour.to_string(),
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

impl TypedFlowElement for ColourElement {
    type Data = ColourData;
}

fn colour(key: &'static str, colour: &'static str) -> Arc<ColourElement> {
    Arc::new(ColourElement {
        key,
        colour,
        filter: EvidenceKeyFilterWhitelist::default(),
    })
}

#[test]
fn element_data_is_got_by_supplying_the_element_that_added_it() {
    let sky = colour("Sky", "blue");
    let grass = colour("grass", "green");
    let pipeline = Pipeline::builder()
        .add_element(sky.clone())
        .add_element(grass.clone())
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    // The data comes back as the element's own data type, not as a trait
    // object, so no key or downcast is needed.
    let sky_data: &ColourData = data.get_from_element(sky.as_ref()).unwrap();
    assert_eq!(sky_data.colour, "blue");
    assert_eq!(data.get_from_element(grass.as_ref()).unwrap().colour, "green");
}

#[test]
fn an_element_that_has_not_processed_gives_no_data() {
    let sky = colour("sky", "blue");
    let absent = colour("sea", "grey");
    let pipeline = Pipeline::builder().add_element(sky).build().unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    assert!(data.get_from_element(absent.as_ref()).is_none());
}
