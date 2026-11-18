//! The readers on Windows, through the IP Helper functions and the current
//! user's proxy setting.
//!
//! What is called, and nothing else:
//!
//! - `GetAdaptersAddresses`: the interfaces, their addresses, their name
//!   servers and search domains, and their metrics;
//! - `GetIpForwardTable2`: the routes;
//! - `GetIpNetTable2`: the neighbour table, as the system holds it;
//! - `GetExtendedTcpTable` and `GetExtendedUdpTable`: the listening sockets
//!   and the process that holds each; `OpenProcess` with the right to query
//!   limited information, and `QueryFullProcessImageNameW`, for the
//!   program's file name;
//! - `WinHttpGetIEProxyConfigForCurrentUser`: the proxy setting, as it is
//!   stored for the current user. It reads a setting; it fetches no script.
//! - `FreeMibTable`, `GlobalFree`, `CloseHandle`: to give back what the
//!   system lent.
//!
//! The wireless network's name is behind the person's location permission
//! on Windows (research note 28), and is not read.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use a2m_core::ScopedHost;
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_BUFFER_OVERFLOW, ERROR_INSUFFICIENT_BUFFER,
    ERROR_NO_DATA, GlobalFree, NO_ERROR,
};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST,
    GetAdaptersAddresses, GetExtendedTcpTable, GetExtendedUdpTable, GetIpForwardTable2,
    GetIpNetTable2, IP_ADAPTER_ADDRESSES_LH, MIB_IPFORWARD_ROW2, MIB_IPFORWARD_TABLE2,
    MIB_IPNET_ROW2, MIB_IPNET_TABLE2, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER, UDP_TABLE_OWNER_PID,
};
use windows_sys::Win32::Networking::WinHttp::{
    WINHTTP_CURRENT_USER_IE_PROXY_CONFIG, WinHttpGetIEProxyConfigForCurrentUser,
};
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6, SOCKADDR_INET,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

use crate::bound::{self, MOST_INTERFACES, MOST_TABLE_BYTES};
use crate::error::{Fact, NetError};
use crate::model::{
    Absent, AbsentReason, Family, HardwareAddress, Interface, InterfaceAddress, InterfaceKind,
    Interfaces, LinkState, Listener, Listeners, Neighbour, NeighbourState, Neighbours, Owner,
    Proxy, ProxySetting, ResolverSetting, Resolvers, Route, Routes, Transport,
};
use crate::text;

/// What one adapter record says, copied out of the system's buffer.
struct Adapter {
    index: u32,
    name: String,
    if_type: u32,
    tunnel: bool,
    oper_status: i32,
    mtu: u32,
    hardware: Option<HardwareAddress>,
    addresses: Vec<InterfaceAddress>,
    name_servers: Vec<IpAddr>,
    search_domains: Vec<String>,
    ipv4_metric: u32,
    ipv6_metric: u32,
    ipv6_index: u32,
}

fn failed(fact: Fact, code: u32) -> NetError {
    if code == ERROR_ACCESS_DENIED {
        NetError::Refused { fact }
    } else {
        NetError::Failed {
            fact,
            code: i32::try_from(code).ok(),
        }
    }
}

/// A wide string the system wrote, up to its zero or [`bound::LONGEST_TEXT`]
/// characters.
///
/// # Safety
///
/// `text` is null, or points to a zero-ended wide string the system wrote
/// and still holds.
#[allow(unsafe_code)] // reads through a pointer the system gave, explained above
unsafe fn wide(text: *const u16) -> String {
    if text.is_null() {
        return String::new();
    }
    let mut length = 0usize;
    // SAFETY: the string ends in a zero within its allocation; the walk
    // stops at the zero or at the bound, whichever comes first.
    while length < bound::LONGEST_TEXT && unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: the `length` characters before the zero were just read.
    let units = unsafe { std::slice::from_raw_parts(text, length) };
    String::from_utf16_lossy(units)
}

