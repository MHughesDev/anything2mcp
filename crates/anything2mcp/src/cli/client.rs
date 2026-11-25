//! The client end of a command: it finds the local engine, sends each
//! action to its REST route over the local channel, and hands the answer
//! to the renderer. It holds no engine logic and looks at nothing in a
//! response but its exit class, which the engine sends as a header.

use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use a2m_actions::{ExitClass, Paths, REST_PREFIX};
use a2m_api::EXIT_CLASS_HEADER;
use a2m_core::{Failure, FailureClass};
use a2m_local::{ClientLabel, Connector, EngineLock, Launch, Link, Started, codes as local_codes};
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
/// How long a client waits for an engine it starts on demand to answer:
/// the start of the program and the opening of its kernel.
const START_WAIT: Duration = Duration::from_secs(30);

/// What a command found on this machine.
pub(crate) enum Found {
    /// An engine answers on the local channel: it ran already, or was
    /// started on demand.
    Engine(Link),
    /// No engine runs for this user and data directory, none was started,
    /// and the engine lock is now this command's: it holds it for as long
    /// as it runs the engine inside itself, so that no second engine opens
    /// the store. The failure, when there is one, is why an engine that was
    /// to be started on demand could not be: the operating system refused
    /// to start a process.
    Nothing(EngineLock, Option<Failure>),
}

fn unreachable(failure: &Failure) -> bool {
    *failure.code() == local_codes::ENGINE_UNREACHABLE
}

