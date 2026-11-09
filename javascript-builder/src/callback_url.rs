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

//! The callback URL the script posts to for updated JSON.
//!
//! The protocol and host usually come from the request, so the URL is checked
//! before it is written into the script, where it sits inside a single-quoted
//! JavaScript string. A URL that is not valid is refused, and the characters
//! after the host and in the endpoint that could end the string or the
//! script are percent-encoded.

use std::net::Ipv6Addr;

/// The callback URL for one request.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CallbackUrl {
    /// The protocol, host or endpoint is empty, so the script has no URL.
    Missing,
    /// The URL is not valid, so the script has no URL. Carries the URL as it
    /// was built, with anything in the host value before its last `@`
    /// replaced by `***`, for the log.
    Invalid(String),
    /// The URL to write into the script.
    Valid(String),
}

/// The largest port number.
const MAX_PORT: u32 = 65_535;

/// What the log shows in place of anything in a refused host value before
/// its last `@`.
const USER_INFO_MARKER: &str = "***";

/// Build the callback URL from its parts and check it.
///
/// One slash separates host and endpoint, being added when neither has one
/// and removed when both do. The scheme and the host are written in lower case
/// and the rest as given, apart from the percent-encoding.
pub(crate) fn callback_url(protocol: &str, host: &str, endpoint: &str) -> CallbackUrl {
    if protocol.trim().is_empty() || host.trim().is_empty() || endpoint.trim().is_empty() {
        return CallbackUrl::Missing;
    }
    let endpoint = match (host.ends_with('/'), endpoint.starts_with('/')) {
        (false, false) => format!("/{endpoint}"),
        (true, true) => endpoint[1..].to_owned(),
        _ => endpoint.to_owned(),
    };
    let end = host.find('/').unwrap_or(host.len());
    let (authority, rest) = host.split_at(end);
    let invalid = || {
        let shown = logged_host(host);
        CallbackUrl::Invalid(format!("{protocol}://{shown}{endpoint}"))
    };
    // A `?` or `#` in the host would start the query or the fragment before
    // the endpoint, and `fetch` refuses a URL with user information.
    if !is_scheme(protocol) || host.contains(['?', '#']) || authority.contains('@') {
        return invalid();
    }
    match checked_host_port(authority) {
        Some(authority) => CallbackUrl::Valid(format!(
            "{}://{authority}{}{}",
            protocol.to_ascii_lowercase(),
            encoded(rest),
            encoded(&endpoint)
        )),
        None => invalid(),
    }
}

/// The host value as the log shows it, with anything before its last `@`
/// replaced by `***`, so a user name or password is never logged, even one
/// holding `/`, `?` or `#`, as
/// [rule 6 of the callback URL](https://github.com/51Degrees/specifications/blob/99189c7aa040eaa7755e395e2c9f7110bc969ab0/pipeline-specification/pipeline-elements/javascript-builder.md#L243-L258)
/// asks.
fn logged_host(host: &str) -> String {
    match host.rfind('@') {
        Some(at) => format!("{USER_INFO_MARKER}{}", &host[at..]),
        None => host.to_owned(),
    }
}

/// `http` or `https`, ignoring case. The script uses the URL only with
/// `fetch` and `XMLHttpRequest`, which reach a server with no other scheme.
fn is_scheme(protocol: &str) -> bool {
    protocol.eq_ignore_ascii_case("http") || protocol.eq_ignore_ascii_case("https")
}

/// A host and an optional port. The host is a bracketed IPv6 address, or a
/// name of non-empty labels separated by dots, with a dot allowed at the end,
/// and the port is up to 65535 or empty. `Ipv6Addr` takes no zone
/// identifier, so a `%` inside the brackets is refused, as a browser's URL
/// parser refuses it. A name that ends in a number, ignoring one `.` at its
/// end, must be an IPv4 address with no `.` at its end, because a browser's
/// URL parser reads it as one, and so that it has one written form.
fn checked_host_port(host_port: &str) -> Option<String> {
    let (host, port) = if let Some(inner) = host_port.strip_prefix('[') {
        let (address, after) = inner.split_once(']')?;
        address.parse::<Ipv6Addr>().ok()?;
        (&host_port[..host_port.len() - after.len()], after)
    } else {
        let end = host_port.find(':').unwrap_or(host_port.len());
        let (host, port) = host_port.split_at(end);
        let name = host.strip_suffix('.').unwrap_or(host);
        if !name.split('.').all(is_label) || (ends_in_a_number(name) && !is_ipv4(host)) {
            return None;
        }
        (host, port)
    };
    if !port.is_empty() {
        let digits = port.strip_prefix(':')?;
        if !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let significant = digits.trim_start_matches('0');
        if significant.len() > 5 || significant.parse::<u32>().is_ok_and(|n| n > MAX_PORT) {
            return None;
        }
    }
    Some(format!("{}{port}", host.to_ascii_lowercase()))
}

