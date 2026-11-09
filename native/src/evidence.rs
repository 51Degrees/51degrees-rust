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

//! Marshalling pipeline evidence into the native evidence representation.
//!
//! The pipeline holds evidence as case-insensitive `prefix.field` string pairs
//! (see [`fiftyone_pipeline_core::Evidence`]). The native Device Detection
//! engine consumes a `fiftyoneDegreesEvidenceKeyValuePairArray` whose pairs are
//! tagged with a native `EvidencePrefix`. This module maps the pipeline keys
//! onto that native array.
//!
//! Two costs are avoided on the hot path. First, the native array itself is
//! pooled per thread and reused, since it is per-request scratch and the
//! pipeline processes one flow data per thread. Second, the key and value
//! strings the native array points at are not copied by the native side, so
//! they must outlive the array. Owned, null terminated copies are kept in a
//! per-thread pool of [`CString`] slots that is cleared and refilled on each
//! call, so the strings stay valid for the duration of one native processing
//! call without a fresh allocation per pair when the pool is already large
//! enough.
//!
//! The native evidence array here is Device Detection's. IP Intelligence
//! compiles its own copy of the common C library with wider file offsets and
//! must not use the `fiftyone-common-sys` evidence functions (doing so would
//! bind the two incompatible common C library builds together), so the IP
//! Intelligence wrapper makes its evidence array with its own copy's functions.
//! It reads each client IP value with [`parse_client_ip`].

#[cfg(feature = "dd")]
use std::cell::RefCell;
#[cfg(feature = "dd")]
use std::ffi::CString;
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
#[cfg(feature = "dd")]
use std::os::raw::c_char;

#[cfg(feature = "dd")]
use fiftyone_common_sys::{
    fiftyoneDegreesEvidenceAddString, fiftyoneDegreesEvidenceCreate, fiftyoneDegreesEvidenceFree,
    EvidenceKeyValuePairArray, EvidencePrefix as NativeEvidencePrefix,
};
use fiftyone_pipeline_core::constants;
use fiftyone_pipeline_core::Evidence;

/// Map a pipeline evidence key to the native evidence prefix the engine expects,
/// together with the field part of the key that the native side uses as its key.
///
/// The pipeline key is `prefix.field` with the prefix lowercased (see the
/// evidence specification). The native engine recognizes four categories that
/// matter to detection: HTTP header strings, query string values, cookies and
/// server values such as the client IP, which arrive as an IP address string.
/// Keys whose prefix the native engine does not understand return [`None`] and
/// are skipped.
///
/// Query values keep their own category, as the other 51Degrees SDKs do. The
/// engine reads a query value as a header when matching, in preference to the
/// real header, so a User-Agent supplied for off-line processing still works.
/// It also reads the values the client-side JavaScript sends back (the
/// `51D_` property overrides and profile ids) only from the query and cookie
/// categories, so labelling a query value as a header would hide them.
///
/// The native [`fiftyoneDegreesEvidenceAddString`] takes the prefix as the enum
/// argument and the bare field name as the key, NOT the full `prefix.field`
/// pipeline key (see the device detection examples in 51degrees-cxx, which add
/// evidence keyed on `"user-agent"`, `"sec-ch-ua-mobile"` and so on). The
/// native matcher then compares that key against the data set header names.
/// Passing the whole pipeline key, prefix included, makes every header
/// comparison miss, so the detection reads no values. The field part returned
/// here is therefore the substring after the first separator, which is what the
/// native key must be.
///
/// Only Device Detection builds a native evidence array, so this is gated behind
/// the `dd` feature alongside the `fiftyone-common-sys` prefix type it returns.
#[cfg(feature = "dd")]
fn native_prefix_for(key: &str) -> Option<(NativeEvidencePrefix, &str)> {
    let (prefix, field) = key.split_once(constants::EVIDENCE_SEPARATOR)?;
    let native_prefix = match prefix {
        // Headers are parsed by the engine as header strings.
        constants::EVIDENCE_HTTP_HEADER_PREFIX => NativeEvidencePrefix::HttpHeaderString,
        // Query values are matched like headers, taking precedence over them,
        // and are also where the engine looks for client-side overrides.
        constants::EVIDENCE_QUERY_PREFIX => NativeEvidencePrefix::Query,
        // Server values carry the client IP address list to be parsed.
        constants::EVIDENCE_SERVER_PREFIX => NativeEvidencePrefix::HttpHeaderIpAddresses,
        // Cookies can carry client-side collected evidence the engine reads.
        constants::EVIDENCE_COOKIE_PREFIX => NativeEvidencePrefix::Cookie,
        _ => return None,
    };
    Some((native_prefix, field))
}

