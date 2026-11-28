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

//! The released `fodid-client` reading the answers the service sends today,
//! which add `status`, `humanConfidence`, `humanConfidenceNullReason` and
//! `obsolete` to what the released client was built against. Each check
//! asserts what the released client reports, so a change to an answer that
//! the released client would read differently fails here.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use fodid::{Creator, Crypto, FodId};
use fodid_client::{
    ContextOutcome, DidClient, DidHttpClient, DidHttpRequest, DidHttpResponse, FactorOutcome,
    LocalBoxFuture, RedeemResult, SignatureOutcome,
};

const VERIFIED_AT: &str = "2026-09-03T10:15:30+00:00";

/// What the released client reports for a redeem answer.
#[derive(Debug, PartialEq)]
struct Read {
    context: ContextOutcome,
    context_value: Option<String>,
    signature: SignatureOutcome,
    factors: Option<HashMap<String, FactorOutcome>>,
    verified_at: Option<String>,
    seconds_since_verified: Option<i64>,
    status: u16,
}

impl Read {
    fn of(result: &RedeemResult) -> Self {
        Self {
            context: result.context(),
            context_value: result.context_value().map(str::to_owned),
            signature: result.signature(),
            factors: result.factors().cloned(),
            verified_at: result.verified_at().map(|d| d.to_rfc3339()),
            seconds_since_verified: result.seconds_since_verified(),
            status: result.status(),
        }
    }
}

fn read(
    status: u16,
    context: ContextOutcome,
    signature: SignatureOutcome,
    factors: &[(&str, FactorOutcome)],
    seconds_since_verified: Option<i64>,
) -> Read {
    Read {
        context,
        context_value: Some(context.as_cloud().to_string()),
        signature,
        factors: (!factors.is_empty()).then(|| {
            factors
                .iter()
                .map(|(name, outcome)| (name.to_string(), *outcome))
                .collect()
        }),
        verified_at: seconds_since_verified.map(|_| VERIFIED_AT.to_string()),
        seconds_since_verified,
        status,
    }
}

/// Each redeem answer the service sends today, with what the released
/// client reports for it.
fn redeem_answers() -> Vec<(u16, &'static str, Read)> {
    use ContextOutcome as C;
    use FactorOutcome as F;
    use SignatureOutcome as S;
    vec![
        (
            200,
            r#"{"signature":"verified","context":"verified",
                "humanConfidence":"High","status":"success",
                "verifiedAt":"2026-09-03T10:15:30Z","secondsSinceVerified":12,
                "obsolete":["result is obsolete. Use sealedResult."]}"#,
            read(200, C::Verified, S::Verified, &[], Some(12)),
        ),
        (
            200,
            r#"{"signature":"verified","context":"mismatch",
                "factors":{"transport":"verified","device":"mismatch",
                           "asn":"misconfigured","platformversion":"notrecorded"},
                "humanConfidenceNullReason":"humanConfidence is a paid feature. You need a License Key valid for Human Confidence, presented on this call, to retrieve it.",
                "status":"success",
                "verifiedAt":"2026-09-03T10:15:30Z","secondsSinceVerified":3}"#,
            read(
                200,
                C::Mismatch,
                S::Verified,
                &[
                    ("transport", F::Verified),
                    ("device", F::Mismatch),
                    ("asn", F::Misconfigured),
                    ("platformversion", F::NotRecorded),
                ],
                Some(3),
            ),
        ),
        (
            200,
            r#"{"signature":"verified","context":"invaliddate",
                "humanConfidence":"Absent","status":"success",
                "verifiedAt":"2026-09-03T10:15:30Z","secondsSinceVerified":1,
                "obsolete":["result is obsolete. Use sealedResult.",
                            "challenge is obsolete. Use transaction."]}"#,
            read(200, C::InvalidDate, S::Verified, &[], Some(1)),
        ),
        (
            200,
            r#"{"context":"expired","status":"expired",
                "verifiedAt":"2026-09-03T10:15:30Z","secondsSinceVerified":900,
                "obsolete":["result is obsolete. Use sealedResult."]}"#,
            read(200, C::Expired, S::Unknown, &[], Some(900)),
        ),
        (
            200,
            r#"{"context":"replayed","status":"replayed"}"#,
            read(200, C::Replayed, S::Unknown, &[], None),
        ),
        (
            200,
            r#"{"context":"unreadable","status":"unreadable",
                "obsolete":["challenge is obsolete. Use transaction."]}"#,
            read(200, C::Unreadable, S::Unknown, &[], None),
        ),
        (
            503,
            r#"{"context":"unconfirmed","status":"unconfirmed"}"#,
            read(503, C::Unconfirmed, S::Unknown, &[], None),
        ),
    ]
}

