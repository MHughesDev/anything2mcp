//! The readers on macOS.
//!
//! What is called, and nothing else:
//!
//! - `getifaddrs` and `freeifaddrs`: the interfaces, their addresses, and
//!   their link-layer records;
//! - `sysctl` on the routing socket's tables (`CTL_NET`, `PF_ROUTE`): a dump
//!   of the routes (`NET_RT_DUMP`), and of the entries flagged as holding
//!   link-layer information (`NET_RT_FLAGS` with `RTF_LLINFO`), which are
//!   the neighbour table. A `sysctl` read lists a table; it sends nothing;
//! - `/etc/resolv.conf`, which the system writes from its own resolver
//!   setting.
//!
//! Not built here yet: the listening sockets with their owners, and the
//! proxy setting the system configuration holds. Each is a named stop.

use std::collections::BTreeMap;
use std::mem::offset_of;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

use crate::bound::{self, MOST_INTERFACES, MOST_TABLE_BYTES};
use crate::error::{Fact, NetError};
use crate::model::{
    Absent, AbsentReason, Family, HardwareAddress, Interface, InterfaceAddress, InterfaceKind,
    Interfaces, LinkState, Listeners, Neighbour, Neighbours, Proxy, ResolverSetting, Resolvers,
    Route, Routes,
};
use crate::text;

const AF_INET: u8 = libc::AF_INET as u8;
const AF_INET6: u8 = libc::AF_INET6 as u8;
const AF_LINK: u8 = libc::AF_LINK as u8;

/// A socket address's bytes, as long as its own length says.
///
/// # Safety
///
/// `address` is null, or points to a socket address the system wrote and
/// still holds, whose first byte is its length.
#[allow(unsafe_code)] // reads through a pointer the system gave, explained above
unsafe fn sockaddr_bytes(address: *const libc::sockaddr) -> Option<Vec<u8>> {
    if address.is_null() {
        return None;
    }
    // SAFETY: a BSD socket address begins with its length in one byte.
    let length = usize::from(unsafe { *address.cast::<u8>() });
    if length < 2 {
        return None;
    }
    // SAFETY: the system wrote `length` bytes at the address.
    Some(unsafe { std::slice::from_raw_parts(address.cast::<u8>(), length) }.to_vec())
}

/// An IP address from a socket address's bytes. A link-local IPv6 address
/// in a routing message carries its interface in its second sixteen bits,
/// as the BSD kernels write it; that is cleared.
fn ip_of(bytes: &[u8]) -> Option<IpAddr> {
    match bytes.get(1).copied()? {
        AF_INET => {
            let b = bytes.get(4..8)?;
            Some(IpAddr::V4(Ipv4Addr::new(b[0], b[1], b[2], b[3])))
        }
        AF_INET6 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(bytes.get(8..24)?);
            if octets[0] == 0xfe && octets[1] & 0xc0 == 0x80 {
                octets[2] = 0;
                octets[3] = 0;
            }
            Some(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        _ => None,
    }
}

/// A network mask's prefix length, read in the destination's family. A mask
/// in a routing message may be shorter than a whole socket address; the
/// bytes it leaves out are zero.
fn prefix_of(mask: &[u8], family: Family) -> Option<u8> {
    let (from, to) = match family {
        Family::Ipv4 => (4, 8),
        Family::Ipv6 => (8, 24),
    };
    let mut bytes = vec![0u8; to - from];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = mask.get(from + i).copied().unwrap_or(0);
    }
    text::prefix_of_mask(&bytes)
}

/// A link-layer socket address: its interface index, its type, and its
/// hardware address.
fn link_of(bytes: &[u8]) -> Option<(u16, u8, Option<HardwareAddress>)> {
    if bytes.get(1).copied()? != AF_LINK {
        return None;
    }
    let offset = offset_of!(libc::sockaddr_dl, sdl_data);
    let index = u16::from_ne_bytes([*bytes.get(2)?, *bytes.get(3)?]);
    let kind = *bytes.get(4)?;
    let name = usize::from(*bytes.get(5)?);
    let length = usize::from(*bytes.get(6)?);
    let hardware = bytes
        .get(offset + name..offset + name + length)
        .and_then(HardwareAddress::from_octets);
    Some((index, kind, hardware))
}

