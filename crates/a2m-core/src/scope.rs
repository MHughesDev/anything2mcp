//! What kind of place an address is, read from its literal alone (decision
//! 0240).
//!
//! [`ScopedHost`] reads a host as it is written: an IP address, a host name,
//! or neither. It resolves no name and makes no system call. An IPv4 address
//! written inside IPv6 (`::ffff:127.0.0.1`) is read as the IPv4 address it
//! means, and the reading remembers that it was written so. A zone
//! (`fe80::1%1`) is remembered too. What it cannot read is
//! [`AddressScope::Unreadable`], never a guess (rule 7).
//!
//! [`ScopedHost::may_leave_this_machine`] is the one question every check of
//! a plain link asks: whether a link to the host may leave this machine. Only
//! a loopback address written plainly as an IP address stays: no name,
//! `localhost` included (decision 0188), no IPv4 address inside IPv6, and no
//! address with a zone.
//!
//! The ranges are the ones the Rust core library's `core::net` classifies,
//! read from its source (`library/core/src/net/ip_addr.rs`, cited as S37 in
//! research note 28) on 2026-11-18. That source cites the standard for each
//! range, named here beside each. The IANA special-purpose address
//! registries it points to could not be opened from where this was written.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::vocabulary::vocabulary;

vocabulary! {
    /// What kind of place an address is, as far as its literal says. It is
    /// read from the address alone: nothing is resolved and nothing is
    /// asked of the machine, so it says nothing of which network the address
    /// is on or how the machine reaches it.
    pub enum AddressScope: "address scope" {
        /// A loopback address: `127.0.0.0/8` (RFC 1122) or `::1` (RFC 4291
        /// section 2.5.3). Never a name.
        ThisMachine => "this_machine",
        /// The unspecified address, `0.0.0.0` or `::`.
        Unspecified => "unspecified",
        /// Valid on one link only: `169.254.0.0/16` (RFC 3927) or
        /// `fe80::/10` (RFC 4291 section 2.4).
        LinkLocal => "link_local",
        /// For private use: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`
        /// (RFC 1918).
        PrivateUse => "private_use",
        /// The shared address space carriers use for carrier-grade NAT,
        /// `100.64.0.0/10` (RFC 6598).
        CarrierGradeNat => "carrier_grade_nat",
        /// A unique local IPv6 address, `fc00::/7` (RFC 4193).
        UniqueLocal => "unique_local",
        /// A multicast address: `224.0.0.0/4` (RFC 5771) or `ff00::/8`
        /// (RFC 4291 section 2.7).
        Multicast => "multicast",
        /// Reserved for documentation: `192.0.2.0/24`, `198.51.100.0/24`,
        /// `203.0.113.0/24` (RFC 5737); `2001:db8::/32` (RFC 3849) and
        /// `3fff::/20` (RFC 9637).
        Documentation => "documentation",
        /// Another special-purpose range that is not globally reachable:
        /// reserved, benchmarking, broadcast, or assigned to a protocol.
        Reserved => "reserved",
        /// In no special-purpose range this reading holds.
        Public => "public",
        /// A host name. It is kept as written and never resolved here.
        Name => "name",
        /// Neither an IP address nor a host name as this reading takes them:
        /// malformed, a spelling only a lenient parser widens (`127.1`), or no
        /// network address at all, such as a path.
        Unreadable => "unreadable",
    }
}

/// A host, and what kind of place its literal says it is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScopedHost {
    scope: AddressScope,
    ip: Option<IpAddr>,
    name: Option<String>,
    ipv4_in_ipv6: bool,
    zone: bool,
}

