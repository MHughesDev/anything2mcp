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
pub mod binary;
pub mod codes;
mod discover;
mod draft;
mod driver;
mod fish;
mod json;
mod kind;
mod lint;
mod lower;
mod maml;
mod manual;
mod metadata;
mod model;
mod output;
mod own_code;
mod pattern;
mod powershell;
mod psd1;
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
                trust: admin_groups(cx.settings())?,
            }),
        )?;
        to.compiler(
            CompilerRegistrations::new()
                .surface_kind(surface_kind())
                .file_kind(Arc::new(kind::CommandKind::new()))
                .lowering(Arc::new(lower::CliLowering::new()))
                .lint_rule(Arc::new(lint::ShellProgram))
                .lint_rule(Arc::new(lint::WriteExitDeclared))
                .drafting_rule(Arc::new(draft::CommandFromFinding))
                .drafting_rule(Arc::new(draft::OperationFromCommand))
                .drafting_rule(Arc::new(powershell::PowerShellFromFinding)),
        )?;
        to.discoverer(
            Arc::new(discover::HelpDiscoverer {
                clock: cx.telemetry().clock(),
            }),
            Arc::new(discover::HelpDescriber {
                redact: Arc::clone(cx.redact()),
            }),
        )?;
        to.discoverer(
            Arc::new(binary::BinaryDiscoverer {
                clock: cx.telemetry().clock(),
            }),
            Arc::new(binary::BinaryDescriber {
                redact: Arc::clone(cx.redact()),
            }),
        )?;
        Ok(())
    }
}

/// The groups a person named in the protected setting
/// `packs.command.admin_groups`, each by its numeric id.
///
/// # Errors
///
/// `input`/`pack_settings_invalid` for an entry that is not a group's
/// numeric id: a group is never looked up by its name.
fn admin_groups(settings: &a2m_core::Values) -> Result<safety::Trust, Failure> {
    let named = match settings.get("admin_groups") {
        None => Vec::new(),
        Some(Value::List(items)) => items
            .iter()
            .map(|item| {
                item.as_text()
                    .filter(|text| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
                    .and_then(|text| text.parse::<u32>().ok())
            })
            .collect::<Option<Vec<u32>>>()
            .ok_or_else(groups_invalid)?,
        Some(_) => return Err(groups_invalid()),
    };
    Ok(safety::Trust::with_groups(named))
}

fn groups_invalid() -> Failure {
    a2m_provider::codes::row::PACK_SETTINGS_INVALID.failure(
        "`packs.command.admin_groups` holds an entry that is not a group's numeric id.",
        "Name each group by its numeric id, such as \"80\"; a group's name is never looked up.",
    )
}

/// Hands the command pack to a registry builder. This is the one call the
/// product crate makes for the pack.
pub fn register(builder: &mut RegistryBuilder) {
    builder.add_pack(&CommandPack);
}

#[cfg(test)]
mod tests {
    use a2m_core::{Value, Values};

    use super::{admin_groups, safety::Trust};

    #[test]
    fn the_named_groups_are_numbers_and_nothing_else() {
        let with = |items: Vec<&str>| {
            Values::new().with(
                "admin_groups",
                Value::List(items.into_iter().map(Value::from).collect()),
            )
        };
        assert_eq!(admin_groups(&Values::new()), Ok(Trust::default()));
        assert_eq!(
            admin_groups(&with(vec!["80", "1000"])),
            Ok(Trust::with_groups(vec![80, 1000]))
        );
        // A name is never looked up, and a number is a whole group id.
        for bad in ["staff", "-1", "", "4294967296", " 80"] {
            let failure = admin_groups(&with(vec![bad])).unwrap_err();
            assert_eq!(
                failure.code(),
                &a2m_provider::codes::PACK_SETTINGS_INVALID,
                "{bad}"
            );
        }
    }
}
