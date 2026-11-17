//! The stream driver against the test broker, through the driver handle.
//! What a write did is asked of the broker's control interface, never of
//! the pack's own read.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::time::Duration;

use a2m_core::{REDACTED, Value, Values};
use a2m_provider::{Authority, Posture, Sent, StepOutcome};
use common::{
    Ledger, PLANTED, Scene, done, failed, input, read, record, signature_of, signature_with, text,
};
use example_broker::client::Publish;
use example_broker::faults::{Fault, FaultKind};
use example_broker::model::{ChannelSpec, DeliveryOutcome, Op, PublishOutcome, Shape, body_digest};
use serde_json::json;

const STATUS: &str = "example.orders.status";
const EVENTS: &str = "example.orders.events";
const WORK: &str = "example.orders.work";
const NOTICES: &str = "example.orders.notices";

fn key_arg(order: &str) -> Values {
    Values::new().with("key", order)
}

fn status_args(order: &str, status: &str) -> Values {
    Values::new()
        .with("key", order)
        .with("header.correlation_id", order)
        .with("body.order_id", order)
        .with("body.status", status)
}

/// The requests the broker received that were not the opening `hello` or a
/// `describe`.
fn ops(scene: &Scene) -> Vec<Op> {
    scene
        .broker
        .requests()
        .into_iter()
        .map(|request| request.op)
        .filter(|op| !matches!(op, Op::Hello | Op::Describe))
        .collect()
}

// ---- reads ----

#[tokio::test]
async fn a_retained_key_reads_the_same_five_times_and_takes_nothing() {
    let scene = Scene::start().await;
    let steps = &scene.binding("example_order_status_get").act;
    let mut plain = Vec::new();
    let mut acted = Vec::new();
    let mut results = Vec::new();
    let mut means = Vec::new();
    for _ in 0..5 {
        let (handle, mut prepared) = scene.reading().await;
        let StepOutcome::Done(observed) = read(&handle, &mut prepared, steps, &key_arg("A-100"))
            .await
            .unwrap()
        else {
            panic!("the value is held");
        };
        // How the read was done goes to the trace, and its `means` are in
        // the signature: the same five times.
        let acting = observed.acting().expect("how the step was done");
        means.push(acting.means.clone());
        plain.push(signature_of(&steps[0].kind, &observed.values));
        acted.push(signature_with(
            &steps[0].kind,
            &observed.values,
            Some(acting),
        ));
        results.push(observed.values);
        assert!(handle.release(prepared, false).await.complete);
    }
    assert!(plain.windows(2).all(|pair| pair[0] == pair[1]));
    assert!(acted.windows(2).all(|pair| pair[0] == pair[1]));
    assert_ne!(
        plain[0], acted[0],
        "the signature covers how the step was done"
    );
    assert!(means.windows(2).all(|pair| pair[0] == pair[1]));
    let said: Vec<(&str, &str)> = means[0]
        .iter()
        .map(|detail| (detail.name.as_str(), detail.value.as_str()))
        .collect();
    assert_eq!(
        said,
        [("protocol", "example-broker/1"), ("shape_check", "made")]
    );
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(text(&results[0], "status"), "draft");
    assert_eq!(text(&results[0], "order_id"), "A-100");
    assert_eq!(text(record(&results[0], "message_meta"), "id"), "s-1");

    // A key the broker holds no value for is absent, not a failure.
    let (handle, mut prepared) = scene.reading().await;
    let absent = read(&handle, &mut prepared, steps, &key_arg("A-999"))
        .await
        .unwrap();
    assert!(matches!(absent, StepOutcome::Absent(_)));

    // The broker: nothing delivered, nothing published, only looks; and
    // the five runs of one session shared one connection.
    assert!(scene.broker.deliveries().is_empty());
    assert!(scene.broker.received().is_empty());
    assert!(ops(&scene).iter().all(|op| *op == Op::Latest));
    let connections: std::collections::BTreeSet<String> = scene
        .broker
        .requests()
        .into_iter()
        .map(|request| request.connection)
        .collect();
    assert_eq!(connections.len(), 1, "{connections:?}");
}

#[tokio::test]
async fn a_list_is_one_bounded_look_with_its_coverage() {
    let scene = Scene::start().await;
    let (handle, mut prepared) = scene.reading().await;
    let none = Values::new();

    let status = done(
        read(
            &handle,
            &mut prepared,
            &scene.binding("example_order_status_list").act,
            &none,
        )
        .await
        .unwrap(),
    );
    assert_eq!(status.get("count"), Some(&Value::Int(2)));
    assert_eq!(text(&status, "coverage"), "complete");

    // The log's channel file allows two messages a look: the first look
    // says it is partial and where the next begins.
    let events = &scene.binding("example_order_events_list").act;
    let first = done(
        read(
            &handle,
            &mut prepared,
            events,
            &Values::new().with("from", 0_i64),
        )
        .await
        .unwrap(),
    );
    assert_eq!(first.get("count"), Some(&Value::Int(2)));
    assert_eq!(first.get("more"), Some(&Value::Bool(true)));
    assert_eq!(text(&first, "coverage"), "partial");
    assert_eq!(first.get("next"), Some(&Value::Int(2)));
    let rest = done(
        read(
            &handle,
            &mut prepared,
            events,
            &Values::new().with("from", 2_i64),
        )
        .await
        .unwrap(),
    );
    assert_eq!(rest.get("count"), Some(&Value::Int(1)));
    assert_eq!(text(&rest, "coverage"), "complete");
    let last = rest.get("messages").and_then(Value::as_list).unwrap()[0]
        .as_record()
        .unwrap();
    assert_eq!(text(last, "event"), "submitted");

    // A position that is no whole number never reaches the broker.
    let before = scene.broker.requests().len();
    let refused = failed(
        read(
            &handle,
            &mut prepared,
            events,
            &Values::new().with("from", "0; drop"),
        )
        .await,
    );
    assert_eq!(
        refused,
        (
            "input".to_owned(),
            "value_out_of_range".to_owned(),
            Sent::NotSent
        )
    );
    assert_eq!(scene.broker.requests().len(), before);

    // A queue is looked at, and every message stays where it was.
    let work = done(
        read(
            &handle,
            &mut prepared,
            &scene.binding("example_order_work_list").act,
            &none,
        )
        .await
        .unwrap(),
    );
    assert_eq!(work.get("count"), Some(&Value::Int(2)));
    assert!(scene.broker.deliveries().is_empty());
    assert_eq!(scene.broker.queue(WORK).len(), 2);
    assert!(ops(&scene).iter().all(|op| *op == Op::Browse));
}