impl ScopedHost {
    /// An IP address. An IPv4 address inside IPv6 is read as the IPv4
    /// address.
    #[must_use]
    pub fn of_ip(ip: IpAddr) -> Self {
        let (ip, ipv4_in_ipv6) = match ip {
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => (IpAddr::V4(v4), true),
                None => (ip, false),
            },
            IpAddr::V4(_) => (ip, false),
        };
        let scope = match ip {
            IpAddr::V4(v4) => scope_of_v4(v4),
            IpAddr::V6(v6) => scope_of_v6(v6),
        };
        Self {
            scope,
            ip: Some(ip),
            name: None,
            ipv4_in_ipv6,
            zone: false,
        }
    }

    /// A socket address, as a connection would be made to it. Its port is
    /// not kept; a zone (a scope identifier other than 0) is.
    #[must_use]
    pub fn of_socket_address(address: SocketAddr) -> Self {
        let mut host = Self::of_ip(address.ip());
        if let SocketAddr::V6(v6) = address {
            host.zone = v6.scope_id() != 0;
        }
        host
    }

    /// A host as written, without a port: an IPv4 address in four decimal
    /// parts, an IPv6 address bare or in brackets, with or without a zone,
    /// or a host name. This is the form a URL parser gives as a URL's host.
    #[must_use]
    pub fn of_host(host: &str) -> Self {
        if let Some(inner) = host.strip_prefix('[') {
            return match inner.strip_suffix(']') {
                Some(inner) if inner.contains(':') => Self::of_ipv6_text(inner),
                _ => Self::unreadable(),
            };
        }
        if host.contains(':') {
            return Self::of_ipv6_text(host);
        }
        if let Ok(v4) = host.parse::<Ipv4Addr>() {
            return Self::of_ip(IpAddr::V4(v4));
        }
        if is_host_name(host) {
            return Self {
                scope: AddressScope::Name,
                ip: None,
                name: Some(host.to_owned()),
                ipv4_in_ipv6: false,
                zone: false,
            };
        }
        Self::unreadable()
    }

    /// A destination's address as a surface or a manifest writes it: a URL
    /// (`scheme://host:port/path`), a host with a port (`host:port`,
    /// `[v6]:port`), or a bare host. A URL with a user name or a password,
    /// and anything else, is unreadable.
    #[must_use]
    pub fn of_destination(address: &str) -> Self {
        let authority = match address.split_once("://") {
            Some((_, rest)) => rest.split(['/', '?', '#']).next().unwrap_or_default(),
            None => address,
        };
        if authority.contains('@') {
            return Self::unreadable();
        }
        if let Some(inner) = authority.strip_prefix('[') {
            let Some((host, after)) = inner.split_once(']') else {
                return Self::unreadable();
            };
            if !after.is_empty() && !after.strip_prefix(':').is_some_and(is_port) {
                return Self::unreadable();
            }
            return Self::of_host(&format!("[{host}]"));
        }
        match authority.matches(':').count() {
            0 => Self::of_host(authority),
            1 => match authority.split_once(':') {
                Some((host, port)) if is_port(port) => Self::of_host(host),
                _ => Self::unreadable(),
            },
            _ => Self::of_host(authority),
        }
    }

    /// What kind of place the literal says the host is.
    #[must_use]
    pub fn scope(&self) -> AddressScope {
        self.scope
    }

    /// The IP address, read as IPv4 where it was written inside IPv6.
    #[must_use]
    pub fn ip(&self) -> Option<IpAddr> {
        self.ip
    }

    /// The host name, as written. Nothing here resolves it.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Whether the address was written as an IPv4 address inside IPv6.
    #[must_use]
    pub fn written_as_ipv4_in_ipv6(&self) -> bool {
        self.ipv4_in_ipv6
    }

    /// Whether the address was written with a zone.
    #[must_use]
    pub fn written_with_zone(&self) -> bool {
        self.zone
    }

    /// Whether a link to this host may leave this machine. Only a loopback
    /// address written plainly as an IP address stays: not a name, whatever
    /// it resolves to today; not an IPv4 address written inside IPv6, which
    /// not every system's IPv6 sockets carry to IPv4; and not an address
    /// with a zone, whose meaning on the loopback address no standard read
    /// here gives. A plain link that carries a credential is allowed only to
    /// a host for which this is false (decision 0188).
    #[must_use]
    pub fn may_leave_this_machine(&self) -> bool {
        !(self.scope == AddressScope::ThisMachine && !self.ipv4_in_ipv6 && !self.zone)
    }

    fn of_ipv6_text(text: &str) -> Self {
        let (address, zone) = match text.split_once('%') {
            Some((address, zone)) if !zone.is_empty() => (address, true),
            Some(_) => return Self::unreadable(),
            None => (text, false),
        };
        match address.parse::<Ipv6Addr>() {
            Ok(v6) => {
                let mut host = Self::of_ip(IpAddr::V6(v6));
                host.zone = zone;
                host
            }
            Err(_) => Self::unreadable(),
        }
    }

    fn unreadable() -> Self {
        Self {
            scope: AddressScope::Unreadable,
            ip: None,
            name: None,
            ipv4_in_ipv6: false,
            zone: false,
        }
    }
}

