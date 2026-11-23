//! The fingerprint of the network this machine is on, and when two
//! fingerprints are of the same network (decision 0223, package N3,
//! hypothesis H-44).
//!
//! A fingerprint is built from facts the machine already holds, read as an
//! ordinary user, with nothing sent. The **core** is the default gateway's
//! hardware address, the default gateway's address, and the subnet with its
//! prefix. The **secondary** facts are the DNS search domains and the DHCP
//! server that leased the address. It is kept as one digest per fact, never
//! the facts themselves, so a stored fingerprint shows no address, and so
//! each fact can be compared by itself. The wireless network's name and
//! access point are not part of it, and neither is any identifier the
//! operating system gives a network, nor this machine's own hardware address
//! (research note 32).
//!
//! The fingerprint is of one address family: the family of the preferred
//! default route, IPv4 first. A network reached by IPv6 alone is read by its
//! router advertisement's link-local gateway, and its subnet is the stable
//! prefix.
//!
//! **Recognition recalls; it never authorizes.** Every one of these facts can
//! be forged by anyone who runs the network the machine joins (research note
//! 32). A fingerprint decides only which saved reach, if any, applies. It
//! never raises what may be looked at, never sends, and never names a network
//! by itself: only a person does that (section 502).
//!
//! **The comparison** ([`NetworkFingerprint::recognize`]):
//!
//! - All three core facts must be stated by both sides, or the answer is
//!   [`Recognition::Insufficient`], which is not recognized. A gateway whose
//!   hardware address the machine's neighbour table does not hold is such a
//!   case, and the product sends no request to fill it (rule 7).
//! - Every core fact must be equal. A core fact that differs is
//!   [`Recognition::Different`].
//! - A secondary fact is compared only when both sides state it. The DNS
//!   domains agree when they share one domain. A secondary fact that
//!   disagrees is [`Recognition::Different`]; one missing on a side is no
//!   evidence either way.
//!
//! What may change and still be the same network: this machine's own
//! address, the lease's times, the name servers, the interface that carries
//! it, a secondary fact that one side does not state, and a DNS domain added
//! or dropped while another remains. What may not: the gateway's hardware
//! address or address, the subnet, or a secondary fact both sides state.
//!
//! **A tunnel is no network.** When the default route leaves through a
//! tunnel, the fingerprint is empty: a reach named on a home network is not
//! carried across a virtual private network to another place.
//!
//! **What recognition cannot tell.** Two networks of one default range that
//! share a gateway's virtual hardware address read as one when neither has a
//! secondary fact that differs. That false match is the risk H-44 measures.
//!
//! A digest of a hardware address or an address in a small range can be
//! found by trying every value. The digest keeps a stored fingerprint from
//! being read at a glance; the store is protected by the operating system,
//! and that protection is what keeps it from a caller.

use std::collections::BTreeSet;
use std::net::IpAddr;

use a2m_core::{Digest, FieldHasher};
use serde::{Deserialize, Serialize};

use crate::dhcp::DhcpServers;
use crate::error::NetError;
use crate::model::{
    Absent, AbsentReason, Family, InterfaceKind, Interfaces, LinkState, NeighbourState, Neighbours,
    Resolvers, Routes,
};

/// The fact a fingerprint holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FingerprintFact {
    /// The hardware address of the default gateway.
    GatewayHardware,
    /// The default gateway's address.
    GatewayAddress,
    /// The address of the DHCP server that leased this machine's address.
    DhcpServer,
    /// The subnet the machine's address is in.
    Subnet,
    /// The DNS search domains.
    DnsDomain,
}

impl FingerprintFact {
    /// Every fact, in the order they are compared.
    pub const ALL: &'static [Self] = &[
        Self::GatewayHardware,
        Self::GatewayAddress,
        Self::Subnet,
        Self::DhcpServer,
        Self::DnsDomain,
    ];

    /// The fact's name in the documents and in messages.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GatewayHardware => "gateway_hardware",
            Self::GatewayAddress => "gateway_address",
            Self::DhcpServer => "dhcp_server",
            Self::Subnet => "subnet",
            Self::DnsDomain => "dns_domain",
        }
    }

    /// Whether the fact is core: stated by both sides and equal, or the
    /// network is not recognized.
    #[must_use]
    pub const fn is_core(self) -> bool {
        matches!(
            self,
            Self::GatewayHardware | Self::GatewayAddress | Self::Subnet
        )
    }
}

