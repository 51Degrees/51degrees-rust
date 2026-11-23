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

//! Shared HTML and static-asset helpers for the 51Degrees Rust web examples.
//!
//! The Device Detection and IP Intelligence web examples render different result
//! pages (device properties versus weighted location, a client-hints round trip
//! versus a location map), but they share the same scaffolding: the vendored
//! 51Degrees example design-system assets, the routes and handlers that serve
//! them, HTML escaping, and the standard `.c-eg-table` two-column table. Those
//! shared pieces live here, in one place, so each example crate keeps only its
//! product-specific renderers.
//!
//! The crate is presentation only and depends on nothing but `axum` (for the
//! asset-serving [`Response`]). It does not touch the detection facades, so it
//! adds no build cost to console examples.

#![warn(missing_docs)]

use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

/// The route the vendored CSS is served from.
pub const ASSETS_CSS_ROUTE: &str = "/static/examples-main.min.css";
/// The route the vendored JavaScript helper is served from.
pub const ASSETS_JS_ROUTE: &str = "/static/examples.min.js";

/// The vendored 51Degrees example stylesheet, embedded so a binary that serves
/// it is self-contained. A single copy lives in this crate's `assets` directory
/// and is shared by every web example.
const EXAMPLES_CSS: &str = include_str!("../assets/examples-main.min.css");
/// The vendored 51Degrees example JavaScript helper. It binds the detection
/// callback used by the Device Detection pages and provides
/// `fodExamples.initLocationMap` used by the IP Intelligence map.
const EXAMPLES_JS: &str = include_str!("../assets/examples.min.js");

/// Serve the vendored stylesheet with a `text/css` content type.
pub async fn serve_css() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/css")],
        EXAMPLES_CSS,
    )
        .into_response()
}

/// Serve the vendored JavaScript helper with a JavaScript content type.
pub async fn serve_js() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/javascript")],
        EXAMPLES_JS,
    )
        .into_response()
}

/// Wrap pre-rendered `<tr>` rows in a standard two-column `.c-eg-table` with the
/// given column headings.
pub fn two_column_table(rows: &str, key_heading: &str, value_heading: &str) -> String {
    format!(
        "<table class=\"c-eg-table\">\
         <thead class=\"c-eg-table__head\"><tr class=\"c-eg-table__row\">\
         <th class=\"c-eg-table__cell\">{}</th>\
         <th class=\"c-eg-table__cell\">{}</th></tr></thead>\
         <tbody>{rows}</tbody></table>",
        html_escape(key_heading),
        html_escape(value_heading),
    )
}

/// Render the evidence the pipeline received as a two-column `.c-eg-table`. Every
/// supplied pair is shown as "used" because it reached the pipeline's evidence
/// filter.
pub fn evidence_table(evidence: &[(String, String)]) -> String {
    let mut rows = String::new();
    for (key, value) in evidence {
        rows.push_str(&format!(
            "<tr class=\"c-eg-table__row c-eg-table__row--used\">\
             <td class=\"c-eg-table__cell c-eg-table__cell--key\">{}</td>\
             <td class=\"c-eg-table__cell\">{}</td></tr>",
            html_escape(key),
            html_escape(value)
        ));
    }
    two_column_table(&rows, "Key", "Value")
}

/// Escape the five HTML-significant characters so dynamic values cannot break
/// the markup. Small and dependency-free, which suits an example.
pub fn html_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escape_covers_the_five_characters() {
        assert_eq!(
            html_escape("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&#39;f"
        );
        // A plain string is returned unchanged.
        assert_eq!(html_escape("plain text 123"), "plain text 123");
    }

    #[test]
    fn two_column_table_uses_the_given_headings() {
        let table = two_column_table("<tr></tr>", "Property", "Value");
        assert!(table.contains("c-eg-table"));
        assert!(table.contains(">Property<"));
        assert!(table.contains(">Value<"));
        assert!(table.contains("<tbody><tr></tr></tbody>"));
    }

    #[test]
    fn evidence_table_marks_each_pair_used() {
        let evidence = vec![
            ("header.user-agent".to_owned(), "Mozilla/5.0".to_owned()),
            ("query.user-agent".to_owned(), "<script>".to_owned()),
        ];
        let table = evidence_table(&evidence);
        // Two used rows, the value escaped.
        assert_eq!(table.matches("c-eg-table__row--used").count(), 2);
        assert!(table.contains("&lt;script&gt;"));
        assert!(!table.contains("<script>"));
        // Standard Key/Value headings.
        assert!(table.contains(">Key<"));
        assert!(table.contains(">Value<"));
    }
}
