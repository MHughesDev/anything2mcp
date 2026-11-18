//! The command pack: the `cli` driver over declared programs, with typed
//! arguments and parsed output.
//!
//! A target may be a program: something started with arguments, which ends
//! with an exit status and leaves output behind. This crate brings the
//! `cli` mode of the command archetype, the surface kind `program`, the
//! file kind `commands/`, the lowering of a binding to the steps `spawn`,
//! `expect`, and `parse`, the driver that carries those steps out through
//! the kernel's supervisor, and a discoverer that runs only a program's
//! declared help and version forms. It is never a shell: no command line
//! is ever put together from text, and no input can become an option, a
//! program, or a path. Spec section 306 is the design.
//!
//! The product crate makes one call: [`register`].

mod args;
pub mod codes;
mod discover;
mod draft;
mod driver;
mod json;
mod kind;
mod lint;
mod lower;
mod model;
mod output;
mod pattern;
mod safety;

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use a2m_core::{Failure, Value};
use a2m_files::{JsonSchema, SurfaceKindSpec};
use a2m_provider::{
    CompilerRegistrations, Pack, PackContext, Registrar, RegistryBuilder, RootName, SubsystemKind,
};

/// The pack's manifest, as the registry reads it.
pub const MANIFEST: &str = include_str!("../anything2mcp-provider.toml");

/// The command pack. [`register`] hands it to a registry builder.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandPack;

/// The fields a surface of kind `program` states: the program's absolute
/// path, which is its address, and, where the same file is installed on
/// every machine, its one digest. The builds of each platform are listed in
/// the surface's `commands/` files.
fn surface_kind() -> SurfaceKindSpec {
    SurfaceKindSpec::new(
        codes::SURFACE_KIND,
        "path",
        JsonSchema::object()
            .required("path", JsonSchema::text())
            .optional("digest", JsonSchema::text()),
    )
    .with_program("path", Some("digest"))
}

impl Pack for CommandPack {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        let setting = |key: &str, default: u64| {
            cx.settings()
                .get(key)
                .and_then(Value::as_int)
                .and_then(|number| u64::try_from(number).ok())
                .unwrap_or(default)
        };
        to.driver(
            codes::mode(),
            Arc::new(driver::CliDriver {
                roots: cx.roots().clone(),
                redact: Arc::clone(cx.redact()),
                runs_root: RootName::new(driver::RUNS_ROOT)?,
                kind: SubsystemKind::new(driver::SUBSYSTEM)?,
                max_timeout_ms: setting("max_timeout_ms", 600_000),
                max_output_bytes: setting("max_output_bytes", 16 * 1024 * 1024),
                next_folder: AtomicU64::new(1),
            }),
        )?;
        to.compiler(
            CompilerRegistrations::new()
                .surface_kind(surface_kind())
                .file_kind(Arc::new(kind::CommandKind::new()))
                .lowering(Arc::new(lower::CliLowering::new()))
                .lint_rule(Arc::new(lint::ShellProgram))
                .lint_rule(Arc::new(lint::WriteExitDeclared))
                .drafting_rule(Arc::new(draft::CommandFromFinding)),
        )?;
        to.discoverer(
            Arc::new(discover::HelpDiscoverer {
                clock: cx.telemetry().clock(),
            }),
            Arc::new(discover::HelpDescriber {
                redact: Arc::clone(cx.redact()),
            }),
        )?;
        Ok(())
    }
}

/// Hands the command pack to a registry builder. This is the one call the
/// product crate makes for the pack.
pub fn register(builder: &mut RegistryBuilder) {
    builder.add_pack(&CommandPack);
}
