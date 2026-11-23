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

//! @page human-confidence-web-example Human Confidence (Web)
//!
//! Human Confidence web demo. Run it with
//! `cargo run -p human-confidence-examples --bin human-confidence-web` and
//! open <http://localhost:5151/>.
//!
//! Environment variables:
//! - `_51DEGREES_RESOURCE_KEY`, the Resource Key, required.
//! - `_51DEGREES_LICENSE_KEY`, a License Key of the same account, needed to
//!   redeem.
//! - `_51DEGREES_CLOUD_ENDPOINT`, the API base including the `/api/v4/`
//!   segment, defaulting to `https://cloud.51degrees.com/api/v4/`.
//! - `_51DEGREES_EXAMPLE_HTTP_PORTS`, the port to listen on, defaulting to
//!   5151.
//!
//! See the crate README for the steps and what to expect.
//!
//! @snippet human-confidence-web.rs example

use std::net::SocketAddr;

use human_confidence_examples::{
    api_from, app, has_human_confidence, Settings, CONTACT_URL, DEFAULT_PORT, FIND_OUT_MORE,
    PROPERTY,
};

// [example]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Without a Resource Key there is nothing to show, so say how to get one
    // and end without starting the server.
    let Some(resource_key) = examples_shared::resource_key_from_env() else {
        eprintln!(
            "No Resource Key available. Set the {} environment variable to a \
             Resource Key that includes {PROPERTY} to run this demo. The \
             51Degrees cloud service is accessed using a Resource Key, which you \
             can create at https://51degrees.com/configure?utm_source=code&utm_medium=example&utm_campaign=51degrees-rust&utm_content=examples-human-confidence-examples-src-bin-human-confidence-web.rs&utm_term=resource-key-required.",
            examples_shared::RESOURCE_KEY_ENV_VAR
        );
        examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::CLOUD_REPOSITORIES);
        return Ok(());
    };
    let license_key = examples_shared::license_key_from_env();
    if license_key.is_none() {
        println!(
            "No {} set. The page still shows Passive and Active, but an \
             account that holds License Keys can't redeem without one.",
            examples_shared::LICENSE_KEY_ENV_VAR
        );
    }
    let api = api_from(examples_shared::cloud_endpoint_from_env().as_deref());
    let port = examples_shared::example_http_port(DEFAULT_PORT);

    let human_confidence = match has_human_confidence(&api, &resource_key).await {
        Ok(found) => found,
        Err(error) => {
            println!("Couldn't check the Resource Key's properties: {error}");
            true
        }
    };
    if !human_confidence {
        println!(
            "This Resource Key doesn't include the {PROPERTY} property. \
             Contact us to add it: {CONTACT_URL}"
        );
    }

    let router = app(Settings {
        resource_key,
        license_key,
        api: api.clone(),
        human_confidence,
    });
    // Every interface, not only loopback, so the page can be opened from a
    // second device on the same network.
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await?;
    println!("Human Confidence demo on http://localhost:{port}/, using {api}");
    examples_shared::print_find_out_more(FIND_OUT_MORE, examples_shared::CLOUD_REPOSITORIES);
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}
// [example]
