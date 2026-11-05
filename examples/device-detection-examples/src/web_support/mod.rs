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

//! HTML helpers for the Device Detection web examples.
//!
//! This module is pulled into each `dd-web-*` binary with a `#[path = ...]`
//! attribute. It is deliberately not a `src/bin` file (so Cargo does not build
//! it as its own binary) and not part of the crate library, so several web bins
//! can share the same device-detection markup without editing `lib.rs` or
//! fighting over a single file.
//!
//! The scaffolding shared with the IP Intelligence examples (the vendored
//! design-system assets and their serving handlers, [`html_escape`],
//! [`two_column_table`] and [`evidence_table`]) lives in the `examples-web-shared`
//! crate and is re-exported here so the binaries import everything from one
//! place. What remains in this module is device-detection specific: the
//! results table, the response-header table, the "Find out more" section and
//! the page renderers.
//!
//! Because the module is included into each binary separately, any given binary
//! uses only a subset of these helpers (whether defined here or re-exported from
//! `examples-web-shared`). `dead_code` and `unused_imports` are therefore allowed
//! at the module level so a binary that does not call one of them still builds
//! cleanly.

#![allow(dead_code, unused_imports)]

pub use examples_web_shared::{
    evidence_table, html_escape, serve_css, serve_js, two_column_table, ASSETS_CSS_ROUTE,
    ASSETS_JS_ROUTE,
};
use fiftyone_pipeline_core::ElementData;

/// Where the on-premise pages send a reader to ask about a copy of the
/// Enterprise data file.
pub const ENTERPRISE_CONTACT_URL: &str = "https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-web_support-mod.rs&utm_term=enterprise-data-file";

/// The 51degrees.com links every page's "Find out more" section lists.
const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("Device Detection", "https://51degrees.com/device-detection?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-web_support-mod.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-web_support-mod.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-web_support-mod.rs&utm_term=find-out-more"),
];

/// The device-detection properties the example pages display, as
/// `(label, property-name)` pairs.
const DISPLAY_PROPERTIES: &[(&str, &str)] = &[
    ("Hardware Vendor", "hardwarevendor"),
    ("Hardware Name", "hardwarename"),
    ("Hardware Model", "hardwaremodel"),
    ("Device Type", "devicetype"),
    ("Price Band", "priceband"),
    ("Platform Vendor", "platformvendor"),
    ("Platform Name", "platformname"),
    ("Platform Version", "platformversion"),
    ("Browser Vendor", "browservendor"),
    ("Browser Name", "browsername"),
    ("Browser Version", "browserversion"),
    ("Screen width (pixels)", "screenpixelswidth"),
    ("Screen height (pixels)", "screenpixelsheight"),
    ("Screen diagonal (inches)", "screeninchesdiagonal"),
    ("Device Id", "deviceid"),
];

/// Whether `property` is one the pages display that only the Enterprise data
/// file carries, being those in the crate's list plus HardwareName, which the
/// free Lite file does not carry either.
fn enterprise_only(property: &str) -> bool {
    property.eq_ignore_ascii_case("HardwareName")
        || device_detection_examples::ENTERPRISE_PROPERTIES
            .iter()
            .any(|name| name.eq_ignore_ascii_case(property))
}

/// Render the server-side detection results as a `.c-eg-table`.
///
/// `device` is the device element data read from the flow data. Each displayed
/// property is rendered through [`examples_shared::get_property_as_string`], so a
/// missing or no-value property shows the standard marker rather than failing.
///
/// `available` is the on-premise engine's properties, which are those its data
/// file carries, or `None` for the cloud. An Enterprise property the data file
/// does not carry says it needs the Enterprise data file, with a link to ask
/// about a copy.
pub fn detection_results_table(device: &dyn ElementData, available: Option<&[String]>) -> String {
    let mut rows = String::new();
    for (index, (label, property)) in DISPLAY_PROPERTIES.iter().enumerate() {
        let value = match available {
            Some(available)
                if enterprise_only(property)
                    && !examples_shared::is_available(property, available) =>
            {
                format!(
                    "{} (<a href=\"{ENTERPRISE_CONTACT_URL}\">contact us</a> about a copy)",
                    examples_shared::NEEDS_ENTERPRISE_DATA_FILE
                )
            }
            _ => html_escape(&examples_shared::get_property_as_string(device, property)),
        };
        let alt = if index % 2 == 0 {
            " c-eg-table__row--alt"
        } else {
            ""
        };
        rows.push_str(&format!(
            "<tr class=\"c-eg-table__row{alt}\">\
             <td class=\"c-eg-table__cell c-eg-table__cell--key\">{}</td>\
             <td class=\"c-eg-table__cell\">{}</td></tr>",
            html_escape(label),
            value
        ));
    }
    two_column_table(&rows, "Key", "Value")
}

