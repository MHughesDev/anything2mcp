//! The desktop pack: a target can be a desktop application, read through
//! the operating system's accessibility interface as a screen reader reads
//! it.
//!
//! This version reads on Linux, Windows, and macOS, and on Linux also
//! writes (decision 0218). Its reading back end is a look-only handle, over
//! AT-SPI on Linux (decision 0198), over UI Automation on a thread of its
//! own on Windows (decision 0209), and over the system's accessibility
//! interface on a thread of its own on macOS, once the person has allowed
//! the program to use it (decision 0244): it finds
//! the one running application that is the program and the name a surface
//! declares, in the engine's own session, proves its process by what the
//! system says of it,
//! walks a window's tree within bounds, reads an element's text, and waits
//! for the application to signal a change. Its type has no method that sets
//! a value, invokes an action, moves focus, or sends a key or a click, so a
//! read cannot press Save by any route, and the compiler refuses a desktop
//! binding on an operation that writes, as `desktop.writes_not_built`. A
//! control that holds a secret is reported as present and its value is
//! never asked for. An application that is not running is a named stop:
//! the pack starts nothing.
//!
//! The pack owns the `screens/` file kind for surfaces of the kind
//! `desktop`, where every locator lives: a role and an accessible name,
//! both exact. Its driver, of the mode `desktop`, carries out `goto`,
//! `wait`, and `read` steps, and the `snapshot` step its discoverer
//! `desktop_snapshot` sends. Its drafting rules turn a window's shape into a
//! screen and a `get` read of the values it labels, and a screen and a
//! `list` read of each table it names.
//!
//! Not built, and so not declared in the manifest: writes on Windows and
//! macOS (there a `set` or an `invoke` stops with `writes_not_built`), a back
//! end for any other system (there the driver stops at prepare with
//! `system_not_built`), keys and clicks, starting an application, the
//! discoverer `desktop_probe`, the lint rules of the design, and every
//! control but `desktop_text`.
//!
//! The product crate registers the pack with one call, [`add_to`].
//!
//! A pack crate sees the kernel up to the provider contract; the layering
//! table in `docs/reference/technical-reference.md` is the rule, and this
//! crate's design is `docs/architecture/crates/a2m-pack-desktop.md`.

mod backend;
pub mod codes;
mod discover;
mod draft;
mod driver;
mod lower;
mod model;
mod roles;
mod screens;
mod tree;

use std::sync::Arc;

use a2m_core::{ControlName, Failure, FindingKind, Mode, Value};
use a2m_files::{JsonSchema, SurfaceKindSpec};
use a2m_provider::{
    CompilerRegistrations, DiscovererName, Pack, PackContext, Registrar, RegistryBuilder,
};

pub use lower::{GOTO, INVOKE, READ, SET, SNAPSHOT, WAIT};
pub use screens::{CONTROLS, DESKTOP_SURFACE, PACK, SCREEN_KIND};

/// The name of the mode this pack drives.
pub const DESKTOP_MODE: &str = "desktop";

/// The key of a surface of the kind `desktop` that holds its address: the
/// application's program, by its absolute path.
pub const ADDRESS_KEY: &str = "program";

/// The name of the pack's discoverer.
pub const SNAPSHOT_DISCOVERER: &str = "desktop_snapshot";

const MANIFEST: &str = include_str!("anything2mcp-provider.toml");

/// The desktop pack. Made by [`pack`].
#[derive(Debug, Clone, Default)]
pub struct DesktopPack {
    session: Option<Arc<str>>,
}

/// The pack, reading the applications of the session the engine runs in.
#[must_use]
pub fn pack() -> DesktopPack {
    DesktopPack::default()
}

impl DesktopPack {
    /// The same pack, reading the session whose bus is at `address`
    /// instead of the one the engine's environment names: for a test that
    /// starts a session of its own. The product never calls this.
    #[must_use]
    pub fn on_session_bus(address: &str) -> Self {
        Self {
            session: Some(Arc::from(address)),
        }
    }
}

/// Hands the pack to the registry builder. This is the one call the
/// product crate makes for it.
pub fn add_to(builder: &mut RegistryBuilder) {
    builder.add_pack(&pack());
}

