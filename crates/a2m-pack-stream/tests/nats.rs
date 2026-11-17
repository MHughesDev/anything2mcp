//! The stream pack over its first standard protocol, `nats`, against the
//! test server under `targets/example-nats`. Every test asks the server's
//! control interface what it stored, delivered, and was told; none proves a
//! write by the pack's own read path alone.
//!
//! The server was written from the protocol's documents and the server's
//! source, as the client was, so the two can be wrong in the same way. The
//! last test runs the same client against a real server when
//! `A2M_TEST_NATS` names one; what only it can show is in decision 0167.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test file: a helper that cannot do its work stops the test"
)]

mod common;

use std::path::Path;
use std::time::Duration;

use a2m_core::{Environment, Epoch, Plan, RunId, Value, Values};
use a2m_files::TargetRoot;
use a2m_ledger::Fence;
use a2m_provider::{
    Authority, CheckedPaths, Deadline, Destinations, DriverHandle, Posture, PrepareContext,
    Prepared, Registry, Sent, StepFailure, StepOutcome, SurfaceRef,
};
use common::{Folder, Ledger, Planted, done, failed, input, read, record, text};
use example_nats::model::{AckKind, Fault, Outcome, StreamSpec};
use example_nats::{MSG_ID, SEED_CONSUMER, SEED_EVENTS, SEED_STATUS, SEED_WORK, TestNats};

fn nats_root() -> TargetRoot {
    TargetRoot::new(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("example-nats"),
    )
}

/// The registry, the example target over `nats`, and a test server the
/// target's one surface points at.
struct Scene {
    _folder: Folder,
    registry: Registry,
    plan: Plan,
    stand: TestNats,
    surface: SurfaceRef,
    destinations: Destinations,
}

impl Scene {
    async fn start() -> Self {
        let stand = TestNats::start().await.unwrap();
        Self::at(stand, None).await
    }

    async fn at(stand: TestNats, address: Option<String>) -> Self {
        let folder = Folder::new("nats");
        let registry = common::registry(&folder);
        let plan = match common::compile(&registry, &nats_root()) {
            Ok(compiled) => compiled.plan,
            Err(problems) => panic!("the example target over nats does not compile:\n{problems}"),
        };
        let mut surface = plan.surfaces[0].clone();
        surface.address = address.unwrap_or_else(|| stand.address().to_string());
        let destinations = Destinations::new([surface.address.clone()]);
        Self {
            surface: SurfaceRef {
                target: plan.header.target.clone(),
                environment: Environment::new("test").unwrap(),
                surface,
            },
            _folder: folder,
            registry,
            plan,
            stand,
            destinations,
        }
    }

    fn binding(&self, operation: &str) -> &a2m_core::BindingPlan {
        &self
            .plan
            .operations
            .iter()
            .find(|found| found.name.as_str() == operation)
            .unwrap_or_else(|| panic!("no operation `{operation}`"))
            .bindings[0]
    }

    async fn try_prepare(
        &self,
        posture: Posture,
        fence: Option<Fence>,
    ) -> Result<(DriverHandle, Prepared), StepFailure> {
        let handle = self.registry.driver(&common::mode()).unwrap();
        let fence = fence.unwrap_or(Fence {
            run: RunId::from_bytes([7; 16]),
            epoch: Epoch::new(1),
        });
        let supervisor = handle.supervisor(Vec::new());
        let cx = PrepareContext {
            run: fence.run,
            epoch: fence.epoch,
            surface: &self.surface,
            plan: &self.plan,
            posture,
            deadline: Deadline::after(Duration::from_secs(20)),
            redact: &Planted,
            destinations: &self.destinations,
            paths: &CheckedPaths::none(),
            supervisor: &supervisor,
        };
        let prepared = handle.prepare(&cx, None).await?;
        Ok((handle, prepared))
    }

    async fn reading(&self) -> (DriverHandle, Prepared) {
        self.try_prepare(Posture::Live, None).await.unwrap()
    }
}

fn key_arg(order: &str) -> Values {
    Values::new().with("key", order)
}

fn event_args(order: &str, event: &str) -> Values {
    Values::new()
        .with("key", order)
        .with("body.order_id", order)
        .with("body.event", event)
}

fn api_subjects(stand: &TestNats) -> Vec<String> {
    stand
        .api_calls()
        .into_iter()
        .map(|call| call.subject)
        .collect()
}

// ---- 1. the wire, and 2. looks that take nothing ----

