//! The stand as a program of its own, on Linux and Windows, for a test of
//! an application the desktop pack starts (decisions 0467 and 0999).
//!
//! A test of starting an application needs a program the pack starts by
//! its path: the test copies its own binary under one of two names
//! ([`STARTED`], [`MUTE`], with `.exe` after it on Windows), in a folder
//! only its user can write, and names the copy in the surface. The copy,
//! started with no argument, serves the stand before the test harness
//! begins ([`crate::stand_child`] declares the start function) and never
//! returns to it:
//!
//! - under [`STARTED`] it serves the stand's application, showing
//!   [`crate::Order::example`]: on Linux it publishes it on the session's
//!   accessibility bus, whose address is the one variable it is given; on
//!   Windows it shows the stand's window, with no control interface, since
//!   the pack gives it nothing on its standard input;
//! - under [`MUTE`] it publishes nothing and shows no window, as a program
//!   whose accessibility is off, or that is not a desktop application, does.
//!
//! Either way it first writes its process number into the file
//! [`marker`] names, which is the test's ground truth that the pack started
//! it, and runs until it is stopped (on Linux, also until the session's bus
//! ends).

use std::path::{Path, PathBuf};

/// The name under which a copy of a test binary serves the stand's
/// application when started, without the `.exe` it has on Windows.
pub const STARTED: &str = "example-desk-app";
/// The name under which a copy of a test binary publishes nothing when
/// started, without the `.exe` it has on Windows.
pub const MUTE: &str = "example-desk-mute";

/// The file a started copy at `program` writes its process number into.
#[must_use]
pub fn marker(program: &Path) -> PathBuf {
    let mut name = program.as_os_str().to_owned();
    name.push(".started");
    PathBuf::from(name)
}

/// The program a running process runs, as the system reports it, or
/// `None` when no process of that number runs: what a test asks of the
/// process a started copy wrote into its marker.
#[must_use]
pub fn program_of(process: u32) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_link(format!("/proc/{process}/exe")).ok()
    }
    #[cfg(windows)]
    {
        windows_program_of(process)
    }
}

#[cfg(windows)]
#[allow(unsafe_code)] // calls into the system, explained below
fn windows_program_of(process: u32) -> Option<PathBuf> {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::core::PWSTR;
    // SAFETY: three plain values; the handle given is closed below.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process) }.ok()?;
    let mut code = 0_u32;
    let mut buffer = vec![0_u16; 32_768];
    let mut length = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
    // SAFETY: the handle is open; `code`, `buffer`, and `length` are ours
    // and outlive the calls, and `buffer` is writable for `length`
    // characters.
    let program = unsafe {
        let running =
            GetExitCodeProcess(handle, &raw mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
        let named = running
            && QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &raw mut length,
            )
            .is_ok();
        let _ = CloseHandle(handle);
        named
    };
    if !program {
        return None;
    }
    let used = buffer.get(..length as usize)?;
    Some(PathBuf::from(String::from_utf16(used).ok()?))
}

/// Serves the stand and ends the process, when this process is a copy
/// under one of the two names; otherwise returns at once.
pub(crate) fn serve_if_started() {
    let Ok(program) = std::env::current_exe() else {
        return;
    };
    let mute = match program.file_stem().and_then(|name| name.to_str()) {
        Some(STARTED) => false,
        Some(MUTE) => true,
        _ => return,
    };
    let code = serve(&program, mute);
    std::process::exit(code);
}

#[cfg(target_os = "linux")]
fn serve(program: &Path, mute: bool) -> i32 {
    use crate::Order;
    let Ok(address) = std::env::var("DBUS_SESSION_BUS_ADDRESS") else {
        return 2;
    };
    if std::fs::write(marker(program), std::process::id().to_string()).is_err() {
        return 3;
    }
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    else {
        return 4;
    };
    runtime.block_on(async move {
        let app = if mute {
            None
        } else {
            match crate::app::App::start(&address, Order::example()).await {
                Ok(app) => Some(app),
                Err(_) => return 5,
            }
        };
        // Runs until the session's bus ends.
        if let Ok(bus) = zbus::connection::Builder::address(address.as_str())
            .build()
            .await
        {
            use futures_core::Stream as _;
            let mut stream = zbus::MessageStream::from(&bus);
            while std::future::poll_fn(|cx| std::pin::Pin::new(&mut stream).poll_next(cx))
                .await
                .is_some()
            {}
        }
        drop(app);
        0
    })
}

#[cfg(windows)]
fn serve(program: &Path, mute: bool) -> i32 {
    if std::fs::write(marker(program), std::process::id().to_string()).is_err() {
        return 3;
    }
    if mute {
        // Shows nothing and runs until it is stopped: no wait on anything
        // but the stop, which ends the process.
        loop {
            std::thread::park();
        }
    }
    crate::win::window::serve_started()
}
