//! The observing tab: it reads, and its page cannot write.

use std::collections::BTreeMap;

use a2m_provider::shape::{Kind, Shape};

use crate::{BodyEncoding, RawValue, ReadWhat, SafeBody, SafeRequest, SafeRequests, codes};

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
            origin: app.url(""),
            path: ITEM.to_owned(),
            body: SafeBody::Shaped {
                encoding: BodyEncoding::Json,
                shape: closed_object(&[
                    ("description", Kind::String),
                    ("quantity", Kind::Integer),
                    ("length_in", Kind::String),
                    ("version", Kind::Integer),
                ]),
            },
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
    tab.record_exchanges();
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
    let taken = tab.exchanges(soon()).await;
    assert_eq!((taken.dropped, taken.awaited), (0, 0));
    let exchanges = taken.exchanges;
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
            origin: app.url(""),
            path: "/api/orders/A-100/totals".to_owned(),
            query: vec!["lang".to_owned(), "view".to_owned()],
            query_left_out: false,
            credential_headers: vec!["cookie".to_owned(), "x-csrf-token".to_owned()],
            set_headers: vec!["accept".to_owned(), "x-csrf-token".to_owned()],
            status: 200,
            content_type: "application/json".to_owned(),
            fields: Some(fields),
            names_withheld: false,
            body_media_type: String::new(),
            body_fields: None,
            body_names_withheld: false,
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
    assert!(tab.exchanges(soon()).await.exchanges.is_empty());
    // A request with no header of the page's own names none, and a token
    // in its path or as a bare entry of its query is kept as neither.
    let plain = "fetch('/api/orders/A-100/totals?example-token-for-tests-7f3a9c2b1d').then((r) => 'sent:' + r.status, () => 'failed')";
    assert_eq!(must(tab.page.as_the_page(plain, soon()).await), "sent:200");
    let taken = tab.exchanges(soon()).await.exchanges;
    assert_eq!(taken.len(), 1, "{taken:?}");
    assert!(taken[0].set_headers.is_empty(), "{taken:?}");
    assert!(
        taken[0].query.is_empty() && taken[0].query_left_out,
        "{taken:?}"
    );
    assert!(!format!("{taken:?}").contains("example-token"), "{taken:?}");
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
    // A tab that was not asked to record keeps none.
    assert!(tab.exchanges(soon()).await.exchanges.is_empty());
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
        Err(BrowserError::Redirected)
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
    // An answer of the page's own longer than the tab reads is not kept by
    // the browser, and no names are made of it; the tab goes on.
    tab.record_exchanges();
    must(
        tab.goto(
            &Address::new(&app.url("/orders/A-100/totals")).unwrap(),
            soon(),
        )
        .await,
    );
    let loaded = crate::Primitive::Text(
        css("#state"),
        crate::TextMatch {
            text: "Loaded.".to_owned(),
            contains: false,
        },
    );
    must(tab.wait(&loaded, soon()).await);
    let taken = tab.exchanges(soon()).await.exchanges;
    assert_eq!(taken.len(), 1, "{taken:?}");
    assert_eq!(taken[0].fields, None, "{taken:?}");
    must(tab.locate(&css("#state"), soon()).await);
    drop(tab);
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_observing_tab_has_no_write_and_its_browser_fails_one_made_beneath_it() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("page-write-observed");
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
    let page = Address::new(&app.url("/orders/A-100/note")).unwrap();
    must(tab.goto(&page, soon()).await);
    // `ObserveTab` has no `write_from_page` (a doc test of `ActTab` shows
    // it does not compile). Made all the same beneath the tab, the write
    // is failed by the browser before it leaves.
    let path = crate::PagePath::fill("/api/orders/A-100/note", &BTreeMap::new()).unwrap();
    let write = crate::PageWrite::new(
        crate::WriteMethod::Put,
        path,
        crate::BodyEncoding::Json,
        [(
            "note".to_owned(),
            crate::FieldValue::Text("From a tab that may only read".to_owned()),
        )]
        .into_iter()
        .collect(),
        Some(crate::AntiForgery {
            from: crate::TokenFrom::Meta("csrf-token".to_owned()),
            to: crate::TokenTo::Header("X-CSRF-Token".to_owned()),
        }),
    )
    .unwrap();
    let failed = tab.page.write_from_page(&write, soon()).await;
    assert!(
        matches!(failed, Err(BrowserError::RequestFailed(_))),
        "{failed:?}"
    );
    assert!(
        !app.seen()
            .await
            .unwrap()
            .iter()
            .any(|line| line.starts_with("PUT")),
    );
    must(tab.goto(&page, soon()).await);
    assert!(tab.blocked().iter().any(|one| one.method == "PUT"));
    let stored = app.stored_notes().await.unwrap();
    assert_eq!(stored.notes["A-100"], "Leave at the side door.");
    assert!(stored.writes.is_empty());
    drop(tab);
    stand.end().await;
}

