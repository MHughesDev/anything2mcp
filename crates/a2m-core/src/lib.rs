//! Shared types for Anything2MCP: the closed vocabularies, failures, and the secret wrapper.
//!
//! Everything here is the same whatever kind of system is being integrated.
//! A word with a fixed set of values is an enum whose names are the ones the
//! design documents use; `tests/documents.rs` holds the two together. A word
//! whose values arrive with packs, such as a mode, is a checked name.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: none.

mod vocabulary;

mod effect;
mod evidence;
mod failure;
mod kind;
mod name;
mod secret;
mod state;

pub use effect::{EffectClass, Hazard};
pub use evidence::{Coverage, EvidenceLevel};
pub use failure::{Failure, FailureClass, FailureCode};
pub use kind::{Archetype, Mode};
pub use name::{InvalidName, UnknownName};
pub use secret::Secret;
pub use state::{BindingStatus, RecordState, RunState, SessionState};