#[test]
fn the_released_reader_reads_each_new_redeem_answer() {
    for (status, body, expected) in redeem_answers() {
        let result = RedeemResult::from_response(status, body);
        assert_eq!(Read::of(&result), expected, "{body}");
        assert_eq!(result.body(), body);
    }
}

#[tokio::test]
async fn the_released_client_redeems_each_new_answer() {
    let fod_id = signed_fod_id();
    for (status, body, expected) in redeem_answers() {
        let client = client_answering(vec![(status, body)]);
        let result = client
            .redeem_encoded(&fod_id, "sealed", Some("transaction"))
            .await
            .unwrap();
        assert_eq!(Read::of(&result), expected, "{body}");
    }
}

#[tokio::test]
async fn the_released_client_verifies_each_new_answer() {
    let fod_id = signed_fod_id();
    for (status, body, valid) in [
        (200, r#"{"valid":true,"humanConfidence":"High"}"#, true),
        (200, r#"{"valid":true,"humanConfidence":"Absent"}"#, true),
        (
            200,
            r#"{"valid":true,"humanConfidenceNullReason":"humanConfidence is a paid feature. You need a License Key valid for Human Confidence, presented on this call, to retrieve it."}"#,
            true,
        ),
        (400, r#"{"valid":false}"#, false),
    ] {
        let client = client_answering(vec![(status, body)]);
        assert_eq!(
            client.verify_encoded(&fod_id).await.unwrap(),
            valid,
            "{body}"
        );
    }
}

/// A client for the released crate that answers from the list given.
fn client_answering(answers: Vec<(u16, &str)>) -> DidClient {
    let transport = Canned(Mutex::new(
        answers
            .into_iter()
            .map(|(status, body)| DidHttpResponse {
                status,
                body: body.to_string(),
            })
            .collect(),
    ));
    DidClient::builder("resource-key")
        .endpoint("https://example.test/api/v4/")
        .licence_key("licence-key")
        .http_client(Arc::new(transport))
        .build()
        .unwrap()
}

/// Stands in for the network, answering canned responses in order.
struct Canned(Mutex<VecDeque<DidHttpResponse>>);

impl DidHttpClient for Canned {
    fn send<'a>(
        &'a self,
        _request: &'a DidHttpRequest,
    ) -> LocalBoxFuture<'a, Result<DidHttpResponse, String>> {
        Box::pin(async move {
            self.0
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "no canned response left".to_string())
        })
    }
}

/// A genuine 51Did, signed now, so the released client's local check
/// passes and the call is made.
fn signed_fod_id() -> String {
    let creator = Creator::new("51degrees.com", Crypto::new()).expect("create creator");
    // The payload header and match key, with usage bit 0 set because a
    // payload with no usage bit is refused.
    let mut payload = vec![0u8; 5 + 32];
    payload[0] = 0b0000_0001;
    let owid = creator.create(payload).expect("sign the envelope");
    FodId::from_owid(owid)
        .expect("a 51Did")
        .as_base64()
        .expect("encode")
}
