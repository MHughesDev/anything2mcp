//! On Linux: each reader's answer, compared with what the system says by
//! another route the test reads itself. Nothing is assumed about the
//! machine beyond a loopback interface.
#![cfg(target_os = "linux")]

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, TcpListener, UdpSocket};
use std::path::Path;

use a2m_core::AddressScope;
use a2m_netstate::{Family, InterfaceKind, LinkState, Transport, text};

fn sys(name: &str, file: &str) -> Option<String> {
    std::fs::read_to_string(Path::new("/sys/class/net").join(name).join(file))
        .ok()
        .map(|t| t.trim().to_owned())
}

/// The IPv4 addresses the kernel's local table holds, from
/// `/proc/net/fib_trie`: each `|-- address` followed by `/32 host LOCAL`.
fn fib_trie_local() -> BTreeSet<Ipv4Addr> {
    let text = std::fs::read_to_string("/proc/net/fib_trie").unwrap_or_default();
    let mut found = BTreeSet::new();
    let mut last = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(address) = line.strip_prefix("|-- ") {
            last = address.parse::<Ipv4Addr>().ok();
        } else if line == "/32 host LOCAL"
            && let Some(address) = last
        {
            found.insert(address);
        }
    }
    found
}

#[test]
fn the_interfaces_agree_with_sys_and_the_kernels_local_table() {
    let read = a2m_netstate::interfaces().unwrap();
    let loopback: Vec<_> = read
        .interfaces
        .iter()
        .filter(|i| i.kind == Some(InterfaceKind::Loopback))
        .collect();
    assert_eq!(loopback.len(), 1, "{read:?}");
    // In this process's own namespace, `/sys/class/net` names the same
    // interfaces, unless the test runs in a namespace `/sys` does not show.
    let in_sys: BTreeSet<String> = std::fs::read_dir("/sys/class/net")
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let ours: BTreeSet<String> = read.interfaces.iter().map(|i| i.name.clone()).collect();
    assert_eq!(ours, in_sys);
    for interface in &read.interfaces {
        let name = &interface.name;
        assert_eq!(sys(name, "ifindex"), Some(interface.index.to_string()));
        assert_eq!(
            interface.mtu.map(|m| m.to_string()),
            sys(name, "mtu"),
            "{name}"
        );
        let flags =
            u32::from_str_radix(sys(name, "flags").unwrap().trim_start_matches("0x"), 16).unwrap();
        if interface.state == Some(LinkState::Up) {
            assert_ne!(flags & 1, 0, "{name} is up but /sys says down");
        }
        if flags & 1 == 0 {
            assert_eq!(interface.state, Some(LinkState::Down), "{name}");
        }
        // The octets the kernel keeps agree with /sys; an all-zero address
        // is the system saying there is none.
        let in_sys = sys(name, "address").unwrap_or_default();
        let zero = in_sys.chars().all(|c| c == '0' || c == ':');
        match &interface.hardware_address {
            Some(address) => {
                let written: Vec<String> = address
                    .octets()
                    .iter()
                    .map(|o| format!("{o:02x}"))
                    .collect();
                assert_eq!(written.join(":"), in_sys, "{name}");
            }
            None => assert!(zero || in_sys.is_empty(), "{name}"),
        }
        if Path::new("/sys/class/net")
            .join(name)
            .join("wireless")
            .exists()
        {
            assert_eq!(interface.kind, Some(InterfaceKind::Wireless));
        }
    }
    let ipv4: BTreeSet<Ipv4Addr> = read
        .interfaces
        .iter()
        .flat_map(|i| i.addresses.iter())
        .filter_map(|a| match a.address {
            IpAddr::V4(v4) => Some(v4),
            IpAddr::V6(_) => None,
        })
        .collect();
    assert_eq!(ipv4, fib_trie_local());
    // Every IPv6 address `/proc/net/if_inet6` lists is on its interface.
    if let Ok(text) = std::fs::read_to_string("/proc/net/if_inet6") {
        for line in text.lines() {
            let columns: Vec<&str> = line.split_whitespace().collect();
            let address = IpAddr::V6(text::proc_ipv6_plain(columns[0]).unwrap());
            let prefix = u8::from_str_radix(columns[2], 16).unwrap();
            let interface = read
                .interfaces
                .iter()
                .find(|i| i.name == columns[5])
                .unwrap();
            assert!(
                interface
                    .addresses
                    .iter()
                    .any(|a| a.address == address && a.prefix_length == Some(prefix)),
                "{line}"
            );
        }
    }
    // The loopback interface carries a loopback address when it is up.
    if loopback[0].state == Some(LinkState::Up) {
        assert!(
            loopback[0]
                .addresses
                .iter()
                .any(|a| a.address == IpAddr::V4(Ipv4Addr::LOCALHOST))
        );
    }
}

