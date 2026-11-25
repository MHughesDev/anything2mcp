//! Test equipment for this crate's launch tests, never part of the product:
//! a program that stops as a browser does when it cannot make its sandbox.
//! It writes, to the file its `--log-file=` argument names, the line such a
//! browser writes, and the line in `STOP_LINE_FOR_TESTS` when that is set,
//! and exits without answering over its pipe.
//!
//! With `STOP_ORDER_FOR_TESTS=read_end_closed_first`, on Linux and macOS, it
//! closes the end of its pipe it reads from (descriptor 3) once it has
//! written its log, then sends one byte on the end it writes to (descriptor
//! 4) and waits to be stopped. A launch that waits for that byte writes its
//! first request to a pipe nobody reads, so the write fails, every time.

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
    if std::env::var("STOP_ORDER_FOR_TESTS").as_deref() == Ok("read_end_closed_first") {
        return read_end_closed_first();
    }
    std::process::ExitCode::from(1)
}

/// Closes the pipe's end this program reads from, says so on the end it
/// writes to, and waits to be stopped, holding that end open so that the
/// engine never reads the pipe's end before its write fails.
#[cfg(unix)]
fn read_end_closed_first() -> std::process::ExitCode {
    let said = *b"1";
    #[allow(
        unsafe_code,
        reason = "closing and writing to the descriptors the launch placed has no safe form"
    )]
    // SAFETY: descriptors 3 and 4 are the two pipe ends the launch placed
    // on this program before it started; nothing else in this program owns
    // or uses them, so closing 3 leaves no handle dangling, and the write
    // reads one byte from a live local buffer of that length.
    let (closed, written) = unsafe {
        (
            libc::close(3),
            libc::write(4, said.as_ptr().cast(), said.len()),
        )
    };
    if closed != 0 || written != 1 {
        return std::process::ExitCode::from(5);
    }
    loop {
        std::thread::park();
    }
}

#[cfg(not(unix))]
fn read_end_closed_first() -> std::process::ExitCode {
    std::process::ExitCode::from(6)
}
