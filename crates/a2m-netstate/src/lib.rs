//! What this machine already knows about its own place on its networks, read
//! from the operating system as an ordinary user, with nothing sent to any
//! other machine (decision 0243).
//!
//! It is what a person sees in the system's network settings: the
//! interfaces and their addresses, the routes and the default routes, the
//! name servers and search domains, the neighbour table the system already
//! holds, the sockets listening on this machine and their owners, and the
//! proxy setting the system states. And a summary: for each interface, the
//! kind of place each of its addresses is ([`a2m_core::AddressScope`]).
//!
//! **Nothing is sent.** Every reader calls only the system's read
//! interfaces, listed in the crate's design. No reader resolves a name,
//! sends an echo request, opens a connection, or refreshes a table by
//! asking. On Linux the one message sent goes to this machine's kernel over
//! route-netlink and asks it to list a table. A test scans this crate's
//! source for any call that connects, sends, or resolves; another, on
//! Linux, runs every reader inside a network namespace that holds only a
//! loopback interface.
//!
//! **Unknown is never guessed** (rule 7). A field the system does not state
//! is absent. A reader not built for the system is [`NetError::NotBuilt`],
//! a fact the system refuses is [`NetError::Refused`], and an answer in a
//! shape the reader does not know is [`NetError::Unreadable`].
//!
//! **Every list is bounded** ([`bound`]); a reading names each list a bound
//! cut.
//!
//! **Privacy** (decision 0243): a hardware address is serialized as its
//! length and whether it is locally administered, never its octets; no host
//! name is read; the wireless network's name is not read; a program is named
//! by its file name, never its command line; credentials in a proxy setting
//! are removed before the value is kept.
//!
//! Every reader blocks on system calls and file reads; an async caller runs
//! it on a blocking thread.

pub mod bound;
mod error;
mod model;
mod sys;
pub mod text;

use std::collections::BTreeMap;

use a2m_core::ScopedHost;

pub use bound::Cut;
pub use error::{Fact, NetError};
pub use model::{
    Absent, AbsentReason, AddressStanding, Family, HardwareAddress, Interface, InterfaceAddress,
    InterfaceKind, InterfaceStanding, Interfaces, LinkState, Listener, Listeners, Neighbour,
    NeighbourState, Neighbours, Owner, Proxy, ProxySetting, ResolverSetting, Resolvers, Route,
    Routes, Standing, Transport,
};

/// The interfaces and their addresses, ordered by index, each interface's
/// addresses in order.
///
/// # Errors
///
/// [`NetError`] when the system refuses, has no reader here, or answers in
/// an unknown shape.
pub fn interfaces() -> Result<Interfaces, NetError> {
    let mut read = sys::os::interfaces()?;
    read.interfaces.sort_by_key(|i| i.index);
    bound::keep(
        &mut read.interfaces,
        bound::MOST_INTERFACES,
        "interfaces",
        &mut read.cut,
    );
    for interface in &mut read.interfaces {
        interface.addresses.sort();
        interface.addresses.dedup();
        bound::keep(
            &mut interface.addresses,
            bound::MOST_ADDRESSES,
            "addresses",
            &mut read.cut,
        );
        interface.absent.sort();
    }
    Ok(read)
}

/// The names of the interfaces by index, or none where they cannot be read.
fn names() -> BTreeMap<u32, String> {
    interfaces()
        .map(|read| {
            read.interfaces
                .into_iter()
                .map(|i| (i.index, i.name))
                .collect()
        })
        .unwrap_or_default()
}

/// The routes, ordered, with each interface's name where its index names
/// one, and the default routes of each family, lowest metric first.
///
/// # Errors
///
/// As [`interfaces`].
pub fn routes() -> Result<Routes, NetError> {
    let mut read = sys::os::routes()?;
    let names = names();
    for route in &mut read.routes {
        if route.interface.is_none() {
            route.interface = route.interface_index.and_then(|i| names.get(&i).cloned());
        }
    }
    read.routes.sort();
    read.routes.dedup();
    let mut defaults: Vec<Route> = read
        .routes
        .iter()
        .filter(|r| r.is_default())
        .cloned()
        .collect();
    defaults.sort_by_key(|r| (r.family, r.metric.is_none(), r.metric, r.clone()));
    read.default_routes = defaults;
    bound::keep(
        &mut read.routes,
        bound::MOST_ROUTES,
        "routes",
        &mut read.cut,
    );
    Ok(read)
}

/// The name servers and search domains, each setting in the order read.
///
/// # Errors
///
/// As [`interfaces`].
pub fn resolvers() -> Result<Resolvers, NetError> {
    let mut read = sys::os::resolvers()?;
    let most = bound::MOST_RESOLVER_ENTRIES;
    bound::keep(&mut read.settings, most, "settings", &mut read.cut);
    for setting in &mut read.settings {
        bound::keep(&mut setting.name_servers, most, "name_servers", &mut read.cut);
        bound::keep(
            &mut setting.search_domains,
            most,
            "search_domains",
            &mut read.cut,
        );
    }
    Ok(read)
}

/// The neighbour table the system already holds, ordered, with each
/// interface's name. Read only: no entry is refreshed by asking.
///
/// # Errors
///
/// As [`interfaces`].
pub fn neighbours() -> Result<Neighbours, NetError> {
    let mut read = sys::os::neighbours()?;
    let names = names();
    for neighbour in &mut read.neighbours {
        if neighbour.interface.is_none() {
            neighbour.interface = neighbour
                .interface_index
                .and_then(|i| names.get(&i).cloned());
        }
    }
    read.neighbours.sort();
    read.neighbours.dedup();
    bound::keep(
        &mut read.neighbours,
        bound::MOST_NEIGHBOURS,
        "neighbours",
        &mut read.cut,
    );
    Ok(read)
}

