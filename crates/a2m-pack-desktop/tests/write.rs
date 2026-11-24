//! Writes through the desktop driver against the stand, on a session of
//! the test's own (Linux), through the registry's driver handle, with real
//! intent tokens from a real store. Every test asks the stand what it
//! stored, how many times Save stored, and every value set into each
//! field; none proves a write by the pack's own read alone. On every other
//! system a write stops before anything is sent.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

const UPDATE: &str = "example_order_update";

#[cfg(target_os = "linux")]
mod on_linux {
    use std::time::Duration;

    use a2m_core::{FailureClass, Value, Values};
    use a2m_provider::{Posture, Sent, StepFailure};
    use example_desk::{CUSTOMER_MOST, Fault, Order, SAVED};

    use super::UPDATE;
    use super::common::{Scene, Write, normalizer};

    const WITHIN: Duration = Duration::from_secs(10);

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

    fn stored(scene: &Scene, name: &str) -> String {
        scene.desk().truth().stored.get(name).cloned().unwrap()
    }

    /// The act of the update, with the ledger's token for its record.
    async fn act(scene: &Scene, values: &Values, within: Duration) -> Result<Values, StepFailure> {
        let write = Write::new(UPDATE, "A-100").await;
        let binding = scene.binding(UPDATE);
        scene
            .steps(&binding.act, values, Some(&write), Posture::Live, within)
            .await
    }

    /// The read-back of the update: the window reached afresh.
    async fn read_back(scene: &Scene, values: &Values) -> Values {
        let binding = scene.binding(UPDATE);
        scene
            .steps(&binding.read_back, values, None, Posture::Live, WITHIN)
            .await
            .unwrap()
    }

