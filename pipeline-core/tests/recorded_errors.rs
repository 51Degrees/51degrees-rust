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

//! An element that carries on past a failure records it on the flow data,
//! and whether that error fails processing is the element's choice.

use std::sync::Arc;

use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, FlowError,
    Pipeline, PropertyMetaData, Result,
};

/// An element that records an error and carries on.
struct RecordingErrorElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    should_throw: bool,
}

impl RecordingErrorElement {
    fn new(should_throw: bool) -> Self {
        RecordingErrorElement {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: Vec::new(),
            should_throw,
        }
    }
}

impl FlowElement for RecordingErrorElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        data.add_error(FlowError::with_should_throw(
            "recording",
            Error::Pipeline {
                message: "carried on".to_owned(),
            },
            self.should_throw,
        ));
        Ok(())
    }
    fn data_key(&self) -> &str {
        "recording"
    }
    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }
    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

#[test]
fn an_element_can_record_an_error_that_does_not_fail_processing() {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(RecordingErrorElement::new(false)))
        .suppress_process_exceptions(false)
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    assert!(data.process().is_ok());
    assert_eq!(data.errors().len(), 1);
    assert_eq!(data.errors()[0].element_data_key, "recording");
    assert!(!data.errors()[0].should_throw);
    assert_eq!(
        data.errors()[0].source.to_string(),
        "pipeline error: carried on"
    );
}

#[test]
fn an_error_an_element_records_to_throw_fails_processing() {
    let pipeline = Pipeline::builder()
        .add_element(Arc::new(RecordingErrorElement::new(true)))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    match data.process() {
        Err(Error::Aggregate(errors)) => {
            assert_eq!(errors.len(), 1);
            assert!(matches!(errors[0].source, Error::Pipeline { .. }));
            assert_eq!(errors[0].element_data_key, "recording");
        }
        other => panic!("expected an aggregate error, got {other:?}"),
    }
}
