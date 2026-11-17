//! The `a2m-bench` program: runs the benchmark scenarios, and compares a new
//! result with a stored one. All of it is in the library; this only reads the
//! arguments and the environment and sets the exit status.

use std::process::ExitCode;

use a2m_bench::cli::{Command, USAGE};
use a2m_bench::error::BenchError;
use a2m_bench::report::COMMIT_VARIABLE;

#[tokio::main]
async fn main() -> ExitCode {
    let commit = std::env::var(COMMIT_VARIABLE).ok();
    let outcome = match Command::parse(std::env::args().skip(1)) {
        Ok(command) => command.execute(commit).await,
        Err(error) => Err(error),
    };
    match outcome {
        Ok(outcome) => {
            print!("{}", outcome.text);
            if outcome.regression {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("a2m-bench: {error}");
            if matches!(error, BenchError::Usage(_)) {
                eprint!("\n{USAGE}");
            }
            ExitCode::from(2)
        }
    }
}