/// An IP address from a socket address the system wrote.
///
/// # Safety
///
/// `address` is null, or points to a socket address of `length` bytes that
/// the system wrote and still holds.
#[allow(unsafe_code)] // reads through a pointer the system gave, explained above
unsafe fn ip_of_sockaddr(address: *const SOCKADDR, length: i32) -> Option<IpAddr> {
    if address.is_null() {
        return None;
    }
    let length = usize::try_from(length).ok()?;
    // SAFETY: every socket address begins with its family, two bytes.
    let family = unsafe { std::ptr::read_unaligned(address) }.sa_family;
    if family == AF_INET && length >= size_of::<SOCKADDR_IN>() {
        // SAFETY: the family and the length say it is a whole `SOCKADDR_IN`.
        let v4 = unsafe { std::ptr::read_unaligned(address.cast::<SOCKADDR_IN>()) };
        // SAFETY: every member of the address union is the same four bytes.
        let raw = unsafe { v4.sin_addr.S_un.S_addr };
        Some(IpAddr::V4(Ipv4Addr::from(raw.to_ne_bytes())))
    } else if family == AF_INET6 && length >= size_of::<SOCKADDR_IN6>() {
        // SAFETY: the family and the length say it is a whole `SOCKADDR_IN6`.
        let v6 = unsafe { std::ptr::read_unaligned(address.cast::<SOCKADDR_IN6>()) };
        // SAFETY: every member of the address union is the same sixteen bytes.
        let bytes = unsafe { v6.sin6_addr.u.Byte };
        Some(IpAddr::V6(Ipv6Addr::from(bytes)))
    } else {
        None
    }
}

/// An IP address from the union the IP Helper tables carry by value.
#[allow(unsafe_code)] // reads a union whose first member says which it holds
fn ip_of_inet(address: &SOCKADDR_INET) -> Option<IpAddr> {
    // SAFETY: both members of the union begin with the family, and the
    // union is plain data the system wrote whole.
    let family = unsafe { address.si_family };
    let pointer = std::ptr::from_ref(address).cast::<SOCKADDR>();
    // SAFETY: the pointer is to the whole union, which is as long as either
    // member.
    unsafe {
        ip_of_sockaddr(
            pointer,
            i32::try_from(size_of::<SOCKADDR_INET>()).unwrap_or(0),
        )
    }
    .filter(|_| family == AF_INET || family == AF_INET6)
}

