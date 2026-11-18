//! Asking the session's own person to do something only a person can do.
//!
//! The keeper never drives a person's browser and never asks for a password
//! in its own interface. It hands over a link the person opens themself, or
//! asks for a code, through [`AskPerson`]. The crate above joins that to the
//! role's person surface.

use std::collections::BTreeMap;
use std::fmt;

use a2m_core::{REDACTED, Secret};
use a2m_vault::BoxFuture;

use crate::key::SessionRef;

/// Why a person is wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PersonReason {
    /// The person must sign in.
    SignIn,
    /// The person must agree to the access being asked for.
    Consent,
    /// The identity provider demands a second factor.
    SecondFactor,
    /// A stored credential was refused, and someone must look at it.
    CredentialRefused,
}

impl PersonReason {
    /// The reason as the session timeline writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SignIn => "sign_in",
            Self::Consent => "consent",
            Self::SecondFactor => "second_factor",
            Self::CredentialRefused => "credential_refused",
        }
    }

    /// The reason a timeline word names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::SignIn,
            Self::Consent,
            Self::SecondFactor,
            Self::CredentialRefused,
        ]
        .into_iter()
        .find(|reason| reason.as_str() == name)
    }
}

/// One request to the session's own person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonAsk {
    /// A link the person opens in their own browser. Nothing drives that
    /// browser. It is answered when the browser comes back to the surface's
    /// return address, with the parameters it came back with.
    OpenLink {
        /// The address to open. It holds no credential.
        url: String,
        /// Why.
        reason: PersonReason,
    },
    /// A code the person reads somewhere else and enters on the surface. It
    /// is answered with the part `code`.
    EnterCode {
        /// Why.
        reason: PersonReason,
    },
    /// A notice that the session needs the person: the surface tells them
    /// and offers the way to sign in. It is answered as soon as they have
    /// been told.
    Notify {
        /// Why.
        reason: PersonReason,
    },
}

/// What came back from a person: named values, each kept as a secret. A
/// one-time authorization code is as good as a credential until it is spent.
#[derive(Default)]
pub struct Returned(BTreeMap<String, Secret>);

impl Returned {
    /// Nothing returned.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// With one more named value.
    #[must_use]
    pub fn with(mut self, name: &str, value: Secret) -> Self {
        self.0.insert(name.to_owned(), value);
        self
    }

    /// One named value.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Secret> {
        self.0.get(name)
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &Secret> {
        self.0.values()
    }

    /// Every named value, in order of name.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Secret)> {
        self.0.iter().map(|(name, value)| (name.as_str(), value))
    }

    /// Whether nothing was returned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The same values, keeping only those whose name `keep` accepts and
    /// that are not empty.
    #[must_use]
    pub fn only(self, keep: impl Fn(&str) -> bool) -> Self {
        Self(
            self.0
                .into_iter()
                .filter(|(name, value)| keep(name) && !value.expose().is_empty())
                .collect(),
        )
    }
}

impl fmt::Debug for Returned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Returned")
            .field("names", &self.0.keys().collect::<Vec<_>>())
            .field("values", &REDACTED)
            .finish()
    }
}

/// How a request to a person ended.
#[derive(Debug)]
pub enum PersonAnswer {
    /// The person did what was asked.
    Done(Returned),
    /// The person said no.
    Declined,
    /// Nobody answered within the surface's wait.
    NoAnswer,
    /// No surface could reach the person.
    Unreachable,
}

/// The way to the session's own person. `a2m-actions` implements it over the
/// role's person surface; a test implements it with a fixed answer.
///
/// Only the session's own identity is ever asked. An implementation shows
/// the link or the prompt and waits; it never opens, drives, or reads the
/// person's browser.
pub trait AskPerson: Send + Sync {
    /// Asks, and waits for the answer.
    fn ask<'a>(&'a self, session: &'a SessionRef, ask: PersonAsk) -> BoxFuture<'a, PersonAnswer>;

    /// The address a person's browser is sent back to after a link, where
    /// the surface listens: a loopback address on one machine, the console's
    /// address on a server. `None` when the surface cannot receive one.
    fn return_address(&self) -> Option<String> {
        None
    }
}
