//! What the process logs when the MCP endpoint is used, at the most
//! verbose level. The endpoint's protocol layer is this crate's own and
//! logs nothing; this holds it to that, so that a caller's input, a
//! result, or a token cannot reach a log through this door.
//!
//! This test is a program of its own, because which events a subscriber is
//! offered is decided once for the whole process: beside other tests, the
//! capture would be incomplete and the proof would be of nothing.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a test; it shares its helpers with the crate's own tests and uses some of them"
)]

// The helpers are the engine's file: this crate's formatter leaves it be.
#[rustfmt::skip]
#[path = "../../a2m-engine/tests/common/mod.rs"]
mod engine;
#[path = "../src/tests/wire.rs"]
mod wire;
#[path = "../src/tests/world.rs"]
mod world;

use std::sync::{Arc, Mutex};

use a2m_mcp::{HttpRequest, McpPeer, McpService, McpSettings, mcp_path, stdio_proxy};
use serde_json::{Value as Json, json};

use engine::ORDER_GET;
use wire::{Http, Stdio, h, m};
use world::World;

const SECRET: &str = "Example Customer Two";

fn call(id: i64, name: &str, arguments: Json) -> Json {
    h(
        id,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    )
}

/// Sends one request through the proxy and waits for its answer.
async fn ask_lines(stdio: &mut Stdio, message: &Json) {
    stdio.write(message.to_string().as_bytes()).await;
    while stdio.message().await.get("id").is_none() {}
}

/// Plants the secret: the test application is made to answer with a value
/// the vault has released as a secret, the customer's name of one order.
async fn plant(world: &World) {
    let stored = world
        .direct(
            "secret.set",
            json!({"name": "example_password", "value": SECRET}),
        )
        .await;
    assert!(stored.ok, "{:?}", stored.error);
}

/// Every event and span the process logs, at the most verbose level, as
/// text. The SDK logs through the `tracing` crate.
struct Captured(Arc<Mutex<String>>);

struct Fields<'a>(&'a mut String);

impl tracing::field::Visit for Fields<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }
}

impl tracing::Subscriber for Captured {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        span.record(&mut Fields(&mut self.0.lock().unwrap()));
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _span: &tracing::span::Id, values: &tracing::span::Record<'_>) {
        values.record(&mut Fields(&mut self.0.lock().unwrap()));
    }

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut text = self.0.lock().unwrap();
        text.push_str(event.metadata().target());
        event.record(&mut Fields(&mut text));
        text.push('\n');
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}

/// One thread, so that everything the endpoint logs is logged where the
/// capture is.
#[tokio::test]
async fn the_log_at_its_most_verbose_holds_nothing_that_passed_through_the_door() {
    let log = Arc::new(Mutex::new(String::new()));
    let _guard = tracing::subscriber::set_default(Captured(Arc::clone(&log)));
    let world = World::ready().await;
    plant(&world).await;
    let token = "example-token-for-the-log-test";
    let marker = "example-marker-sent-by-the-caller";
    let service = world.service("use", McpSettings::local());
    let http = Http {
        service: service.clone(),
        peer: Some(McpPeer {
            user: world::USER.to_owned(),
            label: "label-of-the-log-test".to_owned(),
        }),
    };
    let mut stdio = Stdio::start(service);
    for message in [
        call(
            1,
            "a2m_tool_call",
            json!({"tool": ORDER_GET, "input": {"order_id": "A-101"}}),
        ),
        m(
            2,
            "tools/call",
            json!({"name": ORDER_GET, "arguments": {"order_id": "A-101"}, "_meta": {"progressToken": 1}}),
        ),
        call(3, "a2m_tool_search", json!({"query": marker})),
    ] {
        // A token beside a peer the channel named is dropped unread.
        let bearer = format!("Bearer {token}");
        let reply = http
            .post_with(&message, &[("authorization", &bearer)])
            .await;
        assert_eq!(reply.status, 200);
        ask_lines(&mut stdio, &message).await;
    }
    let (_, ended) = stdio.close().await;
    ended.unwrap();

    tracing::info!("the capture is listening");
    let logged = log.lock().unwrap().clone();
    // The capture works: an event made on this thread is in it.
    assert!(logged.contains("the capture is listening"), "{logged}");
    // This crate logs nothing of its own.
    assert!(!logged.contains("a2m_mcp"), "{logged}");
    // Nothing that passed through the door is in the log: no secret of
    // the engine, no token, and none of the caller's input.
    assert!(!logged.contains(SECRET), "{logged}");
    assert!(!logged.contains(token), "{logged}");
    assert!(!logged.contains(marker), "{logged}");
}
