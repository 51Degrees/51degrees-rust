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

//! The set-headers element.
//!
//! The set-headers element builds the set of HTTP response headers that other
//! elements in the pipeline want sent to the client, usually to request more
//! evidence such as User-Agent Client Hints. It implements the
//! [set-headers-element specification](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/pipeline-elements/set-headers-element.md).
//!
//! # The naming convention
//!
//! Any property whose name starts with `SetHeader` carries a value to write to
//! a response header. The name is `SetHeader[Identifier][HeaderName]`, where the
//! identifier MUST NOT contain upper-case characters after its first character,
//! and the header name begins at the next upper-case character. For example
//! `SetHeaderBrowserAccept-CH` targets the `Accept-CH` response header.
//!
//! # Start-up scan
//!
//! Scanning the pipeline metadata for `SetHeader*` properties is done once and
//! cached behind a [`OnceLock`], because the property set is fixed for the
//! lifetime of the pipeline. On each request the element only reads the cached
//! mapping and the relevant element-data values.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use fiftyone_pipeline_core::{
    ElementData, EvidenceKeyFilter, EvidenceKeyFilterWhitelist, FlowData, FlowElement,
    MapElementData, NoValueError, PropertyMetaData, PropertyValue, PropertyValueType, Result,
    TypedKey,
};

use crate::constants::{SET_HEADERS_DEFAULT_ELEMENT_DATA_KEY, SET_HEADER_PROPERTY_PREFIX};

/// The name of the single property this element publishes, holding the response
/// header dictionary.
pub const RESPONSE_HEADER_DICTIONARY_PROPERTY: &str = "responseheaderdictionary";

/// The element data produced by the [`SetHeadersElement`].
///
/// It holds the response-header dictionary: a map from HTTP response header name
/// to the value that header should be set to. The dictionary is exposed both as
/// a typed accessor ([`SetHeadersData::response_headers`]) and through the
/// dynamic [`ElementData`] bag under
/// [`RESPONSE_HEADER_DICTIONARY_PROPERTY`] (as a list of `"Name: value"`
/// strings, so it round-trips through the string-based property model).
#[derive(Debug, Clone, Default)]
pub struct SetHeadersData {
    headers: BTreeMap<String, String>,
    inner: MapElementData,
}

impl SetHeadersData {
    fn new(headers: BTreeMap<String, String>) -> Self {
        // Expose the dictionary through the dynamic bag as a string list of
        // "Name: value" entries. A flat representation keeps the value inside
        // the closed PropertyValue model without inventing a map variant.
        let rendered: Vec<String> = headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}"))
            .collect();
        let inner = MapElementData::new().set(RESPONSE_HEADER_DICTIONARY_PROPERTY, rendered);
        SetHeadersData { headers, inner }
    }

    /// The response headers to set, keyed by header name.
    pub fn response_headers(&self) -> &BTreeMap<String, String> {
        &self.headers
    }
}

impl ElementData for SetHeadersData {
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

/// One `SetHeader*` property discovered during the start-up scan.
#[derive(Debug, Clone)]
struct SetHeaderProperty {
    /// The full property name, used to read the value from element data.
    property_name: String,
    /// The data key of the element that populates this property.
    element_data_key: String,
    /// The HTTP response header to set from the property value.
    response_header_name: String,
}

/// Builds the HTTP response headers other elements want set.
///
/// The element uses no evidence and takes no configuration. It scans the
/// pipeline's element properties for the `SetHeader` naming convention and
/// caches the first non-empty result. Caching only a non-empty scan matters for
/// cloud engines, which discover their property metadata lazily on their first
/// process, so a request that reaches this element before that discovery has
/// completed does not freeze an empty set for the life of the element.
pub struct SetHeadersElement {
    filter: EvidenceKeyFilterWhitelist,
    properties: Vec<PropertyMetaData>,
    // Populated from the pipeline metadata with the first non-empty scan, then
    // reused. The element is added to a single pipeline, so one slot suffices.
    set_header_properties: OnceLock<Vec<SetHeaderProperty>>,
}

impl SetHeadersElement {
    /// The typed key under which this element stores its [`SetHeadersData`].
    pub const KEY: TypedKey<SetHeadersData> = TypedKey::new(SET_HEADERS_DEFAULT_ELEMENT_DATA_KEY);

    /// The default element data key, `"set-headers"`.
    pub const DEFAULT_ELEMENT_DATA_KEY: &'static str = SET_HEADERS_DEFAULT_ELEMENT_DATA_KEY;

    /// Create a new set-headers element.
    pub fn new() -> Self {
        let properties = vec![PropertyMetaData::new(
            RESPONSE_HEADER_DICTIONARY_PROPERTY,
            SET_HEADERS_DEFAULT_ELEMENT_DATA_KEY,
            PropertyValueType::StringList,
        )];
        SetHeadersElement {
            filter: EvidenceKeyFilterWhitelist::new(Vec::<String>::new()),
            properties,
            set_header_properties: OnceLock::new(),
        }
    }

