//! Throwaway spike code. Candidate (a): our own client over the debugging pipe.
//! The same steps as every other candidate; see ../README.md.

mod cdp;
mod launch;

use std::time::{Duration, Instant};

use cdp::{Cdp, CdpError, wait_event};
use launch::{Handoff, Launched, launch};
use serde_json::{Value, json};
use spike_browser_common as common;

const DEADLINE: Duration = Duration::from_secs(10);

/// Resolves when a field with this label is laid out. An observer, not a poll.
const FIELD_VISIBLE: &str = r#"(name) => new Promise((resolve) => {
  const found = () => {
    const label = [...document.querySelectorAll("label")].find((l) => l.textContent.trim() === name);
    return label && label.control && label.control.getClientRects().length > 0;
  };
  if (found()) return resolve(true);
  const observer = new MutationObserver(() => { if (found()) { observer.disconnect(); resolve(true); } });
  observer.observe(document, {subtree: true, childList: true, attributes: true});
})"#;

struct Tab {
    cdp: Cdp,
    session: String,
    frame: String,
}

impl Tab {
    async fn call(&self, method: &str, params: Value) -> Result<Value, CdpError> {
        self.cdp.call(Some(&self.session), method, params, DEADLINE).await
    }
}

async fn open_tab(cdp: &Cdp) -> Result<Tab, CdpError> {
    let target = cdp.call(None, "Target.createTarget", json!({"url": "about:blank"}), DEADLINE).await?;
    let frame = target["targetId"].as_str().unwrap_or_default().to_owned();
    let attached = cdp
        .call(None, "Target.attachToTarget", json!({"targetId": frame, "flatten": true}), DEADLINE)
        .await?;
    let tab = Tab { cdp: cdp.clone(), session: attached["sessionId"].as_str().unwrap_or_default().to_owned(), frame };
    tab.call("Page.enable", json!({})).await?;
    Ok(tab)
}

/// Navigates, then waits for a protocol event and for a named condition in the page.
async fn goto_form(tab: &Tab, url: &str) -> Result<(), CdpError> {
    let mut events = tab.cdp.events();
    tab.call("Page.navigate", json!({"url": url})).await?;
    wait_event(&mut events, DEADLINE, |e| (e.method == "Page.domContentEventFired").then_some(())).await?;
    // The helper runs in a world of its own, which the page's scripts cannot reach.
    let world = tab
        .call("Page.createIsolatedWorld", json!({"frameId": tab.frame, "worldName": "a2m-spike"}))
        .await?;
    tab.call(
        "Runtime.evaluate",
        json!({
            "expression": format!("({FIELD_VISIBLE})({})", json!(common::FIELD_NAME)),
            "contextId": world["executionContextId"],
            "awaitPromise": true,
            "returnByValue": true,
        }),
    )
    .await?;
    Ok(())
}

/// Exactly one text box with this accessible name, as a remote object.
async fn locate(tab: &Tab, name: &str) -> Result<String, CdpError> {
    let document = tab.call("DOM.getDocument", json!({"depth": 0})).await?;
    let found = tab
        .call(
            "Accessibility.queryAXTree",
            json!({"nodeId": document["root"]["nodeId"], "accessibleName": name, "role": "textbox"}),
        )
        .await?;
    let nodes = found["nodes"].as_array().cloned().unwrap_or_default();
    assert_eq!(nodes.len(), 1, "the query must name exactly one element");
    let object = tab.call("DOM.resolveNode", json!({"backendNodeId": nodes[0]["backendDOMNodeId"]})).await?;
    Ok(object["object"]["objectId"].as_str().unwrap_or_default().to_owned())
}

async fn read_value(tab: &Tab, object: &str) -> Result<String, CdpError> {
    let value = tab
        .call(
            "Runtime.callFunctionOn",
            json!({"objectId": object, "functionDeclaration": "function () { return this.value; }", "returnByValue": true}),
        )
        .await?;
    Ok(value["result"]["value"].as_str().unwrap_or_default().to_owned())
}

async fn start(tag: &str, handoff: Handoff) -> (Launched, std::path::PathBuf) {
    let dir = common::owned_data_dir(tag);
    let launched = launch(&common::browser_program(), &common::base_args(&dir), handoff).expect("launch");
    (launched, dir)
}

