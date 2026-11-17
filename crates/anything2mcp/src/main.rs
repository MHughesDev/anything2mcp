//! The `anything2mcp` command.

use std::process::ExitCode;

fn main() -> ExitCode {
    anything2mcp::cli::run(std::env::args().skip(1))
}