/// The kind an interface's link-layer type states. An Ethernet-framed
/// interface is wired or wireless, and the type does not say which.
fn kind_of(flags: u32, link_type: u8) -> Option<InterfaceKind> {
    if flags & libc::IFF_LOOPBACK as u32 != 0 || link_type == 0x18 {
        return Some(InterfaceKind::Loopback);
    }
    match link_type {
        0x06 => None,
        0x37 | 0x39 => Some(InterfaceKind::Tunnel),
        0xd1 => Some(InterfaceKind::Virtual),
        _ => Some(InterfaceKind::Other),
    }
}

/// What `getifaddrs` gives for one entry, copied out.
struct Entry {
    name: String,
    flags: u32,
    address: Option<Vec<u8>>,
    netmask: Option<Vec<u8>>,
}

#[allow(unsafe_code)] // two calls into the system and a walk of what it lent, explained below
fn entries(fact: Fact) -> Result<Vec<Entry>, NetError> {
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: the system allocates the list and writes its head.
    if unsafe { libc::getifaddrs(&raw mut list) } != 0 {
        return Err(NetError::from_io(fact, &std::io::Error::last_os_error()));
    }
    let mut found = Vec::new();
    let mut entry = list;
    while !entry.is_null() && found.len() < MOST_INTERFACES * 64 {
        // SAFETY: `entry` is the head or a `ifa_next` of the list the system
        // lent, which is given back only below.
        let record = unsafe { &*entry };
        let name = if record.ifa_name.is_null() {
            String::new()
        } else {
            // SAFETY: the name is a zero-ended string the system wrote.
            unsafe { std::ffi::CStr::from_ptr(record.ifa_name) }
                .to_string_lossy()
                .into_owned()
        };
        found.push(Entry {
            name,
            flags: record.ifa_flags,
            // SAFETY: each is null or a socket address the system wrote.
            address: unsafe { sockaddr_bytes(record.ifa_addr) },
            // SAFETY: as above.
            netmask: unsafe { sockaddr_bytes(record.ifa_netmask) },
        });
        entry = record.ifa_next;
    }
    // SAFETY: the list came from `getifaddrs` and is given back once.
    unsafe { libc::freeifaddrs(list) };
    Ok(found)
}

pub(crate) fn interfaces() -> Result<Interfaces, NetError> {
    let mut by_name: BTreeMap<String, Interface> = BTreeMap::new();
    for entry in entries(Fact::Interfaces)? {
        let interface = by_name
            .entry(entry.name.clone())
            .or_insert_with(|| Interface {
                index: 0,
                name: bound::text(&entry.name),
                kind: None,
                system_kind: None,
                state: Some(
                    if entry.flags & libc::IFF_UP as u32 != 0
                        && entry.flags & libc::IFF_RUNNING as u32 != 0
                    {
                        LinkState::Up
                    } else {
                        LinkState::Down
                    },
                ),
                mtu: None,
                hardware_address: None,
                addresses: Vec::new(),
                absent: Vec::new(),
            });
        let Some(address) = entry.address else {
            continue;
        };
        if let Some((index, link_type, hardware)) = link_of(&address) {
            interface.index = u32::from(index);
            interface.kind = kind_of(entry.flags, link_type);
            interface.system_kind = Some(format!("ift {link_type}"));
            interface.hardware_address = hardware;
            if link_type == 0x06 {
                interface.absent.push(Absent {
                    field: "kind",
                    reason: AbsentReason::NotStated,
                });
            }
        } else if let Some(ip) = ip_of(&address) {
            let prefix_length = entry
                .netmask
                .as_deref()
                .and_then(|m| prefix_of(m, Family::of(&ip)));
            interface.addresses.push(InterfaceAddress {
                address: ip,
                prefix_length,
            });
        }
    }
    Ok(Interfaces {
        interfaces: by_name.into_values().collect(),
        read_from: vec!["getifaddrs"],
        cut: Vec::new(),
    })
}

/// One of the routing socket's tables, as bytes.
#[allow(unsafe_code)] // two calls into the system, explained below
fn route_table(fact: Fact, family: i32, operation: i32, flags: i32) -> Result<Vec<u8>, NetError> {
    let mut mib = [libc::CTL_NET, libc::PF_ROUTE, 0, family, operation, flags];
    for _ in 0..4 {
        let mut size: libc::size_t = 0;
        // SAFETY: with no buffer, the system only writes the size it needs.
        let asked = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                6,
                std::ptr::null_mut(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if asked != 0 {
            return Err(NetError::from_io(fact, &std::io::Error::last_os_error()));
        }
        if size > MOST_TABLE_BYTES {
            return Err(NetError::Unreadable {
                fact,
                what: "a routing table larger than the reader's bound",
            });
        }
        // Room for the table to grow between the two calls.
        let mut buffer = vec![0u8; size + size / 4 + 1024];
        let mut length: libc::size_t = buffer.len();
        // SAFETY: the buffer holds `length` bytes; the system writes no more
        // and sets `length` to what it wrote.
        let read = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                6,
                buffer.as_mut_ptr().cast(),
                &raw mut length,
                std::ptr::null_mut(),
                0,
            )
        };
        if read == 0 {
            buffer.truncate(length);
            return Ok(buffer);
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ENOMEM) {
            return Err(NetError::from_io(fact, &error));
        }
    }
    Err(NetError::Unreadable {
        fact,
        what: "a routing table that kept growing while it was read",
    })
}

