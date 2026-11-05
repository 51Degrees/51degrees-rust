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

//! Framework-neutral request access and evidence population.
//!
//! This module is the framework-neutral web request evidence service. It
//! defines [`RequestData`], a trait an adapter implements over its framework's
//! request type, and [`build_evidence`], a function that turns one request into
//! a [`fiftyone_pipeline_core::Evidence`] honouring the pipeline's evidence-key
//! filter.
//!
//! Keeping the trait abstract is the whole point: nothing here knows about
//! axum, hyper, actix or any other framework. Each adapter supplies the small
//! shim that reads headers, cookies, query and form parameters from a real
//! request, and this crate maps them onto the prefixed evidence keys the
//! pipeline expects.
//!
//! # The mapping
//!
//! Each piece of request data becomes a prefixed evidence key, following the
//! [evidence specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/evidence.md):
//!
//! - every header `name: value` becomes `header.<name>`,
//! - every cookie `name=value` becomes `cookie.<name>`,
//! - every query-string parameter `name=value` becomes `query.<name>`,
//! - every form (POST body) parameter becomes `query.<name>` as well, folding
//!   form fields into the query prefix so the client-side callback (a form POST)
//!   lands under the same keys as a GET,
//! - the client IP address becomes `server.client-ip`,
//! - the request protocol (`http` or `https`) becomes `header.protocol`,
//! - where the adapter supplies the request line, the method, the path and the
//!   query string become `server.request-method`, `server.request-path` and
//!   `server.request-query`, exactly as the request line carried them.
//!
//! Every candidate key is added **only if the supplied filter includes it**, so
//! a pipeline never receives evidence no element can use. This keeps the cache
//! key and the web `Vary` header tight.

use fiftyone_pipeline_core::constants::{
    EVIDENCE_CLIENT_IP_KEY, EVIDENCE_COOKIE_PREFIX, EVIDENCE_HTTP_HEADER_PREFIX,
    EVIDENCE_PROTOCOL_KEY, EVIDENCE_QUERY_PREFIX, EVIDENCE_REQUEST_METHOD_KEY,
    EVIDENCE_REQUEST_PATH_KEY, EVIDENCE_REQUEST_QUERY_KEY, EVIDENCE_SEPARATOR,
};
use fiftyone_pipeline_core::{Evidence, EvidenceBuilder, EvidenceKeyFilter};

/// Read-only access to the parts of an HTTP request the pipeline turns into
/// evidence.
///
/// An adapter implements this over its framework's request type. Every accessor
/// returns owned `(name, value)` pairs (or owned values) so the implementation
/// is free about how it stores the underlying request; the cost of a little
/// copying here is negligible next to pipeline processing.
///
/// All names are returned as the request presents them. [`build_evidence`]
/// lowercases keys when it builds the [`Evidence`], so an implementation need
/// not normalize case itself.
pub trait RequestData {
    /// The request headers as `(name, value)` pairs. A header that appears more
    /// than once may be returned either as repeated pairs or as a single joined
    /// value; the last value for a given name wins when building evidence.
    fn headers(&self) -> Vec<(String, String)>;

    /// The request cookies as `(name, value)` pairs. Empty if there are none.
    fn cookies(&self) -> Vec<(String, String)>;

    /// The query-string parameters as `(name, value)` pairs. Empty if there is
    /// no query string.
    fn query_params(&self) -> Vec<(String, String)>;

    /// The form (POST body) parameters as `(name, value)` pairs.
    ///
    /// These are the `application/x-www-form-urlencoded` fields of a POST. The
    /// default implementation returns nothing, which suits a GET request or an
    /// adapter that does not parse bodies.
    fn form_params(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    /// The client IP address as a string, if the adapter can determine it.
    fn client_ip(&self) -> Option<String>;

    /// True if the request arrived over HTTPS. Drives [`RequestData::protocol`].
    fn is_https(&self) -> bool;

    /// The request protocol, `"https"` when [`RequestData::is_https`] is true
    /// and `"http"` otherwise.
    ///
    /// Override this when the framework exposes the scheme directly (for example
    /// behind a proxy that sets `X-Forwarded-Proto`).
    fn protocol(&self) -> String {
        if self.is_https() {
            "https".to_owned()
        } else {
            "http".to_owned()
        }
    }

    /// The value of the request `Origin` header, if present.
    ///
    /// Used by the client-side endpoints to echo an `Access-Control-Allow-Origin`
    /// header. The default implementation finds it among
    /// [`RequestData::headers`] case-insensitively.
    fn origin(&self) -> Option<String> {
        self.headers()
            .into_iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("origin"))
            .map(|(_, value)| value)
    }

    /// The value of the request `If-None-Match` header, if present.
    ///
    /// Used by the client-side endpoints to decide whether to return `304 Not
    /// Modified`. The default implementation finds it among
    /// [`RequestData::headers`] case-insensitively.
    fn if_none_match(&self) -> Option<String> {
        self.headers()
            .into_iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("if-none-match"))
            .map(|(_, value)| value)
    }

    /// The request method, for example `GET` or `POST`, exactly as the
    /// request line carried it, if the adapter knows it. Method names are
    /// case-sensitive, so the case is kept. The default returns `None`.
    fn method(&self) -> Option<String> {
        None
    }

    /// The request target, being the path and any query string with its `?`,
    /// byte for byte as the request line carried it, with no decoding and no
    /// normalisation, for example `/search?q=a%20b`. The default returns
    /// `None`.
    ///
    /// With [`RequestData::method`] this is the request line, which the agent
    /// signature element rebuilds a signed request from, so one changed byte
    /// makes a valid signature read as invalid.
    fn request_target(&self) -> Option<String> {
        None
    }
}