#[tokio::test]
async fn a_wait_ends_with_a_match_or_with_nothing_and_says_which() {
    let scene = Scene::start().await;
    let wait = &scene.binding("example_order_event_await").act;

    // From a position in the log: the first event of the order after it.
    let (handle, mut prepared) = scene.reading().await;
    let args = Values::new().with("key", "A-101").with("from", 0_i64);
    let found = done(read(&handle, &mut prepared, wait, &args).await.unwrap());
    assert_eq!(found.get("arrived"), Some(&Value::Bool(true)));
    let message = record(&found, "message");
    assert_eq!(text(message, "event"), "created");
    assert_eq!(
        record(message, "message_meta").get("position"),
        Some(&Value::Int(1))
    );
    handle.release(prepared, false).await;

    // From the end of the log, with nothing published: an answer.
    let (handle, mut prepared) = scene.reading().await;
    let args = Values::new().with("key", "A-100").with("from", 3_i64);
    let nothing = done(read(&handle, &mut prepared, wait, &args).await.unwrap());
    assert_eq!(nothing.get("arrived"), Some(&Value::Bool(false)));
    assert_eq!(nothing.get("waited_ms"), Some(&Value::Int(1_000)));
    assert_eq!(text(&nothing, "coverage"), "complete");
    handle.release(prepared, false).await;

    // An operation that needs a message stops, by name.
    let (handle, mut prepared) = scene.reading().await;
    let needed = &scene.binding("example_order_event_needed").act;
    let stopped = failed(read(&handle, &mut prepared, needed, &key_arg("A-100")).await);
    assert_eq!(
        (stopped.0.as_str(), stopped.1.as_str()),
        ("transient", "nothing_arrived")
    );
    handle.release(prepared, false).await;

    // Nothing was taken from anyone, and no subscription outlives its run.
    assert!(
        scene
            .broker
            .deliveries()
            .iter()
            .all(|delivery| delivery.outcome == DeliveryOutcome::NotRequired)
    );
    let truth = scene
        .broker
        .wait_for(Duration::from_secs(10), |truth| {
            truth.channels[EVENTS]
                .subscriptions
                .iter()
                .all(|subscription| subscription.held_by.is_none())
        })
        .await
        .unwrap();
    assert_eq!(truth.channels[EVENTS].log.len(), 3);
}

