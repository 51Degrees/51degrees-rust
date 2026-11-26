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

//! A local HTTP server that stands in for the 51Degrees Distributor in the
//! data update tests, so that no test reaches the real service.
//!
//! It answers as the Distributor does for licence keys that hold several
//! products. A request with no `Product` gets a 300 (Multiple Choices) whose
//! JSON body lists one URI for each product, with a `Location` header naming
//! the first. A product it does not offer gets a 403 (Forbidden). An offered
//! product gets the gzipped data file with its lowercase hex MD5 in
//! `Content-MD5`, or a 304 (Not Modified) when the request's
//! `If-Modified-Since` is not earlier than the file's published date, or when
//! it serves no file. Answers queued with [`StandIn::then`] are sent in place
//! of these, one for each request, in order. Every request is recorded.
//!
//! Each test binary that includes this module uses all of it.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use chrono::{DateTime, Utc};

/// The body of a 403 to a product the licence keys do not hold.
const NOT_OFFERED: &str =
    "{\"Error\":\"The license key provided does not contain the requested product.\"}";

/// The `Error` of a 300, as the Distributor gives it.
const SEVERAL_PRODUCTS: &str = "There are multiple products associated with this license \
     key. Please use the URI returned in the Location header.";

/// The path the stand-in serves, as the Distributor's download endpoint.
const DOWNLOAD_PATH: &str = "/api/v2/download";

/// The longest request head read.
const MAX_HEAD_BYTES: usize = 64 * 1024;

/// One request the stand-in answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The path and query, as sent.
    pub target: String,
    /// The `Product` the request named, if it named one with a value.
    pub product: Option<String>,
    /// The `If-Modified-Since` header, if any.
    pub if_modified_since: Option<String>,
    /// The status of the answer.
    pub status: u16,
}

/// An answer sent in place of the one the stand-in would give.
#[derive(Debug, Clone)]
pub enum Answer {
    /// A response with this status, these headers and this body.
    Status(u16, Vec<(String, String)>, String),
    /// The data file with this `Content-MD5` in place of its own.
    FileWithMd5(String),
    /// The start of the data file, with a `Content-Length` of all of it, as
    /// a download cut short.
    TruncatedFile,
}

/// The data file the stand-in serves.
struct Served {
    compressed: Vec<u8>,
    md5: String,
    published: DateTime<Utc>,
}

struct Shared {
    products: Vec<String>,
    served: Option<Served>,
    requests: Mutex<Vec<Request>>,
    script: Mutex<VecDeque<Answer>>,
    stopping: AtomicBool,
}

/// The stand-in for the Distributor, serving until it is dropped.
pub struct StandIn {
    origin: String,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl StandIn {
    /// A stand-in for licence keys holding `products`, serving `data`, the
    /// uncompressed data file, as published at `published`, or answering 304
    /// to every offered product when `data` is `None`.
    pub fn new(products: &[&str], data: Option<(&[u8], DateTime<Utc>)>) -> StandIn {
        let served = data.map(|(data, published)| {
            let compressed = gzip(data);
            let md5 = md5_hex(&compressed);
            Served {
                compressed,
                md5,
                published,
            }
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let shared = Arc::new(Shared {
            products: products.iter().map(|p| (*p).to_owned()).collect(),
            served,
            requests: Mutex::new(Vec::new()),
            script: Mutex::new(VecDeque::new()),
            stopping: AtomicBool::new(false),
        });
        let serving = Arc::clone(&shared);
        let answer_origin = origin.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if serving.stopping.load(Ordering::SeqCst) {
                    return;
                }
                if let Ok(stream) = stream {
                    answer(&serving, &answer_origin, stream);
                }
            }
        });
        StandIn {
            origin,
            shared,
            thread: Some(thread),
        }
    }

    /// The download URL, as a data update URL is configured.
    pub fn url(&self) -> String {
        format!("{}{DOWNLOAD_PATH}", self.origin)
    }

    /// Queue `answer` for the next request not already given one.
    pub fn then(&self, answer: Answer) -> &StandIn {
        self.shared.script.lock().unwrap().push_back(answer);
        self
    }