/// Every adapter record, copied out of the system's buffer.
#[allow(unsafe_code)] // one call into the system and a walk of what it wrote, explained below
fn adapters(fact: Fact) -> Result<Vec<Adapter>, NetError> {
    let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST;
    let mut size: u32 = 16 * 1024;
    let mut buffer: Vec<u64> = Vec::new();
    let mut filled = false;
    for _ in 0..4 {
        if size as usize > MOST_TABLE_BYTES {
            return Err(NetError::Unreadable {
                fact,
                what: "an adapter list larger than the reader's bound",
            });
        }
        // Eight-byte units, so that the records the system writes are
        // aligned as their type needs.
        buffer = vec![0u64; (size as usize).div_ceil(8)];
        // SAFETY: the buffer is `size` bytes or more and aligned for the
        // records; the system writes no more than `size` bytes into it and
        // sets `size` to what it needs when that is more.
        let result = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_UNSPEC),
                flags,
                std::ptr::null(),
                buffer.as_mut_ptr().cast(),
                &raw mut size,
            )
        };
        match result {
            NO_ERROR => {
                filled = true;
                break;
            }
            ERROR_BUFFER_OVERFLOW => {}
            ERROR_NO_DATA => return Ok(Vec::new()),
            code => return Err(failed(fact, code)),
        }
    }
    if !filled {
        return Err(NetError::Unreadable {
            fact,
            what: "an adapter list that kept growing while it was read",
        });
    }
    let mut found = Vec::new();
    let mut record = buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    // Bounded, so that a malformed list cannot loop.
    while !record.is_null() && found.len() < MOST_INTERFACES * 4 {
        // SAFETY: `record` is the buffer's first record or the `Next` of a
        // record the system wrote in it; the buffer lives to the end of
        // this function.
        let adapter = unsafe { &*record };
        // SAFETY: the union's two members are plain integers.
        let index = unsafe { adapter.Anonymous1.Anonymous.IfIndex };
        let mut addresses = Vec::new();
        let mut unicast = adapter.FirstUnicastAddress;
        while !unicast.is_null() && addresses.len() < bound::MOST_ADDRESSES * 4 {
            // SAFETY: a list the system wrote in the buffer, as above.
            let entry = unsafe { &*unicast };
            // SAFETY: the system wrote the socket address and its length.
            if let Some(address) =
                unsafe { ip_of_sockaddr(entry.Address.lpSockaddr, entry.Address.iSockaddrLength) }
            {
                addresses.push(InterfaceAddress {
                    address,
                    prefix_length: Some(entry.OnLinkPrefixLength),
                });
            }
            unicast = entry.Next;
        }
        let mut name_servers = Vec::new();
        let mut server = adapter.FirstDnsServerAddress;
        while !server.is_null() && name_servers.len() < bound::MOST_RESOLVER_ENTRIES * 4 {
            // SAFETY: a list the system wrote in the buffer, as above.
            let entry = unsafe { &*server };
            // SAFETY: the system wrote the socket address and its length.
            if let Some(address) =
                unsafe { ip_of_sockaddr(entry.Address.lpSockaddr, entry.Address.iSockaddrLength) }
            {
                name_servers.push(address);
            }
            server = entry.Next;
        }
        let mut search_domains = Vec::new();
        // SAFETY: the system wrote the suffix as a zero-ended wide string.
        let suffix = unsafe { wide(adapter.DnsSuffix) };
        if !suffix.is_empty() {
            search_domains.push(suffix);
        }
        let mut more = adapter.FirstDnsSuffix;
        while !more.is_null() && search_domains.len() < bound::MOST_RESOLVER_ENTRIES * 4 {
            // SAFETY: a list the system wrote in the buffer, as above.
            let entry = unsafe { &*more };
            let end = entry
                .String
                .iter()
                .position(|u| *u == 0)
                .unwrap_or(entry.String.len());
            let domain = String::from_utf16_lossy(&entry.String[..end]);
            if !domain.is_empty() && !search_domains.contains(&domain) {
                search_domains.push(domain);
            }
            more = entry.Next;
        }
        let length = (adapter.PhysicalAddressLength as usize).min(adapter.PhysicalAddress.len());
        found.push(Adapter {
            index: if index != 0 {
                index
            } else {
                adapter.Ipv6IfIndex
            },
            // SAFETY: the system wrote the name as a zero-ended wide string.
            name: unsafe { wide(adapter.FriendlyName) },
            if_type: adapter.IfType,
            tunnel: adapter.TunnelType != 0,
            oper_status: adapter.OperStatus,
            mtu: adapter.Mtu,
            hardware: HardwareAddress::from_octets(&adapter.PhysicalAddress[..length]),
            addresses,
            name_servers,
            search_domains,
            ipv4_metric: adapter.Ipv4Metric,
            ipv6_metric: adapter.Ipv6Metric,
            ipv6_index: adapter.Ipv6IfIndex,
        });
        record = adapter.Next;
    }
    Ok(found)
}

fn kind_of(adapter: &Adapter) -> Option<InterfaceKind> {
    Some(match adapter.if_type {
        _ if adapter.tunnel => InterfaceKind::Tunnel,
        24 => InterfaceKind::Loopback,
        71 => InterfaceKind::Wireless,
        6 => InterfaceKind::Wired,
        131 => InterfaceKind::Tunnel,
        53 => InterfaceKind::Virtual,
        _ => InterfaceKind::Other,
    })
}

