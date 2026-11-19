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

//! The typed answer from the redeem endpoint.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::key::parse_utc;
use crate::outcome::{ContextOutcome, Factor, FactorOutcome, SignatureOutcome};

/// The typed answer from the cloud's redeem endpoint, built by
/// [`DidClient::redeem`](crate::DidClient::redeem) from the JSON body.
///
/// [`RedeemResult::body`] keeps the body as received and
/// [`RedeemResult::status`] the HTTP status, so nothing the cloud said is
/// lost in the mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedeemResult {
    context: ContextOutcome,
    context_value: Option<String>,
    signature: SignatureOutcome,
    factors: Option<HashMap<String, FactorOutcome>>,
    verified_at: Option<DateTime<Utc>>,
    seconds_since_verified: Option<i64>,
    status: u16,
    body: String,
}

impl RedeemResult {
    /// Creates a result from its parts. Callers normally get one from
    /// [`RedeemResult::from_response`] rather than building one.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        context: ContextOutcome,
        context_value: Option<String>,
        signature: SignatureOutcome,
        factors: Option<HashMap<String, FactorOutcome>>,
        verified_at: Option<DateTime<Utc>>,
        seconds_since_verified: Option<i64>,
        status: u16,
        body: impl Into<String>,
    ) -> Self {
        Self {
            context,
            context_value,
            signature,
            factors,
            verified_at,
            seconds_since_verified,
            status,
            body: body.into(),
        }
    }

    /// Builds a result from a redeem response.
    ///
    /// A 503 is [`ContextOutcome::Unconfirmed`] whatever the body says,
    /// because the status is the service's own statement that it could not
    /// confirm first use. Otherwise the body is read as a JSON object, and a
    /// body that is not one, or carries no `context`, gives
    /// [`ContextOutcome::Unreadable`] with the body kept in
    /// [`RedeemResult::body`]. Factor values are read with
    /// [`FactorOutcome::from_cloud`], so neither `misconfigured` nor
    /// `notrecorded` falls through to a mismatch.
    pub fn from_response(status: u16, body: &str) -> Self {
        let value: serde_json::Value = match serde_json::from_str(body) {
            Ok(value) => value,
            Err(_) => return Self::unreadable(status, body),
        };
        let Some(root) = value.as_object() else {
            return Self::unreadable(status, body);
        };
        let context_value = read_string(root, "context");
        let context = if status == 503 {
            ContextOutcome::Unconfirmed
        } else {
            ContextOutcome::from_cloud(context_value)
        };
        let factors = root
            .get("factors")
            .and_then(|f| f.as_object())
            .map(|object| {
                object
                    .iter()
                    .map(|(name, value)| (name.clone(), FactorOutcome::from_cloud(value.as_str())))
                    .collect::<HashMap<_, _>>()
            });
        let verified_at = read_string(root, "verifiedAt").and_then(|v| parse_utc(v).ok());
        let seconds_since_verified = root.get("secondsSinceVerified").and_then(|v| v.as_i64());
        Self {
            context,
            context_value: context_value.map(str::to_owned),
            signature: SignatureOutcome::from_cloud(read_string(root, "signature")),
            factors,
            verified_at,
            seconds_since_verified,
            status,
            body: body.to_owned(),
        }
    }

    fn unreadable(status: u16, body: &str) -> Self {
        Self {
            context: if status == 503 {
                ContextOutcome::Unconfirmed
            } else {
                ContextOutcome::Unreadable
            },
            context_value: None,
            signature: SignatureOutcome::Unknown,
            factors: None,
            verified_at: None,
            seconds_since_verified: None,
            status,
            body: body.to_owned(),
        }
    }

    /// The creator context verdict, mapped from the `context` string. A
    /// string this client does not recognise maps to
    /// [`ContextOutcome::Unreadable`], so an unexpected answer never reads
    /// as a pass.
    pub fn context(&self) -> ContextOutcome {
        self.context
    }

    /// The `context` string exactly as the cloud sent it, or `None` when
    /// the body carried none.
    pub fn context_value(&self) -> Option<&str> {
        self.context_value.as_deref()
    }

    /// The signature outcome, mapped from the `signature` string.
    /// [`SignatureOutcome::Unknown`] when the field is absent, which it is
    /// on every outcome other than a redeemed verdict.
    pub fn signature(&self) -> SignatureOutcome {
        self.signature
    }

    /// The outcome of each creator context factor keyed by the name the
    /// cloud sent, being the names [`Factor::as_cloud`] gives. Present only
    /// when the cloud sent `factors`, which it does for a
    /// [`ContextOutcome::Mismatch`] and for a
    /// [`ContextOutcome::Misconfigured`] where some factors were compared.
    /// Every key the cloud sent is kept, including one this client does not
    /// know, so nothing the cloud said is lost.
    pub fn factors(&self) -> Option<&HashMap<String, FactorOutcome>> {
        self.factors.as_ref()
    }

    /// The outcome of one creator context factor, or `None` when the cloud
    /// sent no `factors` or did not report that factor.
    ///
    /// A version mismatch beside a verified name means an upgrade, whilst a
    /// mismatched name means a different operating system or browser, so
    /// read [`Factor::PlatformName`] with [`Factor::PlatformVersion`] and
    /// [`Factor::BrowserName`] with [`Factor::BrowserVersion`].
    pub fn factor(&self, factor: Factor) -> Option<FactorOutcome> {
        self.factors
            .as_ref()
            .and_then(|factors| factors.get(factor.as_cloud()))
            .copied()
    }

    /// When the verify endpoint checked the context and sealed the result,
    /// UTC. Present on the redeemed and expired outcomes.
    pub fn verified_at(&self) -> Option<DateTime<Utc>> {
        self.verified_at
    }

    /// How long before this redemption the verification happened, in whole
    /// seconds by the cloud's clock. Present on the redeemed and expired
    /// outcomes.
    pub fn seconds_since_verified(&self) -> Option<i64> {
        self.seconds_since_verified
    }

    /// The HTTP status the cloud answered with, 200 for every verdict and
    /// 503 for [`ContextOutcome::Unconfirmed`].
    pub fn status(&self) -> u16 {
        self.status
    }

    /// The response body as received.
    pub fn body(&self) -> &str {
        &self.body
    }
}