#[test]
fn the_ipv4_routes_are_those_of_proc_net_route() {
    let read = a2m_netstate::routes().unwrap();
    let names: BTreeSet<String> = a2m_netstate::interfaces()
        .unwrap()
        .interfaces
        .into_iter()
        .map(|i| i.name)
        .collect();
    let text = std::fs::read_to_string("/proc/net/route").unwrap();
    let mut theirs = BTreeSet::new();
    for line in text.lines().skip(1) {
        let c: Vec<&str> = line.split_whitespace().collect();
        let destination = text::proc_ipv4(c[1]).unwrap();
        let gateway = text::proc_ipv4(c[2]).unwrap();
        let flags = u16::from_str_radix(c[3], 16).unwrap();
        let metric: u32 = c[6].parse().unwrap();
        let mask = text::proc_ipv4(c[7]).unwrap();
        let prefix = text::prefix_of_mask(&mask.octets()).unwrap();
        let gateway = (flags & 0x2 != 0).then_some(IpAddr::V4(gateway));
        theirs.insert((
            c[0].to_owned(),
            IpAddr::V4(destination),
            prefix,
            gateway,
            metric,
        ));
    }
    let ours: BTreeSet<_> = read
        .routes
        .iter()
        .filter(|r| r.family == Family::Ipv4)
        .map(|r| {
            (
                r.interface.clone().unwrap(),
                r.destination,
                r.prefix_length,
                r.gateway,
                r.metric.unwrap_or(0),
            )
        })
        .collect();
    assert_eq!(ours, theirs);
    for route in &read.default_routes {
        assert!(route.is_default());
        assert!(names.contains(route.interface.as_ref().unwrap()));
    }
    let defaults_in_proc = theirs.iter().filter(|t| t.2 == 0).count();
    let ours_v4 = read
        .default_routes
        .iter()
        .filter(|r| r.family == Family::Ipv4)
        .count();
    assert_eq!(ours_v4, defaults_in_proc);
}

#[test]
fn the_ipv4_neighbours_include_every_entry_of_proc_net_arp() {
    let proc_arp = || {
        let text = std::fs::read_to_string("/proc/net/arp").unwrap();
        text.lines()
            .skip(1)
            .map(|line| {
                let c: Vec<&str> = line.split_whitespace().collect();
                (
                    c[0].parse::<IpAddr>().unwrap(),
                    c[3].to_owned(),
                    c[5].to_owned(),
                )
            })
            .collect::<BTreeSet<_>>()
    };
    // An entry may age out between two reads; an entry seen before and
    // after the reader's own read must be in it.
    let before = proc_arp();
    let read = a2m_netstate::neighbours().unwrap();
    let after = proc_arp();
    for (address, hardware, device) in before.intersection(&after) {
        let entry = read
            .neighbours
            .iter()
            .find(|n| n.address == *address && n.interface.as_deref() == Some(device))
            .unwrap_or_else(|| panic!("{address} on {device} is missing"));
        let written = entry
            .hardware_address
            .as_ref()
            .map(|h| {
                h.octets()
                    .iter()
                    .map(|o| format!("{o:02x}"))
                    .collect::<Vec<_>>()
                    .join(":")
            })
            .unwrap_or_else(|| "00:00:00:00:00:00".to_owned());
        assert_eq!(&written, hardware);
    }
}

#[test]
fn the_name_servers_are_those_of_the_file() {
    let read = a2m_netstate::resolvers().unwrap();
    match std::fs::read_to_string("/etc/resolv.conf") {
        Ok(text) => {
            let setting = read
                .settings
                .iter()
                .find(|s| s.from == "/etc/resolv.conf")
                .unwrap();
            // Read here by a plain reading of the lines, not the crate's.
            let servers: Vec<IpAddr> = text
                .lines()
                .filter_map(|l| l.trim().strip_prefix("nameserver"))
                .filter_map(|v| v.trim().split('%').next()?.parse().ok())
                .collect();
            assert_eq!(setting.name_servers, servers);
        }
        Err(_) => assert!(read.settings.iter().all(|s| s.from != "/etc/resolv.conf")),
    }
}

#[test]
fn a_socket_this_test_opened_is_listed_with_this_process_as_its_owner() {
    let tcp = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let udp = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let read = a2m_netstate::listeners().unwrap();
    let me = std::process::id();
    let program = std::fs::read_to_string("/proc/self/comm")
        .unwrap()
        .trim()
        .to_owned();
    for (transport, port) in [
        (Transport::Tcp, tcp.local_addr().unwrap().port()),
        (Transport::Udp, udp.local_addr().unwrap().port()),
    ] {
        let found = read
            .listeners
            .iter()
            .find(|l| l.transport == transport && l.port == port)
            .unwrap_or_else(|| panic!("{transport:?} {port} is missing"));
        assert_eq!(found.address, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(found.scope, AddressScope::ThisMachine);
        let owner = found.owner.as_ref().unwrap();
        assert_eq!(owner.process_id, me);
        assert_eq!(owner.program.as_deref(), Some(program.as_str()));
    }
    // Once closed, it is no longer listed.
    let port = tcp.local_addr().unwrap().port();
    drop(tcp);
    let read = a2m_netstate::listeners().unwrap();
    assert!(
        !read
            .listeners
            .iter()
            .any(|l| l.transport == Transport::Tcp && l.port == port)
    );
}

#[test]
fn the_standing_marks_loopback_as_this_machine() {
    let standing = a2m_netstate::standing().unwrap();
    let json = serde_json::to_value(&standing).unwrap();
    for interface in &standing.interfaces {
        for address in &interface.addresses {
            if address.address.is_loopback() {
                assert_eq!(address.scope, AddressScope::ThisMachine);
            }
        }
    }
    // Each preferred default route leaves through a listed interface.
    for route in &standing.default_routes {
        assert!(
            standing
                .interfaces
                .iter()
                .any(|i| i.default_route_for.contains(&route.family)),
            "{json}"
        );
    }
}
