//! Starting the application a surface declares, when it allows it and no
//! application of the program is running (decision 0467).
//!
//! A surface that states `start: true` lets the driver start its
//! `program`: the absolute path the target declares, through the
//! supervisor the pack's context scopes to the programs of the published
//! compiled target, which checks the digest the surface states at the
//! start. Before that the program's file is held to the provider's check
//! of who may change a program (a shell, an interpreter, or a script is
//! refused; so is a file another user can replace), as the command and MCP
//! packs hold theirs. It is given no argument, the variables of
//! [`PASSED`] alone, and nothing on its standard input; what it writes on
//! its standard output is read and dropped. It is started only in a run
//! (`Posture::Live`): a discovery or a sandbox starts nothing.
//!
//! Then the driver waits, by the accessibility bus's own signals, for the
//! application to publish itself, and proves it as it proves one it found
//! running. One that does not by the deadline of the prepare is stopped:
//! the pack leaves nothing it started that it could not prove. One that
//! does stays running for the engine's life, as an application a person
//! started would; the supervisor ends it when the engine stops.
//!
//! Built on Linux. On Windows and macOS nothing is started, as
//! `busy`/`start_not_built`: neither back end yet has an event that says a
//! new application's window appeared, and a wait polls nothing.

use a2m_files::{EnvironmentVariable, ProgramRun, VariableValue};

/// The variables of the engine's own environment a started application is
/// given, when they are set there: what a program needs to find the
/// person's display, session bus, settings, and language. A variable that
/// makes a program load other code is refused by the supervisor whatever
/// this list holds. On a session of a test's own
/// ([`crate::DesktopPack::on_session_bus`]) none is passed, and only the
/// session bus's address is set.
#[cfg(target_os = "linux")]
pub(crate) const PASSED: &[&str] = &[
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "XDG_DATA_DIRS",
    "XDG_CONFIG_DIRS",
    "HOME",
    "USER",
    "LANG",
    "LC_ALL",
    "PATH",
];
/// On Windows and macOS nothing is started in this version.
#[cfg(not(target_os = "linux"))]
pub(crate) const PASSED: &[&str] = &[];

/// The kind of subsystem a started application is, for the supervisor's
/// count.
#[cfg(target_os = "linux")]
pub(crate) const KIND: &str = "desktop_application";

/// What the review shows of a surface that lets the driver start its
/// program: the program, by path and the digest it states, and the
/// variables it is passed.
pub(crate) fn statement_parts(
    path: &str,
    digest: Option<a2m_core::Digest>,
) -> (ProgramRun, Vec<EnvironmentVariable>) {
    (
        ProgramRun::Program {
            path: path.to_owned(),
            digests: digest.into_iter().collect(),
        },
        PASSED
            .iter()
            .map(|name| EnvironmentVariable {
                name: (*name).to_owned(),
                value: VariableValue::Passed,
            })
            .collect(),
    )
}

/// A digest as a surface states it, with or without its `sha256:`.
pub(crate) fn parse_digest(text: &str) -> Option<a2m_core::Digest> {
    text.strip_prefix("sha256:")
        .unwrap_or(text)
        .parse::<a2m_core::Digest>()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_passed_variable_belongs_to_the_engine_or_redirects_loading() {
        for name in PASSED {
            assert!(!a2m_provider::is_engine_variable(name), "{name}");
            assert!(!a2m_provider::redirects_loading(name), "{name}");
        }
    }
}