#[cfg(feature = "dd")]
thread_local! {
    /// The per-thread pooled native evidence array and the owned string slots
    /// that back its keys and values for the duration of one processing call.
    static EVIDENCE_POOL: RefCell<EvidencePool> = RefCell::new(EvidencePool::new());
}

/// A per-thread pool holding one native evidence array and the owned key and
/// value strings the array points at.
#[cfg(feature = "dd")]
struct EvidencePool {
    /// The native array, lazily created and reused. Owned by this pool and freed
    /// when the pool is dropped at thread exit.
    array: *mut EvidenceKeyValuePairArray,
    /// The capacity the native array was created with.
    capacity: u32,
    /// Owned, null terminated copies of every key the array currently points at.
    /// Held so the native pointers stay valid for the processing call.
    keys: Vec<CString>,
    /// Owned, null terminated copies of every value the array points at.
    values: Vec<CString>,
}

#[cfg(feature = "dd")]
impl EvidencePool {
    fn new() -> Self {
        EvidencePool {
            array: std::ptr::null_mut(),
            capacity: 0,
            keys: Vec::new(),
            values: Vec::new(),
        }
    }

    /// Ensure the native array exists and has at least `needed` capacity,
    /// recreating it larger when it does not. A freshly created array starts
    /// empty, so callers must repopulate it.
    ///
    /// The capacity carries headroom beyond `needed` because the engine adds
    /// pairs of its own, the headers it derives from a high-entropy values
    /// blob, during processing. When those do not fit, the engine chains a
    /// further array on through `next`. [`EvidencePool::release_chain`] frees
    /// any such chain after each call, so a reused array never carries pairs
    /// from an earlier request.
    ///
    /// # Safety
    /// Calls into the native evidence allocator. Safe to call repeatedly.
    unsafe fn ensure_capacity(&mut self, needed: u32) {
        if self.array.is_null() || self.capacity < needed {
            if !self.array.is_null() {
                fiftyoneDegreesEvidenceFree(self.array);
                self.array = std::ptr::null_mut();
            }
            // Grow with a little headroom so a request a few pairs larger does
            // not force an immediate reallocation next time.
            let capacity = needed.saturating_add(ENGINE_ADDED_PAIRS).max(8);
            self.array = fiftyoneDegreesEvidenceCreate(capacity);
            self.capacity = if self.array.is_null() { 0 } else { capacity };
        } else {
            // Reset the used count so the reused array starts empty. The array
            // layout begins with the `count` field.
            (*self.array).count = 0;
        }
    }
}

#[cfg(feature = "dd")]
impl EvidencePool {
    /// Free the array if the engine chained another array on to it during the
    /// last call, so the next call starts from a fresh, unchained array.
    ///
    /// Resetting `count` alone would leave the chained arrays in place, and
    /// their pairs point at key and value strings that belong to the earlier
    /// request, which the pool has since dropped.
    ///
    /// # Safety
    /// Calls into the native evidence allocator. `self.array` must be null or
    /// an array from `fiftyoneDegreesEvidenceCreate`.
    unsafe fn release_chain(&mut self) {
        if !self.array.is_null() && !(*self.array).next.is_null() {
            // fiftyoneDegreesEvidenceFree frees every array in the chain.
            fiftyoneDegreesEvidenceFree(self.array);
            self.array = std::ptr::null_mut();
            self.capacity = 0;
        }
    }
}

/// Room left in the native evidence array for the pairs the engine adds while
/// processing, being the User-Agent Client Hints headers it derives from a
/// high-entropy values blob.
#[cfg(feature = "dd")]
const ENGINE_ADDED_PAIRS: u32 = 16;

#[cfg(feature = "dd")]
impl Drop for EvidencePool {
    fn drop(&mut self) {
        if !self.array.is_null() {
            // Safety: the array came from `fiftyoneDegreesEvidenceCreate`.
            unsafe { fiftyoneDegreesEvidenceFree(self.array) };
            self.array = std::ptr::null_mut();
        }
    }
}

