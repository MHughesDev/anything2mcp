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
use common::{Ledger, PLANTED, Scene, done, failed, input, read, record, signature_of, text};
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
    let mut signatures = Vec::new();
    let mut results = Vec::new();
    for _ in 0..5 {
        let (handle, mut prepared) = scene.reading().await;
        let values = done(
            read(&handle, &mut prepared, steps, &key_arg("A-100"))
                .await
                .unwrap(),
        );
        signatures.push(signature_of(&steps[0].kind, &values));
        results.push(values);
        assert!(handle.release(prepared, false).await.complete);
    }
    assert!(signatures.windows(2).all(|pair| pair[0] == pair[1]));
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
                &binding.act[0],
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
    assert!(refused.failure.message().contains("The quota is used up"));
    assert!(!refused.failure.message().contains(PLANTED));

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
    assert!(matches!(received[1].outcome, PublishOutcome::Stored { .. }));
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
