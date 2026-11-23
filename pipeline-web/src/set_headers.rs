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

//! Applying the set-headers element's response headers.
//!
//! This is the framework-neutral set-headers service. The
//! [`fiftyone_pipeline_engines_fiftyone::SetHeadersElement`] writes the set of
//! HTTP response headers other elements want sent (usually `Accept-CH` to
//! request User-Agent Client Hints). This module reads that dictionary back out
//! of a processed flow data so an adapter can merge it into the outgoing
//! response.
//!
//! It does not touch any framework type. [`response_headers`] returns the plain
//! `(name, value)` pairs, and [`apply_set_headers`] folds them into an existing
//! header list using the append semantics the specification requires.

use fiftyone_pipeline_core::FlowData;
use fiftyone_pipeline_engines_fiftyone::SetHeadersElement;

/// Read the response-header dictionary the set-headers element produced.
///
/// Returns the headers as ordered `(name, value)` pairs (the underlying store
/// is sorted by header name, so the order is deterministic). Returns an empty
/// vector when the set-headers element did not run or produced no headers, which
/// is the common case when no element requested extra evidence.
pub fn response_headers(flow_data: &FlowData) -> Vec<(String, String)> {
    match flow_data.get(SetHeadersElement::KEY) {
        Some(data) => data
            .response_headers()
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
        None => Vec::new(),
    }
}

/// Merge the set-headers response headers into an existing header list.
///
/// For each header the element wants set, the value is **appended** to the
/// existing value for that header if one is already present (joined with `, `,
/// the HTTP list separator), or added as a new header otherwise. Appending
/// rather than replacing means a value the application already set (for example
/// its own `Accept-CH`) is preserved alongside the 51Degrees one.
///
/// Header-name matching against the existing list is case-insensitive. The new
/// header keeps the name spelling the element produced when it is added fresh.
///
/// # Example
///
/// ```
/// use fiftyone_pipeline_web::apply_set_headers;
///
/// let mut existing = vec![("Accept-CH".to_owned(), "Sec-CH-UA".to_owned())];
/// let to_set = vec![("Accept-CH".to_owned(), "Sec-CH-UA-Platform".to_owned())];
/// apply_set_headers(&mut existing, &to_set);
/// assert_eq!(existing[0].1, "Sec-CH-UA, Sec-CH-UA-Platform");
/// ```
pub fn apply_set_headers(existing: &mut Vec<(String, String)>, to_set: &[(String, String)]) {
    for (name, value) in to_set {
        if value.is_empty() {
            continue;
        }
        if let Some((_, existing_value)) = existing
            .iter_mut()
            .find(|(existing_name, _)| existing_name.eq_ignore_ascii_case(name))
        {
            if existing_value.is_empty() {
                *existing_value = value.clone();
            } else {
                existing_value.push_str(", ");
                existing_value.push_str(value);
            }
        } else {
            existing.push((name.clone(), value.clone()));
        }
    }
}
