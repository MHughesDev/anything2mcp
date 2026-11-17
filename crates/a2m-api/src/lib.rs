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
//! Not built yet: the event stream of a run, webhooks, the server's
//! listener with its token checks, and the server's approval pages. See
//! `docs/architecture/crates/a2m-api.md`.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`.

pub mod codes;
mod openapi;
mod router;
mod status;

pub use openapi::{OPENAPI_PATH, OPENAPI_VERSION, openapi, operation_id};
pub use router::{
    ApiSettings, DRAIN_FACTOR, DRAIN_TIME, IDEMPOTENCY_KEY_LONGEST, ServedRoute, router,
    served_routes,
};
pub use status::status_of;
