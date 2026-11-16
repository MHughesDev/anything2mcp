//! Runs the test application on a local port, for people and for tools that are
//! not Rust tests. Rust tests start it in-process through `example_app::harness`.

use std::process::ExitCode;

use tokio::net::TcpListener;

/// The port the walkthrough in `docs/reference/walkthrough.md` uses.
const DEFAULT_PORT: u16 = 4010;

fn port_from(mut args: impl Iterator<Item = String>) -> Result<u16, String> {
    match (args.next().as_deref(), args.next(), args.next()) {
        (None, None, None) => Ok(DEFAULT_PORT),
        (Some("--port"), Some(port), None) => port
            .parse()
            .map_err(|_| format!("{port} is not a port number")),
        _ => Err("usage: example-app [--port <number>]".to_owned()),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let port = match port_from(std::env::args().skip(1)) {
        Ok(port) => port,
        Err(message) => {
            eprintln!("example-app: {message}");
            return ExitCode::from(2);
        }
    };
    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(listener) => listener,
        Err(e) => {
            eprintln!("example-app: cannot listen on port {port}: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("example-app is listening on http://127.0.0.1:{port}");
    match example_app::serve(listener).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("example-app: stopped: {e}");
            ExitCode::FAILURE
        }
    }
}
