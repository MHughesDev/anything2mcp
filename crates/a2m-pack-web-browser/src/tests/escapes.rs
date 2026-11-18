//! The ways out of a tab's lock, each tried by a page of the test
//! application: a page that moves the tab by itself, a download, a frame
//! from another site, a window the page opens, a service worker, a shared
//! worker, a permission or a file asked for, and a page on an origin the
//! tab was not opened for. For each, the test
//! application's control interface says that nothing arrived.

use crate::{
    Input, ObserveTab, PageRequest, Primitive, RequestShape, SafeRequests, SnapshotLimits,
    TextMatch,
};

use std::collections::BTreeMap;

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

/// What arrived, once every one of `expected` has. A page that says it
/// made a socket says the socket was made, not that the test application
/// has seen it connect, so a test of what arrives waits for that, up to a
/// deadline, and then compares the whole of what arrived.
async fn arrived_all(app: &TestApp, expected: &[&str]) -> Vec<String> {
    let started = std::time::Instant::now();
    loop {
        let now = arrived(app).await;
        if expected
            .iter()
            .all(|name| now.iter().any(|one| one == name))
        {
            return now;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "not everything arrived: {now:?}, waiting for {expected:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
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
    observing_with(stand, identity, &only(app)).await
}

async fn observing_with(stand: &Stand, identity: &str, origins: &Origins) -> ObserveTab {
    must(
        stand
            .browsers
            .observer()
            .observing_tab(
                &profile(identity),
                &environment(),
                &SafeRequests::default(),
                origins,
            )
            .await,
    )
}

/// The application's own origin, and its other name as a place the tab's
/// page may send requests to.
fn reaching_the_other_name(app: &TestApp) -> Origins {
    only(app).and_requests_to([&at(&other(app, "/"))])
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
    // The frame's origin is one the page may load from; the tab is never
    // on it (decision 0292).
    let tab = observing_with(&stand, "scout", &reaching_the_other_name(&app)).await;
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

    // First, that the test means something. A page of a tab that may act
    // cannot register a worker (decision 0292); the one tab whose page
    // still can is a sign-in tab, which installs it here. Then a page of a
    // tab that may act wakes it, and the worker writes to its own origin,
    // which the tab may reach; its socket is refused, since a service
    // worker is held to the strictest lock in every tab.
    let browser = stand.open(&clerk).await;
    let register = "navigator.serviceWorker.register('/escapes/service-worker.js', {scope: '/escapes/'})\
        .then(() => navigator.serviceWorker.ready).then(() => 'ready', () => 'refused')";
    let acting = must(browser.acting_tab(&environment(), &only(&app)).await);
    must(acting.goto(&quiet, soon()).await);
    assert_eq!(
        must(acting.page.as_the_page(register, soon()).await),
        "refused"
    );
    refused(|| acting.blocked(), "SERVICEWORKER").await;
    let installing = must(
        browser
            .sign_in_tab(&environment(), &only(&app), &Origins::default())
            .await,
    );
    must(installing.goto(&quiet, soon()).await);
    assert_eq!(
        must(installing.page.as_the_page(register, soon()).await),
        "ready"
    );
    drop(installing);
    assert_eq!(
        must(acting.page.as_the_page(poke, soon()).await),
        "post sent, socket refused"
    );
    let post = ["escapes/service-post"];
    assert_eq!(arrived_all(&app, &post).await, post);
    // The tab that may act passes the worker by: a request of its page
    // reaches the application, which serves no such page, and is not
    // answered by the worker (decision 0245, audit finding M6).
    let asked = "fetch('/escapes/answered-by-worker').then((answer) => answer.status + ' ' + answer.statusText)";
    let answered = must(acting.page.as_the_page(asked, soon()).await);
    assert!(answered.starts_with("404"), "{answered}");
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
async fn a_shared_worker_is_refused_to_a_page_of_either_kind_of_tab() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("shared");
    let quiet = at(&app.url("/escapes/quiet"));
    // The page connects to the shared worker, directly or through a frame
    // it makes for the purpose, and the worker writes when it is reached.
    let connect = |through_a_frame: bool| {
        let realm = if through_a_frame {
            "(() => { const frame = document.createElement('iframe'); \
             document.body.append(frame); return frame.contentWindow; })()"
        } else {
            "window"
        };
        format!(
            "(() => {{ const realm = {realm}; let worker; \
             try {{ worker = new realm.SharedWorker('/escapes/shared-worker.js'); }} \
             catch (error) {{ return 'refused'; }} \
             return new Promise((resolve) => {{ worker.port.onmessage = (event) => resolve(event.data); \
             worker.port.start(); }}); }})()"
        )
    };

    // A page of a tab that may act cannot start one: its requests would be
    // the worker's own, which the tab's interception does not hold
    // (decision 0292). So no shared worker runs that a page of the same
    // origin could reach.
    let browser = stand.open(&profile("clerk")).await;
    let acting = must(browser.acting_tab(&environment(), &only(&app)).await);
    must(acting.goto(&quiet, soon()).await);
    assert_eq!(
        must(acting.page.as_the_page(&connect(false), soon()).await),
        "refused"
    );
    refused(|| acting.blocked(), "SHAREDWORKER").await;

    // A page that is only observed cannot start one either: not directly,
    // and not through a frame.
    let tab = observing(&stand, &app, "clerk").await;
    must(tab.goto(&quiet, soon()).await);
    for through_a_frame in [false, true] {
        assert_eq!(
            must(
                tab.page
                    .as_the_page(&connect(through_a_frame), soon())
                    .await
            ),
            "refused",
            "through a frame: {through_a_frame}"
        );
    }
    refused(|| tab.blocked(), "SHAREDWORKER").await;
    drop((tab, acting));
    stand.end().await;
    assert_eq!(arrived(&app).await, Vec::<String>::new());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_s_requests_for_a_permission_or_a_file_are_refused_with_no_prompt() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("prompts");
    let quiet = at(&app.url("/escapes/quiet"));
    // Each request is made as if a person had just used the page, which a
    // browser asks for before it shows a prompt. A prompt left open would
    // leave its request unanswered, and the page says which.
    let ask = "(() => { const answered = (what, promise) => Promise.race([promise, \
        new Promise((resolve) => setTimeout(() => resolve(what + ' unanswered'), 10000))]); \
        return Promise.all([\
        answered('geolocation', new Promise((resolve) => navigator.geolocation.getCurrentPosition(\
            () => resolve('located'), (error) => resolve('geolocation ' + error.code)))),\
        answered('notifications', Notification.requestPermission().then((answer) => 'notifications ' + answer)),\
        answered('clipboard', navigator.clipboard.readText().then(() => 'clipboard read', \
            (error) => 'clipboard ' + error.name)),\
        ]).then((answers) => answers.join(', ')); })()";
    let choose = "(() => { const input = document.createElement('input'); input.type = 'file'; \
        document.body.append(input); input.click(); return 'chosen'; })()";
    // Geolocation is denied with the code for a refused permission.
    let refused = "geolocation 1, notifications denied, clipboard NotAllowedError";

    // And no permission is granted, or left for a prompt to grant: among
    // them a service worker's later requests, and the clipboard. A name the
    // browser does not know is no permission. Asking for storage of an
    // embedded page stays a prompt, and the browser answers it alone.
    let open = "Promise.all(['geolocation', 'notifications', 'camera', 'microphone', 'clipboard-read', \
        'clipboard-write', 'background-sync', 'background-fetch', 'periodic-background-sync', \
        'persistent-storage', 'midi', 'accelerometer', 'gyroscope', 'magnetometer', 'screen-wake-lock', \
        'local-fonts', 'payment-handler', 'idle-detection', 'window-management', 'storage-access']\
        .map((name) => navigator.permissions.query({name}).then((answer) => \
        answer.state === 'denied' || (answer.state === 'prompt' && name === 'storage-access') \
        ? '' : name + '=' + answer.state, () => ''))).then((all) => all.filter(Boolean).join(' '))";
    // Each tab is asked while it is the one shown: a hidden page's request
    // for a location waits until the page is shown again.
    let browser = stand.open(&profile("clerk")).await;
    let acting = must(browser.acting_tab(&environment(), &only(&app)).await);
    must(acting.goto(&quiet, soon()).await);
    assert_eq!(
        must(acting.page.as_the_page_with_a_gesture(ask, soon()).await),
        refused
    );
    assert_eq!(must(acting.page.as_the_page(open, soon()).await), "");
    // A file chooser opens nothing, and the page goes on.
    assert_eq!(
        must(acting.page.as_the_page_with_a_gesture(choose, soon()).await),
        "chosen"
    );
    must(
        acting
            .locate(&Query::role("heading", "Order notes"), soon())
            .await,
    );
    drop(acting);

    let tab = observing(&stand, &app, "clerk").await;
    must(tab.goto(&quiet, soon()).await);
    assert_eq!(
        must(tab.page.as_the_page_with_a_gesture(ask, soon()).await),
        refused
    );
    assert_eq!(must(tab.page.as_the_page(open, soon()).await), "");
    assert_eq!(
        must(tab.page.as_the_page_with_a_gesture(choose, soon()).await),
        "chosen"
    );
    must(
        tab.locate(&Query::role("heading", "Order notes"), soon())
            .await,
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

/// What arrived at `app` from `/escapes/loads`, by name and count.
async fn loads_arrived(app: &TestApp) -> BTreeMap<String, u64> {
    app.trap_writes()
        .await
        .unwrap()
        .into_iter()
        .filter(|(name, _)| name.starts_with("escapes/loads-"))
        .collect()
}

/// The names of `/escapes/loads`' attempts, as the application counts them.
fn loads(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .map(|name| format!("escapes/loads-{name}"))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tab_sends_nothing_to_an_origin_outside_its_reach_and_what_it_may_reach_arrives() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("reach");
    let browser = stand.open(&profile("clerk")).await;
    let page = at(&app.url("/escapes/loads"));
    let elsewhere = |name: &str| other(&app, &format!("{WRITE}/loads-{name}"));
    let shapes = |seen: Vec<RequestShape>, word: &str| -> Vec<String> {
        let mut found: Vec<String> = seen
            .into_iter()
            .filter(|one| one.method == word)
            .map(|one| one.address)
            .collect();
        found.sort();
        found
    };

    // The page loads an image, a script, and a style, sends a `fetch` with
    // GET and one with POST, and opens a socket, all at the application's
    // other name. A tab opened for the application alone, in either
    // posture, sends none of them: each is refused in the browser, and
    // reported with its address.
    let acting = must(browser.acting_tab(&environment(), &only(&app)).await);
    must(acting.goto(&page, soon()).await);
    must(
        acting
            .wait(&Primitive::Visible(css("#tried")), soon())
            .await,
    );
    let from_acting = refused(|| acting.blocked(), "WEBSOCKET").await;
    let observing = observing(&stand, &app, "scout").await;
    must(observing.goto(&page, soon()).await);
    must(
        observing
            .wait(&Primitive::Visible(css("#tried")), soon())
            .await,
    );
    let from_observing = refused(|| observing.blocked(), "WEBSOCKET").await;
    let mut outside: Vec<String> = ["fetch", "image", "post", "script", "style"]
        .iter()
        .map(|name| elsewhere(name))
        .collect();
    outside.sort();
    for seen in [from_acting, from_observing] {
        assert_eq!(shapes(seen.clone(), crate::OUTSIDE), outside);
        assert_eq!(
            shapes(seen, "WEBSOCKET"),
            [elsewhere("socket").replacen("http", "ws", 1)]
        );
    }
    // Neither name of a peer connection gives one, in either posture: the
    // prefixed name is a second property holding the same constructor.
    let peer = "['RTCPeerConnection', 'webkitRTCPeerConnection'].map((name) => { \
        if (!(name in globalThis)) return name + ' absent'; \
        try { new globalThis[name]({iceServers: []}); return name + ' made'; } \
        catch (error) { return name + ' refused'; } }).join(', ')";
    let from_acting = must(acting.page.as_the_page(peer, soon()).await);
    let from_observing = must(observing.page.as_the_page(peer, soon()).await);
    for answer in [from_acting, from_observing] {
        assert!(!answer.contains("made"), "{answer}");
        assert!(answer.starts_with("RTCPeerConnection refused"), "{answer}");
    }
    drop(acting);
    drop(observing);
    assert_eq!(loads_arrived(&app).await, BTreeMap::new());

    // Declared as a place the page may send requests to, the other name
    // receives what each posture lets through: every attempt from a tab
    // that may act; the reads alone from one that observes, which still
    // refuses the POST and the socket as it always has.
    let acting = must(
        browser
            .acting_tab(&environment(), &reaching_the_other_name(&app))
            .await,
    );
    must(acting.goto(&page, soon()).await);
    must(
        acting
            .wait(&Primitive::Visible(css("#tried")), soon())
            .await,
    );
    let all = loads(&["fetch", "image", "post", "script", "socket", "style"]);
    let names: Vec<&str> = all.iter().map(String::as_str).collect();
    let started = std::time::Instant::now();
    while loads_arrived(&app).await.len() < all.len() {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "{:?}",
            loads_arrived(&app).await
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        loads_arrived(&app).await.into_keys().collect::<Vec<_>>(),
        names
    );
    assert_eq!(acting.blocked(), []);
    drop(acting);

    app.reset().await.unwrap();
    let observing = observing_with(&stand, "scout_2", &reaching_the_other_name(&app)).await;
    must(observing.goto(&page, soon()).await);
    must(
        observing
            .wait(&Primitive::Visible(css("#tried")), soon())
            .await,
    );
    let seen = refused(|| observing.blocked(), "WEBSOCKET").await;
    assert_eq!(shapes(seen.clone(), crate::OUTSIDE), Vec::<String>::new());
    assert_eq!(shapes(seen, "POST"), [elsewhere("post")]);
    assert_eq!(
        loads_arrived(&app).await.into_keys().collect::<Vec<_>>(),
        loads(&["fetch", "image", "script", "style"])
    );
    drop(observing);
    drop(browser);
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_frame_in_another_process_of_an_acting_tab_sends_nothing_outside_the_tab_s_reach() {
    // A third origin: a second application, on a port of its own.
    let app = TestApp::start().await.unwrap();
    let third = TestApp::start().await.unwrap();
    let stand = Stand::with_browser("reach-frame");
    let browser = stand.open(&profile("clerk")).await;
    let tab = must(
        browser
            .acting_tab(&environment(), &reaching_the_other_name(&app))
            .await,
    );
    // The page, and a frame of the application's other name, which the
    // browser runs in a process of its own, each try to load from the
    // third origin.
    let frame = other(&app, "/");
    let from = third.url("/");
    let page = format!(
        "/escapes/loads?frame={}&from={}",
        frame
            .trim_end_matches('/')
            .replace(':', "%3A")
            .replace('/', "%2F"),
        from.trim_end_matches('/')
            .replace(':', "%3A")
            .replace('/', "%2F"),
    );
    must(tab.goto(&at(&app.url(&page)), soon()).await);
    must(tab.wait(&Primitive::Visible(css("#tried")), soon()).await);
    must(
        tab.wait(&Primitive::Visible(css("#frame-tried")), soon())
            .await,
    );
    let seen = tab.blocked();
    // Five refusals by the browser, and a socket by the page, for each of
    // the two documents.
    let third_origin = from.trim_end_matches('/');
    let outside: Vec<&RequestShape> = seen
        .iter()
        .filter(|one| one.method == crate::OUTSIDE)
        .collect();
    assert_eq!(outside.len(), 10, "{seen:?}");
    assert!(
        outside
            .iter()
            .all(|one| one.address.starts_with(third_origin)),
        "{seen:?}"
    );
    // The frame did load, from the other name: the application served the
    // page twice, and the third heard nothing at all.
    let served = app
        .seen()
        .await
        .unwrap()
        .into_iter()
        .filter(|line| line == "GET /escapes/loads")
        .count();
    assert_eq!(served, 2);
    assert_eq!(third.seen_all().await.unwrap(), Vec::<String>::new());
    assert_eq!(loads_arrived(&third).await, BTreeMap::new());
    drop(tab);
    drop(browser);
    stand.end().await;
}
