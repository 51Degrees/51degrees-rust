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

//! The element data of the element in this crate is read only once the
//! element has finished processing.

use std::sync::Arc;

use fiftyone_json_builder::{JsonBuilderElement, JSON_BUILDER_DATA_KEY};
use fiftyone_pipeline_core::{ElementData, Error, FlowData, Pipeline, TypedKey};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

/// A write to the data under `key` after processing fails, and the data the
/// element added is still there.
fn assert_read_only<T: ElementData>(data: &mut FlowData, key: TypedKey<T>) {
    assert!(data.get(key).is_some(), "no data under '{}'", key.name());
    match data.get_or_add(key, || unreachable!("the element added its data")) {
        Err(Error::PipelineConfiguration { message }) => {
            assert!(message.contains("cannot be written"), "{message}")
        }
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!(
            "the data under '{}' was written after processing",
            key.name()
        ),
    }
}

#[test]
fn its_element_data_is_read_only_once_processed() {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(JsonBuilderElement::new()))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    assert_read_only(&mut data, JSON_BUILDER_DATA_KEY);
}
