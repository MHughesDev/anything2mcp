//! The observing tab: it reads, and its page cannot write.

use std::collections::BTreeMap;

use crate::{RawValue, ReadWhat, SafeRequest, SafeRequests, codes};

use super::*;

const ITEM: &str = "/api/orders/A-100/items/L-101";

/// What a page does by itself: a write, sent by its own script.
fn put(app: &TestApp) -> String {
    format!(
        "fetch({:?}, {{method: 'PUT', headers: {{'Content-Type': 'application/json'}}, \
         body: JSON.stringify({{description: 'Written by the page', quantity: 1, length_in: '1', version: 1}})}})\
         .then((r) => 'sent:' + r.status, () => 'failed')",
        app.url(ITEM)
    )
}

/// The same write, from a worker the page starts.
fn put_from_worker(app: &TestApp) -> String {
    let script = format!(
        "fetch({:?}, {{method: 'PUT', headers: {{'Content-Type': 'application/json'}}, \
         body: JSON.stringify({{description: 'Written by a worker', quantity: 1, length_in: '1', version: 1}})}})\
         .then((r) => postMessage('sent:' + r.status), () => postMessage('failed'))",
        app.url(ITEM)
    );
    format!(
        "new Promise((resolve) => {{ const worker = new Worker(URL.createObjectURL(new Blob([{script:?}]))); \
         worker.onmessage = (event) => resolve(event.data); worker.onerror = () => resolve('worker error'); }})"
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn an_observing_tab_reads_and_nothing_its_page_does_reaches_the_system() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("observing");
    let clerk = profile("clerk");
    let form = Address::new(&app.url(FORM)).unwrap();

    // First, that the test means something: in an acting tab the same
    // scripts do write, from the page and from a worker.
    let browser = stand.open(&clerk).await;
    let acting = must(browser.acting_tab(&environment(), &origins(&app)).await);
    must(acting.goto(&form, soon()).await);
    assert_eq!(
        must(acting.page.as_the_page(&put(&app), soon()).await),
        "sent:200"
    );
    assert!(
        must(
            acting
                .page
                .as_the_page(&put_from_worker(&app), soon())
                .await
        )
        .starts_with("sent:")
    );
    assert_eq!(app.write_count("A-100", "L-101").await.unwrap(), 1);
    drop(acting);
    app.reset().await.unwrap();

    // The observer can open a tab that looks, in the same browser.
    let observer = stand.browsers.observer();
    let tab = must(
        observer
            .observing_tab(
                &clerk,
                &environment(),
                &SafeRequests::default(),
                &origins(&app),
            )
            .await,
    );
    assert_eq!(stand.supervisor.list().len(), 1);
    must(tab.goto(&form, soon()).await);
    let description = must(tab.locate(&textbox("Description"), soon()).await);
    let stored = app.stored_item("A-100", "L-101").await.unwrap().unwrap();
    assert_eq!(
        must(tab.read(&description, &ReadWhat::Value, soon()).await),
        RawValue::Text(stored.description.clone())
    );
    // A read by the page's own script still works.
    let get = format!(
        "fetch({:?}).then((r) => 'sent:' + r.status, () => 'failed')",
        app.url(ITEM)
    );
    assert_eq!(must(tab.page.as_the_page(&get, soon()).await), "sent:200");

    // A write by the page's own script, by a worker, and a socket.
    assert_eq!(
        must(tab.page.as_the_page(&put(&app), soon()).await),
        "failed"
    );
    assert_eq!(
        must(tab.page.as_the_page(&put_from_worker(&app), soon()).await),
        "failed"
    );
    let socket = "(() => { try { new WebSocket('ws://127.0.0.1:9/socket?token=abc'); return 'made'; } \
                  catch (error) { return 'refused'; } })()";
    assert_eq!(must(tab.page.as_the_page(socket, soon()).await), "refused");
    let shared = "(() => { try { new SharedWorker('/shared.js'); return 'made'; } catch (error) { return 'refused'; } })()";
    assert_eq!(must(tab.page.as_the_page(shared, soon()).await), "refused");
    let socket_in_worker = "new Promise((resolve) => { const worker = new Worker(URL.createObjectURL(new Blob([\
        \"try { new WebSocket('ws://127.0.0.1:9/worker'); postMessage('made'); } catch (error) { postMessage('refused'); }\"]))); \
        worker.onmessage = (event) => resolve(event.data); })";
    assert_eq!(
        must(tab.page.as_the_page(socket_in_worker, soon()).await),
        "refused"
    );

    // The stand says nothing was written, and what it holds is unchanged.
    assert!(app.writes().await.unwrap().is_empty());
    assert_eq!(
        app.stored_item("A-100", "L-101").await.unwrap().unwrap(),
        stored
    );

    // Each refusal is reported, without the query of its address. The
    // tab hears of a refusal as an event; a navigation it has waited out
    // came after every one of them, so all are listed by now.
    must(tab.goto(&form, soon()).await);
    let blocked = tab.blocked();
    let puts: Vec<_> = blocked.iter().filter(|one| one.method == "PUT").collect();
    assert_eq!(puts.len(), 2, "{blocked:?}");
    assert!(puts.iter().all(|one| one.address == app.url(ITEM)));
    assert!(
        blocked
            .iter()
            .any(|one| one.method == "WEBSOCKET" && one.address == "ws://127.0.0.1:9/socket"),
        "{blocked:?}"
    );
    // Taken once.
    assert!(tab.blocked().is_empty());
    drop(tab);

    // A request a person marked safe goes through, and only that one.
    let safe = SafeRequests {
        allowed: vec![SafeRequest {
            method: "PUT".to_owned(),
            address_prefix: app.url(ITEM),
        }],
    };
    let tab = must(
        observer
            .observing_tab(&clerk, &environment(), &safe, &origins(&app))
            .await,
    );
    must(tab.goto(&form, soon()).await);
    assert_eq!(
        must(tab.page.as_the_page(&put(&app), soon()).await),
        "sent:200"
    );
    assert_eq!(app.write_count("A-100", "L-101").await.unwrap(), 1);
    let other = put(&app).replace("L-101", "L-102");
    assert_eq!(must(tab.page.as_the_page(&other, soon()).await), "failed");
    assert_eq!(app.writes().await.unwrap().len(), 1);
    drop(tab);
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_observing_tab_records_its_pages_own_reads_by_structure_and_keeps_no_value() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("exchanges");
    let clerk = profile("clerk");
    let observer = stand.browsers.observer();
    let tab = must(
        observer
            .observing_tab(
                &clerk,
                &environment(),
                &SafeRequests::default(),
                &origins(&app),
            )
            .await,
    );
    let totals = Address::new(&app.url("/orders/A-100/totals")).unwrap();
    must(tab.goto(&totals, soon()).await);
    let loaded = crate::Primitive::Text(
        css("#state"),
        crate::TextMatch {
            text: "Loaded.".to_owned(),
            contains: false,
        },
    );
    must(tab.wait(&loaded, soon()).await);
    let exchanges = tab.exchanges(soon()).await;
    let fields = [
        ("lines", "integer"),
        ("order_id", "string"),
        ("total_length_in", "string"),
        ("total_quantity", "integer"),
    ]
    .into_iter()
    .map(|(name, kind)| (name.to_owned(), kind.to_owned()))
    .collect();
    assert_eq!(
        exchanges,
        [crate::PageExchange {
            method: "GET".to_owned(),
            origin: app.url("").trim_end_matches('/').to_owned(),
            path: "/api/orders/A-100/totals".to_owned(),
            query: vec!["lang".to_owned(), "view".to_owned()],
            credential_headers: vec!["cookie".to_owned(), "x-csrf-token".to_owned()],
            status: 200,
            content_type: "application/json".to_owned(),
            fields: Some(fields),
        }]
    );
    // No value of the query, the header, the cookie, or the answer.
    let whole = format!("{exchanges:?}");
    for value in [
        "summary",
        "example-token-for-tests",
        "secret-for-tests",
        "Example Customer",
    ] {
        assert!(!whole.contains(value), "{whole}");
    }
    // Taken once; the document the tab was sent to is not one.
    assert!(tab.exchanges(soon()).await.is_empty());
    assert!(app.writes().await.unwrap().is_empty());
    drop(tab);
    stand.end().await;
}

/// What the test application holds for an order, as its totals.
async fn totals_of(app: &TestApp, order: &str) -> (usize, u64) {
    let stored = app.stored_order(order).await.unwrap().unwrap();
    let quantity = stored
        .items
        .values()
        .map(|item| u64::from(item.quantity))
        .sum();
    (stored.items.len(), quantity)
}

#[tokio::test(flavor = "multi_thread")]
async fn an_observing_tab_reads_from_inside_its_page_with_get_alone_and_its_guard_still_holds() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("page-read");
    let clerk = profile("clerk");
    let observer = stand.browsers.observer();
    let tab = must(
        observer
            .observing_tab(
                &clerk,
                &environment(),
                &SafeRequests::default(),
                &origins(&app),
            )
            .await,
    );
    let totals = Address::new(&app.url("/orders/A-100/totals")).unwrap();
    must(tab.goto(&totals, soon()).await);
    // The page's own read is done, so that what the application sees next
    // is the tab's alone. The browser's own ask for an icon is not a
    // request of the page, and is left out.
    let loaded = crate::Primitive::Text(
        css("#state"),
        crate::TextMatch {
            text: "Loaded.".to_owned(),
            contains: false,
        },
    );
    must(tab.wait(&loaded, soon()).await);
    assert_eq!(tab.exchanges(soon()).await.len(), 1);
    let seen = || async {
        app.seen()
            .await
            .unwrap()
            .into_iter()
            .filter(|line| line != "GET /favicon.ico")
            .collect::<Vec<_>>()
    };
    let pattern = "/api/orders/{order_id}/totals";
    let key = |order: &str| {
        [("order_id".to_owned(), order.to_owned())]
            .into_iter()
            .collect::<BTreeMap<_, _>>()
    };

    // Two orders, by key, from the page of one: each answer is what the
    // application holds.
    for order in ["A-100", "A-101"] {
        let before = seen().await.len();
        let path = crate::PagePath::fill(pattern, &key(order)).unwrap();
        let answer = must(tab.read_from_page(&path, soon()).await);
        assert_eq!(answer.status, 200);
        assert_eq!(answer.media_type, "application/json");
        let body: serde_json::Value = serde_json::from_str(&answer.body).unwrap();
        let (lines, quantity) = totals_of(&app, order).await;
        assert_eq!(body["order_id"], order);
        assert_eq!(body["lines"], lines);
        assert_eq!(body["total_quantity"], quantity);
        // The page's cookie and its token are in no part of the answer.
        let whole = format!("{answer:?}");
        assert!(!whole.contains("secret-for-tests-cookie"), "{whole}");
        assert!(!whole.contains("example-token-for-tests"), "{whole}");
        // One request, with GET, to the path made, and nothing else.
        let now = seen().await;
        assert_eq!(
            now[before..],
            [format!("GET /api/orders/{order}/totals")],
            "{now:?}"
        );
    }

    // A value that would leave its segment is refused before any request.
    let before = seen().await;
    for value in ["A-100/../../orders", "..", "A-100?x=1", "%2e%2e", "A 100"] {
        let refused = crate::PagePath::fill(pattern, &key(value)).unwrap_err();
        assert_eq!(refused.code(), &codes::ADDRESS_VALUE_REFUSED);
    }
    assert_eq!(seen().await, before);

    // A redirect is not followed: the application's start page redirects.
    let root = crate::PagePath::fill("/", &BTreeMap::new()).unwrap();
    assert!(matches!(
        tab.read_from_page(&root, soon()).await,
        Err(BrowserError::RequestFailed(_))
    ));
    let now = seen().await;
    assert_eq!(now[before.len()..], ["GET /".to_owned()], "{now:?}");

    // The guard beneath it: the same request from the same page, with the
    // guard's rule broken by another method, is failed by the browser and
    // never reaches the application.
    let posted = tab
        .page
        .fetch(
            &crate::PageRequest {
                method: "POST".to_owned(),
                address: Address::new(&app.url("/api/orders/A-100/totals")).unwrap(),
                headers: Vec::new(),
                body: None,
            },
            soon(),
        )
        .await;
    assert!(
        matches!(posted, Err(BrowserError::RequestFailed(_))),
        "{posted:?}"
    );
    let after = seen().await;
    assert!(
        !after.iter().any(|line| line.starts_with("POST")),
        "{after:?}"
    );
    must(tab.goto(&totals, soon()).await);
    assert!(tab.blocked().iter().any(|one| one.method == "POST"));

    // A page with no origin of its own has no session to read with.
    must(
        tab.page
            .goto_text(&page_of("<p>Nothing here.</p>"), soon())
            .await,
    );
    let before = seen().await;
    let path = crate::PagePath::fill(pattern, &key("A-100")).unwrap();
    assert!(tab.read_from_page(&path, soon()).await.is_err());
    assert_eq!(seen().await, before);

    assert!(app.writes().await.unwrap().is_empty());
    drop(tab);
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_from_inside_the_page_longer_than_the_limit_is_refused_whole() {
    let app = TestApp::start().await.unwrap();
    let mut settings = test_support::settings();
    settings.max_read_chars = 16;
    let stand = Stand::new("page-read-limit", settings, &[]);
    let tab = must(
        stand
            .browsers
            .observer()
            .observing_tab(
                &profile("clerk"),
                &environment(),
                &SafeRequests::default(),
                &origins(&app),
            )
            .await,
    );
    must(
        tab.goto(&Address::new(&app.url("/orders/A-100")).unwrap(), soon())
            .await,
    );
    let path = crate::PagePath::fill(
        "/api/orders/{order_id}/totals",
        &[("order_id".to_owned(), "A-100".to_owned())]
            .into_iter()
            .collect(),
    )
    .unwrap();
    assert!(matches!(
        tab.read_from_page(&path, soon()).await,
        Err(BrowserError::TooLarge { limit: 16 })
    ));
    drop(tab);
    stand.end().await;
}
