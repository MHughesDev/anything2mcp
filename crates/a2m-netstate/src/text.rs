//! Readers of the text forms the systems give: the name-server file, the
//! kernel's socket tables, and a proxy setting. Pure functions, tested on
//! every system.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::bound;
use crate::model::{Absent, AbsentReason, ProxySetting, Transport};

/// The name servers and search domains a `resolv.conf` states.
///
/// The manual of that file says the `domain` and `search` keywords exclude
/// each other and the last one wins; that is the rule followed here. A
/// `nameserver` that is not an address is left out, never resolved. A zone
/// on an IPv6 server (`fe80::1%eth0`) is dropped, keeping the address.
#[must_use]
pub fn resolv_conf(text: &str) -> (Vec<IpAddr>, Vec<String>) {
    let mut servers = Vec::new();
    let mut search = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let mut words = line.split_whitespace();
        match words.next() {
            Some("nameserver") => {
                if let Some(word) = words.next() {
                    let address = word.split('%').next().unwrap_or_default();
                    if let Ok(address) = address.parse::<IpAddr>() {
                        servers.push(address);
                    }
                }
            }
            Some("search" | "domain") => {
                search = words
                    .take_while(|w| !w.starts_with('#') && !w.starts_with(';'))
                    .map(bound::text)
                    .collect();
            }
            _ => {}
        }
    }
    (servers, search)
}

/// An IPv4 address as the Linux kernel prints it in its `/proc/net` tables:
/// eight hexadecimal digits of the address's four bytes read as one number
/// in this machine's byte order.
#[must_use]
pub fn proc_ipv4(hex: &str) -> Option<Ipv4Addr> {
    if hex.len() != 8 {
        return None;
    }
    let number = u32::from_str_radix(hex, 16).ok()?;
    Some(Ipv4Addr::from(number.to_ne_bytes()))
}

/// An IPv6 address as the Linux kernel prints it in its socket tables: four
/// groups of eight hexadecimal digits, each four bytes read as one number in
/// this machine's byte order.
#[must_use]
pub fn proc_ipv6_words(hex: &str) -> Option<Ipv6Addr> {
    if hex.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (i, chunk) in bytes.chunks_mut(4).enumerate() {
        let word = u32::from_str_radix(hex.get(i * 8..i * 8 + 8)?, 16).ok()?;
        chunk.copy_from_slice(&word.to_ne_bytes());
    }
    Some(Ipv6Addr::from(bytes))
}

/// An IPv6 address as the Linux kernel prints it in its route and address
/// tables: thirty-two hexadecimal digits in network byte order.
#[must_use]
pub fn proc_ipv6_plain(hex: &str) -> Option<Ipv6Addr> {
    if hex.len() != 32 {
        return None;
    }
    let number = u128::from_str_radix(hex, 16).ok()?;
    Some(Ipv6Addr::from(number))
}

/// One socket in the kernel's table: its transport, local address, local
/// port, and inode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcSocket {
    /// TCP or UDP.
    pub transport: Transport,
    /// The local address.
    pub address: IpAddr,
    /// The local port.
    pub port: u16,
    /// The socket's inode, by which a process's descriptor names it.
    pub inode: u64,
}

/// The listening sockets in one of the kernel's tables (`/proc/net/tcp`,
/// `tcp6`, `udp`, `udp6`). For TCP, a socket in the listening state (`0A`).
/// For UDP, a socket with no peer (remote port 0), which is one that
/// receives datagrams. A line in another shape is a stop for the whole
/// table, never skipped by a guess: `None`.
#[must_use]
pub fn proc_sockets(text: &str, transport: Transport, v6: bool) -> Option<Vec<ProcSocket>> {
    let mut sockets = Vec::new();
    for line in text.lines().skip(1) {
        let columns: Vec<&str> = line.split_whitespace().collect();
        if columns.is_empty() {
            continue;
        }
        let (Some(local), Some(remote), Some(state), Some(inode)) = (
            columns.get(1),
            columns.get(2),
            columns.get(3),
            columns.get(9),
        ) else {
            return None;
        };
        let (address, port) = local.split_once(':')?;
        let (_, remote_port) = remote.split_once(':')?;
        let address = if v6 {
            IpAddr::V6(proc_ipv6_words(address)?)
        } else {
            IpAddr::V4(proc_ipv4(address)?)
        };
        let port = u16::from_str_radix(port, 16).ok()?;
        let remote_port = u16::from_str_radix(remote_port, 16).ok()?;
        let inode = inode.parse::<u64>().ok()?;
        let listening = match transport {
            Transport::Tcp => *state == "0A",
            Transport::Udp => remote_port == 0,
        };
        if listening {
            sockets.push(ProcSocket {
                transport,
                address,
                port,
                inode,
            });
        }
    }
    Some(sockets)
}

