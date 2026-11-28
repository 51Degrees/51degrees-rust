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
use crate::outcome::{
    read_string, ContextOutcome, Factor, FactorOutcome, HumanConfidence, HumanConfidenceFields,
    RedemptionOutcome, SignatureOutcome,
};

/// The typed answer from the cloud's redeem endpoint, built by
/// [`DidClient::redeem`](crate::DidClient::redeem) from the JSON body.
///
/// [`RedeemResult::outcome`] says what happened at redemption, and only
/// [`RedemptionOutcome::Success`] carries an unsealed result, being the
/// creator context verdict and, for a licence key valid for Human
/// Confidence, the [`HumanConfidence`] Value the 51Did carries.
///
/// [`RedeemResult::body`] keeps the body as received and
/// [`RedeemResult::status`] the HTTP status, so nothing the cloud said is
/// lost in the mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedeemResult {
    context: ContextOutcome,
    context_value: Option<String>,
    outcome: RedemptionOutcome,
    status_value: Option<String>,
    signature: SignatureOutcome,
    factors: Option<HashMap<String, FactorOutcome>>,
    human_confidence: HumanConfidenceFields,
    verified_at: Option<DateTime<Utc>>,
    seconds_since_verified: Option<i64>,
    status: u16,
    body: String,
}