/// Looks for the local engine, and when none answers starts one on demand
/// as `launch` says, when it says anything (decision 0165).
///
/// When none is started, the engine lock is taken, in the same step that
/// finds it free, and handed back: nothing can open the store between the
/// look and the open. An engine that holds the lock and does not answer
/// is waited for, for a bounded time: a `serve` that is starting or
/// closing, or another command that runs the engine inside itself.
///
/// # Errors
///
/// What `Connector::connect` answers for an engine that cannot be used,
/// `busy`/`engine_unreachable` for a lock that was held for the whole
/// wait or an engine started on demand that never answered, and what
/// `EngineLock::take` answers for a lock file that cannot be opened.
pub(crate) async fn find(
    paths: &Paths,
    label: ClientLabel,
    launch: impl FnOnce() -> Option<Launch>,
) -> Result<Found, Failure> {
    let connector = Connector::new(paths, program_version()?, label)?;
    match connector.connect().await {
        Ok(link) => return Ok(Found::Engine(link)),
        Err(failure) if unreachable(&failure) => {}
        Err(failure) => return Err(failure),
    }
    let mut refused = None;
    if let Some(launch) = launch() {
        match connector
            .connect_or_start(paths, launch, START_WAIT)
            .await?
        {
            Started::Found(link) | Started::Launched(link) => return Ok(Found::Engine(link)),
            // A machine where a detached start is refused: the command
            // runs the engine inside itself, as `--in-process` does.
            Started::Refused(failure) => refused = Some(failure),
        }
    }
    let give_up = tokio::time::Instant::now() + STARTING_WAIT;
    loop {
        match connector.connect().await {
            Ok(link) => return Ok(Found::Engine(link)),
            Err(failure) if unreachable(&failure) => {
                let lock_paths = paths.clone();
                let taken = tokio::task::spawn_blocking(move || {
                    let taken = EngineLock::take(&lock_paths);
                    // An engine of another runtime directory never answers
                    // on this one's channel: it is not waited for, and its
                    // refusal says where it is (decision 0432).
                    let elsewhere = match &taken {
                        Err(failure) if *failure.code() == local_codes::ENGINE_RUNNING => {
                            EngineLock::elsewhere(&lock_paths).ok().flatten()
                        }
                        _ => None,
                    };
                    (taken, elsewhere)
                })
                .await
                .map_err(|_| {
                    Failure::new(
                        FailureClass::Busy,
                        local_codes::CHANNEL_UNAVAILABLE,
                        "The engine lock could not be taken: the task that took it stopped.",
                        "Run the command again.",
                    )
                })?;
                let taken = match taken {
                    (_, Some(elsewhere)) => return Err(elsewhere),
                    (taken, None) => taken,
                };
                match taken {
                    Ok(lock) => return Ok(Found::Nothing(lock, refused)),
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

/// Starts the engine on demand for a client that cannot run it inside
/// itself, the bridge, and connects to it.
///
/// # Errors
///
/// What `Connector::connect_or_start` answers, and the refusal of the
/// operating system to start a process, as `busy`/`engine_unreachable`.
pub(crate) async fn start(
    connector: &Connector,
    paths: &Paths,
    launch: Launch,
) -> Result<(Link, bool), Failure> {
    match connector
        .connect_or_start(paths, launch, START_WAIT)
        .await?
    {
        Started::Found(link) => Ok((link, false)),
        Started::Launched(link) => Ok((link, true)),
        Started::Refused(failure) => Err(Failure::new(
            FailureClass::Busy,
            local_codes::ENGINE_UNREACHABLE,
            format!(
                "No engine answers on the local channel, and this machine refused to start one on demand: {}",
                failure.message()
            ),
            "Start the engine with `anything2mcp serve`, then try again.",
        )),
    }
}

/// What the engine answered to one action: the response as JSON, and its
/// exit class.
#[derive(Debug)]
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

/// What a command sent, as far as the advice after a lost answer needs it.
struct Sent<'a> {
    /// The command carried an idempotency key.
    keyed: bool,
    /// The tool the request named, for a `tool.call`.
    tool: Option<&'a str>,
    /// When the request was sent, in milliseconds since 1970.
    at_ms: u128,
}

impl<'a> Sent<'a> {
    fn of(step: &'a Step, globals: &Globals, at_ms: u128) -> Self {
        Self {
            keyed: globals.idempotency_key.is_some(),
            tool: if step.action == "tool.call" {
                step.input.get("tool").and_then(Json::as_str)
            } else {
                None
            },
            at_ms,
        }
    }
}

/// The engine had the request, and its answer cannot be read: the
/// request is in doubt, as one whose answer was lost. With a key the
/// command is run again as it was; without one, the run is looked up
/// from a minute before the request was sent.
fn not_an_answer(sent: &Sent<'_>) -> Failure {
    let next = if sent.keyed {
        "Run the same command again with the same `--idempotency-key` and nothing changed: it gives back the run the first one started, finished or not, and starts no other.".to_owned()
    } else {
        let since = sent.at_ms.saturating_sub(60_000);
        let tool = sent
            .tool
            .map_or_else(String::new, |tool| format!(" --tool {tool}"));
        format!(
            "Do not run the command again: a second run of it may start a second run and write twice. Find its run first with `anything2mcp run list --since {since}{tool}`, and do what that run's `next` says."
        )
    };
    Failure::new(
        FailureClass::PossibleCommit,
        local_codes::ANSWER_LOST,
        "The engine had the request, and its answer could not be read: what it asked may have been done, and a write may have started a run.",
        next,
    )
}

/// What the engine answered, as the response it is. An answer of the
/// channel itself, which refused the request before any part of the engine
/// saw it, has its four parts and no `ok`: it becomes the failure it
/// names. Anything else that is not a response is no answer.
fn response_of(body: Json, sent: &Sent<'_>) -> Result<Json, Failure> {
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
        _ => Err(not_an_answer(sent)),
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
    let at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let sent = Sent::of(step, globals, at_ms);
    let answer = link.send(request).await?;
    let exit = exit_class(
        answer
            .headers()
            .get(EXIT_CLASS_HEADER)
            .and_then(|value| value.to_str().ok()),
    );
    let bytes = axum::body::to_bytes(answer.into_body(), ANSWER_LIMIT)
        .await
        .map_err(|_| not_an_answer(&sent))?;
    let body: Json = serde_json::from_slice(&bytes).map_err(|_| not_an_answer(&sent))?;
    let body = response_of(body, &sent)?;
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

/// The action that stops the engine.
const STOP_ACTION: &str = "engine.stop";

/// What a command sends its steps with, beside the link: its global
/// options, the project folder it was run in, and the engine's paths.
pub(crate) struct Asking<'a> {
    pub(crate) globals: &'a Globals,
    pub(crate) project: Option<&'a Path>,
    pub(crate) paths: &'a Paths,
}

/// What an `engine.stop` whose answer was lost comes to. A stop writes
/// nothing of the person's, so its lost answer is no write in doubt, and
/// asking for it again is safe. The client looks at the channel once
/// more: an engine that stops lets go of its channel before it ends, and
/// takes no connection after it was asked, so one that no longer answers
/// has stopped serving, which is what its answer says, and the command
/// succeeds with that answer and a warning that says how it was learned.
/// An engine that still answers did not take the stop: the command fails
/// with `busy`/`engine_running`, and running it again asks again
/// (decision 0382).
async fn stop_answer_lost(paths: &Paths) -> Result<Answered, Failure> {
    let connector = Connector::new(paths, program_version()?, ClientLabel::CommandLine)?;
    match connector.connect().await {
        Ok(_still) => Err(Failure::new(
            FailureClass::Busy,
            local_codes::ENGINE_RUNNING,
            "The engine was asked to stop, its answer was lost, and it still answers on the local channel: it did not take the stop.",
            "Run `anything2mcp engine stop` again: a stop writes nothing, so asking again is safe.",
        )),
        Err(failure)
            if unreachable(&failure) || *failure.code() == local_codes::CONNECTION_LOST =>
        {
            let response = a2m_actions::Response {
                ok: true,
                action: STOP_ACTION.to_owned(),
                result: Some(serde_json::json!({"stopping": true})),
                error: None,
                warnings: vec![
                    "The engine's answer to the stop was lost; it no longer answers on the local channel, so it has stopped serving.".to_owned(),
                ],
                next: None,
                exit: ExitClass::Succeeded,
                decision: None,
            };
            Ok(Answered {
                body: response.to_json(),
                exit: ExitClass::Succeeded,
            })
        }
        Err(failure) => Err(failure),
    }
}

/// Sends the steps of one command to the engine in turn and renders the
/// last answer: the first that is not a success, or the final one. The
/// same rule as a command that runs the engine in its own process.
pub(crate) async fn run_steps(
    link: &Link,
    tree: &Tree,
    asking: &Asking<'_>,
    steps: Vec<Step>,
    json: bool,
    warning: Option<&str>,
) -> Finished {
    let mut last = None;
    for step in steps {
        let sent = match send(link, &step, asking.globals, asking.project).await {
            Err(failure)
                if step.action == STOP_ACTION
                    && failure.class() == FailureClass::PossibleCommit =>
            {
                stop_answer_lost(asking.paths).await
            }
            sent => sent,
        };
        match sent {
            Err(failure) if failure.code() == &local_codes::ENGINE_RUNNING => {
                return render::render(
                    &render::refused(&step.action, &failure, ExitClass::Failed),
                    tree,
                    json,
                );
            }
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
        Some(mut answered) => {
            // The client's own warning stands with the engine's.
            if let (Some(warning), Some(warnings)) = (
                warning,
                answered
                    .body
                    .get_mut("warnings")
                    .and_then(Json::as_array_mut),
            ) {
                warnings.push(Json::from(warning));
            }
            render::show(&answered.body, answered.exit.exit_code(), tree, json)
        }
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

    use super::{Sent, exit_class, not_an_answer, stop_answer_lost};

    /// A lost answer to `engine.stop` is never a write in doubt: with no
    /// engine on the channel after it, the stop is what the command says,
    /// with a warning; with one still answering, the command fails by name,
    /// and is safe to run again.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_lost_answer_to_a_stop_is_a_stop_when_the_channel_no_longer_answers() {
        let home = std::env::temp_dir().join(format!("a2m-stop-lost-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let paths = a2m_actions::Paths::under(&home);

        let stopped = stop_answer_lost(&paths).await.unwrap();
        assert_eq!(stopped.exit, ExitClass::Succeeded);
        assert_eq!(stopped.body["ok"], true, "{}", stopped.body);
        assert_eq!(stopped.body["action"], "engine.stop");
        assert_eq!(stopped.body["result"]["stopping"], true);
        assert_eq!(stopped.body["warnings"].as_array().unwrap().len(), 1);

        let channel = a2m_local::Channel::bind(&paths, crate::local::program_version().unwrap())
            .await
            .unwrap();
        let control = channel.control();
        let serving =
            tokio::spawn(channel.serve(axum::Router::new(), a2m_local::IdlePolicy::never()));
        let still = stop_answer_lost(&paths).await.unwrap_err();
        assert_eq!(still.code(), &a2m_local::codes::ENGINE_RUNNING, "{still:?}");
        assert_eq!(still.class(), a2m_core::FailureClass::Busy);
        control.stop(a2m_local::StopReason::Asked);
        drop(serving.await.unwrap());
        let _ = std::fs::remove_dir_all(&home);
    }

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

    #[test]
    fn an_answer_that_cannot_be_read_says_to_repeat_a_keyed_command_and_to_look_up_an_unkeyed_one()
    {
        let keyed = not_an_answer(&Sent {
            keyed: true,
            tool: Some("example_orders_order_update"),
            at_ms: 1_000_000,
        });
        assert_eq!(
            keyed.next_step(),
            "Run the same command again with the same `--idempotency-key` and nothing changed: it gives back the run the first one started, finished or not, and starts no other."
        );
        let unkeyed = not_an_answer(&Sent {
            keyed: false,
            tool: Some("example_orders_order_update"),
            at_ms: 1_000_000,
        });
        assert_eq!(
            unkeyed.next_step(),
            "Do not run the command again: a second run of it may start a second run and write twice. Find its run first with `anything2mcp run list --since 940000 --tool example_orders_order_update`, and do what that run's `next` says."
        );
        let other = not_an_answer(&Sent {
            keyed: false,
            tool: None,
            at_ms: 30,
        });
        assert!(
            other
                .next_step()
                .contains("`anything2mcp run list --since 0`"),
            "{}",
            other.next_step()
        );
    }
}
