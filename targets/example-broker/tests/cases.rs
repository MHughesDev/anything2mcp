//! The broker's own tests: every case and every fault in `README.md`. Each test talks to
//! a real broker over TCP as an ordinary client, and proves what happened by asking the
//! control interface.

use std::error::Error;
use std::time::{Duration, Instant};

use example_broker::client::{Client, ClientError, Delivered, Publish};
use example_broker::faults::{Fault, FaultKind};
use example_broker::harness::TestBroker;
use example_broker::model::{
    ChannelSpec, DeliveryCause, DeliveryOutcome, HeldState, Op, PublishOutcome, Shape, body_digest,
};
use example_broker::{SEED_DEDUPE_HEADER, SEED_SUBSCRIPTION, SILENT_CONSUMER};
use serde_json::{Value, json};

type Outcome = Result<(), Box<dyn Error>>;

const STATUS: &str = "example.orders.status";
const EVENTS: &str = "example.orders.events";
const WORK: &str = "example.orders.work";
const NOTICES: &str = "example.orders.notices";

/// The longest a test waits for a condition that is expected to come true at once.
const LIMIT: Duration = Duration::from_secs(20);
const NOW: Duration = Duration::ZERO;

/// The broker's error code, where a refusal is what the test expects.
fn refused<T: std::fmt::Debug>(result: Result<T, ClientError>) -> String {
    match result {
        Err(ClientError::Refused { code, .. }) => code,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn ids(delivered: &[Delivered]) -> Vec<&str> {
    delivered.iter().map(|d| d.message.id.as_str()).collect()
}

fn status(order: &str, value: &str) -> Publish {
    Publish::new(STATUS, json!({"order_id": order, "status": value})).key(order)
}

fn work(order: &str, token: &str) -> Publish {
    Publish::new(WORK, json!({"order_id": order, "task": "ship"}))
        .key(order)
        .header(SEED_DEDUPE_HEADER, token)
}

/// The stand's proof: an ordinary client publishes, and the control interface reports
/// the message exactly once, as sent and as stored.
#[tokio::test]
async fn a_publish_is_reported_exactly_once_by_the_control_interface() -> Outcome {
    let broker = TestBroker::start().await?;
    let mut outside = broker.client("outside").await?;
    let message = status("A-100", "shipped").header("correlation_id", "r-1");

    let published = outside.publish(&message).await?;
    assert_eq!((published.id.as_str(), published.position), ("m-1", None));

    let received = broker.received();
    assert_eq!(received.len(), 1);
    let one = &received[0];
    assert_eq!((one.client.as_str(), one.channel.as_str()), ("outside", STATUS));
    assert_eq!(one.key.as_deref(), Some("A-100"));
    assert_eq!(one.headers["correlation_id"], "r-1");
    assert_eq!(one.body, message.body);
    assert_eq!(one.body_digest, body_digest(&json!({"status": "shipped", "order_id": "A-100"})));
    let stored = PublishOutcome::Stored {
        id: "m-1".to_owned(),
        position: None,
        retained: true,
    };
    assert_eq!(one.outcome, stored);
    assert!(one.answered);

    let held = broker.retained(STATUS, "A-100").ok_or("nothing retained")?;
    assert_eq!((held.id.as_str(), &held.body), ("m-1", &message.body));
    assert!(broker.deliveries().is_empty());
    Ok(())
}

/// Listing, describing, browsing, and reading the latest value take nothing from anyone.
#[tokio::test]
async fn a_look_takes_nothing() -> Outcome {
    let broker = TestBroker::start().await?;
    let before = broker.truth().channels;
    let mut scout = broker.client("scout").await?;

    let channels = scout.channels().await?;
    let shapes: Vec<(&str, Shape)> = channels
        .iter()
        .map(|c| (c.spec.address.as_str(), c.spec.shape))
        .collect();
    let expected = [
        (EVENTS, Shape::Log),
        (NOTICES, Shape::Topic),
        (STATUS, Shape::Retained),
        (WORK, Shape::Queue),
    ];
    assert_eq!(shapes, expected);
    let events = scout.describe(EVENTS).await?;
    assert_eq!((events.held, events.start, events.end), (3, Some(0), Some(3)));
    assert_eq!(events.spec.key_field.as_deref(), Some("order_id"));
    assert!(events.spec.schema.is_some());
    assert_eq!(events.subscriptions, [SEED_SUBSCRIPTION]);

    let queue = scout.browse(WORK, 10, None).await?;
    assert_eq!(queue.messages.len(), 2);
    assert!(queue.messages.iter().all(|m| m.state == Some(HeldState::Ready)));
    let first = scout.browse(WORK, 1, None).await?;
    assert_eq!((first.messages.len(), first.more, first.held), (1, true, 2));
    let log = scout.browse(EVENTS, 10, None).await?;
    let positions: Vec<_> = log.messages.iter().map(|m| m.message.position).collect();
    assert_eq!(positions, [Some(0), Some(1), Some(2)]);
    assert_eq!(scout.browse(STATUS, 10, None).await?.messages.len(), 2);
    let latest = scout.latest(STATUS, "A-101").await?.ok_or("no value")?;
    assert_eq!(latest.body["status"], "submitted");
    assert_eq!(scout.latest(STATUS, "A-999").await?, None);
    // A topic keeps nothing, so there is nothing to look at, and the broker says so.
    assert_eq!(refused(scout.browse(NOTICES, 10, None).await), "not_supported");

    let truth = broker.truth();
    assert!(truth.deliveries.is_empty());
    assert_eq!(truth.channels, before);
    assert!(truth.requests.iter().any(|r| r.op == Op::Browse && r.client == "scout"));
    Ok(())
}

/// A queue hands each message to one consumer, and hands it over again until it is acknowledged.
#[tokio::test]
async fn a_queue_delivers_to_one_consumer_and_redelivers_what_is_not_acknowledged() -> Outcome {
    let broker = TestBroker::start().await?;
    let mut first = broker.client("first").await?;
    let mut second = broker.client("second").await?;
    let first_sub = first.subscribe(WORK).await?.subscription;
    let second_sub = second.subscribe(WORK).await?.subscription;

    let to_first = first.fetch(&first_sub, 1, NOW).await?;
    assert_eq!(ids(&to_first), ["s-6"]);
    let to_second = second.fetch(&second_sub, 10, NOW).await?;
    assert_eq!(ids(&to_second), ["s-7"]);
    second.ack(&second_sub, &to_second[0].delivery).await?;
    // A delivery belongs to the consumer it went to.
    let stolen = second.ack(&second_sub, &to_first[0].delivery).await;
    assert_eq!(refused(stolen), "unknown_delivery");

    let queue = broker.queue(WORK);
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].state, HeldState::InFlight);
    assert_eq!(queue[0].held_by.as_deref(), Some("first"));

    // The first consumer goes away without acknowledging: the broker takes the message back.
    drop(first);
    let ready = |t: &example_broker::model::Truth| {
        t.channels[WORK].queue.first().is_some_and(|h| h.state == HeldState::Ready)
    };
    broker.wait_for(LIMIT, ready).await?;
    let again = second.fetch(&second_sub, 10, NOW).await?;
    assert_eq!((ids(&again), again[0].attempt), (vec!["s-6"], 2));
    second.release(&second_sub, &again[0].delivery).await?;
    let third = second.fetch(&second_sub, 10, NOW).await?;
    assert_eq!((ids(&third), third[0].attempt), (vec!["s-6"], 3));
    second.ack(&second_sub, &third[0].delivery).await?;

    let truth = broker.truth();
    assert!(truth.channels[WORK].queue.is_empty());
    let history: Vec<_> = truth
        .deliveries_of("s-6")
        .iter()
        .map(|d| (d.subscription.as_str(), d.attempt, d.cause, d.outcome))
        .collect();
    let expected = [
        ("first", 1, DeliveryCause::First, DeliveryOutcome::Returned),
        ("second", 2, DeliveryCause::Redelivery, DeliveryOutcome::Released),
        ("second", 3, DeliveryCause::Redelivery, DeliveryOutcome::Acknowledged),
    ];
    assert_eq!(history, expected);
    let acknowledged: Vec<_> = truth.acknowledgements.iter().map(|a| a.message.as_str()).collect();
    assert_eq!(acknowledged, ["s-7", "s-6"]);
    Ok(())
}

