//! Every way into the kernel, defined once.
//!
//! An action is a name, an input schema, an output schema, the lowest role
//! that may use it, whether it is listed to agents, and a handler. This
//! crate holds the registry of actions ([`ActionRegistry`]), the dispatcher
//! that every front door calls ([`Kernel::dispatch`]), the settings and the
//! loader that layers them ([`Settings`]), the on-disk layout ([`Paths`]),
//! and the [`Kernel`] handle that joins the services below it. In one
//! process it is also the runner: a loop that claims jobs from the
//! scheduler and hands them to the engine.
//!
//! It decides nothing itself. Whether something may happen is the policy
//! engine's; signing in and renewing are the session keeper's; carrying
//! out a step is the engine's. Recording a person's approval is not an
//! action: the one handle that can do it is given to the role's
//! [`PersonSurface`] and to nothing a caller can reach.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: layers 1 to 6.

mod acts;
mod bounds;
mod catalog;
pub mod codes;
mod control;
mod def;
mod dispatch;
mod drift;
mod events;
mod evidence;
mod faces;
mod fixture;
mod handlers;
mod health;
pub mod init;
mod keeper;
mod kernel;
mod offline;
mod origins;
mod paths;
mod person;
mod places;
mod project;
mod registry;
mod render;
mod runner;
pub mod settings;
mod signals;
mod space;
mod tools;

/// An idempotency key, as the store keeps it (decision 0160).
pub use a2m_files::ReviewSummary;
pub use a2m_ledger::{CALLER_KEY_LONGEST, CallerKey};
pub use acts::{
    ACTS_KEPT, ACTS_WAITING, ActSurface, ActWarning, AskedSecret, NEW_PLACE, PersonActAsk,
    PersonActId, SHOWN_VALUE_CHARS, ShownField,
};
pub use def::{
    ActionDef, ActionName, BoxFuture, BuiltInToolset, Effect, Faces, Field, FieldType, Handler,
    HandlerFn, HealthNeed, Input, Listed, PersonNeed, RestAlias, RunsIn, Shape, Who,
};
pub use dispatch::{
    Call, Caller, ErrorBody, ExitClass, FrontDoor, Next, Output, Principal, ProjectFolder, Request,
    Response,
};
pub use events::{EVENTS_PAGE, Followed, RunEventView, RunEvents, RunsInHand};
pub use faces::{
    CommandArg, CommandFace, McpResultFace, McpToolFace, PROCESS_COMMANDS, PROGRAM, REST_PREFIX,
    RestRouteFace, SHORTCUTS, SdkMethodFace, Shortcut, ShortcutOption, ShortcutStep, TOOL_PREFIX,
    ToolAnnotationsFace, command_faces, command_faces_of, has_sdk_method, mcp_tool, mcp_tools,
    rest_routes, said_line, sdk_methods, shortcuts_offered, tool_name,
};
pub use health::{Health, HealthState, Impaired, Part};
pub use kernel::{CallerCredential, CloseReason, Kernel, KernelParts, Prepared, StorePlace};
pub use paths::{APP_FOLDER, Env, HOME_VARIABLE, OsFamily, PASSPHRASE_VARIABLE, Paths};
pub use person::{
    ApprovalAsk, ApprovalDesk, ApprovalPreview, BINDING_LATER, MAY_NOT_BE_READ_BACK, NOT_READ_BACK,
    NOW_CANNOT_BE_READ, PRESENCE_WINDOW_MS, PREVIEW_RECORDS, PREVIEW_VALUE_CHARS, PersonSurface,
    PresenceAsk, PresenceProof, PresenceReport, PreviewBinding, PreviewField, PreviewNotice,
    PreviewRecord, SessionAsk, SignInAsk, SurfaceLink, WaitingApproval,
};
pub use project::PROJECT_SETTINGS_DIFFER;
pub use registry::{ActionRegistry, CATALOG_FORMAT, Catalog, CatalogEntry, CatalogField};
pub use runner::RunnerLoop;
pub use settings::{Settings, Sources};
pub use space::{FreeSpace, VolumeFreeSpace};

/// The crate under its own name, so that the tests below read as the crates
/// above this one will write them.
#[cfg(test)]
extern crate self as a2m_actions;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests; the helpers they share do not carry the test attribute"
)]
mod tests {
    mod access;
    mod audit;
    mod authoring;
    mod drift;
    mod guard;
    mod kernel;
    mod keys;
    mod net;
    mod others;
    mod pass_two;
    mod probe;
    mod reach;
    mod renewal;
    mod runs;
    mod second;
    mod unbuilt;
    mod watches;
}