#[tokio::test]
async fn a_wait_finds_what_arrives_through_the_runtimes_own_subscription() {
    let scene = Scene::start().await;
    let wait = &scene.binding("example_order_status_await").act;
    let (handle, mut prepared) = scene.reading().await;
    let (subscribe, await_) = (&wait[0], &wait[1]);
    let args = key_arg("A-100");
    handle
        .act(&mut prepared, subscribe, &input(&args), Authority::Read)
        .await
        .unwrap();

    // Another run cannot hold the same subscription: busy, by name.
    let (other, mut second) = scene.reading().await;
    let busy = failed(
        other
            .act(&mut second, subscribe, &input(&args), Authority::Read)
            .await,
    );
    assert_eq!(
        (busy.0.as_str(), busy.1.as_str(), busy.2),
        ("busy", "subscription_in_use", Sent::Answered)
    );
    other.release(second, false).await;

    // Someone else publishes two statuses; the wait gives the one whose
    // key matches, and not a value retained before it subscribed.
    let mut outside = scene.broker.client("outside").await.unwrap();
    for (order, status) in [("A-101", "shipped"), ("A-100", "submitted")] {
        outside
            .publish(&Publish::new(STATUS, json!({"order_id": order, "status": status})).key(order))
            .await
            .unwrap();
    }
    let found = done(
        handle
            .act(&mut prepared, await_, &input(&args), Authority::Read)
            .await
            .unwrap(),
    );
    assert_eq!(found.get("arrived"), Some(&Value::Bool(true)));
    assert_eq!(text(record(&found, "message"), "status"), "submitted");
    assert_eq!(found.get("examined"), Some(&Value::Int(2)));
    assert!(handle.release(prepared, false).await.complete);
    scene
        .broker
        .wait_for(Duration::from_secs(10), |truth| {
            truth.channels[STATUS]
                .subscriptions
                .iter()
                .all(|subscription| subscription.held_by.is_none())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn a_message_delivered_twice_is_returned_once() {
    let scene = Scene::start().await;
    let wait = &scene.binding("example_order_event_await").act;
    let (handle, mut prepared) = scene.reading().await;
    let args = Values::new().with("key", "A-101").with("from", 1_i64);
    scene
        .broker
        .set_fault(Fault::once(FaultKind::DuplicateDelivery).on_channel(EVENTS));
    let first = done(read(&handle, &mut prepared, wait, &args).await.unwrap());
    assert_eq!(text(record(&first, "message"), "event"), "created");
    // The broker hands `created` over again. The second wait of the same
    // run passes it by and gives the next event.
    let second = done(
        handle
            .act(&mut prepared, &wait[1], &input(&args), Authority::Read)
            .await
            .unwrap(),
    );
    assert_eq!(text(record(&second, "message"), "event"), "submitted");
    assert_eq!(second.get("examined"), Some(&Value::Int(2)));
    assert_eq!(scene.broker.applied_faults().len(), 1);
}

// ---- writes ----

#[tokio::test]
async fn a_publish_to_a_state_channel_is_one_message_and_reads_back() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let binding = scene.binding("example_order_status_set");
    let write = ledger.write("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let args = status_args("A-100", "shipped");

    // Without the record's intent token nothing is sent.
    let refused = failed(
        handle
            .act(
                &mut prepared,
                &binding.act[0],
                &write.input(&args, None),
                Authority::Read,
            )
            .await,
    );
    assert_eq!((refused.0.as_str(), refused.2), ("policy", Sent::NotSent));
    assert!(scene.broker.received().is_empty());

    let sent = done(
        handle
            .act(
                &mut prepared,
                &binding.act[0],
                &write.input(&args, Some("tok-1")),
                write.authority(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(text(&sent, "message_id"), "m-1");

    // The broker: exactly one publish, of exactly the intended message.
    let received = scene.broker.received();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].client, "anything2mcp");
    assert_eq!(received[0].key.as_deref(), Some("A-100"));
    assert_eq!(
        received[0].body_digest,
        body_digest(&json!({"order_id": "A-100", "status": "shipped"}))
    );
    assert_eq!(received[0].headers["correlation_id"], "A-100");
    assert!(matches!(
        received[0].outcome,
        PublishOutcome::Stored { retained: true, .. }
    ));
    assert_eq!(
        scene.broker.retained(STATUS, "A-100").unwrap().body["status"],
        "shipped"
    );

    // The read-back gives what the broker holds for the key.
    let back = done(
        read(&handle, &mut prepared, &binding.read_back, &args)
            .await
            .unwrap(),
    );
    assert_eq!(text(&back, "status"), "shipped");

    // Another producer changes the key: the read-back shows the other
    // value, which the engine compares as a mismatch.
    let mut outside = scene.broker.client("outside").await.unwrap();
    outside
        .publish(
            &Publish::new(STATUS, json!({"order_id": "A-100", "status": "draft"})).key("A-100"),
        )
        .await
        .unwrap();
    let changed = done(
        read(&handle, &mut prepared, &binding.read_back, &args)
            .await
            .unwrap(),
    );
    assert_eq!(text(&changed, "status"), "draft");
    assert_eq!(scene.broker.received_len_by("anything2mcp"), 1);
}

trait ReceivedBy {
    fn received_len_by(&self, client: &str) -> usize;
}

impl ReceivedBy for example_broker::harness::TestBroker {
    fn received_len_by(&self, client: &str) -> usize {
        self.received()
            .iter()
            .filter(|received| received.client == client)
            .count()
    }
}

#[tokio::test]
async fn a_message_that_does_not_fit_is_never_sent() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let write = ledger.write("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();

    // A status the schema does not list.
    let args = status_args("A-100", "lost");
    let publish = &scene.binding("example_order_status_set").act[0];
    let invalid = failed(
        handle
            .act(
                &mut prepared,
                publish,
                &write.input(&args, None),
                write.authority(),
            )
            .await,
    );
    assert_eq!(
        invalid,
        (
            "input".to_owned(),
            "schema_invalid".to_owned(),
            Sent::NotSent
        )
    );

    // A body over the channel's limit.
    let long = "x".repeat(70_000);
    let args = Values::new()
        .with("header.correlation_id", "A-100")
        .with("body.order_id", "A-100")
        .with("body.text", long);
    let notice = &scene.binding("example_order_notice_send").act[0];
    let large = failed(
        handle
            .act(
                &mut prepared,
                notice,
                &write.input(&args, None),
                write.authority(),
            )
            .await,
    );
    assert_eq!(
        large,
        (
            "input".to_owned(),
            "message_too_large".to_owned(),
            Sent::NotSent
        )
    );

    // A moment has no declared form in a JSON message.
    let args = Values::new()
        .with("header.correlation_id", "A-100")
        .with("body.order_id", "A-100")
        .with("body.text", a2m_core::Timestamp::from_unix_millis(5));
    let odd = failed(
        handle
            .act(
                &mut prepared,
                notice,
                &write.input(&args, None),
                write.authority(),
            )
            .await,
    );
    assert_eq!(
        odd,
        (
            "input".to_owned(),
            "value_out_of_range".to_owned(),
            Sent::NotSent
        )
    );
    assert!(scene.broker.received().is_empty());
}

#[tokio::test]
async fn a_lost_confirmation_is_a_possible_commit_and_never_a_second_message() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let binding = scene.binding("example_order_status_set");
    let args = status_args("A-100", "shipped");

    // Stored, then the connection drops before the confirmation.
    scene
        .broker
        .set_fault(Fault::once(FaultKind::StoreThenDrop));
    let write = ledger.write("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let lost = failed(
        handle
            .act(
                &mut prepared,
                &binding.act[0],
                &write.input(&args, None),
                write.authority(),
            )
            .await,
    );
    assert_eq!(
        lost,
        (
            "possible_commit".to_owned(),
            "no_confirmation".to_owned(),
            Sent::Unknown
        )
    );
    // The driver sent it once and did not send it again.
    let received = scene.broker.received();
    assert_eq!(received.len(), 1);
    assert!(!received[0].answered);
    assert!(handle.release(prepared, true).await.complete);

    // Reconcile is the read-back on its own, in a new run: the intended
    // value is there, so the publish landed and nothing is sent again.
    let (handle, mut prepared) = scene.reading().await;
    let back = done(
        read(&handle, &mut prepared, &binding.read_back, &args)
            .await
            .unwrap(),
    );
    assert_eq!(text(&back, "status"), "shipped");
    assert_eq!(scene.broker.received().len(), 1);
    handle.release(prepared, false).await;

    // The same sight with nothing stored: the connection drops first. It
    // is still a possible commit; reconcile shows the value seen before.
    scene
        .broker
        .set_fault(Fault::once(FaultKind::DropBeforeStore));
    let write = ledger.write("A-101").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let args = status_args("A-101", "shipped");
    let dropped = failed(
        handle
            .act(
                &mut prepared,
                &binding.act[0],
                &write.input(&args, None),
                write.authority(),
            )
            .await,
    );
    assert_eq!(dropped.0, "possible_commit");
    handle.release(prepared, true).await;
    let (handle, mut prepared) = scene.reading().await;
    let back = done(
        read(&handle, &mut prepared, &binding.read_back, &args)
            .await
            .unwrap(),
    );
    assert_eq!(text(&back, "status"), "submitted");
    handle.release(prepared, false).await;

    // Stored, and no confirmation ever comes: the step's own time limit
    // ends the wait, and it is the same possible commit.
    scene
        .broker
        .set_fault(Fault::once(FaultKind::StoreThenSilent));
    let write = ledger.write("A-100 again").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let args = status_args("A-100", "draft");
    let mut short = write.input(&args, None);
    short.deadline = a2m_provider::Deadline::after(Duration::from_secs(1));
    let silent = failed(
        handle
            .act(&mut prepared, &binding.act[0], &short, write.authority())
            .await,
    );
    assert_eq!(
        silent,
        (
            "possible_commit".to_owned(),
            "no_confirmation".to_owned(),
            Sent::Unknown
        )
    );
    // A later step of the same run finds its connection given up.
    let after = failed(read(&handle, &mut prepared, &binding.read_back, &args).await);
    assert_eq!(
        (after.1.as_str(), after.2),
        ("connection_lost", Sent::NotSent)
    );
    assert_eq!(scene.broker.received().len(), 3);
}

#[tokio::test]
async fn a_resumed_publish_with_the_records_token_delivers_one_message() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let publish = &scene.binding("example_order_work_add").act[0];
    let args = Values::new()
        .with("key", "A-102")
        .with("body.order_id", "A-102")
        .with("body.task", "ship");

    let write = ledger.write("A-102").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let first = done(
        handle
            .act(
                &mut prepared,
                publish,
                &write.input(&args, Some("tok-7")),
                write.authority(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(first.get("duplicate"), Some(&Value::Bool(false)));
    handle.release(prepared, false).await;

    // The run is resumed and sends the same record again, with its token.
    // (The test's ledger keeps the first run's lock on the record, as a
    // real one would until reconcile; the second token is under a name of
    // its own.)
    let again = ledger.write("A-102 resumed").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(again.fence))
        .await
        .unwrap();
    let second = done(
        handle
            .act(
                &mut prepared,
                publish,
                &again.input(&args, Some("tok-7")),
                again.authority(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(second.get("duplicate"), Some(&Value::Bool(true)));
    assert_eq!(second.get("message_id"), first.get("message_id"));

    // The broker: two publishes received, one message stored.
    let received = scene.broker.received();
    assert_eq!(received.len(), 2);
    assert_eq!(received[0].headers["idempotency-token"], "tok-7");
    assert!(matches!(received[0].outcome, PublishOutcome::Stored { .. }));
    assert!(matches!(
        received[1].outcome,
        PublishOutcome::Duplicate { .. }
    ));
    assert_eq!(scene.broker.queue(WORK).len(), 3);
}

#[tokio::test]
async fn a_publish_to_a_log_is_found_again_by_its_token() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let binding = scene.binding("example_order_event_record");
    let args = Values::new()
        .with("key", "A-100")
        .with("body.order_id", "A-100")
        .with("body.event", "shipped");
    let write = ledger.write("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let sent = done(
        handle
            .act(
                &mut prepared,
                &binding.act[1],
                &write.input(&args, Some("tok-9")),
                write.authority(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(sent.get("position"), Some(&Value::Int(3)));
    let log = scene.broker.log(EVENTS);
    assert_eq!(log.len(), 4);
    assert_eq!(log[3].headers["idempotency-token"], "tok-9");

    // Read back in the same run: the message at the confirmed position.
    let mut stamped = input(&args);
    stamped.stamp = Some("tok-9");
    let back = done(
        handle
            .act(
                &mut prepared,
                &binding.read_back[0],
                &stamped,
                Authority::Read,
            )
            .await
            .unwrap(),
    );
    assert_eq!(text(&back, "event"), "shipped");
    handle.release(prepared, false).await;

    // Reconcile in a new run: a scan of the log, two messages a look,
    // finds the token; a token that was never sent is absent.
    let (handle, mut prepared) = scene.reading().await;
    let found = handle
        .act(&mut prepared, &binding.lookup[0], &stamped, Authority::Read)
        .await
        .unwrap();
    assert_eq!(
        record(&done(found), "message_meta").get("position"),
        Some(&Value::Int(3))
    );
    let mut other = input(&args);
    other.stamp = Some("tok-never");
    let absent = handle
        .act(&mut prepared, &binding.lookup[0], &other, Authority::Read)
        .await
        .unwrap();
    assert!(matches!(absent, StepOutcome::Absent(_)));
    assert_eq!(scene.broker.received().len(), 1);
    assert!(scene.broker.deliveries().is_empty());
}

#[tokio::test]
async fn a_refusal_is_the_brokers_own_and_a_topic_keeps_nothing() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let notice = &scene.binding("example_order_notice_send").act[0];
    let args = Values::new()
        .with("header.correlation_id", "A-100")
        .with("body.order_id", "A-100")
        .with("body.text", "Ready");
    let write = ledger.write("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();

    scene.broker.set_fault(Fault::once(FaultKind::Refuse {
        reason: format!("The quota is used up, says {PLANTED}."),
    }));
    let refused = handle
        .act(
            &mut prepared,
            notice,
            &write.input(&args, None),
            write.authority(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        (
            refused.failure.class().to_string(),
            refused.failure.code().as_str(),
            refused.sent
        ),
        ("rejected".to_owned(), "broker_refused", Sent::Answered)
    );
    // The broker's words are its own, apart from the sentence, which is the
    // product's; neither holds the secret.
    let said = refused.failure.said().expect("what the broker said");
    assert!(said.contains("The quota is used up"), "{said}");
    assert!(!said.contains(PLANTED));
    assert!(!refused.failure.message().contains("The quota is used up"));
    assert!(!refused.failure.message().contains(PLANTED));

    // A secret that lies across the place where the broker's words are cut
    // is redacted whole: no part of it is left at the cut.
    scene.broker.set_fault(Fault::once(FaultKind::Refuse {
        reason: format!("{}{PLANTED} and more", "x".repeat(110)),
    }));
    let cut = handle
        .act(
            &mut prepared,
            notice,
            &write.input(&args, None),
            write.authority(),
        )
        .await
        .unwrap_err();
    assert_eq!(cut.failure.code().as_str(), "broker_refused");
    let said = cut.failure.said().expect("what the broker said");
    assert!(!said.contains(&PLANTED[..6]), "{said}");
    assert!(said.contains(REDACTED), "{said}");
    assert!(!cut.failure.message().contains(&PLANTED[..6]));

    // Sent again by the caller's choice: one copy to whoever listens.
    let mut listener = scene.broker.client("outside").await.unwrap();
    let subscribed = listener.subscribe(NOTICES).await.unwrap();
    done(
        handle
            .act(
                &mut prepared,
                notice,
                &write.input(&args, None),
                write.authority(),
            )
            .await
            .unwrap(),
    );
    let heard = listener
        .fetch(&subscribed.subscription, 5, Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(heard.len(), 1);
    let received = scene.broker.received();
    assert!(matches!(
        received[0].outcome,
        PublishOutcome::Refused { .. }
    ));
    assert!(matches!(
        received[1].outcome,
        PublishOutcome::Refused { .. }
    ));
    assert!(matches!(received[2].outcome, PublishOutcome::Stored { .. }));
}

#[tokio::test]
async fn a_take_records_then_acknowledges_on_the_runtimes_own_queue() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let take = &scene.binding("example_order_work_take").act;
    let none = Values::new();

    // Taken and not acknowledged: the broker gives the message back when
    // the run lets go, and nothing is lost.
    let (handle, mut prepared) = scene.reading().await;
    let StepOutcome::Done(taken) = read(&handle, &mut prepared, &take[..2], &none)
        .await
        .unwrap()
    else {
        panic!("a message");
    };
    assert_eq!(text(record(&taken.values, "message"), "task"), "pick");
    assert_eq!(taken.assigned_key, Some(common::key("s-6")));
    assert_eq!(text(&taken.for_reconcile, "message_id"), "s-6");
    let unsent = failed(
        handle
            .act(&mut prepared, &take[2], &input(&none), Authority::Read)
            .await,
    );
    assert_eq!((unsent.0.as_str(), unsent.2), ("policy", Sent::NotSent));
    handle.release(prepared, false).await;
    assert!(scene.broker.acknowledgements().is_empty());
    assert_eq!(scene.broker.queue(WORK).len(), 2);

    // With the record's token: a message is taken again and acknowledged
    // once. The broker says which message is gone.
    let write = ledger.write("taken").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let StepOutcome::Done(taken) = read(&handle, &mut prepared, &take[..2], &none)
        .await
        .unwrap()
    else {
        panic!("a message");
    };
    let id = text(
        record(record(&taken.values, "message"), "message_meta"),
        "id",
    )
    .to_owned();
    assert_eq!(taken.assigned_key, Some(common::key(&id)));
    let acknowledged = done(
        handle
            .act(
                &mut prepared,
                &take[2],
                &write.input(&none, None),
                write.authority(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(text(&acknowledged, "message_id"), id);
    handle.release(prepared, false).await;
    let acknowledgements = scene.broker.acknowledgements();
    assert_eq!(acknowledgements.len(), 1);
    assert_eq!(acknowledgements[0].message, id);
    let left = scene.broker.queue(WORK);
    assert_eq!(left.len(), 1);
    assert_ne!(left[0].message.id, id);
}

// ---- faults ----

#[tokio::test]
async fn a_system_that_changed_is_named_and_never_guessed_at() {
    let scene = Scene::start().await;
    let steps = &scene.binding("example_order_status_get").act;
    let args = key_arg("A-100");

    // A field of the message was renamed.
    scene.broker.set_fault(Fault::once(FaultKind::RenameField {
        from: "status".to_owned(),
        to: "state".to_owned(),
    }));
    let (handle, mut prepared) = scene.reading().await;
    let renamed = failed(read(&handle, &mut prepared, steps, &args).await);
    assert_eq!(
        renamed,
        (
            "drift".to_owned(),
            "response_schema_mismatch".to_owned(),
            Sent::Answered
        )
    );

    // An answer outside the protocol.
    scene
        .broker
        .set_fault(Fault::once(FaultKind::UnexpectedReply).on_op(Op::Latest));
    let odd = failed(read(&handle, &mut prepared, steps, &args).await);
    assert_eq!(
        (odd.0.as_str(), odd.1.as_str()),
        ("unknown_variant", "unexpected_reply")
    );
    handle.release(prepared, true).await;

    // The channel became a log.
    scene
        .broker
        .declare_channel(ChannelSpec::new(STATUS, Shape::Log).key_field("order_id"));
    let (handle, mut prepared) = scene.reading().await;
    let reshaped = failed(read(&handle, &mut prepared, steps, &args).await);
    assert_eq!(
        (reshaped.0.as_str(), reshaped.1.as_str()),
        ("drift", "shape_changed")
    );
    handle.release(prepared, false).await;

    // The channel is gone.
    assert!(scene.broker.remove_channel(STATUS));
    let (handle, mut prepared) = scene.reading().await;
    let missing = failed(read(&handle, &mut prepared, steps, &args).await);
    assert_eq!(
        (missing.0.as_str(), missing.1.as_str()),
        ("drift", "channel_missing")
    );
    assert!(scene.broker.received().is_empty());
}

#[tokio::test]
async fn a_broker_that_cannot_be_reached_or_refuses_the_session_is_named() {
    let mut scene = Scene::start().await;
    let steps = scene.binding("example_order_status_get").act.clone();
    let args = key_arg("A-100");

    // The broker stops accepting what it accepted: the next request.
    let (handle, mut prepared) = scene.reading().await;
    scene.broker.set_required_token(Some("another"));
    let expired = failed(read(&handle, &mut prepared, &steps, &args).await);
    assert_eq!(
        (expired.0.as_str(), expired.1.as_str()),
        ("session_expired", "needs_login")
    );
    handle.release(prepared, true).await;
    // And refuses a new connection that presents nothing.
    let refused = scene.try_prepare(Posture::Live, None).await.unwrap_err();
    assert_eq!(refused.failure.code().as_str(), "needs_login");
    scene.broker.set_required_token(None);

    // Every connection drops, as when the broker restarts: a read says so
    // and sends nothing on a connection it knows has ended.
    let (handle, mut prepared) = scene.reading().await;
    scene.broker.drop_connections();
    scene
        .broker
        .wait_for(Duration::from_secs(10), |truth| {
            truth.connections.is_empty()
        })
        .await
        .unwrap();
    let lost = failed(read(&handle, &mut prepared, &steps, &args).await);
    assert_eq!(
        (lost.0.as_str(), lost.1.as_str()),
        ("transient", "connection_lost")
    );
    handle.release(prepared, true).await;

    // An address the run was not given is never dialled.
    let allowed = std::mem::take(&mut scene.destinations);
    let outside = scene.try_prepare(Posture::Live, None).await.unwrap_err();
    assert_eq!(
        (outside.failure.code().as_str(), outside.sent),
        ("surface_not_declared", Sent::NotSent)
    );
    scene.destinations = allowed;

    // Nothing listens at the address.
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().to_string()
    };
    scene.surface.surface.address = closed.clone();
    scene.destinations = a2m_provider::Destinations::new([closed]);
    let down = scene.try_prepare(Posture::Live, None).await.unwrap_err();
    assert_eq!(
        (
            down.failure.class().to_string(),
            down.failure.code().as_str()
        ),
        ("transient".to_owned(), "upstream_unavailable")
    );
}

#[tokio::test]
async fn a_secret_in_what_the_broker_says_reaches_no_result() {
    let scene = Scene::start().await;
    let mut outside = scene.broker.client("outside").await.unwrap();
    outside
        .publish(
            &Publish::new(WORK, json!({"order_id": "A-102", "task": PLANTED}))
                .key("A-102")
                .header("note", PLANTED),
        )
        .await
        .unwrap();
    let (handle, mut prepared) = scene.reading().await;
    let listed = done(
        read(
            &handle,
            &mut prepared,
            &scene.binding("example_order_work_list").act,
            &Values::new(),
        )
        .await
        .unwrap(),
    );
    let shown = format!("{listed:?}");
    assert!(!shown.contains(PLANTED), "{shown}");
    assert!(shown.contains(REDACTED));
    // The trace the engine would keep of it holds none either.
    signature_of(
        &scene.binding("example_order_work_list").act[0].kind,
        &listed,
    );
}

#[tokio::test]
async fn where_nothing_may_be_written_nothing_is_sent_or_taken() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let write = ledger.write("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Discovery, Some(write.fence))
        .await
        .unwrap();
    let args = status_args("A-100", "shipped");
    let publish = failed(
        handle
            .act(
                &mut prepared,
                &scene.binding("example_order_status_set").act[0],
                &write.input(&args, None),
                write.authority(),
            )
            .await,
    );
    assert_eq!(
        (publish.0.as_str(), publish.1.as_str(), publish.2),
        ("policy", "read_only_posture", Sent::NotSent)
    );
    let subscribe = failed(
        handle
            .act(
                &mut prepared,
                &scene.binding("example_order_status_await").act[0],
                &input(&args),
                Authority::Read,
            )
            .await,
    );
    assert_eq!(
        (subscribe.0.as_str(), subscribe.1.as_str(), subscribe.2),
        ("policy", "read_only_posture", Sent::NotSent)
    );
    // A look is still a look.
    done(
        read(
            &handle,
            &mut prepared,
            &scene.binding("example_order_status_get").act,
            &args,
        )
        .await
        .unwrap(),
    );
    assert!(scene.broker.received().is_empty());
    assert!(scene.broker.deliveries().is_empty());
    assert!(ops(&scene).iter().all(|op| *op == Op::Latest));
}

// ---- the audit of decision 0150 ----

/// What the engine does with the values a step gives for reconcile: it
/// stores them, in this form, before the committing step is sent, and
/// reads them back in a run that is resumed.
fn stored_and_read_again(for_reconcile: &Values) -> Values {
    let stored = a2m_core::CanonicalJson::encode(for_reconcile).unwrap();
    stored.decode().unwrap()
}

fn in_doubt() -> (String, String, Sent) {
    (
        "possible_commit".to_owned(),
        "no_confirmation".to_owned(),
        Sent::Unknown,
    )
}

#[tokio::test]
async fn a_take_whose_acknowledgement_is_lost_has_its_message_in_the_record() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let take = &scene.binding("example_order_work_take").act;
    let none = Values::new();
    let first = scene.broker.queue(WORK)[0].message.clone();

    // The broker acknowledges, and the answer never arrives.
    scene.broker.set_fault(Fault::once(FaultKind::AckThenDrop));
    let write = ledger.write("taken").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let StepOutcome::Done(taken) = read(&handle, &mut prepared, &take[..2], &none)
        .await
        .unwrap()
    else {
        panic!("a message");
    };
    // Before the acknowledgement is sent, the engine holds the message.
    let ledger_row = stored_and_read_again(&taken.for_reconcile);
    let lost = failed(
        handle
            .act(
                &mut prepared,
                &take[2],
                &write.input(&none, None),
                write.authority(),
            )
            .await,
    );
    assert_eq!(lost, in_doubt());
    handle.release(prepared, true).await;

    // The broker: the message was acknowledged and is gone.
    let acknowledgements = scene.broker.acknowledgements();
    assert_eq!(acknowledgements.len(), 1);
    assert_eq!(acknowledgements[0].message, first.id);
    let left = scene.broker.queue(WORK);
    assert_eq!(left.len(), 1);
    assert_ne!(left[0].message.id, first.id);

    // The resume: the record is in doubt and has no identity, so no step is
    // sent for it. What it returns is what the ledger holds, which is the
    // message the broker gave up, whole.
    assert_eq!(text(&ledger_row, "message_id"), first.id);
    let message = record(&ledger_row, "message");
    assert_eq!(text(message, "order_id"), first.body["order_id"]);
    assert_eq!(text(message, "task"), first.body["task"]);
    assert_eq!(text(record(message, "message_meta"), "id"), first.id);
    assert_eq!(
        record(message, "message_meta").get("key"),
        first.key.as_deref().map(Value::from).as_ref()
    );
    // And no other message was taken: one delivery, and the queue as it was.
    assert_eq!(scene.broker.deliveries().len(), 1);
    assert_eq!(scene.broker.queue(WORK).len(), 1);
}

#[tokio::test]
async fn a_message_too_large_to_record_is_given_back_and_never_acknowledged() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let take = &scene.binding("example_order_work_take").act;
    let none = Values::new();
    // The queue holds one message, larger than the channel file allows.
    // It still refuses a repeated token in the header the file declares.
    let mut spec = ChannelSpec::new(WORK, Shape::Queue)
        .key_field("order_id")
        .dedupe_header(example_broker::SEED_DEDUPE_HEADER);
    spec.max_body_bytes = 200_000;
    scene.broker.declare_channel(spec);
    let mut outside = scene.broker.client("outside").await.unwrap();
    outside
        .publish(
            &Publish::new(
                WORK,
                json!({"order_id": "A-102", "task": "x".repeat(70_000)}),
            )
            .key("A-102"),
        )
        .await
        .unwrap();

    let write = ledger.write("taken").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let large = failed(read(&handle, &mut prepared, &take[..2], &none).await);
    assert_eq!(
        large,
        (
            "policy".to_owned(),
            "take_too_large".to_owned(),
            Sent::Answered
        )
    );
    // Nothing was taken, so nothing is there to acknowledge.
    let nothing = failed(
        handle
            .act(
                &mut prepared,
                &take[2],
                &write.input(&none, None),
                write.authority(),
            )
            .await,
    );
    assert_eq!(nothing.2, Sent::NotSent);
    handle.release(prepared, true).await;
    assert!(scene.broker.acknowledgements().is_empty());
    let truth = scene
        .broker
        .wait_for(Duration::from_secs(10), |truth| {
            truth.channels[WORK]
                .queue
                .iter()
                .all(|held| held.held_by.is_none())
        })
        .await
        .unwrap();
    assert_eq!(truth.channels[WORK].queue.len(), 1);
}

#[tokio::test]
async fn a_scan_that_did_not_cover_the_log_never_says_absent() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let binding = scene.binding("example_order_event_record");
    let args = Values::new()
        .with("key", "A-100")
        .with("body.order_id", "A-100")
        .with("body.event", "shipped");

    // A publish lands, and its confirmation is lost.
    scene
        .broker
        .set_fault(Fault::once(FaultKind::StoreThenDrop));
    let write = ledger.write("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let lost = failed(
        handle
            .act(
                &mut prepared,
                &binding.act[1],
                &write.input(&args, Some("tok-lost")),
                write.authority(),
            )
            .await,
    );
    assert_eq!(lost, in_doubt());
    handle.release(prepared, true).await;

    // The resume looks the record up. The broker's first look says the log
    // holds more and gives no place to go on from: the scan has not covered
    // the log, and says so; it never says the message is not there.
    scene
        .broker
        .set_fault(Fault::once(FaultKind::EmptyLook { more: true }).on_channel(EVENTS));
    let mut stamped = input(&args);
    stamped.stamp = Some("tok-lost");
    let (handle, mut prepared) = scene.reading().await;
    let uncovered = handle
        .act(&mut prepared, &binding.lookup[0], &stamped, Authority::Read)
        .await;
    assert!(
        !matches!(uncovered, Ok(StepOutcome::Absent(_))),
        "an uncovered log was reported as not holding the message"
    );
    let uncovered = failed(uncovered);
    assert_eq!(
        (uncovered.0.as_str(), uncovered.1.as_str()),
        ("unknown_variant", "unexpected_reply")
    );
    handle.release(prepared, true).await;
    // The same when a look says that was all, and the log's own end says
    // otherwise.
    scene
        .broker
        .set_fault(Fault::once(FaultKind::EmptyLook { more: false }).on_channel(EVENTS));
    let (handle, mut prepared) = scene.reading().await;
    let contradicted = failed(
        handle
            .act(&mut prepared, &binding.lookup[0], &stamped, Authority::Read)
            .await,
    );
    assert_eq!(
        (contradicted.0.as_str(), contradicted.1.as_str()),
        ("unknown_variant", "unexpected_reply")
    );
    handle.release(prepared, true).await;

    // The broker holds the message, once. A look that covers the log finds
    // it, and nothing was published again.
    let held: Vec<_> = scene
        .broker
        .log(EVENTS)
        .into_iter()
        .filter(|message| {
            message.headers.get("idempotency-token").map(String::as_str) == Some("tok-lost")
        })
        .collect();
    assert_eq!(held.len(), 1);
    let (handle, mut prepared) = scene.reading().await;
    let found = done(
        handle
            .act(&mut prepared, &binding.lookup[0], &stamped, Authority::Read)
            .await
            .unwrap(),
    );
    assert_eq!(text(&found, "event"), "shipped");
    assert_eq!(scene.broker.received().len(), 1);
}

#[tokio::test]
async fn a_refusal_the_protocol_does_not_define_leaves_a_write_in_doubt() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let refuse = |code: &str| code.to_owned();

    // A publish is stored, and the broker answers with a code of no rule.
    scene
        .broker
        .set_fault(Fault::once(FaultKind::StoreThenRefuse {
            code: refuse("replica_timeout"),
        }));
    let write = ledger.write("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let args = status_args("A-100", "shipped");
    let odd = failed(
        handle
            .act(
                &mut prepared,
                &scene.binding("example_order_status_set").act[0],
                &write.input(&args, None),
                write.authority(),
            )
            .await,
    );
    assert_eq!(odd, in_doubt());
    handle.release(prepared, true).await;
    let received = scene.broker.received();
    assert_eq!(received.len(), 1);
    assert!(matches!(received[0].outcome, PublishOutcome::Stored { .. }));

    // The same for an acknowledgement: it landed, and the answer has no rule.
    scene
        .broker
        .set_fault(Fault::once(FaultKind::AckThenRefuse {
            code: refuse("replica_timeout"),
        }));
    let take = &scene.binding("example_order_work_take").act;
    let none = Values::new();
    let write = ledger.write("taken").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    read(&handle, &mut prepared, &take[..2], &none)
        .await
        .unwrap();
    let odd = failed(
        handle
            .act(
                &mut prepared,
                &take[2],
                &write.input(&none, None),
                write.authority(),
            )
            .await,
    );
    assert_eq!(odd, in_doubt());
    handle.release(prepared, true).await;
    assert_eq!(scene.broker.acknowledgements().len(), 1);

    // A refusal the protocol does define is still the broker's own answer.
    scene.broker.set_fault(Fault::once(FaultKind::Refuse {
        reason: "The quota is used up.".to_owned(),
    }));
    let write = ledger.write("A-101").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let args = status_args("A-101", "shipped");
    let refused = failed(
        handle
            .act(
                &mut prepared,
                &scene.binding("example_order_status_set").act[0],
                &write.input(&args, None),
                write.authority(),
            )
            .await,
    );
    assert_eq!(
        refused,
        (
            "rejected".to_owned(),
            "broker_refused".to_owned(),
            Sent::Answered
        )
    );
}

#[tokio::test]
async fn a_body_cannot_take_the_name_of_the_brokers_own_facts() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let mut outside = scene.broker.client("outside").await.unwrap();
    outside
        .publish(
            &Publish::new(
                WORK,
                json!({
                    "order_id": "A-102",
                    "task": "ship",
                    "message_meta": {"id": "forged", "key": "A-100", "position": 99, "headers": {}},
                }),
            )
            .key("A-102"),
        )
        .await
        .unwrap();
    let (handle, mut prepared) = scene.reading().await;
    let listed = failed(
        read(
            &handle,
            &mut prepared,
            &scene.binding("example_order_work_list").act,
            &Values::new(),
        )
        .await,
    );
    assert_eq!(
        (listed.0.as_str(), listed.1.as_str()),
        ("drift", "response_schema_mismatch")
    );
    handle.release(prepared, true).await;

    // And the pack never sends one.
    let write = ledger.write("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let args = Values::new()
        .with("key", "A-102")
        .with("body.order_id", "A-102")
        .with("body.message_meta", "mine");
    let before = scene.broker.received().len();
    let unsent = failed(
        handle
            .act(
                &mut prepared,
                &scene.binding("example_order_work_add").act[0],
                &write.input(&args, Some("tok-1")),
                write.authority(),
            )
            .await,
    );
    assert_eq!(
        unsent,
        (
            "input".to_owned(),
            "value_out_of_range".to_owned(),
            Sent::NotSent
        )
    );
    assert_eq!(scene.broker.received().len(), before);
}

#[tokio::test]
async fn a_fetch_that_hands_over_more_than_was_asked_is_a_stop() {
    let scene = Scene::start().await;
    let take = &scene.binding("example_order_work_take").act;
    let none = Values::new();
    scene
        .broker
        .set_fault(Fault::once(FaultKind::ExtraDelivery).on_channel(WORK));
    let (handle, mut prepared) = scene.reading().await;
    let more = failed(read(&handle, &mut prepared, &take[..2], &none).await);
    assert_eq!(
        (more.0.as_str(), more.1.as_str()),
        ("unknown_variant", "unexpected_reply")
    );
    handle.release(prepared, true).await;
    // Both messages were handed over, and both are back: the driver let
    // the connection go, and neither was looked at or acknowledged.
    assert_eq!(scene.broker.deliveries().len(), 2);
    let truth = scene
        .broker
        .wait_for(Duration::from_secs(10), |truth| {
            truth.channels[WORK]
                .queue
                .iter()
                .all(|held| held.held_by.is_none())
        })
        .await
        .unwrap();
    assert_eq!(truth.channels[WORK].queue.len(), 2);
    assert!(scene.broker.acknowledgements().is_empty());
}

#[tokio::test]
async fn a_wait_that_saw_nothing_says_what_it_did_not_cover() {
    let scene = Scene::start().await;
    // Through the runtime's named subscription on a channel with no
    // positions: what was published before the wait began is not covered.
    let wait = &scene.binding("example_order_status_await").act;
    let (handle, mut prepared) = scene.reading().await;
    let nothing = done(
        read(&handle, &mut prepared, wait, &key_arg("A-100"))
            .await
            .unwrap(),
    );
    assert_eq!(nothing.get("arrived"), Some(&Value::Bool(false)));
    assert_eq!(text(&nothing, "coverage"), "partial");
    assert_eq!(text(&nothing, "limitation"), "starts_at_subscription");
    handle.release(prepared, false).await;
}

#[tokio::test]
async fn only_the_headers_a_channel_declares_reach_a_caller() {
    let scene = Scene::start().await;
    let mut outside = scene.broker.client("outside").await.unwrap();
    outside
        .publish(
            &Publish::new(STATUS, json!({"order_id": "A-100", "status": "shipped"}))
                .key("A-100")
                .header("correlation_id", "r-1")
                .header("authorization", "example-token-of-another-producer"),
        )
        .await
        .unwrap();
    let (handle, mut prepared) = scene.reading().await;
    let read_ = done(
        read(
            &handle,
            &mut prepared,
            &scene.binding("example_order_status_get").act,
            &key_arg("A-100"),
        )
        .await
        .unwrap(),
    );
    let headers = record(record(&read_, "message_meta"), "headers");
    assert_eq!(text(headers, "correlation_id"), "r-1");
    assert_eq!(headers.len(), 1, "{headers:?}");
    assert!(!format!("{read_:?}").contains("example-token-of-another-producer"));
}

#[tokio::test]
async fn a_connection_is_not_kept_after_a_failure() {
    let scene = Scene::start().await;
    let steps = &scene.binding("example_order_status_get").act;
    let args = key_arg("A-100");
    scene.broker.set_fault(Fault::once(FaultKind::RenameField {
        from: "status".to_owned(),
        to: "state".to_owned(),
    }));
    let (handle, mut prepared) = scene.reading().await;
    let renamed = failed(read(&handle, &mut prepared, steps, &args).await);
    assert_eq!(renamed.2, Sent::Answered);
    // The engine did not mark the run's context as spoiled; the driver
    // gives the connection up all the same.
    handle.release(prepared, false).await;
    let (handle, mut prepared) = scene.reading().await;
    done(read(&handle, &mut prepared, steps, &args).await.unwrap());
    handle.release(prepared, false).await;
    let connections: std::collections::BTreeSet<String> = scene
        .broker
        .requests()
        .into_iter()
        .map(|request| request.connection)
        .collect();
    assert_eq!(connections.len(), 2, "{connections:?}");
}

// ---- the guard in a sandbox with test records ----

#[tokio::test]
async fn a_sandbox_with_test_records_lifts_the_guard_only_for_the_step_that_commits() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let test_records = Posture::Sandbox { test_records: true };
    let none = Values::new();

    // A hand-built step that changes the queue and is not marked `commits`:
    // attaching a consumer to take from it. Refused by the driver, and the
    // broker shows nothing taken.
    let take = &scene.binding("example_order_work_take").act;
    assert!(take[0].kind.as_str() == "subscribe" && !take[0].commits);
    let write = ledger.write("taken").await;
    let (handle, mut prepared) = scene
        .try_prepare(test_records, Some(write.fence))
        .await
        .unwrap();
    let refused = failed(
        handle
            .act(&mut prepared, &take[0], &input(&none), Authority::Read)
            .await,
    );
    assert_eq!(
        (refused.0.as_str(), refused.2),
        ("policy", Sent::NotSent),
        "{refused:?}"
    );
    handle.release(prepared, false).await;
    assert!(scene.broker.deliveries().is_empty());
    assert!(scene.broker.acknowledgements().is_empty());
    assert_eq!(scene.broker.queue(WORK).len(), 2);

    // A publish whose mark is taken off is refused before it reaches the
    // driver, and the broker received nothing.
    let binding = scene.binding("example_order_status_set");
    let mut unmarked = binding.act[0].clone();
    assert!(unmarked.commits);
    unmarked.commits = false;
    let write = ledger.write("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(test_records, Some(write.fence))
        .await
        .unwrap();
    let args = status_args("A-100", "shipped");
    let refused = failed(
        handle
            .act(
                &mut prepared,
                &unmarked,
                &write.input(&args, Some("tok-1")),
                write.authority(),
            )
            .await,
    );
    assert_eq!(refused.2, Sent::NotSent, "{refused:?}");
    assert!(scene.broker.received().is_empty());

    // The step marked `commits` is the one the guard is lifted for.
    done(
        handle
            .act(
                &mut prepared,
                &binding.act[0],
                &write.input(&args, Some("tok-1")),
                write.authority(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(scene.broker.received().len(), 1);
}

// ---- what earlier steps recorded (decision 0162, point 1) ----

/// How a step was done, as `(name, value)` pairs of its `means`.
fn means_of(outcome: &StepOutcome) -> Vec<(String, String)> {
    let (StepOutcome::Done(observed) | StepOutcome::Absent(observed)) = outcome;
    observed
        .acting()
        .expect("how the step was done")
        .means
        .into_iter()
        .map(|detail| (detail.name, detail.value))
        .collect()
}

fn mean<'a>(means: &'a [(String, String)], name: &str) -> Option<&'a str> {
    means
        .iter()
        .find(|(found, _)| found == name)
        .map(|(_, value)| value.as_str())
}

#[tokio::test]
async fn a_take_in_doubt_is_settled_by_a_look_at_the_queue_for_the_message_it_recorded() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let binding = scene.binding("example_order_work_take");
    let (take, look) = (&binding.act, &binding.read_back[0]);
    let none = Values::new();
    let first = scene.broker.queue(WORK)[0].message.clone();

    // The broker acknowledges, and the answer never arrives.
    scene.broker.set_fault(Fault::once(FaultKind::AckThenDrop));
    let write = ledger.write("taken").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let StepOutcome::Done(taken) = read(&handle, &mut prepared, &take[..2], &none)
        .await
        .unwrap()
    else {
        panic!("a message");
    };
    let ledger_row = stored_and_read_again(&taken.for_reconcile);
    let lost = failed(
        handle
            .act(
                &mut prepared,
                &take[2],
                &write.keyless(&none),
                write.authority(),
            )
            .await,
    );
    assert_eq!(lost, in_doubt());
    handle.release(prepared, true).await;

    // The resume is handed what the record's earlier steps recorded. The
    // look finds the message gone from the queue the runtime owns: the
    // acknowledgement landed, and the record is found.
    let before = scene.broker.requests().len();
    let (handle, mut prepared) = scene.reading().await;
    let settled = handle
        .act_recorded(
            &mut prepared,
            look,
            &write.keyless(&none),
            Authority::Read,
            &ledger_row,
        )
        .await
        .unwrap();
    assert_eq!(mean(&means_of(&settled), "look"), Some("whole_queue"));
    let settled = done(settled);
    assert_eq!(settled.get("acknowledged"), Some(&Value::Bool(true)));
    assert_eq!(text(&settled, "message_id"), first.id);
    handle.release(prepared, false).await;
    // The broker: the look took nothing and acknowledged nothing.
    let looked: Vec<Op> = scene.broker.requests()[before..]
        .iter()
        .map(|request| request.op)
        .filter(|op| !matches!(op, Op::Hello))
        .collect();
    assert_eq!(looked, [Op::Describe, Op::Browse]);
    assert_eq!(scene.broker.acknowledgements().len(), 1);
    assert_eq!(scene.broker.deliveries().len(), 1);

    // A take whose run ended before its acknowledgement left: the message
    // went back to the queue, and the look says the record is absent.
    let again = ledger.write("taken again").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(again.fence))
        .await
        .unwrap();
    let StepOutcome::Done(held) = read(&handle, &mut prepared, &take[..2], &none)
        .await
        .unwrap()
    else {
        panic!("a message");
    };
    let row = stored_and_read_again(&held.for_reconcile);
    handle.release(prepared, true).await;
    scene
        .broker
        .wait_for(Duration::from_secs(10), |truth| {
            truth.channels[WORK]
                .queue
                .iter()
                .all(|held| held.held_by.is_none())
        })
        .await
        .unwrap();
    let (handle, mut prepared) = scene.reading().await;
    let absent = handle
        .act_recorded(
            &mut prepared,
            look,
            &again.keyless(&none),
            Authority::Read,
            &row,
        )
        .await
        .unwrap();
    assert!(matches!(absent, StepOutcome::Absent(_)), "{absent:?}");
    // A look that does not cover the queue never says the message is gone.
    scene
        .broker
        .set_fault(Fault::once(FaultKind::EmptyLook { more: false }).on_channel(WORK));
    let uncovered = failed(
        handle
            .act_recorded(
                &mut prepared,
                look,
                &again.keyless(&none),
                Authority::Read,
                &row,
            )
            .await,
    );
    assert_eq!(
        (uncovered.0.as_str(), uncovered.1.as_str()),
        ("unknown_variant", "unexpected_reply")
    );
    handle.release(prepared, true).await;
    assert_eq!(scene.broker.acknowledgements().len(), 1);
    assert!(
        scene
            .broker
            .queue(WORK)
            .iter()
            .any(|held| held.message.id == text(&row, "message_id"))
    );

    // Handed nothing recorded, the look has nothing to look for and sends
    // nothing.
    let before = scene.broker.requests().len();
    let (handle, mut prepared) = scene.reading().await;
    let nothing = failed(
        handle
            .act(&mut prepared, look, &again.keyless(&none), Authority::Read)
            .await,
    );
    assert_eq!(
        nothing,
        (
            "input".to_owned(),
            "nothing_recorded".to_owned(),
            Sent::NotSent
        )
    );
    handle.release(prepared, true).await;
    assert!(
        scene.broker.requests()[before..]
            .iter()
            .all(|request| request.op == Op::Hello)
    );
}

#[tokio::test]
async fn a_publish_in_doubt_is_found_by_a_scan_from_where_the_log_ended_before_it() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let binding = scene.binding("example_order_event_record");
    let args = Values::new()
        .with("key", "A-100")
        .with("body.order_id", "A-100")
        .with("body.event", "shipped");
    // Others have written to the log: it holds seven messages, positions 0
    // to 6, and a scan from its start reads them two at a time.
    let mut outside = scene.broker.client("outside").await.unwrap();
    for n in 0..4 {
        outside
            .publish(
                &Publish::new(
                    EVENTS,
                    json!({"order_id": "A-102", "event": format!("e{n}")}),
                )
                .key("A-102"),
            )
            .await
            .unwrap();
    }

    // The position is read and recorded before the publish; the publish
    // lands, and its confirmation is lost.
    let write = ledger.write("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let step = write.input(&args, Some("tok-lost"));
    let StepOutcome::Done(marked) = handle
        .act(&mut prepared, &binding.act[0], &step, Authority::Read)
        .await
        .unwrap()
    else {
        panic!("a position");
    };
    assert_eq!(marked.for_reconcile.get("scan_from"), Some(&Value::Int(7)));
    let row = stored_and_read_again(&marked.for_reconcile);
    scene
        .broker
        .set_fault(Fault::once(FaultKind::StoreThenDrop));
    let lost = failed(
        handle
            .act(&mut prepared, &binding.act[1], &step, write.authority())
            .await,
    );
    assert_eq!(lost, in_doubt());
    handle.release(prepared, true).await;

    // The resume: the scan starts where the log ended before the publish,
    // and one look finds the message.
    let browses = |scene: &Scene, from: usize| {
        scene.broker.requests()[from..]
            .iter()
            .filter(|request| request.op == Op::Browse)
            .count()
    };
    let before = scene.broker.requests().len();
    let mut stamped = input(&args);
    stamped.stamp = Some("tok-lost");
    let (handle, mut prepared) = scene.reading().await;
    let found = handle
        .act_recorded(
            &mut prepared,
            &binding.read_back[0],
            &stamped,
            Authority::Read,
            &row,
        )
        .await
        .unwrap();
    assert_eq!(mean(&means_of(&found), "from"), Some("recorded"));
    let found = done(found);
    assert_eq!(
        record(&found, "message_meta").get("position"),
        Some(&Value::Int(7))
    );
    assert_eq!(browses(&scene, before), 1);

    // Without what was recorded, the same scan starts at the log's start
    // and reads it page by page.
    let before = scene.broker.requests().len();
    let from_start = handle
        .act(
            &mut prepared,
            &binding.read_back[0],
            &stamped,
            Authority::Read,
        )
        .await
        .unwrap();
    assert_eq!(mean(&means_of(&from_start), "from"), Some("start"));
    assert_eq!(browses(&scene, before), 4);

    // A token that was never stored is absent from the place recorded to
    // the log's end.
    let mut other = input(&args);
    other.stamp = Some("tok-never");
    let absent = handle
        .act_recorded(
            &mut prepared,
            &binding.read_back[0],
            &other,
            Authority::Read,
            &row,
        )
        .await
        .unwrap();
    assert!(matches!(absent, StepOutcome::Absent(_)));
    handle.release(prepared, false).await;

    // The broker stored the message once and was sent nothing again.
    assert_eq!(scene.broker.received().len(), 5);
    let stored: Vec<_> = scene
        .broker
        .log(EVENTS)
        .into_iter()
        .filter(|message| {
            message.headers.get("idempotency-token").map(String::as_str) == Some("tok-lost")
        })
        .collect();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].position, Some(7));
}