/// The answer to whether two fingerprints are of one network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Recognition {
    /// The same network: every core fact is stated and equal, and no
    /// secondary fact that both sides state differs.
    Same,
    /// A different network: this fact is stated by both and disagrees.
    Different(FingerprintFact),
    /// A core fact is not stated by one side, so nothing can be said. Not
    /// recognized: a caller treats it as a network it has never seen.
    Insufficient,
}

impl Recognition {
    /// Whether the networks are recognized as the same.
    #[must_use]
    pub fn is_same(self) -> bool {
        self == Self::Same
    }
}

/// The facts that identify a network, each as a digest. Serializable, so that
/// a reach can be stored against it; it holds no address.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NetworkFingerprint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gateway_hardware: Option<Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gateway_address: Option<Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dhcp_server: Option<Digest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    subnet: Option<Digest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dns_domains: Vec<Digest>,
}

fn digest(fact: FingerprintFact, value: &[u8]) -> Digest {
    let mut hasher = FieldHasher::new();
    hasher.field("purpose", b"a2m network fingerprint v1");
    hasher.field(fact.as_str(), value);
    hasher.finish()
}

/// An address with all but its first `prefix` bits cleared, as bytes.
fn masked(address: IpAddr, prefix: u8) -> Vec<u8> {
    let mut octets = match address {
        IpAddr::V4(v4) => v4.octets().to_vec(),
        IpAddr::V6(v6) => v6.octets().to_vec(),
    };
    let bits = usize::from(prefix).min(octets.len() * 8);
    for (index, octet) in octets.iter_mut().enumerate() {
        let from = index * 8;
        if from >= bits {
            *octet = 0;
        } else if bits - from < 8 {
            *octet &= 0xffu8 << (8 - (bits - from));
        }
    }
    octets
}

fn same_subnet(a: IpAddr, b: IpAddr, prefix: u8) -> bool {
    prefix > 0 && a.is_ipv4() == b.is_ipv4() && masked(a, prefix) == masked(b, prefix)
}

/// A search domain as compared: lower case, with no leading or trailing dot.
fn normal_domain(domain: &str) -> Option<String> {
    let domain = domain.trim().trim_matches('.').to_ascii_lowercase();
    (!domain.is_empty()).then_some(domain)
}

