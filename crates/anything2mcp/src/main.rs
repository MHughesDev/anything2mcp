//! The `anything2mcp` command.

use std::process::ExitCode;

#[allow(
    unsafe_code,
    reason = "`run` changes the process environment, which is sound before any thread starts"
)]
fn main() -> ExitCode {
    // SAFETY: this is the first thing the program does: no other thread
    // has started, which is what `run` asks.
    unsafe { anything2mcp::cli::run(std::env::args().skip(1)) }
}
