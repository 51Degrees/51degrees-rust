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

//! The element data of each element in this crate is read only once the
//! element has finished processing.

use std::sync::Arc;

use fiftyone_pipeline_core::{ElementData, Error, FlowData, Pipeline, TypedKey};
use fiftyone_pipeline_engines_fiftyone::{
    SequenceElement, SetHeadersElement, ShareUsageConfig, ShareUsageElement,
};

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
fn each_elements_data_is_read_only_once_processed() {
    // A share percentage of zero sends nothing.
    let share = ShareUsageConfig::builder().share_percentage(0.0).build();
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(SetHeadersElement::new()))
        .add_element(Arc::new(ShareUsageElement::new(share)))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();

    assert_read_only(&mut data, SequenceElement::KEY);
    assert_read_only(&mut data, SetHeadersElement::KEY);
    // The usage sharing element adds no element data to write.
    assert_eq!(data.data_keys(), ["sequence", "set-headers"]);
}
