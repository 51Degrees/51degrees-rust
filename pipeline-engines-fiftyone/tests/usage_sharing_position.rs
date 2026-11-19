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

//! A pipeline built in code refuses the usage sharing element anywhere but
//! last on its own, as the usage sharing element specification's "Position
//! in the Pipeline" section requires.

use std::sync::Arc;

use fiftyone_pipeline_core::{Error, FlowElement, Pipeline};
use fiftyone_pipeline_engines_fiftyone::{
    SequenceElement, SetHeadersElement, ShareUsageConfig, ShareUsageElement,
};

fn share() -> Arc<dyn FlowElement> {
    // A share percentage of zero sends nothing.
    Arc::new(ShareUsageElement::new(
        ShareUsageConfig::builder().share_percentage(0.0).build(),
    ))
}

const REFUSED: &str = "The usage sharing element, with the data key 'shareusage', must be \
                       the last flow element in a pipeline and outside any parallel group.";

#[test]
fn usage_sharing_is_built_only_last_on_its_own() {
    let built = Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(SetHeadersElement::new()))
        .add_element(share())
        .build()
        .unwrap();
    assert_eq!(
        built.flow_elements().last().unwrap().data_key(),
        "shareusage"
    );

    for builder in [
        Pipeline::builder()
            .add_element(share())
            .add_element(Arc::new(SequenceElement::new())),
        Pipeline::builder()
            .add_element(Arc::new(SequenceElement::new()))
            .add_parallel_elements([share(), Arc::new(SetHeadersElement::new())]),
    ] {
        match builder.build() {
            Err(Error::PipelineConfiguration { message }) => assert_eq!(message, REFUSED),
            Err(other) => panic!("not a configuration error: {other}"),
            Ok(_) => panic!("the pipeline was built"),
        }
    }
}
