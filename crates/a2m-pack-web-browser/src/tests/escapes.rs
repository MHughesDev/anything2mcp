//! BEFORE-FIX DEMONSTRATION (temporary): each test shows a finding against the stand.

use crate::{Input, Primitive, SafeRequests, SnapshotLimits, TextMatch};

use super::*;

fn tried() -> Primitive {
    Primitive::Visible(css("#tried"))
}

async fn escapes(app: &TestApp) -> Vec<String> {
    app.trap_writes()
        .await
        .unwrap()
        .into_keys()
        .filter(|name| name.starts_with("escapes/"))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn before_b1_a_page_moves_an_observing_tab() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("before-b1");
    let observer = stand.browsers.observer();
    let tab = must(
        observer
            .observing_tab(&profile("scout"), &environment(), &SafeRequests::default())
            .await,
    );
    let quiet = Address::new(&app.url("/escapes/quiet")).unwrap();
    let mut seen = Vec::new();
    for page in ["script", "form", "elsewhere", "meta"] {
        must(tab.goto(&Address::new(&app.url(&format!("/escapes/{page}"))).unwrap(), soon()).await);
        // The page tries after it has loaded; a wait that sees it gone, or sees it tried, ends.
        let _ = tab
            .wait(
                &Primitive::Any(vec![
                    tried(),
                    Primitive::Url(TextMatch { text: "/traps/write/".to_owned(), contains: true }),
                ]),
                Deadline::after(Duration::from_secs(5)),
            )
            .await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        must(tab.goto(&quiet, soon()).await);
        seen.push((page, escapes(&app).await));
    }
    let redirected = tab.goto(&Address::new(&app.url("/escapes/redirect")).unwrap(), soon()).await;
    drop(tab);
    stand.end().await;
    assert!(escapes(&app).await.is_empty(), "{seen:?} redirect: {redirected:?} {:?}", escapes(&app).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn before_b2_a_download_writes_a_file() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("before-b2");
    let name = format!("a2m-escape-{}.csv", std::process::id());
    let tab = must(
        stand
            .browsers
            .observer()
            .observing_tab(&profile("scout"), &environment(), &SafeRequests::default())
            .await,
    );
    let _ = tab
        .goto(&Address::new(&app.url(&format!("/escapes/download/{name}"))).unwrap(), soon())
        .await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let places = [std::env::temp_dir().join("Downloads").join(&name)];
    let written: Vec<_> = places.iter().filter(|place| place.exists()).collect();
    for place in &written {
        let _ = std::fs::remove_file(place);
    }
    drop(tab);
    stand.end().await;
    assert!(written.is_empty(), "{written:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn before_b3_a_frame_from_another_site_opens_a_socket() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("before-b3");
    let tab = must(
        stand
            .browsers
            .observer()
            .observing_tab(&profile("scout"), &environment(), &SafeRequests::default())
            .await,
    );
    must(tab.goto(&Address::new(&app.url("/escapes/frame")).unwrap(), soon()).await);
    let waited = tab.wait(&tried(), Deadline::after(Duration::from_secs(10))).await;
    let blocked = tab.blocked();
    drop(tab);
    stand.end().await;
    panic!("{:?} {waited:?} {blocked:?}", app.trap_writes().await.unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn before_b4_a_service_worker_writes_for_an_observed_page() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("before-b4");
    let clerk = profile("clerk");
    let browser = stand.open(&clerk).await;
    let acting = must(browser.acting_tab(&environment()).await);
    let quiet = Address::new(&app.url("/escapes/quiet")).unwrap();
    must(acting.goto(&quiet, soon()).await);
    let register = "navigator.serviceWorker.register('/escapes/service-worker.js', {scope: '/escapes/'})\
        .then(() => navigator.serviceWorker.ready).then(() => 'ready', (e) => 'failed ' + e)";
    assert_eq!(must(acting.page.as_the_page(register, soon()).await), "ready");
    let tab = must(
        stand
            .browsers
            .observer()
            .observing_tab(&clerk, &environment(), &SafeRequests::default())
            .await,
    );
    must(tab.goto(&quiet, soon()).await);
    let poke = "navigator.serviceWorker.getRegistration('/escapes/').then((r) => new Promise((resolve) => {\
        navigator.serviceWorker.onmessage = (e) => resolve(e.data); r.active.postMessage('go'); \
        setTimeout(() => resolve('no answer'), 5000); }))";
    let answer = tab.page.as_the_page(poke, soon()).await;
    drop(tab);
    drop(acting);
    stand.end().await;
    panic!("{:?} {answer:?}", app.trap_writes().await.unwrap());
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn before_b5_the_profile_folder_is_readable_by_others() {
    use std::os::unix::fs::PermissionsExt;
    let stand = Stand::with_browser("before-b5");
    let clerk = profile("clerk");
    let _browser = stand.open(&clerk).await;
    let folder = stand.folder.0.join("profiles").join(clerk.place());
    let mode = std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777;
    stand.end().await;
    assert_eq!(mode, 0o700, "{mode:o}");
}

#[tokio::test(flavor = "multi_thread")]
async fn before_b6_b7_b8_a_password_leaves() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("before-b6");
    let browser = stand.open(&profile("clerk")).await;
    let tab = must(browser.acting_tab(&environment()).await);
    let page = page_of(
        "<!doctype html><title>Sign in</title>\
         <input type=password aria-label=Password value=hunter2-password>\
         <div id=host></div>\
         <script>document.getElementById('host').attachShadow({mode: 'closed'}).innerHTML = \
         '<input aria-label=Shown autocomplete=current-password value=closed-root-password>';</script>",
    );
    must(tab.page.goto_text(&page, soon()).await);
    let snapshot = must(tab.snapshot(&SnapshotLimits::default(), soon()).await).to_text();
    let guess = tab.locate(&css("input[type=password][value^='h']"), soon()).await;
    let wrong = tab.locate(&css("input[type=password][value^='x']"), soon()).await;
    must(tab.goto(&Address::new(&format!("{}?reset=example-token-for-reset#part", app.url(FORM))).unwrap(), soon()).await);
    let with_query = must(tab.snapshot(&SnapshotLimits::default(), soon()).await).to_text();
    let typed = format!("{:?}", Input::Fill("secret-for-tests-typed".to_owned()));
    drop(tab);
    stand.end().await;
    assert!(!snapshot.contains("closed-root-password"), "B6: {snapshot}");
    assert!(guess.is_err() == wrong.is_err(), "B7: {guess:?} {wrong:?}");
    assert!(!with_query.contains("example-token-for-reset"), "B8: {with_query}");
    assert!(!typed.contains("secret-for-tests-typed"), "B9: {typed}");
}
