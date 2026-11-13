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

/// The characters [`url_encode`] percent-encodes. Letters, digits,
/// `- _ . ! * ( )` and the space are left out, as .NET's
/// `WebUtility.UrlEncode` leaves them, and the space is then written as `+`.
const URL_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'*')
    .remove(b'(')
    .remove(b')')
    .remove(b' ');

/// Encode a parameter name or value as .NET's `WebUtility.UrlEncode` does.
/// A space becomes `+`, and every byte of the UTF-8 text other than a letter,
/// a digit or one of `- _ . ! * ( )` becomes `%` and two upper case hex
/// digits.
fn url_encode(text: &str) -> String {
    utf8_percent_encode(text, URL_ENCODE_SET)
        .to_string()
        .replace(' ', "+")
}

/// Every `query.` evidence value apart from the session id and sequence,
/// named by the text after `query.`, with the name and value encoded as
/// .NET's `WebUtility.UrlEncode` encodes them, in name order.
///
/// Evidence holds no order of its own, so the parameters are given in name
/// order, where .NET gives them in the order the evidence was added.
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
        parameters.insert(url_encode(field), url_encode(value));
    }

    parameters.into_iter().collect()
}

/// The sequence from the Sequence element's data, which has already added
/// one to any `query.sequence` evidence. `1` when no Sequence element ran,
/// it could not read the request's sequence, or its sequence is not
/// positive.
///
/// The specification says a pipeline with no Sequence element renders the
/// sequence as `1`, so the `query.sequence` evidence is never read here.
pub fn get_sequence(context: &JavaScriptBuilderContext<'_>) -> i32 {
    let sequence = context
        .data()
        .get(SequenceElement::KEY)
        .and_then(|sequence| sequence.sequence())
        .and_then(|sequence| i32::try_from(sequence).ok())
        .unwrap_or(1);
    safe_sequence(sequence)
}

/// The session id from the Sequence element's data. An empty string when no
/// Sequence element ran or its session id is not 1 to 64 ASCII letters,
/// digits and hyphens.
///
/// The specification says a pipeline with no Sequence element renders an
/// empty session id, so the `query.session-id` evidence is never read here.
pub fn get_session_id(context: &JavaScriptBuilderContext<'_>) -> String {
    let session_id = context
        .data()
        .get(SequenceElement::KEY)
        .and_then(|sequence| sequence.session_id())
        .unwrap_or("");
    safe_session_id(session_id.to_owned())
}

/// True when the pipeline has an element whose data key is `fodid`, ignoring
/// case, offering at least one available property, as .NET decides.
///
/// A builder given
/// [`set_user_prompt`](crate::JavaScriptBuilderElementBuilder::set_user_prompt)
/// gives that answer instead. A builder given a cloud request engine, through
/// [`set_shared_cloud_request_engine`](crate::JavaScriptBuilderElementBuilder::set_shared_cloud_request_engine)
/// or
/// [`set_cloud_request_engine`](crate::JavaScriptBuilderElementBuilder::set_cloud_request_engine),
/// gives true when the engine's licensed products include 51Did, and false
/// while the shared engine's discovery has not completed.
pub fn render_user_prompt(context: &JavaScriptBuilderContext<'_>) -> bool {
    context.element().user_prompt(context.data())
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

    /// Build a flow data carrying the supplied evidence and process it on a
    /// pipeline whose only element is the Sequence element, so the Sequence
    /// element's data is there to read.
    fn flow_data_after_sequence(pairs: &[(&str, &str)]) -> FlowData {
        let mut builder = Evidence::builder();
        for (key, value) in pairs {
            builder = builder.add(*key, *value);
        }
        let pipeline = Pipeline::builder()
            .add_element(Arc::new(SequenceElement::new()))
            .build()
            .expect("pipeline builds");
        let mut data = pipeline.create_flow_data_with(builder.build());
        data.process().expect("the Sequence element runs");
        data
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
    fn session_id_and_sequence_come_from_the_sequence_element() {
        let element = JavaScriptBuilderElement::new();
        let data =
            flow_data_after_sequence(&[("query.session-id", "abc-123"), ("query.sequence", "7")]);
        let context = JavaScriptBuilderContext::new(&element, &data);
        assert_eq!(get_session_id(&context), "abc-123");
        assert_eq!(get_sequence(&context), 8);
    }

    #[test]
    fn without_a_sequence_element_the_evidence_is_not_read() {
        let element = JavaScriptBuilderElement::new();
        let data = flow_data_with(&[("query.session-id", "abc-123"), ("query.sequence", "7")]);
        let context = JavaScriptBuilderContext::new(&element, &data);
        assert_eq!(get_session_id(&context), "");
        assert_eq!(get_sequence(&context), 1);
    }

    #[test]
    fn session_id_and_sequence_are_safe() {
        let element = JavaScriptBuilderElement::new();
        let data =
            flow_data_after_sequence(&[("query.session-id", "a\"b"), ("query.sequence", "-2")]);
        // The Sequence element keeps the session id as given, which is not
        // safe for the script, and reads -2 as no sequence, giving 1.
        let sequence = data.get(SequenceElement::KEY).expect("sequence data");
        assert_eq!(sequence.session_id(), Some("a\"b"));
        assert_eq!(sequence.sequence(), Some(1));
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
        // the space in the value is encoded as '+', as .NET encodes it.
        assert_eq!(parameter(&parameters, "user-agent"), Some("test+agent"));
        assert_eq!(parameter(&parameters, "fod-js-object-name"), Some("myObj"));
        assert_eq!(parameters.len(), 2);
    }

    #[test]
    fn parameter_names_and_values_are_encoded_as_dotnet_encodes_them() {
        // Each pair is the text and what .NET's WebUtility.UrlEncode gives
        // for it.
        for (text, encoded) in [
            ("test agent", "test+agent"),
            ("a+b", "a%2Bb"),
            ("a=b&c", "a%3Db%26c"),
            ("-_.!*()", "-_.!*()"),
            ("~", "%7E"),
            ("'", "%27"),
            ("\"", "%22"),
            ("/?#[]@", "%2F%3F%23%5B%5D%40"),
            ("%", "%25"),
            ("caf\u{e9}", "caf%C3%A9"),
            ("\u{20ac}", "%E2%82%AC"),
            ("\u{1f600}", "%F0%9F%98%80"),
            ("  ", "++"),
            ("", ""),
            ("x<y>z", "x%3Cy%3Ez"),
            ("^`{|}", "%5E%60%7B%7C%7D"),
            ("line\nbreak", "line%0Abreak"),
            ("tab\there", "tab%09here"),
            (",;:$", "%2C%3B%3A%24"),
        ] {
            assert_eq!(url_encode(text), encoded, "{text:?}");
        }

        let parameters = parameters(&[("query.a b", "c d")]);
        assert_eq!(parameter(&parameters, "a+b"), Some("c+d"));
    }

    #[test]
    fn parameters_are_in_name_order() {
        let parameters = parameters(&[("query.b", "2"), ("query.a", "1"), ("query.c", "3")]);
        let names: Vec<&str> = parameters.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]);
    }

    #[test]
    fn the_user_prompt_is_off_in_a_pipeline_with_no_51did_element() {
        let element = JavaScriptBuilderElement::new();
        let data = flow_data_with(&[]);
        assert!(!render_user_prompt(&JavaScriptBuilderContext::new(
            &element, &data
        )));
    }
}