#[tokio::test]
async fn a_retained_key_reads_the_same_five_times_over_one_connection_and_takes_nothing() {
    let scene = Scene::start().await;
    let steps = &scene.binding("example_order_status_get").act;
    let mut results = Vec::new();
    let mut means = Vec::new();
    for _ in 0..5 {
        let (handle, mut prepared) = scene.reading().await;
        let outcome = read(&handle, &mut prepared, steps, &key_arg("A-100"))
            .await
            .unwrap();
        // How the read was done, which the trace's signature covers.
        means.push(means_of(&outcome));
        results.push(done(outcome));
        assert!(handle.release(prepared, false).await.complete);
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
    assert!(means.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(mean(&means[0], "protocol"), Some("nats"));
    assert_eq!(text(&results[0], "status"), "submitted");
    let meta = record(&results[0], "message_meta");
    assert_eq!(text(meta, "id"), "1");
    assert_eq!(text(meta, "key"), "A-100");

    let (handle, mut prepared) = scene.reading().await;
    let absent = read(&handle, &mut prepared, steps, &key_arg("A-999"))
        .await
        .unwrap();
    assert!(matches!(absent, StepOutcome::Absent(_)));
    handle.release(prepared, false).await;

    // The server: one connection, opened with headers and with word when
    // nobody listens; nothing delivered, published, or made; and only the
    // stream's own reads were asked for.
    let connects = scene.stand.connects();
    assert_eq!(connects.len(), 1, "{connects:?}");
    assert!(connects[0].headers && connects[0].no_responders && !connects[0].verbose);
    assert_eq!(connects[0].presented, "none");
    assert!(scene.stand.deliveries().is_empty());
    assert!(scene.stand.received().is_empty());
    assert!(api_subjects(&scene.stand).iter().all(|subject| {
        subject.starts_with("$JS.API.STREAM.MSG.GET.")
            || subject.starts_with("$JS.API.STREAM.INFO.")
            || subject.starts_with("$JS.API.CONSUMER.NAMES.")
    }));
    let status = scene.stand.stream(SEED_STATUS).unwrap();
    assert!(status.consumers.is_empty());
}

#[tokio::test]
async fn a_list_is_one_bounded_look_and_a_queue_looked_at_holds_what_it_held() {
    let scene = Scene::start().await;
    let events = &scene.binding("example_order_events_list").act;
    let (handle, mut prepared) = scene.reading().await;
    // Two messages a look: the log holds three.
    let first = done(
        read(&handle, &mut prepared, events, &Values::new())
            .await
            .unwrap(),
    );
    assert_eq!(first.get("count"), Some(&Value::Int(2)));
    assert_eq!(first.get("more"), Some(&Value::Bool(true)));
    assert_eq!(text(&first, "coverage"), "partial");
    assert_eq!(first.get("next"), Some(&Value::Int(3)));
    let rest = done(
        read(
            &handle,
            &mut prepared,
            events,
            &Values::new().with("from", 3),
        )
        .await
        .unwrap(),
    );
    assert_eq!(rest.get("count"), Some(&Value::Int(1)));
    assert_eq!(text(&rest, "coverage"), "complete");

    let work = &scene.binding("example_order_work_list").act;
    let listed = done(
        read(&handle, &mut prepared, work, &Values::new())
            .await
            .unwrap(),
    );
    assert_eq!(listed.get("count"), Some(&Value::Int(2)));
    handle.release(prepared, false).await;

    // The server: the queue holds both messages, none went out, none is
    // pending with the runtime's consumer.
    let queue = scene.stand.stream(SEED_WORK).unwrap();
    assert_eq!(queue.messages.len(), 2);
    assert!(queue.consumers.iter().all(|c| c.pending.is_empty()));
    assert!(scene.stand.deliveries().is_empty());
    assert!(scene.stand.acknowledgements().is_empty());
}

#[tokio::test]
async fn a_wait_on_a_log_reads_through_a_cursor_the_server_discards_and_a_topic_through_its_own() {
    let scene = Scene::start().await;
    let wait = &scene.binding("example_order_event_await").act;
    let (handle, mut prepared) = scene.reading().await;
    let args = Values::new().with("key", "A-100").with("from", 4);
    handle
        .act(&mut prepared, &wait[0], &input(&args), Authority::Read)
        .await
        .unwrap();
    // A consumer of the run's own, made with no acknowledgement.
    let made = scene.stand.stream(SEED_EVENTS).unwrap().consumers;
    assert_eq!(made.len(), 1);
    assert!(made[0].ephemeral);
    scene.stand.publish(
        "example.orders.events.A-100",
        &[],
        r#"{"order_id":"A-100","event":"shipped"}"#,
    );
    let arrived = done(
        handle
            .act(&mut prepared, &wait[1], &input(&args), Authority::Read)
            .await
            .unwrap(),
    );
    assert_eq!(arrived.get("arrived"), Some(&Value::Bool(true)));
    assert_eq!(text(record(&arrived, "message"), "event"), "shipped");
    handle.release(prepared, false).await;
    // Let go: the consumer is deleted, and nothing was acknowledged.
    assert!(
        scene
            .stand
            .stream(SEED_EVENTS)
            .unwrap()
            .consumers
            .is_empty()
    );
    assert!(scene.stand.acknowledgements().is_empty());

    // A topic: a subscription of the connection's own, and a notice that
    // carries the request's correlation.
    let notice = &scene.binding("example_order_notice_await").act;
    let (handle, mut prepared) = scene.reading().await;
    let args = Values::new().with("correlation", "r-1");
    handle
        .act(&mut prepared, &notice[0], &input(&args), Authority::Read)
        .await
        .unwrap();
    scene.stand.publish(
        "example.orders.notices",
        &[("correlation_id", "r-0")],
        r#"{"order_id":"A-100","text":"another"}"#,
    );
    scene.stand.publish(
        "example.orders.notices",
        &[("correlation_id", "r-1")],
        r#"{"order_id":"A-100","text":"yours"}"#,
    );
    let got = done(
        handle
            .act(&mut prepared, &notice[1], &input(&args), Authority::Read)
            .await
            .unwrap(),
    );
    assert_eq!(text(record(&got, "message"), "text"), "yours");
    handle.release(prepared, false).await;
}

// ---- 3. publishing with a key that makes a resume safe ----

#[tokio::test]
async fn a_publish_carries_the_records_token_and_the_window_bounds_what_the_server_remembers() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let binding = scene.binding("example_order_event_record");
    let args = event_args("A-100", "shipped");

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
                &write.input(&args, Some("tok-1")),
                write.authority(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(sent.get("position"), Some(&Value::Int(4)));
    assert_eq!(text(&sent, "message_id"), "4");
    assert_eq!(sent.get("duplicate"), Some(&Value::Bool(false)));
    handle.release(prepared, false).await;
    let log = scene.stand.messages(SEED_EVENTS);
    assert_eq!(log.len(), 4);
    assert_eq!(log[3].headers[MSG_ID], "tok-1");
    assert_eq!(log[3].subject, "example.orders.events.A-100");

    // Sent again within the window: the server stores nothing and names
    // the message it stored first.
    let again = ledger.write("A-100 again").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(again.fence))
        .await
        .unwrap();
    let repeat = done(
        handle
            .act(
                &mut prepared,
                &binding.act[1],
                &again.input(&args, Some("tok-1")),
                again.authority(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(repeat.get("duplicate"), Some(&Value::Bool(true)));
    assert_eq!(repeat.get("position"), Some(&Value::Int(4)));
    handle.release(prepared, false).await;
    assert_eq!(scene.stand.messages(SEED_EVENTS).len(), 4);
    assert_eq!(scene.stand.received()[1].outcome, Outcome::Duplicate(4));

    // Past the window the server has forgotten the token. A resume
    // reconciles first: the scan finds the message by its token, so
    // nothing is sent again.
    scene.stand.advance(Duration::from_secs(180));
    let (handle, mut prepared) = scene.reading().await;
    let mut stamped = input(&args);
    stamped.stamp = Some("tok-1");
    let found = done(
        handle
            .act(&mut prepared, &binding.lookup[0], &stamped, Authority::Read)
            .await
            .unwrap(),
    );
    assert_eq!(
        record(&found, "message_meta").get("position"),
        Some(&Value::Int(4))
    );
    handle.release(prepared, false).await;

    // What the scan prevents: the same token sent past the window is
    // stored a second time.
    let late = ledger.write("A-100 late").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(late.fence))
        .await
        .unwrap();
    let twice = done(
        handle
            .act(
                &mut prepared,
                &binding.act[1],
                &late.input(&args, Some("tok-1")),
                late.authority(),
            )
            .await
            .unwrap(),
    );
    assert_eq!(twice.get("duplicate"), Some(&Value::Bool(false)));
    handle.release(prepared, false).await;
    assert_eq!(scene.stand.received()[2].outcome, Outcome::Stored(5));
}

#[tokio::test]
async fn a_stream_that_remembers_a_token_for_less_than_the_file_says_is_drift() {
    let scene = Scene::start().await;
    scene.stand.declare_stream(
        StreamSpec::new(SEED_EVENTS, ["example.orders.events.*"]).window(Duration::from_secs(60)),
    );
    let ledger = Ledger::open().await;
    let binding = scene.binding("example_order_event_record");
    let write = ledger.write("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let args = event_args("A-100", "shipped");
    let refused = failed(
        handle
            .act(
                &mut prepared,
                &binding.act[1],
                &write.input(&args, Some("tok-1")),
                write.authority(),
            )
            .await,
    );
    assert_eq!(
        (refused.0.as_str(), refused.1.as_str()),
        ("drift", "shape_changed")
    );
    handle.release(prepared, true).await;
    assert!(scene.stand.received().is_empty());
}

#[tokio::test]
async fn a_lost_or_unreadable_acknowledgement_is_a_write_in_doubt_and_a_refusal_is_the_servers() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let binding = scene.binding("example_order_event_record");
    let args = event_args("A-101", "shipped");
    let mut stored = 3;
    for (fault, token) in [
        (Fault::DropPublishAck, "tok-drop"),
        (Fault::CloseAfterStore, "tok-close"),
        (Fault::UnknownCodeAfterStore, "tok-code"),
    ] {
        scene.stand.clear_faults();
        let write = ledger.write(token).await;
        let (handle, mut prepared) = scene
            .try_prepare(Posture::Live, Some(write.fence))
            .await
            .unwrap();
        // The shape is compared before the fault is on: the fault is the
        // publish's alone.
        scene.stand.set_fault(fault.clone());
        let mut step = write.input(&args, Some(token));
        step.deadline = Deadline::after(Duration::from_secs(2));
        let doubt = failed(
            handle
                .act(&mut prepared, &binding.act[1], &step, write.authority())
                .await,
        );
        assert_eq!(
            doubt,
            (
                "possible_commit".to_owned(),
                "no_confirmation".to_owned(),
                Sent::Unknown
            ),
            "{fault:?}"
        );
        handle.release(prepared, true).await;
        stored += 1;
        // The server stored it once, and the record's token finds it.
        let log = scene.stand.messages(SEED_EVENTS);
        assert_eq!(log.len(), stored, "{fault:?}");
        assert_eq!(log[stored - 1].headers[MSG_ID], token);
        scene.stand.clear_faults();
        let (handle, mut prepared) = scene.reading().await;
        let mut stamped = input(&args);
        stamped.stamp = Some(token);
        let found = handle
            .act(&mut prepared, &binding.lookup[0], &stamped, Authority::Read)
            .await
            .unwrap();
        assert!(matches!(found, StepOutcome::Done(_)), "{fault:?}");
        handle.release(prepared, false).await;
    }

    // A refusal the protocol defines: answered, and nothing stored.
    scene
        .stand
        .set_fault(Fault::RefusePublish { err_code: 10077 });
    let write = ledger.write("refused").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let refused = failed(
        handle
            .act(
                &mut prepared,
                &binding.act[1],
                &write.input(&args, Some("tok-refused")),
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
    handle.release(prepared, true).await;
    assert_eq!(scene.stand.messages(SEED_EVENTS).len(), stored);
    assert!(matches!(
        scene.stand.received().last().unwrap().outcome,
        Outcome::Refused(10077)
    ));
}

#[tokio::test]
async fn nothing_a_step_sends_can_move_a_message_to_another_subject_or_past_the_servers_limit() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let binding = scene.binding("example_order_event_record");
    let write = ledger.write("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    for key in ["A.100", ">", "A-100 B"] {
        let args = event_args(key, "shipped");
        let refused = failed(
            handle
                .act(
                    &mut prepared,
                    &binding.act[1],
                    &write.input(&args, Some("tok-1")),
                    write.authority(),
                )
                .await,
        );
        assert_eq!(
            refused,
            (
                "input".to_owned(),
                "value_out_of_range".to_owned(),
                Sent::NotSent
            ),
            "{key}"
        );
    }
    handle.release(prepared, true).await;

    // The server's own payload limit, from its `INFO`, is held before
    // anything is sent.
    scene.stand.set_max_payload(48);
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let large = failed(
        handle
            .act(
                &mut prepared,
                &binding.act[1],
                &write.input(&event_args("A-100", "shipped"), Some("tok-1")),
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
    handle.release(prepared, true).await;
    assert!(scene.stand.received().is_empty());
}

// ---- 4. taking from a work queue ----

#[tokio::test]
async fn a_take_records_the_message_then_acknowledges_and_the_server_answers_the_acknowledgement() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let take = &scene.binding("example_order_work_take").act;
    let none = Values::new();

    // Taken and let go unacknowledged: given back at once.
    let (handle, mut prepared) = scene.reading().await;
    let StepOutcome::Done(taken) = read(&handle, &mut prepared, &take[..2], &none)
        .await
        .unwrap()
    else {
        panic!("a message");
    };
    assert_eq!(text(record(&taken.values, "message"), "task"), "pick");
    assert_eq!(text(&taken.for_reconcile, "message_id"), "1");
    handle.release(prepared, false).await;
    let given_back = scene.stand.acknowledgements();
    assert_eq!(given_back.len(), 1);
    assert_eq!((given_back[0].kind, given_back[0].seq), (AckKind::Nak, 1));
    let queue = scene.stand.stream(SEED_WORK).unwrap();
    assert_eq!(queue.messages.len(), 2);
    assert!(queue.consumers[0].pending.is_empty());

    // With the record's token: taken again, recorded, acknowledged, and
    // the server says it is gone.
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
    assert_eq!(
        text(record(&taken.for_reconcile, "message"), "task"),
        "pick"
    );
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
    assert_eq!(text(&acknowledged, "message_id"), "1");
    handle.release(prepared, false).await;
    let acks = scene.stand.acknowledgements();
    assert_eq!(acks.len(), 2);
    assert_eq!(
        (acks[1].kind, acks[1].seq, acks[1].was_pending),
        (AckKind::Ack, 1, true)
    );
    let left = scene.stand.messages(SEED_WORK);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].seq, 2);
    // Two deliveries of the first message: the one given back, and the take.
    let deliveries = scene.stand.deliveries();
    assert_eq!(deliveries.len(), 2);
    assert_eq!(deliveries[1].attempt, 2);
}

#[tokio::test]
async fn an_acknowledgement_whose_answer_is_lost_is_in_doubt_and_its_message_is_in_the_record() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let take = &scene.binding("example_order_work_take").act;
    let none = Values::new();
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
    scene.stand.set_fault(Fault::DropAckReply);
    let mut step = write.input(&none, None);
    step.deadline = Deadline::after(Duration::from_secs(2));
    let doubt = failed(
        handle
            .act(&mut prepared, &take[2], &step, write.authority())
            .await,
    );
    assert_eq!(
        doubt,
        (
            "possible_commit".to_owned(),
            "no_confirmation".to_owned(),
            Sent::Unknown
        )
    );
    handle.release(prepared, true).await;
    // The server carried it out: the message is gone, and the record holds
    // the whole of it.
    assert_eq!(scene.stand.acknowledgements()[0].kind, AckKind::Ack);
    assert_eq!(scene.stand.messages(SEED_WORK).len(), 1);
    let kept = record(&taken.for_reconcile, "message");
    assert_eq!(text(kept, "task"), "pick");
    assert_eq!(text(kept, "order_id"), "A-100");
}

#[tokio::test]
async fn an_acknowledgement_nobody_listens_for_acknowledged_nothing() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
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
    assert!(scene.stand.remove_consumer(SEED_WORK, SEED_CONSUMER));
    let gone = failed(
        handle
            .act(
                &mut prepared,
                &take[2],
                &write.input(&none, None),
                write.authority(),
            )
            .await,
    );
    assert_eq!(
        gone,
        (
            "drift".to_owned(),
            "channel_missing".to_owned(),
            Sent::Answered
        )
    );
    handle.release(prepared, true).await;
    assert!(scene.stand.acknowledgements().is_empty());
    assert_eq!(scene.stand.messages(SEED_WORK).len(), 2);
}

#[tokio::test]
async fn a_pull_that_hands_over_more_than_was_asked_is_a_stop() {
    let scene = Scene::start().await;
    let take = &scene.binding("example_order_work_take").act;
    scene.stand.set_fault(Fault::DeliverExtra);
    let (handle, mut prepared) = scene.reading().await;
    let more = failed(read(&handle, &mut prepared, &take[..2], &Values::new()).await);
    assert_eq!(
        (more.0.as_str(), more.1.as_str()),
        ("unknown_variant", "unexpected_reply")
    );
    handle.release(prepared, true).await;
    assert!(
        scene
            .stand
            .acknowledgements()
            .iter()
            .all(|ack| ack.kind != AckKind::Ack)
    );
    assert_eq!(scene.stand.messages(SEED_WORK).len(), 2);
}

// ---- unknown means stop ----

#[tokio::test]
async fn a_line_the_protocol_does_not_have_stops_a_read_and_leaves_a_write_in_doubt() {
    let scene = Scene::start().await;
    let steps = &scene.binding("example_order_status_get").act;
    let (handle, mut prepared) = scene.reading().await;
    scene.stand.set_fault(Fault::UnknownLine);
    let stopped = failed(read(&handle, &mut prepared, steps, &key_arg("A-100")).await);
    assert_eq!(
        (stopped.0.as_str(), stopped.1.as_str()),
        ("unknown_variant", "unexpected_reply")
    );
    handle.release(prepared, true).await;
    scene.stand.clear_faults();

    let ledger = Ledger::open().await;
    let binding = scene.binding("example_order_event_record");
    let write = ledger.write("A-100").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let args = event_args("A-100", "shipped");
    handle
        .act(
            &mut prepared,
            &binding.act[1],
            &write.input(&args, Some("tok-a")),
            write.authority(),
        )
        .await
        .unwrap();
    scene.stand.set_fault(Fault::UnknownLine);
    let doubt = failed(
        handle
            .act(
                &mut prepared,
                &binding.act[1],
                &write.input(&args, Some("tok-b")),
                write.authority(),
            )
            .await,
    );
    assert_eq!(
        doubt,
        (
            "possible_commit".to_owned(),
            "no_confirmation".to_owned(),
            Sent::Unknown
        )
    );
    handle.release(prepared, true).await;
}

#[tokio::test]
async fn a_server_that_refuses_the_session_or_asks_for_tls_or_is_not_on_this_machine_is_named() {
    let scene = Scene::start().await;
    scene.stand.require_token(Some("example-token-for-tests"));
    let refused = scene.try_prepare(Posture::Live, None).await.err().unwrap();
    assert_eq!(
        (
            refused.failure.class().to_string(),
            refused.failure.code().to_string()
        ),
        ("session_expired".to_owned(), "needs_login".to_owned())
    );
    assert_eq!(scene.stand.connects()[0].presented, "none");
    assert!(!scene.stand.connects()[0].accepted);
    scene.stand.require_token(None);

    scene.stand.set_tls_required(true);
    let tls = scene.try_prepare(Posture::Live, None).await.err().unwrap();
    assert_eq!(
        (tls.failure.code().to_string(), tls.sent),
        ("transport_refused".to_owned(), Sent::NotSent)
    );
    assert_eq!(scene.stand.connects().len(), 1);

    // An address that is not this machine's own: nothing is opened.
    let far = Scene::at(
        TestNats::start().await.unwrap(),
        Some("192.0.2.10:4222".to_owned()),
    )
    .await;
    let elsewhere = far.try_prepare(Posture::Live, None).await.err().unwrap();
    assert_eq!(
        (
            elsewhere.failure.class().to_string(),
            elsewhere.failure.code().to_string(),
            elsewhere.sent
        ),
        (
            "policy".to_owned(),
            "transport_refused".to_owned(),
            Sent::NotSent
        )
    );
}

// ---- 5. discovery ----

#[tokio::test]
async fn a_discovery_lists_streams_and_consumers_through_reads_only() {
    use a2m_core::{Archetype, Failure, Finding, SurfaceKind};
    use a2m_provider::{DiscoveryScope, FindingSink, ReadOnlyPort, SurfaceDescription};

    struct Kept(Vec<Finding>);
    impl FindingSink for Kept {
        fn found(&mut self, finding: Finding) -> Result<(), Failure> {
            self.0.push(finding);
            Ok(())
        }
    }

    let scene = Scene::start().await;
    let description = SurfaceDescription {
        kind: SurfaceKind::new("broker").unwrap(),
        archetype: Archetype::Stream,
        address: scene.surface.surface.address.clone(),
        facts: Values::new(),
    };
    let discoverer = scene.registry.discoverers_for(&description).remove(0);
    let (handle, prepared) = scene.try_prepare(Posture::Discovery, None).await.unwrap();
    let port = ReadOnlyPort::new(handle, prepared);
    let focus = Values::new();
    let scope = DiscoveryScope {
        surface: &scene.surface,
        bounds: discoverer.declared().bounds,
        deadline: Deadline::after(Duration::from_secs(20)),
        paths: &CheckedPaths::none(),
        redact: &Planted,
        focus: &focus,
    };
    let mut kept = Kept(Vec::new());
    discoverer.discover(&port, &scope, &mut kept).await.unwrap();
    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;

    let found: String = kept
        .0
        .iter()
        .map(|finding| format!("{finding:?}"))
        .collect();
    for address in [
        "ORDERS_STATUS/example.orders.status",
        "ORDERS_EVENTS/example.orders.events",
        "ORDERS_WORK/example.orders.work",
    ] {
        assert!(found.contains(address), "{address} not in {found}");
    }
    // The server: only lists and reads; no consumer made, nothing out.
    assert!(api_subjects(&scene.stand).iter().all(|subject| {
        subject == "$JS.API.STREAM.LIST"
            || subject.starts_with("$JS.API.CONSUMER.NAMES.")
            || subject.starts_with("$JS.API.STREAM.INFO.")
            || subject.starts_with("$JS.API.STREAM.MSG.GET.")
    }));
    assert!(scene.stand.deliveries().is_empty());
    assert!(scene.stand.acknowledgements().is_empty());
    assert!(scene.stand.received().is_empty());
    assert_eq!(scene.stand.messages(SEED_WORK).len(), 2);
    assert_eq!(scene.stand.stream(SEED_WORK).unwrap().consumers.len(), 1);
}

#[tokio::test]
async fn the_drafted_reads_over_nats_return_what_the_server_holds() {
    use a2m_core::{Archetype, Failure, Finding, SurfaceKind};
    use a2m_provider::{DiscoveryScope, FindingSink, ReadOnlyPort, SurfaceDescription};

    struct Kept(Vec<Finding>);
    impl FindingSink for Kept {
        fn found(&mut self, finding: Finding) -> Result<(), Failure> {
            self.0.push(finding);
            Ok(())
        }
    }

    let mut scene = Scene::start().await;
    let description = SurfaceDescription {
        kind: SurfaceKind::new("broker").unwrap(),
        archetype: Archetype::Stream,
        address: scene.surface.surface.address.clone(),
        facts: Values::new(),
    };
    let discoverer = scene.registry.discoverers_for(&description).remove(0);
    let (handle, prepared) = scene.try_prepare(Posture::Discovery, None).await.unwrap();
    let port = ReadOnlyPort::new(handle, prepared);
    let focus = Values::new();
    let scope = DiscoveryScope {
        surface: &scene.surface,
        bounds: discoverer.declared().bounds,
        deadline: Deadline::after(Duration::from_secs(20)),
        paths: &CheckedPaths::none(),
        redact: &Planted,
        focus: &focus,
    };
    let mut kept = Kept(Vec::new());
    discoverer.discover(&port, &scope, &mut kept).await.unwrap();
    let (handle, prepared) = port.into_parts();
    handle.release(prepared, false).await;

    let folder = Folder::new("nats-drafted-reads");
    let root = folder.path().join("target");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::copy(
        nats_root().path().join("target.yaml"),
        root.join("target.yaml"),
    )
    .unwrap();
    let report = a2m_files::Drafter::new(scene.registry.compiler_set(), &TargetRoot::new(&root))
        .draft(&kept.0)
        .unwrap();
    // A stream states no key and holds no schema: a look at each, which
    // states what every look returns, and no read of one key.
    let lists: Vec<&str> = report
        .written
        .iter()
        .map(|path| path.as_str())
        .filter(|path| path.starts_with("operations/"))
        .collect();
    assert_eq!(
        lists,
        [
            "operations/orders_events_list.yaml",
            "operations/orders_status_list.yaml",
            "operations/orders_work_list.yaml"
        ]
    );
    common::answer_roles(&root);
    scene.plan = match common::compile(&scene.registry, &TargetRoot::new(&root)) {
        Ok(compiled) => compiled.plan,
        Err(problems) => panic!("the drafts do not compile:\n{problems}"),
    };
    let held: Vec<(String, usize)> = [SEED_EVENTS, SEED_STATUS, SEED_WORK]
        .iter()
        .map(|stream| ((*stream).to_owned(), scene.stand.messages(stream).len()))
        .collect();
    let (handle, mut prepared) = scene.reading().await;
    for ((operation, (stream, count)), _) in [
        "example_orders_events_list",
        "example_orders_status_list",
        "example_orders_work_list",
    ]
    .iter()
    .zip(&held)
    .zip(0..)
    {
        let steps = scene.binding(operation).act.clone();
        let observed = done(
            read(&handle, &mut prepared, &steps, &Values::new())
                .await
                .unwrap(),
        );
        let result = common::outputs_of(&scene.plan, operation, &observed);
        assert_eq!(
            result.get("count"),
            Some(&Value::Int(i64::try_from(*count).unwrap())),
            "{operation} over {stream}"
        );
        assert_eq!(result.get("more"), Some(&Value::Bool(false)));
    }
    handle.release(prepared, false).await;
    // The server: no consumer made, nothing delivered, acknowledged, or
    // received; every stream holds what it held.
    assert!(scene.stand.deliveries().is_empty());
    assert!(scene.stand.acknowledgements().is_empty());
    assert!(scene.stand.received().is_empty());
    for (stream, count) in &held {
        assert_eq!(scene.stand.messages(stream).len(), *count);
    }
    assert_eq!(scene.stand.stream(SEED_WORK).unwrap().consumers.len(), 1);
}

// ---- 6. the protocol is one interface; its limits are the compiler's ----

#[tokio::test]
async fn a_channel_states_only_what_the_protocol_can_carry() {
    let folder = Folder::new("nats-lint");
    let registry = common::registry(&folder);
    let root = folder.path().join("target");
    let copy = |from: &Path, to: &Path| {
        fn walk(from: &Path, to: &Path) {
            std::fs::create_dir_all(to).unwrap();
            for entry in std::fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                let target = to.join(entry.file_name());
                if entry.path().is_dir() {
                    walk(&entry.path(), &target);
                } else {
                    std::fs::copy(entry.path(), target).unwrap();
                }
            }
        }
        walk(from, to);
    };
    copy(nats_root().path(), &root);
    let channels = root.join("channels").join("orders.yaml");
    let original = std::fs::read_to_string(&channels).unwrap();
    for (from, to) in [
        // A log that names no stream.
        (
            "address: ORDERS_EVENTS/example.orders.events",
            "address: example.orders.events",
        ),
        // A token the stream does not read.
        (
            "idempotency: {header: Nats-Msg-Id, broker_dedupe: true, window: 2m}\nwait: {default: 2s, max: 10s}\nlimits: {browse_count: 2}",
            "idempotency: {header: idempotency-token, broker_dedupe: true, window: 2m}\nwait: {default: 2s, max: 10s}\nlimits: {browse_count: 2}",
        ),
        // A window the file does not state.
        (
            "idempotency: {header: Nats-Msg-Id, broker_dedupe: true, window: 2m}\nwait: {default: 2s, max: 10s}\nlimits: {browse_count: 2}",
            "idempotency: {header: Nats-Msg-Id, broker_dedupe: true}\nwait: {default: 2s, max: 10s}\nlimits: {browse_count: 2}",
        ),
        // A take with no consumer a person made.
        ("subscription: example_runtime\n", ""),
    ] {
        assert!(original.contains(from), "{from}");
        std::fs::write(&channels, original.replace(from, to)).unwrap();
        let problems = common::compile(&registry, &TargetRoot::new(root.clone()))
            .err()
            .unwrap_or_else(|| panic!("compiled with `{to}`"));
        assert!(
            problems.to_string().contains("stream.protocol_unfit"),
            "{problems}"
        );
    }
}

