//! The stand itself: its application is on the accessibility bus through
//! the registry, and its control interface counts what it is asked. A
//! machine without the programs says so and skips; with
//! `A2M_REQUIRE_DESKTOP=1` it fails instead.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

/// The stand's own tests, on the one system it runs on.
#[cfg(target_os = "linux")]
mod on_linux {
    use std::time::Duration;

    use example_desk::{Desk, LATE, Order, SECRET, Session};

    fn session() -> Option<Session> {
        let session = Session::start().unwrap();
        if session.is_none() {
            eprintln!(
                "skipped: this machine has no session bus and accessibility bus to give the stand"
            );
        }
        session
    }

    async fn a11y(session: &Session) -> zbus::Connection {
        let bus = zbus::connection::Builder::address(session.address())
            .build()
            .await
            .unwrap();
        let reply = bus
            .call_method(
                Some("org.a11y.Bus"),
                "/org/a11y/bus",
                Some("org.a11y.Bus"),
                "GetAddress",
                &(),
            )
            .await
            .unwrap();
        let address: String = reply.body().deserialize().unwrap();
        zbus::connection::Builder::address(address.as_str())
            .build()
            .await
            .unwrap()
    }

    async fn call<R>(conn: &zbus::Connection, bus: &str, path: &str, iface: &str, member: &str) -> R
    where
        R: for<'d> zbus::wire::DynamicDeserialize<'d>,
    {
        let reply = conn
            .call_method(Some(bus), path, Some(iface), member, &())
            .await
            .unwrap();
        reply.body().deserialize::<R>().unwrap()
    }

    #[tokio::test]
    async fn the_application_is_on_the_bus_and_its_control_interface_counts_what_it_is_asked() {
        let Some(session) = session() else { return };
        let desk = Desk::start(&session, Order::example()).await.unwrap();
        let conn = a11y(&session).await;

        // The registry lists it on the desktop.
        let apps: Vec<(String, zbus::OwnedObjectPath)> = call(
            &conn,
            "org.a11y.atspi.Registry",
            "/org/a11y/atspi/accessible/root",
            "org.a11y.atspi.Accessible",
            "GetChildren",
        )
        .await;
        let mut ours = None;
        for (bus, path) in &apps {
            let reply = conn
                .call_method(
                    Some(bus.as_str()),
                    path.as_str(),
                    Some("org.freedesktop.DBus.Properties"),
                    "Get",
                    &("org.a11y.atspi.Accessible", "Name"),
                )
                .await
                .unwrap();
            let name: zbus::OwnedVariant = reply.body().deserialize().unwrap();
            if String::try_from(name).unwrap() == example_desk::APPLICATION {
                ours = Some(bus.clone());
            }
        }
        let ours = ours.expect("the stand's application is on the desktop");
        assert!(!desk.truth().acted());

        // A reader that asks the password field's text is counted.
        let field = "/org/a11y/atspi/accessible/17";
        let role: u32 = call(&conn, &ours, field, "org.a11y.atspi.Accessible", "GetRole").await;
        assert_eq!(role, 40, "the node at {field} is the password field");
        let reply = conn
            .call_method(
                Some(ours.as_str()),
                field,
                Some("org.a11y.atspi.Text"),
                "GetText",
                &(0_i32, -1_i32),
            )
            .await
            .unwrap();
        let shown: String = reply.body().deserialize().unwrap();
        assert_eq!(shown, SECRET);
        assert_eq!(desk.truth().secret_reads, 1);

        // A press of Save is counted as a write, and as an act.
        let save = "/org/a11y/atspi/accessible/35";
        let role: u32 = call(&conn, &ours, save, "org.a11y.atspi.Accessible", "GetRole").await;
        assert_eq!(role, 43, "the node at {save} is the Save button");
        conn.call_method(
            Some(ours.as_str()),
            save,
            Some("org.a11y.atspi.Action"),
            "DoAction",
            &(0_i32,),
        )
        .await
        .unwrap();
        let truth = desk.truth();
        assert_eq!(truth.writes, 1);
        assert!(truth.acted());

        // The late value appears when the control interface says so.
        assert_eq!(truth.tracking, "");
        desk.show_late(Duration::from_millis(10));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while desk.truth().tracking.is_empty() && tokio::time::Instant::now() < deadline {
            tokio::task::yield_now().await;
        }
        assert_eq!(desk.truth().tracking, LATE);
    }
}

example_desk::stand_child!();

/// The stand's own tests on Windows: its window is up in a process of its
/// own, and its witness counts what a client sends its controls.
#[cfg(windows)]
mod on_windows {
    use std::time::{Duration, Instant};

    use example_desk::{Desk, LATE, Order, Session};
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        BM_CLICK, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_GETTEXT,
    };

    /// Sends one message to a control of the stand, as another process.
    #[allow(unsafe_code)] // one call into the system, explained below
    fn send(control: isize, message: u32, wparam: usize, lparam: isize) {
        let mut result = 0_usize;
        // SAFETY: the handle is a window of the stand's; for `WM_GETTEXT`
        // the system copies the text into the test's buffer, whose length
        // `wparam` states; the call gives up after five seconds.
        let sent = unsafe {
            SendMessageTimeoutW(
                HWND(control as *mut _),
                message,
                WPARAM(wparam),
                LPARAM(lparam),
                SMTO_ABORTIFHUNG,
                5_000,
                Some(&raw mut result),
            )
        };
        assert_ne!(
            sent.0, 0,
            "the stand's control did not take message {message:#x}"
        );
    }

    #[tokio::test]
    async fn the_window_is_up_and_its_control_interface_counts_what_it_is_asked() {
        let Some(session) = Session::start().unwrap() else {
            eprintln!("skipped: this machine has no interactive desktop for the stand");
            return;
        };
        let desk = Desk::start(&session, Order::example()).await.unwrap();
        let truth = desk.truth();
        assert_eq!(truth.writes, 0);
        assert!(!truth.acted(), "{:?}", truth.asked);
        assert_eq!(truth.tracking, "");

        // A client that asks the password edit for its text is counted.
        let handles = desk.handles();
        let mut buffer = vec![0_u16; 256];
        send(
            handles.password,
            WM_GETTEXT,
            buffer.len(),
            buffer.as_mut_ptr() as isize,
        );
        let truth = desk.truth();
        assert_eq!(truth.secret_reads, 1, "{:?}", truth.asked);

        // A click of Save is counted as an act. Whether the button then
        // tells its window, which the stand counts as a write, can depend
        // on the window being active, which it never is.
        send(handles.save, BM_CLICK, 0, 0);
        let truth = desk.truth();
        assert!(truth.acted(), "{:?}", truth.asked);
        assert_eq!(
            truth.asked.get("button:save.BM_CLICK"),
            Some(&1),
            "{:?}",
            truth.asked
        );
        eprintln!("writes counted after one click of Save: {}", truth.writes);

        // The late value appears when the control interface says so.
        desk.show_late(Duration::from_millis(10));
        let start = Instant::now();
        while desk.truth().tracking.is_empty() && start.elapsed() < Duration::from_secs(5) {
            tokio::task::yield_now().await;
        }
        assert_eq!(desk.truth().tracking, LATE);
    }
}

#[test]
fn a_machine_without_the_programs_skips_unless_required() {
    // Either the programs are here, or the start says why it gives none.
    match example_desk::Session::start() {
        Ok(_) => {}
        Err(error) => assert!(example_desk::required(), "{error}"),
    }
}
