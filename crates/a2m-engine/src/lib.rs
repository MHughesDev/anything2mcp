//! The engine loop: one job in, one outcome out.
//!
//! The engine carries out a compiled plan through the eight parts of every
//! operation call: guard, choose, prepare, intend, act, confirm, read back,
//! record. It chooses among the plan's bindings in their declared order,
//! normalizes and compares values, builds the diff of a write and its hash,
//! reconciles before it writes on resume, and gives discoverers a sandbox
//! through which a write is impossible.
//!
//! It names no mode, driver, or pack: it asks the registry in
//! `a2m-provider` for the driver of a binding's mode and reaches it only
//! through the handle. It opens no store, calls no policy engine, and reads
//! no target file: everything it needs from outside its loop it asks for
//! through [`Control`]. Nothing in a run calls a model or improvises; a
//! step that cannot be done as written stops with a named failure class.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: layers 1 to 5, except the
//! compiler, the policy engine, and the scheduler.

pub mod codes;
mod compare;
mod control;
mod diff;
mod engine;
mod guard;
mod job;
mod loaded;
mod run;
mod sandbox;
mod tool;
mod values;

/// The boxed future every method of [`Control`] returns: the same type the
/// provider contract uses.
pub use a2m_provider::BoxFuture;

pub use compare::Comparison;
pub use control::{
    Clearance, Control, EngineEvent, Evidence, GuardQuestion, PassedOver, Preview, SessionWant,
};
pub use diff::{Diff, DiffAction, DiffCounts, DiffField, DiffRecord};
pub use engine::{Engine, EngineDeps, EngineSettings, ExclusiveNeed};
pub use job::{
    Conditions, Directive, JobKind, JobOutcome, JobSpec, KernelHealth, LeaseClock, Parallelism,
    RecordSnapshot, RunSnapshot, ToolRef,
};
pub use loaded::CURRENT;
pub use values::{decode_values, encode_values};