pub(crate) fn interfaces() -> Result<Interfaces, NetError> {
    let interfaces = adapters(Fact::Interfaces)?
        .into_iter()
        .map(|adapter| {
            let kind = kind_of(&adapter);
            let mut absent = Vec::new();
            if kind == Some(InterfaceKind::Wireless) {
                absent.push(Absent {
                    field: "wireless_network_name",
                    reason: AbsentReason::NeedsLocationPermission,
                });
            }
            Interface {
                index: adapter.index,
                name: bound::text(&adapter.name),
                kind,
                system_kind: Some(format!("if_type {}", adapter.if_type)),
                state: match adapter.oper_status {
                    1 => Some(LinkState::Up),
                    2 | 5..=7 => Some(LinkState::Down),
                    _ => None,
                },
                mtu: (adapter.mtu != u32::MAX).then_some(adapter.mtu),
                hardware_address: adapter.hardware,
                addresses: adapter.addresses,
                absent,
            }
        })
        .collect();
    Ok(Interfaces {
        interfaces,
        read_from: vec!["GetAdaptersAddresses"],
        cut: Vec::new(),
    })
}

/// The rows of a table the IP Helper lent, copied, and the table given
/// back.
///
/// # Safety
///
/// `table` is a table the system returned through one of the `Get...2`
/// functions, whose first member is the row count and whose rows follow at
/// `rows`, and it has not been given back.
#[allow(unsafe_code)] // reads a table the system lent, explained above
unsafe fn rows<Row: Copy>(
    count: u32,
    rows: *const Row,
    table: *const core::ffi::c_void,
) -> Vec<Row> {
    let count = (count as usize).min(MOST_TABLE_BYTES / size_of::<Row>().max(1));
    // SAFETY: the system wrote `count` rows after the count; the bound
    // only shortens the read.
    let copied = unsafe { std::slice::from_raw_parts(rows, count) }.to_vec();
    // SAFETY: the table came from the system and is given back once.
    unsafe { FreeMibTable(table) };
    copied
}

#[allow(unsafe_code)] // one call into the system, explained below
pub(crate) fn routes() -> Result<Routes, NetError> {
    let fact = Fact::Routes;
    let adapters = adapters(fact)?;
    let mut table: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
    // SAFETY: the system allocates the table and writes its address.
    let result = unsafe { GetIpForwardTable2(AF_UNSPEC, &raw mut table) };
    if result != NO_ERROR || table.is_null() {
        return Err(failed(fact, result));
    }
    // SAFETY: the table is the system's, not yet given back; its rows
    // follow its count.
    let read: Vec<MIB_IPFORWARD_ROW2> = unsafe {
        rows(
            (*table).NumEntries,
            std::ptr::addr_of!((*table).Table).cast(),
            table.cast(),
        )
    };
    let routes = read
        .iter()
        .filter_map(|row| {
            let destination = ip_of_inet(&row.DestinationPrefix.Prefix)?;
            let family = Family::of(&destination);
            let gateway = ip_of_inet(&row.NextHop).filter(|g| !g.is_unspecified());
            // The system chooses by the route's metric plus its interface's.
            let interface_metric = adapters
                .iter()
                .find(|a| a.index == row.InterfaceIndex || a.ipv6_index == row.InterfaceIndex)
                .map(|a| match family {
                    Family::Ipv4 => a.ipv4_metric,
                    Family::Ipv6 => a.ipv6_metric,
                });
            Some(Route {
                family,
                destination,
                prefix_length: row.DestinationPrefix.PrefixLength,
                gateway,
                interface_index: Some(row.InterfaceIndex),
                interface: None,
                metric: interface_metric.map(|m| m.saturating_add(row.Metric)),
            })
        })
        .collect();
    Ok(Routes {
        routes,
        default_routes: Vec::new(),
        read_from: vec!["GetIpForwardTable2", "GetAdaptersAddresses"],
        cut: Vec::new(),
    })
}

pub(crate) fn resolvers() -> Result<Resolvers, NetError> {
    let settings = adapters(Fact::Resolvers)?
        .into_iter()
        .filter(|a| !a.name_servers.is_empty() || !a.search_domains.is_empty())
        .map(|a| ResolverSetting {
            from: format!("interface {}", bound::text(&a.name)),
            name_servers: a.name_servers,
            search_domains: a.search_domains.iter().map(|d| bound::text(d)).collect(),
        })
        .collect();
    Ok(Resolvers {
        settings,
        read_from: vec!["GetAdaptersAddresses"],
        absent: Vec::new(),
        cut: Vec::new(),
    })
}

