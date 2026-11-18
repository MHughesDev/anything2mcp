//! A destination's scope is read from its address when the set is made, and
//! the allow-list decides exactly as it did before it carried one (decision
//! 0240).

use a2m_core::{AddressScope, ScopedHost};
use a2m_provider::Destinations;

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
    assert!(destinations.iter().eq({
        let mut sorted = addresses.clone();
        sorted.sort();
        sorted.dedup();
        sorted
    }
    .iter()
    .map(String::as_str)));
}

#[test]
fn the_allow_list_still_compares_addresses_as_written_whatever_their_scope() {
    let destinations = Destinations::new(["http://127.0.0.1:8080".to_owned()]);
    // The same scope, written another way, is not a destination.
    for other in ["http://127.0.0.1:8080/", "http://127.1:8080", "127.0.0.1:8080"] {
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
