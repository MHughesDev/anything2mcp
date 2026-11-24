//! Package 8.2a: the tools a page registers for agents, called through the
//! `site_tools` mode, against the test application's page
//! `/cases/site-tools`. Every proof asks the application what the page was
//! told, never the driver's own account.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{FailureClass, NoSecrets, Plan, Value, Values};
use a2m_provider::{Posture, Sent, StepFailure};
use common::{Scene, Stand, Write, compile, compile_changed};
use example_app::harness::TestApp;

const NO_DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn plan_with(stand: &Stand, learned: &[(&str, &str)]) -> Plan {
    if learned.is_empty() {
        return compile(&stand.registry, "cases").plan;
    }
    compile_changed(&stand.registry, "cases", "operations/tools.yaml", |text| {
        let mut text = text.to_owned();
        for (tool, digest) in learned {
            text = text.replace(
                &format!("tool: {tool}\n    digest: \"{NO_DIGEST}\""),
                &format!("tool: {tool}\n    digest: \"{digest}\""),
            );
        }
        text
    })
    .unwrap_or_else(|problems| panic!("{}", common::shown(&problems)))
    .plan
}

async fn told(app: &TestApp, count: usize) -> Vec<String> {
    let started = std::time::Instant::now();
    loop {
        let events = app.case_events().await.unwrap();
        if events.len() >= count || started.elapsed() > Duration::from_secs(20) {
            return events;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Calls one operation of the fixture on a fresh page.
async fn call(
    app: &TestApp,
    learned: &[(&str, &str)],
    operation: &str,
    inputs: &Values,
) -> Result<common::Ran, StepFailure> {
    let stand = Stand::new("site-tools", Arc::new(NoSecrets));
    let plan = plan_with(&stand, learned);
    let scene = Scene::over(stand, plan, &app.url(""));
    let write = Write::new(&["site-tools"]).await;
    let (handle, mut prepared) = scene
        .try_prepare_in(&common::site_tools_mode(), Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let steps = &scene.binding_in(operation, &common::site_tools_mode()).act;
    let ran = scene
        .run(
            &handle,
            &mut prepared,
            steps,
            inputs,
            Some(write.authority()),
        )
        .await;
    let ended = handle.release(prepared, ran.is_err()).await;
    assert!(ended.complete || ran.is_err());
    scene.stand.end().await;
    ran
}

/// The digest the page's tool has, learned the way a person learns it: the
/// call with a digest that is not it stops, and says which it is.
async fn learned(app: &TestApp, tool: &str, operation: &str, inputs: &Values) -> String {
    let stopped = call(app, &[], operation, inputs).await.unwrap_err();
    assert_eq!(
        (stopped.failure.class(), stopped.failure.code().as_str()),
        (FailureClass::Drift, "site_tool_changed"),
        "{tool}: {stopped}"
    );
    assert_eq!(stopped.sent, Sent::NotSent);
    let message = stopped.failure.message().to_owned();
    let (_, after) = message.split_once("its digest is ").unwrap();
    after[..64].to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_the_page_registered_is_called_by_its_name_once_its_digest_is_the_one_held() {
    let app = TestApp::start().await.unwrap();
    let inputs = Values::new().with("text", "Dock 4 at nine");
    // A digest that is not the tool's: nothing is sent, and the stop says
    // which digest the page's tool has.
    let digest = learned(&app, "example_note_save", "example_page_note_save", &inputs).await;
    assert!(app.case_events().await.unwrap().is_empty());

    let ran = call(
        &app,
        &[("example_note_save", &digest)],
        "example_page_note_save",
        &inputs,
    )
    .await
    .unwrap();
    assert_eq!(
        ran.values.get("saying"),
        Some(&Value::from("Saved note: Dock 4 at nine"))
    );
    assert_eq!(told(&app, 1).await, ["note-saved: Dock 4 at nine"]);
    assert!(
        ran.means_text().contains("call_tool"),
        "{}",
        ran.means_text()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_the_page_marks_as_reading_is_still_held_to_an_effect_nothing_can_take_back() {
    let app = TestApp::start().await.unwrap();
    let inputs = Values::new().with("order", "A-100");
    let digest = learned(&app, "example_total_get", "example_page_total_get", &inputs).await;
    let ran = call(
        &app,
        &[("example_total_get", &digest)],
        "example_page_total_get",
        &inputs,
    )
    .await
    .unwrap();
    assert_eq!(
        ran.values.get("saying"),
        Some(&Value::from("Total of A-100: 42"))
    );
    // The page's word that the tool only reads does not lower what the
    // binding is: an operation that says it reads does not compile.
    let stand = Stand::new("site-tools-effect", Arc::new(NoSecrets));
    let refused = compile_changed(&stand.registry, "cases", "operations/tools.yaml", |text| {
        text.replacen("effect: irreversible", "effect: read", 2)
    });
    assert!(
        refused.is_err(),
        "an operation that reads compiled over a tool of a page"
    );
    stand.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_that_fails_is_a_write_whose_end_is_not_known_and_one_never_offered_is_a_drift() {
    let app = TestApp::start().await.unwrap();
    let digest = learned(
        &app,
        "example_fail",
        "example_page_fail_call",
        &Values::new(),
    )
    .await;
    let failed = call(
        &app,
        &[("example_fail", &digest)],
        "example_page_fail_call",
        &Values::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        (failed.failure.class(), failed.failure.code().as_str()),
        (FailureClass::PossibleCommit, "no_confirmation"),
        "{failed}"
    );
    assert_eq!(failed.sent, Sent::Unknown);
    assert_eq!(told(&app, 1).await, ["failing: now"]);

    let gone_app = TestApp::start().await.unwrap();
    let stopped = call_over(
        &gone_app,
        "example_never_registered",
        "example_page_fail_call",
    )
    .await;
    assert_eq!(
        (stopped.failure.class(), stopped.failure.code().as_str()),
        (FailureClass::Drift, "site_tool_not_offered"),
        "{stopped}"
    );
    assert_eq!(stopped.sent, Sent::NotSent);
    assert!(gone_app.case_events().await.unwrap().is_empty());
}

/// Calls an operation whose tool name is changed in the fixture.
async fn call_over(app: &TestApp, tool: &str, operation: &str) -> StepFailure {
    let stand = Stand::new("site-tools-over", Arc::new(NoSecrets));
    let plan = compile_changed(&stand.registry, "cases", "operations/tools.yaml", |text| {
        text.replace("tool: example_fail", &format!("tool: {tool}"))
    })
    .unwrap()
    .plan;
    let scene = Scene::over(stand, plan, &app.url(""));
    let write = Write::new(&["site-tools-over"]).await;
    let (handle, mut prepared) = scene
        .try_prepare_in(&common::site_tools_mode(), Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let steps = &scene.binding_in(operation, &common::site_tools_mode()).act;
    let stopped = scene
        .run(
            &handle,
            &mut prepared,
            steps,
            &Values::new(),
            Some(write.authority()),
        )
        .await
        .unwrap_err();
    handle.release(prepared, true).await;
    scene.stand.end().await;
    stopped
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tool_the_page_registers_a_moment_after_it_loads_is_waited_for() {
    let app = TestApp::start().await.unwrap();
    let digest = learned(
        &app,
        "example_late_tool",
        "example_page_late_call",
        &Values::new(),
    )
    .await;
    let ran = call(
        &app,
        &[("example_late_tool", &digest)],
        "example_page_late_call",
        &Values::new(),
    )
    .await
    .unwrap();
    assert_eq!(ran.values.get("saying"), Some(&Value::from("late")));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_that_may_not_act_calls_no_tool() {
    let app = TestApp::start().await.unwrap();
    let stand = Stand::new("site-tools-look", Arc::new(NoSecrets));
    let plan = plan_with(&stand, &[]);
    let scene = Scene::over(stand, plan, &app.url(""));
    let (handle, mut prepared) = scene
        .try_prepare_in(
            &common::site_tools_mode(),
            None,
            Posture::Sandbox {
                test_records: false,
            },
        )
        .await
        .unwrap();
    let steps = &scene
        .binding_in("example_page_note_save", &common::site_tools_mode())
        .act;
    let stopped = scene
        .run(
            &handle,
            &mut prepared,
            steps,
            &Values::new().with("text", "x"),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(stopped.sent, Sent::NotSent, "{stopped}");
    handle.release(prepared, true).await;
    assert!(app.case_events().await.unwrap().is_empty());
    scene.stand.end().await;
}

/// A browser that gives a page no place to register tools is not a page
/// that dropped its tool: the stop says so, with its own code, and nothing
/// is sent.
#[tokio::test(flavor = "multi_thread")]
async fn a_browser_with_no_place_for_tools_is_a_named_stop_and_not_a_missing_tool() {
    let app = TestApp::start().await.unwrap();
    let stopped = call(&app, &[], "example_page_unsupported_call", &Values::new())
        .await
        .unwrap_err();
    assert_eq!(
        (stopped.failure.class(), stopped.failure.code().as_str()),
        (FailureClass::UnknownVariant, "site_tools_unavailable"),
        "{stopped}"
    );
    assert!(
        stopped.failure.message().contains("modelContext"),
        "{stopped}"
    );
    assert_eq!(stopped.sent, Sent::NotSent);
    assert!(app.case_events().await.unwrap().is_empty());
}
