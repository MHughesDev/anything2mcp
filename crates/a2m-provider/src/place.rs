//! The network place of an address: where it is, and how this machine
//! reaches it (decision 0223, built by package N2b).
//!
//! A place is derived, never taken from an input (section 502). It is told
//! from the address's scope, which is read from its literal alone
//! ([`ScopedHost`], decision 0240), and from the machine's own interfaces
//! ([`MachineState`], read by `a2m-netstate`, which sends nothing). A name
//! is never resolved on the way: a name has the place `unknown`, and a place
//! that cannot be told is `unknown`, a stop wherever a place decides
//! something, never a guess (rule 7).

use std::net::IpAddr;

use a2m_core::{AddressScope, ScopedHost};
use a2m_netstate::{Interfaces, LinkState};

/// Where an address is, and how this machine reaches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NetworkPlace {
    /// A loopback address written plainly as an IP address.
    ThisMachine,
    /// An address inside the subnet of one of this machine's own interfaces
    /// that is up.
    LocalNetwork,
    /// An address in no special-purpose range and on no attached subnet.
    PublicInternet,
    /// An address reached only through a gateway the person named. No
    /// gateway can be named yet (decisions 0208 and 0223, package N3), so
    /// [`NetworkPlace::of`] never gives it, and a connection is only ever
    /// made directly (decision 0197).
    ViaGateway,
    /// A place that cannot be told from the literal and the machine's
    /// state: a name, an unreadable address, a private address on no
    /// attached subnet, a special-purpose range, or a machine whose state
    /// could not be read.
    Unknown,
}

impl NetworkPlace {
    /// Every place, in the order the documents list them.
    pub const ALL: &'static [Self] = &[
        Self::ThisMachine,
        Self::LocalNetwork,
        Self::PublicInternet,
        Self::ViaGateway,
        Self::Unknown,
    ];

    /// The stable name used in the documents and in messages.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ThisMachine => "this_machine",
            Self::LocalNetwork => "local_network",
            Self::PublicInternet => "public_internet",
            Self::ViaGateway => "via_gateway",
            Self::Unknown => "unknown",
        }
    }

    /// Whether a connection to a destination of this place is made
    /// directly, with no gateway between. Every place the derivation gives
    /// is; a connection through a gateway is not built.
    #[must_use]
    pub fn is_direct(self) -> bool {
        self != Self::ViaGateway
    }

    /// The place of a host, and the reason for it. `state` is what the
    /// machine says of itself; nothing is sent and no name is resolved.
    #[must_use]
    pub fn of(host: &ScopedHost, state: &MachineState) -> Placed {
        let (place, because) = Self::tell(host, state);
        Placed { place, because }
    }

    fn tell(host: &ScopedHost, state: &MachineState) -> (Self, &'static str) {
        match host.scope() {
            AddressScope::ThisMachine if !host.may_leave_this_machine() => {
                return (Self::ThisMachine, "a loopback address written plainly");
            }
            AddressScope::ThisMachine => {
                return (
                    Self::Unknown,
                    "a loopback address written with a zone or inside IPv6, which is not read as this machine",
                );
            }
            AddressScope::Name => return (Self::Unknown, "a name, which is never resolved"),
            AddressScope::Unreadable => return (Self::Unknown, "an address that cannot be read"),
            AddressScope::Unspecified | AddressScope::Multicast => {
                return (Self::Unknown, "an address that names no one host");
            }
            _ => {}
        }
        let Some(ip) = host.ip() else {
            return (Self::Unknown, "an address that cannot be read");
        };
        let Some(subnets) = state.subnets() else {
            return (Self::Unknown, "the machine's interfaces could not be read");
        };
        if subnets.iter().any(|subnet| subnet.holds(ip)) {
            return (
                Self::LocalNetwork,
                "inside the subnet of an interface that is up",
            );
        }
        match host.scope() {
            AddressScope::Public => (Self::PublicInternet, "public, and on no attached subnet"),
            AddressScope::Documentation | AddressScope::Reserved => (
                Self::Unknown,
                "a special-purpose range, on no attached subnet",
            ),
            AddressScope::LinkLocal => (Self::Unknown, "link-local, and on no attached subnet"),
            _ => (
                Self::Unknown,
                "a private range on no attached subnet, reached by a route nothing here reads",
            ),
        }
    }
}

