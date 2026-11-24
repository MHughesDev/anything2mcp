//! The input commit a platform profile states, applied by the driver's set
//! step on a surface that names the profile, against the test application's
//! pages that commit a value as each family of web interface does, through
//! the screens of `tests/fixtures/commits` (decision 0385). Every proof asks
//! the application what its page was told or was sent, never the driver.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use a2m_core::{FailureClass, NoSecrets, Values};
use a2m_provider::{DriverHandle, Posture, Prepared, Sent, StepFailure};
use common::{Scene, Stand, Write, compile_changed_files};
use example_app::harness::TestApp;

/// The `commits` fixture with its surface naming `profile`, or none.
async fn scene(name: &str, app: &TestApp, profile: Option<&str>) -> Scene {
    let stand = Stand::new(name, Arc::new(NoSecrets));
    let target = |text: &str| match profile {
        Some(named) => text.replacen(
            "    kind: web\n",
            &format!("    kind: web\n    profile: {named}\n"),
            1,
        ),
        None => text.to_owned(),
    };
    let compiled_target =
        compile_changed_files(&stand.registry, "commits", &[("target.yaml", &target)])
            .unwrap_or_else(|problems| panic!("{}", common::shown(&problems)))
            .compiled_target;
    Scene::over(stand, compiled_target, &app.url(""))
}

/// What the application was told, once it has been told at least `count`
/// things: a page's tell is a request of its own and may land a moment after
/// the step ends.
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

/// Runs the `act` steps of an operation's binding, as a write: all of them
/// when `saving`, and otherwise those before the action that saves, so that
/// nothing but the set step's own commit can tell the page of the value.
async fn write(
    scene: &Scene,
    operation: &str,
    inputs: &Values,
    saving: bool,
) -> (Result<common::Ran, StepFailure>, DriverHandle, Prepared) {
    let write = Write::new(&["commits"]).await;
    let (handle, mut prepared) = scene
        .try_prepare(Some(write.fence), Posture::Live)
        .await
        .unwrap();
    let mut steps = scene.binding(operation).act.as_slice();
    if !saving {
        let (save, before) = steps.split_last().unwrap();
        assert!(save.commits, "the last step is the action that saves");
        steps = before;
    }
    let ran = scene
        .run(
            &handle,
            &mut prepared,
            steps,
            inputs,
            Some(write.authority()),
        )
        .await;
    (ran, handle, prepared)
}

/// The strategy the set step says held.
fn strategy(ran: &common::Ran) -> String {
    let means = ran.means();
    means
        .iter()
        .flatten()
        .find(|detail| detail.name == "strategy")
        .map(|detail| detail.value.clone())
        .unwrap_or_default()
}

/// A page the server draws whole: under `plain`, the value is put in and
/// the control loses the focus, and the page is told of the change once,
/// with the value, before anything is saved.
#[tokio::test(flavor = "multi_thread")]
async fn a_plain_page_is_told_of_a_change_when_the_control_loses_the_focus() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("commit-plain", &app, Some("plain")).await;
    let (ran, handle, prepared) = write(
        &scene,
        "example_reference_change",
        &Values::new().with("reference", "R-200"),
        false,
    )
    .await;
    let ran = ran.unwrap();
    assert!(handle.release(prepared, false).await.complete);
    assert_eq!(strategy(&ran), "blur");
    // Nothing was saved: the page heard of the value from its control
    // losing the focus, and from nothing else.
    assert_eq!(told(&app, 1).await, ["changed: R-200"]);
    scene.stand.end().await;
}

/// An input whose value is kept in a component's state: under
/// `controlled_inputs`, the value is typed, so the page's own key handlers
/// see it, and nothing is put in any other way first; the page keeps it
/// and saves it.
#[tokio::test(flavor = "multi_thread")]
async fn a_controlled_input_takes_the_value_typed_and_nothing_set_before_it() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("commit-controlled", &app, Some("controlled_inputs")).await;
    let (ran, handle, prepared) = write(
        &scene,
        "example_reference_save",
        &Values::new().with("reference", "R-300"),
        true,
    )
    .await;
    let ran = ran.unwrap();
    assert!(handle.release(prepared, false).await.complete);
    assert_eq!(strategy(&ran), "type_then_blur");
    // No `ignored` event: nothing was set without key presses first, as the
    // rule of no profile would have done.
    assert_eq!(told(&app, 1).await, ["reference: R-300"]);
    scene.stand.end().await;
}

