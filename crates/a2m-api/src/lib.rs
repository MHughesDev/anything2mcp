//! The REST interface of the kernel: every route generated from the action
//! registry, and the OpenAPI document generated from the same registry.
//!
//! [`router`] builds an Axum router with no socket of its own; the product
//! crate mounts it on the local channel, and later on the server's
//! listener. Whoever mounts it verifies the caller through
//! `Kernel::verify_caller` and puts the [`a2m_actions::Caller`] on each
//! request as an extension. The router decides nothing: it reads the
//! input, hands one request to `Kernel::dispatch`, and renders the
//! response, choosing the status from [`status_of`] alone.
//!
//! Two routes are served that no action generates: the OpenAPI document,
//! and a run's events, as a page or as a stream of server-sent events
//! ([`RUN_EVENTS_PATH`]). Reading a run's events is not an action: the
//! kernel shows a caller only a run the same caller started.
//!
//! Not built yet: webhooks, the server's listener with its token checks,
//! and the server's approval pages. See
//! `docs/architecture/crates/a2m-api.md`.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`.

pub mod codes;
mod events;
mod openapi;
mod router;
mod status;

/// The project folder a request works in, as the mount states it. The one
/// type every door reads: whoever mounts the router puts one on a request
/// as an extension, beside the caller.
pub use a2m_actions::ProjectFolder;
pub use events::{KEEP_ALIVE, RUN_EVENTS_PATH};
pub use openapi::{OPENAPI_PATH, OPENAPI_VERSION, openapi, operation_id};
pub use router::{
    ApiSettings, DRAIN_FACTOR, DRAIN_TIME, IDEMPOTENCY_KEY_LONGEST, ServedRoute, router,
    served_routes,
};
pub use status::{EXIT_CLASS_HEADER, status_of};
