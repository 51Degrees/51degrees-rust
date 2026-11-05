//! The address each usage record gives as `ServerIP`.
//!
//! It is the first address the host's own name resolves to that is IPv4 and
//! is not `127.0.0.1`, otherwise the first that is neither `127.0.0.1` nor
//! `::1`, otherwise empty. On Linux and macOS the addresses of the host's
//! interfaces that are up follow the resolver's answer, as .NET's resolver
//! adds them for the host's own name, so a name that resolves only to
//! `127.0.0.1` still gives the address of a network interface.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};

/// The space a host name is read into, in bytes, with room for the
/// terminating zero. POSIX and Windows host names are shorter.
#[cfg(any(unix, windows))]
const HOST_NAME_BUFFER_LENGTH: usize = 256;

/// The address, or empty when the host has no name, as on WebAssembly, or
/// its name does not resolve.
pub(crate) fn host_address() -> String {
    let Some(name) = host_name() else {
        log::debug!("Usage sharing found no host name, so ServerIP is empty");
        return String::new();
    };
    match (name.as_str(), 0).to_socket_addrs() {
        Ok(resolved) => {
            let mut addresses: Vec<SocketAddr> = resolved.collect();
            addresses.extend(interface_addresses());
            choose(&addresses)
        }
        Err(e) => {
            log::debug!(
                "Usage sharing could not resolve the host name {name}, so \
                 ServerIP is empty: {e}"
            );
            String::new()
        }
    }
}

/// The first address that is IPv4 and not `127.0.0.1`, otherwise the first
/// that is neither `127.0.0.1` nor `::1`, otherwise empty.
fn choose(addresses: &[SocketAddr]) -> String {
    addresses
        .iter()
        .find(|address| address.is_ipv4() && !is_local_host(address))
        .or_else(|| addresses.iter().find(|address| !is_local_host(address)))
        .map(address_text)
        .unwrap_or_default()
}

/// Whether the address is exactly `127.0.0.1`, or `::1` with no scope.
/// Other loopback addresses, such as `127.0.1.1`, can be chosen.
fn is_local_host(address: &SocketAddr) -> bool {
    match address {
        SocketAddr::V4(v4) => *v4.ip() == Ipv4Addr::LOCALHOST,
        SocketAddr::V6(v6) => *v6.ip() == Ipv6Addr::LOCALHOST && v6.scope_id() == 0,
    }
}

/// The address as text, with an IPv6 scope after `%` where there is one.
fn address_text(address: &SocketAddr) -> String {
    match address {
        SocketAddr::V4(v4) => v4.ip().to_string(),
        SocketAddr::V6(v6) if v6.scope_id() != 0 => format!("{}%{}", v6.ip(), v6.scope_id()),
        SocketAddr::V6(v6) => v6.ip().to_string(),
    }
}

/// The host's name, from `gethostname`.
#[cfg(unix)]
fn host_name() -> Option<String> {
    let mut buffer = [0u8; HOST_NAME_BUFFER_LENGTH];
    // SAFETY: the buffer is writable for its whole length.
    if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } != 0 {
        return None;
    }
    // A name that fills the buffer may have no terminating zero, so the last
    // byte is never part of it.
    let readable = &buffer[..HOST_NAME_BUFFER_LENGTH - 1];
    let length = readable
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(readable.len());
    std::str::from_utf8(&readable[..length])
        .ok()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