/// A topic gives every subscription a copy and keeps nothing for one that comes later.
#[tokio::test]
async fn a_topic_gives_every_subscription_a_copy_and_keeps_nothing() -> Outcome {
    let broker = TestBroker::start().await?;
    let mut one = broker.client("one").await?;
    let mut two = broker.client("two").await?;
    let mut outside = broker.client("outside").await?;
    let one_sub = one.subscribe(NOTICES).await?.subscription;
    let two_sub = two.subscribe(NOTICES).await?.subscription;

    outside.publish(&Publish::new(NOTICES, json!("stock count tonight"))).await?;
    assert_eq!(ids(&one.fetch(&one_sub, 10, NOW).await?), ["m-1"]);
    assert_eq!(ids(&two.fetch(&two_sub, 10, NOW).await?), ["m-1"]);
    assert!(one.fetch(&one_sub, 10, NOW).await?.is_empty());

    let late = one.subscribe(NOTICES).await?.subscription;
    assert!(one.fetch(&late, 10, NOW).await?.is_empty());
    let contents = broker.inspect(NOTICES).ok_or("no channel")?;
    assert!(contents.queue.is_empty() && contents.retained.is_empty() && contents.log.is_empty());

    // The declared subscription collected its copy while nobody held it, and one
    // connection at a time may hold it.
    let named = Some(SEED_SUBSCRIPTION);
    let mine = two.subscribe_with(NOTICES, named, None, None).await?;
    assert_eq!(mine.name, SEED_SUBSCRIPTION);
    let taken = one.subscribe_with(NOTICES, named, None, None).await;
    assert_eq!(refused(taken), "subscription_in_use");
    let missing = one.subscribe_with(NOTICES, Some("example.nobody"), None, None).await;
    assert_eq!(refused(missing), "subscription_missing");
    assert_eq!(ids(&two.fetch(&mine.subscription, 10, NOW).await?), ["m-1"]);

    let to: Vec<_> = broker.deliveries().into_iter().map(|d| d.subscription).collect();
    assert_eq!(to.len(), 3);
    assert_eq!(to[2], SEED_SUBSCRIPTION);
    assert!(broker.deliveries().iter().all(|d| d.outcome == DeliveryOutcome::NotRequired));
    Ok(())
}

