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

//! HTML helpers for the IP Intelligence web examples.
//!
//! This module is pulled into each `ipi-web-*` binary with a `#[path = ...]`
//! attribute. It is deliberately not a `src/bin` file (so Cargo does not build
//! it as its own binary) and not part of the crate library, so several web bins
//! can share the same markup without editing `lib.rs` or fighting over a single
//! file.
//!
//! The scaffolding shared with the Device Detection examples (the vendored
//! design-system assets and their serving handlers, [`html_escape`],
//! [`two_column_table`] and [`evidence_table`]) lives in the `examples-web-shared`
//! crate and is re-exported here so the binaries import everything from one
//! place. What remains in this module is IP-intelligence specific: the results
//! table, the location map, the Enterprise data file notes, the "Find out
//! more" section and the page renderer.
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
use fiftyone_ip_intelligence::{AspectPropertyValue, IpIntelligenceData, WeightedValue};

/// Where to ask about a copy of the Enterprise data file.
pub const ENTERPRISE_CONTACT_URL: &str = "https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-web_support-mod.rs&utm_term=enterprise-data-file";

/// The links the page's "Find out more" section starts with.
pub const FIND_OUT_MORE: &[(&str, &str)] = &[
    ("IP Intelligence", "https://51degrees.com/ip-intelligence?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-web_support-mod.rs&utm_term=find-out-more"),
    ("Pricing", "https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-web_support-mod.rs&utm_term=find-out-more"),
    ("Documentation", "https://51degrees.com/documentation/index.html?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-web_support-mod.rs&utm_term=find-out-more"),
];

/// One displayed IP-intelligence property reduced to its plain value, ready to
/// render. A property with no value carries the engine's explanation in `value`.
pub struct DisplayValue {
    /// The property's name, such as `CountryCode`.
    pub property: String,
    /// The human-readable label for the property row.
    pub label: String,
    /// The resolved value, or the no-value message.
    pub value: String,
}

/// Reduce a plain [`AspectPropertyValue`] to its display value, rendering the
/// value with the supplied closure. A no-value carries the engine's explanation.
fn display_value<T>(
    property: &str,
    label: &str,
    value: &AspectPropertyValue<T>,
    render: impl Fn(&T) -> String,
) -> DisplayValue {
    let value = match value.value() {
        Ok(value) => render(value),
        Err(error) => error.to_string(),
    };
    DisplayValue {
        property: property.to_owned(),
        label: label.to_owned(),
        value,
    }
}

/// Reduce a weighted [`AspectPropertyValue`] to its display value, rendering each
/// candidate as `value (weight)` ordered high weighting first and joined with
/// commas. A no-value carries the engine's explanation.
///
/// This is how the genuinely weighted properties (the country-code distributions
/// and `Mcc`) are shown: a single IP can overlap several countries, so the value
/// is a weighted list rather than one definite value.
fn display_weighted(
    property: &str,
    label: &str,
    value: &AspectPropertyValue<Vec<WeightedValue<String>>>,
) -> DisplayValue {
    display_value(property, label, value, |list| {
        list.iter()
            .map(|candidate| format!("{} ({:.2})", candidate.value, candidate.weighting()))
            .collect::<Vec<_>>()
            .join(", ")
    })
}

