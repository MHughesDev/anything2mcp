//! The client end of a command: it finds the local engine, sends each
//! action to its REST route over the local channel, and hands the answer
//! to the renderer. It holds no engine logic and looks at nothing in a
//! response but its exit class, which the engine sends as a header.

use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use a2m_actions::{ExitClass, Paths, REST_PREFIX};
use a2m_api::EXIT_CLASS_HEADER;
use a2m_core::{Failure, FailureClass};
use a2m_local::{ClientLabel, Connector, EngineLock, Link, codes as local_codes};
use a2m_mcp::{HttpRequest, HttpResponse};
use axum::body::{Body, BodyDataStream, Bytes};
use axum::http::{Method, Request, header};
use serde_json::Value as Json;

use super::Finished;
use super::parse::{Globals, Step, Tree};
use super::render;
use crate::local::program_version;
use crate::wire;

/// The exit code of a command whose engine could not be reached.
pub(crate) const EXIT_UNREACHABLE: u8 = 4;

/// The most an answer of the engine may hold. The engine caps a result
/// well below this; the bound is there so that nothing reads without one.
const ANSWER_LIMIT: usize = 256 * 1024 * 1024;

/// How long a command waits for an engine that holds the engine lock and
/// does not answer: a `serve` that is starting or closing, or another
/// command that runs the engine inside its own process.
const STARTING_WAIT: Duration = Duration::from_secs(20);
/// The pause between two looks at the channel during that wait.
const STARTING_PAUSE: Duration = Duration::from_millis(25);

/// What a command found on this machine.
pub(crate) enum Found {
    /// An engine answers on the local channel.
    Engine(Link),
    /// No engine runs for this user and data directory, and the engine
    /// lock is now this command's: it holds it for as long as it runs the
    /// engine inside itself, so that no second engine opens the store.
    Nothing(EngineLock),
}

fn unreachable(failure: &Failure) -> bool {
    *failure.code() == local_codes::ENGINE_UNREACHABLE
}

/// Looks for the local engine. When none answers, the engine lock is
/// taken, in the same step that finds it free, and handed back: nothing
/// can open the store between the look and the open. An engine that holds
/// the lock and does not answer is waited for, for a bounded time: a
/// `serve` that is starting or closing, or another command that runs the
/// engine inside itself. Nothing is started.
///
/// # Errors
///
/// What `Connector::connect` answers for an engine that cannot be used,
/// `busy`/`engine_unreachable` for a lock that was held for the whole
/// wait, and what `EngineLock::take` answers for a lock file that cannot
/// be opened.
pub(crate) async fn find(paths: &Paths, label: ClientLabel) -> Result<Found, Failure> {
    let connector = Connector::new(paths, program_version()?, label)?;
    let give_up = tokio::time::Instant::now() + STARTING_WAIT;
    loop {
        match connector.connect().await {
            Ok(link) => return Ok(Found::Engine(link)),
            Err(failure) if unreachable(&failure) => {
                let lock_paths = paths.clone();
                let taken = tokio::task::spawn_blocking(move || EngineLock::take(&lock_paths))
                    .await
                    .map_err(|_| {
                        Failure::new(
                            FailureClass::Busy,
                            local_codes::CHANNEL_UNAVAILABLE,
                            "The engine lock could not be taken: the task that took it stopped.",
                            "Run the command again.",
                        )
                    })?;
                match taken {
                    Ok(lock) => return Ok(Found::Nothing(lock)),
                    Err(failure) if *failure.code() != local_codes::ENGINE_RUNNING => {
                        return Err(failure);
                    }
                    Err(_) => {}
                }
                if tokio::time::Instant::now() >= give_up {
                    return Err(Failure::new(
                        FailureClass::Busy,
                        local_codes::ENGINE_UNREACHABLE,
                        "An engine holds this data directory and does not answer on the local channel: a `serve` that is starting or closing, or another command that runs the engine inside itself.",
                        "Run the command again once the other command has ended. If it stays so, stop that engine's process and start one with `anything2mcp serve`, which commands then share.",
                    ));
                }
                // Not a wait for a fixed time: the channel and the lock
                // are looked at again until one of them answers, or the
                // time is up.
                tokio::time::sleep(STARTING_PAUSE).await;
            }
            Err(failure) => return Err(failure),
        }
    }
}

/// What the engine answered to one action: the response as JSON, and its
/// exit class.
struct Answered {
    body: Json,
    exit: ExitClass,
}

fn exit_class(word: Option<&str>) -> ExitClass {
    [
        ExitClass::Succeeded,
        ExitClass::Failed,
        ExitClass::Malformed,
        ExitClass::PersonNeeded,
    ]
    .into_iter()
    .find(|class| Some(class.as_str()) == word)
    // An answer that names no exit class is not one this client can
    // call a success.
    .unwrap_or(ExitClass::Failed)
}

/// The engine had the request, and its answer cannot be read: the
/// request is in doubt, as one whose answer was lost.
fn not_an_answer() -> Failure {
    Failure::new(
        FailureClass::PossibleCommit,
        local_codes::ANSWER_LOST,
        "The engine had the request, and its answer could not be read: what it asked may have been done, and a write may have started a run.",
        "Do not run the command again before you know what became of it: a second run of it may start a second run and write twice. Find it first with `anything2mcp run list`.",
    )
}

