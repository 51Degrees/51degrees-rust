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

//! The sequence element.
//!
//! The sequence element establishes the session id and sequence number used to
//! correlate the callbacks that the client-side JavaScript makes to the server.
//! It implements the
//! [sequence-element specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/sequence-element.md).
//!
//! # Where the results are written
//!
//! [`fiftyone_pipeline_core::Evidence`] is immutable, so the element writes the
//! new session id and the incremented sequence into its own element data rather
//! than back into the evidence collection. Downstream elements that need the
//! values (notably the [`crate::ShareUsageElement`]) read them from the element
//! data, or from evidence when the caller supplied them. The observable
//! behavior is a stable session id and a monotonically increasing sequence.

use std::any::Any;

use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, PropertyMetaData, PropertyValue, PropertyValueType, Result,
    TypedKey,
};

use crate::constants::{
    EVIDENCE_SEQUENCE, EVIDENCE_SEQUENCE_SUFFIX, EVIDENCE_SESSIONID, EVIDENCE_SESSIONID_SUFFIX,
    SEQUENCE_DEFAULT_ELEMENT_DATA_KEY,
};

/// The element data produced by the [`SequenceElement`].
///
/// It exposes two properties, matching the specification element-data table:
///
/// - `session-id` (a string) identifying this session.
/// - `sequence` (an integer) counting how many callbacks have been made.
///
/// The element data is backed by a [`MapElementData`] so it gets the dynamic
/// property bag for free. [`ElementData::get`] returns `Err(NoValueError)` for
/// any name this data does not own, as the access-to-results contract requires.
#[derive(Debug, Clone)]
pub struct SequenceData {
    inner: MapElementData,
}

impl SequenceData {
    /// Build sequence data from a session id and sequence number.
    fn new(session_id: String, sequence: i64) -> Self {
        let inner = MapElementData::new()
            .set(EVIDENCE_SESSIONID_SUFFIX, session_id)
            .set(EVIDENCE_SEQUENCE_SUFFIX, sequence);
        SequenceData { inner }
    }

    /// The session id for this flow data.
    pub fn session_id(&self) -> Option<&str> {
        self.inner
            .get_value(EVIDENCE_SESSIONID_SUFFIX)
            .and_then(PropertyValue::as_str)
    }

    /// The sequence number for this flow data.
    pub fn sequence(&self) -> Option<i64> {
        self.inner
            .get_value(EVIDENCE_SEQUENCE_SUFFIX)
            .and_then(PropertyValue::as_integer)
    }
}

impl ElementData for SequenceData {
    fn get(&self, name: &str) -> std::result::Result<PropertyValue, NoValueError> {
        self.inner.get(name)
    }

    fn keys(&self) -> Vec<String> {
        self.inner.keys()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Establishes the session id and sequence evidence in the pipeline.
///
/// On [`FlowElement::process`]:
///
/// - If `query.session-id` is absent from the evidence, a new GUID is generated
///   using the `uuid` crate.
/// - If `query.sequence` is present, it is parsed and incremented, wrapping
///   after `i64::MAX` as the .NET and Java pipelines do. Otherwise, or when it
///   is not an integer, the sequence is set to `1`.
///
/// Both values are written to a [`SequenceData`] under this element's data key.
/// The element takes no configuration.
pub struct SequenceElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
}

impl SequenceElement {
    /// The typed key under which this element stores its [`SequenceData`].
    pub const KEY: TypedKey<SequenceData> = TypedKey::new(SEQUENCE_DEFAULT_ELEMENT_DATA_KEY);

    /// The default element data key, `"sequence"`.
    pub const DEFAULT_ELEMENT_DATA_KEY: &'static str = SEQUENCE_DEFAULT_ELEMENT_DATA_KEY;

    /// Create a new sequence element.
    pub fn new() -> Self {
        let filter = EvidenceKeyFilterWhitelist::new([EVIDENCE_SESSIONID, EVIDENCE_SEQUENCE]);
        let properties = vec![
            PropertyMetaData::new(
                EVIDENCE_SESSIONID_SUFFIX,
                SEQUENCE_DEFAULT_ELEMENT_DATA_KEY,
                PropertyValueType::String,
            ),
            PropertyMetaData::new(
                EVIDENCE_SEQUENCE_SUFFIX,
                SEQUENCE_DEFAULT_ELEMENT_DATA_KEY,
                PropertyValueType::Integer,
            ),
        ];
        SequenceElement { filter, properties }
    }

    /// Generate a new session id. A random v4 GUID, the same form the other
    /// 51Degrees ports use.
    fn new_session_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

impl Default for SequenceElement {
    fn default() -> Self {
        Self::new()
    }
}

impl FlowElement for SequenceElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        // Read the incoming evidence (immutable) before taking the mutable
        // borrow needed to add element data.
        let session_id = data
            .evidence()
            .get(EVIDENCE_SESSIONID)
            .map(str::to_owned)
            .unwrap_or_else(Self::new_session_id);

        // If a sequence is supplied, increment it, wrapping after the largest
        // integer as the .NET and Java pipelines do. Anything that does not
        // parse as an integer is logged at error level, as .NET logs it, and
        // treated as a fresh sequence of 1 rather than failing the whole flow.
        let sequence = match data.evidence().get(EVIDENCE_SEQUENCE) {
            None => 1,
            Some(raw) => match raw.trim().parse::<i64>() {
                Ok(sequence) => sequence.wrapping_add(1),
                Err(_) => {
                    log::error!(
                        "Failed to increment the usage sequence number, because \
                         the sequence is not an integer."
                    );
                    1
                }
            },
        };

        data.get_or_add(Self::KEY, || SequenceData::new(session_id, sequence))?;
        Ok(())
    }

    fn data_key(&self) -> &str {
        SEQUENCE_DEFAULT_ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}