/// The prefix length a network mask's bytes state; `None` when the mask's
/// ones are not contiguous.
#[must_use]
pub fn prefix_of_mask(mask: &[u8]) -> Option<u8> {
    let mut length: u32 = 0;
    let mut ended = false;
    for byte in mask {
        if ended {
            if *byte != 0 {
                return None;
            }
            continue;
        }
        let ones = byte.leading_ones();
        if byte.checked_shl(ones).unwrap_or(0) != 0 {
            return None;
        }
        length += ones;
        if ones < 8 {
            ended = true;
        }
    }
    u8::try_from(length).ok()
}

/// The schemes a proxy value is read as starting with. Any other text
/// before `://` is read as part of the value, because in a value with no
/// scheme it may be a user name.
const PROXY_SCHEMES: &[&str] = &[
    "http", "https", "socks", "socks4", "socks4a", "socks5", "socks5h",
];

/// A proxy value with any user name and password, query, and fragment
/// removed, and whether credentials were in it. Nothing in the value is
/// resolved or reached.
///
/// Credentials are removed before any other cut: everything up to the last
/// `@` after the scheme goes, whatever the password holds (`/`, `?`, `#`,
/// `;`, a space, or another `@`). Only then are the query and the fragment
/// cut off. A value whose path holds an `@` therefore loses everything
/// before that `@`, the host included; that is the safe direction, and
/// `credentials_removed` says so.
///
/// The scheme is kept only when it is one of the proxy schemes (`http`,
/// `https`, `socks` and its versions, in any case). A value with no scheme,
/// or with another one, is read whole as `host:port`, so that a user name
/// is never mistaken for a scheme and kept.
#[must_use]
pub fn clean_proxy(value: &str) -> (String, bool) {
    let value = value.trim();
    let (scheme, rest) = match value.split_once("://") {
        Some((scheme, rest))
            if PROXY_SCHEMES
                .iter()
                .any(|known| known.eq_ignore_ascii_case(scheme)) =>
        {
            (Some(scheme), rest)
        }
        _ => (None, value),
    };
    // First, and before anything else: the credentials.
    let (rest, had_credentials) = match rest.rfind('@') {
        Some(at) => (&rest[at + 1..], true),
        None => (rest, false),
    };
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    let rest = &rest[..end];
    let cleaned = match scheme {
        Some(scheme) => format!("{scheme}://{rest}"),
        None => rest.to_owned(),
    };
    (bound::text(&cleaned), had_credentials)
}

/// The names of the environment variables that state a proxy by the common
/// convention, in the order they are reported.
pub const PROXY_VARIABLES: &[(&str, &str)] = &[
    ("http_proxy", "http"),
    ("HTTP_PROXY", "http"),
    ("https_proxy", "https"),
    ("HTTPS_PROXY", "https"),
    ("ftp_proxy", "ftp"),
    ("FTP_PROXY", "ftp"),
    ("all_proxy", "all"),
    ("ALL_PROXY", "all"),
    ("no_proxy", "bypass"),
    ("NO_PROXY", "bypass"),
];

/// The entry left out in place of a list that holds an `@`: a list is split
/// on characters a password may hold, so where a password ends cannot be
/// told, and no part of the list is kept (rule 7).
fn left_out(field: &'static str) -> Absent {
    Absent {
        field,
        reason: AbsentReason::MayHoldCredentials,
    }
}

