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

//! Adding Evidence, element data, errors or warnings to a flow data that has
//! finished processing fails, as the thread-safety specification's Flow Data
//! section requires. The methods that return nothing panic, and
//! `FlowData::get_or_add` returns an error.

use std::sync::Arc;

use fiftyone_pipeline_core::{
    Error, Evidence, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    FlowError, MapElementData, Pipeline, PropertyMetaData, Result, TypedKey,
};

/// What an element does when it processes.
type Action = Box<dyn Fn(&mut FlowData) -> Result<()> + Send + Sync>;

/// An element that runs `action`.
struct Element {
    key: &'static str,
    action: Action,
    filter: EvidenceKeyFilterWhitelist,
}

impl FlowElement for Element {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        (self.action)(data)
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

fn element(
    key: &'static str,
    action: impl Fn(&mut FlowData) -> Result<()> + Send + Sync + 'static,
) -> Arc<dyn FlowElement> {
    Arc::new(Element {
        key,
        action: Box::new(action),
        filter: EvidenceKeyFilterWhitelist::default(),
    })
}

/// A flow data processed by an element that adds Evidence, element data, an
/// error and a warning while it processes, which it may.
fn finished() -> FlowData {
    let pipeline = Pipeline::builder()
        .add_element(element("writer", |data| {
            data.add_evidence("query.during", "1");
            data.get_or_add(TypedKey::<MapElementData>::new("writer"), || {
                MapElementData::new().set("value", "1")
            })?;
            data.add_warning("a warning while processing", None);
            data.add_error(
                FlowError::with_should_throw("writer", Error::configuration("an error"), false),
                false,
            );
            Ok(())
        }))
        .build()
        .unwrap();
    let mut data =
        pipeline.create_flow_data_with(Evidence::builder().add("query.before", "0").build());
    data.process().unwrap();
    data
}

#[test]
fn an_element_adds_while_it_processes() {
    let data = finished();
    assert_eq!(data.evidence().get("query.during"), Some("1"));
    assert!(data.get_str("writer").is_some());
    assert_eq!(data.warnings().len(), 1);
    assert_eq!(data.errors().len(), 1);
}

#[test]
#[should_panic(expected = "Evidence cannot be added to a flow data that has finished processing")]
fn adding_evidence_after_processing_panics() {
    finished().add_evidence("query.after", "2");
}

#[test]
#[should_panic(expected = "An error cannot be added to a flow data that has finished processing")]
fn adding_an_error_after_processing_panics() {
    finished().add_error(
        FlowError::new("caller", Error::configuration("late")),
        false,
    );
}

#[test]
#[should_panic(expected = "A warning cannot be added to a flow data that has finished processing")]
fn adding_a_warning_after_processing_panics() {
    finished().add_warning("late", None);
}

#[test]
fn adding_element_data_after_processing_fails() {
    let mut data = finished();
    match data.get_or_add(TypedKey::<MapElementData>::new("late"), MapElementData::new) {
        Err(Error::PipelineConfiguration { message }) => assert_eq!(
            message,
            "Element data under key 'late' cannot be added to a flow data that has \
             finished processing."
        ),
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("element data was added after processing"),
    }
    assert!(data.get_str("late").is_none());
}

/// Processing that returns an error has finished all the same.
#[test]
#[should_panic(expected = "Evidence cannot be added to a flow data that has finished processing")]
fn a_flow_data_whose_processing_failed_has_finished() {
    let pipeline = Pipeline::builder()
        .add_element(element("fails", |_| Err(Error::configuration("failed"))))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    assert!(data.process().is_err());
    data.add_evidence("query.after", "2");
}

/// The caller adds what it likes before processing.
#[test]
fn the_caller_adds_before_processing() {
    let pipeline = Pipeline::builder()
        .add_element(element("idle", |_| Ok(())))
        .build()
        .unwrap();
    let mut data = pipeline.create_flow_data();
    data.add_evidence("query.before", "1");
    data.add_warning("before", None);
    data.get_or_add(
        TypedKey::<MapElementData>::new("caller"),
        MapElementData::new,
    )
    .unwrap();
    data.process().unwrap();
    assert_eq!(data.evidence().get("query.before"), Some("1"));
    assert_eq!(data.warnings().len(), 1);
    assert!(data.get_str("caller").is_some());
}
