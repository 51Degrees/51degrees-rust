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

//! The element data: the twelve properties, each of which can say it has no
//! value and why.

use std::any::Any;
use std::sync::Arc;

use chrono::{DateTime, SecondsFormat, Utc};
use fiftyone_pipeline_core::{ElementData, NoValueError, PropertyValue};
use fiftyone_pipeline_engines::AspectPropertyValue;

use crate::constants::{
    PROPERTY_AGENT, PROPERTY_ALGORITHM, PROPERTY_CARD_URL, PROPERTY_CREATED, PROPERTY_EXPIRES,
    PROPERTY_KEY_ID, PROPERTY_NAME, PROPERTY_NONCE, PROPERTY_PRODUCT_TOKEN, PROPERTY_PURPOSE,
    PROPERTY_REASON, PROPERTY_STATUS,
};

/// The property names in the order the element lists them.
pub(crate) const PROPERTY_NAMES: [&str; 12] = [
    PROPERTY_STATUS,
    PROPERTY_REASON,
    PROPERTY_AGENT,
    PROPERTY_KEY_ID,
    PROPERTY_ALGORITHM,
    PROPERTY_CREATED,
    PROPERTY_EXPIRES,
    PROPERTY_NONCE,
    PROPERTY_PURPOSE,
    PROPERTY_NAME,
    PROPERTY_PRODUCT_TOKEN,
    PROPERTY_CARD_URL,
];

/// The twelve values for one request.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AgentSignatureValues {
    pub(crate) status: AspectPropertyValue<String>,
    pub(crate) reason: AspectPropertyValue<String>,
    pub(crate) agent: AspectPropertyValue<String>,
    pub(crate) key_id: AspectPropertyValue<String>,
    pub(crate) algorithm: AspectPropertyValue<String>,
    pub(crate) created: AspectPropertyValue<DateTime<Utc>>,
    pub(crate) expires: AspectPropertyValue<DateTime<Utc>>,
    pub(crate) nonce: AspectPropertyValue<String>,
    pub(crate) purpose: AspectPropertyValue<String>,
    pub(crate) name: AspectPropertyValue<String>,
    pub(crate) product_token: AspectPropertyValue<String>,
    pub(crate) card_url: AspectPropertyValue<String>,
}

/// What the agent signature element reports about one request.
///
/// Every property can say it has no value together with the reason, so an
/// absent detail never reads as an answer. Only
/// [`AgentSignatureData::agent_signature`] and
/// [`AgentSignatureData::agent_signature_reason`] always have a value.
///
/// Through [`ElementData::get`] the two times are text in the RFC 3339 form
/// `2025-01-01T00:00:00+00:00`, because the pipeline's property values have
/// no type for a point in time. The accessors give them as times.
#[derive(Debug, Clone)]
pub struct AgentSignatureData {
    values: Arc<AgentSignatureValues>,
}

impl AgentSignatureData {
    pub(crate) fn new(values: Arc<AgentSignatureValues>) -> Self {
        AgentSignatureData { values }
    }

    /// The status, one of the `STATUS_` constants. Always has a value.
    pub fn agent_signature(&self) -> &AspectPropertyValue<String> {
        &self.values.status
    }

    /// The reason code, one of the `REASON_` constants. Always has a value.
    pub fn agent_signature_reason(&self) -> &AspectPropertyValue<String> {
        &self.values.reason
    }

    /// The `Signature-Agent` member value exactly as the agent sent it.
    pub fn agent_signature_agent(&self) -> &AspectPropertyValue<String> {
        &self.values.agent
    }

    /// The `keyid` signature parameter, being the thumbprint of the key the
    /// agent signed with.
    pub fn agent_signature_key_id(&self) -> &AspectPropertyValue<String> {
        &self.values.key_id
    }

    /// The RFC 9421 registry name of the algorithm settled on, or the name
    /// the agent asked for when the element does not verify that algorithm.
    pub fn agent_signature_algorithm(&self) -> &AspectPropertyValue<String> {
        &self.values.algorithm
    }

    /// The point in time the `created` parameter names.
    pub fn agent_signature_created(&self) -> &AspectPropertyValue<DateTime<Utc>> {
        &self.values.created
    }

    /// The point in time the `expires` parameter names.
    pub fn agent_signature_expires(&self) -> &AspectPropertyValue<DateTime<Utc>> {
        &self.values.expires
    }

    /// The `nonce` parameter. Checking that a nonce is never reused is the
    /// caller's job, because only the caller knows the traffic.
    pub fn agent_signature_nonce(&self) -> &AspectPropertyValue<String> {
        &self.values.nonce
    }

    /// What the agent says it uses fetched pages for, from the key directory
    /// or, where the directory does not say, from the agent card.
    pub fn agent_signature_purpose(&self) -> &AspectPropertyValue<String> {
        &self.values.purpose
    }

    /// The `client_name` of the agent card.
    pub fn agent_signature_name(&self) -> &AspectPropertyValue<String> {
        &self.values.name
    }

    /// The `rfc9309-product-token` of the agent card, being the name the
    /// agent answers to in a robots.txt file.
    pub fn agent_signature_product_token(&self) -> &AspectPropertyValue<String> {
        &self.values.product_token
    }

    /// The `client_id` of the agent card, which is the address the card was
    /// fetched from.
    pub fn agent_signature_card_url(&self) -> &AspectPropertyValue<String> {
        &self.values.card_url
    }
}

impl ElementData for AgentSignatureData {
    fn get(&self, name: &str) -> Result<PropertyValue, NoValueError> {
        let values = &self.values;
        match name.to_ascii_lowercase().as_str() {
            PROPERTY_STATUS => text(&values.status),
            PROPERTY_REASON => text(&values.reason),
            PROPERTY_AGENT => text(&values.agent),
            PROPERTY_KEY_ID => text(&values.key_id),
            PROPERTY_ALGORITHM => text(&values.algorithm),
            PROPERTY_CREATED => time(&values.created),
            PROPERTY_EXPIRES => time(&values.expires),
            PROPERTY_NONCE => text(&values.nonce),
            PROPERTY_PURPOSE => text(&values.purpose),
            PROPERTY_NAME => text(&values.name),
            PROPERTY_PRODUCT_TOKEN => text(&values.product_token),
            PROPERTY_CARD_URL => text(&values.card_url),
            _ => Err(NoValueError::new(format!(
                "No value for property '{name}'."
            ))),
        }
    }

    fn has_property(&self, name: &str) -> bool {
        PROPERTY_NAMES
            .iter()
            .any(|property| property.eq_ignore_ascii_case(name))
    }

    fn keys(&self) -> Vec<String> {
        PROPERTY_NAMES
            .iter()
            .map(|name| (*name).to_owned())
            .collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn text(value: &AspectPropertyValue<String>) -> Result<PropertyValue, NoValueError> {
    value
        .value()
        .map(|text| PropertyValue::String(text.clone()))
}

fn time(value: &AspectPropertyValue<DateTime<Utc>>) -> Result<PropertyValue, NoValueError> {
    value
        .value()
        .map(|time| PropertyValue::String(format_time(time)))
}

/// A point in time as RFC 3339 text with the offset written out, which is
/// the form the .NET JSON builder writes for the same value.
pub(crate) fn format_time(time: &DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Secs, false)
}
