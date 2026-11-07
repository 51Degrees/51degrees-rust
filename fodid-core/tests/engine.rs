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

//! An engine producing 51Dids built on this crate alone, with no cloud
//! request engine, runs in a pipeline, and its result reads through the
//! shared data model.

use std::sync::Arc;

use fiftyone_fodid_core::{
    default_aspect_property_metadata, default_property_metadata, FodIdData, FodIdDataBase,
    FodIdEngine, FODID_DATA_KEY, FODID_ELEMENT_DATA_KEY, ID_PROB_GLOBAL_PROPERTY,
};
use fiftyone_pipeline_core::{
    EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement, Pipeline,
    PropertyMetaData, Result,
};
use fiftyone_pipeline_engines::{AspectEngine, AspectPropertyMetaData, EngineDeployment};
use fodid::{Creator, Crypto, IdType, Usage};

/// The flags byte of the 51Did the engine issues, being the probabilistic
/// type, payload version 0 and the non-marketing usage bit.
const FLAGS: u8 = 0b0000_0001;

/// The license id the engine writes after the flags byte.
const LICENSE_ID: u32 = 0x0102_0304;

/// The match key the engine writes after the license id.
const MATCH_KEY: [u8; 32] = [0x2A; 32];

/// An engine that issues the same probabilistic 51Did for every request,
/// signed with a key of its own.
struct StandInEngine {
    envelope: String,
    evidence_filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    aspect_properties: Vec<AspectPropertyMetaData>,
}

impl StandInEngine {
    fn new() -> Self {
        let mut payload = vec![FLAGS];
        payload.extend_from_slice(&LICENSE_ID.to_le_bytes());
        payload.extend_from_slice(&MATCH_KEY);
        let creator = Creator::new("example.com", Crypto::new()).unwrap();
        let envelope = creator.create(payload).unwrap().as_base64().unwrap();
        StandInEngine {
            envelope,
            evidence_filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties: default_property_metadata(),
            aspect_properties: default_aspect_property_metadata(),
        }
    }
}

impl FlowElement for StandInEngine {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        let element = FodIdDataBase::new().set(ID_PROB_GLOBAL_PROPERTY, self.envelope.clone());
        data.get_or_add(FODID_DATA_KEY, || element)?;
        Ok(())
    }

    fn data_key(&self) -> &str {
        FODID_ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.evidence_filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

impl AspectEngine for StandInEngine {
    fn data_source_tier(&self) -> &str {
        ""
    }

    fn aspect_properties(&self) -> &[AspectPropertyMetaData] {
        &self.aspect_properties
    }
}

impl FodIdEngine for StandInEngine {}

/// Builds a pipeline from any engine producing 51Dids, as code written
/// against the trait would.
fn pipeline_of(engine: Arc<dyn FodIdEngine>) -> Arc<Pipeline> {
    Pipeline::builder().add_element(engine).build().unwrap()
}

#[test]
fn an_engine_built_on_this_crate_runs_in_a_pipeline() {
    let pipeline = pipeline_of(Arc::new(StandInEngine::new()));
    let mut data = pipeline.create_flow_data();
    data.process().unwrap();
    let result = data
        .get(FODID_DATA_KEY)
        .expect("the engine stores its data under the 51Did key");
    let parsed = result.id_prob_global_fod_id();
    let fod_id = parsed.value().unwrap();
    assert_eq!(fod_id.id_type(), IdType::Probabilistic);
    assert_eq!(fod_id.usage(), Usage::NonMarketing);
    assert_eq!(fod_id.license_id(), LICENSE_ID);
    assert_eq!(fod_id.match_key(), MATCH_KEY.as_slice());
}

#[test]
fn the_engine_is_reached_through_the_trait() {
    let engine: Arc<dyn FodIdEngine> = Arc::new(StandInEngine::new());
    assert_eq!(engine.data_key(), FODID_ELEMENT_DATA_KEY);
    assert_eq!(engine.deployment(), EngineDeployment::OnPremise);
    assert!(engine.has_loaded_properties());
    assert_eq!(
        engine.aspect_properties(),
        default_aspect_property_metadata().as_slice()
    );
}
