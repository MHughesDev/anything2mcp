//! Test equipment for this crate's launch tests, never part of the product:
//! a program that stops as a browser does when it cannot make its sandbox.
//! It writes, to the file its `--log-file=` argument names, the line such a
//! browser writes, and the line in `STOP_LINE_FOR_TESTS` when that is set,
//! and exits without answering over its pipe.

use std::io::Write as _;

const SANDBOX: &str = "[10:10:1125/023650.374426:FATAL:content/browser/zygote_host/zygote_host_impl_linux.cc:242] Zygote process exited prematurely with exit code 82 (setuid sandbox: failed to move to new namespaces)";

fn main() -> std::process::ExitCode {
    let Some(log) = std::env::args_os().find_map(|argument| {
        argument
            .to_str()
            .and_then(|text| text.strip_prefix("--log-file="))
            .map(std::path::PathBuf::from)
    }) else {
        return std::process::ExitCode::from(2);
    };
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    else {
        return std::process::ExitCode::from(3);
    };
    let mut text = format!("{SANDBOX}\n");
    if let Ok(line) = std::env::var("STOP_LINE_FOR_TESTS") {
        text.push_str(&format!("[10:10:1125/023650.4:ERROR:a/b.cc:1] {line}\n"));
    }
    if file.write_all(text.as_bytes()).is_err() {
        return std::process::ExitCode::from(4);
    }
    std::process::ExitCode::from(1)
}