    /// Whether what was read back is, under each output's field type, what
    /// the write intended: the engine's question on resume.
    fn landed(scene: &Scene, back: &Values, values: &Values) -> bool {
        ["customer", "status"].iter().all(|output| {
            let rule = normalizer(&scene.compiled_target, UPDATE, output);
            rule.normalize(back.get(output).unwrap())
                .unwrap()
                .agrees_with(&rule.normalize(values.get(output).unwrap()).unwrap())
        })
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_update_is_previewed_approved_stored_once_and_read_back_afresh() {
        let Some(scene) = Scene::start("update").await else {
            return;
        };
        let order = Order::example();
        let binding = scene.binding(UPDATE).clone();
        let values = inputs("Example Traders", "held");

        // The preview reads the record as it is, in a posture that cannot
        // act, and acts on nothing.
        let before = scene
            .steps(
                &binding.read_back,
                &values,
                None,
                Posture::Sandbox {
                    test_records: false,
                },
                WITHIN,
            )
            .await
            .unwrap();
        assert_eq!(
            before.get("customer"),
            Some(&Value::from(order.customer.as_str()))
        );
        assert_eq!(
            before.get("status"),
            Some(&Value::from(order.status.as_str()))
        );
        // Nor can the act itself run there: its first `set` is refused.
        let refused = scene
            .steps(
                &binding.act,
                &values,
                None,
                Posture::Sandbox {
                    test_records: false,
                },
                WITHIN,
            )
            .await
            .unwrap_err();
        assert_eq!(
            stopped(&refused),
            (FailureClass::Policy, "read_only_posture", Sent::NotSent)
        );
        let truth = scene.desk().truth();
        assert!(truth.sets.is_empty() && truth.writes == 0, "{truth:?}");

        // Without the ledger's token, the fields are set and Save is
        // refused before the driver sees it: nothing is stored.
        let refused = scene
            .steps(&binding.act, &values, None, Posture::Live, WITHIN)
            .await
            .unwrap_err();
        assert_eq!(
            stopped(&refused),
            (FailureClass::Policy, "intent_missing", Sent::NotSent)
        );
        let truth = scene.desk().truth();
        assert_eq!((truth.writes, truth.stores), (0, 0));
        // What was typed is not what is stored.
        assert_eq!(stored(&scene, "customer"), order.customer);

        // Approved, with the token: set, saved once, confirmed.
        act(&scene, &values, WITHIN).await.unwrap();
        let truth = scene.desk().truth();
        assert_eq!((truth.writes, truth.stores), (1, 1));
        assert_eq!(stored(&scene, "customer"), "Example Traders");
        assert_eq!(stored(&scene, "status"), "held");
        assert_eq!(truth.message, SAVED);
        let last: Vec<(&str, &str)> = truth.sets[truth.sets.len() - 2..]
            .iter()
            .map(|set| (set.field.as_str(), set.value.as_str()))
            .collect();
        assert_eq!(
            last,
            [("New customer", "Example Traders"), ("New status", "held")]
        );
        assert!(
            truth
                .sets
                .iter()
                .all(|set| set.window == example_desk::WINDOW)
        );

        // The read-back reaches the window afresh and reads what is
        // stored, which the field type compares with what was intended.
        let back = read_back(&scene, &values).await;
        assert_eq!(back.get("customer"), Some(&Value::from("Example Traders")));
        assert!(landed(&scene, &back, &values));
        assert!(!landed(&scene, &back, &inputs("Example Traders", "open")));
        // The read-back pressed nothing.
        assert_eq!(scene.desk().truth().writes, 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_write_in_doubt_is_reconciled_by_reading_before_writing_again() {
        // Save stores, then the application exits before it confirms.
        let Some(mut scene) = Scene::start("exits").await else {
            return;
        };
        let values = inputs("Example Traders", "held");
        scene.desk().fault(&Fault::ExitAfterSave).unwrap();
        let doubt = act(&scene, &values, WITHIN).await.unwrap_err();
        assert_eq!(
            stopped(&doubt),
            (
                FailureClass::PossibleCommit,
                "no_confirmation",
                Sent::Unknown
            )
        );
        assert_eq!(scene.desk().truth().stores, 1);
        // The application is started again; the resume reads the stored
        // value first, finds the write landed, and does not press Save
        // again.
        scene.desk.as_mut().unwrap().restart().await.unwrap();
        let back = read_back(&scene, &values).await;
        assert!(landed(&scene, &back, &values), "{back:?}");
        let truth = scene.desk().truth();
        assert_eq!((truth.writes, truth.stores), (1, 1));

        // Save stores nothing and says nothing.
        let Some(scene) = Scene::start("silent").await else {
            return;
        };
        scene.desk().fault(&Fault::SaveSilently).unwrap();
        let doubt = act(&scene, &values, Duration::from_millis(1500))
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
        // The fields hold the new values; the window, reached afresh,
        // shows what is stored, which is not them: the resume writes once.
        let back = read_back(&scene, &values).await;
        assert!(!landed(&scene, &back, &values), "{back:?}");
        act(&scene, &values, WITHIN).await.unwrap();
        let truth = scene.desk().truth();
        assert_eq!((truth.writes, truth.stores), (2, 1));
        assert!(landed(&scene, &read_back(&scene, &values).await, &values));
    }

    /// A field the application shows disabled is not set: the driver reads
    /// its enabled state before the set, and stops before anything is sent
    /// to it. The stand also refuses a set of a disabled field, so a set
    /// that was sent would have come back as an answered failure, not an
    /// unsent one.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_disabled_field_is_refused_before_anything_is_set_into_it() {
        let Some(scene) = Scene::start("disabled").await else {
            return;
        };
        scene
            .desk()
            .fault(&Fault::Disabled("New status".to_owned()))
            .unwrap();
        let refused = act(&scene, &inputs("Example Traders", "held"), WITHIN)
            .await
            .unwrap_err();
        assert_eq!(
            stopped(&refused),
            (
                FailureClass::UnknownVariant,
                "field_read_only",
                Sent::NotSent
            )
        );
        assert!(
            refused.failure.message().contains("disabled"),
            "{}",
            refused.failure.message()
        );
        let truth = scene.desk().truth();
        assert_eq!((truth.writes, truth.stores), (0, 0));
        assert!(
            truth.sets.iter().all(|set| set.field != "New status"),
            "{:?}",
            truth.sets
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_late_confirmation_ends_the_wait_by_its_event() {
        let Some(scene) = Scene::start("late-save").await else {
            return;
        };
        scene
            .desk()
            .fault(&Fault::ConfirmLate(Duration::from_millis(400)))
            .unwrap();
        act(&scene, &inputs("Example Traders", "held"), WITHIN)
            .await
            .unwrap();
        let truth = scene.desk().truth();
        assert_eq!((truth.writes, truth.stores), (1, 1));
        assert_eq!(truth.message, SAVED);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn each_stop_says_whether_anything_was_sent_and_commits_nothing() {
        let Some(scene) = Scene::start("stops").await else {
            return;
        };
        let order = Order::example();

        // The application cuts a long value: the run stops before Save.
        let long = "Example Supplies and Sons, Limited";
        assert!(long.chars().count() > CUSTOMER_MOST);
        let cut = act(&scene, &inputs(long, "held"), WITHIN)
            .await
            .unwrap_err();
        assert_eq!(
            stopped(&cut),
            (
                FailureClass::UnknownVariant,
                "value_not_held",
                Sent::Answered
            )
        );
        assert_eq!(scene.desk().truth().writes, 0);

        // The application refuses a value: its words are carried as what
        // it said, apart from the product's own, and nothing is stored.
        let refused = act(&scene, &inputs("Example Traders", "lost"), WITHIN)
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
        let said = refused.failure.said().unwrap();
        assert!(said.contains("Status must be"), "{said}");
        assert!(!refused.failure.message().contains(said));
        let truth = scene.desk().truth();
        assert_eq!((truth.writes, truth.stores), (1, 0));
        assert_eq!(stored(&scene, "customer"), order.customer);

        // A field that is read-only today: refused before anything is set
        // into it.
        let sets = scene.desk().truth().sets.len();
        scene
            .desk()
            .fault(&Fault::ReadOnly("New status".to_owned()))
            .unwrap();
        let locked = act(&scene, &inputs("Example Traders", "held"), WITHIN)
            .await
            .unwrap_err();
        assert_eq!(
            stopped(&locked),
            (
                FailureClass::UnknownVariant,
                "field_read_only",
                Sent::NotSent
            )
        );
        let truth = scene.desk().truth();
        assert_eq!((truth.writes, truth.stores), (1, 0));
        assert!(
            truth.sets[sets..]
                .iter()
                .all(|set| set.field != "New status"),
            "{:?}",
            truth.sets
        );
        // The window shows another record than the one the run is for.
        let other = act(
            &scene,
            &inputs("Example Traders", "held").with("order_number", "B-200"),
            WITHIN,
        )
        .await
        .unwrap_err();
        assert_eq!(
            stopped(&other),
            (
                FailureClass::UnknownVariant,
                "unexpected_page",
                Sent::NotSent
            )
        );
        assert_eq!(scene.desk().truth().writes, 1);
        assert_eq!(scene.desk().truth().secret_reads, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_second_window_with_the_same_field_is_never_written() {
        // Of the same title: no window can be told apart, and nothing is
        // set anywhere.
        let Some(scene) = Scene::start("twin").await else {
            return;
        };
        scene
            .desk()
            .open_window(example_desk::WINDOW)
            .await
            .unwrap();
        let twice = act(&scene, &inputs("Example Traders", "held"), WITHIN)
            .await
            .unwrap_err();
        assert_eq!(
            stopped(&twice),
            (FailureClass::Drift, "locator_ambiguous", Sent::NotSent)
        );
        let truth = scene.desk().truth();
        assert!(truth.sets.is_empty() && truth.writes == 0, "{truth:?}");

        // Of another title, which no screen names: no act is done while it
        // is open, in either window (decision 0244). Before, the write went
        // to the screen's window only; an unknown window may be a dialog
        // that changes what the screen's window means.
        let Some(scene) = Scene::start("copy").await else {
            return;
        };
        scene
            .desk()
            .open_window("Example order (copy)")
            .await
            .unwrap();
        let other = act(&scene, &inputs("Example Traders", "held"), WITHIN)
            .await
            .unwrap_err();
        assert_eq!(
            stopped(&other),
            (
                FailureClass::UnknownVariant,
                "unexpected_page",
                Sent::NotSent
            )
        );
        assert_eq!(other.failure.said(), Some("Example order (copy)"));
        let truth = scene.desk().truth();
        assert!(truth.sets.is_empty() && truth.writes == 0, "{truth:?}");
    }

    /// Between the `wait` that proved the record and the act, the window
    /// moves to another record, or the application raises a dialog no
    /// screen names: each act proves both again, on the walk it acts on,
    /// and stops before anything is set or pressed.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_act_never_lands_on_another_record_or_under_an_unknown_dialog() {
        let Some(scene) = Scene::start("moved").await else {
            return;
        };
        scene
            .desk()
            .fault(&Fault::AnotherRecordAfterRead("B-200".to_owned()))
            .unwrap();
        let moved = act(&scene, &inputs("Example Traders", "held"), WITHIN)
            .await
            .unwrap_err();
        assert_eq!(
            stopped(&moved),
            (
                FailureClass::UnknownVariant,
                "unexpected_page",
                Sent::NotSent
            ),
            "{}",
            moved.failure
        );
        let truth = scene.desk().truth();
        assert!(truth.sets.is_empty() && truth.writes == 0, "{truth:?}");

        let Some(scene) = Scene::start("dialog").await else {
            return;
        };
        scene.desk().fault(&Fault::DialogAfterRead).unwrap();
        let dialog = act(&scene, &inputs("Example Traders", "held"), WITHIN)
            .await
            .unwrap_err();
        assert_eq!(
            stopped(&dialog),
            (
                FailureClass::UnknownVariant,
                "unexpected_page",
                Sent::NotSent
            ),
            "{}",
            dialog.failure
        );
        assert!(
            dialog.failure.message().contains("no screen"),
            "{}",
            dialog.failure
        );
        let truth = scene.desk().truth();
        assert!(truth.sets.is_empty() && truth.writes == 0, "{truth:?}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn two_writes_at_once_do_not_interleave_their_sets() {
        let Some(scene) = Scene::start("together").await else {
            return;
        };
        let first = inputs("Alpha Traders", "held");
        let second = inputs("Beta Traders", "closed");
        let (one, two) = tokio::join!(act(&scene, &first, WITHIN), act(&scene, &second, WITHIN));
        one.unwrap();
        two.unwrap();
        let truth = scene.desk().truth();
        assert_eq!((truth.writes, truth.stores), (2, 2));
        let sets: Vec<&str> = truth.sets.iter().map(|set| set.value.as_str()).collect();
        assert!(
            sets == ["Alpha Traders", "held", "Beta Traders", "closed"]
                || sets == ["Beta Traders", "closed", "Alpha Traders", "held"],
            "{sets:?}"
        );
    }
}

/// On every system but Linux, a write stops before anything is sent.
#[cfg(not(target_os = "linux"))]
#[tokio::test(flavor = "multi_thread")]
async fn a_write_stops_before_anything_is_sent_where_it_is_not_built() {
    use a2m_core::{FailureClass, Values};
    use a2m_provider::{Posture, Sent};

    let Some(scene) = common::Scene::start("not-built").await else {
        return;
    };
    let binding = scene.binding(UPDATE);
    let values = Values::new()
        .with("order_number", "A-100")
        .with("customer", "Example Traders")
        .with("status", "held");
    let write = common::Write::new(UPDATE, "A-100").await;
    let failure = scene
        .steps(
            &binding.act,
            &values,
            Some(&write),
            Posture::Live,
            std::time::Duration::from_secs(10),
        )
        .await
        .unwrap_err();
    assert_eq!(
        (failure.failure.class(), failure.failure.code().as_str()),
        (FailureClass::Busy, "writes_not_built")
    );
    assert_eq!(failure.sent, Sent::NotSent);
    assert_eq!(scene.desk().truth().writes, 0);
}