/// A retained channel keeps the last retained value per key; an unretained message passes through.
#[tokio::test]
async fn a_retained_channel_keeps_the_last_retained_value_per_key() -> Outcome {
    let broker = TestBroker::start().await?;
    let mut outside = broker.client("outside").await?;
    let mut reader = broker.client("reader").await?;
    let live = reader.subscribe_with(STATUS, None, None, Some(false)).await?.subscription;
    assert!(reader.fetch(&live, 10, NOW).await?.is_empty());

    outside.publish(&status("A-100", "shipped")).await?;
    outside.publish(&status("A-100", "draft").retain(false)).await?;
    assert_eq!(refused(outside.publish(&Publish::new(STATUS, json!({}))).await), "key_required");

    // Both went to the live subscription; only the retained one is the channel's value.
    assert_eq!(ids(&reader.fetch(&live, 10, NOW).await?), ["m-1", "m-2"]);
    let latest = reader.latest(STATUS, "A-100").await?.ok_or("no value")?;
    assert_eq!((latest.id.as_str(), &latest.body["status"]), ("m-1", &json!("shipped")));
    assert_eq!(broker.retained(STATUS, "A-100").map(|m| m.id), Some("m-1".to_owned()));

    // A subscription opened now is handed the retained values first, marked as such.
    let late = reader.subscribe(STATUS).await?.subscription;
    let handed = reader.fetch(&late, 10, NOW).await?;
    assert_eq!(ids(&handed), ["m-1", "s-2"]);
    assert!(handed.iter().all(|d| d.retained));

    assert_eq!(broker.expire_retention(STATUS), 2);
    assert_eq!(reader.latest(STATUS, "A-100").await?, None);
    Ok(())
}

