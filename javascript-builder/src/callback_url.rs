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
//! in the user information, after the host and in the endpoint that could
//! end the string or the script are percent-encoded.

use std::net::Ipv6Addr;

/// The callback URL for one request.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CallbackUrl {
    /// The protocol, host or endpoint is empty, so the script has no URL.
    Missing,
    /// The URL is not valid, so the script has no URL. Carries the URL as it
    /// was built, for the log.
    Invalid(String),
    /// The URL to write into the script.
    Valid(String),
}

/// The largest port number.
const MAX_PORT: u32 = 65_535;

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
    let built = format!("{protocol}://{host}{endpoint}");
    if !is_scheme(protocol) {
        return CallbackUrl::Invalid(built);
    }
    let end = host.find(['/', '?', '#']).unwrap_or(host.len());
    let (authority, rest) = host.split_at(end);
    match checked_authority(authority) {
        Some(authority) => CallbackUrl::Valid(format!(
            "{}://{authority}{}{}",
            protocol.to_ascii_lowercase(),
            encoded(rest),
            encoded(&endpoint)
        )),
        None => CallbackUrl::Invalid(built),
    }
}

/// `http` or `https`, ignoring case. The script uses the URL only with
/// `fetch` and `XMLHttpRequest`, which reach a server with no other scheme.
fn is_scheme(protocol: &str) -> bool {
    protocol.eq_ignore_ascii_case("http") || protocol.eq_ignore_ascii_case("https")
}

/// The user information, host and port, or `None` when the host or the port is
/// not valid.
fn checked_authority(authority: &str) -> Option<String> {
    let (user, host_port) = match authority.split_once('@') {
        Some((user, host_port)) => (Some(user), host_port),
        None => (None, authority),
    };
    let host_port = checked_host_port(host_port)?;
    Some(match user {
        Some(user) => format!("{}@{host_port}", encoded(user)),
        None => host_port,
    })
}

/// A host and an optional port. The host is a bracketed IPv6 address, or a
/// name of non-empty labels separated by dots, with a dot allowed at the end,
/// and the port is up to 65535 or empty.
fn checked_host_port(host_port: &str) -> Option<String> {
    let (host, port) = if let Some(inner) = host_port.strip_prefix('[') {
        let (address, after) = inner.split_once(']')?;
        let (address, zone) = match address.split_once("%25") {
            Some((address, zone)) => (address, Some(zone)),
            None => (address, None),
        };
        address.parse::<Ipv6Addr>().ok()?;
        if zone.is_some_and(|zone| zone.is_empty() || !zone.bytes().all(is_unreserved)) {
            return None;
        }
        (&host_port[..host_port.len() - after.len()], after)
    } else {
        let end = host_port.find(':').unwrap_or(host_port.len());
        let (host, port) = host_port.split_at(end);
        let name = host.strip_suffix('.').unwrap_or(host);
        if !name.split('.').all(is_label) {
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

/// The unreserved characters of RFC 3986 section 2.3.
fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
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
    // crate writes.
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
            (
                "[fe80::1%25eth0]",
                "https://[fe80::1%25eth0]/51dpipeline/json",
            ),
            (
                "user:pw@example.com",
                "https://user:pw@example.com/51dpipeline/json",
            ),
            (
                "u x@example.com",
                "https://u%20x@example.com/51dpipeline/json",
            ),
            (
                "u\tx@example.com",
                "https://u%09x@example.com/51dpipeline/json",
            ),
            ("example.com.", "https://example.com./51dpipeline/json"),
            (
                "example.com/path",
                "https://example.com/path/51dpipeline/json",
            ),
            ("example.com?x", "https://example.com?x/51dpipeline/json"),
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
            ("300.1.1.1", "https://300.1.1.1/51dpipeline/json"),
            ("localhost", "https://localhost/51dpipeline/json"),
            ("1", "https://1/51dpipeline/json"),
            ("0x7f.1", "https://0x7f.1/51dpipeline/json"),
            ("a.b.c.d.e.f", "https://a.b.c.d.e.f/51dpipeline/json"),
            (
                "xn--bcher-kva.de",
                "https://xn--bcher-kva.de/51dpipeline/json",
            ),
            (
                "user@example.com",
                "https://user@example.com/51dpipeline/json",
            ),
            (
                "u%41@example.com",
                "https://u%41@example.com/51dpipeline/json",
            ),
            (
                "u\"x@example.com",
                "https://u%22x@example.com/51dpipeline/json",
            ),
            (
                "u\nx@example.com",
                "https://u%0Ax@example.com/51dpipeline/json",
            ),
            (
                "u\u{1}x@example.com",
                "https://u%01x@example.com/51dpipeline/json",
            ),
            ("example.com#x", "https://example.com#x/51dpipeline/json"),
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

    // Inside the brackets only an IPv6 address, optionally followed by `%25`
    // and a zone of unreserved characters, and after them only a port.
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
        assert_eq!(
            url("https", "[fe80::1%25eth0]:8080"),
            valid("https://[fe80::1%25eth0]:8080/51dpipeline/json")
        );
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

    // Each character rule 8 lists, in the user information, after the host
    // and in the endpoint, and a `%` without two hexadecimal digits after it,
    // even at the end.
    #[test]
    fn encodes_the_characters_rule_8_lists() {
        assert_eq!(
            callback_url("https", "u\"^`{|}@example.com/\"^`{|}", "/j\"^`{|}"),
            valid(
                "https://u%22%5E%60%7B%7C%7D@example.com/%22%5E%60%7B%7C%7D\
                 /j%22%5E%60%7B%7C%7D"
            )
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

    // Only `http` and `https`, ignoring case, as specifications#74 at 5e998b1
    // asks.
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
            "example.com?');alert(1);//",
            "example.com#');alert(1);//",
            "u'x@example.com",
            "u\\x@example.com",
            "example.com/a\\b",
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
