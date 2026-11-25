//! An action done by a key (decision 0999): the element's own action that
//! it binds to a key of the pack's closed list, never a synthetic key. On
//! Linux, against the stand, whose field labelled `New status` saves on
//! Enter: its one action, `activate`, is bound to Return. Every test asks
//! the stand what it stored and how many times it was pressed. On Windows
//! and macOS an action by a key stops before anything is sent.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::time::Duration;

use a2m_core::{FailureClass, Values};
use a2m_provider::{Posture, Sent, StepFailure};
use common::{Folder, Scene, Write, compile_at, fixture_copy, registry, shown};

const UPDATE: &str = "example_order_update";
const WITHIN: Duration = Duration::from_secs(10);

/// The fixture with the update's commit done by `action`, a new action of
/// the edit screen written as `lines`.
fn target_with(folder: &Folder, action: &str, lines: &str) -> std::path::PathBuf {
    let root = folder.path().join("keys");
    let lines = lines.to_owned();
    fixture_copy(&root, "screens/order_edit.yaml", move |text| {
        text.replace("actions:\n", &format!("actions:\n{lines}"))
    });
    let operations = root.join("operations").join("order_write.yaml");
    let text = std::fs::read_to_string(&operations).unwrap();
    std::fs::write(
        &operations,
        text.replacen("    action: save\n", &format!("    action: {action}\n"), 1),
    )
    .unwrap();
    root
}

const ON_ENTER: &str = "  save_on_enter:\n    locate: [{role: entry, name: \"New status\"}]\n    key: enter\n    confirm: {text: {locate: {role: entry, name: \"Message\"}, equals: \"Saved.\"}}\n    commits: true\n";

fn inputs(customer: &str, status: &str) -> Values {
    Values::new()
        .with("order_number", "A-100")
        .with("customer", customer)
        .with("status", status)
}

fn stopped(failure: &StepFailure) -> (FailureClass, &str, Sent) {
    (
        failure.failure.class(),
        failure.failure.code().as_str(),
        failure.sent,
    )
}

async fn act(scene: &Scene, values: &Values, write: Option<&Write>) -> Result<Values, StepFailure> {
    let binding = scene.binding(UPDATE);
    scene
        .steps(&binding.act, values, write, Posture::Live, WITHIN)
        .await
}

