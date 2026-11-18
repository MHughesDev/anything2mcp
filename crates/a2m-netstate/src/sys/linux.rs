//! The readers on Linux.
//!
//! What is read, and through what:
//!
//! - interfaces, addresses, routes, neighbours: the route-netlink dumps in
//!   [`super::netlink`], which answer for this process's own network
//!   namespace; and, for whether an Ethernet-framed interface is wired or
//!   wireless, the files under `/sys/class/net`, only when that folder names
//!   the same interface by the same index (it shows the namespace that
//!   mounted it, which may not be this one);
//! - name servers: `/etc/resolv.conf`, and the upstream file systemd-resolved
//!   keeps, where it exists;
//! - listening sockets: `/proc/net/tcp`, `tcp6`, `udp`, `udp6`, and each
//!   process's descriptor folder under `/proc` for the socket's owner, with
//!   the program's name from that process's `comm` file. Never a command
//!   line or an environment: those can hold secrets;
//! - the proxy: this process's environment, by the common convention.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

use a2m_core::ScopedHost;

use super::netlink::{self, RTM_GETADDR, RTM_GETLINK, RTM_GETNEIGH, RTM_GETROUTE, attributes};
use crate::bound::{self, MOST_PROCESSES, MOST_TABLE_BYTES};
use crate::error::{Fact, NetError};
use crate::model::{
    Absent, AbsentReason, Family, HardwareAddress, Interface, InterfaceAddress, InterfaceKind,
    Interfaces, LinkState, Listener, Listeners, Neighbour, NeighbourState, Neighbours, Owner,
    Proxy, ResolverSetting, Resolvers, Route, Routes, Transport,
};
use crate::text;

const AF_INET: u8 = 2;
const AF_INET6: u8 = 10;
const IFF_UP: u32 = 0x1;
const IFF_LOOPBACK: u32 = 0x8;
const IFF_RUNNING: u32 = 0x40;
const RT_TABLE_MAIN: u32 = 254;
const RTN_UNICAST: u8 = 1;
const RTM_F_CLONED: u32 = 0x200;

/// Software link kinds the kernel names that carry traffic inside other
/// traffic. Any other named kind is a software link that is not a tunnel.
const TUNNEL_KINDS: &[&str] = &[
    "tun", "wireguard", "ipip", "gre", "gretap", "ip6gre", "ip6gretap", "sit", "ip6tnl", "vti",
    "vti6", "vxlan", "geneve", "erspan", "ip6erspan", "bareudp", "gtp",
];

