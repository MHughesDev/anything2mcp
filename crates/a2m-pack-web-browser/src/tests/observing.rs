//! The observing tab: it reads, and its page cannot write.

use crate::{RawValue, ReadWhat, SafeRequest, SafeRequests};

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
    let acting = must(browser.acting_tab(&environment()).await);
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
            .observing_tab(&clerk, &environment(), &SafeRequests::default())
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

    // Each refusal is reported, without the query of its address.
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
    let tab = must(observer.observing_tab(&clerk, &environment(), &safe).await);
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