/// Build a key with the given prefix and field, joined by the evidence
/// separator, for example `("header", "user-agent") -> "header.user-agent"`.
fn prefixed(prefix: &str, field: &str) -> String {
    format!("{prefix}{EVIDENCE_SEPARATOR}{field}")
}

/// Build a [`fiftyone_pipeline_core::Evidence`] from a request, including only
/// the keys the supplied filter accepts.
///
/// This is the framework-neutral core of the web request evidence service.
/// The `filter` is normally the pipeline-wide filter
/// ([`fiftyone_pipeline_core::Pipeline::evidence_key_filter`]), so the resulting
/// evidence contains exactly the keys some element in the pipeline can use and
/// nothing more.
///
/// # Ordering and conflicts
///
/// Sources are added in the order headers, cookies, query parameters, form
/// parameters, then the client IP, the protocol and the request line. Because
/// form parameters share the `query.` prefix with query-string parameters, a
/// form field overwrites a query field of the same name (last-writer-wins).
/// The [`Evidence`] itself lowercases keys, so matching downstream is
/// case-insensitive regardless of how the request presented them.
///
/// # Example
///
/// ```
/// use fiftyone_pipeline_core::{EvidenceKeyFilterWhitelist};
/// use fiftyone_pipeline_web::{build_evidence, RequestData};
///
/// struct Req;
/// impl RequestData for Req {
///     fn headers(&self) -> Vec<(String, String)> {
///         vec![("User-Agent".into(), "test".into())]
///     }
///     fn cookies(&self) -> Vec<(String, String)> { Vec::new() }
///     fn query_params(&self) -> Vec<(String, String)> { Vec::new() }
///     fn client_ip(&self) -> Option<String> { Some("1.2.3.4".into()) }
///     fn is_https(&self) -> bool { true }
/// }
///
/// // Only the User-Agent header is in the filter, so only it is collected.
/// let filter = EvidenceKeyFilterWhitelist::new(["header.user-agent"]);
/// let evidence = build_evidence(&Req, &filter);
/// assert_eq!(evidence.get("header.user-agent"), Some("test"));
/// assert_eq!(evidence.get("server.client-ip"), None);
/// ```
pub fn build_evidence(request: &dyn RequestData, filter: &dyn EvidenceKeyFilter) -> Evidence {
    let mut builder = EvidenceBuilder::new();

    builder = add_prefixed(
        builder,
        filter,
        EVIDENCE_HTTP_HEADER_PREFIX,
        request.headers(),
    );
    builder = add_prefixed(builder, filter, EVIDENCE_COOKIE_PREFIX, request.cookies());
    builder = add_prefixed(
        builder,
        filter,
        EVIDENCE_QUERY_PREFIX,
        request.query_params(),
    );
    // Form fields are folded into the query prefix.
    builder = add_prefixed(
        builder,
        filter,
        EVIDENCE_QUERY_PREFIX,
        request.form_params(),
    );

    // The client IP uses the complete server.client-ip key rather than a
    // prefix + field split, so it is added directly.
    if let Some(ip) = request.client_ip() {
        if filter.include(EVIDENCE_CLIENT_IP_KEY) {
            builder = builder.add(EVIDENCE_CLIENT_IP_KEY, ip);
        }
    }

    // The protocol is exposed under the fixed header.protocol key so the
    // JavaScript builder can construct an absolute callback URL even when no
    // explicit protocol header was sent.
    if filter.include(EVIDENCE_PROTOCOL_KEY) {
        builder = builder.add(EVIDENCE_PROTOCOL_KEY, request.protocol());
    }

    // The request line goes in whole or not at all, with the query empty
    // where the target has none, so that a missing key always means the
    // adapter does not supply the request line rather than that the request
    // had no query string.
    if let (Some(method), Some(target)) = (request.method(), request.request_target()) {
        let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
        for (key, value) in [
            (EVIDENCE_REQUEST_METHOD_KEY, method.as_str()),
            (EVIDENCE_REQUEST_PATH_KEY, path),
            (EVIDENCE_REQUEST_QUERY_KEY, query),
        ] {
            if filter.include(key) {
                builder = builder.add(key, value);
            }
        }
    }

    builder.build()
}

/// Add each `(field, value)` under the given prefix, skipping any key the filter
/// rejects. Returns the builder for chaining.
fn add_prefixed(
    mut builder: EvidenceBuilder,
    filter: &dyn EvidenceKeyFilter,
    prefix: &str,
    pairs: Vec<(String, String)>,
) -> EvidenceBuilder {
    for (field, value) in pairs {
        let key = prefixed(prefix, &field);
        // The filter is queried with the lowercased key because evidence keys
        // are case-insensitive; the filter implementations also fold case, so
        // this is belt-and-braces.
        if filter.include(&key.to_lowercase()) {
            builder = builder.add(key, value);
        }
    }
    builder
}