/// The proxy settings an environment states, given a way to read a
/// variable, and what was left out. A bypass list is kept as written, cut
/// to the longest text, unless it holds an `@`: then it is left out whole,
/// named by its variable.
pub fn proxy_from_environment(
    read: impl Fn(&str) -> Option<String>,
) -> (Vec<ProxySetting>, Vec<Absent>) {
    let mut settings = Vec::new();
    let mut absent = Vec::new();
    for (name, applies_to) in PROXY_VARIABLES {
        let Some(value) = read(name) else { continue };
        if value.trim().is_empty() {
            continue;
        }
        let (value, credentials_removed) = if *applies_to == "bypass" {
            if value.contains('@') {
                absent.push(left_out(name));
                continue;
            }
            (bound::text(value.trim()), false)
        } else {
            clean_proxy(&value)
        };
        settings.push(ProxySetting {
            from: (*name).to_owned(),
            applies_to: (*applies_to).to_owned(),
            value,
            credentials_removed,
        });
    }
    (settings, absent)
}

/// A bypass list as the system states it, kept as written and cut to the
/// longest text; or, when it holds an `@`, the entry that says it was left
/// out whole, under `field`.
///
/// # Errors
///
/// The [`Absent`] entry, when the list holds an `@`.
pub fn proxy_bypass(from: &str, field: &'static str, list: &str) -> Result<ProxySetting, Absent> {
    if list.contains('@') {
        return Err(left_out(field));
    }
    Ok(ProxySetting {
        from: from.to_owned(),
        applies_to: "bypass".to_owned(),
        value: bound::text(list.trim()),
        credentials_removed: false,
    })
}

/// The entries of a proxy list in the form `scheme=host:port;host:port`,
/// each with what it applies to, credentials removed.
///
/// A list that holds an `@` anywhere gives no entries and the absent entry
/// for `list`: the list is split on `;` and space, which a password may
/// hold, so no entry could be cut from it safely.
#[must_use]
pub fn proxy_list(from: &str, list: &str) -> (Vec<ProxySetting>, Option<Absent>) {
    if list.contains('@') {
        return (Vec::new(), Some(left_out("list")));
    }
    let settings = list
        .split([';', ' '])
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (applies_to, value) = match entry.split_once('=') {
                Some((scheme, value)) => (scheme.to_owned(), value),
                None => ("all".to_owned(), entry),
            };
            let (value, credentials_removed) = clean_proxy(value);
            ProxySetting {
                from: from.to_owned(),
                applies_to,
                value,
                credentials_removed,
            }
        })
        .collect();
    (settings, None)
}

/// The DHCP server a lease file of the `KEY=value` form states, as
/// systemd-networkd and NetworkManager write it: the `SERVER_ADDRESS` line.
/// A value that is not an address gives none; nothing is resolved.
#[must_use]
pub fn dhcp_lease_server(text: &str) -> Option<IpAddr> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("SERVER_ADDRESS="))
        .find_map(|value| value.trim().trim_matches('"').parse::<IpAddr>().ok())
}