/// A read is bounded: it ends with nothing when nothing arrives, and as soon as something does.
#[tokio::test]
async fn a_bounded_read_ends_with_nothing_or_as_soon_as_a_message_arrives() -> Outcome {
    let broker = TestBroker::start().await?;
    let mut reader = broker.client("reader").await?;
    let mut outside = broker.client("outside").await?;
    let sub = reader.subscribe(NOTICES).await?.subscription;

    // Nothing arriving is an answer, not a failure.
    assert!(reader.fetch(&sub, 5, Duration::from_millis(50)).await?.is_empty());
    let too_long = reader.fetch(&sub, 5, Duration::from_secs(61)).await;
    assert_eq!(refused(too_long), "wait_too_long");
    let unbounded = reader.request("fetch", json!({"subscription": sub})).await;
    assert_eq!(refused(unbounded), "bad_request");

    // A read that would wait a minute returns when a message arrives.
    reader
        .send("fetch", json!({"subscription": sub, "max": 5, "wait_ms": 60_000}))
        .await?;
    let waiting = |t: &example_broker::model::Truth| {
        t.requests.iter().filter(|r| r.op == Op::Fetch).count() == 4
    };
    broker.wait_for(LIMIT, waiting).await?;
    outside.publish(&Publish::new(NOTICES, json!("doors open"))).await?;
    let reply = tokio::time::timeout(LIMIT, reader.receive()).await??;
    assert_eq!(reply["messages"][0]["message"]["body"], "doors open");
    assert_eq!(reply["messages"].as_array().map(Vec::len), Some(1));
    Ok(())
}

/// A log keeps messages in order at positions it reports, until its retention runs out.
#[tokio::test]
async fn a_log_keeps_order_and_positions() -> Outcome {
    let broker = TestBroker::start().await?;
    let mut outside = broker.client("outside").await?;
    let mut reader = broker.client("reader").await?;
    let tail = reader.subscribe(EVENTS).await?;
    assert_eq!(tail.position, Some(3));

    for (order, expected) in [("A-102", 3), ("A-100", 4), ("A-102", 5)] {
        let event = Publish::new(EVENTS, json!({"order_id": order, "event": "shipped"})).key(order);
        assert_eq!(outside.publish(&event).await?.position, Some(expected));
    }

    let position = |d: &Delivered| d.message.position;
    let from_one = reader.subscribe_with(EVENTS, None, Some(1), None).await?.subscription;
    let head: Vec<_> = reader.fetch(&from_one, 2, NOW).await?.iter().map(position).collect();
    assert_eq!(head, [Some(1), Some(2)]);
    let rest: Vec<_> = reader.fetch(&from_one, 10, NOW).await?.iter().map(position).collect();
    assert_eq!(rest, [Some(3), Some(4), Some(5)]);
    assert!(reader.fetch(&from_one, 10, NOW).await?.is_empty());
    // A subscription opened at the end sees only what came after it.
    assert_eq!(ids(&reader.fetch(&tail.subscription, 10, NOW).await?), ["m-1", "m-2", "m-3"]);

    let page = reader.browse(EVENTS, 10, Some(4)).await?;
    let seen: Vec<_> = page.messages.iter().map(|m| m.message.position).collect();
    assert_eq!((seen, page.start, page.end), (vec![Some(4), Some(5)], Some(0), Some(6)));
    assert_eq!(broker.log(EVENTS).len(), 6);

    // Retention runs out: the positions carry on, and an old position is refused.
    assert_eq!(broker.expire_retention(EVENTS), 6);
    let described = reader.describe(EVENTS).await?;
    assert_eq!((described.held, described.start, described.end), (0, Some(6), Some(6)));
    let old = reader.subscribe_with(EVENTS, None, Some(2), None).await;
    assert_eq!(refused(old), "position_out_of_range");
    assert_eq!(refused(reader.browse(EVENTS, 10, Some(7)).await), "position_out_of_range");
    let next = Publish::new(EVENTS, json!({"order_id": "A-100", "event": "shipped"}));
    assert_eq!(outside.publish(&next).await?.position, Some(6));
    Ok(())
}

/// With a de-duplication header declared, a repeated token stores one message.
#[tokio::test]
async fn a_repeated_token_is_refused_and_one_message_is_stored() -> Outcome {
    let broker = TestBroker::start().await?;
    let mut outside = broker.client("outside").await?;

    assert_eq!(outside.publish(&work("A-102", "t-1")).await?.id, "m-1");
    let Err(ClientError::Refused { code, reply, .. }) = outside.publish(&work("A-102", "t-1")).await
    else {
        return Err("the repeat was accepted".into());
    };
    assert_eq!((code.as_str(), &reply["id"]), ("duplicate_token", &json!("m-1")));
    assert_eq!(broker.queue(WORK).len(), 3);
    let outcomes: Vec<_> = broker.received().into_iter().map(|r| r.outcome).collect();
    assert!(matches!(outcomes[0], PublishOutcome::Stored { .. }));
    assert_eq!(outcomes[1], PublishOutcome::Duplicate { of: "m-1".to_owned() });

    // The same broker, told to accept a repeat: now there are two.
    assert!(broker.set_dedupe_header(WORK, None));
    assert_eq!(outside.publish(&work("A-102", "t-1")).await?.id, "m-2");
    assert_eq!(broker.queue(WORK).len(), 4);
    Ok(())
}