/// A port: one to five decimal digits, at most 65535.
fn is_port(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && text.parse::<u16>().is_ok()
}

/// A host name: labels of letters, digits, `-`, and `_`, each 1 to 63
/// characters, at most 253 in all, with one final dot allowed. A name whose
/// last label is a number (`127.1`, `0x7f`) is not one: a URL parser reads it
/// as an IPv4 address in a lenient form, so it is neither a name nor an
/// address this reading takes.
fn is_host_name(host: &str) -> bool {
    let bare = host.strip_suffix('.').unwrap_or(host);
    if bare.is_empty() || bare.len() > 253 {
        return false;
    }
    let labels_fit = bare.split('.').all(|label| {
        (1..=63).contains(&label.len())
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    });
    let last = bare.rsplit('.').next().unwrap_or_default();
    let numeric = last.bytes().all(|byte| byte.is_ascii_digit())
        || last
            .strip_prefix("0x")
            .or_else(|| last.strip_prefix("0X"))
            .is_some_and(|hex| hex.bytes().all(|byte| byte.is_ascii_hexdigit()));
    labels_fit && !numeric
}

/// The scope of an IPv4 address. The ranges and their order are those of
/// `Ipv4Addr::is_global` and the methods it calls, in the Rust core
/// library's source.
fn scope_of_v4(ip: Ipv4Addr) -> AddressScope {
    let [a, b, c, d] = ip.octets();
    if ip.is_unspecified() {
        AddressScope::Unspecified
    } else if ip.is_loopback() {
        // 127.0.0.0/8, RFC 1122.
        AddressScope::ThisMachine
    } else if ip.is_private() {
        // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, RFC 1918.
        AddressScope::PrivateUse
    } else if a == 100 && (b & 0b1100_0000) == 0b0100_0000 {
        // 100.64.0.0/10, RFC 6598.
        AddressScope::CarrierGradeNat
    } else if ip.is_link_local() {
        // 169.254.0.0/16, RFC 3927.
        AddressScope::LinkLocal
    } else if ip.is_documentation() {
        // 192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24, RFC 5737.
        AddressScope::Documentation
    } else if ip.is_multicast() {
        // 224.0.0.0/4, RFC 5771.
        AddressScope::Multicast
    } else if reserved_v4(a, b, c, d) {
        AddressScope::Reserved
    } else {
        AddressScope::Public
    }
}

/// The other IPv4 ranges that are not globally reachable.
fn reserved_v4(a: u8, b: u8, c: u8, d: u8) -> bool {
    // "This network", 0.0.0.0/8.
    a == 0
        // IETF protocol assignments, 192.0.0.0/24, but for the two the
        // registry calls globally reachable.
        || (a == 192 && b == 0 && c == 0 && d != 9 && d != 10)
        // The 6a44 relay anycast address, 192.88.99.2/32.
        || (a == 192 && b == 88 && c == 99 && d == 2)
        // Benchmarking, 198.18.0.0/15: RFC 2544 and its erratum 423.
        || (a == 198 && (b & 0xfe) == 18)
        // Reserved, 240.0.0.0/4 (RFC 1112), with the limited broadcast
        // address 255.255.255.255 (RFC 919) inside it.
        || (a & 0xf0) == 0xf0
}