/// One message of a routing table: its interface index, its flags, and its
/// socket addresses by their slot (destination, gateway, mask, and on).
struct RouteMessage {
    index: u16,
    flags: i32,
    addresses: [Option<Vec<u8>>; 8],
}

fn messages(fact: Fact, table: &[u8]) -> Result<Vec<RouteMessage>, NetError> {
    let header = size_of::<libc::rt_msghdr>();
    let field = |at: usize, offset: usize, width: usize| table.get(at + offset..at + offset + width);
    let mut found = Vec::new();
    let mut at = 0usize;
    while at + header <= table.len() {
        let (Some(length), Some(index), Some(flags), Some(addrs)) = (
            field(at, offset_of!(libc::rt_msghdr, rtm_msglen), 2),
            field(at, offset_of!(libc::rt_msghdr, rtm_index), 2),
            field(at, offset_of!(libc::rt_msghdr, rtm_flags), 4),
            field(at, offset_of!(libc::rt_msghdr, rtm_addrs), 4),
        ) else {
            break;
        };
        let length = usize::from(u16::from_ne_bytes([length[0], length[1]]));
        if length < header || at + length > table.len() {
            return Err(NetError::Unreadable {
                fact,
                what: "a routing message whose length does not fit its table",
            });
        }
        let index = u16::from_ne_bytes([index[0], index[1]]);
        let flags = i32::from_ne_bytes([flags[0], flags[1], flags[2], flags[3]]);
        let addrs = i32::from_ne_bytes([addrs[0], addrs[1], addrs[2], addrs[3]]);
        let body = &table[at + header..at + length];
        let mut addresses: [Option<Vec<u8>>; 8] = Default::default();
        let mut offset = 0usize;
        for (slot, address) in addresses.iter_mut().enumerate() {
            if addrs & (1 << slot) == 0 {
                continue;
            }
            let Some(own) = body.get(offset).map(|l| usize::from(*l)) else {
                break;
            };
            *address = Some(body.get(offset..offset + own).unwrap_or_default().to_vec());
            // Each address takes its own length, rounded up to four bytes;
            // an empty one takes four.
            offset += if own == 0 { 4 } else { 1 + ((own - 1) | 3) };
        }
        found.push(RouteMessage {
            index,
            flags,
            addresses,
        });
        at += length;
    }
    Ok(found)
}

pub(crate) fn routes() -> Result<Routes, NetError> {
    let fact = Fact::Routes;
    let table = route_table(fact, 0, libc::NET_RT_DUMP, 0)?;
    let mut routes = Vec::new();
    for message in messages(fact, &table)? {
        let skipped = libc::RTF_LLINFO | libc::RTF_WASCLONED | libc::RTF_IFSCOPE;
        if message.flags & skipped != 0 || message.flags & libc::RTF_UP == 0 {
            continue;
        }
        let Some(destination) = message.addresses[0].as_deref().and_then(ip_of) else {
            continue;
        };
        let family = Family::of(&destination);
        let host = match family {
            Family::Ipv4 => 32,
            Family::Ipv6 => 128,
        };
        let prefix_length = if message.flags & libc::RTF_HOST != 0 {
            Some(host)
        } else {
            message.addresses[2]
                .as_deref()
                .and_then(|m| prefix_of(m, family))
        };
        // A route with neither a mask nor the host flag has no stated
        // prefix: it is left out, not guessed.
        let Some(prefix_length) = prefix_length else {
            continue;
        };
        let gateway = if message.flags & libc::RTF_GATEWAY != 0 {
            message.addresses[1].as_deref().and_then(ip_of)
        } else {
            None
        };
        routes.push(Route {
            family,
            destination,
            prefix_length,
            gateway,
            interface_index: Some(u32::from(message.index)),
            interface: None,
            metric: None,
        });
    }
    Ok(Routes {
        routes,
        default_routes: Vec::new(),
        read_from: vec!["sysctl NET_RT_DUMP, routes not scoped to one interface"],
        cut: Vec::new(),
    })
}

