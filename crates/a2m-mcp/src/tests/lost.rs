//! The proxy against an engine of the test's own that takes a request and
//! then goes away: a lost answer to a tool call is a write in doubt, never
//! "run it again"; a request refused before it was sent may be made again.
//! And a line longer than a request may be is refused without being kept.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2m_core::{Failure, FailureClass, FailureCode};
use a2m_policy::ToolsetName;
use axum::body::{Body, Bytes};
use axum::http::{Response, StatusCode};
use serde_json::{Value as Json, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

use crate::{HttpRequest, HttpResponse, LINE_LIMIT, stdio_proxy};

const PATIENCE: Duration = Duration::from_secs(60);

/// How a fake engine answers.
#[derive(Clone, Copy)]
enum Engine {
    /// It takes the request and drops the connection with no answer: the
    /// local channel says `answer_lost`.
    Drops,
    /// Nothing answers: refused before anything was sent.
    Unreachable,
    /// The connection ended before the request was sent: the local
    /// channel says `connection_lost`.
    EndedFirst,
    /// It answers a stream with a note of progress, then breaks off.
    BreaksOff,
    /// It answers with a status and an empty body.
    Status(u16),
    /// It answers a JSON response.
    Answers,
}

struct Proxy {
    input: DuplexStream,
    output: BufReader<DuplexStream>,
    taken: Arc<AtomicUsize>,
}

fn start(engine: Engine) -> Proxy {
    let taken = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taken);
    let upstream = move |request: HttpRequest| {
        let counted = Arc::clone(&counted);
        async move {
            match engine {
                Engine::Unreachable => {
                    return Err(Failure::new(
                        FailureClass::Busy,
                        FailureCode::fixed("engine_unreachable"),
                        "No engine is running.",
                        "Start one.",
                    ));
                }
                Engine::EndedFirst => {
                    return Err(Failure::new(
                        FailureClass::Transient,
                        FailureCode::fixed("connection_lost"),
                        "The connection to the engine ended before the request was sent.",
                        "Run the command again once an engine answers.",
                    ));
                }
                _ => {}
            }
            let body = axum::body::to_bytes(request.into_body(), usize::MAX)
                .await
                .unwrap();
            counted.fetch_add(1, Ordering::SeqCst);
            let message: Json = serde_json::from_slice(&body).unwrap();
            let response: HttpResponse = match engine {
                Engine::Drops => {
                    return Err(Failure::new(
                        FailureClass::PossibleCommit,
                        FailureCode::fixed("answer_lost"),
                        "The engine may have had the request, and its answer was lost.",
                        "Do not send it again before you know what became of it.",
                    ));
                }
                Engine::BreaksOff => {
                    let note = json!({"jsonrpc": "2.0", "method": "notifications/progress",
                        "params": {"progressToken": "t", "progress": 1, "message": "The call was handed to the engine."}});
                    let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
                        Ok(Bytes::from(format!("data: {note}\n\n"))),
                        Err(std::io::Error::other("the engine went away")),
                    ];
                    Response::builder()
                        .header("content-type", "text/event-stream")
                        .body(Body::from_stream(futures_stream(chunks)))
                        .unwrap()
                }
                Engine::Status(status) => Response::builder()
                    .status(StatusCode::from_u16(status).unwrap())
                    .body(Body::empty())
                    .unwrap(),
                Engine::Answers => Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({"jsonrpc": "2.0", "id": message["id"], "result": {}}).to_string(),
                    ))
                    .unwrap(),
                Engine::Unreachable | Engine::EndedFirst => unreachable!(),
            };
            Ok(response)
        }
    };
    let (input, proxy_in) = tokio::io::duplex(1 << 16);
    let (proxy_out, output) = tokio::io::duplex(1 << 16);
    let toolset = ToolsetName::new("use").unwrap();
    tokio::spawn(async move { stdio_proxy(upstream, &toolset, proxy_in, proxy_out).await });
    Proxy {
        input,
        output: BufReader::new(output),
        taken,
    }
}

/// A body stream from a list of chunks.
fn futures_stream(
    chunks: Vec<Result<Bytes, std::io::Error>>,
) -> impl futures_core::Stream<Item = Result<Bytes, std::io::Error>> + Send {
    struct Chunks(std::vec::IntoIter<Result<Bytes, std::io::Error>>);
    impl futures_core::Stream for Chunks {
        type Item = Result<Bytes, std::io::Error>;
        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            std::task::Poll::Ready(self.0.next())
        }
    }
    Chunks(chunks.into_iter())
}

impl Proxy {
    async fn write(&mut self, message: &Json) {
        self.input
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
    }

    async fn read(&mut self) -> Json {
        let mut line = Vec::new();
        tokio::time::timeout(PATIENCE, self.output.read_until(b'\n', &mut line))
            .await
            .expect("the proxy answered")
            .unwrap();
        serde_json::from_slice(&line).expect("one JSON message per line")
    }