/// The fields a surface of the kind `desktop` has.
fn surface_fields() -> JsonSchema {
    JsonSchema::object()
        .required(ADDRESS_KEY, JsonSchema::text())
        .required("application", JsonSchema::text())
        .optional("digest", JsonSchema::text())
}

fn setting(cx: &PackContext, key: &str) -> Option<usize> {
    cx.settings()
        .get(key)
        .and_then(Value::as_int)
        .and_then(|number| usize::try_from(number).ok())
}

/// The manifest's provider version.
fn version() -> String {
    MANIFEST
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or("0.0.0")
        .to_owned()
}

impl Pack for DesktopPack {
    fn manifest(&self) -> &'static str {
        MANIFEST
    }

    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        let mode = Mode::new(DESKTOP_MODE)?;
        let mut limits = backend::Limits::default();
        if let Some(nodes) = setting(cx, "max_nodes") {
            limits.nodes = nodes;
        }
        if let Some(depth) = setting(cx, "max_depth").and_then(|depth| u32::try_from(depth).ok()) {
            limits.depth = depth;
        }
        if let Some(chars) = setting(cx, "max_read_chars") {
            limits.chars = chars;
        }
        to.driver(
            mode.clone(),
            Arc::new(driver::DesktopDriver::new(self.session.clone(), limits)),
        )?;
        let window = FindingKind::new(discover::WINDOW_FINDING)?;
        let mut compiler = CompilerRegistrations::new()
            .surface_kind(
                SurfaceKindSpec::new(DESKTOP_SURFACE, ADDRESS_KEY, surface_fields())
                    .with_program(ADDRESS_KEY, Some("digest")),
            )
            .file_kind(Arc::new(screens::ScreenKind::new()))
            .lowering(Arc::new(lower::DesktopLowering::new(mode)))
            .drafting_rule(Arc::new(draft::WindowScreenRule {
                kind: window.clone(),
            }))
            .drafting_rule(Arc::new(draft::WindowReadRule {
                kind: window.clone(),
            }));
        for control in CONTROLS {
            compiler = compiler.control(ControlName::new(control)?);
        }
        to.compiler(compiler)?;
        to.discoverer(
            Arc::new(discover::DesktopSnapshot {
                name: DiscovererName::new(SNAPSHOT_DISCOVERER)?,
                kind: window.clone(),
                version: version(),
                clock: cx.telemetry().clock(),
            }),
            Arc::new(discover::DesktopDescriber { kind: window }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_failure_code_is_declared_with_its_class() {
        for (class, code) in codes::ALL {
            let declared = format!("[[failure_codes]]\nclass = \"{class}\"\ncode = \"{code}\"\n");
            assert!(MANIFEST.contains(&declared), "{declared}");
        }
        for code in codes::PASSED_ON {
            let declared = format!("code = \"{code}\"\n");
            assert!(MANIFEST.contains(&declared), "{declared}");
        }
        assert_eq!(
            MANIFEST.matches("[[failure_codes]]").count(),
            codes::ALL.len() + codes::PASSED_ON.len()
        );
        assert_eq!(version(), "0.0.1");
    }

    /// No source of the pack gives a failure a class of its own: each is
    /// made from a row of a table, which gives it its class.
    #[test]
    fn every_failure_takes_its_class_from_a_table() {
        fn walk(dir: &std::path::Path, found: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if path.is_dir() {
                    if name != "tests" {
                        walk(&path, found);
                    }
                } else if name.ends_with(".rs") && name != "tests.rs" {
                    let text = std::fs::read_to_string(&path).unwrap();
                    let text = text.split("#[cfg(test)]").next().unwrap();
                    for made in ["Failure::new(", "class: FailureClass::"] {
                        if text.contains(made) {
                            found.push(format!("{}: {made}", path.display()));
                        }
                    }
                }
            }
        }
        let mut found = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut found,
        );
        assert!(
            found.is_empty(),
            "made with a class of their own: {found:?}"
        );
        for row in codes::TABLE {
            let made = codes::failure(row, "A case.", row.next);
            assert_eq!((made.class(), made.code().as_str()), (row.class, row.code));
        }
    }
}
