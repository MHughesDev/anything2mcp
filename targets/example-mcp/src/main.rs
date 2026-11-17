//! The stdio form of the stand: reads newline-delimited JSON-RPC messages on
//! standard input and answers on standard output. It takes no arguments. Its
//! state folder is named by `EXAMPLE_MCP_STATE`; without one, the state lives
//! in memory and ends with the program.

use std::path::PathBuf;
use std::process::ExitCode;

use example_mcp::stdio::{STATE_ENV, TOKEN_ENV, run};

fn main() -> ExitCode {
    if std::env::args().len() > 1 {
        eprintln!("example-mcp: takes no arguments; it speaks MCP on standard input and output");
        return ExitCode::from(2);
    }
    let folder = std::env::var_os(STATE_ENV).map(PathBuf::from);
    // The credential is compared and never printed.
    let token = std::env::var(TOKEN_ENV).ok();
    match run(folder.as_deref(), token.as_deref()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("example-mcp: stopped: {e}");
            ExitCode::FAILURE
        }
    }
}
