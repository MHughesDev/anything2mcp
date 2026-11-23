//! A reach: what the person has said, for one network they named, may be
//! looked at and sent to, and through which gateway (decision 0223, package
//! N3).
//!
//! A reach is the person's, and only a person's act makes or changes one:
//! no input an agent supplies names a network, a destination, or a gateway
//! (section 502). This module holds the record, the test of whether it is in
//! force on the network the machine is on now, and the derivation of the
//! place `via_gateway` from it. Keeping it, bound to the operating system's
//! presence check, is the protected setting store's work, in `a2m-actions`.
//!
//! **A reach named on one network is not in force on another.** It is in
//! force only when the fingerprint it was named on is recognized as the
//! fingerprint of the network the machine is on now
//! ([`NetworkFingerprint::recognize`] says [`Recognition::Same`]). A network
//! not recognized, because a core fact differs or cannot be read, has no
//! reach at all.
//!
//! **`via_gateway` is derived, never asserted.** An address has the place
//! `via_gateway` only when a reach in force names a gateway for that same
//! destination. The gateway is the name of a setting the person made, never a
//! value an input gives. A loopback address is always `this_machine`: it
//! never leaves the machine, so no gateway is read into it. Two reaches in
//! force that name different gateways for one destination leave the place
//! `unknown`, a stop, never a guess (rule 7).

use std::collections::BTreeSet;
use std::fmt;

use a2m_core::{AddressScope, ScopedHost};
use a2m_netstate::{NetworkFingerprint, Recognition};
use serde::{Deserialize, Serialize};

use crate::place::{MachineState, NetworkPlace};

/// The name of a gateway the person named: lowercase letters, digits, and
/// underscores, starting with a letter, at most 64 characters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GatewayName(String);

impl GatewayName {
    /// The longest a gateway name may be.
    pub const LONGEST: usize = 64;

    /// Checks `name` and keeps it.
    ///
    /// # Errors
    ///
    /// A message when `name` is not a name of that shape.
    pub fn new(name: &str) -> Result<Self, String> {
        let mut chars = name.chars();
        let shaped = chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && name.len() <= Self::LONGEST;
        if !shaped {
            return Err(format!(
                "a gateway name starts with a lowercase letter, continues with lowercase letters, digits, and underscores, and has at most {} characters",
                Self::LONGEST
            ));
        }
        Ok(Self(name.to_owned()))
    }

    /// The name as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for GatewayName {
    type Error = String;
    fn try_from(name: String) -> Result<Self, String> {
        Self::new(&name)
    }
}

impl From<GatewayName> for String {
    fn from(name: GatewayName) -> Self {
        name.0
    }
}

impl fmt::Display for GatewayName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One destination a reach allows, and the gateway the person said it is
/// reached through, if any.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachEntry {
    /// The destination as the person wrote it: a host, or a host and port,
    /// or an address.
    pub destination: String,
    /// The gateway it is reached through. None: it is reached directly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<GatewayName>,
}

impl ReachEntry {
    /// An entry reached directly.
    #[must_use]
    pub fn direct(destination: &str) -> Self {
        Self {
            destination: destination.to_owned(),
            gateway: None,
        }
    }

    /// An entry reached through a gateway.
    #[must_use]
    pub fn through(destination: &str, gateway: GatewayName) -> Self {
        Self {
            destination: destination.to_owned(),
            gateway: Some(gateway),
        }
    }

    /// Whether this entry is for `host`: the same address, or the same name
    /// in either case. A destination that cannot be read is for nothing.
    fn is_for(&self, host: &ScopedHost) -> bool {
        let mine = ScopedHost::of_destination(&self.destination);
        if mine.scope() == AddressScope::Unreadable || host.scope() == AddressScope::Unreadable {
            return false;
        }
        match (mine.name(), host.name()) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            (None, None) => mine == *host,
            _ => false,
        }
    }
}

/// What the person said of one network: where it was named, what may be
/// looked at, and what may be sent to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reach {
    /// The person's own label for the network, to show it back to them.
    pub label: String,
    /// The fingerprint of the network when the person named it.
    pub network: NetworkFingerprint,
    /// Destinations that may be looked at.
    #[serde(default)]
    pub look: Vec<ReachEntry>,
    /// Destinations that may be sent to.
    #[serde(default)]
    pub send: Vec<ReachEntry>,
}

