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

//! The default of each step in
//! [`JavaScriptBuilderSteps`](crate::JavaScriptBuilderSteps).
//!
//! A caller's step can call the function of the same name here to keep the
//! default for part of its work.

use std::collections::BTreeMap;

use fiftyone_pipeline_core::constants::{EVIDENCE_QUERY_PREFIX, EVIDENCE_SEPARATOR};
use fiftyone_pipeline_engines_fiftyone::constants::{EVIDENCE_SEQUENCE, EVIDENCE_SESSIONID};
use fiftyone_pipeline_engines_fiftyone::SequenceElement;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

use crate::steps::JavaScriptBuilderContext;
use crate::template_data::{safe_sequence, safe_session_id};

/// The `application/x-www-form-urlencoded` percent-encoding set used for the
/// request parameter names and values.
///
/// The set percent-encodes everything except the unreserved characters
/// `A-Z a-z 0-9 - _ . *` (and encodes a space as `%20`, not `+`, because each
/// name and value is encoded on its own before being placed into the JSON
/// object). It starts from "encode all non-alphanumerics" and adds back the
/// four exceptions.
const URL_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'*');

/// Every `query.` evidence value apart from the session id and sequence,
/// named by the text after `query.`, with the name and value percent-encoded,
/// in name order.
pub fn get_parameters(context: &JavaScriptBuilderContext<'_>) -> Vec<(String, String)> {
    let query_prefix = format!("{EVIDENCE_QUERY_PREFIX}{EVIDENCE_SEPARATOR}");
    let mut parameters: BTreeMap<String, String> = BTreeMap::new();

    for (key, value) in context.data().evidence().iter() {
        if !key.starts_with(&query_prefix) {
            continue;
        }
        // Evidence keys are lowercased, and so are the excluded keys.
        if key.eq_ignore_ascii_case(EVIDENCE_SESSIONID)
            || key.eq_ignore_ascii_case(EVIDENCE_SEQUENCE)
        {
            continue;
        }
        let field = &key[query_prefix.len()..];
        let encoded_key = utf8_percent_encode(field, URL_ENCODE_SET).to_string();
        let encoded_value = utf8_percent_encode(value, URL_ENCODE_SET).to_string();
        parameters.insert(encoded_key, encoded_value);
    }

    parameters.into_iter().collect()
}

/// The sequence from the Sequence element's data, which has already added
/// one to any `query.sequence` evidence, as the specification says. Without a
/// Sequence element the `query.sequence` evidence is read instead. `1` when
/// neither holds a positive 32-bit integer.
pub fn get_sequence(context: &JavaScriptBuilderContext<'_>) -> i32 {
    let data = context.data();
    let sequence = data
        .get(SequenceElement::KEY)
        .and_then(|sequence| sequence.sequence())
        .and_then(|sequence| i32::try_from(sequence).ok())
        .or_else(|| {
            data.evidence()
                .get(EVIDENCE_SEQUENCE)
                .and_then(|s| s.trim().parse::<i32>().ok())
        })
        .unwrap_or(1);
    safe_sequence(sequence)
}

/// The session id from the Sequence element's data. Without a Sequence
/// element the `query.session-id` evidence is read instead. An empty string
/// when neither holds 1 to 64 ASCII letters, digits and hyphens.
///
/// The specification takes the session id from the Sequence element's data,
/// because on a first request there is no `query.session-id` evidence and the
/// Sequence element is what creates the id. Evidence cannot be changed once
/// processing starts, so the id the Sequence element creates is only in its
/// element data.
pub fn get_session_id(context: &JavaScriptBuilderContext<'_>) -> String {
    let data = context.data();
    let session_id = data
        .get(SequenceElement::KEY)
        .and_then(|sequence| sequence.session_id())
        .or_else(|| data.evidence().get(EVIDENCE_SESSIONID))
        .unwrap_or("");
    safe_session_id(session_id.to_owned())
}

