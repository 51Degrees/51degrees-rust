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

//! The render context for the bundled Mustache template.
//!
//! This packages the twelve parameters the template expects and renders them
//! with the crate's small [`crate::mustache`] renderer, implementing its [`Context`]
//! trait so that HTML escaping is disabled for every field.

use crate::constants::MISSING_JSON_OBJECT;
use crate::mustache::{Context, Template, Value};

/// The longest session id the script is given, in bytes.
const MAX_SESSION_ID_LENGTH: usize = 64;

/// The session id to render, which is the one given when it is safe and an
/// empty string when it is not.
///
/// The template writes the session id inside quotes without any escaping, so
/// a value holding a quote, a backslash or a line break would end the string
/// early and break the script or change what it does. The
/// [javascript-builder specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/javascript-builder.md)
/// says a session id that is not 1 to 64 ASCII letters, digits and hyphens is
/// rendered as an empty string.
pub(crate) fn safe_session_id(session_id: String) -> String {
    let safe = !session_id.is_empty()
        && session_id.len() <= MAX_SESSION_ID_LENGTH
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    if safe {
        session_id
    } else {
        String::new()
    }
}

/// The sequence to render, which is the one given when it is positive and `1`
/// when it is not.
///
/// The template writes the sequence as bare code, and the script counts its
/// own requests up from it, so a value of zero or less is not a sequence the
/// script can use. The specification says such a value is rendered as `1`.
pub(crate) fn safe_sequence(sequence: i32) -> i32 {
    if sequence > 0 {
        sequence
    } else {
        1
    }
}

/// The parameters required by the `JavaScriptResource.mustache` template.
///
/// The field names match the Mustache variable names exactly (`_objName`,
/// `_jsonObject`, ...). Every string field is emitted without HTML escaping so
/// the JSON payload, the callback URL and the object name reach the client
/// verbatim.
///
/// Construct it with [`JavaScriptResource::new`], which applies the
/// missing-JSON fallback, then render it through
/// [`JavaScriptResource::render`].
#[derive(Debug, Clone)]
pub struct JavaScriptResource {
    /// The name of the global-scope object the client JavaScript creates.
    obj_name: String,
    /// The JSON data payload inserted into the template.
    json_object: String,
    /// The session id used in the JavaScript response.
    session_id: String,
    /// The sequence value used in the JavaScript response.
    sequence: i32,
    /// Whether to produce JavaScript that uses promises.
    supports_promises: bool,
    /// Whether to produce JavaScript that uses the fetch API.
    supports_fetch: bool,
    /// The callback URL, empty when no valid URL could be built.
    url: String,
    /// The request parameters appended to the callback URL, as a JSON object.
    parameters: String,
    /// Whether client-side processing stores results in cookies.
    enable_cookies: bool,
    /// Whether the background callback mechanism is enabled.
    update_enabled: bool,
    /// Whether the payload contains delayed-execution JavaScript properties.
    has_delayed_properties: bool,
    /// Whether the script carries the section that gathers the visitor's
    /// answer a 51Did is created from.
    user_prompt: bool,
}

