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

//! A pipeline refuses an element that must run last, being the usage sharing
//! element, anywhere but last on its own, as the usage sharing element
//! specification's "Position in the Pipeline" section requires.

use std::sync::Arc;

use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PipelineBuilder, PropertyMetaData, Result,
};

/// An element that does nothing, and must run last when `last` is set.
struct Element {
    key: &'static str,
    last: bool,
    filter: EvidenceKeyFilterWhitelist,
}

impl FlowElement for Element {
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
        &[]
    }
    fn must_run_last(&self) -> bool {
        self.last
    }
}

fn element(key: &'static str, last: bool) -> Arc<dyn FlowElement> {
    Arc::new(Element {
        key,
        last,
        filter: EvidenceKeyFilterWhitelist::default(),
    })
}

fn other(key: &'static str) -> Arc<dyn FlowElement> {
    element(key, false)
}

fn usage() -> Arc<dyn FlowElement> {
    element("shareusage", true)
}

const REFUSED: &str = "The usage sharing element, with the data key 'shareusage', must be \
                       the last flow element in a pipeline and outside any parallel group.";

fn refusal(builder: PipelineBuilder) -> String {
    match builder.build() {
        Err(Error::PipelineConfiguration { message }) => message,
        Err(other) => panic!("not a configuration error: {other}"),
        Ok(_) => panic!("the pipeline was built"),
    }
}

#[test]
fn usage_sharing_last_on_its_own_is_built() {
    for builder in [
        Pipeline::builder().add_element(usage()),
        Pipeline::builder()
            .add_element(other("first"))
            .add_parallel_elements([other("a"), other("b")])
            .add_element(usage()),
        // A group of one is the element on its own.
        Pipeline::builder()
            .add_element(other("first"))
            .add_parallel_elements([usage()]),
    ] {
        let pipeline = builder.build().unwrap();
        assert!(pipeline
            .flow_elements()
            .iter()
            .any(|element| element.data_key() == "shareusage"));
    }
}

#[test]
fn usage_sharing_before_another_element_is_refused() {
    let builder = Pipeline::builder()
        .add_element(usage())
        .add_element(other("after"));
    assert_eq!(refusal(builder), REFUSED);

    let builder = Pipeline::builder()
        .add_element(usage())
        .add_parallel_elements([other("a"), other("b")]);
    assert_eq!(refusal(builder), REFUSED);
}

#[test]
fn usage_sharing_in_a_parallel_group_is_refused() {
    for group in [
        vec![usage(), other("beside")],
        vec![other("beside"), usage()],
    ] {
        let builder = Pipeline::builder()
            .add_element(other("first"))
            .add_parallel_elements(group);
        assert_eq!(refusal(builder), REFUSED);
    }
}

#[test]
fn a_second_usage_sharing_element_is_refused() {
    let builder = Pipeline::builder()
        .add_element(usage())
        .add_element(usage());
    assert_eq!(refusal(builder), REFUSED);
}
