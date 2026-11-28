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

//! The two Human Confidence Properties a sealed result gives, from
//! [`CloudRequestEngine::redeem_human_confidence`](crate::CloudRequestEngine::redeem_human_confidence).
//!
//! The rules are those of
//! [Human Confidence](https://github.com/51Degrees/specifications/blob/main/pipeline-specification/features/human-confidence.md#when-humanconfidence-arrives-sealed).

/// The Requested Property a redemption names, so the response carries the
/// Value.
pub(crate) const PROPERTY: &str = "derived.HumanConfidence";

/// The form field the sealed result is sent in.
pub(crate) const SEALED_RESULT_FIELD: &str = "humanconfidence";

/// The form field the transaction is sent in.
pub(crate) const TRANSACTION_FIELD: &str = "transaction";

/// How confident the remote server is that a person is using the browser,
/// the `HumanConfidence` Property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HumanConfidence {
    /// High confidence.
    High,
    /// Medium confidence, which is also the Value of every sealed result
    /// that was not redeemed.
    Medium,
    /// Low confidence.
    Low,
}

impl HumanConfidence {
    /// The word for this Value, `High`, `Medium` or `Low`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
        }
    }

    /// The Value a response's `derived.humanconfidence` holds, or `None` for
    /// anything other than exactly `High`, `Medium` or `Low`.
    pub(crate) fn from_response(word: Option<&str>) -> Option<Self> {
        match word? {
            "High" => Some(Self::High),
            "Medium" => Some(Self::Medium),
            "Low" => Some(Self::Low),
            _ => None,
        }
    }
}

/// How [`HumanConfidence`] was worked out, the `HumanConfidenceSource`
/// Property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HumanConfidenceSource {
    /// From the request alone.
    Passive,
    /// The browser also ran the 51Degrees browser tests. Every request that
    /// carries a sealed result is `Active`, whether or not it redeemed.
    Active,
}

impl HumanConfidenceSource {
    /// The word for this source, `Passive` or `Active`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passive => "Passive",
            Self::Active => "Active",
        }
    }
}

/// The two Properties for a request that carries a sealed Human Confidence
/// result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SealedHumanConfidence {
    value: HumanConfidence,
}

impl SealedHumanConfidence {
    /// The Properties of a sealed result that redeemed to `value`.
    pub(crate) fn redeemed(value: HumanConfidence) -> Self {
        Self { value }
    }

    /// The Properties of a sealed result that was not redeemed, which are
    /// `Medium` and `Active`.
    pub(crate) fn not_redeemed() -> Self {
        Self {
            value: HumanConfidence::Medium,
        }
    }

    /// The `HumanConfidence` Property: the Value the sealed result redeemed
    /// to, or [`HumanConfidence::Medium`] where it was not redeemed.
    pub fn human_confidence(&self) -> HumanConfidence {
        self.value
    }

    /// The `HumanConfidenceSource` Property, always
    /// [`HumanConfidenceSource::Active`] for a sealed result.
    pub fn human_confidence_source(&self) -> HumanConfidenceSource {
        HumanConfidenceSource::Active
    }
}
