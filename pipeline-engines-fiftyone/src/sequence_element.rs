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
//! The element writes the session id and the new sequence into the flow
//! data's evidence with [`FlowData::add_evidence`], as .NET's Sequence element
//! does, so the elements after it and the caller read them as
//! `query.session-id` and `query.sequence`. It also holds them in its own
//! [`SequenceData`], because the specification lists them as the element's
//! properties.

use std::any::Any;

use fiftyone_pipeline_core::{
    ElementData, Error, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    FlowError, MapElementData, NoValueError, PropertyMetaData, PropertyValue, PropertyValueType,
    Result, TypedKey,
};

use crate::constants::{
    EVIDENCE_SEQUENCE, EVIDENCE_SEQUENCE_SUFFIX, EVIDENCE_SESSIONID, EVIDENCE_SESSIONID_SUFFIX,
    SEQUENCE_DEFAULT_ELEMENT_DATA_KEY,
};

/// The message of the error a sequence that is not a 32-bit integer gives.
const SEQUENCE_NOT_VALID: &str = "The value for the 'sequence' parameter is not valid. See https://51degrees.com/documentation/4.5/_services__cloud__error_messages.html?utm_source=code&utm_medium=comment&utm_campaign=51degrees-rust&utm_content=pipeline-engines-fiftyone-src-sequence_element.rs&utm_term=sequence-not-valid for more information.";

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
    /// Build sequence data from a session id and, when the request's sequence
    /// could be read, the new sequence number.
    fn new(session_id: String, sequence: Option<i32>) -> Self {
        let mut inner = MapElementData::new().set(EVIDENCE_SESSIONID_SUFFIX, session_id);
        if let Some(sequence) = sequence {
            inner = inner.set(EVIDENCE_SEQUENCE_SUFFIX, i64::from(sequence));
        }
        SequenceData { inner }
    }

    /// The session id for this flow data.
    pub fn session_id(&self) -> Option<&str> {
        self.inner
            .get_value(EVIDENCE_SESSIONID_SUFFIX)
            .and_then(PropertyValue::as_str)
    }

    /// The sequence number for this flow data, which is always a 32-bit
    /// integer. `None` when the request's sequence is not a 32-bit integer.
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
/// - If `query.session-id` is absent from the evidence, a new GUID is added
///   as the session id. A session id that is present, even an empty one, is
///   kept as it is.
/// - If `query.sequence` is absent, a sequence of `1` is added.
/// - If `query.sequence` is a 32-bit integer, read as .NET's `int.TryParse`
///   reads it, one is added to it, wrapping from `2147483647` to
///   `-2147483648`, and the result replaces it.
/// - Otherwise the sequence is left as it was and an error is recorded on the
///   flow data, logged at error level whether or not errors are suppressed,
///   followed by a second error level record saying the sequence was not
///   incremented. When errors are not suppressed, processing returns the
///   error once every element has run.
///
/// Both values are also written to a [`SequenceData`] under this element's
/// data key, which holds no sequence when the request's could not be read.
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

    /// Generate a new session id. A random v4 GUID in its 36 character lower
    /// case form with hyphens, the form .NET's `Guid.ToString()` gives.
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
        let given_session_id = data.evidence().get(EVIDENCE_SESSIONID).map(str::to_owned);
        let session_id = match given_session_id {
            Some(session_id) => session_id,
            None => {
                let session_id = Self::new_session_id();
                data.add_evidence(EVIDENCE_SESSIONID, session_id.clone());
                session_id
            }
        };

        let sequence = match data.evidence().get(EVIDENCE_SEQUENCE) {
            None => Some(1),
            Some(text) => parse_sequence(text).map(|sequence| sequence.wrapping_add(1)),
        };
        match sequence {
            Some(sequence) => data.add_evidence(EVIDENCE_SEQUENCE, sequence.to_string()),
            None => {
                data.add_error(
                    FlowError::new(
                        self.data_key(),
                        Error::Pipeline {
                            message: SEQUENCE_NOT_VALID.to_owned(),
                        },
                    ),
                    true,
                );
                log::error!(
                    "Failed to increment the usage sequence number, because \
                     the sequence is not a 32-bit integer."
                );
            }
        }

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

/// Read a sequence number from evidence text as .NET's `int.TryParse` reads
/// it, being white space, an optional sign, the digits 0 to 9, white space,
/// then any number of NUL characters. White space here is space and U+0009
/// to U+000D. `None` when the text is not a 32-bit integer.
pub(crate) fn parse_sequence(text: &str) -> Option<i32> {
    text.trim_end_matches('\0')
        .trim_matches(is_number_white_space)
        .parse()
        .ok()
}

/// The white space .NET allows around a number.
fn is_number_white_space(c: char) -> bool {
    matches!(c, ' ' | '\t'..='\r')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sequence_is_read_as_dotnet_reads_an_integer() {
        for (text, expected) in [
            ("3", Some(3)),
            (" \t+7\r\n", Some(7)),
            ("-2", Some(-2)),
            ("0005", Some(5)),
            ("2147483647", Some(i32::MAX)),
            ("-2147483648", Some(i32::MIN)),
            ("5\0\0", Some(5)),
            ("5 \0", Some(5)),
            ("2147483648", None),
            ("5\0 ", None),
            ("\u{a0}5", None),
            ("\u{663}", None),
            ("1.0", None),
            ("five", None),
            ("+", None),
            (" ", None),
            ("", None),
        ] {
            assert_eq!(parse_sequence(text), expected, "{text:?}");
        }
    }
}