/// Gather the displayed IP-intelligence properties from the result, read through
/// the strongly-typed accessors. The order is the autonomous system and network
/// registration first, then the textual location, the coordinates and accuracy,
/// then the network flags and the weighted country-code distributions.
///
/// Plain properties (strings, the [`f32`] coordinates, the [`i32`] radii and the
/// [`bool`] flags) resolve to a single value. The weighted properties at the end
/// (`CountryCodesGeographical`, `CountryCodesPopulation`, `Mcc`) resolve to a
/// weighted list, shown as `value (weight)` pairs.
pub fn display_values(ip: &dyn IpIntelligenceData) -> Vec<DisplayValue> {
    let string_render = |value: &String| value.clone();
    let number_render = |value: &f32| format!("{value:.4}");
    let integer_render = |value: &i32| value.to_string();
    let bool_render = |value: &bool| if *value { "Yes" } else { "No" }.to_owned();

    vec![
        display_value("Asn", "ASN", &ip.asn(), string_render),
        display_value("AsnName", "ASN Name", &ip.asn_name(), string_render),
        display_value(
            "RegisteredName",
            "Registered Name",
            &ip.registered_name(),
            string_render,
        ),
        display_value(
            "RegisteredOwner",
            "Registered Owner",
            &ip.registered_owner(),
            string_render,
        ),
        display_value(
            "RegisteredCountry",
            "Registered Country",
            &ip.registered_country(),
            string_render,
        ),
        display_value(
            "IpRangeStart",
            "IP Range Start",
            &ip.ip_range_start(),
            string_render,
        ),
        display_value(
            "IpRangeEnd",
            "IP Range End",
            &ip.ip_range_end(),
            string_render,
        ),
        display_value("Country", "Country", &ip.country(), string_render),
        display_value(
            "CountryCode",
            "Country Code",
            &ip.country_code(),
            string_render,
        ),
        display_value(
            "CountryCode3",
            "Country Code (3)",
            &ip.country_code3(),
            string_render,
        ),
        display_value("Region", "Region", &ip.region(), string_render),
        display_value("State", "State", &ip.state(), string_render),
        display_value("Town", "Town", &ip.town(), string_render),
        display_value("Latitude", "Latitude", &ip.latitude(), number_render),
        display_value("Longitude", "Longitude", &ip.longitude(), number_render),
        display_value(
            "AccuracyRadiusMin",
            "Accuracy Radius Min (km)",
            &ip.accuracy_radius_min(),
            integer_render,
        ),
        display_value(
            "TimeZoneOffset",
            "Time Zone Offset (min)",
            &ip.time_zone_offset(),
            integer_render,
        ),
        display_value(
            "ConnectionType",
            "Connection Type",
            &ip.connection_type(),
            string_render,
        ),
        display_value("IsHosted", "Is Hosted", &ip.is_hosted(), bool_render),
        display_value("IsVPN", "Is VPN", &ip.is_vpn(), bool_render),
        display_value("IsProxy", "Is Proxy", &ip.is_proxy(), bool_render),
        display_value("IsCellular", "Is Cellular", &ip.is_cellular(), bool_render),
        display_value("IsEu", "Is in EU", &ip.is_eu(), bool_render),
        display_weighted(
            "CountryCodesGeographical",
            "Country Codes (geographical)",
            &ip.country_codes_geographical(),
        ),
        display_weighted(
            "CountryCodesPopulation",
            "Country Codes (population)",
            &ip.country_codes_population(),
        ),
        display_weighted("Mcc", "Mobile Country Code (MCC)", &ip.mcc()),
    ]
}

/// Replace the value of each row for a property only the Enterprise data file
/// carries, when the data file in use does not carry it, with a note saying
/// so. `available` holds the properties the data file carries.
pub fn mark_enterprise_only(values: &mut [DisplayValue], available: &[String]) {
    for row in values {
        let enterprise = ip_intelligence_examples::ENTERPRISE_PROPERTIES
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&row.property));
        if enterprise && !examples_shared::is_available(&row.property, available) {
            row.value = examples_shared::NEEDS_ENTERPRISE_DATA_FILE.to_owned();
        }
    }
}

/// A message naming the Enterprise properties the data file does not carry,
/// with a link to ask about a copy of the Enterprise data file. Empty when
/// `missing` is empty.
pub fn enterprise_message_html(missing: &[&str]) -> String {
    if missing.is_empty() {
        return String::new();
    }
    let names = match missing {
        [only] => (*only).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        [] => String::new(),
    };
    let need = if missing.len() == 1 {
        "It needs"
    } else {
        "They need"
    };
    format!(
        "<div class=\"c-eg-message\">\
         <p class=\"c-eg-message__text\">The data file in use does not carry {names}. \
         {need} the Enterprise data file. \
         <a href=\"{url}\">Contact us</a> about getting a copy.</p>\
         <a class=\"b-btn c-eg-message__cta\" href=\"{url}\">Contact us</a>\
         </div>",
        names = html_escape(&names),
        url = ENTERPRISE_CONTACT_URL,
    )
}

