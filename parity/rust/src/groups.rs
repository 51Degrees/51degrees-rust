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

//! The pipelines that run Device Detection, IP Intelligence and an element
//! that always fails, once as a parallel group and once in sequence, each
//! with process errors collected and with them returned. The .NET producer
//! builds the same four, with its failing element giving the same message.

use std::sync::Arc;

use anyhow::Result;
use fiftyone_pipeline_core::{
    Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData,
};

/// The `pipeline` of a case that runs through each of the four pipelines.
pub const GROUP: &str = "group";
pub const GROUP_UNSUPPRESSED: &str = "group-unsuppressed";
pub const IN_SEQUENCE: &str = "in-sequence";
pub const IN_SEQUENCE_UNSUPPRESSED: &str = "in-sequence-unsuppressed";

/// The failing element's data key.
const FAILING_DATA_KEY: &str = "failing";

/// The message of the error the failing element returns.
const FAILURE: &str = "The failing element fails differently.";

/// An element that reads no evidence, lists no properties and returns an
/// error from every call.
struct FailingElement {
    filter: EvidenceKeyFilterWhitelist,
}

impl FlowElement for FailingElement {
    fn process(&self, _data: &mut FlowData) -> fiftyone_pipeline_core::Result<()> {
        Err(Error::Pipeline {
            message: FAILURE.to_owned(),
        })
    }

    fn data_key(&self) -> &str {
        FAILING_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &[]
    }
}

/// The four pipelines, each with the name a case gives as its `pipeline`.
/// Each holds `device`, `ip` and the failing element in that order.
pub fn build_pipelines(
    device: &Arc<dyn FlowElement>,
    ip: &Arc<dyn FlowElement>,
) -> Result<Vec<(&'static str, Arc<Pipeline>)>> {
    let failing: Arc<dyn FlowElement> = Arc::new(FailingElement {
        filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
    });
    let elements = || [Arc::clone(device), Arc::clone(ip), Arc::clone(&failing)];
    let mut pipelines = Vec::new();
    for (name, grouped, suppress) in [
        (GROUP, true, true),
        (GROUP_UNSUPPRESSED, true, false),
        (IN_SEQUENCE, false, true),
        (IN_SEQUENCE_UNSUPPRESSED, false, false),
    ] {
        let mut builder = Pipeline::builder();
        if grouped {
            builder = builder.add_parallel_elements(elements());
        } else {
            for element in elements() {
                builder = builder.add_element(element);
            }
        }
        let pipeline = builder.suppress_process_exceptions(suppress).build()?;
        pipelines.push((name, pipeline));
    }
    Ok(pipelines)
}