fn read_string<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> Option<&'a str> {
    object.get(name).and_then(|v| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verified_answer_is_read_in_full() {
        let result = RedeemResult::from_response(
            200,
            r#"{"context":"verified","signature":"verified",
                "verifiedAt":"2026-09-03T10:15:30Z","secondsSinceVerified":12}"#,
        );
        assert_eq!(result.context(), ContextOutcome::Verified);
        assert_eq!(result.context_value(), Some("verified"));
        assert_eq!(result.signature(), SignatureOutcome::Verified);
        assert!(result.factors().is_none());
        assert_eq!(
            result.verified_at().map(|d| d.to_rfc3339()),
            Some("2026-09-03T10:15:30+00:00".to_string())
        );
        assert_eq!(result.seconds_since_verified(), Some(12));
        assert_eq!(result.status(), 200);
        assert!(result.body().contains("verified"));
    }

    #[test]
    fn a_mismatch_names_its_factors() {
        let result = RedeemResult::from_response(
            200,
            r#"{"context":"mismatch","signature":"verified",
                "factors":{"transport":"verified","device":"mismatch",
                           "browserip":"verified"}}"#,
        );
        assert_eq!(result.context(), ContextOutcome::Mismatch);
        let factors = result.factors().expect("factors are present");
        assert_eq!(factors.len(), 3);
        assert_eq!(factors["transport"], FactorOutcome::Verified);
        assert_eq!(factors["device"], FactorOutcome::Mismatch);
    }

    #[test]
    fn the_four_browser_factors_are_read_into_their_own_members() {
        let result = RedeemResult::from_response(
            200,
            r#"{"signature":"verified","context":"mismatch",
                "factors":{"transport":"verified","device":"verified",
                           "browserip":"verified","connectionip":"verified",
                           "asn":"verified",
                           "platformname":"verified",
                           "platformversion":"misconfigured",
                           "browsername":"verified",
                           "browserversion":"mismatch"},
                "verifiedAt":"2026-08-07T09:15:32Z",
                "secondsSinceVerified":2}"#,
        );
        assert_eq!(result.context(), ContextOutcome::Mismatch);
        assert_eq!(
            result.factor(Factor::PlatformName),
            Some(FactorOutcome::Verified)
        );
        assert_eq!(
            result.factor(Factor::PlatformVersion),
            Some(FactorOutcome::Misconfigured)
        );
        assert_eq!(
            result.factor(Factor::BrowserName),
            Some(FactorOutcome::Verified)
        );
        assert_eq!(
            result.factor(Factor::BrowserVersion),
            Some(FactorOutcome::Mismatch)
        );
        for factor in [
            Factor::Transport,
            Factor::Device,
            Factor::BrowserIp,
            Factor::ConnectionIp,
            Factor::Asn,
        ] {
            assert_eq!(
                result.factor(factor),
                Some(FactorOutcome::Verified),
                "{factor:?}"
            );
        }
        assert_eq!(result.factors().unwrap().len(), 9);
    }

    #[test]
    fn an_old_browser_factor_populates_none_of_the_four() {
        let result = RedeemResult::from_response(
            200,
            r#"{"signature":"verified","context":"mismatch",
                "factors":{"transport":"verified","browser":"mismatch"}}"#,
        );
        for factor in [
            Factor::PlatformName,
            Factor::PlatformVersion,
            Factor::BrowserName,
            Factor::BrowserVersion,
        ] {
            assert_eq!(result.factor(factor), None, "{factor:?}");
        }
        assert_eq!(Factor::from_cloud("browser"), None);
        assert_eq!(
            result.factor(Factor::Transport),
            Some(FactorOutcome::Verified)
        );
    }

    #[test]
    fn factors_name_the_cloud_keys_in_the_documented_order() {
        let names: Vec<&str> = Factor::ALL.iter().map(|f| f.as_cloud()).collect();
        assert_eq!(
            names,
            [
                "transport",
                "device",
                "browserip",
                "connectionip",
                "asn",
                "platformname",
                "platformversion",
                "browsername",
                "browserversion",
            ]
        );
        for factor in Factor::ALL {
            assert_eq!(Factor::from_cloud(factor.as_cloud()), Some(factor));
        }
    }

    #[test]
    fn a_misconfigured_factor_is_not_a_mismatch() {
        let result = RedeemResult::from_response(
            200,
            r#"{"context":"misconfigured",
                "factors":{"transport":"misconfigured","device":"verified"}}"#,
        );
        assert_eq!(result.context(), ContextOutcome::Misconfigured);
        let factors = result.factors().expect("factors are present");
        assert_eq!(
            factors["transport"],
            FactorOutcome::Misconfigured,
            "the checking service could not determine this factor, which \
             says nothing about the identifier"
        );
        assert_ne!(factors["transport"], FactorOutcome::Mismatch);
        assert_eq!(factors["device"], FactorOutcome::Verified);
        assert!(
            !factors.values().any(|f| *f == FactorOutcome::Mismatch),
            "nothing here is a replay indicator"
        );
    }

    #[test]
    fn a_not_recorded_factor_is_its_own_outcome() {
        let result = RedeemResult::from_response(
            200,
            r#"{"context":"mismatch",
                "factors":{"transport":"notrecorded","device":"verified",
                           "browserip":"mismatch","asn":"misconfigured",
                           "platformversion":"notrecorded"}}"#,
        );
        let factors = result.factors().expect("factors are present");
        assert_eq!(
            factors["transport"],
            FactorOutcome::NotRecorded,
            "the creating service recorded no value for this factor, so the \
             identifier says nothing about it"
        );
        assert_eq!(factors["platformversion"], FactorOutcome::NotRecorded);
        assert_ne!(factors["transport"], FactorOutcome::Mismatch);
        assert_ne!(factors["transport"], FactorOutcome::Misconfigured);
        assert_eq!(factors["browserip"], FactorOutcome::Mismatch);
        assert_eq!(factors["asn"], FactorOutcome::Misconfigured);
        assert_eq!(factors["device"], FactorOutcome::Verified);
    }

    #[test]
    fn every_factor_outcome_reports_the_clouds_own_word() {
        for (outcome, word) in [
            (FactorOutcome::Verified, "verified"),
            (FactorOutcome::Mismatch, "mismatch"),
            (FactorOutcome::Misconfigured, "misconfigured"),
            (FactorOutcome::NotRecorded, "notrecorded"),
        ] {
            assert_eq!(outcome.as_cloud(), word);
            assert_eq!(FactorOutcome::from_cloud(Some(word)), outcome);
        }
    }

    #[test]
    fn an_unknown_factor_value_is_a_mismatch_not_a_pass() {
        let result = RedeemResult::from_response(
            200,
            r#"{"context":"mismatch","factors":{"asn":"something-new"}}"#,
        );
        assert_eq!(result.factors().unwrap()["asn"], FactorOutcome::Mismatch);
    }

    #[test]
    fn invaliddate_is_read_and_carries_no_factors() {
        let result = RedeemResult::from_response(200, r#"{"context":"invaliddate"}"#);
        assert_eq!(result.context(), ContextOutcome::InvalidDate);
        assert_eq!(result.context_value(), Some("invaliddate"));
        assert!(result.factors().is_none());
        assert_eq!(result.signature(), SignatureOutcome::Unknown);
        assert!(result.verified_at().is_none());
        assert!(result.seconds_since_verified().is_none());
    }

    #[test]
    fn a_503_is_unconfirmed() {
        let result = RedeemResult::from_response(503, r#"{"context":"unconfirmed"}"#);
        assert_eq!(result.context(), ContextOutcome::Unconfirmed);
        assert_eq!(result.status(), 503);

        let empty = RedeemResult::from_response(503, "");
        assert_eq!(empty.context(), ContextOutcome::Unconfirmed);
        assert_eq!(empty.body(), "");
    }

    #[test]
    fn unreadable_json_is_unreadable() {
        let result = RedeemResult::from_response(200, "<html>not json</html>");
        assert_eq!(result.context(), ContextOutcome::Unreadable);
        assert!(result.context_value().is_none());
        assert_eq!(result.body(), "<html>not json</html>");

        let array = RedeemResult::from_response(200, "[1,2,3]");
        assert_eq!(array.context(), ContextOutcome::Unreadable);
    }

    #[test]
    fn a_missing_or_unknown_context_is_unreadable() {
        let missing = RedeemResult::from_response(200, r#"{"signature":"verified"}"#);
        assert_eq!(missing.context(), ContextOutcome::Unreadable);
        assert!(missing.context_value().is_none());
        assert_eq!(missing.signature(), SignatureOutcome::Verified);

        let unknown = RedeemResult::from_response(200, r#"{"context":"brand-new"}"#);
        assert_eq!(unknown.context(), ContextOutcome::Unreadable);
        assert_eq!(
            unknown.context_value(),
            Some("brand-new"),
            "the raw word is kept"
        );
    }

    #[test]
    fn expired_carries_when_and_how_long_ago() {
        let result = RedeemResult::from_response(
            200,
            r#"{"context":"expired","verifiedAt":"2026-09-03T10:15:30Z",
                "secondsSinceVerified":900}"#,
        );
        assert_eq!(result.context(), ContextOutcome::Expired);
        assert!(result.verified_at().is_some());
        assert_eq!(result.seconds_since_verified(), Some(900));
    }

    #[test]
    fn a_verified_at_that_cannot_be_read_is_absent() {
        let result = RedeemResult::from_response(
            200,
            r#"{"context":"verified","verifiedAt":"yesterday",
                "secondsSinceVerified":"soon"}"#,
        );
        assert!(result.verified_at().is_none());
        assert!(result.seconds_since_verified().is_none());
    }
}