fn ip_of(family: u8, bytes: &[u8]) -> Option<IpAddr> {
    match (family, bytes.len()) {
        (AF_INET, 4) => Some(IpAddr::V4(Ipv4Addr::new(
            bytes[0], bytes[1], bytes[2], bytes[3],
        ))),
        (AF_INET6, 16) => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(bytes);
            Some(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        _ => None,
    }
}

/// Reads a file the system writes, up to [`MOST_TABLE_BYTES`]; whether it
/// was cut.
fn read_text(path: &Path) -> std::io::Result<(String, bool)> {
    let file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MOST_TABLE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let cut = bytes.len() > MOST_TABLE_BYTES;
    bytes.truncate(MOST_TABLE_BYTES);
    Ok((String::from_utf8_lossy(&bytes).into_owned(), cut))
}

fn sys_text(name: &str, file: &str) -> Option<String> {
    std::fs::read_to_string(Path::new("/sys/class/net").join(name).join(file))
        .ok()
        .map(|t| t.trim().to_owned())
}

/// Wired or wireless, for an Ethernet-framed interface, from `/sys`, only
/// where `/sys` names the same interface by the same index.
fn ethernet_kind(name: &str, index: u32) -> Option<InterfaceKind> {
    if name.is_empty() || name.contains('/') || name.starts_with('.') {
        return None;
    }
    if sys_text(name, "ifindex")? != index.to_string() {
        return None;
    }
    let folder = Path::new("/sys/class/net").join(name);
    if folder.join("wireless").exists() || folder.join("phy80211").exists() {
        Some(InterfaceKind::Wireless)
    } else if folder.join("device").exists() {
        Some(InterfaceKind::Wired)
    } else {
        None
    }
}

fn kind_of(
    link_type: u16,
    flags: u32,
    link_kind: Option<&str>,
    name: &str,
    index: u32,
) -> (Option<InterfaceKind>, Option<String>) {
    if flags & IFF_LOOPBACK != 0 || link_type == 772 {
        return (Some(InterfaceKind::Loopback), None);
    }
    if let Some(kind) = link_kind {
        let ours = if TUNNEL_KINDS.contains(&kind) {
            InterfaceKind::Tunnel
        } else {
            InterfaceKind::Virtual
        };
        return (Some(ours), Some(bound::text(kind)));
    }
    let system = Some(format!("arphrd {link_type}"));
    let ours = match link_type {
        1 => ethernet_kind(name, index),
        768 | 769 | 776 | 778 | 823 | 65534 => Some(InterfaceKind::Tunnel),
        801..=803 => Some(InterfaceKind::Wireless),
        _ => Some(InterfaceKind::Other),
    };
    (ours, system)
}

pub(crate) fn interfaces() -> Result<Interfaces, NetError> {
    let fact = Fact::Interfaces;
    let links = netlink::dump(RTM_GETLINK, fact)?;
    let addresses = netlink::dump(RTM_GETADDR, fact)?;
    let mut cut = Vec::new();
    let mut found: BTreeMap<u32, Interface> = BTreeMap::new();
    for message in links.messages.iter().filter(|m| m.kind == 16) {
        let body = &message.body;
        if body.len() < 16 {
            return Err(NetError::Unreadable {
                fact,
                what: "a link message shorter than its fixed part",
            });
        }
        let link_type = netlink::u16_of(&body[2..4]);
        let index = netlink::u32_of(&body[4..8]);
        let flags = netlink::u32_of(&body[8..12]);
        let mut name = None;
        let mut mtu = None;
        let mut hardware = None;
        let mut link_kind = None;
        for (kind, value) in attributes(&body[16..]) {
            match kind {
                1 => hardware = HardwareAddress::from_octets(value),
                3 => name = Some(netlink::text_of(value)),
                4 if value.len() == 4 => mtu = Some(netlink::u32_of(value)),
                18 => {
                    link_kind = attributes(value)
                        .into_iter()
                        .find(|(k, _)| *k == 1)
                        .map(|(_, v)| netlink::text_of(v));
                }
                _ => {}
            }
        }
        let Some(name) = name else {
            return Err(NetError::Unreadable {
                fact,
                what: "a link with no name",
            });
        };
        let (kind, system_kind) = kind_of(link_type, flags, link_kind.as_deref(), &name, index);
        let state = if flags & IFF_UP != 0 && flags & IFF_RUNNING != 0 {
            LinkState::Up
        } else {
            LinkState::Down
        };
        let mut absent = Vec::new();
        if kind == Some(InterfaceKind::Wireless) {
            absent.push(Absent {
                field: "wireless_network_name",
                reason: AbsentReason::NotRead,
            });
        }
        found.insert(
            index,
            Interface {
                index,
                name: bound::text(&name),
                kind,
                system_kind,
                state: Some(state),
                mtu,
                hardware_address: hardware,
                addresses: Vec::new(),
                absent,
            },
        );
    }
    for message in addresses.messages.iter().filter(|m| m.kind == 20) {
        let body = &message.body;
        if body.len() < 8 {
            return Err(NetError::Unreadable {
                fact,
                what: "an address message shorter than its fixed part",
            });
        }
        let family = body[0];
        let prefix = body[1];
        let index = netlink::u32_of(&body[4..8]);
        let mut address = None;
        let mut local = None;
        for (kind, value) in attributes(&body[8..]) {
            match kind {
                1 => address = ip_of(family, value),
                2 => local = ip_of(family, value),
                _ => {}
            }
        }
        // On a point-to-point link the kernel gives the peer as the address
        // and this machine's own as the local one.
        let Some(address) = local.or(address) else {
            continue;
        };
        if let Some(interface) = found.get_mut(&index) {
            interface.addresses.push(InterfaceAddress {
                address,
                prefix_length: Some(prefix),
            });
        }
    }
    if links.cut || addresses.cut {
        cut.push(bound::table_cut("interfaces", found.len()));
    }
    Ok(Interfaces {
        interfaces: found.into_values().collect(),
        read_from: vec!["netlink RTM_GETLINK", "netlink RTM_GETADDR", "/sys/class/net"],
        cut,
    })
}

pub(crate) fn routes() -> Result<Routes, NetError> {
    let fact = Fact::Routes;
    let dump = netlink::dump(RTM_GETROUTE, fact)?;
    let mut routes = Vec::new();
    for message in dump.messages.iter().filter(|m| m.kind == 24) {
        let body = &message.body;
        if body.len() < 12 {
            return Err(NetError::Unreadable {
                fact,
                what: "a route message shorter than its fixed part",
            });
        }
        let family = body[0];
        let prefix_length = body[1];
        let mut table = u32::from(body[4]);
        let route_type = body[7];
        let flags = netlink::u32_of(&body[8..12]);
        if family != AF_INET && family != AF_INET6 {
            continue;
        }
        let mut destination = None;
        let mut gateway = None;
        let mut interface_index = None;
        let mut metric = None;
        for (kind, value) in attributes(&body[12..]) {
            match kind {
                1 => destination = ip_of(family, value),
                4 if value.len() == 4 => interface_index = Some(netlink::u32_of(value)),
                5 => gateway = ip_of(family, value),
                6 if value.len() == 4 => metric = Some(netlink::u32_of(value)),
                15 if value.len() == 4 => table = netlink::u32_of(value),
                _ => {}
            }
        }
        if table != RT_TABLE_MAIN || route_type != RTN_UNICAST || flags & RTM_F_CLONED != 0 {
            continue;
        }
        let destination = destination.unwrap_or(if family == AF_INET {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        });
        routes.push(Route {
            family: Family::of(&destination),
            destination,
            prefix_length,
            gateway,
            interface_index,
            interface: None,
            metric,
        });
    }
    let mut cut = Vec::new();
    if dump.cut {
        cut.push(bound::table_cut("routes", routes.len()));
    }
    Ok(Routes {
        routes,
        default_routes: Vec::new(),
        read_from: vec!["netlink RTM_GETROUTE, main table, unicast routes"],
        cut,
    })
}

fn neighbour_state(state: u16) -> Option<NeighbourState> {
    Some(match state {
        0x01 => NeighbourState::Incomplete,
        0x02 => NeighbourState::Reachable,
        0x04 => NeighbourState::Stale,
        0x08 => NeighbourState::Delay,
        0x10 => NeighbourState::Probe,
        0x20 => NeighbourState::Failed,
        0x40 => NeighbourState::NoArp,
        0x80 => NeighbourState::Permanent,
        _ => return None,
    })
}

pub(crate) fn neighbours() -> Result<Neighbours, NetError> {
    let fact = Fact::Neighbours;
    let dump = netlink::dump(RTM_GETNEIGH, fact)?;
    let mut neighbours = Vec::new();
    for message in dump.messages.iter().filter(|m| m.kind == 28) {
        let body = &message.body;
        if body.len() < 12 {
            return Err(NetError::Unreadable {
                fact,
                what: "a neighbour message shorter than its fixed part",
            });
        }
        let family = body[0];
        if family != AF_INET && family != AF_INET6 {
            continue;
        }
        let interface_index = netlink::u32_of(&body[4..8]);
        let state = netlink::u16_of(&body[8..10]);
        let mut address = None;
        let mut hardware = None;
        for (kind, value) in attributes(&body[12..]) {
            match kind {
                1 => address = ip_of(family, value),
                2 => hardware = HardwareAddress::from_octets(value),
                _ => {}
            }
        }
        let Some(address) = address else { continue };
        neighbours.push(Neighbour {
            address,
            interface_index: Some(interface_index),
            interface: None,
            hardware_address: hardware,
            state: neighbour_state(state),
        });
    }
    let mut cut = Vec::new();
    if dump.cut {
        cut.push(bound::table_cut("neighbours", neighbours.len()));
    }
    Ok(Neighbours {
        neighbours,
        read_from: vec!["netlink RTM_GETNEIGH"],
        absent: Vec::new(),
        cut,
    })
}

pub(crate) fn resolvers() -> Result<Resolvers, NetError> {
    let mut settings = Vec::new();
    let mut absent = Vec::new();
    let mut read_from = Vec::new();
    for (path, name) in [
        ("/etc/resolv.conf", "/etc/resolv.conf"),
        (
            "/run/systemd/resolve/resolv.conf",
            "/run/systemd/resolve/resolv.conf",
        ),
    ] {
        match read_text(Path::new(path)) {
            Ok((text, _)) => {
                let (name_servers, search_domains) = text::resolv_conf(&text);
                read_from.push(name);
                settings.push(ResolverSetting {
                    from: path.to_owned(),
                    name_servers,
                    search_domains,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(NetError::from_io(Fact::Resolvers, &error)),
        }
    }
    if settings.is_empty() {
        absent.push(Absent {
            field: "settings",
            reason: AbsentReason::NotStated,
        });
    }
    Ok(Resolvers {
        settings,
        read_from,
        absent,
        cut: Vec::new(),
    })
}

/// The owner of each socket inode asked for, by walking the descriptor
/// folders of the processes this user may see.
fn owners(inodes: &BTreeSet<u64>) -> BTreeMap<u64, Owner> {
    let mut owners = BTreeMap::new();
    let Ok(processes) = std::fs::read_dir("/proc") else {
        return owners;
    };
    for entry in processes.flatten().take(MOST_PROCESSES) {
        let Some(process_id) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(descriptors) = std::fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        for descriptor in descriptors.flatten() {
            let Ok(target) = std::fs::read_link(descriptor.path()) else {
                continue;
            };
            let Some(inode) = target
                .to_str()
                .and_then(|t| t.strip_prefix("socket:["))
                .and_then(|t| t.strip_suffix(']'))
                .and_then(|t| t.parse::<u64>().ok())
            else {
                continue;
            };
            if inodes.contains(&inode) && !owners.contains_key(&inode) {
                let program = std::fs::read_to_string(entry.path().join("comm"))
                    .ok()
                    .map(|p| bound::text(p.trim()));
                owners.insert(
                    inode,
                    Owner {
                        process_id,
                        program,
                    },
                );
            }
        }
        if owners.len() == inodes.len() {
            break;
        }
    }
    owners
}

pub(crate) fn listeners() -> Result<Listeners, NetError> {
    let fact = Fact::Listeners;
    let mut sockets = Vec::new();
    let mut cut = Vec::new();
    for (file, transport, v6) in [
        ("/proc/net/tcp", Transport::Tcp, false),
        ("/proc/net/tcp6", Transport::Tcp, true),
        ("/proc/net/udp", Transport::Udp, false),
        ("/proc/net/udp6", Transport::Udp, true),
    ] {
        let (table, table_cut) = match read_text(Path::new(file)) {
            Ok(read) => read,
            // A system without IPv6 has no `tcp6` or `udp6`.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(NetError::from_io(fact, &error)),
        };
        // A cut table may end in half a line; only whole lines are read.
        let table = if table_cut {
            table.rsplit_once('\n').map_or("", |(whole, _)| whole)
        } else {
            &table
        };
        sockets.extend(
            text::proc_sockets(table, transport, v6).map_err(|()| NetError::Unreadable {
                fact,
                what: "a socket table line without the documented columns",
            })?,
        );
        if table_cut {
            cut = vec![bound::table_cut("listeners", sockets.len())];
        }
    }
    let inodes: BTreeSet<u64> = sockets.iter().map(|s| s.inode).collect();
    let owners = owners(&inodes);
    let listeners = sockets
        .into_iter()
        .map(|socket| {
            let owner = owners.get(&socket.inode).cloned();
            Listener {
                transport: socket.transport,
                address: socket.address,
                port: socket.port,
                scope: ScopedHost::of_ip(socket.address).scope(),
                owner_absent: owner
                    .is_none()
                    .then_some(AbsentReason::NotVisibleToThisUser),
                owner,
            }
        })
        .collect();
    Ok(Listeners {
        listeners,
        read_from: vec![
            "/proc/net/tcp",
            "/proc/net/tcp6",
            "/proc/net/udp",
            "/proc/net/udp6",
            "/proc/<pid>/fd",
            "/proc/<pid>/comm",
        ],
        cut,
    })
}

pub(crate) fn proxy() -> Result<Proxy, NetError> {
    let settings = text::proxy_from_environment(|name| std::env::var(name).ok());
    Ok(Proxy {
        settings,
        detects_automatically: None,
        read_from: vec!["this process's environment"],
        absent: vec![Absent {
            field: "desktop_settings",
            reason: AbsentReason::NotRead,
        }],
        cut: Vec::new(),
    })
}