/// The scope of an IPv6 address that is not an IPv4 address inside IPv6.
/// The ranges are those of `Ipv6Addr::is_global` and the methods it calls,
/// in the Rust core library's source, and the IPv4-compatible range that
/// source describes as deprecated.
fn scope_of_v6(ip: Ipv6Addr) -> AddressScope {
    let s = ip.segments();
    let whole = u128::from_be_bytes(ip.octets());
    if ip.is_unspecified() {
        AddressScope::Unspecified
    } else if ip.is_loopback() {
        // ::1, RFC 4291 section 2.5.3.
        AddressScope::ThisMachine
    } else if ip.is_multicast() {
        // ff00::/8, RFC 4291 section 2.7.
        AddressScope::Multicast
    } else if ip.is_unicast_link_local() {
        // fe80::/10, RFC 4291 section 2.4.
        AddressScope::LinkLocal
    } else if ip.is_unique_local() {
        // fc00::/7, RFC 4193.
        AddressScope::UniqueLocal
    } else if (s[0] == 0x2001 && s[1] == 0xdb8) || (s[0] == 0x3fff && s[1] <= 0x0fff) {
        // 2001:db8::/32 (RFC 3849) and 3fff::/20 (RFC 9637).
        AddressScope::Documentation
    } else if reserved_v6(s, whole) {
        AddressScope::Reserved
    } else {
        AddressScope::Public
    }
}

/// The other IPv6 ranges that are not globally reachable.
fn reserved_v6(s: [u16; 8], whole: u128) -> bool {
    // IPv4-compatible, ::/96: RFC 4291 section 2.5.5.1, deprecated.
    matches!(s, [0, 0, 0, 0, 0, 0, _, _])
        // IPv4-IPv6 translation, 64:ff9b:1::/48.
        || matches!(s, [0x64, 0xff9b, 1, _, _, _, _, _])
        // Discard-only, 100::/64, and the dummy prefix, 100:0:0:1::/64.
        || matches!(s, [0x100, 0, 0, 0 | 1, _, _, _, _])
        // IETF protocol assignments, 2001::/23, benchmarking 2001:2::/48
        // (RFC 5180 and its erratum 1752) among them, but for the parts the
        // registry calls globally reachable.
        || (s[0] == 0x2001 && s[1] < 0x200 && !global_in_protocol_assignments(s, whole))
        // 6to4, 2002::/16.
        || s[0] == 0x2002
        // Segment routing, 5f00::/16.
        || s[0] == 0x5f00
}

/// The parts of 2001::/23 the registry calls globally reachable, as the
/// Rust core library's source lists them.
fn global_in_protocol_assignments(s: [u16; 8], whole: u128) -> bool {
    // Port Control Protocol, TURN, and DNS-SD service registration anycast.
    whole == 0x2001_0001_0000_0000_0000_0000_0000_0001
        || whole == 0x2001_0001_0000_0000_0000_0000_0000_0002
        || whole == 0x2001_0001_0000_0000_0000_0000_0000_0003
        // AMT, 2001:3::/32.
        || s[1] == 3
        // AS112-v6, 2001:4:112::/48.
        || (s[1] == 4 && s[2] == 0x112)
        // ORCHIDv2, 2001:20::/28, and drone remote ID tags, 2001:30::/28.
        || (0x20..=0x3f).contains(&s[1])
}

#[cfg(test)]
mod tests {
    use super::{AddressScope, ScopedHost};

    fn scope(host: &str) -> AddressScope {
        ScopedHost::of_host(host).scope()
    }