/// A label of a host name, being ASCII letters, digits, `-` and `_`. A host is
/// ASCII only, so an internationalized name is given as A-labels.
fn is_label(label: &str) -> bool {
    !label.is_empty()
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Whether the last label of a name is a number as the URL Standard's ends in
/// a number checker reads one, being all ASCII digits, or `0x` or `0X`
/// followed by zero or more hex digits. The name has had one `.` at its end
/// removed. Such a name must be an IPv4 address, as
/// [rule 4 of the callback URL](https://github.com/51Degrees/specifications/blob/99189c7aa040eaa7755e395e2c9f7110bc969ab0/pipeline-specification/pipeline-elements/javascript-builder.md#L223-L240)
/// asks.
fn ends_in_a_number(name: &str) -> bool {
    let last = name.rsplit_once('.').map_or(name, |(_, last)| last);
    match last.strip_prefix("0x").or_else(|| last.strip_prefix("0X")) {
        Some(hex) => hex.bytes().all(|b| b.is_ascii_hexdigit()),
        None => !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit()),
    }
}

/// Four decimal parts of 0 to 255 separated by dots, each with no zero before
/// another digit, so `0` and `101` pass and `00` and `01` fail.
fn is_ipv4(name: &str) -> bool {
    let is_part = |part: &str| {
        part.bytes().all(|b| b.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
            && part.parse::<u8>().is_ok()
    };
    name.split('.').count() == 4 && name.split('.').all(is_part)
}

/// Percent-encode the characters a URL cannot hold as they are and those that
/// would end the string or the script the URL is written into. A `%` that does
/// not start an escape is encoded too.
fn encoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    for (i, &b) in bytes.iter().enumerate() {
        let keep = match b {
            b'%' => {
                bytes.len() > i + 2
                    && bytes[i + 1].is_ascii_hexdigit()
                    && bytes[i + 2].is_ascii_hexdigit()
            }
            b' ' | b'"' | b'\'' | b'<' | b'>' | b'\\' | b'^' | b'`' | b'{' | b'|' | b'}' => false,
            _ => b.is_ascii() && !b.is_ascii_control(),
        };
        if keep {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(protocol: &str, host: &str) -> CallbackUrl {
        callback_url(protocol, host, "/51dpipeline/json")
    }

    fn valid(text: &str) -> CallbackUrl {
        CallbackUrl::Valid(text.to_owned())
    }

    #[test]
    fn adds_a_single_slash() {
        assert_eq!(
            callback_url("https", "example.com", "51dpipeline/json"),
            valid("https://example.com/51dpipeline/json")
        );
    }

    #[test]
    fn keeps_a_single_slash_on_the_endpoint() {
        assert_eq!(
            callback_url("https", "example.com", "/51dpipeline/json"),
            valid("https://example.com/51dpipeline/json")
        );
    }

    #[test]
    fn collapses_a_double_slash() {
        assert_eq!(
            callback_url("http", "example.com/", "/json"),
            valid("http://example.com/json")
        );
    }

    #[test]
    fn missing_when_a_part_is_empty() {
        assert_eq!(callback_url("https", "", "/json"), CallbackUrl::Missing);
        assert_eq!(
            callback_url("", "example.com", "/json"),
            CallbackUrl::Missing
        );
        assert_eq!(
            callback_url("https", "example.com", ""),
            CallbackUrl::Missing
        );
        assert_eq!(callback_url("https", "  ", "/json"), CallbackUrl::Missing);
    }

    // The hosts .NET 4.5.128's JavaScript builder accepts, with the URL the
    // crate writes. The hosts .NET accepts that hold a zone identifier, user
    // information, a `?` or a `#`, or that end in a number without being an
    // IPv4 address, are refused, in the tests further down.
    #[test]
    fn accepts_the_hosts_dotnet_accepts() {
        let cases = [
            ("EXAMPLE.com", "https://example.com/51dpipeline/json"),
            (
                "example.com:8080",
                "https://example.com:8080/51dpipeline/json",
            ),
            ("example.com:", "https://example.com:/51dpipeline/json"),
            (
                "example.com:65535",
                "https://example.com:65535/51dpipeline/json",
            ),
            ("a_b.com", "https://a_b.com/51dpipeline/json"),
            ("-ab.com", "https://-ab.com/51dpipeline/json"),
            ("127.0.0.1:80", "https://127.0.0.1:80/51dpipeline/json"),
            ("[::1]", "https://[::1]/51dpipeline/json"),
            ("[::1]:8080", "https://[::1]:8080/51dpipeline/json"),
            ("example.com.", "https://example.com./51dpipeline/json"),
            (
                "example.com/path",
                "https://example.com/path/51dpipeline/json",
            ),
            (
                "example.com/a b",
                "https://example.com/a%20b/51dpipeline/json",
            ),
            (
                "example.com/a%zzb",
                "https://example.com/a%25zzb/51dpipeline/json",
            ),
            (
                "example.com/a%41b",
                "https://example.com/a%41b/51dpipeline/json",
            ),
            (
                "example.com/\u{fc}",
                "https://example.com/%C3%BC/51dpipeline/json",
            ),
            (
                "example.com/a\nb",
                "https://example.com/a%0Ab/51dpipeline/json",
            ),
            (
                "example.com/a\u{2028}b",
                "https://example.com/a%E2%80%A8b/51dpipeline/json",
            ),
            ("a-b.com", "https://a-b.com/51dpipeline/json"),
            ("ab-.com", "https://ab-.com/51dpipeline/json"),
            ("127.0.0.1", "https://127.0.0.1/51dpipeline/json"),
            ("localhost", "https://localhost/51dpipeline/json"),
            ("a.b.c.d.e.f", "https://a.b.c.d.e.f/51dpipeline/json"),
            (
                "xn--bcher-kva.de",
                "https://xn--bcher-kva.de/51dpipeline/json",
            ),
            (
                "example.com:8080/x",
                "https://example.com:8080/x/51dpipeline/json",
            ),
            (
                "example.com/a'b",
                "https://example.com/a%27b/51dpipeline/json",
            ),
        ];
        for (host, expected) in cases {
            assert_eq!(url("https", host), valid(expected), "host {host:?}");
        }
        for length in [63, 64] {
            let host = format!("{}.com", "a".repeat(length));
            assert_eq!(
                url("https", &host),
                valid(&format!("https://{host}/51dpipeline/json")),
                "host {host:?}"
            );
        }
    }

    // The hosts .NET 4.5.128's JavaScript builder refuses.
    #[test]
    fn refuses_the_hosts_dotnet_refuses() {
        let hosts = [
            "example.com:65536",
            "example.com:99999",
            "example.com:abc",
            "exa mple.com",
            "example.com');alert(1);//",
            "a\"b.com",
            "a'b.com",
            "a<b.com",
            "a>b.com",
            "::1",
            "[::1",
            "[v1.x]",
            "[1.2.3.4]",
            "a@b@example.com",
            "a..b.com",
            "%41.com",
            "a\\b.com",
            "a`b.com",
            "a{b}.com",
            "a|b.com",
            "a^b.com",
            "a(b).com",
            "a;b.com",
            "a,b.com",
            "a!b.com",
            "a*b.com",
            "a+b.com",
            "a=b.com",
            "a$b.com",
            "a&b.com",
            "a~b.com",
            "a\tb.com",
            "a\nb.com",
            " example.com",
            "example.com ",
            "example.com:8080');alert(1);//",
        ];
        for host in hosts {
            assert!(
                matches!(url("https", host), CallbackUrl::Invalid(_)),
                "host {host:?}"
            );
        }
    }

    // Inside the brackets only an IPv6 address, and after them only a port.
    #[test]
    fn refuses_a_zone_or_text_after_the_bracket_that_is_not_valid() {
        let hosts = [
            "[fe80::1%25]",
            "[fe80::1%25'+alert(1)+']",
            "[fe80::1%eth0]",
            "[fe80::1%25eth%0]",
            "[::1]x",
            "[::1]80",
            "[::1]:80x",
            "[::1]]",
        ];
        for host in hosts {
            assert!(
                matches!(url("https", host), CallbackUrl::Invalid(_)),
                "host {host:?}"
            );
        }
    }

    // A `%` inside the brackets is refused, zone identifier or not, because
    // the URL Standard's IPv6 parser fails on it, so a browser could not
    // parse the URL.
    #[test]
    fn refuses_a_zone_identifier() {
        let hosts = [
            "[fe80::1%25eth0]",
            "[fe80::1%25eth0]:8080",
            "[FE80::1%25ETH0]",
            "[fe80::1%251]",
            "[::1%25lo]",
            "[fe80::1%]",
            "[fe80::1%25eth0]/path",
        ];
        for host in hosts {
            assert!(
                matches!(url("https", host), CallbackUrl::Invalid(_)),
                "host {host:?}"
            );
        }
    }

    // Any `@` in the authority is refused, because `fetch` throws a
    // TypeError for a URL with a user name or a password. An `@` after the
    // authority is part of the path.
    #[test]
    fn refuses_user_information() {
        let hosts = [
            "user@example.com",
            "user:pw@example.com",
            "@example.com",
            ":@example.com",
            "user@example.com:8080",
            "user@[::1]",
            "user@example.com/path",
            "u%41@example.com",
            "u x@example.com",
            "u\tx@example.com",
            "u\"x@example.com",
            "u'x@example.com",
            "u\\x@example.com",
            "u\nx@example.com",
            "u\u{1}x@example.com",
        ];
        for host in hosts {
            assert!(
                matches!(url("https", host), CallbackUrl::Invalid(_)),
                "host {host:?}"
            );
        }
        assert_eq!(
            url("https", "example.com/a@b"),
            valid("https://example.com/a@b/51dpipeline/json")
        );
    }

    // A `?` or `#` anywhere in the host is refused, because it would start
    // the query or the fragment before the endpoint, so the script would
    // call back to the wrong path.
    #[test]
    fn refuses_a_query_or_fragment_in_the_host() {
        let hosts = [
            "example.com?x",
            "example.com#x",
            "example.com?",
            "example.com#",
            "example.com:8080?x",
            "[::1]#x",
            "example.com/path?x",
            "example.com/path#x",
            "example.com?');alert(1);//",
            "example.com#');alert(1);//",
        ];
        for host in hosts {
            assert!(
                matches!(url("https", host), CallbackUrl::Invalid(_)),
                "host {host:?}"
            );
        }
    }

    // A host whose last label is all digits, or `0x` or `0X` followed by zero
    // or more hex digits, is read by a browser's URL parser as an IPv4
    // address, so it must be four decimal parts of 0 to 255, each with no zero
    // before another digit, and no `.` at the end. One `.` at the end is
    // dropped before the last label is read, so `example.123.` ends in a
    // number too.
    #[test]
    fn a_host_ending_in_a_number_must_be_ipv4() {
        let hosts = [
            "example.123",
            "example.0x1f",
            "example.0X",
            "1.2.3.999",
            "1.2.3",
            "127.1",
            "0x7f.0.0.1",
            "01.2.3.4",
            "00.1.2.3",
            "example.123.",
            "1.2.3.4.",
            "1.2.3.4.5",
            "1",
            "300.1.1.1",
            "0x7f.1",
        ];
        for host in hosts {
            assert!(
                matches!(url("https", host), CallbackUrl::Invalid(_)),
                "host {host:?}"
            );
        }
        let cases = [
            ("1.2.3.4", "https://1.2.3.4/51dpipeline/json"),
            ("1.2.3.4:8080", "https://1.2.3.4:8080/51dpipeline/json"),
            ("1.2.3.4/path", "https://1.2.3.4/path/51dpipeline/json"),
            ("example.com", "https://example.com/51dpipeline/json"),
            ("example.co1", "https://example.co1/51dpipeline/json"),
            (
                "123.example.com",
                "https://123.example.com/51dpipeline/json",
            ),
            ("0.0.0.0", "https://0.0.0.0/51dpipeline/json"),
            ("101.1.1.1", "https://101.1.1.1/51dpipeline/json"),
        ];
        for (host, expected) in cases {
            assert_eq!(url("https", host), valid(expected), "host {host:?}");
        }
    }

    // A port goes by its value, so leading zeros are allowed, and a port too
    // long to parse is refused rather than read as small enough.
    #[test]
    fn checks_the_port_by_its_value() {
        assert_eq!(
            url("https", "example.com:00080"),
            valid("https://example.com:00080/51dpipeline/json")
        );
        assert_eq!(
            url("https", "example.com:0065535"),
            valid("https://example.com:0065535/51dpipeline/json")
        );
        for host in [
            "example.com:0065536",
            "example.com:123456",
            "example.com:4294967296",
            "example.com:99999999999999999999",
        ] {
            assert!(
                matches!(url("https", host), CallbackUrl::Invalid(_)),
                "host {host:?}"
            );
        }
    }

    // Each character rule 8 lists, after the host and in the endpoint, and a
    // `%` without two hexadecimal digits after it, even at the end.
    #[test]
    fn encodes_the_characters_rule_8_lists() {
        assert_eq!(
            callback_url("https", "example.com/\"^`{|}", "/j\"^`{|}"),
            valid("https://example.com/%22%5E%60%7B%7C%7D/j%22%5E%60%7B%7C%7D")
        );
        let cases = [
            (
                "example.com/a%",
                "https://example.com/a%25/51dpipeline/json",
            ),
            (
                "example.com/a%4",
                "https://example.com/a%254/51dpipeline/json",
            ),
            (
                "example.com/a%41",
                "https://example.com/a%41/51dpipeline/json",
            ),
        ];
        for (host, expected) in cases {
            assert_eq!(url("https", host), valid(expected), "host {host:?}");
        }
        assert_eq!(
            callback_url("https", "example.com", "/json%"),
            valid("https://example.com/json%25")
        );
    }

    #[test]
    fn the_invalid_url_is_the_one_built() {
        assert_eq!(
            url("https", "exa mple.com"),
            CallbackUrl::Invalid("https://exa mple.com/51dpipeline/json".to_owned())
        );
    }

    // Rule 6 of the callback URL keeps everything in the host value before
    // its last `@` out of the log, after the first `/` too, so a password
    // holding `/`, `?` or `#` is hidden.
    // https://github.com/51Degrees/specifications/blob/99189c7aa040eaa7755e395e2c9f7110bc969ab0/pipeline-specification/pipeline-elements/javascript-builder.md#L243-L258
    #[test]
    fn the_invalid_url_hides_the_host_value_before_its_last_at() {
        let cases = [
            (
                "user:pa/ss@example.com",
                "https://***@example.com/51dpipeline/json",
            ),
            (
                "user:pa?ss@example.com",
                "https://***@example.com/51dpipeline/json",
            ),
            ("exa mple.com/a@b", "https://***@b/51dpipeline/json"),
            ("u@exa mple.com/a@b/c", "https://***@b/c/51dpipeline/json"),
            (
                "a@b@example.com",
                "https://***@example.com/51dpipeline/json",
            ),
            ("exa mple.com/a", "https://exa mple.com/a/51dpipeline/json"),
        ];
        for (host, logged) in cases {
            assert_eq!(
                url("https", host),
                CallbackUrl::Invalid(logged.to_owned()),
                "host {host:?}"
            );
        }
    }

    // Only `http` and `https`, ignoring case, as rule 1 of the callback URL
    // rule in the JavaScript builder specification asks.
    #[test]
    fn checks_the_scheme() {
        for protocol in ["https", "http", "HtTp"] {
            assert!(
                matches!(url(protocol, "example.com"), CallbackUrl::Valid(_)),
                "protocol {protocol:?}"
            );
        }
        assert_eq!(
            url("HTTPS", "example.com"),
            valid("https://example.com/51dpipeline/json")
        );
        for protocol in [
            "ftp",
            "h+t-p.s",
            "ws",
            "javascript",
            "httpx",
            "https ",
            "1http",
            "h t",
            "x');alert(1);//",
            "a://b",
        ] {
            assert!(
                matches!(url(protocol, "example.com"), CallbackUrl::Invalid(_)),
                "protocol {protocol:?}"
            );
        }
    }

    // A quote, a backslash or a line break never reaches the script, wherever
    // in the URL it was given.
    #[test]
    fn nothing_given_can_end_the_string() {
        let hosts = [
            "example.com/');alert(1);//",
            "example.com/a\\b",
            "example.com/a\"b",
            "example.com/</script>",
        ];
        for host in hosts {
            let CallbackUrl::Valid(text) = url("https", host) else {
                panic!("host {host:?} was refused");
            };
            assert!(
                !text.contains(['\'', '"', '\\', '<', '>', '\n', '\r', '\u{2028}', '\u{2029}']),
                "host {host:?} gave {text:?}"
            );
        }
        let CallbackUrl::Valid(text) = callback_url("https", "example.com", "/json'x") else {
            panic!("the endpoint was refused");
        };
        assert_eq!(text, "https://example.com/json%27x");
    }

    // A host must be ASCII, so an internationalized name is given as A-labels,
    // and anything else outside ASCII is refused.
    #[test]
    fn refuses_hosts_outside_ascii() {
        for host in [
            "b\u{fc}cher.de",
            "a\u{a0}b.com",
            "a\u{200b}b.com",
            "a\u{2028}b.com",
            "\u{65e5}\u{672c}.jp",
            "\u{e9}xample.com",
            "\u{430}pple.com",
        ] {
            assert!(
                matches!(url("https", host), CallbackUrl::Invalid(_)),
                "host {host:?}"
            );
        }
    }
}