impl RedeemResult {
    /// Creates a result from its parts. Callers normally get one from
    /// [`RedeemResult::from_response`] rather than building one.
    ///
    /// [`RedeemResult::outcome`] is worked out from `context`, as
    /// [`RedeemResult::from_response`] does for an answer with no `status`
    /// field, and no Human Confidence Value is held. A 503 built this way
    /// carries [`ContextOutcome::Unconfirmed`], as one read from a response
    /// does.
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
            outcome: outcome_without_status(context),
            context,
            context_value,
            status_value: None,
            signature,
            factors,
            human_confidence: HumanConfidenceFields::default(),
            verified_at,
            seconds_since_verified,
            status,
            body: body.into(),
        }
    }

    /// Builds a result from a redeem response.
    ///
    /// A 503 is [`RedemptionOutcome::Unconfirmed`] and
    /// [`ContextOutcome::Unconfirmed`] whatever the body says, because the
    /// status is the service's own statement that it could not confirm
    /// first use. [`RedeemResult::context_value`] and
    /// [`RedeemResult::status_value`] are both `unconfirmed`, the 503's own
    /// word. Nothing else is read from a 503 body, so it carries no
    /// signature, factors, times or Human Confidence fields, and the body is
    /// kept in [`RedeemResult::body`].
    ///
    /// Otherwise the body is read as a JSON object, and a body that is not
    /// one gives [`RedemptionOutcome::Unreadable`] with the body kept in
    /// [`RedeemResult::body`]. The outcome is read from `status`. A service
    /// that predates that field wrote the reason to `context` alone, so
    /// where `status` is absent the outcome is worked out from `context`,
    /// and a verdict there is a success.
    ///
    /// [`RedeemResult::context`] is read as before, so a body that is not a
    /// JSON object, or one that carries no `context`, gives the same
    /// [`ContextOutcome`] it always has. Factor values are read with
    /// [`FactorOutcome::from_cloud`], so neither `misconfigured` nor
    /// `notrecorded` falls through to a mismatch. Fields this client does
    /// not read, such as `obsolete`, are kept in [`RedeemResult::body`].
    #[allow(deprecated)]
    pub fn from_response(status: u16, body: &str) -> Self {
        if status == 503 {
            // The words the 503 itself stands for, not read from the body, so
            // a caller relaying the raw words relays `unconfirmed`.
            let word = RedemptionOutcome::Unconfirmed.as_cloud();
            let mut result = Self::without_fields(ContextOutcome::Unconfirmed, status, body);
            result.context_value = Some(word.to_owned());
            result.status_value = Some(word.to_owned());
            return result;
        }
        let value: serde_json::Value = match serde_json::from_str(body) {
            Ok(value) => value,
            Err(_) => return Self::without_fields(ContextOutcome::Unreadable, status, body),
        };
        let Some(root) = value.as_object() else {
            return Self::without_fields(ContextOutcome::Unreadable, status, body);
        };
        let context_value = read_string(root, "context");
        let context = ContextOutcome::from_cloud(context_value);
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
        let status_value = read_string(root, "status");
        let outcome = match status_value {
            Some(word) => RedemptionOutcome::from_cloud(Some(word)),
            None => outcome_without_status(context),
        };
        Self {
            context,
            context_value: context_value.map(str::to_owned),
            outcome,
            status_value: status_value.map(str::to_owned),
            signature: SignatureOutcome::from_cloud(read_string(root, "signature")),
            factors,
            human_confidence: HumanConfidenceFields::read(root),
            verified_at,
            seconds_since_verified,
            status,
            body: body.to_owned(),
        }
    }

    /// A result that reads nothing from the body but keeps it, for a 503 or
    /// a body that is not a JSON object.
    fn without_fields(context: ContextOutcome, status: u16, body: &str) -> Self {
        Self {
            context,
            context_value: None,
            outcome: outcome_without_status(context),
            status_value: None,
            signature: SignatureOutcome::Unknown,
            factors: None,
            human_confidence: HumanConfidenceFields::default(),
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
    /// the body carried none. On a 503 it is `unconfirmed`, whatever the
    /// body says.
    pub fn context_value(&self) -> Option<&str> {
        self.context_value.as_deref()
    }

    /// What happened at redemption, mapped from the `status` string, or
    /// worked out from the HTTP status and `context` where the service sent
    /// no `status`. A string this client does not recognise maps to
    /// [`RedemptionOutcome::Unreadable`], so an unexpected answer never
    /// reads as a success. Read [`RedeemResult::context`] as a verdict only
    /// when this is [`RedemptionOutcome::Success`].
    pub fn outcome(&self) -> RedemptionOutcome {
        self.outcome
    }

    /// The `status` string exactly as the cloud sent it, or `None` when the
    /// body carried none, as from a service that predates the field. On a
    /// 503 it is `unconfirmed`, whatever the body says.
    pub fn status_value(&self) -> Option<&str> {
        self.status_value.as_deref()
    }

    /// The Human Confidence Value the 51Did carries, mapped from the
    /// `humanConfidence` string.
    ///
    /// Present only on [`RedemptionOutcome::Success`] for a call that
    /// presented a licence key valid for Human Confidence, and then
    /// [`HumanConfidence::Absent`] where the 51Did carries no Value. `None`
    /// when the answer carried no `humanConfidence`, and
    /// [`RedeemResult::human_confidence_null_reason`] then says why where
    /// the cloud gave a reason. A word this client does not know reads as
    /// [`HumanConfidence::Absent`], with the word kept in
    /// [`RedeemResult::human_confidence_value`].
    pub fn human_confidence(&self) -> Option<HumanConfidence> {
        self.human_confidence.value
    }

    /// The `humanConfidence` string exactly as the cloud sent it, or `None`
    /// when the body carried none.
    pub fn human_confidence_value(&self) -> Option<&str> {
        self.human_confidence.word.as_deref()
    }

    /// Why [`RedeemResult::human_confidence`] is not present, from the
    /// `humanConfidenceNullReason` string, which the cloud sends on a
    /// success to a call that did not present a licence key valid for Human
    /// Confidence. `None` when the body carried none.
    pub fn human_confidence_null_reason(&self) -> Option<&str> {
        self.human_confidence.null_reason.as_deref()
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

    /// The HTTP status the cloud answered with, 503 for
    /// [`RedemptionOutcome::Unconfirmed`] and 200 for every other outcome.
    /// Not the `status` field of the body, which [`RedeemResult::outcome`]
    /// reads.
    pub fn status(&self) -> u16 {
        self.status
    }

    /// The response body as received.
    pub fn body(&self) -> &str {
        &self.body
    }
}

/// The outcome of an answer that carries no `status` field. A service that
/// predates the field wrote the reason to `context` alone, and a verdict
/// there means the sealed result was redeemed. Only the verdicts the
/// [package surface](https://github.com/51Degrees/specifications/blob/main/did-specification/package-surface.md#the-redemption-outcome)
/// lists count as one, so `notcheckable` is Unreadable like any other word.
#[allow(deprecated)]
fn outcome_without_status(context: ContextOutcome) -> RedemptionOutcome {
    match context {
        ContextOutcome::Verified
        | ContextOutcome::Mismatch
        | ContextOutcome::NoContext
        | ContextOutcome::Misconfigured
        | ContextOutcome::InvalidDate => RedemptionOutcome::Success,
        ContextOutcome::Expired => RedemptionOutcome::Expired,
        ContextOutcome::Replayed => RedemptionOutcome::Replayed,
        ContextOutcome::Unconfirmed => RedemptionOutcome::Unconfirmed,
        ContextOutcome::NotCheckable | ContextOutcome::Unreadable => RedemptionOutcome::Unreadable,
    }
}

#[cfg(test)]
// The tests read the deprecated ContextOutcome members too, to show they
// still work.
#[allow(deprecated)]
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
    fn a_factor_word_is_read_whatever_its_letter_case() {
        let result = RedeemResult::from_response(
            200,
            r#"{"context":"mismatch",
                "factors":{"transport":"Verified","device":"NotRecorded",
                           "browserip":"MISCONFIGURED","asn":"Mismatch",
                           "platformname":"Unknown"}}"#,
        );
        let factors = result.factors().expect("factors are present");
        assert_eq!(factors["transport"], FactorOutcome::Verified);
        assert_eq!(
            factors["device"],
            FactorOutcome::NotRecorded,
            "a factor the creating service did not record must not read as \
             a mismatch because of its letter case"
        );
        assert_eq!(factors["browserip"], FactorOutcome::Misconfigured);
        assert_eq!(factors["asn"], FactorOutcome::Mismatch);
        assert_eq!(
            factors["platformname"],
            FactorOutcome::Mismatch,
            "a word this client does not know is still a mismatch"
        );
    }

    #[test]
    fn a_context_or_signature_word_is_read_whatever_its_letter_case() {
        let result =
            RedeemResult::from_response(200, r#"{"context":"Verified","signature":"VERIFIED"}"#);
        assert_eq!(result.context(), ContextOutcome::Verified);
        assert_eq!(
            result.context_value(),
            Some("Verified"),
            "the word is kept as the service sent it"
        );
        assert_eq!(result.signature(), SignatureOutcome::Verified);

        let result =
            RedeemResult::from_response(200, r#"{"context":"NoContext","signature":"Invalid"}"#);
        assert_eq!(result.context(), ContextOutcome::NoContext);
        assert_eq!(result.signature(), SignatureOutcome::Invalid);

        let result = RedeemResult::from_response(200, r#"{"context":"Matched"}"#);
        assert_eq!(
            result.context(),
            ContextOutcome::Unreadable,
            "a word this client does not know is still unreadable"
        );
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
        assert_eq!(empty.context_value(), Some("unconfirmed"));
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

    // Answers from a service that sends status, humanConfidence and
    // obsolete. Each is read exactly as today's answer with the same
    // context is read, and the new members read the added fields.

    /// Every answer of today's redeem, paired with the same answer as a
    /// service that sends the new fields gives it.
    const TODAY_AND_NEW: [(u16, &str, &str); 7] = [
        (
            200,
            r#"{"context":"verified","signature":"verified",
                "verifiedAt":"2026-09-03T10:15:30Z","secondsSinceVerified":12}"#,
            r#"{"signature":"verified","context":"verified",
                "humanConfidence":"High","status":"success",
                "verifiedAt":"2026-09-03T10:15:30Z","secondsSinceVerified":12,
                "obsolete":["result is obsolete. Use sealedResult."]}"#,
        ),
        (
            200,
            r#"{"context":"mismatch","signature":"verified",
                "factors":{"transport":"verified","device":"mismatch",
                           "asn":"misconfigured","platformversion":"notrecorded"},
                "verifiedAt":"2026-09-03T10:15:30Z","secondsSinceVerified":3}"#,
            r#"{"signature":"verified","context":"mismatch",
                "factors":{"transport":"verified","device":"mismatch",
                           "asn":"misconfigured","platformversion":"notrecorded"},
                "humanConfidenceNullReason":"humanConfidence is a paid feature. You need a License Key valid for Human Confidence, presented on this call, to retrieve it.",
                "status":"success",
                "verifiedAt":"2026-09-03T10:15:30Z","secondsSinceVerified":3}"#,
        ),
        (
            200,
            r#"{"context":"invaliddate","signature":"verified"}"#,
            r#"{"signature":"verified","context":"invaliddate",
                "humanConfidence":"Absent","status":"success",
                "verifiedAt":"2026-09-03T10:15:30Z","secondsSinceVerified":1,
                "obsolete":["result is obsolete. Use sealedResult.",
                            "challenge is obsolete. Use transaction."]}"#,
        ),
        (
            200,
            r#"{"context":"expired","verifiedAt":"2026-09-03T10:15:30Z",
                "secondsSinceVerified":900}"#,
            r#"{"context":"expired","status":"expired",
                "verifiedAt":"2026-09-03T10:15:30Z","secondsSinceVerified":900,
                "obsolete":["result is obsolete. Use sealedResult."]}"#,
        ),
        (
            200,
            r#"{"context":"replayed"}"#,
            r#"{"context":"replayed","status":"replayed"}"#,
        ),
        (
            200,
            r#"{"context":"unreadable"}"#,
            r#"{"context":"unreadable","status":"unreadable",
                "obsolete":["challenge is obsolete. Use transaction."]}"#,
        ),
        (
            503,
            r#"{"context":"unconfirmed"}"#,
            r#"{"context":"unconfirmed","status":"unconfirmed"}"#,
        ),
    ];

    #[test]
    fn a_new_answer_reads_exactly_as_todays() {
        for (status, today, new) in TODAY_AND_NEW {
            let today = RedeemResult::from_response(status, today);
            let new = RedeemResult::from_response(status, new);
            assert_eq!(new.context(), today.context(), "{new:?}");
            assert_eq!(new.context_value(), today.context_value(), "{new:?}");
            assert_eq!(new.signature(), today.signature(), "{new:?}");
            assert_eq!(new.factors(), today.factors(), "{new:?}");
            assert_eq!(new.status(), today.status(), "{new:?}");
            assert_eq!(new.outcome(), today.outcome(), "{new:?}");
            if today.verified_at().is_some() {
                assert_eq!(new.verified_at(), today.verified_at(), "{new:?}");
                assert_eq!(
                    new.seconds_since_verified(),
                    today.seconds_since_verified(),
                    "{new:?}"
                );
            }
        }
    }

    #[test]
    fn status_is_read_into_the_outcome() {
        for (status, word, outcome) in [
            (200, "success", RedemptionOutcome::Success),
            (200, "expired", RedemptionOutcome::Expired),
            (200, "replayed", RedemptionOutcome::Replayed),
            (200, "unreadable", RedemptionOutcome::Unreadable),
            (503, "unconfirmed", RedemptionOutcome::Unconfirmed),
        ] {
            let body = format!(r#"{{"context":"verified","status":"{word}"}}"#);
            let result = RedeemResult::from_response(status, &body);
            assert_eq!(result.outcome(), outcome, "{word}");
            assert_eq!(result.status_value(), Some(word));
            assert_eq!(outcome.as_cloud(), word);
            assert_eq!(RedemptionOutcome::from_cloud(Some(word)), outcome);
        }
    }

    #[test]
    fn an_unknown_status_is_unreadable_not_a_success() {
        let result =
            RedeemResult::from_response(200, r#"{"context":"verified","status":"Accepted"}"#);
        assert_eq!(result.outcome(), RedemptionOutcome::Unreadable);
        assert_eq!(
            result.status_value(),
            Some("Accepted"),
            "the raw word is kept"
        );
        let upper =
            RedeemResult::from_response(200, r#"{"context":"verified","status":"SUCCESS"}"#);
        assert_eq!(upper.outcome(), RedemptionOutcome::Success);
    }

    #[test]
    fn a_503_is_unconfirmed_whatever_status_says() {
        let result =
            RedeemResult::from_response(503, r#"{"context":"verified","status":"success"}"#);
        assert_eq!(result.outcome(), RedemptionOutcome::Unconfirmed);
        assert_eq!(result.context(), ContextOutcome::Unconfirmed);
        assert_eq!(
            RedeemResult::from_response(503, "").outcome(),
            RedemptionOutcome::Unconfirmed
        );
    }

    #[test]
    fn a_503_carries_nothing_from_its_body() {
        let body = r#"{"status":"success","context":"verified","signature":"verified",
            "factors":{"transport":"verified","device":"verified"},
            "humanConfidence":"High",
            "humanConfidenceNullReason":"humanConfidence is a paid feature.",
            "verifiedAt":"2026-09-03T10:15:30Z","secondsSinceVerified":12}"#;
        let result = RedeemResult::from_response(503, body);
        assert_eq!(result.outcome(), RedemptionOutcome::Unconfirmed);
        assert_eq!(result.context(), ContextOutcome::Unconfirmed);
        // The 503's own word, not the body's `verified` and `success`.
        assert_eq!(result.context_value(), Some("unconfirmed"));
        assert_eq!(result.status_value(), Some("unconfirmed"));
        assert_eq!(result.signature(), SignatureOutcome::Unknown);
        assert!(result.factors().is_none());
        assert_eq!(result.factor(Factor::Transport), None);
        assert_eq!(result.human_confidence(), None);
        assert_eq!(result.human_confidence_value(), None);
        assert_eq!(result.human_confidence_null_reason(), None);
        assert!(result.verified_at().is_none());
        assert!(result.seconds_since_verified().is_none());
        assert_eq!(result.status(), 503);
        assert_eq!(result.body(), body, "the body is kept as received");
    }

    #[test]
    fn without_status_the_outcome_is_worked_out_from_context() {
        for (status, body, outcome) in [
            (200, r#"{"context":"verified"}"#, RedemptionOutcome::Success),
            (200, r#"{"context":"mismatch"}"#, RedemptionOutcome::Success),
            (
                200,
                r#"{"context":"nocontext"}"#,
                RedemptionOutcome::Success,
            ),
            (
                200,
                r#"{"context":"misconfigured"}"#,
                RedemptionOutcome::Success,
            ),
            (
                200,
                r#"{"context":"invaliddate"}"#,
                RedemptionOutcome::Success,
            ),
            (200, r#"{"context":"expired"}"#, RedemptionOutcome::Expired),
            (
                200,
                r#"{"context":"replayed"}"#,
                RedemptionOutcome::Replayed,
            ),
            (
                200,
                r#"{"context":"unreadable"}"#,
                RedemptionOutcome::Unreadable,
            ),
            (
                200,
                r#"{"context":"brand-new"}"#,
                RedemptionOutcome::Unreadable,
            ),
            (
                200,
                r#"{"signature":"verified"}"#,
                RedemptionOutcome::Unreadable,
            ),
            (200, "<html>not json</html>", RedemptionOutcome::Unreadable),
            (
                503,
                r#"{"context":"unconfirmed"}"#,
                RedemptionOutcome::Unconfirmed,
            ),
        ] {
            let result = RedeemResult::from_response(status, body);
            assert_eq!(result.outcome(), outcome, "{body}");
            // Only a 503 has a status word, its own, since none was sent.
            let word = (status == 503).then_some("unconfirmed");
            assert_eq!(result.status_value(), word, "{body}");
        }
    }

    #[test]
    fn without_status_notcheckable_is_unreadable_not_a_success() {
        // notcheckable is not one of the verdicts the specification lists as
        // a success, so with no status it can't be read as one.
        let result = RedeemResult::from_response(200, r#"{"context":"notcheckable"}"#);
        assert_eq!(result.outcome(), RedemptionOutcome::Unreadable);
        assert_eq!(result.context(), ContextOutcome::NotCheckable);
        assert_eq!(result.context_value(), Some("notcheckable"));
        let built = RedeemResult::new(
            ContextOutcome::NotCheckable,
            None,
            SignatureOutcome::Verified,
            None,
            None,
            None,
            200,
            "",
        );
        assert_eq!(built.outcome(), RedemptionOutcome::Unreadable);
        // With a status the status decides, as for every other word.
        let with_status =
            RedeemResult::from_response(200, r#"{"context":"notcheckable","status":"success"}"#);
        assert_eq!(with_status.outcome(), RedemptionOutcome::Success);
    }

    #[test]
    fn a_result_built_from_its_parts_works_out_its_outcome() {
        let result = RedeemResult::new(
            ContextOutcome::Replayed,
            Some("replayed".to_string()),
            SignatureOutcome::Unknown,
            None,
            None,
            None,
            200,
            "",
        );
        assert_eq!(result.outcome(), RedemptionOutcome::Replayed);
        assert_eq!(result.human_confidence(), None);
        let result = RedeemResult::new(
            ContextOutcome::Verified,
            None,
            SignatureOutcome::Verified,
            None,
            None,
            None,
            200,
            "",
        );
        assert_eq!(result.outcome(), RedemptionOutcome::Success);
    }

    #[test]
    fn each_human_confidence_value_is_read() {
        for (word, value) in [
            ("High", HumanConfidence::High),
            ("Medium", HumanConfidence::Medium),
            ("Low", HumanConfidence::Low),
            ("Absent", HumanConfidence::Absent),
            ("absent", HumanConfidence::Absent),
        ] {
            let body = format!(
                r#"{{"signature":"verified","context":"verified",
                    "humanConfidence":"{word}","status":"success"}}"#
            );
            let result = RedeemResult::from_response(200, &body);
            assert_eq!(result.human_confidence(), Some(value), "{word}");
            assert_eq!(result.human_confidence_null_reason(), None);
        }
        assert_eq!(HumanConfidence::Absent.as_cloud(), "Absent");
    }

    #[test]
    fn without_a_licence_for_the_value_the_reason_is_read() {
        let result = RedeemResult::from_response(
            200,
            r#"{"signature":"verified","context":"verified",
                "humanConfidenceNullReason":"humanConfidence is a paid feature.",
                "status":"success"}"#,
        );
        assert_eq!(result.human_confidence(), None);
        assert_eq!(
            result.human_confidence_null_reason(),
            Some("humanConfidence is a paid feature.")
        );
    }

    #[test]
    fn an_unknown_human_confidence_word_is_absent_and_the_word_is_kept() {
        let result = RedeemResult::from_response(
            200,
            r#"{"context":"verified","humanConfidence":"Certain","status":"success"}"#,
        );
        assert_eq!(result.human_confidence(), Some(HumanConfidence::Absent));
        assert_eq!(result.human_confidence_value(), Some("Certain"));
        assert_eq!(result.human_confidence_null_reason(), None);
        let number = RedeemResult::from_response(
            200,
            r#"{"context":"verified","humanConfidence":3,"status":"success"}"#,
        );
        assert_eq!(number.human_confidence(), Some(HumanConfidence::Absent));
        let empty = RedeemResult::from_response(
            200,
            r#"{"context":"verified","humanConfidence":"","status":"success"}"#,
        );
        assert_eq!(empty.human_confidence(), Some(HumanConfidence::Absent));
    }

    #[test]
    fn a_missing_human_confidence_is_none_not_absent() {
        let missing =
            RedeemResult::from_response(200, r#"{"context":"verified","status":"success"}"#);
        assert_eq!(missing.human_confidence(), None);
        assert_eq!(missing.human_confidence_value(), None);
        assert_eq!(missing.human_confidence_null_reason(), None);
        let null = RedeemResult::from_response(
            200,
            r#"{"context":"verified","humanConfidence":null,"status":"success"}"#,
        );
        assert_eq!(null.human_confidence(), None);
        let with_reason = RedeemResult::from_response(
            200,
            r#"{"context":"verified","status":"success",
                "humanConfidenceNullReason":"humanConfidence is a paid feature."}"#,
        );
        assert_eq!(with_reason.human_confidence(), None);
        assert_eq!(with_reason.human_confidence_value(), None);
        assert_eq!(
            with_reason.human_confidence_null_reason(),
            Some("humanConfidence is a paid feature.")
        );
        let known = RedeemResult::from_response(
            200,
            r#"{"context":"verified","humanConfidence":"High","status":"success"}"#,
        );
        assert_eq!(known.human_confidence_value(), Some("High"));
    }

    #[test]
    fn verify_and_redeem_read_the_human_confidence_fields_the_same() {
        for fields in [
            r#""humanConfidence":"High""#,
            r#""humanConfidence":"absent""#,
            r#""humanConfidence":"Certain""#,
            r#""humanConfidence":3"#,
            r#""humanConfidence":null"#,
            r#""humanConfidenceNullReason":"humanConfidence is a paid feature.""#,
            r#""other":1"#,
        ] {
            let redeem = RedeemResult::from_response(
                200,
                &format!(r#"{{"context":"verified","status":"success",{fields}}}"#),
            );
            let verify =
                crate::VerifyAnswer::from_response(200, &format!(r#"{{"valid":true,{fields}}}"#))
                    .unwrap();
            assert_eq!(
                redeem.human_confidence(),
                verify.human_confidence(),
                "{fields}"
            );
            assert_eq!(
                redeem.human_confidence_value(),
                verify.human_confidence_value(),
                "{fields}"
            );
            assert_eq!(
                redeem.human_confidence_null_reason(),
                verify.human_confidence_null_reason(),
                "{fields}"
            );
        }
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