/// The possible commit: the message is stored and the confirmation never comes.
#[tokio::test]
async fn a_confirmation_can_be_lost_after_the_message_was_stored() -> Outcome {
    let broker = TestBroker::start().await?;

    // Stored, then the connection is dropped.
    broker.set_fault(Fault::once(FaultKind::StoreThenDrop).on_channel(WORK));
    let mut sender = broker.client("sender").await?;
    let lost = sender.publish(&work("A-102", "t-9")).await;
    assert!(matches!(lost, Err(ClientError::Closed)), "got {lost:?}");
    let received = broker.received();
    assert!(matches!(received[0].outcome, PublishOutcome::Stored { .. }));
    assert!(!received[0].answered);
    assert_eq!(broker.queue(WORK).len(), 3);
    // A resumed publish with the same token delivers one message.
    let mut resumed = broker.client("sender").await?;
    assert_eq!(refused(resumed.publish(&work("A-102", "t-9")).await), "duplicate_token");
    assert_eq!(broker.queue(WORK).len(), 3);

    // Stored, and no confirmation on a connection that stays open: the next answer
    // on that connection is to the next request.
    broker.set_fault(Fault::once(FaultKind::StoreThenSilent));
    let silent = Publish::new(NOTICES, json!("no answer")).key("n-1");
    resumed.send("publish", json!({"channel": NOTICES, "body": silent.body})).await?;
    resumed.channels().await?;
    let last = broker.received().pop().ok_or("nothing received")?;
    assert!(matches!(last.outcome, PublishOutcome::Stored { .. }));
    assert!(!last.answered);

    // Dropped before anything was stored: the same sight from outside, another truth.
    broker.set_fault(Fault::once(FaultKind::DropBeforeStore));
    let dropped = resumed.publish(&status("A-100", "shipped")).await;
    assert!(matches!(dropped, Err(ClientError::Closed)), "got {dropped:?}");
    let last = broker.received().pop().ok_or("nothing received")?;
    assert_eq!(last.outcome, PublishOutcome::Dropped);
    assert_eq!(broker.retained(STATUS, "A-100").map(|m| m.id), Some("s-1".to_owned()));

    assert!(broker.faults().is_empty());
    let applied: Vec<_> = broker.applied_faults().into_iter().map(|a| a.kind).collect();
    let expected = [
        FaultKind::StoreThenDrop,
        FaultKind::StoreThenSilent,
        FaultKind::DropBeforeStore,
    ];
    assert_eq!(applied, expected);
    Ok(())
}

/// A refusal stores nothing; a delay makes the broker slow; a body over the limit is refused.
#[tokio::test]
async fn a_publish_can_be_refused_and_the_broker_can_be_slow() -> Outcome {
    let broker = TestBroker::start().await?;
    let mut outside = broker.client("outside").await?;

    let reason = "The channel is closed for stock count.";
    broker.set_fault(Fault::once(FaultKind::Refuse { reason: reason.to_owned() }));
    let Err(ClientError::Refused { code, message, .. }) = outside.publish(&work("A-102", "t-1")).await
    else {
        return Err("the publish was accepted".into());
    };
    assert_eq!((code.as_str(), message.as_str()), ("refused", reason));
    assert_eq!(broker.queue(WORK).len(), 2);
    let refusal = PublishOutcome::Refused { code: "refused".to_owned() };
    assert_eq!(broker.received()[0].outcome, refusal);

    broker.set_fault(Fault::once(FaultKind::Delay { ms: 300 }).on_op(Op::Publish));
    outside.channels().await?;
    assert_eq!(broker.faults().len(), 1, "a delay on publish applied to another request");
    let started = Instant::now();
    outside.publish(&work("A-102", "t-1")).await?;
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert_eq!(broker.applied_faults().len(), 2);
    assert!(broker.faults().is_empty());

    let mut small = ChannelSpec::new("example.orders.small", Shape::Topic);
    small.max_body_bytes = 8;
    broker.declare_channel(small);
    let large = Publish::new("example.orders.small", json!("more than eight bytes"));
    assert_eq!(refused(outside.publish(&large).await), "message_too_large");
    Ok(())
}

