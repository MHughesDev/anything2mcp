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
pub use connector::{Connector, HELLO_PATIENCE, Link};
pub use lock::{ENGINE_LOCK_NAME, EngineLock, LockPlace};
pub use os::{Kept, kept_by};

/// Makes one of the engine's own folders this user's alone, as the
/// runtime folder is: made if it is not there, refused if it is not a
/// folder of this user's, and closed to every other account (on Unix the
/// mode 0700; on Windows an access list that names only this user, which
/// what is made inside it inherits). What is inside it is then out of
/// other accounts' reach, whatever its own mode (decision 0241).
///
/// # Errors
///
/// What the operating system answers, or `PermissionDenied` for a folder
/// that is not this user's.
pub fn keep_folder_private(dir: &std::path::Path) -> std::io::Result<()> {
    os::prepare_dir(dir)
}
pub use presence::field::FieldPrompt;
pub use presence::{
    Availability, NoPrompt, PresenceKind, PresencePrompt, PresenceReason, PromptAnswer, PromptFor,
    PromptName, PromptText, presence_available, system_prompt, system_prompt_for,
};
pub use start::{
    ENGINE_LOG_NAME, Launch, START_LOCK_NAME, STARTED_LINE, STOPPED_LINE, Started, stopped_line,
};
pub use surface::{FAILURE_CODE_HEADER, LocalSurface, SurfaceParts};

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests; the helpers they share do not carry the test attribute"
)]
mod tests {
    mod channel;
    mod field;
    mod local_authentication;
    #[cfg(target_os = "linux")]
    mod polkit;
    mod surface;
    mod windows_hello;
    mod world;
}