/// True when the builder was given a cloud request engine whose licensed
/// products include 51Did, through
/// [`set_shared_cloud_request_engine`](crate::JavaScriptBuilderElementBuilder::set_shared_cloud_request_engine)
/// or
/// [`set_cloud_request_engine`](crate::JavaScriptBuilderElementBuilder::set_cloud_request_engine),
/// and false while the shared engine's discovery has not completed.
pub fn render_user_prompt(context: &JavaScriptBuilderContext<'_>) -> bool {
    context.element().user_prompt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::JavaScriptBuilderElement;
    use fiftyone_pipeline_core::{Evidence, FlowData, Pipeline};
    use std::sync::Arc;

    /// Build a flow data carrying the supplied evidence on a pipeline whose
    /// only element is the JavaScript builder, without processing it.
    fn flow_data_with(pairs: &[(&str, &str)]) -> FlowData {
        let mut builder = Evidence::builder();
        for (key, value) in pairs {
            builder = builder.add(*key, *value);
        }
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(JavaScriptBuilderElement::new()))
            .build()
            .expect("pipeline builds");
        pipeline.create_flow_data_with(builder.build())
    }

    /// The default parameters for the supplied evidence.
    fn parameters(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        let element = JavaScriptBuilderElement::new();
        let data = flow_data_with(pairs);
        get_parameters(&JavaScriptBuilderContext::new(&element, &data))
    }

    /// The value of one parameter, if it is there.
    fn parameter<'a>(parameters: &'a [(String, String)], name: &str) -> Option<&'a str> {
        parameters
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn session_id_and_sequence_come_from_evidence() {
        let element = JavaScriptBuilderElement::new();
        let data = flow_data_with(&[("query.session-id", "abc-123"), ("query.sequence", "7")]);
        let context = JavaScriptBuilderContext::new(&element, &data);
        assert_eq!(get_session_id(&context), "abc-123");
        assert_eq!(get_sequence(&context), 7);
    }

    #[test]
    fn session_id_and_sequence_are_safe() {
        let element = JavaScriptBuilderElement::new();
        let data = flow_data_with(&[("query.session-id", "a\"b"), ("query.sequence", "-1")]);
        let context = JavaScriptBuilderContext::new(&element, &data);
        assert_eq!(get_session_id(&context), "");
        assert_eq!(get_sequence(&context), 1);
    }

    #[test]
    fn without_evidence_the_session_id_is_empty_and_the_sequence_one() {
        let element = JavaScriptBuilderElement::new();
        let data = flow_data_with(&[]);
        let context = JavaScriptBuilderContext::new(&element, &data);
        assert_eq!(get_session_id(&context), "");
        assert_eq!(get_sequence(&context), 1);
    }

    #[test]
    fn parameters_exclude_session_and_sequence() {
        let parameters = parameters(&[
            ("query.session-id", "abc"),
            ("query.sequence", "3"),
            ("query.user-agent", "test agent"),
            ("query.fod-js-object-name", "myObj"),
            ("header.host", "ignored"),
        ]);

        // Session id and sequence are excluded.
        assert_eq!(parameter(&parameters, "session-id"), None);
        assert_eq!(parameter(&parameters, "sequence"), None);
        // header.* evidence is excluded, as only query.* is taken.
        assert!(!parameters.iter().any(|(_, value)| value == "ignored"));
        // The remaining query.* keys are present with the prefix stripped, and
        // the space in the value is URL-encoded as %20 (not '+').
        assert_eq!(parameter(&parameters, "user-agent"), Some("test%20agent"));
        assert_eq!(parameter(&parameters, "fod-js-object-name"), Some("myObj"));
        assert_eq!(parameters.len(), 2);
    }

    #[test]
    fn parameters_are_in_name_order() {
        let parameters = parameters(&[("query.b", "2"), ("query.a", "1"), ("query.c", "3")]);
        let names: Vec<&str> = parameters.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]);
    }

    #[test]
    fn the_user_prompt_is_off_without_a_cloud_request_engine() {
        let element = JavaScriptBuilderElement::new();
        let data = flow_data_with(&[]);
        assert!(!render_user_prompt(&JavaScriptBuilderContext::new(
            &element, &data
        )));
    }
}