// ---- the same client against a real server ----

/// A real server's ground truth, read by a few lines of the protocol of the
/// test's own, never by the pack's client.
mod real {
    use std::time::Duration;

    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    pub struct Raw {
        stream: BufReader<TcpStream>,
        next: u64,
    }

    impl Raw {
        pub async fn connect(address: &str) -> Self {
            let stream = TcpStream::connect(address).await.unwrap();
            let mut raw = Self {
                stream: BufReader::new(stream),
                next: 0,
            };
            let mut info = String::new();
            raw.stream.read_line(&mut info).await.unwrap();
            assert!(info.starts_with("INFO "), "{info}");
            raw.stream
                .get_mut()
                .write_all(
                    b"CONNECT {\"verbose\":false,\"headers\":true,\"no_responders\":true,\"protocol\":1}\r\nSUB _INBOX.rawtest.* 1\r\nPING\r\n",
                )
                .await
                .unwrap();
            loop {
                let mut line = String::new();
                raw.stream.read_line(&mut line).await.unwrap();
                if line.starts_with("PONG") {
                    break;
                }
            }
            raw
        }

        /// One request on the JetStream interface, and its JSON answer.
        pub async fn api(&mut self, subject: &str, body: &str) -> serde_json::Value {
            self.next += 1;
            let reply = format!("_INBOX.rawtest.{}", self.next);
            let line = format!("PUB {subject} {reply} {}\r\n{body}\r\n", body.len());
            self.stream
                .get_mut()
                .write_all(line.as_bytes())
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let mut head = String::new();
                    self.stream.read_line(&mut head).await.unwrap();
                    let parts: Vec<&str> = head.split_whitespace().collect();
                    match parts.first().copied() {
                        Some("MSG") | Some("HMSG") => {
                            let size: usize = parts.last().unwrap().parse().unwrap();
                            let mut payload = vec![0_u8; size + 2];
                            self.stream.read_exact(&mut payload).await.unwrap();
                            if parts[1] == reply {
                                payload.truncate(size);
                                let text = String::from_utf8_lossy(&payload).into_owned();
                                let json = text
                                    .rfind("\r\n\r\n")
                                    .map_or(text.as_str(), |at| &text[at + 4..]);
                                return serde_json::from_str(json)
                                    .unwrap_or(serde_json::Value::Null);
                            }
                        }
                        Some("PING") => self.stream.get_mut().write_all(b"PONG\r\n").await.unwrap(),
                        _ => {}
                    }
                }
            })
            .await
            .unwrap()
        }
    }
}

