//! The facts each reader returns, as typed values.
//!
//! A field the system did not state is `None` or empty: never filled in by a
//! guess (rule 7). Where a field is left out on purpose, the reading names it
//! in its `absent` list with the reason.
//!
//! The serialized form is the one an action shows a caller, and it applies
//! the privacy choices of decision 0243: a hardware address is shown as its
//! length and whether it is locally administered, never its octets. The
//! typed value keeps the octets for the kernel's own use.

use std::net::IpAddr;

use a2m_core::AddressScope;
use serde::{Serialize, Serializer};

use crate::bound::Cut;

/// What kind of interface the system says this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceKind {
    /// The system's loopback interface: this machine talking to itself.
    Loopback,
    /// A physical wired interface.
    Wired,
    /// A wireless interface.
    Wireless,
    /// A software interface that is not a tunnel: a bridge, a virtual pair,
    /// a dummy, a bond.
    Virtual,
    /// A tunnel: the system says traffic on it is carried inside other
    /// traffic.
    Tunnel,
    /// The system states a kind, and it is none of the above.
    Other,
}

/// Whether the interface is up, as the system states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    /// Switched on and carrying traffic.
    Up,
    /// Switched off, or on with nothing connected.
    Down,
}

/// An address family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// IPv4.
    Ipv4,
    /// IPv6.
    Ipv6,
}

impl Family {
    /// The family of an address.
    #[must_use]
    pub fn of(address: &IpAddr) -> Self {
        match address {
            IpAddr::V4(_) => Self::Ipv4,
            IpAddr::V6(_) => Self::Ipv6,
        }
    }
}

/// A link-layer address. Personal data (research note 29): it identifies a
/// device across networks. The octets are kept for the kernel and never
/// serialized.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HardwareAddress(Vec<u8>);

impl HardwareAddress {
    /// The longest link-layer address kept, in octets.
    pub const LONGEST: usize = 32;

    /// A hardware address from its octets; `None` when there are none, when
    /// every octet is zero (the system's way of saying it has none), or when
    /// there are more than [`Self::LONGEST`].
    #[must_use]
    pub fn from_octets(octets: &[u8]) -> Option<Self> {
        if octets.is_empty() || octets.len() > Self::LONGEST || octets.iter().all(|o| *o == 0) {
            return None;
        }
        Some(Self(octets.to_vec()))
    }

    /// The octets, whole. For the kernel's own use; an action does not show
    /// them (decision 0243).
    #[must_use]
    pub fn octets(&self) -> &[u8] {
        &self.0
    }

    /// Whether the address is locally administered: the second-lowest bit of
    /// the first octet, as IEEE 802 defines it. A randomized address is one.
    #[must_use]
    pub fn locally_administered(&self) -> bool {
        self.0.first().is_some_and(|o| o & 0b10 != 0)
    }
}

impl std::fmt::Debug for HardwareAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HardwareAddress(length {})", self.0.len())
    }
}

impl Serialize for HardwareAddress {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Shown {
            length: usize,
            locally_administered: bool,
        }
        Shown {
            length: self.0.len(),
            locally_administered: self.locally_administered(),
        }
        .serialize(serializer)
    }
}

/// Why a field is left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AbsentReason {
    /// The system does not state it through the interface read.
    NotStated,
    /// The system gives it only with the person's location permission,
    /// which this reader does not ask for.
    NeedsLocationPermission,
    /// The system does not let this user see it.
    NotVisibleToThisUser,
    /// This reader did not read it: it is not built to yet, or a bound
    /// stopped the search for it.
    NotRead,
    /// A list that may hold a user name and password, in a form that cannot
    /// be split safely, so none of it is kept (rule 4, rule 7).
    MayHoldCredentials,
}

/// A field left out, and why.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Absent {
    /// The field's name, as it would appear in the output.
    pub field: &'static str,
    /// Why it is left out.
    pub reason: AbsentReason,
}

/// One address on an interface.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct InterfaceAddress {
    /// The address. An IPv6 address keeps no zone: the interface is its zone.
    pub address: IpAddr,
    /// The length of its network prefix, where the system states one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix_length: Option<u8>,
}

/// One network interface of this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Interface {
    /// The system's index for the interface.
    pub index: u32,
    /// The interface's name as the system gives it.
    pub name: String,
    /// What kind of interface the system says it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<InterfaceKind>,
    /// The system's own word for the kind, where it has one, such as a
    /// software link's kind or a numbered interface type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_kind: Option<String>,
    /// Whether it is up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<LinkState>,
    /// The largest packet it carries, in bytes, where stated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtu: Option<u32>,
    /// Its link-layer address; serialized without its octets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware_address: Option<HardwareAddress>,
    /// Its addresses, in order.
    pub addresses: Vec<InterfaceAddress>,
    /// Fields left out, and why.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub absent: Vec<Absent>,
}

/// The interfaces, and what was cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Interfaces {
    /// The interfaces, by index.
    pub interfaces: Vec<Interface>,
    /// Which interfaces of the system were read.
    pub read_from: Vec<&'static str>,
    /// What a bound cut.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cut: Vec<Cut>,
}

/// One route.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Route {
    /// The address family.
    pub family: Family,
    /// The destination network.
    pub destination: IpAddr,
    /// The destination's prefix length: 0 for a default route.
    pub prefix_length: u8,
    /// The next hop, where the route goes through one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<IpAddr>,
    /// The interface's index, where stated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_index: Option<u32>,
    /// The interface's name, where the index names one in the interface
    /// list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<String>,
    /// The route's metric, where stated. Lower is preferred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric: Option<u32>,
}