    /// Every request answered so far, in order.
    pub fn requests(&self) -> Vec<Request> {
        self.shared.requests.lock().unwrap().clone()
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        // A connection wakes the accepting thread, which then stops.
        let _ = TcpStream::connect(self.origin.trim_start_matches("http://"));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Read one request from `stream`, answer it and record it.
fn answer(shared: &Shared, origin: &str, mut stream: TcpStream) {
    let Some(head) = read_head(&mut stream) else {
        return;
    };
    let mut lines = head.split("\r\n");
    let target = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .unwrap_or("/")
        .to_owned();
    let if_modified_since = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("If-Modified-Since"))
        .map(|(_, value)| value.trim().to_owned());
    let product = product_of(&target);
    let scripted = shared.script.lock().unwrap().pop_front();
    let (status, response) = match scripted {
        Some(answer) => scripted_response(shared, answer),
        None => natural_response(
            shared,
            origin,
            &target,
            product.as_deref(),
            &if_modified_since,
        ),
    };
    shared.requests.lock().unwrap().push(Request {
        target,
        product,
        if_modified_since,
        status,
    });
    let _ = stream.write_all(&response);
    let _ = stream.flush();
}

/// The answer the Distributor gives.
fn natural_response(
    shared: &Shared,
    origin: &str,
    target: &str,
    product: Option<&str>,
    since: &Option<String>,
) -> (u16, Vec<u8>) {
    let Some(product) = product else {
        let uris: Vec<String> = shared
            .products
            .iter()
            .map(|p| with_product(origin, target, p))
            .collect();
        let refs: Vec<&str> = uris.iter().map(String::as_str).collect();
        let mut headers = json_headers();
        if let Some(first) = uris.first() {
            headers.push(("Location".to_owned(), first.clone()));
        }
        let body = format!(
            "{{\"AvailableUris\":{},\"Error\":\"{SEVERAL_PRODUCTS}\"}}",
            json_strings(&refs)
        );
        return (300, response(300, &headers, body.as_bytes()));
    };
    if !shared
        .products
        .iter()
        .any(|offered| offered.eq_ignore_ascii_case(product))
    {
        return (403, response(403, &json_headers(), NOT_OFFERED.as_bytes()));
    }
    let Some(served) = &shared.served else {
        return (304, response(304, &[], b""));
    };
    let not_modified = since
        .as_deref()
        .and_then(|since| DateTime::parse_from_rfc2822(since).ok())
        .is_some_and(|since| since.with_timezone(&Utc) >= served.published);
    if not_modified {
        return (304, response(304, &[], b""));
    }
    (200, file_response(&served.compressed, &served.md5, None))
}

/// A queued answer.
fn scripted_response(shared: &Shared, answer: Answer) -> (u16, Vec<u8>) {
    match answer {
        Answer::Status(status, headers, body) => {
            (status, response(status, &headers, body.as_bytes()))
        }
        Answer::FileWithMd5(md5) => {
            let served = shared.served.as_ref().expect("a file to serve");
            (200, file_response(&served.compressed, &md5, None))
        }
        Answer::TruncatedFile => {
            let served = shared.served.as_ref().expect("a file to serve");
            let half = &served.compressed[..served.compressed.len() / 2];
            (
                200,
                file_response(half, &served.md5, Some(served.compressed.len())),
            )
        }
    }
}

/// A 200 with `body`, its `Content-MD5` and a `Content-Length` of `length`,
/// or of the body when `length` is `None`.
fn file_response(body: &[u8], md5: &str, length: Option<usize>) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
         Content-MD5: {md5}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        length.unwrap_or(body.len())
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

/// A response with `status`, `headers` and `body`.
fn response(status: u16, headers: &[(String, String)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} Status\r\n");
    for (name, value) in headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    ));
    let mut out = out.into_bytes();
    out.extend_from_slice(body);
    out
}

fn json_headers() -> Vec<(String, String)> {
    vec![("Content-Type".to_owned(), "application/json".to_owned())]
}

/// A JSON array of `values`, which hold no character JSON escapes.
fn json_strings(values: &[&str]) -> String {
    let quoted: Vec<String> = values.iter().map(|v| format!("\"{v}\"")).collect();
    format!("[{}]", quoted.join(","))
}

/// The head of a request, up to the blank line.
fn read_head(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && head.len() < MAX_HEAD_BYTES {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return None,
        }
    }
    String::from_utf8(head).ok()
}

/// The `Product` with a value in the query of `target`, matched ignoring
/// case.
fn product_of(target: &str) -> Option<String> {
    let query = target.split_once('?')?.1;
    query
        .split('&')
        .filter_map(|parameter| parameter.split_once('='))
        .find(|(name, value)| name.eq_ignore_ascii_case("Product") && !value.is_empty())
        .map(|(_, value)| value.to_owned())
}

/// The URI the stand-in lists for `product`, being `target` with every
/// `Product` parameter replaced by `product`.
fn with_product(origin: &str, target: &str, product: &str) -> String {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut parameters: Vec<&str> = query
        .split('&')
        .filter(|parameter| {
            !parameter.is_empty()
                && !parameter
                    .split('=')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case("Product"))
        })
        .collect();
    let named = format!("Product={product}");
    parameters.push(&named);
    format!("{origin}{path}?{}", parameters.join("&"))
}

/// `data` compressed as the Distributor serves it.
fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// The lowercase hex MD5 of `data`, as the Distributor sends it.
fn md5_hex(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    Md5::digest(data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