    /// Sends `message` and returns the error that answers it, after any
    /// notes before it.
    async fn error_for(&mut self, message: &Json) -> Json {
        self.write(message).await;
        loop {
            let answer = self.read().await;
            if answer.get("id").is_some() {
                assert_eq!(answer["id"], message["id"], "{answer}");
                assert_eq!(answer["error"]["code"], -32000, "{answer}");
                return answer["error"].clone();
            }
        }
    }
}

fn tool_call(id: i64) -> Json {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": "example_items_item_upsert", "arguments": {"code": "L-1"}}})
}

fn assert_in_doubt(error: &Json) {
    let data = &error["data"];
    assert_eq!(data["class"], "possible_commit", "{error}");
    assert_eq!(data["code"], "answer_lost", "{error}");
    assert_eq!(data["retry_safe"], false, "{error}");
    let fix = data["fix"].as_str().unwrap();
    assert!(fix.starts_with("Do not call the tool again"), "{fix}");
    assert!(!fix.contains("Run the command again"), "{fix}");
}

#[tokio::test]
async fn a_tool_call_whose_answer_is_lost_after_the_engine_took_it_is_a_write_in_doubt() {
    let mut proxy = start(Engine::Drops);
    let error = proxy.error_for(&tool_call(1)).await;
    assert_eq!(proxy.taken.load(Ordering::SeqCst), 1);
    assert_in_doubt(&error);

    // A list writes nothing: the same loss may be asked again.
    let error = proxy
        .error_for(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
        .await;
    assert_eq!(error["data"]["code"], "answer_lost", "{error}");
    assert_eq!(error["data"]["retry_safe"], true, "{error}");
}

#[tokio::test]
async fn a_tool_call_refused_before_it_was_sent_may_be_made_again() {
    for (engine, code) in [
        (Engine::Unreachable, "engine_unreachable"),
        (Engine::EndedFirst, "connection_lost"),
    ] {
        let mut proxy = start(engine);
        let error = proxy.error_for(&tool_call(1)).await;
        assert_eq!(proxy.taken.load(Ordering::SeqCst), 0);
        assert_eq!(error["data"]["code"], code, "{error}");
        assert_eq!(error["data"]["retry_safe"], true, "{error}");
    }
}

#[tokio::test]
async fn a_stream_that_breaks_off_after_a_note_is_still_answered_and_in_doubt() {
    let mut proxy = start(Engine::BreaksOff);
    proxy.write(&tool_call(7)).await;
    let note = proxy.read().await;
    assert_eq!(note["method"], "notifications/progress", "{note}");
    // The note is not the answer: the call is answered all the same.
    let answer = proxy.read().await;
    assert_eq!(answer["id"], 7, "{answer}");
    assert_in_doubt(&answer["error"]);
}

#[tokio::test]
async fn an_answer_that_is_no_message_is_in_doubt_after_a_dispatch_and_not_before_one() {
    let mut proxy = start(Engine::Status(500));
    assert_in_doubt(&proxy.error_for(&tool_call(1)).await);
    // The endpoint's own refusals come before any dispatch.
    let mut proxy = start(Engine::Status(401));
    let error = proxy.error_for(&tool_call(2)).await;
    assert_eq!(error["data"]["status"], 401, "{error}");
    assert_eq!(error["data"]["retry_safe"], true, "{error}");
}

#[tokio::test]
async fn a_line_longer_than_a_request_may_be_is_refused_and_the_next_line_is_carried() {
    let mut proxy = start(Engine::Answers);
    let long = vec![b'x'; LINE_LIMIT + 10];
    let mut input = proxy.input;
    let writer = tokio::spawn(async move {
        input.write_all(&long).await.unwrap();
        input
            .write_all(b"\n{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/list\"}\n")
            .await
            .unwrap();
        input
    });
    let mut line = Vec::new();
    let output = &mut proxy.output;
    tokio::time::timeout(PATIENCE, output.read_until(b'\n', &mut line))
        .await
        .unwrap()
        .unwrap();
    let refused: Json = serde_json::from_slice(&line).unwrap();
    assert_eq!(refused["error"]["code"], -32600, "{refused}");
    assert_eq!(refused["id"], Json::Null);
    line.clear();
    tokio::time::timeout(PATIENCE, output.read_until(b'\n', &mut line))
        .await
        .unwrap()
        .unwrap();
    let answered: Json = serde_json::from_slice(&line).unwrap();
    assert_eq!(answered["id"], 3, "{answered}");
    // The long line never reached the engine.
    assert_eq!(proxy.taken.load(Ordering::SeqCst), 1);
    drop(writer.await.unwrap());
}