#[test]
fn an_action_by_a_key_names_one_of_the_closed_list_and_no_element_action_beside_it() {
    let folder = Folder::new("keys-compile");
    let registry = registry(folder.path(), None);
    let root = target_with(&folder, "save_on_enter", ON_ENTER);
    compile_at(&registry, &root).unwrap_or_else(|problems| panic!("{}", shown(&problems)));
    for (name, lines, code) in [
        (
            "both",
            ON_ENTER.replace(
                "    key: enter\n",
                "    key: enter\n    perform: activate\n",
            ),
            "desktop.key_and_perform",
        ),
        (
            "unknown",
            ON_ENTER.replace("key: enter", "key: f5"),
            "kernel.unknown_value",
        ),
    ] {
        let folder = Folder::new(&format!("keys-{name}"));
        let root = target_with(&folder, "save_on_enter", &lines);
        let problems = compile_at(&registry, &root).unwrap_err();
        let text = shown(&problems);
        assert!(text.contains(&format!("error {code}")), "{name}: {text}");
    }
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn an_update_saved_by_its_fields_enter_key_is_stored_once_and_read_back() {
    let folder = Folder::new("keys-enter");
    let root = target_with(&folder, "save_on_enter", ON_ENTER);
    let Some(scene) = Scene::at("keys-enter", &root).await else {
        return;
    };
    let values = inputs("Example Traders", "held");
    // Without the ledger's token, nothing is pressed.
    let refused = act(&scene, &values, None).await.unwrap_err();
    assert_eq!(
        stopped(&refused),
        (FailureClass::Policy, "intent_missing", Sent::NotSent)
    );
    assert_eq!(scene.desk().truth().writes, 0);
    // With it, the field's own action bound to Enter is done once, and the
    // application stores what its fields hold.
    let write = Write::new(UPDATE, "A-100").await;
    act(&scene, &values, Some(&write)).await.unwrap();
    let truth = scene.desk().truth();
    assert_eq!((truth.writes, truth.stores), (1, 1));
    assert_eq!(truth.asked.get("Action.DoAction"), Some(&1));
    assert_eq!(truth.stored.get("status").map(String::as_str), Some("held"));
    assert!(!truth.asked.keys().any(|member| member.contains("Key")));
    // The read-back reaches the window afresh and reads what is stored.
    let back = scene
        .steps(
            &scene.binding(UPDATE).read_back,
            &values,
            None,
            Posture::Live,
            WITHIN,
        )
        .await
        .unwrap();
    assert_eq!(
        back.get("status").and_then(a2m_core::Value::as_text),
        Some("held")
    );
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn a_key_the_field_binds_to_nothing_a_refused_value_and_a_silent_save_each_stop_by_name() {
    // A key the element binds to no action: nothing is pressed.
    let folder = Folder::new("keys-escape");
    let root = target_with(
        &folder,
        "save_on_enter",
        &ON_ENTER.replace("key: enter", "key: escape"),
    );
    let Some(scene) = Scene::at("keys-escape", &root).await else {
        return;
    };
    let write = Write::new(UPDATE, "A-100").await;
    let refused = act(&scene, &inputs("Example Traders", "held"), Some(&write))
        .await
        .unwrap_err();
    assert_eq!(
        stopped(&refused),
        (
            FailureClass::UnknownVariant,
            "unexpected_control",
            Sent::NotSent
        )
    );
    assert_eq!(scene.desk().truth().writes, 0);
    drop(scene);

    let folder = Folder::new("keys-faults");
    let root = target_with(&folder, "save_on_enter", ON_ENTER);
    let scene = Scene::at("keys-faults", &root).await.unwrap();
    // A value the application refuses: answered, nothing stored, its words
    // carried as said.
    let write = Write::new(UPDATE, "A-100").await;
    let refused = act(&scene, &inputs("Example Traders", "bogus"), Some(&write))
        .await
        .unwrap_err();
    assert_eq!(
        stopped(&refused),
        (
            FailureClass::Rejected,
            "app_validation_error",
            Sent::Answered
        )
    );
    assert_eq!(scene.desk().truth().stores, 0);
    // The key's action stores nothing and says nothing: in doubt, as a
    // press of Save would be.
    scene
        .desk()
        .fault(&example_desk::Fault::SaveSilently)
        .unwrap();
    let write = Write::new(UPDATE, "A-100").await;
    let binding = scene.binding(UPDATE);
    let doubt = scene
        .steps(
            &binding.act,
            &inputs("Example Traders", "held"),
            Some(&write),
            Posture::Live,
            Duration::from_millis(1500),
        )
        .await
        .unwrap_err();
    assert_eq!(
        stopped(&doubt),
        (
            FailureClass::PossibleCommit,
            "no_confirmation",
            Sent::Unknown
        )
    );
    assert_eq!(scene.desk().truth().stores, 0);
}

#[cfg(any(windows, target_os = "macos"))]
#[tokio::test(flavor = "multi_thread")]
async fn an_action_by_a_key_is_not_built_here_and_sends_nothing() {
    let folder = Folder::new("keys-elsewhere");
    let root = target_with(&folder, "save_on_enter", ON_ENTER);
    let Some(scene) = Scene::at("keys-elsewhere", &root).await else {
        return;
    };
    let write = Write::new(UPDATE, "A-100").await;
    let refused = act(&scene, &inputs("Example Traders", "held"), Some(&write))
        .await
        .unwrap_err();
    assert_eq!(
        stopped(&refused),
        (FailureClass::Busy, "keys_not_built", Sent::NotSent)
    );
    assert_eq!(scene.desk().truth().writes, 0);
}
