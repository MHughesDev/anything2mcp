//! The stand as a program of its own, on Linux, for a test of an
//! application the desktop pack starts (decision 0467).
//!
//! On Linux the stand is hosted in the test's own process. A test of
//! starting an application needs a program the pack starts by its path:
//! the test copies its own binary under one of two names ([`STARTED`],
//! [`MUTE`]), in a folder only its user can write, and names the copy in
//! the surface. The copy, started with no argument and only the session
//! bus's address in its environment, serves the stand before the test
//! harness begins ([`crate::stand_child`] declares the start function on
//! Linux too) and never returns to it:
//!
//! - under [`STARTED`] it publishes the stand's application, showing
//!   [`crate::Order::example`], on the session's accessibility bus;
//! - under [`MUTE`] it publishes nothing, as a program whose accessibility
//!   is off, or that is not a desktop application, does.
//!
//! Either way it first writes its process number into the file
//! [`marker`] names, which is the test's ground truth that the pack started
//! it, and runs until the session's bus ends or it is stopped.

use std::path::{Path, PathBuf};

use crate::Order;

/// The name under which a copy of a test binary serves the stand's
/// application when started.
pub const STARTED: &str = "example-desk-app";
/// The name under which a copy of a test binary publishes nothing when
/// started.
pub const MUTE: &str = "example-desk-mute";

/// The file a started copy at `program` writes its process number into.
#[must_use]
pub fn marker(program: &Path) -> PathBuf {
    let mut name = program.as_os_str().to_owned();
    name.push(".started");
    PathBuf::from(name)
}

/// Serves the stand and ends the process, when this process is a copy
/// under one of the two names; otherwise returns at once.
pub(crate) fn serve_if_started() {
    let Ok(program) = std::env::current_exe() else {
        return;
    };
    let mute = match program.file_name().and_then(|name| name.to_str()) {
        Some(STARTED) => false,
        Some(MUTE) => true,
        _ => return,
    };
    let code = serve(&program, mute);
    std::process::exit(code);
}

fn serve(program: &Path, mute: bool) -> i32 {
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
