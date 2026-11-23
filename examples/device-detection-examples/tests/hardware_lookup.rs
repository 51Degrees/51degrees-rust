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

//! Runs the cloud TAC and native model lookup examples as programs and
//! checks what they print.
//!
//! The tests against the cloud are ignored, run with `cargo test -p
//! device-detection-examples -- --ignored`, and check nothing when
//! `_51DEGREES_RESOURCE_KEY_BESPOKE` is not set. They give the example that
//! key as its resource key, and the example asks the cloud
//! `_51DEGREES_CLOUD_ENDPOINT` names, or the public one when it is unset.
//! Each fails when the key does not include HardwarePlatform.Profiles, or
//! when a looked up value lists no device with a vendor and a model.
//!
//! The other tests run each example against a local stand-in for the cloud,
//! which lists no hardware properties for the key, and check the example
//! says what the key lacks and makes no lookup.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, ExitStatus};
use std::sync::{Arc, Mutex};

use device_detection_examples::NO_VALUE;
use examples_shared::{
    CLOUD_ENDPOINT_ENV_VAR, FIND_OUT_MORE_HEADING, LIVE_TEST_RESOURCE_KEY_ENV_VAR,
    RESOURCE_KEY_ENV_VAR,
};

/// The text that shows the resource key does not include the lookup.
const NOT_INCLUDED: &str = "does not include HardwarePlatform.Profiles";

/// The start of every lookup's heading.
const HEADING_START: &str = "Which devices are associated";

/// One of the two examples, with the values it looks up.
struct Example {
    /// The example's program.
    program: &'static str,
    /// The file the example's links are tagged with.
    content: &'static str,
    /// The values the example looks up.
    values: [&'static str; 2],
    /// What the example's headings call a value.
    value_name: &'static str,
    /// The lookup the example makes.
    lookup: &'static str,
}

const TAC: Example = Example {
    program: env!("CARGO_BIN_EXE_dd-cloud-tac"),
    content: "examples-device-detection-examples-src-bin-dd-cloud-tac.rs",
    values: ["35925406", "86386802"],
    value_name: "TAC",
    lookup: "TAC lookup",
};

const NATIVE_MODEL: Example = Example {
    program: env!("CARGO_BIN_EXE_dd-cloud-native-model"),
    content: "examples-device-detection-examples-src-bin-dd-cloud-native-model.rs",
    values: ["SC-03L", "iPhone11,8"],
    value_name: "native model name",
    lookup: "native model lookup",
};

/// The cloud's list of the properties a key without the hardware lookup
/// includes, cut down to one device property.
const NO_HARDWARE_PROPERTIES: &str = r#"{"Products":{"device":{
    "DataTier":"CloudV5Basic","Properties":[{"Name":"IsMobile",
    "Type":"Boolean","Category":"Device","DelayExecution":false}]}}}"#;

/// The evidence keys the stand-in accepts, a part of the cloud's list that
/// holds the two the examples send.
const EVIDENCE_KEYS: &str = r#"["header.User-Agent","query.User-Agent",
    "query.tac","query.nativemodel"]"#;

/// Run `command` and return how it ended and everything it printed, standard
/// output then standard error.
fn run(command: &mut Command) -> (ExitStatus, String) {
    let output = command.output().expect("the example runs");
    let printed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status, printed)
}

/// The lines under `heading` in `printed`, being the tab indented lines that
/// follow it. Empty when the heading is not there.
fn lines_under<'a>(printed: &'a str, heading: &str) -> Vec<&'a str> {
    let mut lines = printed.lines().skip_while(|line| *line != heading);
    if lines.next().is_none() {
        return Vec::new();
    }
    lines.take_while(|line| line.starts_with('\t')).collect()
}

/// Whether `line` lists a device as `<vendor> <name> (<model>)`, with a
/// vendor and a model. The vendor is the text before the first space and the
/// model the text inside the final parentheses, and neither may be empty or
/// [`NO_VALUE`].
fn has_vendor_and_model(line: &str) -> bool {
    let line = line.trim();
    let Some((vendor, _)) = line.split_once(' ') else {
        return false;
    };
    let Some(inner) = line.strip_suffix(')') else {
        return false;
    };
    let Some(open) = inner.rfind('(') else {
        return false;
    };
    let model = inner[open + 1..].trim();
    [vendor, model]
        .iter()
        .all(|part| !part.is_empty() && *part != NO_VALUE)
}

/// Run `example` against the cloud with the live test key, and require a
/// device with a vendor and a model for each value it looks up.
fn check_against_the_cloud(example: &Example) {
    let Some(resource_key) = examples_shared::live_test_resource_key() else {
        eprintln!("skipping: no resource key set");
        return;
    };
    let mut command = Command::new(example.program);
    command.env(RESOURCE_KEY_ENV_VAR, resource_key);
    let (status, printed) = run(&mut command);

    if printed.contains(NOT_INCLUDED) {
        panic!(
            "The resource key in {LIVE_TEST_RESOURCE_KEY_ENV_VAR} does not include \
             HardwarePlatform.Profiles, which the {} needs.\n{printed}",
            example.lookup
        );
    }
    assert!(
        status.success(),
        "The example did not end normally, exit {:?}.\n{printed}",
        status.code()
    );
    for value in example.values {
        let heading = format!("{HEADING_START} with the {} '{value}'?", example.value_name);
        let listed = lines_under(&printed, &heading)
            .iter()
            .any(|line| has_vendor_and_model(line));
        assert!(
            listed,
            "No device with a vendor and a model was listed for the {} '{value}'.\n{printed}",
            example.value_name
        );
    }
}