impl NetworkFingerprint {
    /// A fingerprint that states nothing. It is recognized as no network.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Whether it states no fact at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.gateway_hardware.is_none()
            && self.gateway_address.is_none()
            && self.dhcp_server.is_none()
            && self.subnet.is_none()
            && self.dns_domains.is_empty()
    }

    /// The facts it states, in the order they are compared.
    #[must_use]
    pub fn stated(&self) -> Vec<FingerprintFact> {
        FingerprintFact::ALL
            .iter()
            .copied()
            .filter(|fact| match fact {
                FingerprintFact::GatewayHardware => self.gateway_hardware.is_some(),
                FingerprintFact::GatewayAddress => self.gateway_address.is_some(),
                FingerprintFact::DhcpServer => self.dhcp_server.is_some(),
                FingerprintFact::Subnet => self.subnet.is_some(),
                FingerprintFact::DnsDomain => !self.dns_domains.is_empty(),
            })
            .collect()
    }

    /// A fingerprint from given facts. The facts a reading gives, and what
    /// a test states directly.
    #[must_use]
    pub fn of_facts(
        gateway_hardware: Option<&[u8]>,
        gateway_address: Option<IpAddr>,
        dhcp_server: Option<IpAddr>,
        subnet: Option<(IpAddr, u8)>,
        dns_domains: &[&str],
    ) -> Self {
        let domains: BTreeSet<Digest> = dns_domains
            .iter()
            .filter_map(|d| normal_domain(d))
            .map(|d| digest(FingerprintFact::DnsDomain, d.as_bytes()))
            .collect();
        Self {
            gateway_hardware: gateway_hardware
                .filter(|octets| octets.iter().any(|o| *o != 0))
                .map(|octets| digest(FingerprintFact::GatewayHardware, octets)),
            gateway_address: gateway_address.map(|address| {
                digest(
                    FingerprintFact::GatewayAddress,
                    address.to_string().as_bytes(),
                )
            }),
            dhcp_server: dhcp_server
                .map(|address| digest(FingerprintFact::DhcpServer, address.to_string().as_bytes())),
            subnet: subnet
                .filter(|(_, prefix)| *prefix > 0)
                .map(|(address, prefix)| {
                    let mut bytes = vec![u8::from(address.is_ipv6()), prefix];
                    bytes.extend(masked(address, prefix));
                    digest(FingerprintFact::Subnet, &bytes)
                }),
            dns_domains: domains.into_iter().collect(),
        }
    }

    /// Whether `self` and `other` are of one network, by the rules of the
    /// module's documentation. Symmetric.
    #[must_use]
    pub fn recognize(&self, other: &Self) -> Recognition {
        let mut missing = false;
        for fact in FingerprintFact::ALL.iter().copied() {
            let both = match fact {
                FingerprintFact::GatewayHardware => pair(
                    self.gateway_hardware.as_ref(),
                    other.gateway_hardware.as_ref(),
                ),
                FingerprintFact::GatewayAddress => pair(
                    self.gateway_address.as_ref(),
                    other.gateway_address.as_ref(),
                ),
                FingerprintFact::DhcpServer => {
                    pair(self.dhcp_server.as_ref(), other.dhcp_server.as_ref())
                }
                FingerprintFact::Subnet => pair(self.subnet.as_ref(), other.subnet.as_ref()),
                FingerprintFact::DnsDomain => {
                    if self.dns_domains.is_empty() || other.dns_domains.is_empty() {
                        None
                    } else {
                        Some(
                            self.dns_domains
                                .iter()
                                .any(|d| other.dns_domains.contains(d)),
                        )
                    }
                }
            };
            match both {
                Some(false) => return Recognition::Different(fact),
                None if fact.is_core() => missing = true,
                _ => {}
            }
        }
        if missing {
            Recognition::Insufficient
        } else {
            Recognition::Same
        }
    }
}

fn pair(a: Option<&Digest>, b: Option<&Digest>) -> Option<bool> {
    Some(a? == b?)
}

/// Everything a fingerprint is read from. Each part that may be refused or
/// not built is a result, so that a fingerprint says which fact it lacks and
/// why.
#[derive(Debug)]
pub struct Readings {
    /// The interfaces.
    pub interfaces: Interfaces,
    /// The routes.
    pub routes: Routes,
    /// The neighbour table.
    pub neighbours: Result<Neighbours, NetError>,
    /// The name servers and search domains.
    pub resolvers: Result<Resolvers, NetError>,
    /// The DHCP servers.
    pub dhcp: Result<DhcpServers, NetError>,
}

/// A fingerprint, the interface that carries it, and the facts it lacks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FingerprintReading {
    /// The fingerprint.
    pub fingerprint: NetworkFingerprint,
    /// The name of the interface the default route leaves through.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<String>,
    /// The facts left out, and why.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub absent: Vec<Absent>,
}

fn why<T>(read: &Result<T, NetError>) -> AbsentReason {
    match read {
        Err(NetError::Refused { .. }) => AbsentReason::NotVisibleToThisUser,
        _ => AbsentReason::NotRead,
    }
}

