//! How a system is reached: the seven access kinds, which are fixed, and modes,
//! which arrive with packs.

use std::fmt;
use std::str::FromStr;

use crate::name::{InvalidName, check_name};
use crate::vocabulary::vocabulary;

vocabulary! {
    /// One of the seven ways of reaching a system (spec section 201).
    ///
    /// An access kind fixes how a step is prepared, acted, confirmed, and read
    /// back. Every mode belongs to exactly one.
    pub enum AccessKind: "access kind" {
        /// The elements a person would use, found by role and name.
        Interface => "interface",
        /// A typed request over a protocol, and its answer.
        Request => "request",
        /// Starting a program with arguments and reading its status and output.
        Command => "command",
        /// Code running inside the host, through an interface the host offers.
        Bridge => "bridge",
        /// Reading and writing a file in a known format. Written as
        /// `artifact`, its old name, which manifests and stored rows already
        /// carry (decision 0402).
        File => "artifact",
        /// Waiting for, or adding to, a flow of events.
        Stream => "stream",
        /// Exchanging frames with hardware over a bus or a link.
        Device => "device",
    }
}

/// The name of a mode, such as `ui` or `http`.
///
/// A mode is one concrete way of reaching a system, supplied by a pack or a
/// plug-in and belonging to one [`Access kind`]. The kernel does not know the
/// list of modes: a provider registers each one it brings, and a binding that
/// names a mode nobody registered does not compile (decision 0060). This type
/// guarantees only that the name is well formed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mode(String);

impl Mode {
    /// The longest a mode's name may be.
    pub const LONGEST: usize = 32;

    /// Checks `name` and makes a mode of it.
    ///
    /// # Errors
    ///
    /// [`InvalidName`] when the name is not lowercase letters, digits, and
    /// underscores starting with a letter, or is longer than
    /// [`Mode::LONGEST`].
    pub fn new(name: &str) -> Result<Self, InvalidName> {
        check_name("mode", name, Self::LONGEST)?;
        Ok(Self(name.to_owned()))
    }

    /// The name, as written in a binding.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Mode {
    type Err = InvalidName;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::new(text)
    }
}

impl serde::Serialize for Mode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for Mode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{AccessKind, Mode};

    #[test]
    fn a_mode_keeps_the_name_it_was_given() {
        let mode = Mode::new("page_http").unwrap();
        assert_eq!(mode.as_str(), "page_http");
        assert_eq!(mode.to_string(), "page_http");
        assert_eq!("page_http".parse::<Mode>().unwrap(), mode);
    }

    #[test]
    fn a_badly_formed_mode_is_refused() {
        for bad in ["", "UI", "page-http", "a mode", &"m".repeat(33)] {
            assert!(Mode::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_mode_is_read_and_written_as_its_name() {
        let mode = Mode::new("ui").unwrap();
        assert_eq!(serde_json::to_string(&mode).unwrap(), "\"ui\"");
        assert_eq!(serde_json::from_str::<Mode>("\"ui\"").unwrap(), mode);
        let error = serde_json::from_str::<Mode>("\"Not A Mode\"").unwrap_err();
        assert!(
            error.to_string().contains("not a valid mode name"),
            "{error}"
        );
    }

    #[test]
    fn there_are_seven_access_kinds() {
        assert_eq!(AccessKind::ALL.len(), 7);
    }
}