/// Runs only when `A2M_TEST_NATS` names a real server, such as
/// `127.0.0.1:4222`, started with JetStream (`nats-server -js`). It makes
/// its own streams under names no other test uses, deletes them at the end,
/// and reads what the server holds through the test's own requests.
#[tokio::test]
async fn the_same_client_against_a_real_server() {
    let Ok(address) = std::env::var("A2M_TEST_NATS") else {
        eprintln!("skipped: A2M_TEST_NATS names no real server");
        return;
    };
    let mut raw = real::Raw::connect(&address).await;
    for name in [SEED_STATUS, SEED_EVENTS, SEED_WORK] {
        raw.api(&format!("$JS.API.STREAM.DELETE.{name}"), "").await;
    }
    let made = [
        (
            SEED_STATUS,
            r#"{"name":"ORDERS_STATUS","subjects":["example.orders.status.*"],"retention":"limits","max_msgs_per_subject":1,"storage":"memory","duplicate_window":120000000000}"#,
        ),
        (
            SEED_EVENTS,
            r#"{"name":"ORDERS_EVENTS","subjects":["example.orders.events.*"],"retention":"limits","storage":"memory","duplicate_window":120000000000}"#,
        ),
        (
            SEED_WORK,
            r#"{"name":"ORDERS_WORK","subjects":["example.orders.work.*"],"retention":"workqueue","storage":"memory","duplicate_window":120000000000}"#,
        ),
    ];
    for (name, config) in made {
        let answer = raw
            .api(&format!("$JS.API.STREAM.CREATE.{name}"), config)
            .await;
        assert!(answer.get("error").is_none(), "{answer}");
    }
    let consumer = r#"{"stream_name":"ORDERS_WORK","config":{"durable_name":"example_runtime","ack_policy":"explicit","ack_wait":30000000000}}"#;
    let answer = raw
        .api(
            "$JS.API.CONSUMER.CREATE.ORDERS_WORK.example_runtime",
            consumer,
        )
        .await;
    assert!(answer.get("error").is_none(), "{answer}");
    for (subject, body) in [
        (
            "example.orders.status.A-100",
            r#"{"order_id":"A-100","status":"submitted"}"#,
        ),
        (
            "example.orders.events.A-100",
            r#"{"order_id":"A-100","event":"created"}"#,
        ),
        (
            "example.orders.work.A-100",
            r#"{"order_id":"A-100","task":"pick"}"#,
        ),
    ] {
        let ack = raw.api(subject, body).await;
        assert!(ack.get("seq").is_some(), "{ack}");
    }

    let stand = TestNats::start().await.unwrap();
    let scene = Scene::at(stand, Some(address.clone())).await;
    let ledger = Ledger::open().await;

    // A look takes nothing.
    let (handle, mut prepared) = scene.reading().await;
    let status = done(
        read(
            &handle,
            &mut prepared,
            &scene.binding("example_order_status_get").act,
            &key_arg("A-100"),
        )
        .await
        .unwrap(),
    );
    assert_eq!(text(&status, "status"), "submitted");
    handle.release(prepared, false).await;

    // A publish with the record's token, sent twice: one stored.
    let binding = scene.binding("example_order_event_record");
    let args = event_args("A-100", "shipped");
    let mut seqs = Vec::new();
    for name in ["first", "again"] {
        let write = ledger.write(name).await;
        let (handle, mut prepared) = scene
            .try_prepare(Posture::Live, Some(write.fence))
            .await
            .unwrap();
        let sent = done(
            handle
                .act(
                    &mut prepared,
                    &binding.act[1],
                    &write.input(&args, Some("tok-real")),
                    write.authority(),
                )
                .await
                .unwrap(),
        );
        seqs.push((
            sent.get("position").cloned(),
            sent.get("duplicate").cloned(),
        ));
        handle.release(prepared, false).await;
    }
    assert_eq!(seqs[0].0, seqs[1].0);
    assert_eq!(seqs[1].1, Some(Value::Bool(true)));
    let info = raw.api("$JS.API.STREAM.INFO.ORDERS_EVENTS", "").await;
    assert_eq!(info["state"]["messages"], 2, "{info}");

    // A take: recorded, then acknowledged; the server no longer holds it.
    let take = &scene.binding("example_order_work_take").act;
    let write = ledger.write("taken").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let StepOutcome::Done(taken) = read(&handle, &mut prepared, &take[..2], &Values::new())
        .await
        .unwrap()
    else {
        panic!("a message");
    };
    assert_eq!(
        text(record(&taken.for_reconcile, "message"), "task"),
        "pick"
    );
    done(
        handle
            .act(
                &mut prepared,
                &take[2],
                &write.input(&Values::new(), None),
                write.authority(),
            )
            .await
            .unwrap(),
    );
    handle.release(prepared, false).await;
    let info = raw.api("$JS.API.STREAM.INFO.ORDERS_WORK", "").await;
    assert_eq!(info["state"]["messages"], 0, "{info}");

    for name in [SEED_STATUS, SEED_EVENTS, SEED_WORK] {
        raw.api(&format!("$JS.API.STREAM.DELETE.{name}"), "").await;
    }
}