/// The same input under a profile whose commit sets the value without key
/// presses: the page ignores it, and the step stops by name with the fate
/// of the input unknown, never trying another way after it.
#[tokio::test(flavor = "multi_thread")]
async fn a_controlled_input_under_the_wrong_profile_is_a_named_stop() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("commit-wrong", &app, Some("plain")).await;
    let (ran, handle, prepared) = write(
        &scene,
        "example_reference_save",
        &Values::new().with("reference", "R-400"),
        true,
    )
    .await;
    let failure = ran.unwrap_err();
    handle.release(prepared, false).await;
    assert_eq!(
        (failure.failure.class(), failure.failure.code().as_str()),
        (FailureClass::UnknownVariant, "unexpected_control"),
        "{failure:?}"
    );
    assert_eq!(failure.sent, Sent::Unknown);
    assert!(failure.failure.message().contains("blur"), "{failure:?}");
    // The page was told once that it ignored a value, and saved nothing.
    assert_eq!(told(&app, 1).await, ["ignored: not typed"]);
    scene.stand.end().await;
}

/// A page of a server-rendered forms framework: under `aspnet_webforms`,
/// the change is posted back, the page draws the form again, and the value
/// is read back from the control drawn anew. The application received the
/// value once.
#[tokio::test(flavor = "multi_thread")]
async fn a_post_back_page_receives_the_change_and_the_value_is_read_from_the_form_drawn_again() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("commit-postback", &app, Some("aspnet_webforms")).await;
    let (ran, handle, prepared) = write(
        &scene,
        "example_quantity_post",
        &Values::new().with("quantity", "12"),
        false,
    )
    .await;
    let ran = ran.unwrap();
    assert!(handle.release(prepared, false).await.complete);
    assert_eq!(strategy(&ran), "blur_then_wait_ready");
    assert_eq!(told(&app, 1).await, ["posted: 12"]);
    scene.stand.end().await;
}

/// A post-back that never draws the form again is a named stop, with the
/// fate of the value unknown: the change was sent, and nothing says what
/// the page made of it (rule 7).
#[tokio::test(flavor = "multi_thread")]
async fn a_post_back_that_never_redraws_is_a_named_stop_with_its_fate_unknown() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("commit-stuck", &app, Some("aspnet_webforms")).await;
    let (ran, handle, prepared) = write(
        &scene,
        "example_quantity_stuck",
        &Values::new().with("quantity", "13"),
        false,
    )
    .await;
    let failure = ran.unwrap_err();
    handle.release(prepared, false).await;
    assert_eq!(
        (failure.failure.class(), failure.failure.code().as_str()),
        (FailureClass::UnknownVariant, "commit_not_redrawn"),
        "{failure:?}"
    );
    assert_eq!(failure.sent, Sent::Unknown);
    // The application did receive it: the stop is not a claim it did not.
    assert_eq!(told(&app, 1).await, ["posted: 13"]);
    scene.stand.end().await;
}

/// `auto` is settled by the probe's detection, which is not built: a
/// surface of `auto` sets a text control as one that names no profile.
#[tokio::test(flavor = "multi_thread")]
async fn a_surface_of_auto_sets_a_text_control_by_the_rule_of_no_profile() {
    let app = TestApp::start().await.unwrap();
    let scene = scene("commit-auto", &app, Some("auto")).await;
    let (ran, handle, prepared) = write(
        &scene,
        "example_reference_save",
        &Values::new().with("reference", "R-500"),
        true,
    )
    .await;
    let ran = ran.unwrap();
    assert!(handle.release(prepared, false).await.complete);
    assert_eq!(strategy(&ran), "type");
    assert_eq!(
        told(&app, 2).await,
        ["ignored: not typed", "reference: R-500"]
    );
    scene.stand.end().await;
}