/// A local stand-in for the cloud service, and the path of each request it
/// was sent.
struct StandIn {
    endpoint: String,
    requests: Arc<Mutex<Vec<String>>>,
}

/// Start a stand-in on a free local port. It answers the evidence keys and
/// a list of properties with no hardware element, and anything else with
/// 404.
fn start_stand_in() -> StandIn {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local port binds");
    let endpoint = format!(
        "http://{}/api/v4/",
        listener.local_addr().expect("the port is known")
    );
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&requests);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let recorded = Arc::clone(&recorded);
            std::thread::spawn(move || answer(stream, &recorded));
        }
    });
    StandIn { endpoint, requests }
}

/// Read one request from `stream`, record its path without the query, which
/// holds the key, and answer it.
fn answer(mut stream: TcpStream, requests: &Mutex<Vec<String>>) {
    let mut reader = BufReader::new(stream.try_clone().expect("the stream clones"));
    let mut request_line = String::new();
    let _ = reader.read_line(&mut request_line);
    let mut length = 0;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
            break;
        }
        if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; length];
    let _ = reader.read_exact(&mut body);

    let target = request_line.split_whitespace().nth(1).unwrap_or_default();
    let path = target.split('?').next().unwrap_or_default().to_owned();
    let (status, body) = if path.ends_with("/accessibleproperties") {
        ("200 OK", NO_HARDWARE_PROPERTIES)
    } else if path.ends_with("/evidencekeys") {
        ("200 OK", EVIDENCE_KEYS)
    } else {
        ("404 Not Found", "")
    };
    requests.lock().unwrap().push(path);
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// Run `example` against a stand-in that lists no hardware properties for
/// the key, and require the line that says what the key lacks, no lookup
/// heading and no lookup request.
fn check_the_message(example: &Example) {
    let stand_in = start_stand_in();
    let mut command = Command::new(example.program);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("_51DEGREES_") {
            command.env_remove(name);
        }
    }
    command
        .env(RESOURCE_KEY_ENV_VAR, "stand-in-key")
        .env(CLOUD_ENDPOINT_ENV_VAR, &stand_in.endpoint);
    let (status, printed) = run(&mut command);

    assert!(
        status.success(),
        "The example did not end normally, exit {:?}.\n{printed}",
        status.code()
    );
    let tags = format!(
        "utm_source=code&utm_medium=example&utm_campaign=51degrees-rust\
         &utm_content={}&utm_term=no-hardware-lookup",
        example.content
    );
    let message = format!(
        "The resource key does not include HardwarePlatform.Profiles, the \
         hardware lookup this example needs, so no devices are listed. Create a \
         resource key that includes it at \
         https://51degrees.com/configure/QKyYH5XT?{tags}, or contact us about it \
         at https://51degrees.com/contact-us?{tags}."
    );
    let Some(at) = printed.lines().position(|line| line == message) else {
        panic!("The line saying what the key lacks was not printed.\n{printed}");
    };
    assert!(
        printed
            .lines()
            .skip(at)
            .any(|line| line == FIND_OUT_MORE_HEADING),
        "No Find out more section followed the line.\n{printed}"
    );
    assert!(
        !printed.lines().any(|line| line.starts_with(HEADING_START)),
        "A lookup heading was printed.\n{printed}"
    );
    let requests = stand_in.requests.lock().unwrap();
    assert!(
        requests
            .iter()
            .any(|path| path.ends_with("/accessibleproperties")),
        "The stand-in was not asked for the properties, only {requests:?}"
    );
    assert!(
        !requests.iter().any(|path| path.ends_with("/json")),
        "A lookup was sent to the stand-in, {requests:?}"
    );
}

#[test]
#[ignore = "requires a live cloud resource key (set _51DEGREES_RESOURCE_KEY_BESPOKE)"]
fn the_tac_example_lists_devices_from_the_cloud() {
    check_against_the_cloud(&TAC);
}

#[test]
#[ignore = "requires a live cloud resource key (set _51DEGREES_RESOURCE_KEY_BESPOKE)"]
fn the_native_model_example_lists_devices_from_the_cloud() {
    check_against_the_cloud(&NATIVE_MODEL);
}

#[test]
fn the_tac_example_says_when_the_key_lacks_the_lookup() {
    check_the_message(&TAC);
}

#[test]
fn the_native_model_example_says_when_the_key_lacks_the_lookup() {
    check_the_message(&NATIVE_MODEL);
}

#[test]
fn a_device_line_needs_a_vendor_and_a_model() {
    assert!(has_vendor_and_model("\tApple iPhone 6 (A1586)"));
    assert!(has_vendor_and_model("\tSamsung Galaxy S10 (SC-03L)"));
    assert!(!has_vendor_and_model("\tUnknown iPhone 6 (A1586)"));
    assert!(!has_vendor_and_model("\tApple iPhone 6 (Unknown)"));
    assert!(!has_vendor_and_model("\tApple iPhone 6 ()"));
    assert!(!has_vendor_and_model(
        "\tNo devices are associated with this TAC."
    ));
}