/// A closed object whose every field is required and of a kind.
fn closed_object(fields: &[(&str, Kind)]) -> Shape {
    Shape {
        properties: fields
            .iter()
            .map(|(name, kind)| ((*name).to_owned(), Shape::of(*kind)))
            .collect(),
        required: fields.iter().map(|(name, _)| (*name).to_owned()).collect(),
        closed: true,
        ..Shape::of(Kind::Object)
    }
}

const ROWS_PAGE: &str = "/cases/rows-by-post";
const ROWS_SEARCH: &str = "/api/cases/rows/search";

/// The search of `/cases/rows-by-post` as a person asserts it: `POST`, the
/// stand's origin, the path, and `{"status": text, "op": "search"}`.
fn rows_search(app: &TestApp) -> SafeRequest {
    let mut shape = closed_object(&[("status", Kind::String), ("op", Kind::String)]);
    if let Some(op) = shape.properties.get_mut("op") {
        op.one_of = vec!["search".to_owned()];
    }
    SafeRequest {
        method: "POST".to_owned(),
        origin: app.url(""),
        path: ROWS_SEARCH.to_owned(),
        body: SafeBody::Shaped {
            encoding: BodyEncoding::Json,
            shape,
        },
    }
}

/// The page's script sends `body` to `path` with `method`, as JSON unless
/// `kind` says otherwise; `sent:<status>` or `failed`.
fn sends(app: &TestApp, method: &str, path: &str, kind: &str, body: &str) -> String {
    format!(
        "fetch({:?}, {{method: {method:?}, headers: {{'Content-Type': {kind:?}}}, body: {body}}})\
         .then((r) => 'sent:' + r.status, () => 'failed')",
        app.url(path)
    )
}

fn state_is(text: &str) -> crate::Primitive {
    crate::Primitive::Text(
        css("#state"),
        crate::TextMatch {
            text: text.to_owned(),
            contains: false,
        },
    )
}

/// Where a read that carries a method override is sent, one path for each
/// method: the stand would hear it, had it left.
const OVERRIDDEN: &str = "/api/cases/rows/overridden";

/// A page's own read of `method`, carrying header `name` that says it is
/// `says`.
fn overridden(app: &TestApp, method: &str, name: &str, says: &str) -> String {
    format!(
        "fetch({:?}, {{method: {method:?}, headers: {{{name:?}: {says:?}}}}})\
         .then((r) => 'sent:' + r.status, () => 'failed')",
        app.url(&format!("{OVERRIDDEN}/{method}"))
    )
}

/// The `POST` lines the stand heard, in order.
async fn posts_heard(app: &TestApp) -> Vec<String> {
    let mut heard = app.seen().await.unwrap();
    heard.retain(|line| line.starts_with("POST "));
    heard
}

