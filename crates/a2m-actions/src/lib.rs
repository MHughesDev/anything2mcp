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

mod catalog;
pub mod codes;
mod control;
mod def;
mod dispatch;
mod faces;
mod handlers;
mod health;
mod keeper;
mod kernel;
mod paths;
mod person;
mod registry;
mod runner;
pub mod settings;
mod tools;

pub use def::{
    ActionDef, ActionName, BoxFuture, BuiltInToolset, Effect, Faces, Field, FieldType, Handler,
    HandlerFn, HealthNeed, Input, Listed, PersonNeed, RestAlias, RunsIn, Shape, Who,
};
pub use dispatch::{
    Call, Caller, ErrorBody, ExitClass, FrontDoor, Next, Output, Principal, Request, Response,
};
pub use faces::{
    CommandArg, CommandFace, McpResultFace, McpToolFace, PROCESS_COMMANDS, PROGRAM, REST_PREFIX,
    RestRouteFace, SHORTCUTS, SdkMethodFace, Shortcut, ShortcutStep, TOOL_PREFIX,
    ToolAnnotationsFace, command_faces, command_faces_of, mcp_tool, mcp_tools, rest_routes,
    sdk_methods, tool_name,
};
pub use health::{Health, HealthState, Impaired, Part};
pub use kernel::{CallerCredential, CloseReason, Kernel, KernelParts, Prepared, StorePlace};
pub use paths::{APP_FOLDER, Env, HOME_VARIABLE, OsFamily, Paths};
pub use person::{
    ApprovalAsk, ApprovalDesk, PersonSurface, PresenceAsk, PresenceProof, SessionAsk, SignInAsk,
    SurfaceLink, WaitingApproval,
};
pub use registry::{ActionRegistry, CATALOG_FORMAT, Catalog, CatalogEntry, CatalogField};
pub use runner::RunnerLoop;
pub use settings::{Settings, Sources};

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
    mod kernel;
}
