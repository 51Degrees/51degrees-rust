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

//! The agent signature pipeline: the Sequence element and the agent
//! signature element, whose key directories come from a client inside this
//! program, as the .NET producer builds it.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use fiftyone_agent_signature::constants::DIRECTORY_MEDIA_TYPE;
use fiftyone_agent_signature::{AgentSignatureElement, HttpClient, HttpRequest, HttpResponse};
use fiftyone_pipeline_core::Pipeline;
use fiftyone_pipeline_engines_fiftyone::SequenceElement;

/// The `pipeline` of a case that runs through this pipeline.
pub const PIPELINE: &str = "agent-signature";

/// Where the agent's key directory is served.
const AGENT: &str = "https://agent.parity.test/.well-known/http-message-signatures-directory";

/// Where the slow agent's key directory is served.
const SLOW_AGENT: &str =
    "https://slow-agent.parity.test/.well-known/http-message-signatures-directory";

/// The public part of the Ed25519 test key of RFC 9421 Appendix B.1.4.
const DIRECTORY: &str = concat!(
    r#"{"keys":[{"kty":"OKP","crv":"Ed25519","kid":"test-key-ed25519","#,
    r#""x":"JrQLj5P_89iXES9-vFgrIy29clF9CC_oPPsw3c5D0bs"}],"purpose":"parity"}"#
);

/// Long enough that the first fetch of a directory the client serves at once
/// never runs out, and shorter than the slow agent's delay.
const WAIT_BUDGET: Duration = Duration::from_secs(2);

/// How long the slow agent's directory takes to answer.
const SLOW_DELAY: Duration = Duration::from_secs(10);

/// Serves the agents' key directories in place of the network. The slow
/// agent answers after the wait budget, so its first request reports
/// Timeout. Any other address is not found.
struct KeyDirectories;

impl HttpClient for KeyDirectories {
    fn get(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
        if request.url == SLOW_AGENT {
            thread::sleep(SLOW_DELAY);
        } else if request.url != AGENT {
            return Ok(HttpResponse {
                status: 404,
                url: request.url.to_owned(),
                ..HttpResponse::default()
            });
        }
        Ok(HttpResponse {
            status: 200,
            url: request.url.to_owned(),
            headers: vec![("Content-Type".to_owned(), DIRECTORY_MEDIA_TYPE.to_owned())],
            body: DIRECTORY.as_bytes().to_vec(),
        })
    }
}

/// Sequence and the agent signature element, with process errors collected
/// rather than returned.
pub fn build_pipeline() -> Result<Arc<Pipeline>> {
    let element = AgentSignatureElement::builder()
        .set_http_client(Arc::new(KeyDirectories))
        .set_wait_budget(WAIT_BUDGET)
        .build()?;
    Ok(Pipeline::builder()
        .add_element(Arc::new(SequenceElement::new()))
        .add_element(Arc::new(element))
        .suppress_process_exceptions(true)
        .build()?)
}
