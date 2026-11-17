//! The ways out of a tab's lock, each tried by a page of the test
//! application: a page that moves the tab by itself, a download, a frame
//! from another site, a window the page opens, a service worker, and a
//! page on an origin the tab was not opened for. For each, the test
//! application's control interface says that nothing arrived.

use crate::{
    Input, ObserveTab, PageRequest, Primitive, RequestShape, SafeRequests, SnapshotLimits,
    TextMatch,
};

use super::*;

/// Where every attempt of an escaping page lands, and is counted.
const WRITE: &str = "/traps/write/escapes";

/// What arrived at the test application from an escaping page, by name.
async fn arrived(app: &TestApp) -> Vec<String> {
    app.trap_writes()
        .await
        .unwrap()
        .into_keys()
        .filter(|name| name.starts_with("escapes/"))
        .collect()
}

/// The test application under its other loopback name: to a browser,
/// another origin and another site.
fn other(app: &TestApp, path: &str) -> String {
    app.url(path).replacen("127.0.0.1", "localhost", 1)
}

fn at(address: &str) -> Address {
    Address::new(address).unwrap()
}

fn only(app: &TestApp) -> Origins {
    Origins::of([&at(&app.url("/"))])
}

/// What a tab refused, collected until one refusal carries `word`. A tab
/// hears of a refusal as an event, a moment after the page was refused.
async fn refused(blocked: impl Fn() -> Vec<RequestShape>, word: &str) -> Vec<RequestShape> {
    let started = std::time::Instant::now();
    let mut seen = Vec::new();
    loop {
        seen.extend(blocked());
        if seen.iter().any(|one| one.method == word) {
            return seen;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "nothing was refused as {word}: {seen:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The same, until one refusal carries `word` for exactly `address`.
async fn refused_at(
    blocked: impl Fn() -> Vec<RequestShape>,
    word: &str,
    address: &str,
) -> Vec<RequestShape> {
    let mut seen = Vec::new();
    while !seen
        .iter()
        .any(|one: &RequestShape| one.method == word && one.address == address)
    {
        seen.extend(refused(&blocked, word).await);
    }
    seen
}

async fn observing(stand: &Stand, app: &TestApp, identity: &str) -> ObserveTab {
    must(
        stand
            .browsers
            .observer()
            .observing_tab(
                &profile(identity),
                &environment(),
                &SafeRequests::default(),
                &only(app),
            )
            .await,
    )
}

fn outside(error: &BrowserError, origin: &str) {
    assert_eq!(error, &BrowserError::ForeignOrigin(origin.to_owned()));
    assert_eq!(error.dispatch(), Dispatch::NotSent);
    let failure = error.failure().unwrap();
    assert_eq!(
        (failure.class(), failure.code()),
        (FailureClass::Policy, &codes::SURFACE_NOT_DECLARED)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_cannot_move_a_tab_that_only_observes_and_goto_leaves_no_origin_it_was_given() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("moves");
    let tab = observing(&stand, &app, "scout").await;
    let elsewhere = other(&app, &format!("{WRITE}/elsewhere"));

    // Each page tries once it has loaded: by its script, by a refresh tag,
    // by submitting a form with GET and with POST, to another origin, and
    // with a beacon. The tab says what it refused, and the page the caller
    // sent it to is still the page it shows.
    for (page, word, aimed, title) in [
        (
            "script",
            "NAVIGATION",
            app.url(&format!("{WRITE}/script")),
            "Order archive",
        ),
        (
            "meta",
            "NAVIGATION",
            app.url(&format!("{WRITE}/meta")),
            "Order summary",
        ),
        (
            "form",
            "NAVIGATION",
            app.url(&format!("{WRITE}/form")),
            "Order search",
        ),
        (
            "elsewhere",
            "NAVIGATION",
            elsewhere.clone(),
            "Order partners",
        ),
        (
            "form-post",
            "POST",
            app.url(&format!("{WRITE}/form-post")),
            "Order renewal",
        ),
        (
            "beacon",
            "POST",
            app.url(&format!("{WRITE}/beacon")),
            "Order statistics",
        ),
    ] {
        let sent_to = app.url(&format!("/escapes/{page}"));
        must(tab.goto(&at(&sent_to), soon()).await);
        let seen = refused(|| tab.blocked(), word).await;
        assert!(
            seen.iter()
                .any(|one| one.method == word && one.address == aimed),
            "{page}: {seen:?}"
        );
        must(
            tab.wait(
                &Primitive::Url(TextMatch {
                    text: sent_to,
                    contains: false,
                }),
                soon(),
            )
            .await,
        );
        must(tab.locate(&Query::role("heading", title), soon()).await);
    }

    // A `goto` is held to the tab's origins too: before it starts, and at
    // every redirect on its way.
    let origin = other(&app, "");
    outside(
        &tab.goto(&at(&elsewhere), soon()).await.unwrap_err(),
        &origin,
    );
    let redirected = tab.goto(&at(&app.url("/escapes/redirect")), soon()).await;
    outside(&redirected.unwrap_err(), &origin);
    // The tab is where it was, and still reads.
    must(
        tab.locate(&Query::role("heading", "Order statistics"), soon())
            .await,
    );

    drop(tab);
    stand.end().await;
    assert_eq!(arrived(&app).await, Vec::<String>::new());
}

/// Every place a browser might put a file it saved under this name.
fn saved(stand: &Stand, name: &str) -> Vec<PathBuf> {
    let mut places = vec![std::env::temp_dir().join("Downloads").join(name)];
    for home in ["HOME", "USERPROFILE"] {
        if let Some(home) = std::env::var_os(home) {
            places.push(PathBuf::from(home).join("Downloads").join(name));
        }
    }
    if let Ok(here) = std::env::current_dir() {
        places.push(here.join(name));
    }
    let mut folders = vec![stand.folder.0.clone()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if entry.file_name().to_string_lossy().contains(name) {
                places.push(path);
            }
        }
    }
    places.into_iter().filter(|place| place.exists()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_download_writes_no_file_and_a_window_a_page_opens_is_closed_before_it_fetches() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("windows");
    let export = format!("example-orders-{}.csv", std::process::id());
    let link_export = "example-orders.csv";
    for name in [export.as_str(), link_export] {
        for stale in saved(&stand, name) {
            let _ = std::fs::remove_file(stale);
        }
    }

    // A tab that may act: a person's click on a link to a download, and on
    // a link that opens a new window.
    let browser = stand.open(&profile("clerk")).await;
    let acting = must(browser.acting_tab(&environment(), &only(&app)).await);
    let links = at(&app.url("/escapes/links"));
    must(acting.goto(&links, soon()).await);
    let download = must(
        acting
            .locate(&Query::role("link", "Export the orders"), soon())
            .await,
    );
    must(acting.input(&download, &Input::Click, soon()).await);
    let seen = refused(|| acting.blocked(), "DOWNLOAD").await;
    assert_eq!(
        seen,
        [RequestShape {
            method: "DOWNLOAD".to_owned(),
            address: app.url(&format!("/escapes/download/{link_export}")),
        }]
    );
    let window = must(
        acting
            .locate(&Query::role("link", "Open the packing list"), soon())
            .await,
    );
    must(acting.input(&window, &Input::Click, soon()).await);
    // Reported when it opens, and again with the page it asked for.
    let asked = app.url(&format!("{WRITE}/window"));
    let seen = refused_at(|| acting.blocked(), "POPUP", &asked).await;
    assert!(seen.iter().all(|one| one.method == "POPUP"), "{seen:?}");
    // The tab is where it was, and no other page is left open.
    must(
        acting
            .locate(&Query::role("heading", "Order documents"), soon())
            .await,
    );
    assert_eq!(stand.browsers.list()[0].tabs, 1);
    drop(acting);

    // A tab that only observes: a `goto` whose answer is a file, and a
    // page that has a person's gesture to open a window with.
    let tab = observing(&stand, &app, "clerk").await;
    let file = tab
        .goto(
            &at(&app.url(&format!("/escapes/download/{export}"))),
            soon(),
        )
        .await;
    assert!(matches!(file, Err(BrowserError::NotLoaded(_))), "{file:?}");
    let seen = refused(|| tab.blocked(), "DOWNLOAD").await;
    assert_eq!(seen.len(), 1, "{seen:?}");
    // A page that has a person's gesture to open a window with, which a
    // tab that only observes never gives. The window is closed while the
    // browser holds it: it never asks for its page, and the script that
    // opened it never goes on, so each window is opened from a timer. The
    // engine's own reads of the page still answer.
    must(tab.goto(&at(&app.url("/escapes/quiet")), soon()).await);
    let open = format!("setTimeout(() => window.open('{WRITE}/opened'), 0); 'asked'");
    must(tab.page.as_the_page_with_a_gesture(&open, soon()).await);
    let seen = refused(|| tab.blocked(), "POPUP").await;
    assert!(
        seen.iter()
            .all(|one| one.method == "POPUP" && one.address.is_empty()),
        "{seen:?}"
    );
    must(
        tab.locate(&Query::role("heading", "Order notes"), soon())
            .await,
    );
    assert_eq!(stand.browsers.list()[0].tabs, 1);
    drop(tab);
    // A window that asks for no page gives its opener nothing to use: no
    // socket through it, and no page it is sent to afterwards.
    let tab = observing(&stand, &app, "clerk").await;
    must(tab.goto(&at(&app.url("/escapes/quiet")), soon()).await);
    let through_a_window = format!(
        "setTimeout(() => {{ const opened = window.open(); if (!opened) return; \
         try {{ new opened.WebSocket('ws://' + location.host + '{WRITE}/window-socket'); }} \
         catch (error) {{}} \
         opened.location.href = '{WRITE}/sent-later'; }}, 0); 'asked'"
    );
    must(
        tab.page
            .as_the_page_with_a_gesture(&through_a_window, soon())
            .await,
    );
    refused(|| tab.blocked(), "POPUP").await;
    must(
        tab.locate(&Query::role("heading", "Order notes"), soon())
            .await,
    );
    assert_eq!(stand.browsers.list()[0].tabs, 1);
    drop(tab);

    // Closing the browser lets it finish whatever it had begun to write.
    let written: Vec<PathBuf> = [export.as_str(), link_export]
        .iter()
        .flat_map(|name| saved(&stand, name))
        .collect();
    stand.end().await;
    assert_eq!(written, Vec::<PathBuf>::new());
    assert_eq!(arrived(&app).await, Vec::<String>::new());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_frame_from_another_site_runs_locked_in_every_document_it_loads() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("frames");
    let tab = observing(&stand, &app, "scout").await;
    // The page embeds a frame from another site, which the browser runs in
    // a process of its own. The frame opens a socket and sends a POST, in
    // its first document and in a second one it moves itself to, and the
    // page says when both have tried.
    must(tab.goto(&at(&app.url("/escapes/frame")), soon()).await);
    must(tab.wait(&Primitive::Visible(css("#tried")), soon()).await);
    let mut seen = tab.blocked();
    seen.sort_by(|a, b| (&a.method, &a.address).cmp(&(&b.method, &b.address)));
    let shape = |method: &str, address: String| RequestShape {
        method: method.to_owned(),
        address,
    };
    let socket = |name: &str| other(&app, &format!("{WRITE}/{name}")).replacen("http", "ws", 1);
    assert_eq!(
        seen,
        [
            shape("POST", other(&app, &format!("{WRITE}/frame-post-first"))),
            shape("POST", other(&app, &format!("{WRITE}/frame-post-second"))),
            shape("WEBSOCKET", socket("frame-socket-first")),
            shape("WEBSOCKET", socket("frame-socket-second")),
        ]
    );
    drop(tab);
    stand.end().await;
    assert_eq!(arrived(&app).await, Vec::<String>::new());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_service_worker_is_locked_when_an_observed_page_wakes_it() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("workers");
    let clerk = profile("clerk");
    let quiet = at(&app.url("/escapes/quiet"));
    let poke = "navigator.serviceWorker.getRegistration('/escapes/').then((found) => \
        new Promise((resolve) => { navigator.serviceWorker.onmessage = (event) => resolve(event.data); \
        found.active.postMessage('go'); }))";

    // First, that the test means something: a page of a tab that may act
    // installs the worker, and the worker writes when it is told to.
    let browser = stand.open(&clerk).await;
    let acting = must(browser.acting_tab(&environment(), &only(&app)).await);
    must(acting.goto(&quiet, soon()).await);
    let register = "navigator.serviceWorker.register('/escapes/service-worker.js', {scope: '/escapes/'})\
        .then(() => navigator.serviceWorker.ready).then(() => 'ready')";
    assert_eq!(
        must(acting.page.as_the_page(register, soon()).await),
        "ready"
    );
    assert_eq!(
        must(acting.page.as_the_page(poke, soon()).await),
        "post sent, socket made"
    );
    assert_eq!(
        arrived(&app).await,
        ["escapes/service-post", "escapes/service-socket"]
    );
    drop(acting);
    app.reset().await.unwrap();

    // The same worker, installed and running, woken by a page that is only
    // observed: its write is failed and its socket refused.
    let tab = observing(&stand, &app, "clerk").await;
    must(tab.goto(&quiet, soon()).await);
    assert_eq!(
        must(tab.page.as_the_page(poke, soon()).await),
        "post failed, socket refused"
    );
    // A page cannot get an unlocked socket, or register a worker, through
    // a frame it makes for the purpose.
    let through_a_frame = "(() => { const frame = document.createElement('iframe'); \
        document.body.append(frame); const inner = frame.contentWindow; \
        let socket = 'made'; \
        try { new inner.WebSocket('ws://' + location.host + '/traps/write/escapes/inner-socket'); } \
        catch (error) { socket = 'refused'; } \
        return inner.navigator.serviceWorker.register('/escapes/service-worker.js') \
        .then(() => socket + ', registered', () => socket + ', not registered'); })()";
    assert_eq!(
        must(tab.page.as_the_page(through_a_frame, soon()).await),
        "refused, not registered"
    );
    let seen = refused(|| tab.blocked(), "POST").await;
    assert!(
        seen.iter()
            .any(|one| one.method == "POST"
                && one.address == app.url(&format!("{WRITE}/service-post"))),
        "{seen:?}"
    );
    drop(tab);
    stand.end().await;
    assert_eq!(arrived(&app).await, Vec::<String>::new());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tab_goes_to_looks_at_and_acts_on_no_origin_it_was_not_opened_for() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("origins");
    let browser = stand.open(&profile("clerk")).await;
    // Opened for the test application alone.
    let tab = must(browser.acting_tab(&environment(), &only(&app)).await);
    let origin = other(&app, "");
    let moved = other(&app, &format!("{WRITE}/moved"));

    // It is not sent there, by an address or by a redirect.
    outside(&tab.goto(&at(&moved), soon()).await.unwrap_err(), &origin);
    let redirected = tab.goto(&at(&app.url("/escapes/redirect")), soon()).await;
    outside(&redirected.unwrap_err(), &origin);

    // Its page does not take it there: after the `goto` has returned, the
    // page's own script moves on, as a page does that hands over to
    // another system. The tab stays, says so, and still acts on the form.
    must(tab.goto(&at(&app.url(FORM)), soon()).await);
    let description = must(tab.locate(&textbox("Description"), soon()).await);
    let leave = format!("location.href = '{moved}'; 'leaving'");
    must(tab.page.as_the_page(&leave, soon()).await);
    let seen = refused(|| tab.blocked(), "NAVIGATION").await;
    assert_eq!(
        seen,
        [RequestShape {
            method: "NAVIGATION".to_owned(),
            address: moved.clone(),
        }]
    );
    must(
        tab.input(
            &description,
            &Input::Fill("Kept on its origin".to_owned()),
            soon(),
        )
        .await,
    );
    assert_eq!(
        must(
            tab.read(&description, &crate::ReadWhat::Value, soon())
                .await
        ),
        crate::RawValue::Text("Kept on its origin".to_owned())
    );

    // A request from inside the page carries the page's session: it goes
    // to no other origin, and follows no redirect to one.
    let request = |address: String| PageRequest {
        method: "GET".to_owned(),
        address: at(&address),
        headers: Vec::new(),
        body: None,
    };
    outside(
        &tab.fetch(&request(moved), soon()).await.unwrap_err(),
        &origin,
    );
    let followed = tab
        .fetch(&request(app.url("/escapes/redirect")), soon())
        .await;
    assert!(
        matches!(followed, Err(BrowserError::RequestFailed(_))),
        "{followed:?}"
    );
    let inside = must(
        tab.fetch(&request(app.url("/api/orders?status=draft")), soon())
            .await,
    );
    assert_eq!(
        (inside.status, inside.address),
        (200, app.url("/api/orders"))
    );

    // And if a page of another origin is in the tab all the same, nothing
    // is located, waited for, or shown there. A test puts one there the
    // one way it can: a page with no origin, past the check of `goto`.
    let nowhere = "about:";
    let foreign = page_of("<!doctype html><title>Elsewhere</title><input aria-label=Password>");
    let landed = tab.page.goto_text(&foreign, soon()).await;
    assert!(landed.is_err(), "{landed:?}");
    if must(tab.page.as_the_page("location.protocol", soon()).await) != "data:" {
        // The browser did not go there at all. An empty page stands in.
        must(
            tab.page
                .as_the_page("location.href = 'about:blank'; 'left'", soon())
                .await,
        );
    }
    let here = must(tab.page.as_the_page("location.protocol", soon()).await);
    assert!(here == "data:" || here == nowhere, "{here}");
    let password = textbox("Password");
    outside(&tab.locate(&password, soon()).await.unwrap_err(), &here);
    let shown = Primitive::Visible(password);
    outside(&tab.wait(&shown, soon()).await.unwrap_err(), &here);
    let snapshot = tab.snapshot(&SnapshotLimits::default(), soon()).await;
    outside(&snapshot.unwrap_err(), &here);
    // The element found on the form is of a page that is gone.
    assert_eq!(
        tab.input(
            &description,
            &Input::Fill("Typed elsewhere".to_owned()),
            soon()
        )
        .await
        .unwrap_err(),
        BrowserError::NotFound
    );

    drop(tab);
    stand.end().await;
    assert_eq!(arrived(&app).await, Vec::<String>::new());
    assert!(app.writes().await.unwrap().is_empty());
}

#[test]
fn a_request_marked_safe_is_matched_by_origin_and_path_not_by_how_its_text_begins() {
    let safe = |prefix: &str| SafeRequests {
        allowed: vec![crate::SafeRequest {
            method: "POST".to_owned(),
            address_prefix: prefix.to_owned(),
        }],
    };
    let search = safe("https://app.example.com/api/search");
    for allowed in [
        "https://app.example.com/api/search",
        "https://app.example.com/api/search?q=bolts",
        "https://app.example.com/api/search/orders",
        "https://APP.example.com:443/api/search",
    ] {
        assert!(search.allows("POST", allowed), "{allowed}");
    }
    for refused in [
        "https://app.example.com/api/search-and-delete",
        "https://app.example.com/api/searches",
        "https://app.example.com.other.example/api/search",
        "http://app.example.com/api/search",
        "https://app.example.com:8443/api/search",
        "https://app.example.com/api",
        "https://other.example/?https://app.example.com/api/search",
    ] {
        assert!(!search.allows("POST", refused), "{refused}");
    }
    assert!(!search.allows("PUT", "https://app.example.com/api/search"));
    // A whole origin is marked by its address alone, and a longer host is
    // not under it.
    let whole = safe("https://app.example.com");
    assert!(whole.allows("POST", "https://app.example.com/anything"));
    assert!(!whole.allows("POST", "https://app.example.com.other.example/anything"));
    // What is no address, or carries a query, marks nothing.
    for nothing in [
        "",
        "/api/search",
        "app.example.com",
        "https://app.example.com/api?x=1",
    ] {
        assert!(
            !safe(nothing).allows("POST", "https://app.example.com/api?x=1"),
            "{nothing}"
        );
    }
    // A read needs no mark.
    assert!(SafeRequests::default().allows("GET", "https://app.example.com/"));
}

#[test]
fn what_is_typed_and_what_a_request_carries_is_never_printed() {
    const TYPED: &str = "secret-for-tests-typed";
    for input in [
        Input::Fill(TYPED.to_owned()),
        Input::Type(TYPED.to_owned()),
        Input::NativeSet(TYPED.to_owned()),
    ] {
        let printed = format!("{input:?}");
        assert!(!printed.contains(TYPED), "{printed}");
        assert!(printed.contains("22 characters"), "{printed}");
    }
    let request = PageRequest {
        method: "POST".to_owned(),
        address: at("https://app.example.com/api/session?ticket=example-token-in-query"),
        headers: vec![(
            "x-example-key".to_owned(),
            "example-key-in-header".to_owned(),
        )],
        body: Some("password=secret-for-tests-body".to_owned()),
    };
    let printed = format!("{request:?}");
    for hidden in [
        "example-token-in-query",
        "example-key-in-header",
        "secret-for-tests-body",
    ] {
        assert!(!printed.contains(hidden), "{printed}");
    }
    assert!(
        printed.contains("x-example-key") && printed.contains("/api/session"),
        "{printed}"
    );
}

#[test]
fn the_browser_s_reviewed_digests_are_read_from_a_pack_s_settings_or_refused() {
    use a2m_core::{Value, Values};
    let one = Digest::of(b"a browser program");
    let two = Digest::of(b"another");
    let list = |texts: &[String]| {
        Value::List(texts.iter().map(|text| Value::Text(text.clone())).collect())
    };
    let read = |settings: &Values| BrowserSettings::default().with_pack_settings(settings);
    // No key: nothing is pinned, as before.
    assert!(read(&Values::default()).unwrap().known_digests.is_empty());
    let pinned = Values::default().with(
        crate::DIGESTS_SETTING,
        list(&[one.to_string(), two.to_string()]),
    );
    assert_eq!(read(&pinned).unwrap().known_digests, [one, two]);
    // A pin that cannot be read is never taken as no pin.
    for wrong in [
        Value::Text(one.to_string()),
        list(&["not a digest".to_owned()]),
        Value::List(vec![Value::Int(1)]),
    ] {
        let failure = read(&Values::default().with(crate::DIGESTS_SETTING, wrong)).unwrap_err();
        assert_eq!(failure.class(), FailureClass::Input);
    }
}
