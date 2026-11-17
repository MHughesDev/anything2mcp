//! Navigating, locating, reading, waiting, and snapshots.

use crate::{Primitive, RawValue, ReadWhat, SnapshotLimits, TextMatch};

use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn a_field_is_located_by_role_and_name_and_read_and_only_one_match_will_do() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("looking");
    let browser = stand.open(&profile("clerk")).await;
    let tab = must(browser.acting_tab(&environment()).await);
    assert_eq!(browser.health(), BrowserHealth::Busy);

    let loaded = must(
        tab.goto(&Address::new(&app.url(FORM)).unwrap(), soon())
            .await,
    );
    assert_eq!(loaded.status, Some(200));
    assert_eq!(loaded.address, app.url(FORM));
    assert_eq!(loaded.title, "Line item - Example Orders");

    // Role and name: the first choice. The stand says what it stored.
    let stored = app.stored_item("A-100", "L-101").await.unwrap().unwrap();
    must(
        tab.wait(&Primitive::Visible(textbox("Description")), soon())
            .await,
    );
    let description = must(tab.locate(&textbox("Description"), soon()).await);
    for _ in 0..5 {
        assert_eq!(
            must(tab.read(&description, &ReadWhat::Value, soon()).await),
            RawValue::Text(stored.description.clone())
        );
    }
    assert_eq!(
        must(
            tab.read(
                &description,
                &ReadWhat::Attribute {
                    name: "name".to_owned()
                },
                soon()
            )
            .await
        ),
        RawValue::Text("description".to_owned())
    );
    assert_eq!(
        must(tab.read(&description, &ReadWhat::Checked, soon()).await),
        RawValue::Absent
    );

    // The other ways to match find the same kind of thing.
    let by_label = Query::new(By::Label {
        text: "Quantity".to_owned(),
    });
    let quantity = must(tab.locate(&by_label, soon()).await);
    assert_eq!(
        must(tab.read(&quantity, &ReadWhat::Value, soon()).await),
        RawValue::Text(stored.quantity.to_string())
    );
    let heading = must(
        tab.locate(&Query::role("heading", "Line Item Details"), soon())
            .await,
    );
    assert_eq!(
        must(tab.read(&heading, &ReadWhat::Text, soon()).await),
        RawValue::Text("Line Item Details".to_owned())
    );
    let by_text = Query::new(By::Text {
        text: "inches".to_owned(),
        contains: false,
    });
    must(tab.locate(&by_text, soon()).await);
    // Inside one element: the length, within the group it belongs to.
    let within = textbox("Length").within(Query::role("group", "Dimensions"));
    must(tab.locate(&within, soon()).await);
    let nowhere = textbox("Description").within(Query::role("group", "Dimensions"));
    assert_eq!(
        tab.locate(&nowhere, soon()).await.unwrap_err(),
        BrowserError::NotFound
    );

    // Exactly one, or a failure. The first of several is never picked.
    assert_eq!(
        tab.locate(&css("#line-item input"), soon())
            .await
            .unwrap_err(),
        BrowserError::Ambiguous { count: 4 }
    );
    assert_eq!(
        tab.locate(&Query::role("button", "Delete"), soon())
            .await
            .unwrap_err(),
        BrowserError::NotFound
    );
    // The name is exact: a part of it matches nothing.
    assert_eq!(
        tab.locate(&textbox("Descript"), soon()).await.unwrap_err(),
        BrowserError::NotFound
    );

    // The snapshot is the accessibility tree, in the reference form.
    let snapshot = must(tab.snapshot(&SnapshotLimits::default(), soon()).await);
    let text = snapshot.to_text();
    assert!(
        text.starts_with(&format!(
            "page \"Line item - Example Orders\" {}\n",
            app.url(FORM)
        )),
        "{text}"
    );
    assert!(
        text.contains("heading \"Line Item Details\" level=2"),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "textbox \"Description\" value=\"{}\" required",
            stored.description
        )),
        "{text}"
    );
    assert!(text.contains("button \"Save\""), "{text}");
    assert!(!snapshot.cut);
    // Twice the same page, twice the same snapshot.
    assert_eq!(
        must(tab.snapshot(&SnapshotLimits::default(), soon()).await),
        snapshot
    );

    // The browser says it is automated; the engine does not hide it.
    assert_eq!(
        must(
            tab.page
                .as_the_page("String(navigator.webdriver)", soon())
                .await
        ),
        "true"
    );

    // An element belongs to one page generation.
    let other = must(
        tab.goto(&Address::new(&app.url("/orders/NOPE")).unwrap(), soon())
            .await,
    );
    assert_eq!(other.status, Some(404));
    assert_eq!(
        tab.read(&description, &ReadWhat::Value, soon())
            .await
            .unwrap_err(),
        BrowserError::NotFound
    );
    // And to one tab.
    let second = must(browser.acting_tab(&environment()).await);
    must(
        second
            .goto(&Address::new(&app.url(FORM)).unwrap(), soon())
            .await,
    );
    let theirs = must(second.locate(&textbox("Description"), soon()).await);
    assert_eq!(
        tab.read(&theirs, &ReadWhat::Value, soon())
            .await
            .unwrap_err(),
        BrowserError::NotFound
    );

    // A page that does not load says why, and nothing was sent.
    let unused = {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.local_addr().unwrap().port()
    };
    let refused = tab
        .goto(
            &Address::new(&format!("http://127.0.0.1:{unused}/")).unwrap(),
            soon(),
        )
        .await
        .unwrap_err();
    assert!(matches!(refused, BrowserError::NotLoaded(_)), "{refused}");

    drop((tab, second));
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wait_ends_on_an_event_of_the_page_and_never_on_a_guess() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("waiting");
    let browser = stand.open(&profile("clerk")).await;
    let tab = must(browser.acting_tab(&environment()).await);

    // An element that arrives late is noticed when it arrives.
    let late = page_of(
        "<!doctype html><title>Late</title><p id=old>Loading</p><script>\
         setTimeout(() => { const b = document.createElement('button'); b.textContent = 'Late'; \
         document.body.append(b); document.getElementById('old').remove(); }, 150);</script>",
    );
    must(tab.page.goto_text(&late, soon()).await);
    let button = Query::role("button", "Late");
    assert_eq!(
        tab.locate(&button, soon()).await.unwrap_err(),
        BrowserError::NotFound
    );
    must(
        tab.wait(
            &Primitive::All(vec![
                Primitive::Visible(button.clone()),
                Primitive::Hidden(css("#old")),
            ]),
            soon(),
        )
        .await,
    );
    must(tab.locate(&button, soon()).await);
    must(tab.wait(&Primitive::Enabled(button.clone()), soon()).await);

    // A condition that never holds ends at its deadline, and is transient.
    let never = tab
        .wait(
            &Primitive::Visible(css("#never")),
            Deadline::after(Duration::from_millis(300)),
        )
        .await
        .unwrap_err();
    assert_eq!(
        never,
        BrowserError::Timeout {
            dispatch: Dispatch::NotSent
        }
    );
    assert_eq!(never.failure().unwrap().code(), &codes::TIMEOUT);

    // A wait that is under way when the page moves on is re-armed in the
    // new document, by the event that replaced the old one.
    must(
        tab.goto(&Address::new(&app.url(FORM)).unwrap(), soon())
            .await,
    );
    must(
        tab.page
            .as_the_page(
                "setTimeout(() => { location.href = '/orders/A-100'; }, 150); 'armed'",
                soon(),
            )
            .await,
    );
    must(
        tab.wait(
            &Primitive::Visible(Query::role("table", "Line items")),
            soon(),
        )
        .await,
    );
    must(
        tab.wait(
            &Primitive::Url(TextMatch {
                text: app.url("/orders/A-100"),
                contains: false,
            }),
            soon(),
        )
        .await,
    );
    // A text that the page shows, on the element that shows it.
    let first_row = Query::role("link", "L-101");
    must(
        tab.wait(
            &Primitive::Text(
                first_row,
                TextMatch {
                    text: "L-10".to_owned(),
                    contains: true,
                },
            ),
            soon(),
        )
        .await,
    );
    drop(tab);
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_password_is_never_read_and_a_known_secret_never_leaves() {
    const SECRET: &str = "planted-secret-value";
    let mut settings = test_support::settings();
    settings.max_read_chars = 64;
    let stand = Stand::with_redactor("secrets", settings, &[], Arc::new(Knows(SECRET)));
    let browser = stand.open(&profile("clerk")).await;
    let tab = must(browser.acting_tab(&environment()).await);
    let page = page_of(&format!(
        "<!doctype html><title>Sign in {SECRET}</title>\
         <input type=password aria-label=Password value=hunter2-password>\
         <input aria-label=Token value={SECRET}>\
         <input aria-label=Shown autocomplete=current-password value=hunter2-password>\
         <p id=note>The token is {SECRET}.</p>\
         <p id=long>{}</p>\
         <div id=host></div>\
         <script>document.getElementById('host').attachShadow({{mode: 'open'}}).innerHTML = \
         '<button>Inside</button>';</script>",
        "long ".repeat(40)
    ));
    let loaded = must(tab.page.goto_text(&page, soon()).await);
    assert!(!loaded.title.contains(SECRET), "{}", loaded.title);

    // A password input is found and never read, whatever is asked of it.
    for name in ["Password", "Shown"] {
        let password = must(tab.locate(&textbox(name), soon()).await);
        for what in [
            ReadWhat::Value,
            ReadWhat::Text,
            ReadWhat::Attribute {
                name: "value".to_owned(),
            },
        ] {
            let refused = tab.read(&password, &what, soon()).await.unwrap_err();
            assert_eq!(refused, BrowserError::PasswordField);
            assert_eq!(
                refused.failure().unwrap().code(),
                &codes::PASSWORD_READ_REFUSED
            );
        }
    }
    // A wait on its text does not hold either: it would be a read.
    let guessed = Primitive::Text(
        textbox("Password"),
        TextMatch {
            text: "hunter2-password".to_owned(),
            contains: false,
        },
    );
    assert!(
        tab.wait(&guessed, Deadline::after(Duration::from_millis(300)))
            .await
            .is_err()
    );

    // A value the redactor knows is replaced before it leaves the crate.
    let token = must(tab.locate(&textbox("Token"), soon()).await);
    assert_eq!(
        must(tab.read(&token, &ReadWhat::Value, soon()).await),
        RawValue::Text("[redacted]".to_owned())
    );
    let note = must(tab.locate(&css("#note"), soon()).await);
    assert_eq!(
        must(tab.read(&note, &ReadWhat::Text, soon()).await),
        RawValue::Text("The token is [redacted].".to_owned())
    );
    let snapshot = must(tab.snapshot(&SnapshotLimits::default(), soon()).await).to_text();
    assert!(!snapshot.contains(SECRET), "{snapshot}");
    assert!(!snapshot.contains("hunter2-password"), "{snapshot}");
    assert!(snapshot.contains("textbox \"Password\""), "{snapshot}");

    // What is read has a limit, and nothing is cut silently.
    let long = must(tab.locate(&css("#long"), soon()).await);
    assert_eq!(
        tab.read(&long, &ReadWhat::Text, soon()).await.unwrap_err(),
        BrowserError::TooLarge { limit: 64 }
    );

    // An open shadow root is looked through.
    must(tab.locate(&Query::role("button", "Inside"), soon()).await);
    drop(tab);
    stand.end().await;
}
