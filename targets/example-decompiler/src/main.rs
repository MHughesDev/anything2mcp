//! A stand for an installed decompiling engine.
//!
//! It is handed one file, as its one argument, and reads it as bytes. It
//! prints, on standard output, one JSON line for each function it made text
//! of and a last line that says how many it printed; that is the form
//! `a2m-provider`'s decompiler reads. What it does besides is chosen by
//! `EXAMPLE_DECOMPILER_MODE`, and every start is appended, as one JSON line
//! of its arguments, to the file `EXAMPLE_DECOMPILER_LOG` names, so that a
//! test can ask what it was given.
//!
//! Modes: unset or `ok`; `fail` (prints nothing, exits with status 3);
//! `garbage` (prints a line that is not JSON); `no_end` (leaves out the last
//! line); `secret` (one function's text holds `EXAMPLE_DECOMPILER_SECRET`);
//! `flood` (prints 2000 functions).

use std::io::Write as _;
use std::process::ExitCode;

use serde_json::json;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(log) = std::env::var_os("EXAMPLE_DECOMPILER_LOG")
        && let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
    {
        let _ = writeln!(file, "{}", json!({ "args": args }));
    }
    let mode = std::env::var("EXAMPLE_DECOMPILER_MODE").unwrap_or_default();
    let [path] = args.as_slice() else {
        eprintln!("usage: example-decompiler <file>");
        return ExitCode::from(2);
    };
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("cannot read the file");
        return ExitCode::from(2);
    };
    let first = bytes.first().copied().unwrap_or(0);
    let mut out = std::io::stdout().lock();
    if mode == "fail" {
        eprintln!("the analysis failed");
        return ExitCode::from(3);
    }
    if mode == "garbage" {
        let _ = writeln!(out, "this is not a function");
        return ExitCode::SUCCESS;
    }
    let count: u32 = if mode == "flood" { 2000 } else { 3 };
    for index in 0..count {
        let text = if mode == "secret" && index == 1 {
            format!(
                "int fn_{index}(void) {{ return {}; }}",
                std::env::var("EXAMPLE_DECOMPILER_SECRET").unwrap_or_default()
            )
        } else {
            format!("int fn_{index}(void) {{ return {first}; }}")
        };
        let _ = writeln!(
            out,
            "{}",
            json!({ "function": format!("fn_{index}"), "address": format!("0x{:x}", 0x1000 + index * 16), "text": text })
        );
    }
    if mode != "no_end" {
        let _ = writeln!(out, "{}", json!({ "end": true, "functions": count }));
    }
    ExitCode::SUCCESS
}
