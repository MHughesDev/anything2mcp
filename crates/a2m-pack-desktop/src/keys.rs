//! The keys an action may be done by, a closed list, and how each system
//! spells them (decision 0999).
//!
//! An action that states `key` is done as the element's own action that
//! the element binds to that key, as the accessibility interface lists it:
//! on Linux, the action of AT-SPI's action interface whose key binding is
//! the key. No synthetic key is ever sent: it would go to whatever window
//! has the focus, and cannot be bound to the element (decision 0218). A
//! key the element binds to no action, or to more than one, is a stop,
//! nothing sent. On Windows and macOS an action by a key stops before
//! anything is sent, as `keys_not_built`: neither interface lists the key
//! an element's action is bound to for a field.

/// The keys an action may state, by the pack's own words.
pub(crate) const KEYS: &[&str] = &["enter", "escape"];

/// How AT-SPI's key bindings spell each key: a binding names the key
/// alone, by one of these. A binding with a modifier is not one of them.
#[cfg(target_os = "linux")]
const SPELLINGS: &[(&str, &[&str])] = &[
    ("enter", &["Return", "<Return>", "KP_Enter", "<KP_Enter>"]),
    ("escape", &["Escape", "<Escape>"]),
];

/// Whether an action's key binding, as AT-SPI gives it (up to three
/// bindings apart by `;`: a mnemonic, a sequence, a shortcut), binds the
/// action to `key` alone.
#[cfg(target_os = "linux")]
pub(crate) fn binds(binding: &str, key: &str) -> bool {
    let Some((_, spelled)) = SPELLINGS.iter().find(|(word, _)| *word == key) else {
        return false;
    };
    binding
        .split(';')
        .map(str::trim)
        .any(|part| spelled.contains(&part))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_its_spellings_and_no_spelling_is_two_keys() {
        for key in KEYS {
            assert!(SPELLINGS.iter().any(|(word, _)| word == key), "{key}");
        }
        let mut seen = std::collections::BTreeSet::new();
        for (_, spelled) in SPELLINGS {
            for one in *spelled {
                assert!(seen.insert(*one), "{one}");
            }
        }
        assert!(binds("Return", "enter"));
        assert!(binds(";;<Return>", "enter"));
        assert!(!binds("<Control>Return", "enter"));
        assert!(!binds("Return", "escape"));
        assert!(!binds("", "enter"));
    }
}
