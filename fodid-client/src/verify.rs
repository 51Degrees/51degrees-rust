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

//! The typed answer from the verify endpoint.

use crate::outcome::HumanConfidence;

/// The typed answer from the cloud's verify endpoint, built by
/// [`DidClient::verify_detailed`](crate::DidClient::verify_detailed) from
/// the JSON body.
///
/// [`VerifyAnswer::body`] keeps the body as received and
/// [`VerifyAnswer::status`] the HTTP status, so nothing the cloud said is
/// lost in the mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyAnswer {
    valid: bool,
    human_confidence: Option<HumanConfidence>,
    human_confidence_value: Option<String>,
    human_confidence_null_reason: Option<String>,
    status: u16,
    body: String,
}

impl VerifyAnswer {
    /// Builds an answer from a verify response, or `None` when the body is
    /// not a JSON object carrying the `valid` boolean.
    pub fn from_response(status: u16, body: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(body).ok()?;
        let root = value.as_object()?;
        let valid = root.get("valid")?.as_bool()?;
        let read_string = |name: &str| root.get(name).and_then(|v| v.as_str());
        let human_confidence_value = read_string("humanConfidence");
        Some(Self {
            valid,
            human_confidence: HumanConfidence::from_field(root.get("humanConfidence")),
            human_confidence_value: human_confidence_value.map(str::to_owned),
            human_confidence_null_reason: read_string("humanConfidenceNullReason")
                .map(str::to_owned),
            status,
            body: body.to_owned(),
        })
    }

    /// Whether the 51Did has the right shape and a genuine signature, the
    /// same answer [`DidClient::verify`](crate::DidClient::verify) gives.
    pub fn valid(&self) -> bool {
        self.valid
    }

    /// The Human Confidence Value the 51Did carries, mapped from the
    /// `humanConfidence` string.
    ///
    /// Present only for a 51Did whose signature verifies, to a call that
    /// presented a licence key valid for Human Confidence, and then
    /// [`HumanConfidence::Absent`] where the 51Did carries no Value. `None`
    /// when the answer carried no `humanConfidence`, and
    /// [`VerifyAnswer::human_confidence_null_reason`] then says why where
    /// the cloud gave a reason. A word this client does not know reads as
    /// [`HumanConfidence::Absent`], with the word kept in
    /// [`VerifyAnswer::human_confidence_value`].
    pub fn human_confidence(&self) -> Option<HumanConfidence> {
        self.human_confidence
    }

    /// The `humanConfidence` string exactly as the cloud sent it, or `None`
    /// when the body carried none.
    pub fn human_confidence_value(&self) -> Option<&str> {
        self.human_confidence_value.as_deref()
    }

    /// Why [`VerifyAnswer::human_confidence`] is not present, from the
    /// `humanConfidenceNullReason` string, which the cloud sends for a valid
    /// 51Did to a call that did not present a licence key valid for Human
    /// Confidence. `None` when the body carried none.
    pub fn human_confidence_null_reason(&self) -> Option<&str> {
        self.human_confidence_null_reason.as_deref()
    }

    /// The HTTP status the cloud answered with, 200 for a valid 51Did and
    /// 400 for one that is not.
    pub fn status(&self) -> u16 {
        self.status
    }

    /// The response body as received.
    pub fn body(&self) -> &str {
        &self.body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todays_answers_are_read_as_before() {
        let valid = VerifyAnswer::from_response(200, r#"{"valid":true}"#).unwrap();
        assert!(valid.valid());
        assert_eq!(valid.human_confidence(), None);
        assert_eq!(valid.human_confidence_null_reason(), None);
        assert_eq!(valid.status(), 200);

        let invalid = VerifyAnswer::from_response(400, r#"{"valid":false}"#).unwrap();
        assert!(!invalid.valid());
        assert_eq!(invalid.status(), 400);
    }

    #[test]
    fn an_answer_with_a_value_is_read() {
        for (word, value) in [
            ("High", HumanConfidence::High),
            ("Medium", HumanConfidence::Medium),
            ("Low", HumanConfidence::Low),
            ("Absent", HumanConfidence::Absent),
        ] {
            let body = format!(r#"{{"valid":true,"humanConfidence":"{word}"}}"#);
            let answer = VerifyAnswer::from_response(200, &body).unwrap();
            assert!(answer.valid());
            assert_eq!(answer.human_confidence(), Some(value), "{word}");
            assert_eq!(value.as_cloud(), word);
            assert_eq!(answer.human_confidence_null_reason(), None);
        }
    }

    #[test]
    fn an_answer_without_a_licence_for_the_value_says_why() {
        let answer = VerifyAnswer::from_response(
            200,
            r#"{"valid":true,"humanConfidenceNullReason":"humanConfidence is a paid feature. You need a License Key valid for Human Confidence, presented on this call, to retrieve it. Visit https://51degrees.com/pricing for details"}"#,
        )
        .unwrap();
        assert!(answer.valid());
        assert_eq!(answer.human_confidence(), None);
        assert!(answer
            .human_confidence_null_reason()
            .unwrap()
            .starts_with("humanConfidence is a paid feature."));
    }

    #[test]
    fn an_unknown_value_is_absent_and_the_word_is_kept() {
        let answer =
            VerifyAnswer::from_response(200, r#"{"valid":true,"humanConfidence":"Certain"}"#)
                .unwrap();
        assert_eq!(answer.human_confidence(), Some(HumanConfidence::Absent));
        assert_eq!(answer.human_confidence_value(), Some("Certain"));
        assert_eq!(answer.human_confidence_null_reason(), None);
        let number =
            VerifyAnswer::from_response(200, r#"{"valid":true,"humanConfidence":3}"#).unwrap();
        assert_eq!(number.human_confidence(), Some(HumanConfidence::Absent));
        assert_eq!(number.human_confidence_value(), None);
        let lower =
            VerifyAnswer::from_response(200, r#"{"valid":true,"humanConfidence":"high"}"#).unwrap();
        assert_eq!(lower.human_confidence(), Some(HumanConfidence::High));
        assert_eq!(lower.human_confidence_value(), Some("high"));
    }

    #[test]
    fn a_missing_value_is_none_not_absent() {
        let missing = VerifyAnswer::from_response(200, r#"{"valid":true}"#).unwrap();
        assert_eq!(missing.human_confidence(), None);
        assert_eq!(missing.human_confidence_value(), None);
        let null =
            VerifyAnswer::from_response(200, r#"{"valid":true,"humanConfidence":null}"#).unwrap();
        assert_eq!(null.human_confidence(), None);
        let with_reason = VerifyAnswer::from_response(
            200,
            r#"{"valid":true,"humanConfidenceNullReason":"humanConfidence is a paid feature."}"#,
        )
        .unwrap();
        assert_eq!(with_reason.human_confidence(), None);
        assert_eq!(with_reason.human_confidence_value(), None);
        assert_eq!(
            with_reason.human_confidence_null_reason(),
            Some("humanConfidence is a paid feature.")
        );
        let unknown =
            VerifyAnswer::from_response(200, r#"{"valid":true,"humanConfidence":"Certain"}"#)
                .unwrap();
        assert_ne!(unknown.human_confidence(), missing.human_confidence());
    }

    #[test]
    fn a_body_without_valid_is_not_an_answer() {
        assert!(VerifyAnswer::from_response(200, "not json").is_none());
        assert!(VerifyAnswer::from_response(200, "[true]").is_none());
        assert!(VerifyAnswer::from_response(400, r#"{"errors":["bad"]}"#).is_none());
        assert!(VerifyAnswer::from_response(200, r#"{"valid":"yes"}"#).is_none());
    }
}
