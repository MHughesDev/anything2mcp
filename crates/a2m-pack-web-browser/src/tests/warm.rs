//! A warm observing tab (decision 0999): kept between two reads of the same
//! setup and session, made fresh first, refusing what a new tab refuses,
//! and closed, not kept, when something unexpected happened in it.

use crate::SafeRequests;

use super::*;

const ITEM: &str = "/api/orders/A-100/items/L-101";

/// A write by the page's own script.
fn put(app: &TestApp) -> String {
    format!(
        "fetch({:?}, {{method: 'PUT', headers: {{'Content-Type': 'application/json'}}, \
         body: JSON.stringify({{description: 'Written by the page', quantity: 1, length_in: '1', version: 1}})}})\
         .then((r) => 'sent:' + r.status, () => 'failed')",
        app.url(ITEM)
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tab_kept_warm_is_handed_to_the_next_read_of_its_session_fresh_and_refusing_what_a_new_tab_refuses()
 {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("warm");
    let clerk = profile("clerk");
    let observer = stand.browsers.observer();
    let form = Address::new(&app.url(FORM)).unwrap();
    let none = SafeRequests::default();
    let warm = |session: &'static str| {
        let observer = observer.clone();
        let clerk = clerk.clone();
        let origins = origins(&app);
        let none = none.clone();
        async move {
            must(
                observer
                    .warm_observing_tab(&clerk, &environment(), &none, &origins, session)
                    .await,
            )
        }
    };

    // A first read leaves state a tab of its own holds: its session
    // storage, a global of the page, and its history.
    let tab = warm("session-1").await;
    assert!(!tab.reused());
    let target = tab.page.target.clone();
    must(tab.goto(&form, soon()).await);
    must(
        tab.page
            .as_the_page(
                "(() => { sessionStorage.setItem('left', 'by the read'); window.leftBehind = 1; history.pushState({}, '', '#moved'); return 'set'; })()",
                soon(),
            )
            .await,
    );
    assert!(tab.blocked().is_empty());
    observer
        .keep_warm(tab, "session-1", soon())
        .await
        .expect("a read that went as written is kept warm");
    assert_eq!(observer.warm_tabs(), 1);
    // A tab kept warm is not a tab held: the browser is between two runs.
    let listing = stand.browsers.list();
    assert_eq!(
        (listing[0].tabs, listing[0].health),
        (0, BrowserHealth::Idle)
    );

    // Another session, or another setup, is given a tab of its own.
    let other = warm("session-2").await;
    assert!(!other.reused());
    assert_ne!(other.page.target, target);
    drop(other);
    let quiet = must(
        observer
            .warm_observing_tab(
                &clerk,
                &environment(),
                &none,
                &origins(&app)
                    .and_refused_quietly([&Address::new("https://elsewhere.example").unwrap()]),
                "session-1",
            )
            .await,
    );
    assert!(!quiet.reused());
    drop(quiet);

    // The next read of the session is handed the same tab, fresh: on the
    // empty page, with one entry of history, and with nothing of the read
    // before on the application's page.
    let tab = warm("session-1").await;
    assert!(tab.reused());
    assert_eq!(tab.page.target, target);
    assert_eq!(observer.warm_tabs(), 0);
    assert_eq!(stand.browsers.list()[0].tabs, 1);
    must(tab.goto(&form, soon()).await);
    assert_eq!(
        must(
            tab.page
                .as_the_page(
                    "JSON.stringify([sessionStorage.length, typeof window.leftBehind, history.length, location.hash])",
                    soon(),
                )
                .await
        ),
        r#"[0,"undefined",2,""]"#
    );
    // It refuses what a new observing tab refuses, and the application
    // stored nothing.
    assert_eq!(
        must(tab.page.as_the_page(&put(&app), soon()).await),
        "failed"
    );
    must(tab.goto(&form, soon()).await);
    let blocked = tab.blocked();
    assert!(blocked.iter().any(|one| one.method == "PUT"), "{blocked:?}");
    assert!(app.writes().await.unwrap().is_empty());

    // A tab that saw something unexpected is closed, not kept: here a
    // refusal nobody took.
    assert_eq!(
        must(tab.page.as_the_page(&put(&app), soon()).await),
        "failed"
    );
    must(tab.goto(&form, soon()).await);
    assert!(observer.keep_warm(tab, "session-1", soon()).await.is_err());
    assert_eq!(observer.warm_tabs(), 0);
    let next = warm("session-1").await;
    assert!(!next.reused());
    assert_ne!(next.page.target, target);
    must(next.goto(&form, soon()).await);
    observer.keep_warm(next, "session-1", soon()).await.unwrap();

    // A browser that is closed takes its warm tabs with it.
    let cleanup = stand.browsers.close(&clerk).await;
    assert!(cleanup.complete, "{cleanup:?}");
    assert_eq!(observer.warm_tabs(), 0);
    let fresh = warm("session-1").await;
    assert!(!fresh.reused());
    drop(fresh);
    stand.end().await;
}
