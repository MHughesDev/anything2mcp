//! The `web2mcp` command. No command exists yet; it reports its version.

use std::process::ExitCode;

/// Exit code for a malformed command, from the command output contract in
/// `docs/reference/technical-reference.md`.
const MALFORMED_COMMAND: u8 = 2;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match (args.next().as_deref(), args.next()) {
        (None | Some("--version" | "-V"), None) => {
            println!("web2mcp {}", web2mcp::version());
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "web2mcp: no commands exist yet; run it with no arguments to see the version"
            );
            ExitCode::from(MALFORMED_COMMAND)
        }
    }
}