/// The DHCP server of each interface a `dhclient` lease file states: for each
/// interface, the `dhcp-server-identifier` of the last lease that names one.
/// The leases are blocks of `interface "name";` and `option ...;` lines. A
/// line that does not parse is left out.
#[must_use]
pub fn dhclient_leases(text: &str) -> Vec<(String, IpAddr)> {
    let mut found: Vec<(String, IpAddr)> = Vec::new();
    let mut interface: Option<String> = None;
    let mut server: Option<IpAddr> = None;
    for line in text.lines() {
        let line = line.trim().trim_end_matches(';').trim();
        if line.starts_with("lease ") {
            interface = None;
            server = None;
        } else if let Some(name) = line.strip_prefix("interface ") {
            interface = Some(bound::text(name.trim().trim_matches('"')));
        } else if let Some(value) = line.strip_prefix("option dhcp-server-identifier ") {
            server = value.trim().parse::<IpAddr>().ok();
        } else if line == "}"
            && let (Some(name), Some(address)) = (interface.take(), server.take())
        {
            found.retain(|(known, _)| *known != name);
            found.push((name, address));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dhcp_server_is_read_from_the_lease_forms() {
        assert_eq!(
            dhcp_lease_server("ADDRESS=192.168.1.20\nSERVER_ADDRESS=192.168.1.1\n"),
            Some("192.168.1.1".parse().unwrap())
        );
        assert_eq!(dhcp_lease_server("SERVER_ADDRESS=router\n"), None);
        assert_eq!(dhcp_lease_server(""), None);
        let leases = "lease {\n  interface \"eth0\";\n  option dhcp-server-identifier 10.0.0.1;\n}\n\
            lease {\n  interface \"eth0\";\n  option dhcp-server-identifier 10.0.0.2;\n}\n\
            lease {\n  interface \"wlan0\";\n}\n";
        assert_eq!(
            dhclient_leases(leases),
            vec![("eth0".to_owned(), "10.0.0.2".parse().unwrap())]
        );
    }

    #[test]
    fn the_name_server_file_is_read_by_its_manuals_rules() {
        let text = "# a comment\nnameserver 192.0.2.53\nnameserver fe80::1%eth0\nnameserver not-an-address\nsearch one.example two.example\ndomain three.example\noptions ndots:1\n";
        let (servers, search) = resolv_conf(text);
        assert_eq!(
            servers,
            vec![
                "192.0.2.53".parse::<IpAddr>().unwrap(),
                "fe80::1".parse().unwrap()
            ]
        );
        // The last of `search` and `domain` wins.
        assert_eq!(search, vec!["three.example".to_owned()]);
    }

    #[test]
    fn the_kernels_addresses_are_read_in_its_byte_order() {
        let loopback = u32::from_ne_bytes([127, 0, 0, 1]);
        assert_eq!(
            proc_ipv4(&format!("{loopback:08X}")),
            Some(Ipv4Addr::LOCALHOST)
        );
        assert_eq!(
            proc_ipv6_plain("fe800000000000000000000000000001"),
            Some("fe80::1".parse().unwrap())
        );
        let mut words = String::new();
        for chunk in Ipv6Addr::LOCALHOST.octets().chunks(4) {
            let word = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            words.push_str(&format!("{word:08X}"));
        }
        assert_eq!(proc_ipv6_words(&words), Some(Ipv6Addr::LOCALHOST));
        assert_eq!(proc_ipv4("zz"), None);
    }

    #[test]
    fn only_listening_sockets_are_kept_and_a_strange_line_stops() {
        let loopback = u32::from_ne_bytes([127, 0, 0, 1]);
        let any = 0u32;
        let text = format!(
            "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: {loopback:08X}:1F90 {any:08X}:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 4242 1\n   1: {loopback:08X}:1F91 {loopback:08X}:0050 01 00000000:00000000 00:00000000 00000000  1000        0 4243 1\n"
        );
        let sockets = proc_sockets(&text, Transport::Tcp, false).unwrap();
        assert_eq!(
            sockets,
            vec![ProcSocket {
                transport: Transport::Tcp,
                address: IpAddr::V4(Ipv4Addr::LOCALHOST),
                port: 8080,
                inode: 4242
            }]
        );
        assert!(proc_sockets("header\n 0: short\n", Transport::Tcp, false).is_none());
    }

    #[test]
    fn a_mask_gives_its_prefix_only_when_contiguous() {
        assert_eq!(prefix_of_mask(&[255, 255, 255, 0]), Some(24));
        assert_eq!(prefix_of_mask(&[255, 255, 240, 0]), Some(20));
        assert_eq!(prefix_of_mask(&[0, 0, 0, 0]), Some(0));
        assert_eq!(prefix_of_mask(&[255; 16]), Some(128));
        assert_eq!(prefix_of_mask(&[255, 0, 255, 0]), None);
        assert_eq!(prefix_of_mask(&[0b1010_0000]), None);
    }

    #[test]
    fn credentials_never_survive_in_a_proxy_value() {
        assert_eq!(
            clean_proxy("http://someone:secret-for-tests@proxy.example:3128/path?token=x#f"),
            ("http://proxy.example:3128/path".to_owned(), true)
        );
        assert_eq!(
            clean_proxy("proxy.example:3128"),
            ("proxy.example:3128".to_owned(), false)
        );
        let (settings, absent) = proxy_from_environment(|name| match name {
            "https_proxy" => Some("http://u:secret-for-tests@proxy.example:8080".to_owned()),
            "no_proxy" => Some("localhost,.example".to_owned()),
            _ => None,
        });
        assert_eq!(settings.len(), 2);
        assert!(absent.is_empty());
        assert_eq!(settings[0].value, "http://proxy.example:8080");
        assert!(settings[0].credentials_removed);
        assert_eq!(settings[1].applies_to, "bypass");
        let json = format!("{settings:?}");
        assert!(!json.contains("secret-for-tests"));
        let (list, absent) = proxy_list("system", "http=a.example:80;https=b.example:443");
        assert!(absent.is_none());
        assert_eq!(list[1].applies_to, "https");
        assert_eq!(list[1].value, "b.example:443");
        assert!(!list[1].credentials_removed);
    }

    /// The characters a password may hold that a cleaner could mistake for
    /// the end of one.
    const AWKWARD: &[char] = &['/', '?', '#', '@', ':', ';', ' ', '%'];

    #[test]
    fn no_byte_of_a_proxy_password_survives_cleaning_whatever_it_holds() {
        for c in AWKWARD {
            let password = format!("pa{c}ss-word-1");
            for value in [
                format!("http://user:{password}@proxy.example:3128/x?q#f"),
                format!("user:{password}@proxy.example:3128"),
            ] {
                let (settings, absent) =
                    proxy_from_environment(|name| (name == "http_proxy").then(|| value.clone()));
                assert!(absent.is_empty());
                let setting = &settings[0];
                let shown = format!("{} {setting:?}", setting.value);
                assert!(
                    !shown.contains("user") && !shown.contains("ss-word"),
                    "{c:?}: {shown}"
                );
                assert!(setting.credentials_removed, "{c:?}: {shown}");
                assert!(setting.value.contains("proxy.example:3128"), "{shown}");
            }
        }
    }

    #[test]
    fn a_list_that_holds_credentials_is_left_out_whole() {
        for list in ["http=user:pa;ss@h:1;https=h:2", "user:pa ss@h:1"] {
            let (settings, absent) = proxy_list("system", list);
            let shown = format!("{settings:?} {absent:?}");
            assert!(settings.is_empty(), "{shown}");
            assert!(!shown.contains("user") && !shown.contains("ss"), "{shown}");
            assert_eq!(
                absent,
                Some(Absent {
                    field: "list",
                    reason: AbsentReason::MayHoldCredentials
                })
            );
        }
        // A bypass list, from the environment or the system, likewise.
        let (settings, absent) = proxy_from_environment(|name| {
            (name == "no_proxy").then(|| "localhost,user:pa,ss@h".to_owned())
        });
        assert!(settings.is_empty());
        assert_eq!(absent[0].field, "no_proxy");
        assert_eq!(absent[0].reason, AbsentReason::MayHoldCredentials);
        let left_out = proxy_bypass("system", "bypass", "<local>;user:pa;ss@h").unwrap_err();
        assert!(!format!("{left_out:?}").contains("user"));
    }

    #[test]
    fn a_value_with_no_credentials_is_kept_as_it_was() {
        for (value, kept) in [
            ("http://proxy.example:3128", "http://proxy.example:3128"),
            ("http://proxy.example:3128/", "http://proxy.example:3128/"),
            ("HTTPS://proxy.example/path", "HTTPS://proxy.example/path"),
            (
                "socks5h://[2001:db8::1]:1080",
                "socks5h://[2001:db8::1]:1080",
            ),
            ("proxy.example:3128", "proxy.example:3128"),
            ("  proxy.example:3128  ", "proxy.example:3128"),
            (
                "http://proxy.example:3128/x?q#f",
                "http://proxy.example:3128/x",
            ),
            ("file://C:/proxy.pac", "file://C:/proxy.pac"),
        ] {
            assert_eq!(clean_proxy(value), (kept.to_owned(), false), "{value}");
        }
        let (list, absent) = proxy_list(
            "system",
            "http=a.example:80;https=b.example:443 c.example:1",
        );
        assert!(absent.is_none());
        let values: Vec<(&str, &str)> = list
            .iter()
            .map(|s| (s.applies_to.as_str(), s.value.as_str()))
            .collect();
        assert_eq!(
            values,
            vec![
                ("http", "a.example:80"),
                ("https", "b.example:443"),
                ("all", "c.example:1")
            ]
        );
        assert_eq!(
            proxy_bypass("system", "bypass", " <local>;*.example ")
                .unwrap()
                .value,
            "<local>;*.example"
        );
    }
}