/// Delivery is at least once: the same message can come twice, looking the same.
#[tokio::test]
async fn a_message_can_be_delivered_twice() -> Outcome {
    let broker = TestBroker::start().await?;
    broker.set_fault(Fault::once(FaultKind::DuplicateDelivery).on_channel(WORK));
    broker.set_fault(Fault::once(FaultKind::DuplicateDelivery).on_channel(NOTICES));
    let mut consumer = broker.client("consumer").await?;

    let queue = consumer.subscribe(WORK).await?.subscription;
    let first = consumer.fetch(&queue, 1, NOW).await?;
    consumer.ack(&queue, &first[0].delivery).await?;
    let second = consumer.fetch(&queue, 1, NOW).await?;
    assert_eq!((ids(&first), ids(&second)), (vec!["s-6"], vec!["s-6"]));
    assert_eq!((first[0].attempt, second[0].attempt), (1, 1));
    assert_ne!(first[0].delivery, second[0].delivery);
    consumer.ack(&queue, &second[0].delivery).await?;
    assert_eq!(ids(&consumer.fetch(&queue, 1, NOW).await?), ["s-7"]);

    let topic = consumer.subscribe(NOTICES).await?.subscription;
    consumer.publish(&Publish::new(NOTICES, json!("once"))).await?;
    assert_eq!(ids(&consumer.fetch(&topic, 10, NOW).await?), ["m-1"]);
    assert_eq!(ids(&consumer.fetch(&topic, 10, NOW).await?), ["m-1"]);
    assert!(consumer.fetch(&topic, 10, NOW).await?.is_empty());

    let truth = broker.truth();
    let causes = |id: &str| -> Vec<_> { truth.deliveries_of(id).iter().map(|d| d.cause).collect() };
    assert_eq!(causes("s-6"), [DeliveryCause::First, DeliveryCause::Duplicate]);
    assert_eq!(causes("m-1"), [DeliveryCause::First, DeliveryCause::Duplicate]);
    assert_eq!(truth.acknowledgements.len(), 2);
    Ok(())
}

/// Another consumer holds messages and never acknowledges; the broker can take them back.
#[tokio::test]
async fn a_consumer_that_never_acknowledges_holds_messages_until_they_are_taken_back() -> Outcome {
    let broker = TestBroker::start().await?;
    broker.set_fault(Fault::times(FaultKind::SilentConsumer, 1).on_channel(WORK));
    let held = broker.queue(WORK);
    assert_eq!(held[0].state, HeldState::InFlight);
    assert_eq!(held[0].held_by.as_deref(), Some(SILENT_CONSUMER));

    let mut consumer = broker.client("consumer").await?;
    let sub = consumer.subscribe(WORK).await?.subscription;
    let got = consumer.fetch(&sub, 10, NOW).await?;
    assert_eq!(ids(&got), ["s-7"]);
    consumer.ack(&sub, &got[0].delivery).await?;
    let seen = consumer.browse(WORK, 10, None).await?;
    assert_eq!(seen.messages[0].state, Some(HeldState::InFlight));
    assert!(consumer.fetch(&sub, 10, NOW).await?.is_empty());

    // The silent consumer lets go: the message is delivered again, counted as a second time.
    broker.clear_faults();
    let again = consumer.fetch(&sub, 10, NOW).await?;
    assert_eq!((ids(&again), again[0].attempt), (vec!["s-6"], 2));

    // The broker's time limit for an acknowledgement runs out, on demand: the old
    // delivery can no longer be acknowledged, and the message comes a third time.
    assert_eq!(broker.redeliver_unacknowledged(WORK), 1);
    let late = consumer.ack(&sub, &again[0].delivery).await;
    assert_eq!(refused(late), "unknown_delivery");
    let third = consumer.fetch(&sub, 10, NOW).await?;
    assert_eq!((ids(&third), third[0].attempt), (vec!["s-6"], 3));
    let outcomes: Vec<_> = broker.truth().deliveries_of("s-6").iter().map(|d| d.outcome).collect();
    let expected = [
        DeliveryOutcome::Returned,
        DeliveryOutcome::Returned,
        DeliveryOutcome::Unacknowledged,
    ];
    assert_eq!(outcomes, expected);
    Ok(())
}