/// What the engine answered, as the response it is. An answer of the
/// channel itself, which refused the request before any part of the engine
/// saw it, has its four parts and no `ok`: it becomes the failure it
/// names. Anything else that is not a response is no answer.
fn response_of(body: Json) -> Result<Json, Failure> {
    if body.get("ok").is_some_and(Json::is_boolean) {
        return Ok(body);
    }
    let part = |name: &str| body.get(name).and_then(Json::as_str).map(str::to_owned);
    match (part("class"), part("code"), part("message"), part("fix")) {
        (Some(class), Some(code), Some(message), Some(fix)) => Ok(serde_json::json!({
            "ok": false,
            "action": "",
            "error": {
                "class": class,
                "code": code,
                "message": message,
                "fix": fix,
                // The channel refuses before anything is dispatched.
                "retry_safe": true,
            },
            "warnings": [],
        })),
        _ => Err(not_an_answer()),
    }
}

/// Sends one action to its REST route and reads the answer.
async fn send(
    link: &Link,
    step: &Step,
    globals: &Globals,
    project: Option<&Path>,
) -> Result<Answered, Failure> {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(format!("{REST_PREFIX}/actions/{}", step.action))
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(project) = project {
        let folder = wire::encode_path(project).ok_or_else(|| {
            Failure::new(
                FailureClass::Input,
                a2m_actions::codes::PATH_NOT_ALLOWED,
                "The project folder's path is not text, so it cannot be named to the engine.",
                "Move the project to a folder whose path is plain text.",
            )
        })?;
        request = request.header(wire::PROJECT_HEADER, folder);
    }
    if let Some(key) = &globals.idempotency_key {
        request = request.header("idempotency-key", key);
    }
    let request = request
        .body(Body::from(step.input.to_string()))
        .map_err(|_| {
            Failure::new(
                FailureClass::Input,
                a2m_actions::codes::SCHEMA_INVALID,
                "The command holds a value that cannot be sent to the engine.",
                "Write the idempotency key as plain printable characters.",
            )
        })?;
    let answer = link.send(request).await?;
    let exit = exit_class(
        answer
            .headers()
            .get(EXIT_CLASS_HEADER)
            .and_then(|value| value.to_str().ok()),
    );
    let bytes = axum::body::to_bytes(answer.into_body(), ANSWER_LIMIT)
        .await
        .map_err(|_| not_an_answer())?;
    let body: Json = serde_json::from_slice(&bytes).map_err(|_| not_an_answer())?;
    let body = response_of(body)?;
    Ok(Answered { body, exit })
}

/// A failure of the way to the engine, as a command ends on it: rendered
/// like any other, with the exit code that says the engine was not
/// reached.
pub(crate) fn not_reached(tree: &Tree, action: &str, failure: &Failure, json: bool) -> Finished {
    let mut finished = render::render(
        &render::refused(action, failure, ExitClass::Failed),
        tree,
        json,
    );
    finished.code = EXIT_UNREACHABLE;
    finished
}

/// Sends the steps of one command to the engine in turn and renders the
/// last answer: the first that is not a success, or the final one. The
/// same rule as a command that runs the engine in its own process.
pub(crate) async fn run_steps(
    link: &Link,
    tree: &Tree,
    globals: &Globals,
    steps: Vec<Step>,
    json: bool,
    project: Option<&Path>,
) -> Finished {
    let mut last = None;
    for step in steps {
        match send(link, &step, globals, project).await {
            Ok(answered) => {
                let go_on = answered.exit == ExitClass::Succeeded;
                last = Some(answered);
                if !go_on {
                    break;
                }
            }
            Err(failure) if failure.class() == FailureClass::Input => {
                return render::render(
                    &render::refused(&step.action, &failure, ExitClass::Malformed),
                    tree,
                    json,
                );
            }
            Err(failure) => return not_reached(tree, &step.action, &failure, json),
        }
    }
    match last {
        Some(answered) => render::show(&answered.body, answered.exit.exit_code(), tree, json),
        None => Finished {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        },
    }
}

/// The body of an answer, with the connection it arrives on. The link is
/// kept until the body has been read or dropped: dropping it closes the
/// connection, which is how the engine learns that a caller has gone.
struct Held {
    body: BodyDataStream,
    _link: Link,
}

impl futures_core::Stream for Held {
    type Item = Result<Bytes, axum::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let next = ready!(Pin::new(&mut self.body).poll_next(cx));
        Poll::Ready(next)
    }
}

/// Carries one request of the stdio proxy to the engine, on a connection
/// of its own, so that a call that takes long holds up no other message.
/// Nothing is kept between requests: an engine that was started, or
/// started again, since the last one is simply found.
///
/// # Errors
///
/// What `Connector::connect` and `Link::send` answer.
pub(crate) async fn carry(
    connector: &Connector,
    request: HttpRequest,
) -> Result<HttpResponse, Failure> {
    let link = connector.connect().await?;
    let (parts, body) = link.send(request).await?.into_parts();
    let held = Held {
        body: body.into_data_stream(),
        _link: link,
    };
    Ok(HttpResponse::from_parts(parts, Body::from_stream(held)))
}

#[cfg(test)]
mod tests {
    use a2m_actions::ExitClass;

    use super::exit_class;

    #[test]
    fn the_exit_class_is_read_from_its_word_and_anything_else_is_a_failure() {
        for class in [
            ExitClass::Succeeded,
            ExitClass::Failed,
            ExitClass::Malformed,
            ExitClass::PersonNeeded,
        ] {
            assert_eq!(exit_class(Some(class.as_str())), class);
        }
        assert_eq!(exit_class(Some("fine")), ExitClass::Failed);
        assert_eq!(exit_class(None), ExitClass::Failed);
    }
}