impl std::fmt::Display for NetworkPlace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A place, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    /// The place.
    pub place: NetworkPlace,
    /// Why, in words: the count of `unknown` places that hypothesis H-43
    /// asks for is a count of these.
    pub because: &'static str,
}

/// One subnet of this machine: an address of an interface that is up, and
/// the length of its prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Subnet {
    network: IpAddr,
    prefix: u8,
}

impl Subnet {
    fn holds(self, ip: IpAddr) -> bool {
        // A subnet of no prefix would hold every address: it is no subnet.
        if self.prefix == 0 {
            return false;
        }
        match (self.network, ip) {
            (IpAddr::V4(a), IpAddr::V4(b)) => {
                same_prefix(&a.octets(), &b.octets(), self.prefix.min(32))
            }
            (IpAddr::V6(a), IpAddr::V6(b)) => {
                same_prefix(&a.octets(), &b.octets(), self.prefix.min(128))
            }
            _ => false,
        }
    }
}

fn same_prefix(a: &[u8], b: &[u8], bits: u8) -> bool {
    let whole = usize::from(bits / 8);
    let rest = bits % 8;
    if a[..whole] != b[..whole] {
        return false;
    }
    rest == 0 || {
        let mask = 0xffu8 << (8 - rest);
        a[whole] & mask == b[whole] & mask
    }
}

/// What this machine says of its own place on its networks, as far as a
/// place needs it: the subnets of its interfaces that are up. Read from the
/// system, sending nothing. It is the machine's, never an input's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineState {
    subnets: Option<Vec<Subnet>>,
}

impl MachineState {
    /// A machine whose state could not be read: every place but
    /// `this_machine` is `unknown`.
    #[must_use]
    pub fn unread() -> Self {
        Self { subnets: None }
    }

    /// The state given by a reading of the interfaces: each address of an
    /// interface that is up, with its prefix. An address with no prefix
    /// stated, or on a loopback interface, names no subnet.
    #[must_use]
    pub fn of(interfaces: &Interfaces) -> Self {
        Self::of_subnets(
            interfaces
                .interfaces
                .iter()
                .filter(|interface| interface.state == Some(LinkState::Up))
                .flat_map(|interface| &interface.addresses)
                .filter_map(|address| Some((address.address, address.prefix_length?))),
        )
    }

    /// The state of a machine with these subnets, each an address of one of
    /// its interfaces and the length of its prefix. A loopback address
    /// names no subnet.
    #[must_use]
    pub fn of_subnets(subnets: impl IntoIterator<Item = (IpAddr, u8)>) -> Self {
        Self {
            subnets: Some(
                subnets
                    .into_iter()
                    .filter(|(network, _)| !network.is_loopback())
                    .map(|(network, prefix)| Subnet { network, prefix })
                    .collect(),
            ),
        }
    }

    /// The state read from the system now. A reading that fails gives
    /// [`MachineState::unread`]: a place is then `unknown`, never guessed.
    #[must_use]
    pub fn read() -> Self {
        a2m_netstate::interfaces().map_or_else(|_| Self::unread(), |read| Self::of(&read))
    }