/// The "Find out more" section that ends every page, set apart by a rule. It
/// lists the 51degrees.com links, then each of `repositories` as its address.
pub fn find_out_more_html(repositories: &[&str]) -> String {
    let mut items = String::new();
    for (label, url) in FIND_OUT_MORE {
        items.push_str(&format!(
            "<li><a href=\"{url}\">{}</a></li>",
            html_escape(label)
        ));
    }
    for url in repositories {
        items.push_str(&format!("<li><a href=\"{url}\">{url}</a></li>"));
    }
    format!(
        "<hr><h3 class=\"c-eg-page__heading\">{}</h3><ul>{items}</ul>",
        examples_shared::FIND_OUT_MORE_HEADING
    )
}

/// Render the set-headers response headers (such as `Accept-CH`) as a
/// `.c-eg-table`.
pub fn response_header_table(headers: &[(String, String)]) -> String {
    if headers.is_empty() {
        return "<p class=\"c-eg-page__lead\">No extra response headers were requested.</p>"
            .to_owned();
    }
    let mut rows = String::new();
    for (key, value) in headers {
        rows.push_str(&format!(
            "<tr class=\"c-eg-table__row c-eg-table__row--present\">\
             <td class=\"c-eg-table__cell c-eg-table__cell--key\">{}</td>\
             <td class=\"c-eg-table__cell\">{}</td></tr>",
            html_escape(key),
            html_escape(value)
        ));
    }
    two_column_table(&rows, "Key", "Value")
}

/// The contact-us banner shown on cloud pages, inviting the reader to discuss
/// an on-premise deployment.
pub const CLOUD_CONTACT_BANNER: &str = "\
<div class=\"c-eg-message\">\
  <p class=\"c-eg-message__text\">Want to try on-premise? \
  <a href=\"https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-web_support-mod.rs&utm_term=on-premise\">Contact us</a> to discuss requirements. \
  <a href=\"https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-web_support-mod.rs&utm_term=on-premise\">See pricing</a>.</p>\
  <a class=\"b-btn c-eg-message__cta\" href=\"https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-web_support-mod.rs&utm_term=on-premise\">Contact us</a>\
</div>";

/// The contact-us banner shown on the on-premise Lite page, listing the benefits
/// of a paid data file.
pub const ONPREM_CONTACT_BANNER: &str = "\
<div class=\"c-eg-message\">\
  <p class=\"c-eg-message__text\">This example uses the free Lite data file, which detects only \
  a limited set of properties. A paid data file adds thousands of device models, hardware vendor \
  and model, finer platform and browser detail and Apple-model resolution. \
  <a href=\"https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-web_support-mod.rs&utm_term=data-file-benefits\">See pricing</a> or \
  <a href=\"https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-web_support-mod.rs&utm_term=data-file-benefits\">contact us</a>.</p>\
  <a class=\"b-btn c-eg-message__cta\" href=\"https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-device-detection-examples-src-web_support-mod.rs&utm_term=data-file-benefits\">See pricing</a>\
</div>";

/// The alert shown when the response carries no `Accept-CH` header, which usually
/// means the browser does not support User-Agent Client Hints.
pub const MISSING_ACCEPT_CH_ALERT: &str = "\
<div class=\"c-eg-alert\">WARNING: There is no Accept-CH header in the response. This may indicate \
that your browser does not support User-Agent Client Hints. If you want to try detection using \
client hints, make sure your browser \
<a href=\"https://developer.mozilla.org/en-US/docs/Web/API/User-Agent_Client_Hints_API#browser_compatibility\">supports them</a>.</div>";

/// The client bootstrap that loads the server-mounted `/51Degrees.core.js` and
/// binds the shared callback so the results table refreshes once the browser has
/// posted high-entropy client evidence back to the JSON endpoint.
pub const SERVER_CLIENT_SCRIPT: &str = "\
<script async src=\"/51Degrees.core.js\" type=\"text/javascript\"></script>\
<script src=\"/static/examples.min.js\"></script>\
<script>window.onload = function () { fodExamples.bindDeviceCallback({ targetId: \"content\" }); };</script>";