#[allow(unsafe_code)] // one call into the system, explained below
pub(crate) fn neighbours() -> Result<Neighbours, NetError> {
    let fact = Fact::Neighbours;
    let mut table: *mut MIB_IPNET_TABLE2 = std::ptr::null_mut();
    // SAFETY: the system allocates the table and writes its address.
    let result = unsafe { GetIpNetTable2(AF_UNSPEC, &raw mut table) };
    if result == ERROR_NO_DATA {
        return Ok(Neighbours {
            neighbours: Vec::new(),
            read_from: vec!["GetIpNetTable2"],
            absent: Vec::new(),
            cut: Vec::new(),
        });
    }
    if result != NO_ERROR || table.is_null() {
        return Err(failed(fact, result));
    }
    // SAFETY: the table is the system's, not yet given back; its rows
    // follow its count.
    let read: Vec<MIB_IPNET_ROW2> = unsafe {
        rows(
            (*table).NumEntries,
            std::ptr::addr_of!((*table).Table).cast(),
            table.cast(),
        )
    };
    let neighbours = read
        .iter()
        .filter_map(|row| {
            let address = ip_of_inet(&row.Address)?;
            let length = (row.PhysicalAddressLength as usize).min(row.PhysicalAddress.len());
            Some(Neighbour {
                address,
                interface_index: Some(row.InterfaceIndex),
                interface: None,
                hardware_address: HardwareAddress::from_octets(&row.PhysicalAddress[..length]),
                state: Some(match row.State {
                    0 => NeighbourState::Unreachable,
                    1 => NeighbourState::Incomplete,
                    2 => NeighbourState::Probe,
                    3 => NeighbourState::Delay,
                    4 => NeighbourState::Stale,
                    5 => NeighbourState::Reachable,
                    6 => NeighbourState::Permanent,
                    _ => return None,
                }),
            })
        })
        .collect();
    Ok(Neighbours {
        neighbours,
        read_from: vec!["GetIpNetTable2"],
        absent: Vec::new(),
        cut: Vec::new(),
    })
}

/// One of the two socket tables, as bytes, for one family.
#[allow(unsafe_code)] // one call into the system, explained below
fn socket_table(fact: Fact, tcp: bool, family: u16) -> Result<Vec<u64>, NetError> {
    let mut size: u32 = 16 * 1024;
    for _ in 0..4 {
        if size as usize > MOST_TABLE_BYTES {
            return Err(NetError::Unreadable {
                fact,
                what: "a socket table larger than the reader's bound",
            });
        }
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        // SAFETY: the buffer is `size` bytes or more and aligned for the
        // table; the system writes no more than `size` bytes and sets
        // `size` to what it needs when that is more.
        let result = unsafe {
            if tcp {
                GetExtendedTcpTable(
                    buffer.as_mut_ptr().cast(),
                    &raw mut size,
                    1,
                    u32::from(family),
                    TCP_TABLE_OWNER_PID_LISTENER,
                    0,
                )
            } else {
                GetExtendedUdpTable(
                    buffer.as_mut_ptr().cast(),
                    &raw mut size,
                    1,
                    u32::from(family),
                    UDP_TABLE_OWNER_PID,
                    0,
                )
            }
        };
        match result {
            NO_ERROR => return Ok(buffer),
            ERROR_INSUFFICIENT_BUFFER => {}
            code => return Err(failed(fact, code)),
        }
    }
    Err(NetError::Unreadable {
        fact,
        what: "a socket table that kept growing while it was read",
    })
}

/// The rows of a socket table: its count, then its rows.
fn table_rows<Row: Copy>(buffer: &[u64]) -> Vec<Row> {
    let bytes: Vec<u8> = buffer.iter().flat_map(|w| w.to_ne_bytes()).collect();
    let Some(count) = bytes
        .get(..4)
        .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
    else {
        return Vec::new();
    };
    // The rows begin where the table's `table` member does: after the
    // count, at the row's alignment.
    let start = size_of::<u32>().next_multiple_of(align_of::<Row>());
    let room = bytes.len().saturating_sub(start) / size_of::<Row>().max(1);
    let count = (count as usize).min(room);
    (0..count)
        .map(|i| {
            let at = start + i * size_of::<Row>();
            read_row(&bytes[at..at + size_of::<Row>()])
        })
        .collect()
}

