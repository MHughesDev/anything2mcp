//! The machine's network facts as actions (decision 0260): each answers an
//! agent acting for the local user with a recorded decision, fits its
//! published output schema, and serves nothing while the kernel is stopped.

use std::net::{IpAddr, Ipv4Addr};

use a2m_actions::{BuiltInToolset, HealthState};
use a2m_core::{AddressScope, FailureClass};
use a2m_netstate::{
    Absent, AbsentReason, AddressStanding, Cut, Family, HardwareAddress, Interface,
    InterfaceAddress, InterfaceKind, InterfaceStanding, Interfaces, LinkState, Listener, Listeners,
    Neighbour, NeighbourState, Neighbours, Owner, Proxy, ProxySetting, ResolverSetting, Resolvers,
    Route, Routes, Standing, Transport,
};
use serde_json::{Value as Json, json};

use super::kernel::{Options, World, world};

const NET: [&str; 7] = [
    "net.interfaces",
    "net.routes",
    "net.resolvers",
    "net.neighbours",
    "net.listeners",
    "net.proxy",
    "net.standing",
];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_network_action_answers_an_agent_with_a_decision_and_fits_its_schema() {
    let world = World::ready().await;
    // The world's caller is an agent at the MCP door acting for the local
    // user: on one machine it may read the machine's own networks.
    assert!(world.caller.is_agent);
    for action in NET {
        let response = world.call(action, json!({})).await;
        assert!(response.decision.is_some(), "{action}: no decision");
        let not_built_here =
            cfg!(target_os = "macos") && matches!(action, "net.listeners" | "net.proxy");
        if not_built_here {
            let error = response.error.expect("an error");
            assert_eq!(error.class, FailureClass::UnknownVariant, "{action}");
            assert_eq!(error.code, "net_reader_not_built", "{action}");
            continue;
        }
        assert!(response.ok, "{action}: {:?}", response.error);
        let result = response.result.unwrap();
        let def = world.kernel.actions().find(action).unwrap();
        def.output
            .check_json(&result)
            .unwrap_or_else(|misfits| panic!("{action}: {misfits}\n{result}"));
        if action == "net.standing" {
            // This machine talks to itself, and the reading says so.
            let scopes: Vec<&Json> = result["interfaces"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|interface| interface["addresses"].as_array().unwrap())
                .map(|address| &address["scope"])
                .collect();
            assert!(scopes.contains(&&json!("this_machine")), "{result}");
            let next = response.next.unwrap();
            assert_eq!(next.action, "action.run");
            assert_eq!(next.args["action"], "net.interfaces");
        }
    }
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

#[test]
fn only_net_standing_is_listed_and_every_one_is_for_the_local_user() {
    let registry = a2m_actions::ActionRegistry::kernel_only();
    let listed: Vec<&str> = registry
        .toolset(BuiltInToolset::Use)
        .iter()
        .map(|def| def.name.as_str())
        .collect();
    for action in NET {
        let def = registry.find(action).unwrap();
        assert_eq!(def.who.as_str(), "Local user", "{action}");
        assert_eq!(
            def.effect,
            a2m_actions::Effect::Fixed(a2m_core::EffectClass::Read),
            "{action}"
        );
        assert!(def.input.fields.is_empty(), "{action}");
        assert_eq!(
            listed.contains(&action),
            action == "net.standing",
            "{action}"
        );
    }
    assert!(registry.past_cap(BuiltInToolset::Use).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stopped_kernel_serves_no_network_fact() {
    // With the default health need, a stopped kernel refuses as it refuses
    // every read: it serves only the engine's own status (decision 0260).
    let world = world(Options::with_blocked_store()).await;
    assert_eq!(world.kernel.health().state, HealthState::Stopped);
    for action in NET {
        let response = world.call(action, json!({})).await;
        let error = response
            .error
            .unwrap_or_else(|| panic!("{action} answered while stopped"));
        assert_eq!(
            (error.class, error.code.as_str()),
            (FailureClass::Busy, "engine_degraded"),
            "{action}"
        );
        assert!(response.result.is_none(), "{action}");
    }
    world.kernel.close(a2m_actions::CloseReason::Asked).await;
}

// Every value of every word list the readers write, so that a word added to
// `a2m_netstate` and not to the catalog fails here. Each list is matched
// whole, with no wildcard, so that a new value does not compile until it is
// listed.

fn kinds() -> Vec<InterfaceKind> {
    use InterfaceKind::{Loopback, Other, Tunnel, Virtual, Wired, Wireless};
    let all = vec![Loopback, Wired, Wireless, Virtual, Tunnel, Other];
    for kind in &all {
        match kind {
            Loopback | Wired | Wireless | Virtual | Tunnel | Other => {}
        }
    }
    all
}

fn reasons() -> Vec<AbsentReason> {
    use AbsentReason::{
        MayHoldCredentials, NeedsLocationPermission, NotRead, NotStated, NotVisibleToThisUser,
    };
    let all = vec![
        NotStated,
        NeedsLocationPermission,
        NotVisibleToThisUser,
        NotRead,
        MayHoldCredentials,
    ];
    for reason in &all {
        match reason {
            NotStated
            | NeedsLocationPermission
            | NotVisibleToThisUser
            | NotRead
            | MayHoldCredentials => {}
        }
    }
    all
}

fn neighbour_states() -> Vec<NeighbourState> {
    use NeighbourState::{
        Delay, Failed, Incomplete, NoArp, Permanent, Probe, Reachable, Stale, Unreachable,
    };
    let all = vec![
        Incomplete,
        Reachable,
        Stale,
        Delay,
        Probe,
        Failed,
        NoArp,
        Permanent,
        Unreachable,
    ];
    for state in &all {
        match state {
            Incomplete | Reachable | Stale | Delay | Probe | Failed | NoArp | Permanent
            | Unreachable => {}
        }
    }
    all
}

fn transports() -> Vec<Transport> {
    let all = vec![Transport::Tcp, Transport::Udp];
    for transport in &all {
        match transport {
            Transport::Tcp | Transport::Udp => {}
        }
    }
    all
}

fn families() -> Vec<Family> {
    let all = vec![Family::Ipv4, Family::Ipv6];
    for family in &all {
        match family {
            Family::Ipv4 | Family::Ipv6 => {}
        }
    }
    all
}

fn links() -> Vec<LinkState> {
    let all = vec![LinkState::Up, LinkState::Down];
    for link in &all {
        match link {
            LinkState::Up | LinkState::Down => {}
        }
    }
    all
}

fn fits(action: &str, reading: &impl serde::Serialize) {
    let registry = a2m_actions::ActionRegistry::kernel_only();
    let value = serde_json::to_value(reading).unwrap();
    registry
        .find(action)
        .unwrap()
        .output
        .check_json(&value)
        .unwrap_or_else(|misfits| panic!("{action}: {misfits}\n{value}"));
}

#[test]
fn every_word_a_reader_writes_is_one_its_schema_takes() {
    let here = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let absent: Vec<Absent> = reasons()
        .into_iter()
        .map(|reason| Absent {
            field: "example",
            reason,
        })
        .collect();
    let cut = vec![Cut {
        list: "example",
        kept: 1,
        seen: 2,
        table_cut: true,
    }];
    let route = |family| Route {
        family,
        destination: here,
        prefix_length: 0,
        gateway: Some(here),
        interface_index: Some(1),
        interface: Some("lo".to_owned()),
        metric: Some(100),
    };
    let routes: Vec<Route> = families().into_iter().map(route).collect();

    let interfaces = Interfaces {
        interfaces: kinds()
            .into_iter()
            .zip(links().into_iter().cycle())
            .map(|(kind, state)| Interface {
                index: 1,
                name: "lo".to_owned(),
                kind: Some(kind),
                system_kind: Some("example".to_owned()),
                state: Some(state),
                mtu: Some(65_536),
                hardware_address: HardwareAddress::from_octets(&[2, 0, 0, 0, 0, 1]),
                addresses: vec![InterfaceAddress {
                    address: here,
                    prefix_length: Some(8),
                }],
                absent: absent.clone(),
            })
            .collect(),
        read_from: vec!["example"],
        cut: cut.clone(),
    };
    fits("net.interfaces", &interfaces);
    fits(
        "net.routes",
        &Routes {
            routes: routes.clone(),
            default_routes: routes.clone(),
            read_from: vec!["example"],
            cut: cut.clone(),
        },
    );
    fits(
        "net.resolvers",
        &Resolvers {
            settings: vec![ResolverSetting {
                from: "example".to_owned(),
                name_servers: vec![here],
                search_domains: vec!["example.com".to_owned()],
            }],
            read_from: vec!["example"],
            absent: absent.clone(),
            cut: cut.clone(),
        },
    );
    fits(
        "net.neighbours",
        &Neighbours {
            neighbours: neighbour_states()
                .into_iter()
                .map(|state| Neighbour {
                    address: here,
                    interface_index: Some(1),
                    interface: Some("lo".to_owned()),
                    hardware_address: HardwareAddress::from_octets(&[2, 0, 0, 0, 0, 1]),
                    state: Some(state),
                })
                .collect(),
            read_from: vec!["example"],
            absent: absent.clone(),
            cut: cut.clone(),
        },
    );
    let mut listeners = Vec::new();
    for transport in transports() {
        for scope in AddressScope::ALL {
            listeners.push(Listener {
                transport,
                address: here,
                port: 65_535,
                scope: *scope,
                owner: Some(Owner {
                    process_id: 1,
                    program: Some("example".to_owned()),
                }),
                owner_absent: None,
            });
        }
        for reason in reasons() {
            listeners.push(Listener {
                transport,
                address: here,
                port: 0,
                scope: AddressScope::ThisMachine,
                owner: None,
                owner_absent: Some(reason),
            });
        }
    }
    fits(
        "net.listeners",
        &Listeners {
            listeners,
            read_from: vec!["example"],
            cut: cut.clone(),
        },
    );
    fits(
        "net.proxy",
        &Proxy {
            settings: vec![ProxySetting {
                from: "http_proxy".to_owned(),
                applies_to: "http".to_owned(),
                value: "http://proxy.example:3128".to_owned(),
                credentials_removed: true,
            }],
            detects_automatically: Some(false),
            read_from: vec!["example"],
            absent,
            cut: cut.clone(),
        },
    );
    fits(
        "net.standing",
        &Standing {
            interfaces: kinds()
                .into_iter()
                .zip(links().into_iter().cycle())
                .map(|(kind, state)| InterfaceStanding {
                    name: "lo".to_owned(),
                    kind: Some(kind),
                    state: Some(state),
                    default_route_for: families(),
                    addresses: AddressScope::ALL
                        .iter()
                        .map(|scope| AddressStanding {
                            address: here,
                            prefix_length: Some(128),
                            scope: *scope,
                        })
                        .collect(),
                })
                .collect(),
            default_routes: routes,
            cut,
        },
    );
}
