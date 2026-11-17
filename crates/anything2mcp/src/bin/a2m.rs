//! `a2m`, the short name of the `anything2mcp` command. It is the same program.

use std::process::ExitCode;

fn main() -> ExitCode {
    anything2mcp::cli::run(std::env::args().skip(1))
}