// ---- what earlier steps recorded (decision 0162, point 1) ----

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

/// The engine's stored form of what a step gave for reconcile, read again.
fn stored_and_read_again(for_reconcile: &Values) -> Values {
    a2m_core::CanonicalJson::encode(for_reconcile)
        .unwrap()
        .decode()
        .unwrap()
}

fn gets_since(stand: &TestNats, from: usize) -> usize {
    api_subjects(stand)[from..]
        .iter()
        .filter(|subject| subject.contains("STREAM.MSG.GET"))
        .count()
}

#[tokio::test]
async fn a_take_in_doubt_is_settled_by_a_look_at_the_sequence_it_recorded() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let binding = scene.binding("example_order_work_take");
    let (take, look) = (&binding.act, &binding.read_back[0]);
    let none = Values::new();

    // The server carries the acknowledgement out and its answer is lost.
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
    let row = stored_and_read_again(&taken.for_reconcile);
    scene.stand.set_fault(Fault::DropAckReply);
    let mut step = write.keyless(&none);
    step.deadline = Deadline::after(Duration::from_secs(2));
    let doubt = failed(
        handle
            .act(&mut prepared, &take[2], &step, write.authority())
            .await,
    );
    assert_eq!(doubt.0, "possible_commit");
    handle.release(prepared, true).await;
    scene.stand.clear_faults();
    assert_eq!(scene.stand.messages(SEED_WORK).len(), 1);

    // The resume looks at the one place the message was stored: it is gone,
    // so the acknowledgement landed.
    let (handle, mut prepared) = scene.reading().await;
    let settled = handle
        .act_recorded(
            &mut prepared,
            look,
            &write.keyless(&none),
            Authority::Read,
            &row,
        )
        .await
        .unwrap();
    assert_eq!(mean(&means_of(&settled), "look"), Some("by_position"));
    let settled = done(settled);
    assert_eq!(settled.get("acknowledged"), Some(&Value::Bool(true)));
    assert_eq!(text(&settled, "message_id"), text(&row, "message_id"));
    handle.release(prepared, false).await;
    assert_eq!(scene.stand.acknowledgements().len(), 1);

    // A take whose run ended before its acknowledgement: the message is
    // still stored, and the record is absent.
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
    handle.release(prepared, false).await;
    assert_eq!(scene.stand.acknowledgements().len(), 1);
    assert_eq!(scene.stand.messages(SEED_WORK).len(), 1);
}