    fn subnets(&self) -> Option<&[Subnet]> {
        self.subnets.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn home() -> MachineState {
        MachineState::of_subnets([
            (ip("192.168.1.20"), 24),
            (ip("fe80::1"), 64),
            (ip("2001:db8:5::7"), 48),
        ])
    }

    fn place(address: &str, state: &MachineState) -> NetworkPlace {
        NetworkPlace::of(&ScopedHost::of_destination(address), state).place
    }

    #[test]
    fn a_place_is_told_from_the_literal_and_the_machines_subnets() {
        use NetworkPlace::{LocalNetwork, PublicInternet, ThisMachine, Unknown};
        let state = home();
        for (address, expected) in [
            ("http://127.0.0.1:8080/x", ThisMachine),
            ("[::1]:4222", ThisMachine),
            ("192.168.1.99", LocalNetwork),
            ("http://192.168.1.255/", LocalNetwork),
            ("[fe80::abcd]:80", LocalNetwork),
            ("[2001:db8:5::9]:80", LocalNetwork),
            ("https://93.184.216.34/", PublicInternet),
            ("[2606:4700::1]:443", PublicInternet),
            // A private range with no subnet of the machine's own.
            ("192.168.2.1", Unknown),
            ("10.0.0.1", Unknown),
            ("169.254.1.1", Unknown),
            // A name is never resolved, whatever it would resolve to.
            ("https://app.example.com/", Unknown),
            ("localhost:8080", Unknown),
            // A loopback address not written plainly is not this machine.
            ("[::ffff:127.0.0.1]:80", Unknown),
            ("[::1%1]:80", Unknown),
            // Special-purpose ranges and what cannot be read.
            ("0.0.0.0", Unknown),
            ("224.0.0.251", Unknown),
            ("203.0.113.5", Unknown),
            ("[2001:db8:9::1]:80", Unknown),
            ("user@127.0.0.1", Unknown),
            ("127.1", Unknown),
        ] {
            assert_eq!(place(address, &state), expected, "{address}");
        }
    }

    #[test]
    fn a_machine_that_cannot_be_read_places_only_this_machine() {
        let state = MachineState::unread();
        assert_eq!(place("127.0.0.1:80", &state), NetworkPlace::ThisMachine);
        for address in ["192.168.1.99", "93.184.216.34", "[2606:4700::1]:443"] {
            let placed = NetworkPlace::of(&ScopedHost::of_destination(address), &state);
            assert_eq!(placed.place, NetworkPlace::Unknown, "{address}");
            assert!(placed.because.contains("could not be read"), "{address}");
        }
    }

    #[test]
    fn a_subnet_holds_exactly_its_prefix() {
        let subnet = |network: &str, prefix| Subnet {
            network: ip(network),
            prefix,
        };
        let twenty = subnet("10.1.2.3", 20);
        assert!(twenty.holds(ip("10.1.0.0")));
        assert!(twenty.holds(ip("10.1.15.255")));
        assert!(!twenty.holds(ip("10.1.16.0")));
        assert!(!twenty.holds(ip("10.0.255.255")));
        assert!(!twenty.holds(ip("fe80::1")));
        assert!(!subnet("10.1.2.3", 0).holds(ip("203.0.113.1")));
        let host = subnet("10.1.2.3", 32);
        assert!(host.holds(ip("10.1.2.3")) && !host.holds(ip("10.1.2.4")));
        // A prefix longer than the family's is held to the family's.
        let long = subnet("10.1.2.3", 200);
        assert!(long.holds(ip("10.1.2.3")) && !long.holds(ip("10.1.2.4")));
    }

    #[test]
    fn a_loopback_subnet_places_nothing_and_no_gateway_is_derived() {
        // A machine that states 127.0.0.1/8 as an interface subnet does not
        // make `127.2.3.4` a local network: loopback is this machine alone.
        let state = MachineState::of_subnets([(ip("127.0.0.1"), 8)]);
        assert_eq!(place("127.2.3.4", &state), NetworkPlace::ThisMachine);
        let state = home();
        for address in [
            "127.0.0.1",
            "192.168.1.5",
            "93.184.216.34",
            "app.example.com",
            "10.0.0.1",
            "",
            "::",
        ] {
            let placed = NetworkPlace::of(&ScopedHost::of_destination(address), &state);
            assert_ne!(placed.place, NetworkPlace::ViaGateway, "{address}");
            assert!(placed.place.is_direct());
        }
        assert!(!NetworkPlace::ViaGateway.is_direct());
    }

    #[test]
    fn the_machine_places_an_address_of_its_own_up_interface_on_its_local_network() {
        // The test asks the system what its interfaces hold, and reads the
        // same listing through the derivation.
        let read = a2m_netstate::interfaces().expect("interfaces are readable");
        let state = MachineState::of(&read);
        assert_eq!(place("127.0.0.1", &state), NetworkPlace::ThisMachine);
        for interface in &read.interfaces {
            if interface.state != Some(LinkState::Up) {
                continue;
            }
            for address in &interface.addresses {
                if address.prefix_length.is_some() && !address.address.is_loopback() {
                    let text = match address.address {
                        IpAddr::V4(v4) => v4.to_string(),
                        IpAddr::V6(v6) => format!("[{v6}]:80"),
                    };
                    assert_eq!(place(&text, &state), NetworkPlace::LocalNetwork, "{text}");
                }
            }
        }
    }

    #[test]
    fn every_place_has_one_name() {
        let names: Vec<_> = NetworkPlace::ALL.iter().map(|p| p.as_str()).collect();
        assert_eq!(
            names,
            [
                "this_machine",
                "local_network",
                "public_internet",
                "via_gateway",
                "unknown"
            ]
        );
    }
}