impl JavaScriptResource {
    /// Build a render context.
    ///
    /// `json_object` is replaced with the missing-JSON placeholder when it is
    /// empty or whitespace. `session_id` is replaced with an empty string
    /// unless it is 1 to 64 ASCII letters, digits and hyphens, and `sequence`
    /// with `1` unless it is positive, because the template writes the
    /// session id inside quotes and the sequence as bare code.
    /// `url` is the already-built callback URL (empty string when none could be
    /// formed). `update_enabled` should be `true` only when that URL is present.
    /// `user_prompt` should be `true` only when the pipeline can return a
    /// 51Did, as
    /// [`JavaScriptBuilderSteps::render_user_prompt`](crate::JavaScriptBuilderSteps::render_user_prompt)
    /// decides. The section it controls sits inside the update section, so it
    /// renders only when `update_enabled` is also `true`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        obj_name: impl Into<String>,
        json_object: impl Into<String>,
        session_id: impl Into<String>,
        sequence: i32,
        supports_promises: bool,
        supports_fetch: bool,
        url: impl Into<String>,
        parameters: impl Into<String>,
        enable_cookies: bool,
        update_enabled: bool,
        has_delayed_properties: bool,
        user_prompt: bool,
    ) -> Self {
        let json_object = json_object.into();
        let json_object = if json_object.trim().is_empty() {
            MISSING_JSON_OBJECT.to_owned()
        } else {
            json_object
        };
        JavaScriptResource {
            obj_name: obj_name.into(),
            json_object,
            session_id: safe_session_id(session_id.into()),
            sequence: safe_sequence(sequence),
            supports_promises,
            supports_fetch,
            url: url.into(),
            parameters: parameters.into(),
            enable_cookies,
            update_enabled,
            has_delayed_properties,
            user_prompt,
        }
    }

    /// Render the supplied template with this context, with HTML escaping
    /// disabled for every field.
    ///
    /// Rendering into a `String` is infallible, so this returns the rendered
    /// content directly.
    pub fn render(&self, template: &Template) -> String {
        template.render(self)
    }
}

impl Context for JavaScriptResource {
    fn value(&self, name: &str) -> Option<Value<'_>> {
        // Every field is emitted verbatim (no HTML escaping). The boolean fields
        // are only used
        // as sections, but answering them here as their lowercase JavaScript
        // spelling keeps the context complete.
        match name {
            "_objName" => Some(Value::Str(&self.obj_name)),
            "_jsonObject" => Some(Value::Str(&self.json_object)),
            "_sessionId" => Some(Value::Str(&self.session_id)),
            // The sequence is a bare integer in the template
            // (`var sequence = {{&_sequence}};`).
            "_sequence" => Some(Value::Int(self.sequence as i64)),
            "_url" => Some(Value::Str(&self.url)),
            "_parameters" => Some(Value::Str(&self.parameters)),
            "_supportsPromises" => Some(Value::Str(bool_str(self.supports_promises))),
            "_supportsFetch" => Some(Value::Str(bool_str(self.supports_fetch))),
            "_enableCookies" => Some(Value::Str(bool_str(self.enable_cookies))),
            "_updateEnabled" => Some(Value::Str(bool_str(self.update_enabled))),
            "_hasDelayedProperties" => Some(Value::Str(bool_str(self.has_delayed_properties))),
            "_userPrompt" => Some(Value::Str(bool_str(self.user_prompt))),
            _ => None,
        }
    }

    fn flag(&self, name: &str) -> Option<bool> {
        match name {
            "_supportsPromises" => Some(self.supports_promises),
            "_supportsFetch" => Some(self.supports_fetch),
            "_enableCookies" => Some(self.enable_cookies),
            "_updateEnabled" => Some(self.update_enabled),
            "_hasDelayedProperties" => Some(self.has_delayed_properties),
            // The renderer treats a name with no arm here as unknown and omits
            // the section, so this arm is what lets the section render.
            "_userPrompt" => Some(self.user_prompt),
            _ => None,
        }
    }
}

