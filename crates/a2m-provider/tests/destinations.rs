//! A destination's scope is read from its address when the set is made, and
//! the allow-list decides exactly as it did before it carried one (decision
//! 0240).

use a2m_core::{AddressScope, ScopedHost};
use a2m_provider::{Destinations, MachineState, NetworkPlace};

/// The shared corpus of address spellings.
const SPELLINGS: &str = include_str!("../../a2m-core/tests/address-spellings.txt");

fn spellings() -> Vec<String> {
    SPELLINGS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| line.split_whitespace().next())
        .map(|spelling| spelling.replace("<empty>", "").replace("<sp>", " "))
        .collect()
}

#[test]
fn each_destination_carries_the_scope_its_address_gives_and_nothing_else() {
    let mut addresses = spellings();
    for spelling in spellings() {
        addresses.push(format!("http://{spelling}/"));
        addresses.push(format!("https://{spelling}"));
    }
    let destinations = Destinations::new(addresses.clone());
    for address in &addresses {
        assert!(destinations.contains(address), "{address}");
        assert_eq!(
            destinations.scope_of(address),
            Some(&ScopedHost::of_destination(address)),
            "{address}"
        );
    }
    assert!(
        destinations.iter().eq({
            let mut sorted = addresses.clone();
            sorted.sort();
            sorted.dedup();
            sorted
        }
        .iter()
        .map(String::as_str))
    );
}

#[test]
fn the_allow_list_still_compares_addresses_as_written_whatever_their_scope() {
    let destinations = Destinations::new(["http://127.0.0.1:8080".to_owned()]);
    // The same scope, written another way, is not a destination.
    for other in [
        "http://127.0.0.1:8080/",
        "http://127.1:8080",
        "127.0.0.1:8080",
    ] {
        assert!(!destinations.contains(other), "{other}");
        assert_eq!(destinations.scope_of(other), None, "{other}");
    }
    assert_eq!(
        destinations
            .scope_of("http://127.0.0.1:8080")
            .map(ScopedHost::scope),
        Some(AddressScope::ThisMachine)
    );
    assert_eq!(Destinations::default().iter().count(), 0);
}

#[test]
fn every_destination_of_the_corpus_has_a_place_derived_from_its_scope_and_the_machine() {
    let mut addresses = spellings();
    for spelling in spellings() {
        addresses.push(format!("http://{spelling}/"));
    }
    let destinations = Destinations::new(addresses.clone());
    let attached = MachineState::of_subnets([
        ("192.168.1.20".parse().unwrap(), 24),
        ("fe80::1".parse().unwrap(), 64),
    ]);
    let mut unknown = 0;
    for state in [MachineState::unread(), attached] {
        for address in &addresses {
            let placed = destinations.place_of(address, &state).expect("declared");
            let host = ScopedHost::of_destination(address);
            // This machine is a loopback address written plainly, and
            // nothing else: no name, no zone, no IPv4 inside IPv6.
            assert_eq!(
                placed.place == NetworkPlace::ThisMachine,
                host.scope() == AddressScope::ThisMachine && !host.may_leave_this_machine(),
                "{address}"
            );
            // A name is never resolved: it has no place but `unknown`.
            if matches!(host.scope(), AddressScope::Name | AddressScope::Unreadable) {
                assert_eq!(placed.place, NetworkPlace::Unknown, "{address}");
            }
            assert!(placed.place.is_direct(), "{address}");
            assert!(!placed.because.is_empty(), "{address}");
            unknown += usize::from(placed.place == NetworkPlace::Unknown);
        }
    }
    // The count H-43 asks for: some spellings are `unknown`, and not all.
    assert!(unknown > 0 && unknown < addresses.len() * 2);
    // An address that is not a destination has no place.
    assert_eq!(
        destinations.place_of("http://not-declared.example.test/", &MachineState::unread()),
        None
    );
}
