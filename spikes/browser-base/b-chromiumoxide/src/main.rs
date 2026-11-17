//! Throwaway spike code. Candidate (b): the `chromiumoxide` crate, 0.9.1.
//! The same steps as every other candidate; see ../README.md.
//!
//! The crate starts the browser itself, on a loopback port it reads back from
//! the browser's error output. It cannot use the debugging pipe.

use std::time::{Duration, Instant};

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::accessibility::GetFullAxTreeParams;
use chromiumoxide::cdp::browser_protocol::fetch::{
    EnableParams, EventRequestPaused, FailRequestParams, RequestPattern, RequestStage,
};
use chromiumoxide::cdp::browser_protocol::network::ErrorReason;
use chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;
use futures::StreamExt;
use spike_browser_common as common;

/// The crate has no wait for an element, so the page waits for itself, as in candidate (a).
const FIELD_VISIBLE: &str = r#"new Promise((resolve) => {
  const found = () => {
    const label = [...document.querySelectorAll("label")].find((l) => l.textContent.trim() === "Description");
    return label && label.control && label.control.getClientRects().length > 0;
  };
  if (found()) return resolve(true);
  const observer = new MutationObserver(() => { if (found()) { observer.disconnect(); resolve(true); } });
  observer.observe(document, {subtree: true, childList: true, attributes: true});
})"#;

async fn start(tag: &str) -> (Browser, tokio::task::JoinHandle<()>, std::path::PathBuf) {
    let dir = common::owned_data_dir(tag);
    let config = BrowserConfig::builder()
        .chrome_executable(common::browser_program())
        .user_data_dir(&dir)
        .no_sandbox() // this container only
        .build()
        .expect("config");
    let (browser, mut handler) = Browser::launch(config).await.expect("launch");
    let pump = tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if event.is_err() {
                break;
            }
        }
    });
    (browser, pump, dir)
}

#[tokio::main]
async fn main() {
    let app = common::TestApp::start()
        .await
        .expect("the test application starts");
    let url = app.url(common::FORM_PATH);
    let mut report = common::Report {
        candidate: "b: chromiumoxide 0.9.1".to_owned(),
        transport: "WebSocket on a loopback port".to_owned(),
        ..Default::default()
    };

    let t0 = Instant::now();
    let (mut browser, pump, dir) = start("oxide").await;
    report.browser_version = browser.version().await.expect("version").product;
    report.launch_to_connected_ms = common::ms(t0.elapsed());
    let page = browser.new_page(url.as_str()).await.expect("page");
    page.wait_for_navigation().await.expect("navigation");
    page.evaluate(
        EvaluateParams::builder()
            .expression(FIELD_VISIBLE)
            .await_promise(true)
            .build()
            .expect("params"),
    )
    .await
    .expect("wait");
    let field = page
        .find_element(format!("#{}", common::FIELD_ID))
        .await
        .expect("field");
    report.field_value = field
        .property("value")
        .await
        .expect("read")
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    report.cold_start_to_first_read_ms = common::ms(t0.elapsed());
    report.wait_kind = "the crate's navigation wait, then our own in-page observer; the crate has no wait for an element".to_owned();

    let mut samples = Vec::new();
    for _ in 0..common::WARM_READS {
        let t = Instant::now();
        field.property("value").await.expect("read");
        samples.push(t.elapsed());
    }
    report.warm_reads = common::summarize(samples);

    let t = Instant::now();
    let tree = page
        .execute(GetFullAxTreeParams::default())
        .await
        .expect("tree");
    report.snapshot_ms = common::ms(t.elapsed());
    report.snapshot_nodes = tree.result.nodes.len();
    report.snapshot_kind = "accessibility tree, through the crate's raw command".to_owned();

    let mut paused = page
        .event_listener::<EventRequestPaused>()
        .await
        .expect("listener");
    let pattern = RequestPattern::builder()
        .url_pattern("*/api/orders/*")
        .request_stage(RequestStage::Request)
        .build();
    page.execute(EnableParams::builder().pattern(pattern).build())
        .await
        .expect("intercept");
    page.evaluate("document.querySelector('#line-item button[type=submit]').click()")
        .await
        .expect("click");
    let event = tokio::time::timeout(Duration::from_secs(10), paused.next())
        .await
        .expect("in time")
        .expect("a paused request");
    page.execute(FailRequestParams::new(
        event.request_id.clone(),
        ErrorReason::BlockedByClient,
    ))
    .await
    .expect("fail");
    report.intercepted = format!(
        "{} {} failed before sending",
        event.request.method,
        event.request.url.replace(&app.url(""), "")
    );
    report.writes_stored_after_blocked_save = app
        .writes()
        .await
        .expect("the control interface answers")
        .len();

    let _ = browser.close().await;
    let exited = tokio::time::timeout(Duration::from_secs(10), browser.wait()).await;
    report.notes.push(format!("exit after close: {exited:?}"));
    pump.abort();
    report.clean_close = common::wait_no_leftovers(&dir, Duration::from_secs(5));
    common::remove_data_dir(&dir);

    // Kill the browser while a call is pending.
    let (mut browser, pump, dir) = start("oxide-kill").await;
    let page = browser.new_page(url.as_str()).await.expect("page");
    page.wait_for_navigation().await.expect("navigation");
    let pending = tokio::spawn(async move {
        page.evaluate(
            EvaluateParams::builder()
                .expression("new Promise(() => {})")
                .await_promise(true)
                .build()
                .expect("params"),
        )
        .await
        .map(|_| ())
    });
    tokio::time::sleep(Duration::from_millis(300)).await; // the spike's own timing, so the call is on the wire
    let t = Instant::now();
    browser
        .get_mut_child()
        .expect("child")
        .inner
        .start_kill()
        .expect("kill");
    report.kill_mid_call.outcome =
        match tokio::time::timeout(Duration::from_secs(15), pending).await {
            Ok(Ok(result)) => format!("{result:?}"),
            Ok(Err(_)) => "panicked".to_owned(),
            Err(_) => "hung: no answer in 15 s".to_owned(),
        };
    report.kill_mid_call.after_kill_ms = common::ms(t.elapsed());
    let late = tokio::time::timeout(Duration::from_secs(5), browser.version()).await;
    report.notes.push(format!(
        "a call after the kill: {:?}",
        late.map(|r| r.map(|v| v.product))
    ));
    let _ = browser.wait().await;
    pump.abort();
    report.kill_mid_call.leftover = common::wait_no_leftovers(&dir, Duration::from_secs(5));
    common::remove_data_dir(&dir);

    report.print();
}