/// Drift: a message's shape changes, a channel changes shape, a channel goes away.
#[tokio::test]
async fn a_changed_message_or_channel_is_seen_by_a_reader() -> Outcome {
    let broker = TestBroker::start().await?;
    let mut reader = broker.client("reader").await?;

    let rename = FaultKind::RenameField {
        from: "status".to_owned(),
        to: "state".to_owned(),
    };
    broker.set_fault(Fault::until_cleared(rename).on_channel(STATUS));
    let seen = reader.latest(STATUS, "A-100").await?.ok_or("no value")?;
    assert_eq!(seen.body, json!({"order_id": "A-100", "state": "draft"}));
    let browsed = reader.browse(STATUS, 10, None).await?;
    assert!(browsed.messages.iter().all(|m| m.message.body.get("status").is_none()));
    // What is stored did not change; only what a reader is shown.
    let stored = broker.retained(STATUS, "A-100").ok_or("nothing retained")?;
    assert_eq!(stored.body["status"], "draft");
    broker.clear_faults();
    let seen = reader.latest(STATUS, "A-100").await?.ok_or("no value")?;
    assert_eq!(seen.body["status"], "draft");

    broker.declare_channel(ChannelSpec::new(STATUS, Shape::Log).key_field("customer_id"));
    let described = reader.describe(STATUS).await?;
    assert_eq!(described.spec.shape, Shape::Log);
    assert_eq!(described.spec.key_field.as_deref(), Some("customer_id"));
    assert_eq!(refused(reader.latest(STATUS, "A-100").await), "not_supported");

    assert!(broker.remove_channel(EVENTS));
    assert_eq!(refused(reader.describe(EVENTS).await), "channel_missing");
    let gone = reader.publish(&Publish::new(EVENTS, json!({}))).await;
    assert_eq!(refused(gone), "channel_missing");
    let refusal = PublishOutcome::Refused { code: "channel_missing".to_owned() };
    assert_eq!(broker.received()[0].outcome, refusal);
    assert_eq!(refused(reader.request("explode", json!({})).await), "unknown_op");
    Ok(())
}

/// The broker can ask for a token, and can stop accepting one it accepted a moment ago.
#[tokio::test]
async fn credentials_can_be_required_and_can_stop_working() -> Outcome {
    let broker = TestBroker::start().await?;
    broker.set_required_token(Some("example-token"));
    assert_eq!(refused(broker.client("visitor").await), "not_authorized");
    let mut member = Client::connect_with(broker.address(), "member", Some("example-token")).await?;
    member.channels().await?;

    broker.set_required_token(Some("example-token-2"));
    assert_eq!(refused(member.channels().await), "not_authorized");

    // A token is never part of what the control interface reports.
    let truth: Value = serde_json::to_value(broker.truth())?;
    assert!(!truth.to_string().contains("example-token"));
    Ok(())
}

/// Every connection can be dropped at once, and a reset restores the seed data.
#[tokio::test]
async fn connections_can_be_dropped_and_a_reset_restores_the_seed_data() -> Outcome {
    let broker = TestBroker::start().await?;
    let mut consumer = broker.client("consumer").await?;
    let sub = consumer.subscribe(WORK).await?.subscription;
    assert_eq!(consumer.fetch(&sub, 1, NOW).await?.len(), 1);

    broker.drop_connections();
    let truth = broker.wait_for(LIMIT, |t| t.connections.is_empty()).await?;
    let lost = consumer.channels().await;
    assert!(matches!(lost, Err(ClientError::Closed)), "got {lost:?}");
    // What the broker held stays, and the unacknowledged message is ready again.
    let queue = &truth.channels[WORK].queue;
    assert_eq!((queue.len(), queue[0].state, queue[0].attempts), (2, HeldState::Ready, 1));

    let mut outside = broker.client("outside").await?;
    outside.publish(&status("A-102", "draft")).await?;
    broker.set_fault(Fault::until_cleared(FaultKind::Delay { ms: 1 }));
    broker.reset();
    let truth = broker.truth();
    let fresh = TestBroker::start().await?;
    assert_eq!(truth.channels, fresh.truth().channels);
    assert!(truth.received.is_empty() && truth.faults.is_empty() && truth.requests.is_empty());
    assert_eq!(outside.publish(&status("A-102", "draft")).await?.id, "m-1");
    // Two brokers in one test share nothing.
    assert!(fresh.received().is_empty());
    Ok(())
}
