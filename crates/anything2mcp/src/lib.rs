//! The Anything2MCP binary and the library facade for embedding the engine.
//!
//! This crate is the product: the one program, and the only code that
//! names a pack. It reads the settings, fills the provider registry with
//! one call per first-party pack, opens the kernel, and runs a command
//! line that is generated from the action registry.
//!
//! In this build every command runs the engine inside its own process
//! ([`Engine`]). The local engine, its channel, the stdio proxy, and the
//! server and runner roles are not here yet.
//!
//! The crates this one may depend on are set by the layering table in
//! `docs/reference/technical-reference.md`: all of them.

pub mod cli;
mod engine;
mod packs;
mod person;

pub use engine::{Engine, EngineOptions, PASSPHRASE_VARIABLE, action_registry};
pub use person::{ProcessTerminal, Terminal, TerminalSurface};

/// The version of this build of Anything2MCP.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn version_has_three_numeric_parts() {
        let core = version().split(['-', '+']).next().unwrap_or_default();
        let parts: Vec<&str> = core.split('.').collect();
        assert_eq!(parts.len(), 3, "not three parts: {core}");
        for part in parts {
            assert!(part.parse::<u64>().is_ok(), "not a number: {part}");
        }
    }
}