/// The host's name, from Winsock's `gethostname`.
#[cfg(windows)]
fn host_name() -> Option<String> {
    /// Room for the `WSADATA` structure that `WSAStartup` fills in, which is
    /// 408 bytes at most, as 8-byte words so it is aligned. Nothing in it is
    /// read.
    const WSA_DATA_WORDS: usize = 64;

    /// Winsock 2.2.
    const WINSOCK_VERSION: u16 = 0x0202;

    #[link(name = "ws2_32")]
    extern "system" {
        fn WSAStartup(version: u16, data: *mut u64) -> i32;
        fn WSACleanup() -> i32;
        fn gethostname(name: *mut u8, length: i32) -> i32;
    }

    let mut buffer = [0u8; HOST_NAME_BUFFER_LENGTH];
    let capacity = i32::try_from(buffer.len()).ok()?;
    let mut data = [0u64; WSA_DATA_WORDS];
    // SAFETY: `data` is writable and larger than WSADATA. Winsock counts each
    // start, so this start and the cleanup below leave any other use of
    // Winsock in the process as it was.
    if unsafe { WSAStartup(WINSOCK_VERSION, data.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: the buffer is writable for `capacity` bytes.
    let result = unsafe { gethostname(buffer.as_mut_ptr(), capacity) };
    // SAFETY: matches the successful start above.
    unsafe { WSACleanup() };
    if result != 0 {
        return None;
    }
    let length = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    std::str::from_utf8(&buffer[..length])
        .ok()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

/// No host name is known on other systems.
#[cfg(not(any(unix, windows)))]
fn host_name() -> Option<String> {
    None
}

/// An address of an interface that is up.
#[cfg(any(target_os = "linux", target_os = "macos", test))]
struct InterfaceAddress {
    address: SocketAddr,
    loopback: bool,
}

/// The addresses of the host's interfaces that are up, in the order the
/// system lists them.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn interface_addresses() -> Vec<SocketAddr> {
    let mut first: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs writes the head of a list it allocates to `first`,
    // which is freed below.
    if unsafe { libc::getifaddrs(&mut first) } != 0 {
        log::debug!(
            "Usage sharing could not list the interface addresses: {}",
            std::io::Error::last_os_error()
        );
        return Vec::new();
    }
    let mut found = Vec::new();
    let mut next = first;
    while !next.is_null() {
        // SAFETY: `next` is a node of the list getifaddrs gave, which stays
        // valid until freeifaddrs.
        let interface = unsafe { &*next };
        next = interface.ifa_next;
        if (interface.ifa_flags & libc::IFF_UP as libc::c_uint) == 0 {
            continue;
        }
        // SAFETY: an interface's address is null or a socket address whose
        // family says its type.
        if let Some(address) = unsafe { socket_address(interface.ifa_addr) } {
            found.push(InterfaceAddress {
                address,
                loopback: (interface.ifa_flags & libc::IFF_LOOPBACK as libc::c_uint) != 0,
            });
        }
    }
    // SAFETY: `first` came from getifaddrs and is freed once.
    unsafe { libc::freeifaddrs(first) };
    leave_out_loopback(found)
}

/// No interface addresses are added on other systems.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn interface_addresses() -> Vec<SocketAddr> {
    Vec::new()
}

/// The IPv4 or IPv6 address a socket address holds.
///
/// # Safety
///
/// `address` is null or points to a socket address whose family says its
/// type.
#[cfg(any(target_os = "linux", target_os = "macos"))]
unsafe fn socket_address(address: *const libc::sockaddr) -> Option<SocketAddr> {
    if address.is_null() {
        return None;
    }
    // SAFETY: the caller's guarantee. Each read copies the bytes, so the
    // socket address needs no particular alignment.
    unsafe {
        let family = std::ptr::addr_of!((*address).sa_family).read_unaligned();
        match libc::c_int::from(family) {
            libc::AF_INET => {
                let v4 = std::ptr::read_unaligned(address.cast::<libc::sockaddr_in>());
                let ip = Ipv4Addr::from(v4.sin_addr.s_addr.to_ne_bytes());
                Some(SocketAddr::V4(std::net::SocketAddrV4::new(ip, 0)))
            }
            libc::AF_INET6 => {
                let v6 = std::ptr::read_unaligned(address.cast::<libc::sockaddr_in6>());
                let ip = Ipv6Addr::from(v6.sin6_addr.s6_addr);
                Some(SocketAddr::V6(std::net::SocketAddrV6::new(
                    ip,
                    0,
                    0,
                    v6.sin6_scope_id,
                )))
            }
            _ => None,
        }
    }
}

/// The interface addresses, less each loopback address of a family in which
/// some interface has an address that is not loopback.
#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn leave_out_loopback(found: Vec<InterfaceAddress>) -> Vec<SocketAddr> {
    let other = |ipv4: bool| {
        found
            .iter()
            .any(|entry| entry.address.is_ipv4() == ipv4 && !entry.loopback)
    };
    let (other_v4, other_v6) = (other(true), other(false));
    found
        .into_iter()
        .filter(|entry| {
            let family_has_other = if entry.address.is_ipv4() {
                other_v4
            } else {
                other_v6
            };
            !(entry.loopback && family_has_other)
        })
        .map(|entry| entry.address)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(text: &str) -> SocketAddr {
        SocketAddr::new(text.parse().unwrap(), 0)
    }

    fn v6(text: &str, scope: u32) -> SocketAddr {
        SocketAddr::V6(std::net::SocketAddrV6::new(
            text.parse().unwrap(),
            0,
            0,
            scope,
        ))
    }

    #[test]
    fn the_first_ipv4_address_other_than_127_0_0_1_is_chosen() {
        let addresses = [
            v6("2001:db8::5", 0),
            v4("127.0.0.1"),
            v4("192.0.2.10"),
            v4("192.0.2.11"),
        ];
        assert_eq!(choose(&addresses), "192.0.2.10");
    }

    #[test]
    fn only_127_0_0_1_is_left_out_of_the_ipv4_loopback_addresses() {
        // Debian and Ubuntu map the host's name to 127.0.1.1.
        assert_eq!(choose(&[v4("127.0.0.1"), v4("127.0.1.1")]), "127.0.1.1");
    }

    #[test]
    fn without_ipv4_the_first_address_other_than_loopback_is_chosen() {
        let addresses = [
            v6("::1", 0),
            v4("127.0.0.1"),
            v6("fe80::1", 3),
            v6("2001:db8::5", 0),
        ];
        assert_eq!(choose(&addresses), "fe80::1%3");
        assert_eq!(choose(&[v6("::1", 0), v6("2001:db8::5", 0)]), "2001:db8::5");
    }

    #[test]
    fn only_127_0_0_1_and_ipv6_loopback_give_empty() {
        assert_eq!(
            choose(&[v4("127.0.0.1"), v6("::1", 0), v4("127.0.0.1")]),
            ""
        );
        assert_eq!(choose(&[]), "");
    }

    #[test]
    fn loopback_is_left_out_only_where_its_family_has_another_address() {
        let entry = |address: SocketAddr, loopback: bool| InterfaceAddress { address, loopback };
        let found = vec![
            entry(v4("127.0.0.1"), true),
            entry(v6("::1", 0), true),
            entry(v4("192.0.2.10"), false),
        ];
        assert_eq!(leave_out_loopback(found), [v6("::1", 0), v4("192.0.2.10")]);

        let found = vec![
            entry(v4("127.0.0.1"), true),
            entry(v6("::1", 0), true),
            entry(v6("fe80::1", 2), false),
        ];
        assert_eq!(
            leave_out_loopback(found),
            [v4("127.0.0.1"), v6("fe80::1", 2)]
        );
    }

    /// The name read through the platform call is the one the system's own
    /// `hostname` command prints.
    #[cfg(any(unix, windows))]
    #[test]
    fn the_host_name_is_the_one_the_system_reports() {
        let Ok(output) = std::process::Command::new("hostname").output() else {
            eprintln!("No hostname command on this host, so nothing to compare");
            return;
        };
        let reported = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        assert!(!reported.is_empty(), "the hostname command printed nothing");
        assert_eq!(host_name().as_deref(), Some(reported.as_str()));
    }

    /// On this host the address is empty or follows the rule.
    #[cfg(any(unix, windows))]
    #[test]
    fn the_address_on_this_host_follows_the_rule() {
        let text = host_address();
        if text.is_empty() {
            return;
        }
        let address = text.split('%').next().unwrap_or_default();
        let ip: std::net::IpAddr = address
            .parse()
            .unwrap_or_else(|e| panic!("{text} is not an address: {e}"));
        assert_ne!(ip, std::net::IpAddr::from(Ipv4Addr::LOCALHOST), "{text}");
        assert_ne!(text, "::1");
        if ip.is_ipv6() {
            let name = host_name().unwrap();
            let mut addresses: Vec<SocketAddr> =
                (name.as_str(), 0).to_socket_addrs().unwrap().collect();
            addresses.extend(interface_addresses());
            assert!(
                !addresses.iter().any(|a| a.is_ipv4() && !is_local_host(a)),
                "an IPv4 address was available but {text} was chosen"
            );
        }
    }
}
