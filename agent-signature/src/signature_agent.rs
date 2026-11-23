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

//! Reads the `Signature-Agent` header, whose members say where each agent's
//! keys are.

use url::Url;

use crate::codec;
use crate::constants::{
    AGENT_TYPE_CIMD, AGENT_TYPE_DIRECTORY, AGENT_TYPE_JWKS_URI, DIRECTORY_MEDIA_TYPE,
    DIRECTORY_PATH,
};
use crate::structured_field::{parse_dictionary, parse_item, Item, Member};

/// The text before the media type of a `data:` URI.
const DATA_SCHEME_PREFIX: &str = "data:";

/// The text that marks a `data:` URI's content as base64.
const BASE64_MARKER: &str = ";base64";

/// One `Signature-Agent` member.
#[derive(Debug, Clone)]
pub(crate) struct SignatureAgentEntry {
    /// The dictionary key, empty for the older bare string form.
    pub(crate) label: String,
    /// The member value exactly as the agent sent it.
    pub(crate) value: String,
    /// One of `directory`, `jwks_uri` and `cimd`.
    pub(crate) agent_type: String,
    /// The address the keys are fetched from, absent for an inline key set.
    pub(crate) key_url: Option<String>,
    /// A key set carried in a `data:` URI.
    pub(crate) inline_directory: Option<Vec<u8>>,
}

/// Read the header. Returns `None` where the header, or any member of it,
/// cannot be read, which reports `Malformed`.
pub(crate) fn parse_signature_agents(
    header: &str,
    allow_legacy_form: bool,
) -> Option<Vec<SignatureAgentEntry>> {
    let trimmed = header.trim();
    if trimmed.starts_with('"') {
        // The bare quoted string form the earlier drafts used.
        if !allow_legacy_form {
            return None;
        }
        let item = parse_item(trimmed)?;
        return Some(vec![create(String::new(), &item)?]);
    }
    parse_dictionary(header)?
        .members()
        .iter()
        .map(|(label, member)| match member {
            Member::Item(item) => create(label.clone(), item),
            Member::InnerList(..) => None,
        })
        .collect()
}

fn create(label: String, item: &Item) -> Option<SignatureAgentEntry> {
    let value = item.as_string()?;
    let agent_type = item
        .string_parameter("type")
        .unwrap_or(AGENT_TYPE_DIRECTORY);
    if !matches!(
        agent_type,
        AGENT_TYPE_DIRECTORY | AGENT_TYPE_JWKS_URI | AGENT_TYPE_CIMD
    ) {
        return None;
    }
    let url = Url::parse(value).ok()?;
    // A directory may be carried inline in a data: URI, which needs no
    // fetch at all.
    if url.scheme() == "data" {
        if agent_type != AGENT_TYPE_DIRECTORY {
            return None;
        }
        return Some(SignatureAgentEntry {
            label,
            value: value.to_owned(),
            agent_type: agent_type.to_owned(),
            key_url: None,
            inline_directory: Some(decode_data_uri(value)?),
        });
    }
    // A key fetched over plain HTTP proves nothing about the agent, so HTTPS
    // is required even though the protocol draft allows HTTP.
    if url.scheme() != "https" {
        return None;
    }
    let key_url = if agent_type == AGENT_TYPE_DIRECTORY {
        // The member carries an origin and nothing else. The well known path
        // is joined on as text, so a member ending in '?' or '#' would
        // otherwise turn it into a query or a fragment and let the sender
        // choose the whole address fetched.
        if url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return None;
        }
        format!("{}{DIRECTORY_PATH}", url.origin().ascii_serialization())
    } else {
        value.to_owned()
    };
    Some(SignatureAgentEntry {
        label,
        value: value.to_owned(),
        agent_type: agent_type.to_owned(),
        key_url: Some(key_url),
        inline_directory: None,
    })
}

/// The content of a `data:` URI, which must state the key directory media
/// type, because an empty one means `text/plain`.
fn decode_data_uri(value: &str) -> Option<Vec<u8>> {
    let comma = value.find(',')?;
    let mut metadata = value.get(DATA_SCHEME_PREFIX.len()..comma)?;
    let payload = &value[comma + 1..];
    let is_base64 = ends_with_ignore_case(metadata, BASE64_MARKER);
    if is_base64 {
        metadata = &metadata[..metadata.len() - BASE64_MARKER.len()];
    }
    if !starts_with_ignore_case(metadata, DIRECTORY_MEDIA_TYPE) {
        return None;
    }
    if is_base64 {
        codec::decode_standard(payload)
    } else {
        Some(percent_encoding::percent_decode_str(payload).collect())
    }
}

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len()
        && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn ends_with_ignore_case(text: &str, suffix: &str) -> bool {
    text.len() >= suffix.len()
        && text.as_bytes()[text.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_member_resolves_to_the_well_known_path() {
        let agents = parse_signature_agents("agent1=\"https://Signer.Example.com\"", true).unwrap();
        assert_eq!(agents[0].label, "agent1");
        assert_eq!(agents[0].value, "https://Signer.Example.com");
        assert_eq!(
            agents[0].key_url.as_deref(),
            Some("https://signer.example.com/.well-known/http-message-signatures-directory")
        );
    }

    #[test]
    fn a_directory_member_must_be_a_bare_https_origin() {
        for header in [
            "a=\"https://signer.example/keys\"",
            "a=\"https://signer.example/?\"",
            "a=\"https://signer.example/#\"",
            "a=\"https://user@signer.example\"",
            "a=\"http://signer.example\"",
            "a=\"https://signer.example\";type=other",
            "a=(\"https://signer.example\")",
        ] {
            assert!(parse_signature_agents(header, true).is_none(), "{header}");
        }
    }

    #[test]
    fn a_jwks_uri_member_is_fetched_as_given() {
        let agents =
            parse_signature_agents("a=\"https://signer.example/k?x=1\";type=jwks_uri", true)
                .unwrap();
        assert_eq!(agents[0].agent_type, AGENT_TYPE_JWKS_URI);
        assert_eq!(
            agents[0].key_url.as_deref(),
            Some("https://signer.example/k?x=1")
        );
    }

    #[test]
    fn the_older_bare_string_form_is_read_only_where_allowed() {
        let agents = parse_signature_agents(" \"https://signer.example\"", true).unwrap();
        assert_eq!(agents[0].label, "");
        assert!(parse_signature_agents("\"https://signer.example\"", false).is_none());
    }

    #[test]
    fn an_inline_directory_needs_the_directory_media_type() {
        let header = "a=\"data:application/http-message-signatures-directory+json;base64,e30=\"";
        let agents = parse_signature_agents(header, true).unwrap();
        assert_eq!(agents[0].inline_directory.as_deref(), Some(&b"{}"[..]));
        assert!(agents[0].key_url.is_none());
        assert!(parse_signature_agents("a=\"data:,%7B%7D\"", true).is_none());
    }
}