impl Reach {
    /// Whether the reach is in force on the network the machine is on now:
    /// the fingerprints are recognized as one network.
    #[must_use]
    pub fn is_in_force(&self, current: &NetworkFingerprint) -> bool {
        self.network.recognize(current) == Recognition::Same
    }

    /// The gateways this reach names for `host`, if it is in force.
    fn gateways_for<'a>(
        &'a self,
        host: &ScopedHost,
        current: &NetworkFingerprint,
    ) -> impl Iterator<Item = &'a GatewayName> {
        let in_force = self.is_in_force(current);
        let host = host.clone();
        self.look
            .iter()
            .chain(&self.send)
            .filter(move |_| in_force)
            .filter(move |entry| entry.is_for(&host))
            .filter_map(|entry| entry.gateway.as_ref())
    }
}

/// A place, why, and the gateway when the place is `via_gateway`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedInReach {
    /// The place.
    pub place: NetworkPlace,
    /// Why, in words.
    pub because: &'static str,
    /// The gateway the person named, for `via_gateway` and for nothing else.
    pub gateway: Option<GatewayName>,
}

impl NetworkPlace {
    /// The place of a host given the reaches the person has saved and the
    /// fingerprint of the network the machine is on now. Nothing is sent and
    /// no name is resolved.
    ///
    /// The place is the one [`NetworkPlace::of`] gives, except that a
    /// destination a reach in force reaches through a named gateway is
    /// `via_gateway`. See the module's documentation for the rules.
    #[must_use]
    pub fn of_in_reach(
        host: &ScopedHost,
        state: &MachineState,
        reaches: &[Reach],
        current: &NetworkFingerprint,
    ) -> PlacedInReach {
        let base = Self::of(host, state);
        let plain = PlacedInReach {
            place: base.place,
            because: base.because,
            gateway: None,
        };
        // Loopback never leaves the machine: a gateway is not read into it.
        if base.place == Self::ThisMachine {
            return plain;
        }
        let named: BTreeSet<&GatewayName> = reaches
            .iter()
            .flat_map(|reach| reach.gateways_for(host, current))
            .collect();
        let mut named = named.into_iter();
        match (named.next(), named.next()) {
            (None, _) => plain,
            (Some(gateway), None) => PlacedInReach {
                place: Self::ViaGateway,
                because: "a reach in force names a gateway for it",
                gateway: Some(gateway.clone()),
            },
            (Some(_), Some(_)) => PlacedInReach {
                place: Self::Unknown,
                because: "reaches in force name different gateways for it",
                gateway: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn network(hardware: u8, gateway: &str, subnet: &str) -> NetworkFingerprint {
        NetworkFingerprint::of_facts(
            Some(&[0, 0x11, 0x22, 0x33, 0x44, hardware]),
            Some(ip(gateway)),
            None,
            Some((ip(subnet), 24)),
            &[],
        )
    }

    fn home() -> NetworkFingerprint {
        network(1, "192.168.1.1", "192.168.1.20")
    }

    fn cafe() -> NetworkFingerprint {
        network(9, "10.20.0.1", "10.20.0.77")
    }

    fn gateway(name: &str) -> GatewayName {
        GatewayName::new(name).unwrap()
    }

    fn reach(on: NetworkFingerprint, entries: Vec<ReachEntry>) -> Reach {
        Reach {
            label: "named".to_owned(),
            network: on,
            look: Vec::new(),
            send: entries,
        }
    }

    fn state() -> MachineState {
        MachineState::of_subnets([(ip("192.168.1.20"), 24)])
    }

    fn place(address: &str, reaches: &[Reach], current: &NetworkFingerprint) -> PlacedInReach {
        NetworkPlace::of_in_reach(
            &ScopedHost::of_destination(address),
            &state(),
            reaches,
            current,
        )
    }

    #[test]
    fn a_destination_a_reach_in_force_sends_through_a_gateway_is_via_gateway() {
        let reaches = [reach(
            home(),
            vec![
                ReachEntry::through("https://app.example.com/x", gateway("office")),
                ReachEntry::through("192.168.1.50:80", gateway("office")),
                ReachEntry::direct("192.168.1.60"),
            ],
        )];
        for address in ["app.example.com", "APP.example.com:8443", "192.168.1.50"] {
            let placed = place(address, &reaches, &home());
            assert_eq!(placed.place, NetworkPlace::ViaGateway, "{address}");
            assert_eq!(placed.gateway, Some(gateway("office")), "{address}");
            assert!(!placed.place.is_direct());
        }
        // An entry with no gateway is reached directly, as it was.
        let direct = place("192.168.1.60", &reaches, &home());
        assert_eq!(direct.place, NetworkPlace::LocalNetwork);
        assert_eq!(direct.gateway, None);
        // A destination no entry names keeps the place it had.
        assert_eq!(
            place("192.168.1.99", &reaches, &home()).place,
            NetworkPlace::LocalNetwork
        );
        assert_eq!(
            place("other.example.com", &reaches, &home()).place,
            NetworkPlace::Unknown
        );
    }

    #[test]
    fn a_reach_named_on_one_network_is_not_in_force_on_another() {
        let reaches = [reach(
            home(),
            vec![ReachEntry::through("app.example.com", gateway("office"))],
        )];
        assert_eq!(
            place("app.example.com", &reaches, &home()).place,
            NetworkPlace::ViaGateway
        );
        // The machine has moved to a network the reach was not named on.
        let moved = place("app.example.com", &reaches, &cafe());
        assert_eq!(moved.place, NetworkPlace::Unknown);
        assert_eq!(moved.gateway, None);
        // A network that cannot be told is no network: nothing is in force.
        let unread = NetworkFingerprint::empty();
        assert_eq!(
            place("app.example.com", &reaches, &unread).place,
            NetworkPlace::Unknown
        );
        assert!(!reaches[0].is_in_force(&cafe()));
        assert!(!reaches[0].is_in_force(&unread));
        assert!(reaches[0].is_in_force(&home()));
    }

    #[test]
    fn loopback_is_never_reached_through_a_gateway() {
        let reaches = [reach(
            home(),
            vec![ReachEntry::through("127.0.0.1", gateway("office"))],
        )];
        let placed = place("127.0.0.1:8080", &reaches, &home());
        assert_eq!(placed.place, NetworkPlace::ThisMachine);
        assert_eq!(placed.gateway, None);
    }

    #[test]
    fn two_reaches_naming_different_gateways_leave_the_place_unknown() {
        let one = reach(
            home(),
            vec![ReachEntry::through("app.example.com", gateway("one"))],
        );
        let two = reach(
            home(),
            vec![ReachEntry::through("app.example.com", gateway("two"))],
        );
        let same = reach(
            home(),
            vec![ReachEntry::through("app.example.com", gateway("one"))],
        );
        assert_eq!(
            place("app.example.com", &[one.clone(), two], &home()).place,
            NetworkPlace::Unknown
        );
        assert_eq!(
            place("app.example.com", &[one, same], &home()).place,
            NetworkPlace::ViaGateway
        );
    }

    #[test]
    fn a_destination_that_cannot_be_read_is_for_no_entry() {
        let reaches = [reach(
            home(),
            vec![ReachEntry::through("", gateway("office"))],
        )];
        assert_eq!(place("", &reaches, &home()).place, NetworkPlace::Unknown);
        assert_eq!(place("user@127.0.0.1", &reaches, &home()).gateway, None);
    }

    #[test]
    fn a_gateway_name_is_a_checked_name_and_a_reach_reads_back_the_same() {
        assert!(GatewayName::new("office_2").is_ok());
        for bad in ["", "Office", "has space", "9lives", &"a".repeat(65)] {
            assert!(GatewayName::new(bad).is_err(), "{bad}");
        }
        let original = reach(
            home(),
            vec![ReachEntry::through("app.example.com", gateway("office"))],
        );
        let json = serde_json::to_string(&original).unwrap();
        assert!(!json.contains("192.168"), "{json}");
        let back: Reach = serde_json::from_str(&json).unwrap();
        assert_eq!(back, original);
        // A stored gateway name that is no name is refused on reading.
        let tampered = json.replace("office", "Not A Name");
        assert!(serde_json::from_str::<Reach>(&tampered).is_err());
    }
}