/// The lowercase JavaScript spelling of a boolean.
fn bool_str(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(json: &str) -> JavaScriptResource {
        JavaScriptResource::new(
            "fod",
            json,
            "sid",
            1,
            false,
            false,
            "https://h/json",
            "{}",
            true,
            true,
            false,
            false,
        )
    }

    #[test]
    fn missing_json_falls_back_to_placeholder() {
        let resource = resource("   ");
        assert_eq!(resource.json_object, MISSING_JSON_OBJECT);
    }

    #[test]
    fn fields_are_not_html_escaped() {
        // A JSON payload with characters that would be HTML-escaped by default.
        let source = "var json = {{&_jsonObject}}; var name = \"{{_objName}}\";";
        let template = Template::parse(source).unwrap();
        let resource = JavaScriptResource::new(
            "ob<j>",
            "{\"a\":\"<b>&\\\"\"}",
            "sid",
            1,
            false,
            false,
            "",
            "{}",
            true,
            false,
            false,
            false,
        );
        let rendered = resource.render(&template);
        // No HTML entities anywhere: the '<', '>', '&' and '"' survive verbatim.
        assert!(!rendered.contains("&lt;"));
        assert!(!rendered.contains("&gt;"));
        assert!(!rendered.contains("&amp;"));
        assert!(!rendered.contains("&quot;"));
        assert!(rendered.contains("ob<j>"));
        assert!(rendered.contains("{\"a\":\"<b>&\\\"\"}"));
    }

    #[test]
    fn boolean_sections_render_their_body() {
        let source =
            "{{#_enableCookies}}YES{{/_enableCookies}}{{^_enableCookies}}NO{{/_enableCookies}}";
        let template = Template::parse(source).unwrap();

        let cookies_on = JavaScriptResource::new(
            "fod", "{}", "", 1, false, false, "", "{}", true, false, false, false,
        );
        assert_eq!(cookies_on.render(&template), "YES");

        let cookies_off = JavaScriptResource::new(
            "fod", "{}", "", 1, false, false, "", "{}", false, false, false, false,
        );
        assert_eq!(cookies_off.render(&template), "NO");
    }

    #[test]
    fn sequence_renders_as_bare_integer() {
        let source = "var sequence = {{&_sequence}};";
        let template = Template::parse(source).unwrap();
        let resource = JavaScriptResource::new(
            "fod", "{}", "", 7, false, false, "", "{}", true, false, false, false,
        );
        assert_eq!(resource.render(&template), "var sequence = 7;");
    }

    #[test]
    fn user_prompt_section_renders_only_when_set() {
        // The same nesting as the template, inside the update section.
        let source = "{{#_updateEnabled}}{{#_userPrompt}}PROMPT{{/_userPrompt}}{{/_updateEnabled}}";
        let template = Template::parse(source).unwrap();

        let entitled = JavaScriptResource::new(
            "fod", "{}", "", 1, false, false, "u", "{}", true, true, false, true,
        );
        assert_eq!(entitled.render(&template), "PROMPT");

        let unentitled = JavaScriptResource::new(
            "fod", "{}", "", 1, false, false, "u", "{}", true, true, false, false,
        );
        assert_eq!(unentitled.render(&template), "");

        // Updates off leaves the section out even for an entitled key.
        let no_updates = JavaScriptResource::new(
            "fod", "{}", "", 1, false, false, "", "{}", true, false, false, true,
        );
        assert_eq!(no_updates.render(&template), "");
    }

    #[test]
    fn safe_session_id_keeps_a_safe_id() {
        for id in ["abc-123", "a", &"a".repeat(64), "0", "A-0-z"] {
            assert_eq!(safe_session_id(id.to_owned()), id, "{id:?} was emptied");
        }
    }

    #[test]
    fn safe_session_id_empties_anything_else() {
        for id in [
            "",
            "a\"b",
            "a\\b",
            "</script>",
            "a b",
            "ab\n",
            "caf\u{e9}",
            &"a".repeat(65),
        ] {
            assert_eq!(safe_session_id(id.to_owned()), "", "{id:?} was kept");
        }
    }

    #[test]
    fn safe_sequence_keeps_a_positive_number() {
        for sequence in [1, 2, i32::MAX] {
            assert_eq!(safe_sequence(sequence), sequence);
        }
    }

    #[test]
    fn safe_sequence_replaces_anything_else_with_one() {
        for sequence in [0, -1, i32::MIN] {
            assert_eq!(safe_sequence(sequence), 1);
        }
    }

    #[test]
    fn an_unsafe_session_id_and_sequence_are_rendered_safe() {
        let template = Template::parse("\"{{&_sessionId}}\" {{&_sequence}}").unwrap();
        let unsafe_values = JavaScriptResource::new(
            "fod", "{}", "a\"b", 0, false, false, "", "{}", true, false, false, false,
        );
        assert_eq!(unsafe_values.render(&template), "\"\" 1");
        let safe_values = JavaScriptResource::new(
            "fod", "{}", "abc-123", 2, false, false, "", "{}", true, false, false, false,
        );
        assert_eq!(safe_values.render(&template), "\"abc-123\" 2");
    }
}