/// The fields a [`render_page`] call interpolates. Carrying them in a struct
/// keeps the renderer's signature stable as pages differ in their banners and
/// alerts.
pub struct PageOptions<'a> {
    /// The page `<h2>` title.
    pub title: &'a str,
    /// The introductory lead paragraph.
    pub lead: &'a str,
    /// A `c-eg-alert` shown at the very top (used for the on-premise stale-data
    /// warning). Empty string for no top alert.
    pub top_alert: &'a str,
    /// The pre-rendered server-side results table (or a placeholder).
    pub results_html: &'a str,
    /// The pre-rendered evidence table.
    pub evidence_html: &'a str,
    /// The pre-rendered response-header table.
    pub headers_html: &'a str,
    /// A `c-eg-alert` shown after the response headers (the missing-Accept-CH
    /// warning). Empty string for none.
    pub accept_ch_alert: &'a str,
    /// The contact-us banner HTML for this deployment.
    pub message_html: &'a str,
    /// The client bootstrap `<script>` block.
    pub client_script: &'a str,
    /// The repositories the "Find out more" section lists.
    pub repositories: &'a [&'a str],
}

/// Render a full HTML document for a server-plus-client web example page.
///
/// The structure is a titled page, a `#content` region holding the
/// detection-results, evidence-used and response-headers tables (the shared
/// JavaScript appends a refreshed results table here after the client round
/// trip), the contact-us banner, the "Find out more" section, and the client
/// bootstrap.
pub fn render_page(options: PageOptions<'_>) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title}</title>\
         <link rel=\"stylesheet\" href=\"{css}\"></head><body>\
         <div class=\"c-eg-page\">\
         <h2 class=\"c-eg-page__title\">{title}</h2>\
         <p class=\"c-eg-page__lead\">{lead}</p>\
         {top_alert}\
         <noscript><div class=\"c-eg-alert\">WARNING: JavaScript is disabled, so the client-side \
         callback will not run and User-Agent Client Hints will not be sent.</div></noscript>\
         <div id=\"content\">\
         <h3 class=\"c-eg-page__heading\">Detection results</h3>\
         <p class=\"c-eg-page__lead\">These values are from server-side device detection on the \
         first request.</p>\
         {results_html}\
         <h3 class=\"c-eg-page__heading\">Evidence used</h3>\
         {evidence_html}\
         <h3 class=\"c-eg-page__heading\">Response headers</h3>\
         {headers_html}\
         {accept_ch_alert}\
         <h3 class=\"c-eg-page__heading\">Client-side evidence</h3>\
         <p class=\"c-eg-page__lead\">The table below is filled after a callback to the server with \
         extra evidence gathered by JavaScript in the browser, including any requested client-hint \
         headers.</p>\
         </div>\
         {message_html}\
         {find_out_more}\
         </div>\
         {client_script}\
         </body></html>",
        title = html_escape(options.title),
        css = ASSETS_CSS_ROUTE,
        lead = html_escape(options.lead),
        top_alert = options.top_alert,
        results_html = options.results_html,
        evidence_html = options.evidence_html,
        headers_html = options.headers_html,
        accept_ch_alert = options.accept_ch_alert,
        message_html = options.message_html,
        find_out_more = find_out_more_html(options.repositories),
        client_script = options.client_script,
    )
}

/// Render a client-only page: no server-side tables, just the `#content` target
/// the client script fills, the contact-us banner, the "Find out more" section
/// listing `repositories`, and the client bootstrap.
pub fn render_client_only_page(
    title: &str,
    lead: &str,
    message_html: &str,
    client_script: &str,
    repositories: &[&str],
) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title}</title>\
         <link rel=\"stylesheet\" href=\"{css}\"></head><body>\
         <div class=\"c-eg-page\">\
         <h2 class=\"c-eg-page__title\">{title}</h2>\
         <p class=\"c-eg-page__lead\">{lead}</p>\
         <noscript><div class=\"c-eg-alert\">WARNING: JavaScript is disabled, so no detection will \
         run on this client-only page.</div></noscript>\
         <div id=\"content\"></div>\
         {message_html}\
         {find_out_more}\
         </div>\
         {client_script}\
         </body></html>",
        title = html_escape(title),
        css = ASSETS_CSS_ROUTE,
        lead = html_escape(lead),
        message_html = message_html,
        find_out_more = find_out_more_html(repositories),
        client_script = client_script,
    )
}