/// Build the native evidence array for the engine from the pipeline evidence,
/// then run `process` with a pointer to the populated array.
///
/// The array and its backing strings live in the thread-local pool and remain
/// valid for the duration of the `process` call, which is the only window in
/// which the native side reads them. The pool is reused on the next call, so no
/// native allocation happens once it has grown large enough.
///
/// Keys whose prefix the native engine does not recognize are skipped. When no
/// usable evidence remains the array is still passed (empty), which the engine
/// handles by producing a result with no matched values.
///
/// # Safety
/// `process` receives a pointer to a populated native evidence array that is
/// valid only for the duration of the call. It must not retain the pointer or
/// the pairs beyond the call.
#[cfg(feature = "dd")]
pub(crate) unsafe fn with_native_evidence<R>(
    evidence: &Evidence,
    process: impl FnOnce(*mut EvidenceKeyValuePairArray) -> R,
) -> R {
    EVIDENCE_POOL.with(|cell| {
        let mut pool = cell.borrow_mut();

        // Collect the evidence the engine understands as (prefix, field, value)
        // triples. The native key is the bare field name (the part after the
        // `prefix.` separator), because the prefix travels separately as the enum
        // argument and the native matcher compares the key against header names.
        // Done first so the array can be sized exactly to the usable subset.
        let usable: Vec<(NativeEvidencePrefix, &str, &str)> = evidence
            .iter()
            .filter_map(|(key, value)| {
                native_prefix_for(key).map(|(prefix, field)| (prefix, field, value))
            })
            .collect();

        pool.ensure_capacity(usable.len() as u32);
        if pool.array.is_null() {
            // Allocation failed. Run with a null array so the caller still gets
            // a well defined (empty) outcome rather than a panic.
            return process(std::ptr::null_mut());
        }

        // Refill the owned string slots. Clearing keeps the allocated capacity,
        // so steady-state traffic reuses the same backing buffers.
        pool.keys.clear();
        pool.values.clear();
        for (_, field, value) in &usable {
            // Interior nul bytes cannot occur in lowercased evidence fields, and
            // a value containing one is truncated at the nul rather than rejected,
            // matching how a C string would be read.
            let key_c = CString::new(*field)
                .unwrap_or_else(|e| CString::new(&field[..e.nul_position()]).unwrap_or_default());
            let value_c = CString::new(*value)
                .unwrap_or_else(|e| CString::new(&value[..e.nul_position()]).unwrap_or_default());
            pool.keys.push(key_c);
            pool.values.push(value_c);
        }

        // Add every pair to the native array. The pointers reference the owned
        // CStrings in the pool, which outlive the `process` call below.
        let array = pool.array;
        for (i, (prefix, _, _)) in usable.iter().enumerate() {
            let key_ptr = pool.keys[i].as_ptr() as *const c_char;
            let value_ptr = pool.values[i].as_ptr() as *const c_char;
            fiftyoneDegreesEvidenceAddString(array, *prefix, key_ptr, value_ptr);
        }

        let result = process(array);
        pool.release_chain();
        result
    })
}

/// Pick a client IP address string from pipeline evidence, trying
/// [`constants::EVIDENCE_CLIENT_IP_KEY`] (`server.client-ip`), then
/// `query.client-ip`, then the first value under any `server.` prefix. Returns
/// [`None`] when none is present.
///
/// IP Intelligence does not use this, because its data file defines the keys
/// and the order to try them in. The IP Intelligence `Results::process_evidence`
/// reads those keys, and [`parse_client_ip`] reads one value.
#[deprecated(note = "IP Intelligence reads the keys its data file defines. Use \
            `ipi::Results::process_evidence`, or `parse_client_ip` for one value.")]
pub fn client_ip_from_evidence(evidence: &Evidence) -> Option<String> {
    if let Some(ip) = evidence.get(constants::EVIDENCE_CLIENT_IP_KEY) {
        return Some(ip.to_owned());
    }
    // A query-supplied client IP is the off-line processing fallback.
    if let Some(ip) = evidence.get("query.client-ip") {
        return Some(ip.to_owned());
    }
    // Finally accept any other server value, which by convention is the IP.
    evidence
        .iter()
        .find(|(k, _)| {
            k.split_once(constants::EVIDENCE_SEPARATOR)
                .map(|(p, _)| p == constants::EVIDENCE_SERVER_PREFIX)
                .unwrap_or(false)
        })
        .map(|(_, v)| v.to_owned())
}