impl Route {
    /// Whether this is a default route.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.prefix_length == 0
    }
}

/// The routes, the default routes for each family, and what was cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Routes {
    /// Every route read, in order.
    pub routes: Vec<Route>,
    /// The default routes, IPv4 first, lowest metric first in each family.
    /// Where the system holds several, all are listed; which it chooses
    /// among equal metrics is not stated.
    pub default_routes: Vec<Route>,
    /// Which interfaces of the system were read.
    pub read_from: Vec<&'static str>,
    /// What a bound cut.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cut: Vec<Cut>,
}

/// One place the system states name servers and search domains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolverSetting {
    /// Where it was read: a file's path, or an interface's name.
    pub from: String,
    /// The name servers, in the order the system lists them.
    pub name_servers: Vec<IpAddr>,
    /// The search domains, in the order the system lists them.
    pub search_domains: Vec<String>,
}

/// The name servers and search domains, and what was cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resolvers {
    /// Each place a setting was read.
    pub settings: Vec<ResolverSetting>,
    /// Which interfaces of the system were read.
    pub read_from: Vec<&'static str>,
    /// What was left out, and why.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub absent: Vec<Absent>,
    /// What a bound cut.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cut: Vec<Cut>,
}

/// The state the system gives a neighbour entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NeighbourState {
    /// Being resolved; no link-layer address yet.
    Incomplete,
    /// Recently confirmed.
    Reachable,
    /// Known, not recently confirmed.
    Stale,
    /// Waiting before the system checks it again.
    Delay,
    /// Being checked again by the system.
    Probe,
    /// The system's check failed.
    Failed,
    /// Needs no link-layer resolution.
    NoArp,
    /// Set by hand or by the system, never expiring.
    Permanent,
    /// The system says the entry is unreachable.
    Unreachable,
}

/// One entry of the system's neighbour table.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Neighbour {
    /// The neighbour's address.
    pub address: IpAddr,
    /// The interface's index, where stated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_index: Option<u32>,
    /// The interface's name, where the index names one in the interface
    /// list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<String>,
    /// The neighbour's link-layer address; serialized without its octets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware_address: Option<HardwareAddress>,
    /// The entry's state, where the system states one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<NeighbourState>,
}

/// The neighbour table, and what was cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Neighbours {
    /// The entries, in order.
    pub neighbours: Vec<Neighbour>,
    /// Which interfaces of the system were read.
    pub read_from: Vec<&'static str>,
    /// What was left out, and why.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub absent: Vec<Absent>,
    /// What a bound cut.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cut: Vec<Cut>,
}

/// A transport protocol of a listening socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// TCP: a socket in the listening state.
    Tcp,
    /// UDP: a bound socket with no peer, which receives datagrams.
    Udp,
}

/// The process that holds a socket.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Owner {
    /// The process's identifier.
    pub process_id: u32,
    /// The program's name, without its folder and without its arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
}

/// One socket listening on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Listener {
    /// TCP or UDP.
    pub transport: Transport,
    /// The local address it is bound to.
    pub address: IpAddr,
    /// The local port.
    pub port: u16,
    /// What kind of place the bound address is.
    pub scope: AddressScope,
    /// The process that holds it, where this user may see it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<Owner>,
    /// Why the owner is left out, when it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_absent: Option<AbsentReason>,
}

/// The listening sockets, and what was cut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listeners {
    /// The sockets, in order.
    pub listeners: Vec<Listener>,
    /// Which interfaces of the system were read.
    pub read_from: Vec<&'static str>,
    /// What a bound cut.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cut: Vec<Cut>,
}

/// One proxy setting, as the system states it. Read to be shown, never
/// applied (decision 0197). Any user name and password in it are removed
/// before it is kept (rule 4), and so are a path's query and fragment.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ProxySetting {
    /// Where it was read: an environment variable's name, or the system's
    /// setting's name.
    pub from: String,
    /// What it is for: a scheme, `all`, `bypass`, or `automatic_script`.
    pub applies_to: String,
    /// The value, with credentials, query, and fragment removed.
    pub value: String,
    /// Whether credentials were removed from it.
    pub credentials_removed: bool,
}

/// The system's proxy setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Proxy {
    /// Each setting found, in order.
    pub settings: Vec<ProxySetting>,
    /// Whether the system says it finds a proxy by itself, where it says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detects_automatically: Option<bool>,
    /// Which interfaces of the system were read.
    pub read_from: Vec<&'static str>,
    /// What was left out, and why.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub absent: Vec<Absent>,
    /// What a bound cut.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cut: Vec<Cut>,
}

/// One address of an interface, and what kind of place it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AddressStanding {
    /// The address.
    pub address: IpAddr,
    /// Its prefix length, where stated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix_length: Option<u8>,
    /// What kind of place its literal says it is (decision 0240).
    pub scope: AddressScope,
}

/// Where one interface stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InterfaceStanding {
    /// The interface's name.
    pub name: String,
    /// Its kind, where stated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<InterfaceKind>,
    /// Whether it is up, where stated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<LinkState>,
    /// The families whose preferred default route leaves through it.
    pub default_route_for: Vec<Family>,
    /// Its addresses, each with its kind of place.
    pub addresses: Vec<AddressStanding>,
}

/// Where the machine stands: for each interface, the kind of place each of
/// its addresses is, and which carries the default routes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Standing {
    /// The interfaces, by index.
    pub interfaces: Vec<InterfaceStanding>,
    /// The preferred default route of each family that has one.
    pub default_routes: Vec<Route>,
    /// What a bound cut, in either reading.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cut: Vec<Cut>,
}
