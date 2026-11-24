//! The session keeper and the auth providers.
//!
//! The [`Keeper`] is the only code that signs in, checks, renews, and ends a
//! session. It hands the engine a [`SessionLease`] and never hands anyone a
//! credential: credentials reach a driver only through the lease's `apply`,
//! which has the session's provider place them into the driver's own client.
//!
//! The keeper has two halves. Its decisions are made where the session's
//! record is: which sessions are wanted, when to check, when to ask a
//! person. Its work runs on the session's home runner, through the
//! [`KeeperWork`] it is handed, where [`KeeperWorker`] is the only caller of
//! an [`AuthProvider`]. The keeper never calls a crate above it.
//!
//! The providers that only hold or exchange a credential live here, in
//! [`providers`]. A provider that needs a surface of the system lives in the
//! pack that owns the surface, and implements [`AuthProvider`].
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: layers 1 to 3.

pub mod codes;
mod credentials;
mod encode;
mod failure;
mod keeper;
mod key;
mod lease;
mod link;
mod material;
mod name;
mod person;
mod provider;
pub mod providers;
mod recommend;
mod sink;
mod state;
mod store;
mod surfaces;
mod tables;
#[cfg(test)]
mod tests;
pub mod tls;
mod work;

pub use credentials::{Credentials, StoredCredentials};
pub use keeper::{
    AUTOMATIC_SIGN_IN_WINDOW, AUTOMATIC_SIGN_INS, CHECKS_FAILED, EndedBy, FORCED_SIGN_INS, Keeper,
    KeeperBuilder, KeeperSettings, RENEWAL_DURATION, RENEWALS, SessionStatus, SignInBy,
    SignInTicket, WeakKeeper,
};
pub use key::{AuthEntry, SessionKey, SessionRef};
pub use lease::{LeaseRequest, Purpose, SessionGrant, SessionLease, Wait};
pub use material::Material;
pub use name::{CredentialRef, ProfileRef, ProviderName};
pub use person::{AskPerson, PersonAnswer, PersonAsk, PersonReason, Returned};
pub use provider::{
    AuthProvider, ExpirySign, ProviderNeeds, ProviderSet, RawSign, Rung, SignInAddress,
    SignInContext, SignedIn, sign_in_addresses,
};
pub use recommend::{Choice, INFERRED, Recommendation, SIGN_IN_METHOD, SignInSign, recommend};
pub use sink::{ChallengeScheme, CredentialSink};
pub use state::{CallRule, call_rule, legal_move};
pub use store::{EventKind, SessionEvent, SessionRow, SessionStore};
pub use surfaces::{EntrySurfaces, SignInDestinations};
pub use work::{
    InProcessWork, KeeperJob, KeeperWork, KeeperWorker, ProbeOutcome, RunProbe, Work, WorkOutcome,
};

/// The boxed future the traits of this crate return, so that each can be
/// used as a trait object. It is the vault's, shared so that one type serves
/// both crates.
pub use a2m_vault::BoxFuture;
