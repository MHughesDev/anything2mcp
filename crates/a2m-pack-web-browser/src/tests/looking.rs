//! Navigating, locating, reading, waiting, and snapshots.

use crate::{MASK, Primitive, RawValue, ReadWhat, SnapshotLimits, TextMatch};

use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn a_field_is_located_by_role_and_name_and_read_and_only_one_match_will_do() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("looking");
    let browser = stand.open(&profile("clerk")).await;
    let tab = must(browser.acting_tab(&environment(), &origins(&app)).await);
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
    // And to one tab. This one is sent with a query, which is where
    // tokens travel: `Loaded` keeps it, for the crates above to decide
    // by, and a snapshot, which is kept, is cut there.
    let second = must(browser.acting_tab(&environment(), &origins(&app)).await);
    let with_query = format!("{}?ticket=example-token-in-query", app.url(FORM));
    let loaded = must(
        second
            .goto(&Address::new(&with_query).unwrap(), soon())
            .await,
    );
    assert_eq!(loaded.address, with_query);
    let kept = must(second.snapshot(&SnapshotLimits::default(), soon()).await);
    assert_eq!(kept.address, app.url(FORM));
    assert!(
        !kept.to_text().contains("example-token-in-query"),
        "{}",
        kept.to_text()
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
    let nobody = Address::new(&format!("http://127.0.0.1:{unused}/")).unwrap();
    let third = must(
        browser
            .acting_tab(&environment(), &Origins::of([&nobody]))
            .await,
    );
    let refused = third.goto(&nobody, soon()).await.unwrap_err();
    assert!(matches!(refused, BrowserError::NotLoaded(_)), "{refused}");

    drop((tab, second, third));
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wait_ends_on_an_event_of_the_page_and_never_on_a_guess() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("waiting");
    let browser = stand.open(&profile("clerk")).await;
    let tab = must(browser.acting_tab(&environment(), &origins(&app)).await);

    // An element that arrives late is noticed when it arrives. Nothing in
    // this page runs on a timer: the element is added when the test says
    // so, so that "not there yet" and "there now" are facts and not a
    // guess at how fast the machine is.
    let late = page_of("<!doctype html><title>Late</title><p id=old>Loading</p>");
    must(tab.page.goto_text(&late, soon()).await);
    let button = Query::role("button", "Late");
    assert_eq!(
        tab.locate(&button, soon()).await.unwrap_err(),
        BrowserError::NotFound
    );
    let arrived = Primitive::All(vec![
        Primitive::Visible(button.clone()),
        Primitive::Hidden(css("#old")),
    ]);
    let arrive = "(() => { const b = document.createElement('button'); b.textContent = 'Late'; \
                  document.body.append(b); document.getElementById('old').remove(); return 'added'; })()";
    // The wait is begun first and the page's change second: both travel on
    // the one link in that order, so the wait is under way in the page
    // when the element is added, and an observer has to notice it.
    let (waited, added) = tokio::join!(
        tab.wait(&arrived, soon()),
        tab.page.as_the_page(arrive, soon())
    );
    assert_eq!(must(added), "added");
    must(waited);
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

    // Asked whether a condition holds now, the page answers at once, on
    // its own word: "no" is an answer. A page that gives none by the
    // deadline is a timeout, never "no": here the page is kept busy for
    // seconds by a script sent first on the one link, so it cannot answer
    // the look sent after it within its 300 ms.
    assert!(must(
        tab.holds(&Primitive::Enabled(button.clone()), soon()).await
    ));
    assert!(!must(
        tab.holds(&Primitive::Visible(css("#never")), soon()).await
    ));
    let busy =
        "(() => { const end = Date.now() + 3000; while (Date.now() < end) {} return 'busy'; })()";
    let nowhere = Primitive::Visible(css("#never"));
    let (kept_busy, busied) = tokio::join!(
        tab.page.as_the_page(busy, soon()),
        tab.holds(&nowhere, Deadline::after(Duration::from_millis(300)))
    );
    assert_eq!(must(kept_busy), "busy");
    assert!(
        matches!(busied, Err(BrowserError::Timeout { .. })),
        "{busied:?}"
    );

    // A wait that is under way when the page moves on is re-armed in the
    // new document, by the event that replaced the old one. Again the test
    // says when: the wait is begun on the form, where the table is not,
    // and then the page is told to move.
    must(
        tab.goto(&Address::new(&app.url(FORM)).unwrap(), soon())
            .await,
    );
    let table = Query::role("table", "Line items");
    assert_eq!(
        tab.locate(&table, soon()).await.unwrap_err(),
        BrowserError::NotFound
    );
    let there = Primitive::Visible(table.clone());
    let (waited, _moving) = tokio::join!(
        tab.wait(&there, soon()),
        // Its own answer may be lost with the document it leaves.
        tab.page
            .as_the_page("location.href = '/orders/A-100'; 'moving'", soon())
    );
    must(waited);
    must(tab.locate(&table, soon()).await);
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
    let tab = must(browser.acting_tab(&environment(), &data_pages()).await);
    let page = page_of(&format!(
        "<!doctype html><title>Sign in {SECRET}</title>\
         <input type=password aria-label=Password value=hunter2-password>\
         <input aria-label=Token value={SECRET}>\
         <input aria-label=Shown autocomplete=current-password value=hunter2-password>\
         <p id=note>The token is {SECRET}.</p>\
         <p id=long>{}</p>\
         <div id=host></div>\
         <div id=sealed></div>\
         <script>document.getElementById('host').attachShadow({{mode: 'open'}}).innerHTML = \
         '<button>Inside</button>';\
         document.getElementById('sealed').attachShadow({{mode: 'closed'}}).innerHTML = \
         '<input aria-label=Sealed autocomplete=current-password value=hunter2-password>';</script>",
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
    // A snapshot masks every password input, also one inside a closed
    // shadow root, which only the browser itself can see into.
    let snapshot = must(tab.snapshot(&SnapshotLimits::default(), soon()).await).to_text();
    assert!(!snapshot.contains(SECRET), "{snapshot}");
    assert!(!snapshot.contains("hunter2-password"), "{snapshot}");
    assert!(snapshot.contains("textbox \"Password\""), "{snapshot}");
    assert!(
        snapshot.contains(&format!("textbox \"Sealed\" value=\"{MASK}\"")),
        "{snapshot}"
    );

    // A selector cannot ask what a password input was served with: a
    // right guess and a wrong one are refused alike, in a locate and in a
    // wait, however the attribute's name is spelled.
    for guess in [
        "input[type=password][value^='h']",
        "input[type=password][value^='x']",
        "input[ VALUE ^= 'hunter2' ]",
        "input[\\76 alue^='h']",
        "body:has(input[value*='hunter2'])",
    ] {
        assert_eq!(
            tab.locate(&css(guess), soon()).await.unwrap_err(),
            BrowserError::PasswordField,
            "{guess}"
        );
        assert_eq!(
            tab.wait(&Primitive::Visible(css(guess)), soon())
                .await
                .unwrap_err(),
            BrowserError::PasswordField,
            "{guess}"
        );
    }
    // A selector that asks nothing about a value is answered as before.
    must(tab.locate(&css("input[type=password]"), soon()).await);

    // A message is redacted whole before it is cut to length: a secret
    // that lies across the cut leaves no part of itself. The browser's
    // complaint about a selector quotes the selector.
    // Wherever the cut falls in the secret, or before it, or after it.
    for before in 100..160 {
        let across = format!("p[title='{}{SECRET}'] >", "x".repeat(before));
        let complaint = tab.locate(&css(&across), soon()).await.unwrap_err();
        let BrowserError::Unexpected(complaint) = complaint else {
            panic!("{complaint}");
        };
        assert!(complaint.contains("xxxx"), "{complaint}");
        assert!(!complaint.contains("plan"), "{before}: {complaint}");
    }
    let whole = format!("{}{SECRET} and more", "x".repeat(190));
    let message = crate::tab::shown(&Knows(SECRET), &whole);
    assert!(message.ends_with("xx[redacted]…"), "{message}");

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

/// A section that has a name is a region, and one without a name is
/// generic, as the HTML accessibility mapping says: a release that wraps
/// a form in a named card gives a screen model a region to place its
/// fields in (package 9.1).
#[tokio::test(flavor = "multi_thread")]
async fn a_named_section_is_a_region_and_an_unnamed_one_is_not() {
    let stand = Stand::with_browser("regions");
    let browser = stand.open(&profile("clerk")).await;
    let tab = must(browser.acting_tab(&environment(), &data_pages()).await);
    let page = page_of(
        "<!doctype html><title>Cards</title>\
         <section aria-labelledby=t><h2 id=t>Line Item Details</h2><input aria-label=Quantity></section>\
         <section aria-label=Notes><input aria-label=Note></section>\
         <section><input aria-label=Other></section>",
    );
    must(tab.page.goto_text(&page, soon()).await);
    let card = Query::role("region", "Line Item Details");
    must(
        tab.locate(&textbox("Quantity").within(card.clone()), soon())
            .await,
    );
    must(
        tab.locate(
            &textbox("Note").within(Query::role("region", "Notes")),
            soon(),
        )
        .await,
    );
    assert_eq!(
        tab.locate(&textbox("Note").within(card), soon())
            .await
            .unwrap_err(),
        BrowserError::NotFound
    );
    // The third section has no name, and is not a region of an empty one.
    assert_eq!(
        tab.locate(&textbox("Other").within(Query::role("region", "")), soon())
            .await
            .unwrap_err(),
        BrowserError::NotFound
    );
    must(tab.locate(&textbox("Other"), soon()).await);
    drop(tab);
    stand.end().await;
}