/// Read a client IP address from one evidence value, only when the whole
/// value is an address.
///
/// The value may be an IPv4 address in dotted-decimal form, an IPv6 address in
/// any RFC 4291 text form, an IPv4 address with a port, or a bracketed IPv6
/// address with or without a port. Anything else gives [`None`] and no part of
/// it is read, so a comma separated list, white space around the address, an
/// IPv6 zone index (`fe80::1%eth0`), a prefix length or trailing text is not
/// read, nor are the shortened, octal and bare number IPv4 forms some parsers
/// accept. Reading part of a value could give a different address from the
/// one the sender meant.
pub fn parse_client_ip(value: &str) -> Option<IpAddr> {
    if let Some(bracketed) = value.strip_prefix('[') {
        // "[2001:db8::1]" or "[2001:db8::1]:443".
        let (address, rest) = bracketed.split_once(']')?;
        if !rest.is_empty() && !rest.strip_prefix(':').is_some_and(is_port) {
            return None;
        }
        return address.parse::<Ipv6Addr>().ok().map(IpAddr::V6);
    }
    if let Ok(address) = value.parse::<IpAddr>() {
        return Some(address);
    }
    // "192.0.2.1:443". Without brackets an IPv6 address keeps all its colons,
    // because a port cannot be told apart from its last group, and a
    // `SocketAddrV6` would take a zone index.
    match value.parse::<SocketAddr>() {
        Ok(SocketAddr::V4(socket)) => Some(IpAddr::V4(*socket.ip())),
        _ => None,
    }
}

