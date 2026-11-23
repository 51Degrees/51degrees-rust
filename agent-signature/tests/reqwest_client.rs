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

//! The built-in client, against a local server.

#![cfg(feature = "reqwest-client")]

use std::thread;
use std::time::Duration;

use fiftyone_agent_signature::{HttpClient, HttpRequest, ReqwestClient};

#[test]
fn the_built_in_client_follows_no_redirect_and_stops_reading_past_the_limit() {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let serving = thread::spawn(move || {
        for _ in 0..2 {
            let request = server.recv().unwrap();
            let response = if request.url() == "/moved" {
                tiny_http::Response::from_string(String::new())
                    .with_status_code(302)
                    .with_header(
                        tiny_http::Header::from_bytes(&b"Location"[..], &b"/keys"[..]).unwrap(),
                    )
            } else {
                tiny_http::Response::from_string("x".repeat(100))
            };
            // The client stops reading past the limit and may close the
            // connection first, so a failed write is not a failure here.
            let _ = request.respond(response);
        }
    });

    let client = ReqwestClient::new().unwrap();
    let get = |path: &str| {
        let url = format!("http://127.0.0.1:{port}{path}");
        client
            .get(&HttpRequest {
                url: &url,
                accept: "application/http-message-signatures-directory+json",
                user_agent: "test",
                timeout: Duration::from_secs(30),
                max_response_bytes: 10,
            })
            .unwrap()
    };

    let moved = get("/moved");
    assert_eq!(moved.status, 302);
    assert_eq!(moved.url, format!("http://127.0.0.1:{port}/moved"));

    let keys = get("/keys");
    assert_eq!(keys.status, 200);
    // One byte past the limit is read, which is enough to refuse the body.
    assert_eq!(keys.body.len(), 11);

    serving.join().unwrap();
}
