//! The MCP face of the kernel.
//!
//! [`McpService`] is one MCP endpoint for one toolset, as a stateless
//! Streamable HTTP service that the product crate mounts on the local
//! channel. It lists tools from run-time data: the MCP faces of the action
//! registry and the published tools of the targets. It turns a tool call
//! into a dispatch, and renders the dispatcher's response. It decides
//! nothing and holds no state between requests.
//!
//! [`stdio_proxy`] is the other half: a line copier between a client's
//! standard streams and that endpoint, for a client that starts a program.
//!
//! The protocol layer is this crate's own (decision 0130): everything
//! promised is a list of messages in the crate's design, tested with raw
//! messages.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: layers 1 to 7.

mod clean;
pub mod codes;
mod http;
mod protocol;
mod proxy;
mod settings;
mod tools;

pub use http::{HttpRequest, HttpResponse, McpPeer, McpService, mcp_path};
pub use proxy::{BoxFuture, LINE_LIMIT, OPEN_LIMIT, Upstream, stdio_proxy};
pub use settings::{Listing, McpSettings, Revision};

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests; the helpers they share do not carry the test attribute"
)]
mod tests;