/// One row, read from its bytes.
#[allow(unsafe_code)] // one read of plain data, explained below
fn read_row<Row: Copy>(bytes: &[u8]) -> Row {
    // SAFETY: the rows are plain integers and byte arrays, valid for any
    // bytes; the slice is exactly one row long, read without assuming its
    // alignment.
    unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<Row>()) }
}

/// A port as the socket tables give it: in network byte order, in the low
/// sixteen bits.
fn port_of(raw: u32) -> u16 {
    u16::from_be((raw & 0xffff) as u16)
}

/// The file name of a process's program, where this user may ask for it.
#[allow(unsafe_code)] // three calls into the system, explained below
fn program_of(process_id: u32) -> Option<String> {
    if process_id == 0 || process_id == 4 {
        return None;
    }
    // SAFETY: asks for a handle with the least right that names the
    // program; a null handle is a refusal.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return None;
    }
    let mut buffer = vec![0u16; 1024];
    let mut length = u32::try_from(buffer.len()).unwrap_or(0);
    // SAFETY: the buffer holds `length` wide characters; the system writes
    // no more and sets `length` to what it wrote.
    let ok = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &raw mut length,
        )
    };
    // SAFETY: the handle was opened above and is closed once.
    unsafe { CloseHandle(handle) };
    if ok == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buffer[..(length as usize).min(buffer.len())]);
    path.rsplit(['\\', '/']).next().map(bound::text)
}

pub(crate) fn listeners() -> Result<Listeners, NetError> {
    let fact = Fact::Listeners;
    let mut found: Vec<(Transport, IpAddr, u16, u32)> = Vec::new();
    for row in table_rows::<MIB_TCPROW_OWNER_PID>(&socket_table(fact, true, AF_INET)?) {
        let address = IpAddr::V4(Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes()));
        found.push((
            Transport::Tcp,
            address,
            port_of(row.dwLocalPort),
            row.dwOwningPid,
        ));
    }
    for row in table_rows::<MIB_TCP6ROW_OWNER_PID>(&socket_table(fact, true, AF_INET6)?) {
        let address = IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr));
        found.push((
            Transport::Tcp,
            address,
            port_of(row.dwLocalPort),
            row.dwOwningPid,
        ));
    }
    for row in table_rows::<MIB_UDPROW_OWNER_PID>(&socket_table(fact, false, AF_INET)?) {
        let address = IpAddr::V4(Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes()));
        found.push((
            Transport::Udp,
            address,
            port_of(row.dwLocalPort),
            row.dwOwningPid,
        ));
    }
    for row in table_rows::<MIB_UDP6ROW_OWNER_PID>(&socket_table(fact, false, AF_INET6)?) {
        let address = IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr));
        found.push((
            Transport::Udp,
            address,
            port_of(row.dwLocalPort),
            row.dwOwningPid,
        ));
    }
    let mut programs = std::collections::BTreeMap::new();
    let listeners = found
        .into_iter()
        .map(|(transport, address, port, process_id)| {
            let program = programs
                .entry(process_id)
                .or_insert_with(|| program_of(process_id))
                .clone();
            Listener {
                transport,
                address,
                port,
                scope: ScopedHost::of_ip(address).scope(),
                owner: Some(Owner {
                    process_id,
                    program,
                }),
                owner_absent: None,
            }
        })
        .collect();
    Ok(Listeners {
        listeners,
        read_from: vec![
            "GetExtendedTcpTable",
            "GetExtendedUdpTable",
            "QueryFullProcessImageNameW",
        ],
        cut: Vec::new(),
    })
}