/// The "Find out more" section that ends every page, set apart by a rule. It
/// lists [`FIND_OUT_MORE`] and then each of `repositories` as a bare address.
pub fn find_out_more_html(repositories: &[&str]) -> String {
    let mut items = String::new();
    for (label, url) in FIND_OUT_MORE {
        items.push_str(&format!(
            "<li><a class=\"b-link\" href=\"{url}\">{}</a></li>",
            html_escape(label)
        ));
    }
    for url in repositories {
        items.push_str(&format!(
            "<li><a class=\"b-link\" href=\"{url}\">{url}</a></li>"
        ));
    }
    format!(
        "<hr><section class=\"c-eg-section\" id=\"find-out-more\">\
         <h3 class=\"c-eg-page__heading\">{heading}</h3><ul>{items}</ul></section>",
        heading = examples_shared::FIND_OUT_MORE_HEADING,
    )
}

/// Render the server-side IP-intelligence results as a two-column `.c-eg-table`
/// (property, value).
pub fn results_table(values: &[DisplayValue]) -> String {
    let mut rows = String::new();
    for (index, item) in values.iter().enumerate() {
        let alt = if index % 2 == 0 {
            " c-eg-table__row--alt"
        } else {
            ""
        };
        rows.push_str(&format!(
            "<tr class=\"c-eg-table__row{alt}\">\
             <td class=\"c-eg-table__cell c-eg-table__cell--key\">{}</td>\
             <td class=\"c-eg-table__cell\">{}</td></tr>",
            html_escape(&item.label),
            html_escape(&item.value),
        ));
    }
    format!(
        "<table class=\"c-eg-table\">\
         <thead class=\"c-eg-table__head\"><tr class=\"c-eg-table__row\">\
         <th class=\"c-eg-table__cell\">Property</th>\
         <th class=\"c-eg-table__cell\">Value</th></tr></thead>\
         <tbody>{rows}</tbody></table>"
    )
}

/// Extract the latitude and longitude from the result as display strings, if
/// both resolved to a value. These feed the location map.
pub fn coordinates(ip: &dyn IpIntelligenceData) -> Option<(String, String)> {
    let latitude = ip
        .latitude()
        .into_value()
        .ok()
        .map(|value| format!("{value:.6}"))?;
    let longitude = ip
        .longitude()
        .into_value()
        .ok()
        .map(|value| format!("{value:.6}"))?;
    Some((latitude, longitude))
}

/// The default DOM ids the shared `fodExamples.initLocationMap` helper uses.
/// They are emitted in [`render_page`] and referenced by [`map_init_script`].
const MAP_SECTION_ID: &str = "map-section";
const MAP_CANVAS_ID: &str = "map";

/// Build the inline `<script>` that initializes the Leaflet location map for the
/// supplied coordinates. When no coordinates resolved the map stays hidden and
/// an empty string is returned, so the page renders without a map.
pub fn map_init_script(coordinates: &Option<(String, String)>) -> String {
    let Some((latitude, longitude)) = coordinates else {
        return String::new();
    };
    // The coordinates come from f64 accessors and were formatted with `{:.6}`, so
    // they are plain numeric literals (digits, an optional sign and one dot). The
    // sanitiser below keeps only those characters as defence in depth before they
    // are embedded in the script, so no script-breaking input can reach the page.
    let latitude = numeric_literal(latitude);
    let longitude = numeric_literal(longitude);
    format!(
        "<script>window.addEventListener('load', function () {{ \
         if (window.fodExamples && window.fodExamples.initLocationMap) {{ \
         window.fodExamples.initLocationMap({{ \
         sectionId: '{section}', canvasId: '{canvas}', \
         latitude: {latitude}, longitude: {longitude}, \
         labels: {{ ipLocation: 'IP Location', lat: 'Lat', lng: 'Lng' }} }}); }} }});</script>",
        section = MAP_SECTION_ID,
        canvas = MAP_CANVAS_ID,
    )
}