pub(crate) fn neighbours() -> Result<Neighbours, NetError> {
    let fact = Fact::Neighbours;
    let mut neighbours = Vec::new();
    for family in [libc::AF_INET, libc::AF_INET6] {
        let table = route_table(fact, family, libc::NET_RT_FLAGS, libc::RTF_LLINFO)?;
        for message in messages(fact, &table)? {
            let Some(address) = message.addresses[0].as_deref().and_then(ip_of) else {
                continue;
            };
            let link = message.addresses[1].as_deref().and_then(link_of);
            neighbours.push(Neighbour {
                address,
                interface_index: Some(u32::from(
                    link.as_ref().map_or(message.index, |(index, _, _)| *index),
                )),
                interface: None,
                hardware_address: link.and_then(|(_, _, hardware)| hardware),
                state: None,
            });
        }
    }
    Ok(Neighbours {
        neighbours,
        read_from: vec!["sysctl NET_RT_FLAGS RTF_LLINFO"],
        absent: vec![Absent {
            field: "state",
            reason: AbsentReason::NotStated,
        }],
        cut: Vec::new(),
    })
}

pub(crate) fn resolvers() -> Result<Resolvers, NetError> {
    let path = "/etc/resolv.conf";
    let mut settings = Vec::new();
    match std::fs::read(Path::new(path)) {
        Ok(mut bytes) => {
            bytes.truncate(MOST_TABLE_BYTES);
            let (name_servers, search_domains) =
                text::resolv_conf(&String::from_utf8_lossy(&bytes));
            settings.push(ResolverSetting {
                from: path.to_owned(),
                name_servers,
                search_domains,
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(NetError::from_io(Fact::Resolvers, &error)),
    }
    Ok(Resolvers {
        settings,
        read_from: vec![path],
        absent: vec![Absent {
            field: "scoped_resolvers",
            reason: AbsentReason::NotRead,
        }],
        cut: Vec::new(),
    })
}

pub(crate) fn listeners() -> Result<Listeners, NetError> {
    Err(NetError::NotBuilt {
        fact: Fact::Listeners,
        system: "macos",
    })
}

pub(crate) fn proxy() -> Result<Proxy, NetError> {
    Err(NetError::NotBuilt {
        fact: Fact::Proxy,
        system: "macos",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_routing_message_is_read_by_its_slots() {
        // A destination, a gateway, and a short mask, as the kernel writes
        // them after the header.
        let mut body = Vec::new();
        body.extend_from_slice(&[16, AF_INET, 0, 0, 192, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        body.extend_from_slice(&[16, AF_INET, 0, 0, 192, 0, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
        // A mask of five bytes: 255 in the first address byte only, so /8.
        body.extend_from_slice(&[5, 0, 0, 0, 255, 0, 0, 0]);
        let header = size_of::<libc::rt_msghdr>();
        let mut table = vec![0u8; header];
        let length = u16::try_from(header + body.len()).unwrap();
        let o = offset_of!(libc::rt_msghdr, rtm_msglen);
        table[o..o + 2].copy_from_slice(&length.to_ne_bytes());
        let o = offset_of!(libc::rt_msghdr, rtm_addrs);
        table[o..o + 4].copy_from_slice(&0b111i32.to_ne_bytes());
        table.extend_from_slice(&body);
        let read = messages(Fact::Routes, &table).unwrap();
        assert_eq!(read.len(), 1);
        let slots = &read[0].addresses;
        assert_eq!(
            slots[0].as_deref().and_then(ip_of),
            Some("192.0.2.0".parse().unwrap())
        );
        assert_eq!(
            slots[1].as_deref().and_then(ip_of),
            Some("192.0.2.1".parse().unwrap())
        );
        assert_eq!(
            slots[2].as_deref().and_then(|m| prefix_of(m, Family::Ipv4)),
            Some(8)
        );
    }

    #[test]
    fn a_link_local_address_loses_the_interface_the_kernel_put_in_it() {
        let mut bytes = vec![28, AF_INET6, 0, 0, 0, 0, 0, 0];
        bytes.extend_from_slice(&[0xfe, 0x80, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(ip_of(&bytes), Some("fe80::1".parse().unwrap()));
    }
}