#[tokio::test(flavor = "multi_thread")]
async fn an_observing_tab_lets_through_a_safe_request_only_on_method_origin_template_and_body() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("safe-requests");
    let clerk = profile("clerk");
    let observer = stand.browsers.observer();
    let page = Address::new(&app.url(ROWS_PAGE)).unwrap();
    let seeded = app.case_rows().await.unwrap();
    assert_eq!(
        seeded
            .rows
            .iter()
            .filter(|row| row.status == "open")
            .count(),
        2
    );

    // With no safe request the page's own search is failed before it leaves
    // and reported, and the stand hears nothing of it.
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
    must(tab.goto(&page, soon()).await);
    must(tab.wait(&state_is("Failed."), soon()).await);
    let blocked = tab.blocked();
    assert!(
        blocked
            .iter()
            .any(|one| one.method == "POST" && one.address == app.url(ROWS_SEARCH)),
        "{blocked:?}"
    );
    assert_eq!(app.case_rows().await.unwrap(), seeded);
    assert!(posts_heard(&app).await.is_empty());
    drop(tab);

    // With the search asserted, the page's rows arrive.
    let safe = SafeRequests {
        allowed: vec![rows_search(&app)],
    };
    let tab = must(
        observer
            .observing_tab(&clerk, &environment(), &safe, &origins(&app))
            .await,
    );
    must(tab.goto(&page, soon()).await);
    must(tab.wait(&state_is("Loaded."), soon()).await);
    let shown = must(
        tab.page
            .as_the_page("document.getElementById('rows').textContent", soon())
            .await,
    );
    assert_eq!(shown, "C-1Leaking valveopenC-2Late deliveryopen");
    let searched = app.case_rows().await.unwrap();
    assert_eq!(searched.received, ["search"]);
    assert_eq!(searched.rows, seeded.rows);
    assert!(tab.blocked().is_empty());

    // The same search, as the browser hands over a body of other forms: a
    // `Blob`, an `ArrayBuffer`, and an `XMLHttpRequest`'s text, each whole.
    let good = "JSON.stringify({status: 'closed', op: 'search'})";
    for body in [
        format!("new Blob([{good}])"),
        format!("new TextEncoder().encode({good}).buffer"),
    ] {
        assert_eq!(
            must(
                tab.page
                    .as_the_page(
                        &sends(&app, "POST", ROWS_SEARCH, "application/json", &body),
                        soon()
                    )
                    .await
            ),
            "sent:200",
            "{body}"
        );
    }
    let xhr = format!(
        "new Promise((done) => {{ const x = new XMLHttpRequest(); x.open('POST', {:?}); \
         x.setRequestHeader('Content-Type', 'application/json'); \
         x.onloadend = () => done('sent:' + x.status); x.send({good}); }})",
        app.url(ROWS_SEARCH)
    );
    assert_eq!(must(tab.page.as_the_page(&xhr, soon()).await), "sent:200");
    assert_eq!(app.case_rows().await.unwrap().received.len(), 4);
    let before = posts_heard(&app).await;

    // Each of these is failed before it leaves the browser.
    let archive = "JSON.stringify({status: 'open', op: 'archive'})";
    let refused = [
        (
            "op: archive against enum: [search]",
            sends(&app, "POST", ROWS_SEARCH, "application/json", archive),
        ),
        (
            "a field the shape does not name",
            sends(
                &app,
                "POST",
                ROWS_SEARCH,
                "application/json",
                "JSON.stringify({status: 'open', op: 'search', all: true})",
            ),
        ),
        (
            "a field of another kind",
            sends(
                &app,
                "POST",
                ROWS_SEARCH,
                "application/json",
                "JSON.stringify({status: 7, op: 'search'})",
            ),
        ),
        (
            "a name given twice",
            sends(
                &app,
                "POST",
                ROWS_SEARCH,
                "application/json",
                "'{\"status\":\"open\",\"op\":\"search\",\"op\":\"archive\"}'",
            ),
        ),
        (
            "a body that is not JSON",
            sends(
                &app,
                "POST",
                ROWS_SEARCH,
                "application/json",
                "'status=open&op=search'",
            ),
        ),
        (
            "a body of another media type",
            sends(
                &app,
                "POST",
                ROWS_SEARCH,
                "text/plain",
                "JSON.stringify({status: 'open', op: 'search'})",
            ),
        ),
        (
            "a body over the bound",
            sends(
                &app,
                "POST",
                ROWS_SEARCH,
                "application/json",
                &format!("' '.repeat(1048576) + {good}"),
            ),
        ),
        (
            "a body the browser does not give whole",
            format!(
                "fetch({:?}, {{method: 'POST', duplex: 'half', headers: {{'Content-Type': 'application/json'}}, \
                 body: new ReadableStream({{start(c) {{ c.enqueue(new TextEncoder().encode({good})); c.close(); }}}})}})\
                 .then((r) => 'sent:' + r.status, () => 'failed')",
                app.url(ROWS_SEARCH)
            ),
        ),
        (
            "a longer path",
            sends(
                &app,
                "POST",
                "/api/cases/rows/search/all",
                "application/json",
                good,
            ),
        ),
        (
            "a query",
            sends(
                &app,
                "POST",
                "/api/cases/rows/search?op=archive",
                "application/json",
                good,
            ),
        ),
        (
            "another method",
            sends(&app, "PUT", ROWS_SEARCH, "application/json", good),
        ),
        (
            "a method override",
            format!(
                "fetch({:?}, {{method: 'POST', headers: {{'Content-Type': 'application/json', 'X-HTTP-Method-Override': 'DELETE'}}, \
                 body: {good}}}).then((r) => 'sent:' + r.status, () => 'failed')",
                app.url(ROWS_SEARCH)
            ),
        ),
        (
            "no listed match",
            sends(&app, "POST", "/cases/events/rows", "application/json", good),
        ),
        // A read that says it is another method is judged by what it says
        // (decision 0270): a system that honours the header would write.
        (
            "a GET that says DELETE",
            overridden(&app, "GET", "X-HTTP-Method-Override", "DELETE"),
        ),
        (
            "a HEAD that says PUT",
            overridden(&app, "HEAD", "x-http-method", "PUT"),
        ),
        (
            "an OPTIONS that says POST",
            overridden(&app, "OPTIONS", "X-Method-Override", "POST"),
        ),
    ];
    for (why, script) in &refused {
        assert_eq!(
            must(tab.page.as_the_page(script, soon()).await),
            "failed",
            "{why}"
        );
    }
    // A navigation the tab has waited out came after every refusal.
    must(tab.goto(&page, soon()).await);
    must(tab.wait(&state_is("Loaded."), soon()).await);
    let blocked = tab.blocked();
    assert_eq!(blocked.len(), refused.len(), "{blocked:?}");
    for method in ["GET", "HEAD", "OPTIONS"] {
        assert!(
            blocked.iter().any(|one| one.method == method
                && one.address == app.url(&format!("{OVERRIDDEN}/{method}"))),
            "{method}: {blocked:?}"
        );
    }
    let seen = app.seen().await.unwrap();
    assert!(
        !seen.iter().any(|line| line.contains(OVERRIDDEN)),
        "{seen:?}"
    );

    // The stand stored nothing and heard nothing of them: the only body it
    // was sent since is the page's own search, loaded again.
    let after = app.case_rows().await.unwrap();
    assert_eq!(
        after.received,
        ["search", "search", "search", "search", "search"]
    );
    assert_eq!(after.rows, seeded.rows);
    let mut heard = posts_heard(&app).await;
    assert_eq!(
        heard.split_off(before.len()),
        [format!("POST {ROWS_SEARCH}")]
    );
    assert!(app.case_events().await.unwrap().is_empty());
    drop(tab);
    stand.end().await;

    // And the stand does archive by the same route, so the refusal above
    // meant something.
    let archived = app
        .http()
        .post(app.url(ROWS_SEARCH))
        .json(&serde_json::json!({"status": "open", "op": "archive"}))
        .send()
        .await
        .unwrap();
    assert!(archived.status().is_success());
    let changed = app.case_rows().await.unwrap();
    assert_eq!(changed.rows[0].status, "archived");
}