/// The contact-us banner shown on the cloud page, inviting the reader to discuss
/// an on-premise deployment.
pub const CLOUD_CONTACT_BANNER: &str = "\
<div class=\"c-eg-message\">\
  <p class=\"c-eg-message__text\">Want to run IP Intelligence on-premise from a local data file? \
  <a href=\"https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-web_support-mod.rs&utm_term=on-premise\">Contact us</a> to discuss requirements. \
  <a href=\"https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-web_support-mod.rs&utm_term=on-premise\">See pricing</a>.</p>\
  <a class=\"b-btn c-eg-message__cta\" href=\"https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-web_support-mod.rs&utm_term=on-premise\">Contact us</a>\
</div>";

/// The contact-us banner shown on the on-premise page, describing what the free
/// IP Intelligence data files cover and what a paid Enterprise file adds.
pub const ONPREM_CONTACT_BANNER: &str = "\
<div class=\"c-eg-message\">\
  <p class=\"c-eg-message__text\">This example uses a free IP Intelligence data file, which resolves \
  a limited set of network and location properties. A paid Enterprise data file adds far more \
  detailed and accurate location, registered network ownership and broader coverage. \
  <a href=\"https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-web_support-mod.rs&utm_term=data-file-benefits\">See pricing</a> or \
  <a href=\"https://51degrees.com/contact-us?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-web_support-mod.rs&utm_term=data-file-benefits\">contact us</a>.</p>\
  <a class=\"b-btn c-eg-message__cta\" href=\"https://51degrees.com/pricing?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-ip-intelligence-examples-src-web_support-mod.rs&utm_term=data-file-benefits\">See pricing</a>\
</div>";

/// The Leaflet and wellknown libraries the shared `initLocationMap` helper needs,
/// loaded from a CDN. They are kept here (rather than vendored) because they are
/// only needed for the optional map.
pub const MAP_LIBRARIES: &str = "\
<link rel=\"stylesheet\" href=\"https://unpkg.com/leaflet@1.9.4/dist/leaflet.css\" \
 integrity=\"sha256-p4NxAoJBhIIN+hmNHrzRCf9tD/miZyoHS5obTRR9BMY=\" crossorigin=\"\">\
<script src=\"https://unpkg.com/leaflet@1.9.4/dist/leaflet.js\" \
 integrity=\"sha256-20nQCchB9co0qIjJZRGuk2/Z9VM+kNiyxNV1lvTlZBo=\" crossorigin=\"\"></script>\
<script src=\"https://unpkg.com/wellknown@0.5.0/wellknown.js\"></script>";

/// The fields a [`render_page`] call interpolates. Carrying them in a struct
/// keeps the renderer's signature stable as the cloud and on-premise pages
/// differ in their banners, alerts and form action.
pub struct PageOptions<'a> {
    /// The page `<h2>` title.
    pub title: &'a str,
    /// The introductory lead paragraph.
    pub lead: &'a str,
    /// A `c-eg-alert` shown at the very top (used for the on-premise stale-data
    /// warning). Empty string for no top alert.
    pub top_alert: &'a str,
    /// The IP address the result describes, shown above the results table.
    pub client_ip: &'a str,
    /// The pre-rendered server-side results table (or a placeholder).
    pub results_html: &'a str,
    /// The pre-rendered evidence table.
    pub evidence_html: &'a str,
    /// The query-key name the IP form submits under (so it lands on an evidence
    /// key the deployment's engine accepts).
    pub form_field: &'a str,
    /// The current value to pre-fill the IP form input with.
    pub form_value: &'a str,
    /// The contact-us banner HTML for this deployment.
    pub message_html: &'a str,
    /// The pre-rendered map-initialization `<script>` (empty when no
    /// coordinates resolved).
    pub map_script: &'a str,
    /// The repositories the "Find out more" section lists.
    pub repositories: &'a [&'a str],
}