/// True when `text` is a port number, being digits only and at most 65535.
fn is_port(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) && text.parse::<u16>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "dd")]
    #[test]
    fn header_maps_to_header_string() {
        // The native prefix is the header-string category and the native key is
        // the bare field name, with the pipeline prefix stripped.
        assert_eq!(
            native_prefix_for("header.user-agent"),
            Some((NativeEvidencePrefix::HttpHeaderString, "user-agent"))
        );
    }

    #[cfg(feature = "dd")]
    #[test]
    fn query_and_cookie_keep_their_own_category() {
        // The engine looks for client-side overrides only under these two
        // categories, and still matches a query User-Agent like a header.
        assert_eq!(
            native_prefix_for("query.user-agent"),
            Some((NativeEvidencePrefix::Query, "user-agent"))
        );
        assert_eq!(
            native_prefix_for("query.51d_screenpixelswidth"),
            Some((NativeEvidencePrefix::Query, "51d_screenpixelswidth"))
        );
        assert_eq!(
            native_prefix_for("cookie.51d_screenpixelswidth"),
            Some((NativeEvidencePrefix::Cookie, "51d_screenpixelswidth"))
        );
    }

    #[cfg(feature = "dd")]
    #[test]
    fn server_maps_to_ip_addresses() {
        assert_eq!(
            native_prefix_for("server.client-ip"),
            Some((NativeEvidencePrefix::HttpHeaderIpAddresses, "client-ip"))
        );
    }

    #[cfg(feature = "dd")]
    #[test]
    fn field_keeps_any_further_separators() {
        // Only the first separator splits the prefix from the field. A field that
        // itself contains a dot is preserved whole.
        assert_eq!(
            native_prefix_for("header.x.custom"),
            Some((NativeEvidencePrefix::HttpHeaderString, "x.custom"))
        );
    }

    #[cfg(feature = "dd")]
    #[test]
    fn unknown_prefix_is_skipped() {
        assert_eq!(native_prefix_for("location.latitude"), None);
        assert_eq!(native_prefix_for("no-separator"), None);
    }

    /// The web request a web integration gives with the evidence, under
    /// `server.http-request`, is not one of its text values, so it never
    /// reaches the C library, which reads a `server.` value as client IP
    /// addresses. Nor does a value only the request holds.
    #[cfg(feature = "dd")]
    #[test]
    fn the_web_request_is_not_handed_to_the_c_library() {
        use std::ffi::CStr;

        let request = Evidence::builder()
            .add("header.user-agent", "UA")
            .add("header.x-test", "only in the request")
            .add("server.client-ip", "192.0.2.1")
            .build();
        let evidence = Evidence::builder()
            .add("header.user-agent", "UA")
            .http_request(request)
            .build();
        // Safety: the array and its strings are read only inside the call.
        let handed = unsafe {
            with_native_evidence(&evidence, |array| {
                assert!(!array.is_null(), "the native array is allocated");
                let array = &*array;
                (0..array.count as usize)
                    .map(|index| {
                        let pair = &*array.items.add(index);
                        let text =
                            |pointer| CStr::from_ptr(pointer).to_str().expect("UTF-8").to_owned();
                        (pair.prefix, text(pair.item.key), text(pair.item.value))
                    })
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(
            handed,
            [(
                NativeEvidencePrefix::HttpHeaderString,
                "user-agent".to_owned(),
                "UA".to_owned()
            )]
        );
    }

    #[test]
    #[allow(deprecated)]
    fn client_ip_prefers_canonical_key() {
        let evidence = Evidence::builder()
            .add("server.client-ip", "1.2.3.4")
            .add("query.client-ip", "5.6.7.8")
            .build();
        assert_eq!(
            client_ip_from_evidence(&evidence).as_deref(),
            Some("1.2.3.4")
        );
    }

    #[test]
    #[allow(deprecated)]
    fn client_ip_falls_back_to_query() {
        let evidence = Evidence::builder()
            .add("query.client-ip", "5.6.7.8")
            .build();
        assert_eq!(
            client_ip_from_evidence(&evidence).as_deref(),
            Some("5.6.7.8")
        );
    }

    #[test]
    #[allow(deprecated)]
    fn client_ip_absent_is_none() {
        let evidence = Evidence::builder().add("header.user-agent", "x").build();
        assert_eq!(client_ip_from_evidence(&evidence), None);
    }

    fn parsed(value: &str) -> Option<String> {
        parse_client_ip(value).map(|address| address.to_string())
    }

    // The forms the client IP rule of the IP Intelligence specification
    // reads, each with the address read from it.
    #[test]
    fn a_client_ip_is_read_in_the_forms_proxies_send() {
        for (value, address) in [
            ("192.0.2.1", "192.0.2.1"),
            ("203.0.113.1", "203.0.113.1"),
            ("0.0.0.0", "0.0.0.0"),
            ("192.0.2.1:443", "192.0.2.1"),
            ("203.0.113.1:8080", "203.0.113.1"),
            ("203.0.113.1:0443", "203.0.113.1"),
            ("203.0.113.1:0", "203.0.113.1"),
            ("203.0.113.1:65535", "203.0.113.1"),
            ("2001:DB8:0::1", "2001:db8::1"),
            ("2001:0DB8:0:0:0:0:0:1", "2001:db8::1"),
            ("::ffff:203.0.113.1", "::ffff:203.0.113.1"),
            ("2001:db8::1:80", "2001:db8::1:80"),
            ("1:2:3:4:5:6:7::", "1:2:3:4:5:6:7:0"),
            ("::", "::"),
            ("[2001:db8::1]", "2001:db8::1"),
            ("[2001:db8::1]:443", "2001:db8::1"),
            ("[2001:db8::1]:8080", "2001:db8::1"),
            ("[2001:db8::1]:0443", "2001:db8::1"),
        ] {
            assert_eq!(parsed(value).as_deref(), Some(address), "{value:?}");
        }
    }

    // A value that is not wholly one of those forms is not read, and no part
    // of it is used. These are the values the specification's table lists,
    // and the zone index and leading zero of its other examples.
    #[test]
    fn a_value_that_is_not_a_whole_address_is_not_read() {
        for value in [
            "",
            "  ",
            "unknown",
            ", 192.0.2.1",
            "192.0.2",
            "203.0.113",
            "3221225985",
            "012.0.2.1",
            "01.2.3.4",
            "1.2.3.04",
            "256.0.113.1",
            "192.0.2.1/24",
            "203.0.113.0/24",
            "2001:db8::/32",
            "192.0.2.1 x",
            "203.0.113.1 x",
            "192.0.2.1, 198.51.100.7",
            "203.0.113.1, 198.51.100.7",
            "203.0.113.1,198.51.100.7",
            " 192.0.2.1 ",
            "203.0.113.1 ",
            " 203.0.113.1",
            "203.0.113.1\t",
            "\t203.0.113.1",
            "203.0.113.1\r\n",
            "203.0.113.1\n",
            "fe80::1%1",
            "fe80::1%eth0",
            "[fe80::1%eth0]",
            "[fe80::1%eth0]:443",
            "[fe80::1%25eth0]",
            "192.0.2.1:65536",
            "203.0.113.1:65536",
            "192.0.2.1:+443",
            "203.0.113.1:",
            "[192.0.2.1]",
            "[203.0.113.1]",
            "[2001:db8::1",
            "2001:db8::1]:80",
            "[2001:db8::1]443",
            "[2001:db8::1]:",
            "[2001:db8::1]:65536",
            "2001:db8::1::2",
            "2001:db8:1:2:3:4:5:6:7",
            "2001:db8::12345",
        ] {
            assert_eq!(parsed(value), None, "{value:?} should not be read");
        }
    }
}
