//! The engine on one machine, and the channel to it.
//!
//! This crate holds both ends of the local channel: [`Channel`], the
//! listener inside the engine, and [`Connector`], which every thin client
//! uses to find the engine and check that it belongs to the same user. The
//! channel is a Unix domain socket on macOS and Linux and a named pipe on
//! Windows, with HTTP/1.1 over it. The service it carries is composed by
//! the product crate; this crate knows neither the REST router nor the MCP
//! endpoint. [`EngineLock`] proves one engine per user and data directory.
//! [`Connector::connect_or_start`] starts the engine on demand, detached,
//! as a [`Launch`] says, under the start lock.
//!
//! It also holds [`LocalSurface`], the person surface on one machine: the
//! only holder of the approval desk, a page on a short-lived loopback
//! listener, and the presence prompt: polkit on a Linux desktop,
//! LocalAuthentication on macOS, and Windows Hello on Windows. Nothing
//! reached through the channel can approve.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: layers 1 to 7.

mod channel;
pub mod codes;
mod connector;
mod lock;
mod os;
mod presence;
mod start;
mod surface;

pub use channel::{
    CLIENT_LABEL_HEADER, CLIENT_VERSION_HEADER, Channel, ChannelControl, ClientLabel, HELLO_PATH,
    IdleHolds, IdlePolicy, LOCAL_API, LOCAL_API_HEADER, OsUser, Peer, StopReason,
};
pub use connector::{Connector, Link};
pub use lock::{ENGINE_LOCK_NAME, EngineLock};
pub use os::{Kept, kept_by};
pub use presence::{
    Availability, NoPrompt, PresenceKind, PresencePrompt, PresenceReason, PromptAnswer, PromptName,
    PromptText, presence_available, system_prompt,
};
pub use start::{ENGINE_LOG_NAME, Launch, START_LOCK_NAME, STARTED_LINE, Started};
pub use surface::{LocalSurface, SurfaceParts};

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests; the helpers they share do not carry the test attribute"
)]
mod tests {
    mod channel;
    mod local_authentication;
    #[cfg(target_os = "linux")]
    mod polkit;
    mod surface;
    mod windows_hello;
    mod world;
}