#[allow(unsafe_code)] // one call into the system and the strings it lent, explained below
pub(crate) fn proxy() -> Result<Proxy, NetError> {
    let fact = Fact::Proxy;
    let mut config = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG {
        fAutoDetect: 0,
        lpszAutoConfigUrl: std::ptr::null_mut(),
        lpszProxy: std::ptr::null_mut(),
        lpszProxyBypass: std::ptr::null_mut(),
    };
    // SAFETY: the system fills the structure; each string it sets is one
    // it allocated for the caller to give back.
    let ok = unsafe { WinHttpGetIEProxyConfigForCurrentUser(&raw mut config) };
    if ok == 0 {
        return Err(NetError::from_io(fact, &std::io::Error::last_os_error()));
    }
    let take = |text: *mut u16| {
        // SAFETY: a zero-ended wide string the system allocated, or null.
        let value = unsafe { wide(text) };
        if !text.is_null() {
            // SAFETY: the string is the caller's to give back, once.
            unsafe { GlobalFree(text.cast()) };
        }
        value
    };
    let script = take(config.lpszAutoConfigUrl);
    let list = take(config.lpszProxy);
    let bypass = take(config.lpszProxyBypass);
    let from = "WinHttpGetIEProxyConfigForCurrentUser";
    let mut settings = text::proxy_list(from, &list);
    if !bypass.trim().is_empty() {
        settings.push(ProxySetting {
            from: from.to_owned(),
            applies_to: "bypass".to_owned(),
            value: bound::text(bypass.trim()),
            credentials_removed: false,
        });
    }
    if !script.trim().is_empty() {
        let (value, credentials_removed) = text::clean_proxy(&script);
        settings.push(ProxySetting {
            from: from.to_owned(),
            applies_to: "automatic_script".to_owned(),
            value,
            credentials_removed,
        });
    }
    Ok(Proxy {
        settings,
        detects_automatically: Some(config.fAutoDetect != 0),
        read_from: vec![from],
        absent: Vec::new(),
        cut: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_socket_table_is_read_from_its_bytes() {
        // A table of two rows, as the system lays it out.
        let rows = [
            MIB_UDPROW_OWNER_PID {
                dwLocalAddr: u32::from_ne_bytes([127, 0, 0, 1]),
                dwLocalPort: u32::from(8080u16.to_be()),
                dwOwningPid: 42,
            },
            MIB_UDPROW_OWNER_PID {
                dwLocalAddr: 0,
                dwLocalPort: u32::from(53u16.to_be()),
                dwOwningPid: 7,
            },
        ];
        let mut bytes = 2u32.to_ne_bytes().to_vec();
        for row in &rows {
            bytes.extend_from_slice(&row.dwLocalAddr.to_ne_bytes());
            bytes.extend_from_slice(&row.dwLocalPort.to_ne_bytes());
            bytes.extend_from_slice(&row.dwOwningPid.to_ne_bytes());
        }
        bytes.resize(bytes.len().next_multiple_of(8), 0);
        let words: Vec<u64> = bytes
            .chunks(8)
            .map(|c| u64::from_ne_bytes(c.try_into().unwrap()))
            .collect();
        let read = table_rows::<MIB_UDPROW_OWNER_PID>(&words);
        assert_eq!(read.len(), 2);
        assert_eq!(port_of(read[0].dwLocalPort), 8080);
        assert_eq!(read[1].dwOwningPid, 7);
    }

    #[test]
    fn a_socket_address_is_read_by_its_family() {
        let mut v4 = SOCKADDR_INET::default();
        v4.Ipv4.sin_family = AF_INET;
        v4.Ipv4.sin_addr.S_un.S_addr = u32::from_ne_bytes([192, 0, 2, 1]);
        assert_eq!(ip_of_inet(&v4), Some("192.0.2.1".parse().unwrap()));
        let mut v6 = SOCKADDR_INET::default();
        v6.Ipv6.sin6_family = AF_INET6;
        v6.Ipv6.sin6_addr.u.Byte = Ipv6Addr::LOCALHOST.octets();
        assert_eq!(ip_of_inet(&v6), Some(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert_eq!(ip_of_inet(&SOCKADDR_INET::default()), None);
    }
}
