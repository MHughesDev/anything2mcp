//! The checked names this crate adds to those of `a2m-core`.

use std::fmt;

use a2m_core::Failure;

use crate::codes::{self, shown};

const fn is_name(text: &str, longest: usize) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > longest || !bytes[0].is_ascii_lowercase() {
        return false;
    }
    let mut i = 1;
    while i < bytes.len() {
        let b = bytes[i];
        if !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_') {
            return false;
        }
        i += 1;
    }
    true
}

fn refused(kind: &str, text: &str) -> Failure {
    codes::failure(
        &codes::row::MANIFEST_INVALID,
        format!("`{}` is not a valid {kind} name.", shown(text)),
        "Write the name in lowercase letters, digits, and underscores, starting with a letter.",
    )
}

macro_rules! plain_name {
    ($(#[$doc:meta])* $name:ident, $kind:literal, $longest:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// The longest the name may be.
            pub const LONGEST: usize = $longest;

            /// Checks `name` and keeps it.
            ///
            /// # Errors
            ///
            /// `input`/`manifest_invalid` when the name is not lowercase
            /// letters, digits, and underscores starting with a letter.
            pub fn new(name: &str) -> Result<Self, Failure> {
                if is_name(name, Self::LONGEST) {
                    Ok(Self(name.to_owned()))
                } else {
                    Err(refused($kind, name))
                }
            }

            /// The name as written.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

plain_name!(
    /// The name of a provider: a pack or a plug-in. It is the name space of
    /// its actions and lint codes, and the key of its settings group. Not to
    /// be confused with the name of an auth provider, which is `a2m-auth`'s.
    ProviderName,
    "provider",
    32
);

plain_name!(
    /// The name of a root: a folder the kernel resolves, such as `project`.
    /// A manifest and a caller name roots; neither ever names a free path.
    RootName,
    "root",
    32
);

plain_name!(
    /// A kind of subsystem, such as `browser` or `command`. The supervisor
    /// counts running subsystems by kind.
    SubsystemKind,
    "subsystem kind",
    32
);

plain_name!(
    /// The name of a discoverer within its provider.
    DiscovererName,
    "discoverer",
    32
);

/// The name of an action a provider adds: the provider's name, one dot, and
/// the action, such as `example_sheet.inspect`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionName(String);

impl ActionName {
    /// Checks `name` and keeps it.
    ///
    /// # Errors
    ///
    /// `input`/`manifest_invalid` when the text is not two names joined by
    /// one dot.
    pub fn new(name: &str) -> Result<Self, Failure> {
        let well_formed = name
            .split_once('.')
            .is_some_and(|(owner, action)| is_name(owner, 32) && is_name(action, 32));
        if well_formed {
            Ok(Self(name.to_owned()))
        } else {
            Err(refused("action", name))
        }
    }

    /// The name as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The provider whose name space the action is in.
    #[must_use]
    pub fn provider(&self) -> &str {
        self.0.split_once('.').map_or("", |(owner, _)| owner)
    }
}

impl fmt::Display for ActionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
