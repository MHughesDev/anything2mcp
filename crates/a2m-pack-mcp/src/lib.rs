//! The MCP pack: the mode `mcp`, a pass-through to an MCP server that a
//! system already provides, with this runtime's verification added.
//!
//! A mapping file names the only tools that can be reached, each pinned by
//! the digest of its definition ([`definition_digest`]). The driver
//! ([`McpDriver`]) keeps one connection per surface and identity, compares
//! the server's tool list with the pins, and calls one mapped tool per
//! step; a changed tool is a named stop, and a write whose answer was lost
//! is never sent again. The server's own tool list is a discoverer
//! ([`McpTools`]) that lists and never calls. [`UpstreamMcp`] is the auth
//! provider that hands the server a bearer token.
//!
//! The pack speaks the protocol through a small client of its own, over
//! HTTP to a remote server and over the standard streams of a local server
//! program the supervisor starts. Text an upstream server writes is
//! untrusted: it never becomes part of a failure, and it reaches a caller
//! only as a value of a field type that marks it, or in a description's
//! untrusted part.
//!
//! Not built: HTTPS, the protocol's authorization flow, a credential for a
//! local server program, a local server program on Windows, answers sent as
//! an event stream, and the drafting of a pass-through operation. Each is a
//! named failure or is named in the crate's design.
//!
//! The product crate registers the pack with one call:
//! `builder.add_pack(&a2m_pack_mcp::pack())`.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: layers 1 to 5. Its design is
//! `docs/architecture/crates/a2m-pack-mcp.md`.

mod auth;
mod client;
pub mod codes;
mod discover;
mod driver;
mod json;
mod lower;
mod mapping;
mod names;

use std::sync::Arc;

use a2m_core::Failure;
use a2m_provider::{CompilerRegistrations, Pack, PackContext, Registrar};

pub use auth::UpstreamMcp;
pub use discover::{McpDescriber, McpTools};
pub use driver::McpDriver;
pub use json::{ToolDefinition, definition_digest};
pub use names::{
    CALL_TOOL, LIST_TOOLS, MAPPING_KIND, Names, PACK, SURFACE_KIND, auth_provider, mapping_kind,
    surface_kind,
};

/// The pack's manifest, as it is embedded in the program.
pub const MANIFEST: &str = include_str!("../anything2mcp-provider.toml");

/// The MCP pack. The product crate hands it to the registry builder.
#[derive(Debug, Clone, Copy, Default)]
pub struct McpPack;

/// The MCP pack, for `RegistryBuilder::add_pack`.
#[must_use]
pub fn pack() -> McpPack {
    McpPack
}

fn setting(cx: &PackContext, key: &str, default: usize) -> usize {
    cx.settings()
        .get(key)
        .and_then(|value| value.as_int())
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(default)
}

impl Pack for McpPack {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        let names = Names::new()?;
        let limits = client::Limits {
            max_message_bytes: setting(cx, "max_message_bytes", 4 * 1024 * 1024),
            max_tools: setting(cx, "max_tools", 500),
        };
        to.driver(
            names.mode.clone(),
            Arc::new(McpDriver::new(Arc::clone(cx.redact()), limits)),
        )?;
        let mut compiler = CompilerRegistrations::new()
            .surface_kind(mapping::surface_kind_spec())
            .file_kind(Arc::new(mapping::MappingKind::new()))
            .lowering(Arc::new(lower::McpLowering::new(names.mode.clone())))
            .drafting_rule(Arc::new(discover::ToolToMapping::new(names.clone())));
        for rule in lower::Rule::ALL {
            compiler = compiler.lint_rule(Arc::new(rule));
        }
        to.compiler(compiler)?;
        to.discoverer(
            Arc::new(McpTools::new(names.clone(), cx.telemetry().clock())),
            Arc::new(McpDescriber::new(names)),
        )?;
        to.auth_provider(Arc::new(UpstreamMcp))?;
        Ok(())
    }
}
