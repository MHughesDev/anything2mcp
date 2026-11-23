//! The field harness (decision 0310).
//!
//! The product's own `serve`, with one difference: its presence prompt is
//! the field prompt, which writes its question to standard output and takes
//! its answer from one line of standard input, a pipe only a field test's
//! runner holds. The shipped program never has this prompt.
//!
//! It serves only inside a closed network. Before it does anything, it tries
//! one public address and refuses to serve when the address answers. It
//! takes the arguments of `serve` and no other command.

mod egress;

use std::process::ExitCode;
use std::sync::Arc;

use egress::{Egress, Tcp, WAIT};

/// Why the harness stopped before it served.
fn refuse(why: &str) -> ExitCode {
    eprintln!("a2m-field-harness: {why}");
    ExitCode::from(2)
}

#[allow(
    unsafe_code,
    reason = "`run_in` changes the process environment, which is sound before any thread starts"
)]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("serve") {
        return refuse("this program only serves; run every other command with `anything2mcp`.");
    }
    let Some(address) = egress::public_address() else {
        return refuse("the public address it tries is not an address.");
    };
    // A runtime of its own, ended before the product starts its own, so that
    // `run_in` is still called before any other thread runs.
    let found = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(egress::check(&Tcp, address, WAIT)),
        Err(_) => return refuse("it could not start a runtime to try the network."),
    };
    if let Egress::Reached(what) = found {
        return refuse(&format!(
            "this network is not closed ({what}), so it refuses to serve."
        ));
    }
    let mut host = anything2mcp::cli::Host::of_process();
    host.prompt = Some(Arc::new(a2m_local::FieldPrompt::new(
        tokio::io::BufReader::new(tokio::io::stdin()),
        tokio::io::stdout(),
    )));
    // SAFETY: nothing but the check's own runtime has run, and it is gone:
    // no other thread is running, which `run_in` asks.
    unsafe { anything2mcp::cli::run_in(args, host) }
}