impl FingerprintReading {
    /// The fingerprint of the network the default route leaves through, from
    /// readings already made. Nothing is read here and nothing is guessed: a
    /// fact the readings do not state is left out and named.
    #[must_use]
    pub fn of(readings: &Readings) -> Self {
        let mut absent = Vec::new();
        let none = |absent: Vec<Absent>, interface: Option<String>| Self {
            fingerprint: NetworkFingerprint::empty(),
            interface,
            absent,
        };
        // The preferred default route whose interface is up. The routes list
        // IPv4 first, lowest metric first.
        let carried = readings.routes.default_routes.iter().find_map(|route| {
            let interface = readings
                .interfaces
                .interfaces
                .iter()
                .find(|i| Some(i.index) == route.interface_index)?;
            (interface.state == Some(LinkState::Up)).then_some((route, interface))
        });
        let Some((route, interface)) = carried else {
            absent.push(Absent {
                field: "default_route",
                reason: AbsentReason::NotStated,
            });
            return none(absent, None);
        };
        if interface.kind == Some(InterfaceKind::Tunnel) {
            absent.push(Absent {
                field: "network_through_tunnel",
                reason: AbsentReason::NotRead,
            });
            return none(absent, Some(interface.name.clone()));
        }
        let family = route.family;

        let hardware = match (&readings.neighbours, route.gateway) {
            (Ok(read), Some(gateway)) => read
                .neighbours
                .iter()
                .filter(|n| n.address == gateway)
                .filter(|n| n.interface_index.is_none_or(|i| i == interface.index))
                .filter(|n| {
                    !matches!(
                        n.state,
                        Some(
                            NeighbourState::Failed
                                | NeighbourState::Incomplete
                                | NeighbourState::Unreachable
                        )
                    )
                })
                .find_map(|n| n.hardware_address.as_ref())
                .map(|h| h.octets().to_vec()),
            _ => None,
        };
        if hardware.is_none() {
            absent.push(Absent {
                field: "gateway_hardware",
                reason: if readings.neighbours.is_err() {
                    why(&readings.neighbours)
                } else {
                    AbsentReason::NotStated
                },
            });
        }

        let dhcp = match &readings.dhcp {
            Ok(read) => read
                .servers
                .iter()
                .find(|s| {
                    s.interface_index == Some(interface.index)
                        || s.interface.as_deref() == Some(interface.name.as_str())
                })
                .map(|s| s.server),
            Err(_) => None,
        };
        if dhcp.is_none() {
            absent.push(Absent {
                field: "dhcp_server",
                reason: if readings.dhcp.is_err() {
                    why(&readings.dhcp)
                } else {
                    AbsentReason::NotStated
                },
            });
        }

        if route.gateway.is_none() {
            absent.push(Absent {
                field: "gateway_address",
                reason: AbsentReason::NotStated,
            });
        }
        let subnet = subnet_of(interface, route.gateway, family);
        if subnet.is_none() {
            absent.push(Absent {
                field: "subnet",
                reason: AbsentReason::NotStated,
            });
        }

        let domains: Vec<String> = match &readings.resolvers {
            Ok(read) => {
                let named: Vec<_> = read
                    .settings
                    .iter()
                    .filter(|s| s.from == interface.name)
                    .collect();
                let chosen = if named.is_empty() {
                    read.settings
                        .iter()
                        .filter(|s| s.from.starts_with('/'))
                        .collect()
                } else {
                    named
                };
                chosen
                    .into_iter()
                    .flat_map(|s| s.search_domains.iter().cloned())
                    .collect()
            }
            Err(_) => Vec::new(),
        };
        let domains: Vec<&str> = domains.iter().map(String::as_str).collect();
        if domains.iter().all(|d| normal_domain(d).is_none()) {
            absent.push(Absent {
                field: "dns_domain",
                reason: if readings.resolvers.is_err() {
                    why(&readings.resolvers)
                } else {
                    AbsentReason::NotStated
                },
            });
        }
        // The wireless network's name is not read (decision 0243).
        if interface.kind == Some(InterfaceKind::Wireless) {
            absent.push(Absent {
                field: "wireless_network_name",
                reason: AbsentReason::NotRead,
            });
        }

        absent.sort();
        Self {
            fingerprint: NetworkFingerprint::of_facts(
                hardware.as_deref(),
                route.gateway,
                dhcp,
                subnet,
                &domains,
            ),
            interface: Some(interface.name.clone()),
            absent,
        }
    }
}

/// The subnet of an interface's address that holds the gateway; where none
/// does (a link-local gateway), the first address of the family that is
/// neither loopback nor link-local, which for IPv6 is the stable prefix.
fn subnet_of(
    interface: &crate::model::Interface,
    gateway: Option<IpAddr>,
    family: Family,
) -> Option<(IpAddr, u8)> {
    let candidates = interface
        .addresses
        .iter()
        .filter(|a| Family::of(&a.address) == family)
        .filter_map(|a| Some((a.address, a.prefix_length.filter(|p| *p > 0)?)));
    let holding = candidates
        .clone()
        .find(|(address, prefix)| gateway.is_some_and(|g| same_subnet(*address, g, *prefix)));
    holding.or_else(|| {
        candidates
            .filter(|(address, _)| !address.is_loopback())
            .find(|(address, _)| {
                !matches!(
                    a2m_core::ScopedHost::of_ip(*address).scope(),
                    a2m_core::AddressScope::LinkLocal
                )
            })
    })
}

