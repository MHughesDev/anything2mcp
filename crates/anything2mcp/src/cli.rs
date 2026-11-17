//! The command line, shared by the `anything2mcp` command and its short name
//! `a2m`. No command exists yet; it reports its version.

use std::process::ExitCode;

/// Exit code for a malformed command, from the command output contract in
/// `docs/reference/technical-reference.md`.
const MALFORMED_COMMAND: u8 = 2;

/// Runs the command line with the arguments that follow the program name.
///
/// The output names the product, never the name the program was started
/// under, so both commands behave identically.
pub fn run(args: impl IntoIterator<Item = String>) -> ExitCode {
    let mut args = args.into_iter();
    match (args.next().as_deref(), args.next()) {
        (None | Some("--version" | "-V"), None) => {
            println!("anything2mcp {}", crate::version());
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "anything2mcp: no commands exist yet; run it with no arguments to see the version"
            );
            ExitCode::from(MALFORMED_COMMAND)
        }
    }
}