    /// Each range at its first and last address and just outside, with the
    /// standard the Rust core library's source cites for it.
    #[test]
    fn each_range_holds_exactly_its_standards_addresses() {
        use AddressScope::*;
        for (host, expected) in [
            // RFC 1122: 127.0.0.0/8.
            ("126.255.255.255", Public),
            ("127.0.0.0", ThisMachine),
            ("127.255.255.255", ThisMachine),
            ("128.0.0.0", Public),
            // RFC 4291 section 2.5.3: ::1 alone.
            ("::1", ThisMachine),
            ("::2", Reserved),
            ("0.0.0.0", Unspecified),
            ("::", Unspecified),
            // 0.0.0.0/8, "this network".
            ("0.255.255.255", Reserved),
            // RFC 1918.
            ("9.255.255.255", Public),
            ("10.0.0.0", PrivateUse),
            ("10.255.255.255", PrivateUse),
            ("11.0.0.0", Public),
            ("172.15.255.255", Public),
            ("172.16.0.0", PrivateUse),
            ("172.31.255.255", PrivateUse),
            ("172.32.0.0", Public),
            ("192.167.255.255", Public),
            ("192.168.0.0", PrivateUse),
            ("192.168.255.255", PrivateUse),
            ("192.169.0.0", Public),
            // RFC 6598: 100.64.0.0/10.
            ("100.63.255.255", Public),
            ("100.64.0.0", CarrierGradeNat),
            ("100.127.255.255", CarrierGradeNat),
            ("100.128.0.0", Public),
            // RFC 3927 and RFC 4291 section 2.4.
            ("169.253.255.255", Public),
            ("169.254.0.0", LinkLocal),
            ("169.254.255.255", LinkLocal),
            ("169.255.0.0", Public),
            ("fe7f:ffff::", Public),
            ("fe80::", LinkLocal),
            ("febf:ffff:ffff:ffff:ffff:ffff:ffff:ffff", LinkLocal),
            ("fec0::", Public),
            // RFC 4193: fc00::/7.
            ("fbff:ffff::", Public),
            ("fc00::", UniqueLocal),
            ("fdff:ffff:ffff:ffff:ffff:ffff:ffff:ffff", UniqueLocal),
            // RFC 5771 and RFC 4291 section 2.7.
            ("223.255.255.255", Public),
            ("224.0.0.0", Multicast),
            ("239.255.255.255", Multicast),
            ("feff::", Public),
            ("ff00::", Multicast),
            ("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff", Multicast),
            // RFC 5737, RFC 3849, RFC 9637.
            ("192.0.1.255", Public),
            ("192.0.2.0", Documentation),
            ("192.0.2.255", Documentation),
            ("192.0.3.0", Public),
            ("198.51.100.0", Documentation),
            ("198.51.100.255", Documentation),
            ("203.0.113.0", Documentation),
            ("203.0.113.255", Documentation),
            ("2001:db7:ffff::", Public),
            ("2001:db8::", Documentation),
            ("2001:db8:ffff:ffff:ffff:ffff:ffff:ffff", Documentation),
            ("2001:db9::", Public),
            ("3fff::", Documentation),
            ("3fff:fff:ffff::", Documentation),
            ("3fff:1000::", Public),
            // 192.0.0.0/24, but .9 and .10.
            ("192.0.0.0", Reserved),
            ("192.0.0.9", Public),
            ("192.0.0.10", Public),
            ("192.0.0.255", Reserved),
            ("192.88.99.2", Reserved),
            ("192.88.99.1", Public),
            // RFC 2544 and erratum 423: 198.18.0.0/15.
            ("198.17.255.255", Public),
            ("198.18.0.0", Reserved),
            ("198.19.255.255", Reserved),
            ("198.20.0.0", Public),
            // RFC 1112 and RFC 919.
            ("240.0.0.0", Reserved),
            ("255.255.255.255", Reserved),
            // IPv6 special-purpose ranges.
            ("::255.255.255.255", Reserved),
            ("::1:0:0:0", Public),
            ("64:ff9b:1::", Reserved),
            ("64:ff9b::7f00:1", Public),
            ("100::", Reserved),
            ("100:0:0:1:ffff::", Reserved),
            ("100:0:0:2::", Public),
            ("2001::", Reserved),
            ("2001:2::", Reserved),
            ("2001:1::1", Public),
            ("2001:1::4", Reserved),
            ("2001:3::", Public),
            ("2001:4:112::", Public),
            ("2001:4:113::", Reserved),
            ("2001:20::", Public),
            ("2001:3f::", Public),
            ("2001:1ff::", Reserved),
            ("2001:200::", Public),
            ("2002::", Reserved),
            ("5f00::", Reserved),
            ("2a00::1", Public),
        ] {
            assert_eq!(scope(host), expected, "{host}");
        }
    }

