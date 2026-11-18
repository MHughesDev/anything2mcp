//! The one error every reader returns, and its failure codes.

use a2m_core::{Failure, FailureClass, FailureCode};

/// Which fact a reader was reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Fact {
    /// The interfaces and their addresses.
    Interfaces,
    /// The routes.
    Routes,
    /// The name servers and search domains.
    Resolvers,
    /// The neighbour table.
    Neighbours,
    /// The listening sockets.
    Listeners,
    /// The proxy setting.
    Proxy,
}

impl Fact {
    /// The fact's name in a message.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Interfaces => "the interfaces",
            Self::Routes => "the routes",
            Self::Resolvers => "the name servers",
            Self::Neighbours => "the neighbour table",
            Self::Listeners => "the listening sockets",
            Self::Proxy => "the proxy setting",
        }
    }
}

/// Why a reader gave no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// The system refused to give the fact to this user.
    Refused {
        /// The fact.
        fact: Fact,
    },
    /// No reader for this fact is built for this operating system.
    NotBuilt {
        /// The fact.
        fact: Fact,
        /// The operating system, as Rust names it.
        system: &'static str,
    },
    /// The system answered in a shape this reader does not know.
    Unreadable {
        /// The fact.
        fact: Fact,
        /// What was wrong, in words that carry no value the system gave.
        what: &'static str,
    },
    /// The system's call failed for another reason.
    Failed {
        /// The fact.
        fact: Fact,
        /// The system's error number, where it gave one.
        code: Option<i32>,
    },
}

impl NetError {
    /// `edge_blocked`: the system refused this fact to this user.
    pub const REFUSED: FailureCode = crate::codes::NET_FACT_REFUSED;
    /// `unknown_variant`: no reader is built for this system.
    pub const NOT_BUILT: FailureCode = crate::codes::NET_READER_NOT_BUILT;
    /// `unknown_variant`: the system's answer has a shape this reader does
    /// not know.
    pub const UNREADABLE: FailureCode = crate::codes::NET_ANSWER_UNREADABLE;
    /// `transient`: the system's call failed.
    pub const FAILED: FailureCode = crate::codes::NET_READ_FAILED;

    /// The fact the error is about.
    #[must_use]
    pub fn fact(&self) -> Fact {
        match self {
            Self::Refused { fact }
            | Self::NotBuilt { fact, .. }
            | Self::Unreadable { fact, .. }
            | Self::Failed { fact, .. } => *fact,
        }
    }

    /// The error from an I/O error: a permission error is a refusal.
    pub(crate) fn from_io(fact: Fact, error: &std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            Self::Refused { fact }
        } else {
            Self::Failed {
                fact,
                code: error.raw_os_error(),
            }
        }
    }
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(Failure::from(self.clone()).message())
    }
}

impl std::error::Error for NetError {}

impl From<NetError> for Failure {
    fn from(error: NetError) -> Self {
        let name = error.fact().name();
        match error {
            NetError::Refused { .. } => Self::new(
                FailureClass::EdgeBlocked,
                NetError::REFUSED,
                format!("The operating system refused to give {name} to this user."),
                "Nothing was changed. The fact is not available to this user; a person may read it in the system's own network settings.",
            ),
            NetError::NotBuilt { system, .. } => Self::new(
                FailureClass::UnknownVariant,
                NetError::NOT_BUILT,
                format!("No reader of {name} is built for {system}."),
                "Read the fact in the system's own network settings; this reader is not built for this system yet.",
            ),
            NetError::Unreadable { what, .. } => Self::new(
                FailureClass::UnknownVariant,
                NetError::UNREADABLE,
                format!(
                    "The system's answer for {name} had a shape this reader does not know: {what}."
                ),
                "Report the operating system and its version; the reader stops rather than guess.",
            ),
            NetError::Failed { code, .. } => Self::new(
                FailureClass::Transient,
                NetError::FAILED,
                match code {
                    Some(code) => format!("Reading {name} failed with system error {code}."),
                    None => format!("Reading {name} failed."),
                },
                "Try again; if it fails again, report the operating system and the error number.",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_error_maps_to_one_class_and_code() {
        let cases = [
            (
                NetError::Refused {
                    fact: Fact::Listeners,
                },
                FailureClass::EdgeBlocked,
                "net_fact_refused",
            ),
            (
                NetError::NotBuilt {
                    fact: Fact::Proxy,
                    system: "example",
                },
                FailureClass::UnknownVariant,
                "net_reader_not_built",
            ),
            (
                NetError::Unreadable {
                    fact: Fact::Routes,
                    what: "a short header",
                },
                FailureClass::UnknownVariant,
                "net_answer_unreadable",
            ),
            (
                NetError::Failed {
                    fact: Fact::Neighbours,
                    code: Some(5),
                },
                FailureClass::Transient,
                "net_read_failed",
            ),
        ];
        for (error, class, code) in cases {
            let failure = Failure::from(error);
            assert_eq!(failure.class(), class);
            assert_eq!(failure.code().as_str(), code);
        }
    }

    #[test]
    fn a_permission_error_is_a_refusal() {
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(
            NetError::from_io(Fact::Listeners, &denied),
            NetError::Refused {
                fact: Fact::Listeners
            }
        );
    }
}