/// The sockets listening on this machine, ordered, each with its owner
/// where this user may see it.
///
/// # Errors
///
/// As [`interfaces`].
pub fn listeners() -> Result<Listeners, NetError> {
    let mut read = sys::os::listeners()?;
    read.listeners.sort_by(|a, b| {
        (a.transport, a.address, a.port, &a.owner).cmp(&(b.transport, b.address, b.port, &b.owner))
    });
    read.listeners.dedup();
    bound::keep(
        &mut read.listeners,
        bound::MOST_LISTENERS,
        "listeners",
        &mut read.cut,
    );
    Ok(read)
}

/// The proxy setting the system states. Read to be shown, never applied
/// (decision 0197).
///
/// # Errors
///
/// As [`interfaces`].
pub fn proxy() -> Result<Proxy, NetError> {
    let mut read = sys::os::proxy()?;
    bound::keep(
        &mut read.settings,
        bound::MOST_PROXY_SETTINGS,
        "settings",
        &mut read.cut,
    );
    Ok(read)
}

/// Where the machine stands, from two readings already made: for each
/// interface, the kind of place each of its addresses is, and which
/// interface carries each family's preferred default route.
#[must_use]
pub fn standing_of(interfaces: &Interfaces, routes: &Routes) -> Standing {
    let mut preferred: Vec<Route> = Vec::new();
    for route in &routes.default_routes {
        if !preferred.iter().any(|r| r.family == route.family) {
            preferred.push(route.clone());
        }
    }
    let standing = interfaces
        .interfaces
        .iter()
        .map(|interface| InterfaceStanding {
            name: interface.name.clone(),
            kind: interface.kind,
            state: interface.state,
            default_route_for: preferred
                .iter()
                .filter(|r| r.interface_index == Some(interface.index))
                .map(|r| r.family)
                .collect(),
            addresses: interface
                .addresses
                .iter()
                .map(|a| AddressStanding {
                    address: a.address,
                    prefix_length: a.prefix_length,
                    scope: ScopedHost::of_ip(a.address).scope(),
                })
                .collect(),
        })
        .collect();
    let mut cut = interfaces.cut.clone();
    cut.extend(routes.cut.iter().cloned());
    Standing {
        interfaces: standing,
        default_routes: preferred,
        cut,
    }
}

/// Where the machine stands now: [`interfaces`] and [`routes`], read, and
/// joined by [`standing_of`].
///
/// # Errors
///
/// As [`interfaces`], from either reading.
pub fn standing() -> Result<Standing, NetError> {
    Ok(standing_of(&interfaces()?, &routes()?))
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use a2m_core::AddressScope;

    use super::*;

    fn interface(index: u32, name: &str, addresses: &[&str]) -> Interface {
        Interface {
            index,
            name: name.to_owned(),
            kind: None,
            system_kind: None,
            state: Some(LinkState::Up),
            mtu: None,
            hardware_address: HardwareAddress::from_octets(&[0x02, 0, 0, 0, 0, 1]),
            addresses: addresses
                .iter()
                .map(|a| InterfaceAddress {
                    address: a.parse().unwrap(),
                    prefix_length: Some(24),
                })
                .collect(),
            absent: Vec::new(),
        }
    }

    fn default_route(index: u32, gateway: &str, metric: Option<u32>) -> Route {
        let gateway: IpAddr = gateway.parse().unwrap();
        Route {
            family: Family::of(&gateway),
            destination: if gateway.is_ipv4() {
                "0.0.0.0".parse().unwrap()
            } else {
                "::".parse().unwrap()
            },
            prefix_length: 0,
            gateway: Some(gateway),
            interface_index: Some(index),
            interface: None,
            metric,
        }
    }

    #[test]
    fn the_standing_names_each_addresses_place_and_the_preferred_default() {
        let interfaces = Interfaces {
            interfaces: vec![
                interface(1, "lo", &["127.0.0.1"]),
                interface(2, "example0", &["192.0.2.10", "10.0.0.2"]),
                interface(3, "example1", &["100.64.0.9"]),
            ],
            read_from: vec![],
            cut: vec![],
        };
        let routes = Routes {
            routes: vec![],
            default_routes: vec![
                default_route(2, "10.0.0.1", Some(100)),
                default_route(3, "100.64.0.1", Some(600)),
            ],
            read_from: vec![],
            cut: vec![],
        };
        let standing = standing_of(&interfaces, &routes);
        assert_eq!(standing.default_routes.len(), 1);
        assert_eq!(standing.interfaces[1].default_route_for, vec![Family::Ipv4]);
        assert!(standing.interfaces[2].default_route_for.is_empty());
        let scopes: Vec<Vec<AddressScope>> = standing
            .interfaces
            .iter()
            .map(|i| i.addresses.iter().map(|a| a.scope).collect())
            .collect();
        assert_eq!(
            scopes,
            vec![
                vec![AddressScope::ThisMachine],
                vec![AddressScope::Documentation, AddressScope::PrivateUse],
                vec![AddressScope::CarrierGradeNat],
            ]
        );
    }

    #[test]
    fn a_hardware_address_is_never_serialized_whole() {
        let one = interface(2, "example0", &["192.0.2.10"]);
        let json = serde_json::to_string(&one).unwrap();
        assert!(json.contains(r#""hardware_address":{"length":6,"locally_administered":true}"#));
        assert!(!json.contains("02:00"));
        assert!(!format!("{one:?}").contains("[2, 0, 0, 0, 0, 1]"));
        // The kernel keeps the octets.
        assert_eq!(
            one.hardware_address.unwrap().octets(),
            &[0x02, 0, 0, 0, 0, 1]
        );
        assert!(HardwareAddress::from_octets(&[0; 6]).is_none());
    }
}
