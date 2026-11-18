//! On Windows and macOS: the readers' answers held to invariants that hold
//! on any machine, and, where the system has a command a hosted runner
//! carries, compared with that command's own output. These run first in CI;
//! they were written on Linux and compiled for both systems there.
#![cfg(any(windows, target_os = "macos"))]

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, TcpListener, UdpSocket};
use std::process::Command;

use a2m_core::AddressScope;
use a2m_netstate::InterfaceKind;

fn command(program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("{program}: {e}"));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn loopback_is_listed_and_is_this_machine() {
    let read = a2m_netstate::interfaces().unwrap();
    let loopback: Vec<_> = read
        .interfaces
        .iter()
        .filter(|i| i.kind == Some(InterfaceKind::Loopback))
        .collect();
    assert!(!loopback.is_empty(), "{read:?}");
    assert!(
        loopback
            .iter()
            .flat_map(|i| i.addresses.iter())
            .any(|a| a.address == IpAddr::V4(Ipv4Addr::LOCALHOST)),
        "{read:?}"
    );
    for address in loopback.iter().flat_map(|i| i.addresses.iter()) {
        if address.address.is_loopback() {
            let standing = a2m_netstate::standing().unwrap();
            let scope = standing
                .interfaces
                .iter()
                .flat_map(|i| i.addresses.iter())
                .find(|a| a.address == address.address)
                .map(|a| a.scope);
            assert_eq!(scope, Some(AddressScope::ThisMachine));
        }
    }
    // No address is the unspecified one, and every name is written.
    for interface in &read.interfaces {
        assert!(!interface.name.is_empty());
        for address in &interface.addresses {
            assert!(!address.address.is_unspecified(), "{interface:?}");
        }
    }
}

#[test]
fn each_default_routes_interface_is_in_the_interface_list() {
    let interfaces = a2m_netstate::interfaces().unwrap();
    let indexes: BTreeSet<u32> = interfaces.interfaces.iter().map(|i| i.index).collect();
    let routes = a2m_netstate::routes().unwrap();
    for route in &routes.default_routes {
        assert!(route.is_default());
        let index = route.interface_index.unwrap();
        assert!(indexes.contains(&index), "{route:?} {interfaces:?}");
    }
}

#[test]
fn a_socket_this_test_opened_is_listed_with_this_process_as_its_owner() {
    let tcp = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let udp = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let read = match a2m_netstate::listeners() {
        Ok(read) => read,
        Err(a2m_netstate::NetError::NotBuilt { .. }) if cfg!(target_os = "macos") => {
            // Not built on macOS yet: a named stop, never a guess.
            let failure = a2m_core::Failure::from(a2m_netstate::listeners().unwrap_err());
            assert_eq!(failure.code().as_str(), "net_reader_not_built");
            return;
        }
        Err(error) => panic!("{error}"),
    };
    let me = std::process::id();
    let program = std::env::current_exe()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    for (transport, port) in [
        (
            a2m_netstate::Transport::Tcp,
            tcp.local_addr().unwrap().port(),
        ),
        (
            a2m_netstate::Transport::Udp,
            udp.local_addr().unwrap().port(),
        ),
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
        assert_eq!(
            owner.program.as_deref().map(str::to_lowercase),
            Some(program.to_lowercase())
        );
    }
}

/// The IPv4 neighbours `arp -a` (Windows) or `arp -an` (macOS) lists with a
/// hardware address.
fn arp_addresses() -> BTreeSet<IpAddr> {
    let mut found = BTreeSet::new();
    if cfg!(windows) {
        for line in command("arp", &["-a"]).lines() {
            let columns: Vec<&str> = line.split_whitespace().collect();
            if let [address, hardware, _kind] = columns[..]
                && hardware.contains('-')
                && let Ok(address) = address.parse::<Ipv4Addr>()
            {
                found.insert(IpAddr::V4(address));
            }
        }
    } else {
        for line in command("arp", &["-an"]).lines() {
            let Some(start) = line.find('(') else {
                continue;
            };
            let Some(end) = line.find(')') else { continue };
            if line.contains("(incomplete)") {
                continue;
            }
            if let Ok(address) = line[start + 1..end].parse::<Ipv4Addr>() {
                found.insert(IpAddr::V4(address));
            }
        }
    }
    found
}

#[test]
fn the_ipv4_neighbours_include_what_the_systems_arp_command_lists() {
    let before = arp_addresses();
    let read = a2m_netstate::neighbours().unwrap();
    let after = arp_addresses();
    let ours: BTreeSet<IpAddr> = read.neighbours.iter().map(|n| n.address).collect();
    for address in before.intersection(&after) {
        assert!(ours.contains(address), "{address} is missing: {read:?}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn the_interface_names_are_those_ifconfig_lists() {
    let listed: BTreeSet<String> = command("ifconfig", &["-l"])
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let ours: BTreeSet<String> = a2m_netstate::interfaces()
        .unwrap()
        .interfaces
        .into_iter()
        .map(|i| i.name)
        .collect();
    // `getifaddrs` lists every interface `ifconfig -l` does.
    assert!(listed.is_subset(&ours), "{listed:?} {ours:?}");
}

#[cfg(target_os = "macos")]
#[test]
fn the_ipv4_default_gateway_is_one_netstat_lists() {
    let theirs: BTreeSet<IpAddr> = command("netstat", &["-rn", "-f", "inet"])
        .lines()
        .filter(|l| l.starts_with("default"))
        .filter_map(|l| l.split_whitespace().nth(1)?.parse().ok())
        .collect();
    let routes = a2m_netstate::routes().unwrap();
    let ours: Vec<IpAddr> = routes
        .default_routes
        .iter()
        .filter(|r| r.family == a2m_netstate::Family::Ipv4)
        .filter_map(|r| r.gateway)
        .collect();
    if !theirs.is_empty() {
        assert!(
            ours.iter().any(|g| theirs.contains(g)),
            "{theirs:?} {routes:?}"
        );
    }
}

#[test]
fn the_name_servers_and_the_proxy_read_without_failing() {
    let resolvers = a2m_netstate::resolvers().unwrap();
    for setting in &resolvers.settings {
        assert!(!setting.from.is_empty());
    }
    match a2m_netstate::proxy() {
        Ok(proxy) => {
            for setting in &proxy.settings {
                assert!(!setting.value.contains('@'), "{setting:?}");
            }
        }
        Err(a2m_netstate::NetError::NotBuilt { .. }) if cfg!(target_os = "macos") => {}
        Err(error) => panic!("{error}"),
    }
}