#[tokio::main]
async fn main() {
    let handoff = match std::env::args().nth(1).as_deref() {
        #[cfg(unix)]
        Some("shell") => Handoff::ShellExec,
        #[cfg(unix)]
        _ => Handoff::MappedFds,
        #[cfg(windows)]
        _ => Handoff::InheritedHandles,
    };
    let app = common::TestApp::start().await.expect("the test application starts");
    let url = app.url(common::FORM_PATH);
    let mut report = common::Report {
        candidate: "a: own client".to_owned(),
        transport: format!("pipe, {handoff:?}"),
        ..Default::default()
    };

    // Launch, connect, open the page, wait, read one field.
    let t0 = Instant::now();
    let (Launched { mut child, to_browser, from_browser }, dir) = start("own", handoff).await;
    let cdp = Cdp::connect(from_browser, to_browser);
    let version = cdp.call(None, "Browser.getVersion", json!({}), DEADLINE).await.expect("version");
    report.launch_to_connected_ms = common::ms(t0.elapsed());
    report.browser_version = version["product"].as_str().unwrap_or_default().to_owned();
    let tab = open_tab(&cdp).await.expect("tab");
    goto_form(&tab, &url).await.expect("page");
    let field = locate(&tab, common::FIELD_NAME).await.expect("locate");
    report.field_value = read_value(&tab, &field).await.expect("read");
    report.cold_start_to_first_read_ms = common::ms(t0.elapsed());
    report.wait_kind = "protocol event, then an in-page observer in an isolated world".to_owned();

    // Reads on a warm page: one round trip each on the located element.
    let mut samples = Vec::new();
    for _ in 0..common::WARM_READS {
        let t = Instant::now();
        read_value(&tab, &field).await.expect("read");
        samples.push(t.elapsed());
    }
    report.warm_reads = common::summarize(samples);
    let mut samples = Vec::new();
    for _ in 0..common::WARM_READS {
        let t = Instant::now();
        let again = locate(&tab, common::FIELD_NAME).await.expect("locate");
        read_value(&tab, &again).await.expect("read");
        samples.push(t.elapsed());
    }
    let located = common::summarize(samples);
    report.notes.push(format!(
        "locate by role and name with Accessibility.queryAXTree, then read (4 round trips): median {} ms, p95 {} ms",
        located.median_ms, located.p95_ms
    ));
    // The same question answered by a helper in the page, in one round trip.
    let world = tab
        .call("Page.createIsolatedWorld", json!({"frameId": tab.frame, "worldName": "a2m-spike-read"}))
        .await
        .expect("world");
    let by_label = format!(
        "[...document.querySelectorAll('label')].filter((l) => l.textContent.trim() === {}).map((l) => l.control.value)",
        json!(common::FIELD_NAME)
    );
    let mut samples = Vec::new();
    for _ in 0..common::WARM_READS {
        let t = Instant::now();
        let values = tab
            .call(
                "Runtime.evaluate",
                json!({"expression": by_label, "contextId": world["executionContextId"], "returnByValue": true}),
            )
            .await
            .expect("helper");
        assert_eq!(values["result"]["value"], json!([report.field_value]));
        samples.push(t.elapsed());
    }
    let helped = common::summarize(samples);
    report.notes.push(format!(
        "locate by label with an in-page helper and read, one round trip: median {} ms, p95 {} ms",
        helped.median_ms, helped.p95_ms
    ));

    // A snapshot of the accessibility tree.
    let t = Instant::now();
    let tree = tab.call("Accessibility.getFullAXTree", json!({})).await.expect("tree");
    report.snapshot_ms = common::ms(t.elapsed());
    report.snapshot_nodes = tree["nodes"].as_array().map_or(0, Vec::len);
    report.snapshot_kind = "accessibility tree".to_owned();

    // Intercept the save the page sends, and fail it before it leaves the browser.
    tab.call("Fetch.enable", json!({"patterns": [{"urlPattern": "*/api/orders/*", "requestStage": "Request"}]}))
        .await
        .expect("intercept");
    let mut events = cdp.events();
    tab.call(
        "Runtime.evaluate",
        json!({"expression": "document.querySelector('#line-item button[type=submit]').click()"}),
    )
    .await
    .expect("click");
    let paused = wait_event(&mut events, DEADLINE, |e| (e.method == "Fetch.requestPaused").then(|| e.params.clone()))
        .await
        .expect("a paused request");
    tab.call("Fetch.failRequest", json!({"requestId": paused["requestId"], "errorReason": "BlockedByClient"}))
        .await
        .expect("fail the request");
    report.intercepted = format!(
        "{} {} failed before sending",
        paused["request"]["method"].as_str().unwrap_or("?"),
        paused["request"]["url"].as_str().unwrap_or("?").replace(&app.url(""), "")
    );
    report.writes_stored_after_blocked_save = app.writes().await.expect("the control interface answers").len();

    // Close, and check that nothing is left.
    let _ = cdp.call(None, "Browser.close", json!({}), DEADLINE).await;
    let exited = tokio::time::timeout(DEADLINE, child.wait()).await;
    report.notes.push(format!("exit after Browser.close: {exited:?}"));
    report.clean_close = common::wait_no_leftovers(&dir, Duration::from_secs(5));
    common::remove_data_dir(&dir);

    // Kill the browser while a call is pending.
    let (Launched { mut child, to_browser, from_browser }, dir) = start("own-kill", handoff).await;
    let cdp = Cdp::connect(from_browser, to_browser);
    let tab = open_tab(&cdp).await.expect("tab");
    goto_form(&tab, &url).await.expect("page");
    let pending = tokio::spawn(async move {
        tab.call("Runtime.evaluate", json!({"expression": "new Promise(() => {})", "awaitPromise": true})).await
    });
    tokio::time::sleep(Duration::from_millis(300)).await; // the spike's own timing, so the call is on the wire
    let t = Instant::now();
    child.start_kill().expect("kill");
    report.kill_mid_call.outcome = match tokio::time::timeout(Duration::from_secs(15), pending).await {
        Ok(Ok(result)) => format!("{result:?}"),
        Ok(Err(_)) => "panicked".to_owned(),
        Err(_) => "hung".to_owned(),
    };
    report.kill_mid_call.after_kill_ms = common::ms(t.elapsed());
    let late = cdp.call(None, "Browser.getVersion", json!({}), DEADLINE).await;
    report.notes.push(format!("a call after the kill: {late:?}"));
    let _ = child.wait().await;
    report.kill_mid_call.leftover = common::wait_no_leftovers(&dir, Duration::from_secs(5));
    common::remove_data_dir(&dir);

    report.print();
}