/// Render a full HTML document for an IP-intelligence web example page.
///
/// The structure is a titled page, the client IP the result describes, a
/// results table, the evidence used, an IP form so a visitor can look up any
/// address, the optional location map, the contact-us banner and the "Find out
/// more" section.
pub fn render_page(options: PageOptions<'_>) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{title}</title>\
         <link rel=\"stylesheet\" href=\"{css}\">\
         {map_libraries}\
         </head><body>\
         <div class=\"c-eg-page\">\
         <h2 class=\"c-eg-page__title\">{title}</h2>\
         <p class=\"c-eg-page__lead\">{lead}</p>\
         {top_alert}\
         <div id=\"content\">\
         <h3 class=\"c-eg-page__heading\">IP Intelligence results</h3>\
         <p class=\"c-eg-page__lead\">These values are the server-side IP Intelligence \
         lookup for <strong>{client_ip}</strong>. Each property resolves to a single value, \
         or a no-value explanation when the data file does not carry it.</p>\
         {results_html}\
         <section class=\"c-eg-section c-eg-map\" id=\"{map_section}\" style=\"display:none\">\
         <h3 class=\"c-eg-map__title\">Approximate location</h3>\
         <div class=\"c-eg-map__canvas\" id=\"{map_canvas}\"></div>\
         </section>\
         <h3 class=\"c-eg-page__heading\">Look up another IP address</h3>\
         <form class=\"c-eg-form\" method=\"get\" action=\"/\">\
         <div class=\"c-eg-form__row\">\
         <label for=\"ip-input\">IP address</label>\
         <input id=\"ip-input\" name=\"{form_field}\" type=\"text\" value=\"{form_value}\" \
         placeholder=\"e.g. 185.28.167.77 or 2001:4860:4860::8888\">\
         </div>\
         <div class=\"c-eg-button-row\">\
         <button class=\"b-btn\" type=\"submit\">Look up IP</button>\
         </div>\
         </form>\
         <h3 class=\"c-eg-page__heading\">Evidence used</h3>\
         {evidence_html}\
         </div>\
         {message_html}\
         {find_out_more}\
         </div>\
         <script src=\"{js}\"></script>\
         {map_script}\
         </body></html>",
        title = html_escape(options.title),
        css = ASSETS_CSS_ROUTE,
        js = ASSETS_JS_ROUTE,
        map_libraries = MAP_LIBRARIES,
        lead = html_escape(options.lead),
        top_alert = options.top_alert,
        client_ip = html_escape(options.client_ip),
        results_html = options.results_html,
        evidence_html = options.evidence_html,
        form_field = html_escape(options.form_field),
        form_value = html_escape(options.form_value),
        map_section = MAP_SECTION_ID,
        map_canvas = MAP_CANVAS_ID,
        message_html = options.message_html,
        find_out_more = find_out_more_html(options.repositories),
        map_script = options.map_script,
    )
}

/// Render the data-file warnings as a single top-of-page `c-eg-alert`, or an
/// empty string when there are none. Shown by the on-premise page.
pub fn warnings_alert(warnings: &[String]) -> String {
    if warnings.is_empty() {
        return String::new();
    }
    let body = warnings
        .iter()
        .map(|w| html_escape(w))
        .collect::<Vec<_>>()
        .join("<br>");
    format!("<div class=\"c-eg-alert\">{body}</div>")
}

/// Keep only the characters that can appear in a decimal number literal (digits,
/// a leading sign, the decimal point and an exponent marker), so a coordinate
/// embedded into the inline map script cannot carry anything script-significant.
/// Falls back to `0` when nothing usable remains.
fn numeric_literal(input: &str) -> String {
    let cleaned: String = input
        .chars()
        .filter(|ch| ch.is_ascii_digit() || matches!(ch, '-' | '+' | '.' | 'e' | 'E'))
        .collect();
    if cleaned.is_empty() {
        "0".to_owned()
    } else {
        cleaned
    }
}