    #[test]
    fn an_ipv4_address_inside_ipv6_is_read_as_ipv4_and_never_stays_on_this_machine() {
        let mapped = ScopedHost::of_host("[::ffff:127.0.0.1]");
        assert_eq!(mapped.scope(), AddressScope::ThisMachine);
        assert!(mapped.written_as_ipv4_in_ipv6());
        assert_eq!(mapped.ip(), Some("127.0.0.1".parse().unwrap()));
        assert!(mapped.may_leave_this_machine());
        assert_eq!(scope("::ffff:10.0.0.1"), AddressScope::PrivateUse);
        assert_eq!(scope("::ffff:0.0.0.0"), AddressScope::Unspecified);
    }

    #[test]
    fn only_a_loopback_address_written_plainly_stays_on_this_machine() {
        for stays in ["127.0.0.1", "127.9.0.1", "[::1]", "::1"] {
            assert!(
                !ScopedHost::of_host(stays).may_leave_this_machine(),
                "{stays}"
            );
        }
        for leaves in [
            "localhost",
            "localhost.",
            "LOCALHOST",
            "[::ffff:127.0.0.1]",
            "[::1%1]",
            "::1%lo",
            "0.0.0.0",
            "[::]",
            "127.1",
            "10.0.0.1",
            "",
        ] {
            assert!(
                ScopedHost::of_host(leaves).may_leave_this_machine(),
                "{leaves}"
            );
        }
        let zoned: std::net::SocketAddr = "[::1%1]:80".parse().unwrap();
        let zoned = ScopedHost::of_socket_address(zoned);
        assert!(zoned.written_with_zone());
        assert!(zoned.may_leave_this_machine());
        let plain: std::net::SocketAddr = "[::1]:80".parse().unwrap();
        assert!(!ScopedHost::of_socket_address(plain).may_leave_this_machine());
    }

    #[test]
    fn a_name_is_kept_as_written_and_never_resolved() {
        let name = ScopedHost::of_host("LocalHost.");
        assert_eq!(name.scope(), AddressScope::Name);
        assert_eq!(name.name(), Some("LocalHost."));
        assert_eq!(name.ip(), None);
        for unreadable in [
            "127.1",
            "0x7f000001",
            "2130706433",
            "a..b",
            "a b",
            "[127.0.0.1]",
        ] {
            assert_eq!(scope(unreadable), AddressScope::Unreadable, "{unreadable}");
        }
    }

    #[test]
    fn a_destination_is_read_from_a_url_a_host_and_port_or_a_bare_host() {
        for (address, expected) in [
            ("https://app.example.test", AddressScope::Name),
            ("http://127.0.0.1:8080/token?x=1", AddressScope::ThisMachine),
            ("http://[::1]:9000", AddressScope::ThisMachine),
            ("http://user@127.0.0.1/", AddressScope::Unreadable),
            ("127.0.0.1:4222", AddressScope::ThisMachine),
            ("[::1]:7400", AddressScope::ThisMachine),
            ("[::1]x", AddressScope::Unreadable),
            ("192.0.2.10", AddressScope::Documentation),
            ("/srv/example/store.db", AddressScope::Unreadable),
            ("C:\\example\\store.db", AddressScope::Unreadable),
            ("127.0.0.1:65536", AddressScope::Unreadable),
        ] {
            assert_eq!(
                ScopedHost::of_destination(address).scope(),
                expected,
                "{address}"
            );
        }
    }

    #[test]
    fn every_name_is_read_back_as_its_value() {
        for scope in AddressScope::ALL {
            assert_eq!(scope.as_str().parse::<AddressScope>().unwrap(), *scope);
        }
    }
}