#[tokio::test]
async fn a_publish_in_doubt_past_the_window_is_found_by_a_scan_from_the_recorded_position() {
    let scene = Scene::start().await;
    let ledger = Ledger::open().await;
    let binding = scene.binding("example_order_event_record");
    let args = event_args("A-101", "shipped");
    // Others have written to the stream: it holds nine messages.
    for n in 0..6 {
        scene.stand.publish(
            "example.orders.events.A-102",
            &[],
            &format!("{{\"order_id\":\"A-102\",\"event\":\"e{n}\"}}"),
        );
    }

    // Where the stream ends is recorded; the publish is stored and its
    // acknowledgement is lost.
    let write = ledger.write("A-101").await;
    let (handle, mut prepared) = scene
        .try_prepare(Posture::Live, Some(write.fence))
        .await
        .unwrap();
    let StepOutcome::Done(marked) = handle
        .act(
            &mut prepared,
            &binding.act[0],
            &write.input(&args, Some("tok-late")),
            Authority::Read,
        )
        .await
        .unwrap()
    else {
        panic!("a position");
    };
    assert_eq!(marked.for_reconcile.get("scan_from"), Some(&Value::Int(10)));
    let row = stored_and_read_again(&marked.for_reconcile);
    scene.stand.set_fault(Fault::DropPublishAck);
    let mut step = write.input(&args, Some("tok-late"));
    step.deadline = Deadline::after(Duration::from_secs(2));
    let doubt = failed(
        handle
            .act(&mut prepared, &binding.act[1], &step, write.authority())
            .await,
    );
    assert_eq!(doubt.0, "possible_commit");
    handle.release(prepared, true).await;
    scene.stand.clear_faults();

    // Past the window the server has forgotten the token, so a resend would
    // store a second message. The resume reconciles first, and the scan
    // from the recorded position finds the one stored in one look.
    scene.stand.advance(Duration::from_secs(180));
    let mut stamped = input(&args);
    stamped.stamp = Some("tok-late");
    let before = api_subjects(&scene.stand).len();
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
    assert_eq!(
        record(&done(found), "message_meta").get("position"),
        Some(&Value::Int(10))
    );
    let recorded_gets = gets_since(&scene.stand, before);
    // From the start of the stream the same scan reads every message.
    let before = api_subjects(&scene.stand).len();
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
    assert!(matches!(from_start, StepOutcome::Done(_)));
    let start_gets = gets_since(&scene.stand, before);
    assert!(recorded_gets <= 2, "{recorded_gets}");
    assert!(start_gets >= 10, "{start_gets}");
    handle.release(prepared, false).await;

    // The server stored it once; nothing was sent again.
    let stored: Vec<_> = scene
        .stand
        .messages(SEED_EVENTS)
        .into_iter()
        .filter(|message| message.headers.get(MSG_ID).map(String::as_str) == Some("tok-late"))
        .collect();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].seq, 10);
}