/// Reads the fingerprint of the network the machine is on now, as an ordinary
/// user, sending nothing.
///
/// # Errors
///
/// [`NetError`] when the interfaces or the routes cannot be read: without
/// them there is no network to name. The other facts, when they cannot be
/// read, are named in [`FingerprintReading::absent`].
pub fn fingerprint() -> Result<FingerprintReading, NetError> {
    let readings = Readings {
        interfaces: crate::interfaces()?,
        routes: crate::routes()?,
        neighbours: crate::neighbours(),
        resolvers: crate::resolvers(),
        dhcp: crate::dhcp_servers(),
    };
    Ok(FingerprintReading::of(&readings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        HardwareAddress, Interface, InterfaceAddress, Neighbour, ResolverSetting, Route,
    };

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    const GATEWAY_A: [u8; 6] = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
    const GATEWAY_B: [u8; 6] = [0x00, 0x11, 0x22, 0x33, 0x44, 0x66];

    /// The facts of a network: hardware, gateway, server, subnet, domains.
    fn net(
        hardware: &[u8],
        gateway: &str,
        server: Option<&str>,
        subnet: &str,
        domains: &[&str],
    ) -> NetworkFingerprint {
        NetworkFingerprint::of_facts(
            Some(hardware),
            Some(ip(gateway)),
            server.map(ip),
            Some((ip(subnet), 24)),
            domains,
        )
    }

    fn home() -> NetworkFingerprint {
        net(
            &GATEWAY_A,
            "192.168.1.1",
            Some("192.168.1.1"),
            "192.168.1.20",
            &["home.example"],
        )
    }

    #[test]
    fn the_same_network_is_recognized_across_a_changed_lease_and_address() {
        // A new address in the subnet, a domain added, the server not
        // stated this time: every core fact still agrees.
        let later = net(
            &GATEWAY_A,
            "192.168.1.1",
            None,
            "192.168.1.77",
            &["Home.Example.", "lab.example"],
        );
        assert_eq!(home().recognize(&later), Recognition::Same);
        assert_eq!(later.recognize(&home()), Recognition::Same);
    }

    #[test]
    fn a_fact_that_disagrees_makes_a_different_network_whatever_else_agrees() {
        use FingerprintFact::{DhcpServer, DnsDomain, GatewayAddress, GatewayHardware, Subnet};
        let cases = [
            (
                net(
                    &GATEWAY_B,
                    "192.168.1.1",
                    Some("192.168.1.1"),
                    "192.168.1.20",
                    &["home.example"],
                ),
                GatewayHardware,
            ),
            (
                net(
                    &GATEWAY_A,
                    "192.168.1.2",
                    Some("192.168.1.1"),
                    "192.168.1.20",
                    &["home.example"],
                ),
                GatewayAddress,
            ),
            (
                net(
                    &GATEWAY_A,
                    "192.168.1.1",
                    Some("192.168.1.1"),
                    "192.168.2.20",
                    &["home.example"],
                ),
                Subnet,
            ),
            (
                net(
                    &GATEWAY_A,
                    "192.168.1.1",
                    Some("192.168.1.2"),
                    "192.168.1.20",
                    &["home.example"],
                ),
                DhcpServer,
            ),
            (
                net(
                    &GATEWAY_A,
                    "192.168.1.1",
                    Some("192.168.1.1"),
                    "192.168.1.20",
                    &["cafe.example"],
                ),
                DnsDomain,
            ),
        ];
        for (other, fact) in cases {
            assert_eq!(home().recognize(&other), Recognition::Different(fact));
            assert_eq!(other.recognize(&home()), Recognition::Different(fact));
        }
    }

    #[test]
    fn a_core_fact_that_is_not_stated_leaves_the_network_unrecognized() {
        // No gateway hardware address, as when the neighbour table holds no
        // entry: everything else agrees, and it is still not recognized.
        let unread = NetworkFingerprint::of_facts(
            None,
            Some(ip("192.168.1.1")),
            Some(ip("192.168.1.1")),
            Some((ip("192.168.1.20"), 24)),
            &["home.example"],
        );
        assert_eq!(home().recognize(&unread), Recognition::Insufficient);
        assert_eq!(unread.recognize(&unread), Recognition::Insufficient);
        assert_eq!(
            NetworkFingerprint::empty().recognize(&NetworkFingerprint::empty()),
            Recognition::Insufficient
        );
        assert!(NetworkFingerprint::empty().recognize(&home()) != Recognition::Same);
        // A disagreeing fact still shows as different when another is
        // missing: a known difference is not hidden by a gap.
        let other = NetworkFingerprint::of_facts(
            Some(&GATEWAY_B),
            None,
            None,
            Some((ip("192.168.1.20"), 24)),
            &[],
        );
        assert_eq!(
            home().recognize(&other),
            Recognition::Different(FingerprintFact::GatewayHardware)
        );
    }

    #[test]
    fn two_networks_of_one_range_and_one_virtual_gateway_read_as_one() {
        // The false match research note 32 names, held here so that it is
        // not forgotten: the facts are equal, so the networks are the same
        // to this comparison. Only a secondary fact that differs separates
        // them.
        let virtual_gateway = [0x00, 0x00, 0x5e, 0x00, 0x01, 0x01];
        let one = net(&virtual_gateway, "10.0.0.1", None, "10.0.0.20", &[]);
        let two = net(&virtual_gateway, "10.0.0.1", None, "10.0.0.99", &[]);
        assert_eq!(one.recognize(&two), Recognition::Same);
        let two = net(
            &virtual_gateway,
            "10.0.0.1",
            None,
            "10.0.0.99",
            &["other.example"],
        );
        let one = net(
            &virtual_gateway,
            "10.0.0.1",
            None,
            "10.0.0.20",
            &["one.example"],
        );
        assert_eq!(
            one.recognize(&two),
            Recognition::Different(FingerprintFact::DnsDomain)
        );
    }

    #[test]
    fn a_subnet_is_told_by_its_prefix_and_a_domain_by_its_lower_case() {
        let sub = |address, prefix| {
            NetworkFingerprint::of_facts(None, None, None, Some((ip(address), prefix)), &[]).subnet
        };
        assert_eq!(sub("10.1.2.3", 20), sub("10.1.15.200", 20));
        assert_ne!(sub("10.1.2.3", 20), sub("10.1.16.1", 20));
        // The same address under another prefix is another subnet.
        assert_ne!(sub("10.1.2.3", 20), sub("10.1.2.3", 24));
        // No prefix, no subnet.
        assert!(sub("10.1.2.3", 0).is_none());
        // An all-zero hardware address is no address.
        assert!(
            NetworkFingerprint::of_facts(Some(&[0; 6]), None, None, None, &[])
                .gateway_hardware
                .is_none()
        );
    }

    #[test]
    fn a_stored_fingerprint_shows_no_address_and_reads_back_the_same() {
        let json = serde_json::to_string(&home()).unwrap();
        for forbidden in ["192.168", "home.example", "00:11"] {
            assert!(!json.contains(forbidden), "{json}");
        }
        let back: NetworkFingerprint = serde_json::from_str(&json).unwrap();
        assert_eq!(back, home());
        assert_eq!(
            home().stated(),
            FingerprintFact::ALL.to_vec(),
            "every fact is stated"
        );
    }

    fn interface(index: u32, name: &str, kind: InterfaceKind, address: &str) -> Interface {
        Interface {
            index,
            name: name.to_owned(),
            kind: Some(kind),
            system_kind: None,
            state: Some(LinkState::Up),
            mtu: None,
            hardware_address: HardwareAddress::from_octets(&[2, 0, 0, 0, 0, 1]),
            addresses: vec![InterfaceAddress {
                address: ip(address),
                prefix_length: Some(24),
            }],
            absent: Vec::new(),
        }
    }

    fn route(index: u32, gateway: &str) -> Route {
        let gateway = ip(gateway);
        Route {
            family: Family::of(&gateway),
            destination: ip("0.0.0.0"),
            prefix_length: 0,
            gateway: Some(gateway),
            interface_index: Some(index),
            interface: None,
            metric: Some(100),
        }
    }

    fn readings(interface: Interface, gateway: &str) -> Readings {
        let index = interface.index;
        let name = interface.name.clone();
        let default = route(index, gateway);
        Readings {
            interfaces: Interfaces {
                interfaces: vec![interface],
                read_from: Vec::new(),
                cut: Vec::new(),
            },
            routes: Routes {
                routes: vec![default.clone()],
                default_routes: vec![default],
                read_from: Vec::new(),
                cut: Vec::new(),
            },
            neighbours: Ok(Neighbours {
                neighbours: vec![Neighbour {
                    address: ip(gateway),
                    interface_index: Some(index),
                    interface: None,
                    hardware_address: HardwareAddress::from_octets(&GATEWAY_A),
                    state: Some(NeighbourState::Reachable),
                }],
                read_from: Vec::new(),
                absent: Vec::new(),
                cut: Vec::new(),
            }),
            resolvers: Ok(Resolvers {
                settings: vec![ResolverSetting {
                    from: "/etc/resolv.conf".to_owned(),
                    name_servers: vec![ip("192.168.1.1")],
                    search_domains: vec!["home.example".to_owned()],
                }],
                read_from: Vec::new(),
                absent: Vec::new(),
                cut: Vec::new(),
            }),
            dhcp: Ok(DhcpServers {
                servers: vec![crate::dhcp::DhcpServer {
                    interface_index: None,
                    interface: Some(name),
                    server: ip("192.168.1.1"),
                }],
                read_from: Vec::new(),
                cut: Vec::new(),
            }),
        }
    }

    #[test]
    fn a_fingerprint_is_read_from_the_machines_own_tables() {
        let read = FingerprintReading::of(&readings(
            interface(2, "eth0", InterfaceKind::Wired, "192.168.1.20"),
            "192.168.1.1",
        ));
        assert_eq!(read.fingerprint, home());
        assert_eq!(read.interface.as_deref(), Some("eth0"));
        assert!(read.absent.is_empty(), "{:?}", read.absent);
    }

    #[test]
    fn a_fact_that_cannot_be_read_is_named_never_guessed() {
        let mut r = readings(
            interface(3, "wlan0", InterfaceKind::Wireless, "192.168.1.20"),
            "192.168.1.1",
        );
        r.neighbours = Err(NetError::Refused {
            fact: crate::Fact::Neighbours,
        });
        r.dhcp = Err(NetError::NotBuilt {
            fact: crate::Fact::DhcpServers,
            system: "example",
        });
        let read = FingerprintReading::of(&r);
        let fields: Vec<_> = read.absent.iter().map(|a| (a.field, a.reason)).collect();
        assert!(fields.contains(&("gateway_hardware", AbsentReason::NotVisibleToThisUser)));
        assert!(fields.contains(&("dhcp_server", AbsentReason::NotRead)));
        assert!(fields.contains(&("wireless_network_name", AbsentReason::NotRead)));
        assert_eq!(
            read.fingerprint.stated(),
            vec![
                FingerprintFact::GatewayAddress,
                FingerprintFact::Subnet,
                FingerprintFact::DnsDomain
            ]
        );
        // A core fact is missing: the network is not recognized.
        assert_eq!(
            home().recognize(&read.fingerprint),
            Recognition::Insufficient
        );
    }

    #[test]
    fn a_failed_neighbour_entry_names_no_gateway() {
        let mut r = readings(
            interface(2, "eth0", InterfaceKind::Wired, "192.168.1.20"),
            "192.168.1.1",
        );
        if let Ok(read) = &mut r.neighbours {
            read.neighbours[0].state = Some(NeighbourState::Failed);
        }
        let read = FingerprintReading::of(&r);
        assert!(
            !read
                .fingerprint
                .stated()
                .contains(&FingerprintFact::GatewayHardware)
        );
    }

    #[test]
    fn a_default_route_through_a_tunnel_is_no_network() {
        let read = FingerprintReading::of(&readings(
            interface(7, "tun0", InterfaceKind::Tunnel, "10.8.0.2"),
            "10.8.0.1",
        ));
        assert!(read.fingerprint.is_empty());
        assert_eq!(read.absent[0].field, "network_through_tunnel");
    }

    #[test]
    fn no_default_route_is_no_network() {
        let mut r = readings(
            interface(2, "eth0", InterfaceKind::Wired, "192.168.1.20"),
            "192.168.1.1",
        );
        r.routes.default_routes.clear();
        let read = FingerprintReading::of(&r);
        assert!(read.fingerprint.is_empty());
        assert_eq!(read.absent[0].field, "default_route");
    }
}