    /// Scan the pipeline's elements for available properties named with the
    /// `SetHeader` convention and record the header each targets. A property
    /// marked unavailable is left out, as .NET reads only the pipeline's
    /// available properties.
    fn scan(data: &FlowData) -> Vec<SetHeaderProperty> {
        let mut found = Vec::new();
        for element in data.pipeline().flow_elements() {
            for property in element.properties().iter().filter(|p| p.available) {
                if !property
                    .name
                    .to_lowercase()
                    .starts_with(&SET_HEADER_PROPERTY_PREFIX.to_lowercase())
                {
                    continue;
                }
                if let Some(header_name) = response_header_name(&property.name) {
                    found.push(SetHeaderProperty {
                        property_name: property.name.clone(),
                        element_data_key: property.element_data_key.clone(),
                        response_header_name: header_name,
                    });
                }
            }
        }
        found
    }

    /// Build the response-header dictionary for one flow data from the cached
    /// `SetHeader*` property list.
    fn build_headers(
        &self,
        data: &FlowData,
        properties: &[SetHeaderProperty],
    ) -> BTreeMap<String, String> {
        // Accumulate the distinct, order-preserving values for each header so
        // that comma-separated lists from several properties merge cleanly.
        let mut accumulators: BTreeMap<String, (BTreeSet<String>, Vec<String>)> = BTreeMap::new();

        for property in properties {
            // The element may not have run (only a subset of properties may
            // have been requested) so look up by string key and skip if absent.
            let Some(element_data) = data.get_str(&property.element_data_key) else {
                continue;
            };
            let header_value = match element_data.get(&property.property_name) {
                Ok(value) => string_value(&value),
                Err(_) => continue,
            };

            let Some(header_value) = header_value else {
                continue;
            };
            if header_value.is_empty() || header_value.eq_ignore_ascii_case("unknown") {
                continue;
            }

            let entry = accumulators
                .entry(property.response_header_name.clone())
                .or_default();
            for segment in header_value.split(',') {
                let segment = segment.trim();
                if segment.is_empty() {
                    continue;
                }
                if entry.0.insert(segment.to_owned()) {
                    entry.1.push(segment.to_owned());
                }
            }
        }

        accumulators
            .into_iter()
            .map(|(header, (_, values))| (header, values.join(",")))
            .collect()
    }
}

impl Default for SetHeadersElement {
    fn default() -> Self {
        Self::new()
    }
}

impl FlowElement for SetHeadersElement {
    fn process(&self, data: &mut FlowData) -> Result<()> {
        // Build the list of SetHeader* properties from pipeline metadata and
        // reuse it on every subsequent request. Only a non-empty scan is cached,
        // because a cloud engine discovers its properties lazily on its first
        // process. A scan that runs before that completes must not freeze an
        // empty result, so it is retried on the next request instead.
        let properties = match self.set_header_properties.get() {
            Some(cached) => cached.clone(),
            None => {
                let scanned = Self::scan(data);
                if !scanned.is_empty() {
                    let _ = self.set_header_properties.set(scanned.clone());
                }
                scanned
            }
        };

        let headers = self.build_headers(data, &properties);
        data.get_or_add(Self::KEY, || SetHeadersData::new(headers))?;
        Ok(())
    }

    fn data_key(&self) -> &str {
        SET_HEADERS_DEFAULT_ELEMENT_DATA_KEY
    }

    fn evidence_key_filter(&self) -> &dyn EvidenceKeyFilter {
        &self.filter
    }

    fn properties(&self) -> &[PropertyMetaData] {
        &self.properties
    }
}

/// Extract the response header name from a `SetHeader[Identifier][HeaderName]`
/// property name.
///
/// The identifier follows the `SetHeader` prefix and its first character may be
/// upper-case. The header name starts at the next upper-case character after
/// that. Returns `None` if the name is too short or has no header-name segment,
/// which causes the property to be skipped rather than panicking.
fn response_header_name(property_name: &str) -> Option<String> {
    let prefix_len = SET_HEADER_PROPERTY_PREFIX.len();
    if !property_name.starts_with(SET_HEADER_PROPERTY_PREFIX) {
        return None;
    }
    // Need at least the prefix, one identifier character and one header
    // character.
    if property_name.len() < prefix_len + 2 {
        return None;
    }

    let bytes = property_name.as_bytes();
    // Start searching one character past the first identifier character, which
    // is allowed to be upper-case.
    let mut next_upper = None;
    let mut i = prefix_len + 1;
    while i < bytes.len() {
        if bytes[i].is_ascii_uppercase() {
            next_upper = Some(i);
            break;
        }
        i += 1;
    }

    next_upper.map(|start| property_name[start..].to_owned())
}

/// Get the string form of a property value for use as a header value. Strings
/// and JavaScript snippets yield their text; other variants yield `None` so the
/// caller skips them.
fn string_value(value: &PropertyValue) -> Option<String> {
    value.as_str().map(str::to_owned)
}
