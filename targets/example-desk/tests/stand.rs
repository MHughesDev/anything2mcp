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

    use example_desk::{Desk, Fault, LATE, Order, SECRET, Session};

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

    async fn text(conn: &zbus::Connection, bus: &str, path: &str) -> String {
        let reply = conn
            .call_method(
                Some(bus),
                path,
                Some("org.a11y.atspi.Text"),
                "GetText",
                &(0_i32, -1_i32),
            )
            .await
            .unwrap();
        reply.body().deserialize().unwrap()
    }

    async fn set(conn: &zbus::Connection, bus: &str, path: &str, value: &str) -> bool {
        let reply = conn
            .call_method(
                Some(bus),
                path,
                Some("org.a11y.atspi.EditableText"),
                "SetTextContents",
                &(value,),
            )
            .await
            .unwrap();
        reply.body().deserialize().unwrap()
    }

    async fn press(conn: &zbus::Connection, bus: &str) {
        conn.call_method(
            Some(bus),
            "/org/a11y/atspi/accessible/35",
            Some("org.a11y.atspi.Action"),
            "DoAction",
            &(0_i32,),
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn what_is_typed_is_stored_only_by_save_and_only_when_the_application_takes_it() {
        let Some(session) = session() else { return };
        let mut desk = Desk::start(&session, Order::example()).await.unwrap();
        let conn = a11y(&session).await;
        let app = ours(&conn).await;
        let (customer, new_customer, new_status, message, problem) = (
            "/org/a11y/atspi/accessible/6",
            "/org/a11y/atspi/accessible/37",
            "/org/a11y/atspi/accessible/39",
            "/org/a11y/atspi/accessible/41",
            "/org/a11y/atspi/accessible/43",
        );
        let role: u32 = call(
            &conn,
            &app,
            new_customer,
            "org.a11y.atspi.Accessible",
            "GetRole",
        )
        .await;
        assert_eq!(
            role, 79,
            "the node at {new_customer} is the customer's edit field"
        );

        // Typed is not stored: the labelled value shows the store until
        // Save.
        assert!(set(&conn, &app, new_customer, "Example Traders").await);
        let truth = desk.truth();
        assert_eq!(truth.stored["customer"], "Example Supplies");
        assert_eq!(text(&conn, &app, customer).await, "Example Supplies");
        assert_eq!(truth.sets.len(), 1);
        assert_eq!(truth.sets[0].field, "New customer");
        assert_eq!(truth.sets[0].value, "Example Traders");
        press(&conn, &app).await;
        let truth = desk.truth();
        assert_eq!((truth.writes, truth.stores), (1, 1));
        assert_eq!(truth.stored["customer"], "Example Traders");
        assert_eq!(text(&conn, &app, customer).await, "Example Traders");
        assert_eq!(text(&conn, &app, message).await, example_desk::SAVED);

        // The field cuts a long value; the application refuses a status
        // it does not know, says so, and stores nothing.
        assert!(set(&conn, &app, new_customer, &"x".repeat(40)).await);
        assert_eq!(
            text(&conn, &app, new_customer).await.chars().count(),
            example_desk::CUSTOMER_MOST
        );
        assert_eq!(text(&conn, &app, message).await, "");
        assert!(set(&conn, &app, new_status, "lost").await);
        press(&conn, &app).await;
        let truth = desk.truth();
        assert_eq!((truth.writes, truth.stores), (2, 1));
        assert!(text(&conn, &app, problem).await.contains("Status must be"));

        // A read-only field takes nothing, and no set of it is recorded.
        desk.fault(&Fault::ReadOnly("New status".to_owned()))
            .unwrap();
        assert!(!set(&conn, &app, new_status, "held").await);
        assert_eq!(desk.truth().sets.len(), 3);

        // Save that stores nothing and says nothing.
        desk.fault(&Fault::SaveSilently).unwrap();
        press(&conn, &app).await;
        let truth = desk.truth();
        assert_eq!((truth.writes, truth.stores), (3, 1));

        // Save that stores and then exits; started again, the application
        // shows what it stored, in its fields too.
        desk.fault(&Fault::ExitAfterSave).unwrap();
        assert!(set(&conn, &app, new_customer, "Example Holdings").await);
        let _ = conn
            .call_method(
                Some(app.as_str()),
                "/org/a11y/atspi/accessible/35",
                Some("org.a11y.atspi.Action"),
                "DoAction",
                &(0_i32,),
            )
            .await;
        assert_eq!(desk.truth().stores, 1, "the status `lost` is still refused");
        desk.restart().await.unwrap();
        let again = ours(&conn).await;
        assert_ne!(again, app);
        assert_eq!(text(&conn, &again, new_customer).await, "Example Traders");
        assert_eq!(text(&conn, &again, message).await, "");
    }

    /// The stand's application's name on the bus, from the registry's list.
    async fn ours(conn: &zbus::Connection) -> String {
        let apps: Vec<(String, zbus::OwnedObjectPath)> = call(
            conn,
            "org.a11y.atspi.Registry",
            "/org/a11y/atspi/accessible/root",
            "org.a11y.atspi.Accessible",
            "GetChildren",
        )
        .await;
        let mut found = None;
        for (bus, path) in &apps {
            let Ok(reply) = conn
                .call_method(
                    Some(bus.as_str()),
                    path.as_str(),
                    Some("org.freedesktop.DBus.Properties"),
                    "Get",
                    &("org.a11y.atspi.Accessible", "Name"),
                )
                .await
            else {
                continue;
            };
            let name: zbus::OwnedVariant = reply.body().deserialize().unwrap();
            if String::try_from(name).unwrap() == example_desk::APPLICATION {
                found = Some(bus.clone());
            }
        }
        found.expect("the stand's application is on the desktop")
    }
}

example_desk::stand_child!();

/// The stand's own tests on Windows: its window is up in a process of its
/// own, and its witness counts what a client sends its controls.
#[cfg(windows)]
mod on_windows {
    use std::time::{Duration, Instant};

    use example_desk::{Desk, LATE, Order, Session};
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        BM_CLICK, GetDlgItem, GetParent, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_GETTEXT,
    };
    use windows::core::HRESULT;

    /// The identifier of the edit labelled `Order number`, in the stand's
    /// table of its controls.
    const ORDER_NUMBER_EDIT: i32 = 102;

    /// Sends one message to a control of the stand, as another process,
    /// and gives the control's answer, or what the system said when the
    /// message was refused.
    #[allow(unsafe_code)] // one call into the system, explained below
    fn sent(
        control: isize,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> Result<usize, windows::core::Error> {
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
        if sent.0 == 0 {
            // What the system said, read at once, before any other call.
            return Err(windows::core::Error::from_thread());
        }
        Ok(result)
    }

    /// The same, for a message the control must take.
    fn send(control: isize, message: u32, wparam: usize, lparam: isize) -> usize {
        sent(control, message, wparam, lparam).unwrap_or_else(|why| {
            panic!(
                "the stand's control {control:#x} did not take message {message:#x}: {} ({})",
                why.message(),
                why.code()
            )
        })
    }

    /// The edit labelled `Order number`, beside the password edit in the
    /// stand's window.
    #[allow(unsafe_code)] // two calls into the system, explained below
    fn order_number_edit(password: isize) -> isize {
        // SAFETY: the handle is a control of the stand's window; both calls
        // only look the window and its control up.
        let edit = unsafe {
            GetParent(HWND(password as *mut _))
                .and_then(|window| GetDlgItem(Some(window), ORDER_NUMBER_EDIT))
        }
        .unwrap();
        edit.0 as isize
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

        // The system itself refuses another process's request for the text
        // of a password edit, before the edit sees it (the runner's first
        // runs showed `Access is denied`): no such read reaches the stand,
        // and it counts none.
        let handles = desk.handles();
        let mut buffer = vec![0_u16; 256];
        let refused = sent(
            handles.password,
            WM_GETTEXT,
            buffer.len(),
            buffer.as_mut_ptr() as isize,
        );
        assert_eq!(
            refused.map_err(|why| why.code()),
            Err(HRESULT::from_win32(ERROR_ACCESS_DENIED.0)),
            "another process read the password edit's text"
        );
        assert!(buffer.iter().all(|unit| *unit == 0));
        let truth = desk.truth();
        assert_eq!(truth.secret_reads, 0, "{:?}", truth.asked);

        // What reaches a control the stand counts: another process's
        // request for an edit's text is answered and counted, through the
        // same witness that counts every request for the password edit's
        // text or length.
        let number = order_number_edit(handles.password);
        let mut buffer = vec![0_u16; 256];
        let length = send(
            number,
            WM_GETTEXT,
            buffer.len(),
            buffer.as_mut_ptr() as isize,
        );
        assert_eq!(
            String::from_utf16_lossy(&buffer[..length]),
            Order::example().number
        );
        let truth = desk.truth();
        assert_eq!(
            truth.asked.get("edit:order_number.WM_GETTEXT"),
            Some(&1),
            "{:?}",
            truth.asked
        );
        assert_eq!(truth.secret_reads, 0, "{:?}", truth.asked);
        assert!(!truth.acted(), "{:?}", truth.asked);

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

/// The stand's own tests on macOS: its window is up in a process of its
/// own, and its witness counts what AppKit's accessibility asks of its
/// controls, against a client of the accessibility interface in this
/// process.
#[cfg(target_os = "macos")]
mod on_macos {
    #![allow(
        unsafe_code,
        reason = "a client of the accessibility interface; each call is explained"
    )]

    use std::ptr::NonNull;
    use std::time::{Duration, Instant};

    use example_desk::{Desk, LATE, Order, Session};
    use objc2_application_services::{AXError, AXUIElement};
    use objc2_core_foundation::{CFArray, CFRetained, CFString, CFType};

    fn copy(element: &AXUIElement, attribute: &str) -> Option<CFRetained<CFType>> {
        let name = CFString::from_str(attribute);
        let mut value: *const CFType = std::ptr::null();
        // SAFETY: a valid element and name; `value` is ours, and gets one
        // object the caller owns, or nothing.
        let said = unsafe { element.copy_attribute_value(&name, NonNull::from(&mut value)) };
        if said != AXError::Success {
            return None;
        }
        // SAFETY: the copy rule: the object is ours to release.
        NonNull::new(value.cast_mut()).map(|value| unsafe { CFRetained::from_raw(value) })
    }

    fn text(element: &AXUIElement, attribute: &str) -> String {
        copy(element, attribute)
            .and_then(|value| value.downcast::<CFString>().ok())
            .map(|text| text.to_string())
            .unwrap_or_default()
    }

    fn elements(element: &AXUIElement, attribute: &str) -> Vec<CFRetained<AXUIElement>> {
        let Some(array) =
            copy(element, attribute).and_then(|value| value.downcast::<CFArray>().ok())
        else {
            return Vec::new();
        };
        // SAFETY: an array of the interface holds Core Foundation objects;
        // each is checked to be an element.
        let array = unsafe { CFRetained::cast_unchecked::<CFArray<CFType>>(array) };
        (0..array.len())
            .filter_map(|at| array.get(at))
            .filter_map(|member| member.downcast::<AXUIElement>().ok())
            .collect()
    }

    /// Every element under `root`, as (role, subrole, title, element).
    fn walk(root: &AXUIElement) -> Vec<(String, String, String, CFRetained<AXUIElement>)> {
        let mut out = Vec::new();
        let mut stack = elements(root, "AXChildren");
        while let Some(element) = stack.pop() {
            if out.len() > 2_000 {
                break;
            }
            stack.extend(elements(&element, "AXChildren"));
            out.push((
                text(&element, "AXRole"),
                text(&element, "AXSubrole"),
                text(&element, "AXTitle"),
                element,
            ));
        }
        out
    }

    #[tokio::test]
    async fn the_window_is_up_and_its_control_interface_counts_what_it_is_asked() {
        let Some(session) = Session::start().unwrap() else {
            eprintln!("skipped: this process may not use the accessibility interface");
            return;
        };
        let desk = Desk::start(&session, Order::example()).await.unwrap();
        let truth = desk.truth();
        assert_eq!(truth.writes, 0);
        assert!(!truth.acted(), "{:?}", truth.asked);
        assert_eq!(truth.tracking, "");

        let process = i32::try_from(desk.process()).unwrap();
        // SAFETY: an element for the stand's process; nothing is asked yet.
        let application = unsafe { AXUIElement::new_application(process) };
        let start = Instant::now();
        let window = loop {
            if let Some(window) = elements(&application, "AXWindows").into_iter().next() {
                break window;
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "the stand's window was not listed within twenty seconds"
            );
            tokio::task::yield_now().await;
        };
        assert_eq!(text(&window, "AXTitle"), example_desk::WINDOW);
        let found = walk(&window);
        let seen: Vec<String> = found
            .iter()
            .map(|(role, subrole, title, _)| format!("{role}/{subrole} {title:?}"))
            .collect();

        // Each labelled field, the secure one too, names its label as its
        // title element, which is how a screen reader, and the desktop
        // pack, find what labels it: the very static text the window lists,
        // as the system compares elements.
        let titled: Vec<(String, String, bool)> = found
            .iter()
            .filter(|(role, _, _, _)| role == "AXTextField")
            .filter_map(|(_, _, _, field)| {
                copy(field, "AXTitleUIElement")?
                    .downcast::<AXUIElement>()
                    .ok()
            })
            .map(|label| {
                let theirs: &CFType = &label;
                let listed = found.iter().any(|(role, _, _, element)| {
                    let mine: &CFType = element;
                    role == "AXStaticText" && theirs == mine
                });
                (
                    text(&label, "AXValue"),
                    format!("{}/{}", text(&label, "AXRole"), text(&label, "AXSubrole")),
                    listed,
                )
            })
            .collect();
        for name in [
            "Order number",
            "Customer",
            "Status",
            "Tracking",
            "Reference",
            "Access code",
        ] {
            assert!(
                titled
                    .iter()
                    .any(|(title, _, listed)| title == name && *listed),
                "no field names as its title element the listed static text {name:?}; each title element as (value, role/subrole, listed): {titled:?}\n{seen:#?}"
            );
        }

        // A client that asks the secure field for its value is counted.
        let secret = found
            .iter()
            .find(|(_, subrole, _, _)| subrole == "AXSecureTextField")
            .unwrap_or_else(|| panic!("no secure text field in {seen:#?}"));
        let _ = copy(&secret.3, "AXValue");
        let truth = desk.truth();
        assert_eq!(truth.secret_reads, 1, "{:?}\n{seen:#?}", truth.asked);

        // A press of Save is counted as an act, and its action as a write.
        let save = found
            .iter()
            .find(|(role, _, title, _)| role == "AXButton" && title == "Save")
            .unwrap_or_else(|| panic!("no Save button in {seen:#?}"));
        let press = CFString::from_str("AXPress");
        // SAFETY: a valid element and action name; the stand's own test
        // presses its own Save.
        let said = unsafe { save.3.perform_action(&press) };
        assert_eq!(said, AXError::Success);
        let truth = desk.truth();
        assert!(truth.acted(), "{:?}", truth.asked);
        assert_eq!(truth.writes, 1, "{:?}", truth.asked);

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
