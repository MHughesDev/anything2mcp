//! The person surface on one machine.
//!
//! [`LocalSurface`] is the only holder of the approval desk. It shows a
//! waiting write on a page served from a listener on the loopback address,
//! which exists only while a page is waiting, on a random port, at an
//! address that holds a one-time code. The page shows the stored change
//! itself, read through the desk, and carries a one-time value of its
//! own; a press without it does nothing. A press of Approve raises the
//! presence prompt, and only the prompt's success reaches the desk, for
//! the change the page showed and no other. One prompt is up at a time,
//! and it ends with the page's window. Silence is a decline.
//!
//! The listener is not a front door: it answers its page and two presses,
//! and nothing else.

use std::collections::{BTreeMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::Duration;

use a2m_actions::{
    ApprovalAsk, ApprovalDesk, ApprovalPreview, BoxFuture, PersonSurface, PresenceAsk,
    PresenceProof, PreviewRecord, SessionAsk, SignInAsk, SurfaceLink,
};
use a2m_auth::PersonAnswer;
use a2m_core::{Digest, Failure, PrincipalId, RunId};
use a2m_policy::{Channel as ApprovalChannel, PresenceCheck, PrincipalRef};
use a2m_telemetry::Clock;
use a2m_vault::RandomSource;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::codes;
use crate::presence::{Availability, PresencePrompt, PromptAnswer, PromptName, PromptText};

const APPROVAL_PAGE: &str = include_str!("pages/approval.html");
const ANSWER_PAGE: &str = include_str!("pages/answer.html");

/// The random bytes in the address's code and in the page's one-time
/// value.
const CODE_BYTES: usize = 16;
/// The most a press may send: one short field.
const PRESS_LIMIT: usize = 256;
/// The most records of one change a page shows. A change with more cannot
/// be read in full here, so it cannot be approved here either.
const RECORDS_SHOWN: usize = 2_000;
/// How many characters of the hash of a change the page shows beside
/// Approve, and a prompt that carries words repeats.
const SHORT_HASH: usize = 12;
/// How many answered changes are remembered. An older one that is asked
/// about again gets a new page, which the desk then refuses to act on.
const ANSWERED_KEPT: usize = 256;
/// What the pages may load: their own inline style, and nothing else. No
/// script runs, nothing is fetched, and no other page may frame them.
const CONTENT_POLICY: &str = "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'";

/// What a [`LocalSurface`] is made of.
pub struct SurfaceParts {
    /// The presence prompt. [`system_prompt`](crate::system_prompt) gives
    /// the one this build has.
    pub prompt: Arc<dyn PresencePrompt>,
    /// The clock a proof of presence is stamped by.
    pub clock: Arc<dyn Clock>,
    /// Where the one-time values come from.
    pub random: Arc<dyn RandomSource>,
    /// How long a page waits for an answer before the write is declined:
    /// the approval window.
    pub window: Duration,
}

/// One write that waits on a page.
struct Page {
    run: RunId,
    tool: String,
    diff_hash: Digest,
    summary: String,
    started_by: PrincipalId,
    /// The one-time value of the page served last. A press takes it.
    once: Option<String>,
    /// A press is being carried out.
    pressed: bool,
    /// The window ended while a press was being carried out.
    ended: bool,
    /// Whether the page served last showed every record of the change.
    whole: bool,
    /// The end of the approval window: the page goes then, and a prompt
    /// raised for it is cancelled then.
    deadline: Instant,
    timer: JoinHandle<()>,
}

struct Listening {
    port: u16,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

#[derive(Default)]
struct Waiting {
    listening: Option<Listening>,
    /// The waiting pages, by the code in their address.
    pages: BTreeMap<String, Page>,
    /// Listeners that were told to stop and may still be closing.
    closing: Vec<JoinHandle<()>>,
    /// The changes a person answered, or whose time ended, newest last. A
    /// run is looked at again after its answer, and gets no second page.
    answered: VecDeque<(RunId, Digest, Answered)>,
}

/// How a page ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answered {
    Approved,
    Declined,
}

/// How a press ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// Nothing was recorded; the page waits on.
    Open,
    /// The answer was recorded.
    Settled(Answered),
    /// The change the page showed no longer waits.
    Gone,
}

struct Inner {
    desk: OnceLock<ApprovalDesk>,
    prompt: Arc<dyn PresencePrompt>,
    clock: Arc<dyn Clock>,
    random: Arc<dyn RandomSource>,
    window: Duration,
    state: Mutex<Waiting>,
    waiting: watch::Sender<usize>,
    /// Held while a prompt is up. One prompt at a time: a person never
    /// answers a prompt that was raised for another change.
    prompting: tokio::sync::Mutex<()>,
}

/// The person surface on one machine. Cheap to clone; every clone is the
/// same surface.
#[derive(Clone)]
pub struct LocalSurface {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for LocalSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalSurface")
            .field("waiting", &self.waiting())
            .finish_non_exhaustive()
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// Whether two texts are the same, in time that does not depend on where
/// they differ.
fn same(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .fold(0_u8, |seen, (a, b)| seen | (a ^ b))
            == 0
}

/// Text made safe to place inside a page.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// `template` with each `{{name}}` replaced by its value, in one pass, so
/// that nothing inside a value is ever taken for a place to fill. The
/// values are markup the caller made: text from anywhere else was escaped
/// first.
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len() + 1024);
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        if let Some((_, value)) = values.iter().find(|(key, _)| *key == name) {
            out.push_str(value);
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// The first characters of a hash, as the page shows them beside Approve.
fn short(diff_hash: Digest) -> String {
    diff_hash.to_string().chars().take(SHORT_HASH).collect()
}

/// The check an approval is recorded with. The policy engine's list of
/// checks has one word for every prompt an operating system owns; when it
/// has a word for each, this is the one place that maps them.
const fn recorded_check(_prompt: PromptName) -> PresenceCheck {
    PresenceCheck::OsPrompt
}

/// A page: the status, the fixed headers that keep it to itself, and the
/// text.
fn page(status: StatusCode, html: String) -> Response {
    let fixed = |text: &'static str| HeaderValue::from_static(text);
    (
        status,
        [
            (header::CONTENT_TYPE, fixed("text/html; charset=utf-8")),
            (header::CACHE_CONTROL, fixed("no-store")),
            (header::CONTENT_SECURITY_POLICY, fixed(CONTENT_POLICY)),
            (header::X_FRAME_OPTIONS, fixed("DENY")),
            (header::X_CONTENT_TYPE_OPTIONS, fixed("nosniff")),
            (header::REFERRER_POLICY, fixed("no-referrer")),
        ],
        html,
    )
        .into_response()
}

/// A short page that says what happened. Both texts are the engine's own.
fn answer(status: StatusCode, title: &str, detail: &str) -> Response {
    page(
        status,
        fill(
            ANSWER_PAGE,
            &[("title", &escape(title)), ("detail", &escape(detail))],
        ),
    )
}

fn nothing_here() -> Response {
    answer(
        StatusCode::NOT_FOUND,
        "Nothing waits here",
        "This address shows nothing. A page for an approval closes when it is answered or when its time ends.",
    )
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, Waiting> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn code(&self) -> Result<String, Failure> {
        let mut bytes = [0_u8; CODE_BYTES];
        self.random.fill(&mut bytes)?;
        Ok(hex(&bytes))
    }

    /// Closes the listener when no page waits, and tells who watches how
    /// many do. Called with the state held, after every change to the
    /// pages.
    fn settle(&self, state: &mut Waiting) {
        if state.pages.is_empty()
            && let Some(listening) = state.listening.take()
        {
            let _ = listening.stop.send(());
            state.closing.push(listening.task);
        }
        self.waiting.send_replace(state.pages.len());
    }

    fn remember(state: &mut Waiting, run: RunId, diff_hash: Digest, how: Answered) {
        if state.answered.len() == ANSWERED_KEPT {
            state.answered.pop_front();
        }
        state.answered.push_back((run, diff_hash, how));
    }

    /// The window of a page ended: silence is a decline.
    async fn end_of_window(&self, code: &str) {
        let run = {
            let mut state = self.state();
            let Some(page) = state.pages.get_mut(code) else {
                return;
            };
            if page.pressed {
                // The press in flight settles it when it comes back.
                page.ended = true;
                return;
            }
            let (run, diff_hash) = (page.run, page.diff_hash);
            state.pages.remove(code);
            Self::remember(&mut state, run, diff_hash, Answered::Declined);
            self.settle(&mut state);
            run
        };
        self.decline_silence(run).await;
    }

    async fn decline_silence(&self, run: RunId) {
        if let Some(desk) = self.desk.get() {
            // A run that no longer waits has nothing to decline.
            let _ = desk.decline(run, None, ApprovalChannel::LocalPrompt).await;
        }
    }
}

/// The listener's guard: every request must name this listener as its
/// host, and a request that says where it comes from must come from this
/// listener's own page. A press must say so.
async fn guard(State(port): State<u16>, request: Request, next: Next) -> Response {
    let host = format!("127.0.0.1:{port}");
    let origin = format!("http://{host}");
    let headers = request.headers();
    let text = |name: header::HeaderName| headers.get(name).and_then(|value| value.to_str().ok());
    let host_fits = headers.get_all(header::HOST).iter().count() == 1
        && text(header::HOST) == Some(host.as_str());
    let origin_fits = match text(header::ORIGIN) {
        Some(from) => from == origin,
        None => request.method() != Method::POST && !headers.contains_key(header::ORIGIN),
    };
    if !host_fits || !origin_fits {
        return answer(
            StatusCode::FORBIDDEN,
            "Not from this machine's own page",
            "This page answers only the browser that opened its link on this machine.",
        );
    }
    next.run(request).await
}

fn once_of(body: &Bytes) -> Option<&str> {
    if body.len() > PRESS_LIMIT {
        return None;
    }
    std::str::from_utf8(body)
        .ok()?
        .split('&')
        .find_map(|field| field.strip_prefix("once="))
}

async fn show(State(inner): State<Arc<Inner>>, Path(code): Path<String>) -> Response {
    let once = match inner.code() {
        Ok(once) => once,
        Err(_) => {
            return answer(
                StatusCode::SERVICE_UNAVAILABLE,
                "The page cannot be shown",
                "This machine gave no random value for the page. Nothing was approved. Open the link again.",
            );
        }
    };
    let mut state = inner.state();
    let Some(found) = state.pages.get_mut(&code) else {
        return nothing_here();
    };
    // Only the page served last can press.
    found.once = Some(once.clone());
    let minutes = inner.window.as_secs().div_ceil(60).max(1);
    page(
        StatusCode::OK,
        APPROVAL_PAGE
            .replace("{{summary}}", &escape(&found.summary))
            .replace("{{tool}}", &escape(&found.tool))
            .replace("{{run}}", &escape(&found.run.to_string()))
            .replace("{{started_by}}", &escape(&found.started_by.to_string()))
            .replace("{{diff_hash}}", &escape(&found.diff_hash.to_string()))
            .replace("{{minutes}}", &minutes.to_string())
            .replace("{{code}}", &escape(&code))
            .replace("{{once}}", &escape(&once)),
    )
}

/// What a press is about, copied out of the page while the state is held.
struct Pressed {
    run: RunId,
    tool: String,
    diff_hash: Digest,
    started_by: PrincipalId,
}

/// Takes the page's one-time value and marks the page as pressed. A press
/// without the value of the page served last changes nothing but that
/// value, which is spent either way.
fn begin(inner: &Inner, code: &str, body: &Bytes) -> Result<Pressed, Box<Response>> {
    let mut state = inner.state();
    let Some(found) = state.pages.get_mut(code) else {
        return Err(Box::new(nothing_here()));
    };
    let expected = found.once.take();
    let fits = match (expected.as_deref(), once_of(body)) {
        (Some(expected), Some(given)) => same(expected, given),
        _ => false,
    };
    if !fits {
        return Err(Box::new(answer(
            StatusCode::FORBIDDEN,
            "Nothing was recorded",
            "The press did not come from the page as it was last shown. Open the link again and press there.",
        )));
    }
    if found.pressed {
        return Err(Box::new(answer(
            StatusCode::CONFLICT,
            "A press is already being answered",
            "Answer the prompt on this machine, or open the link again when it has closed.",
        )));
    }
    found.pressed = true;
    Ok(Pressed {
        run: found.run,
        tool: found.tool.clone(),
        diff_hash: found.diff_hash,
        started_by: found.started_by,
    })
}

/// Ends a press. A page whose answer was recorded, or whose change no
/// longer waits, is removed. Any other waits on, unless its window ended
/// meanwhile, which is a decline.
async fn finish(inner: &Inner, code: &str, outcome: Outcome) {
    let declined = {
        let mut state = inner.state();
        let Some(found) = state.pages.get_mut(code) else {
            return;
        };
        found.pressed = false;
        let (run, diff_hash, ended) = (found.run, found.diff_hash, found.ended);
        let how = match outcome {
            Outcome::Settled(how) => Some(how),
            Outcome::Open if ended => Some(Answered::Declined),
            Outcome::Open | Outcome::Gone => None,
        };
        if outcome != Outcome::Open || ended {
            if let Some(gone) = state.pages.remove(code) {
                gone.timer.abort();
            }
            if let Some(how) = how {
                Inner::remember(&mut state, run, diff_hash, how);
            }
            inner.settle(&mut state);
        }
        (outcome == Outcome::Open && ended).then_some(run)
    };
    if let Some(run) = declined {
        inner.decline_silence(run).await;
    }
}

async fn approve(
    State(inner): State<Arc<Inner>>,
    Path(code): Path<String>,
    body: Bytes,
) -> Response {
    let pressed = match begin(&inner, &code, &body) {
        Ok(pressed) => pressed,
        Err(response) => return *response,
    };
    let (outcome, response) = approve_pressed(&inner, &pressed).await;
    finish(&inner, &code, outcome).await;
    response
}

/// Carries out a press of Approve, and says how it ended.
async fn approve_pressed(inner: &Inner, pressed: &Pressed) -> (Outcome, Response) {
    let Some(desk) = inner.desk.get() else {
        return (
            Outcome::Open,
            answer(
                StatusCode::SERVICE_UNAVAILABLE,
                "Nothing was approved",
                "The engine cannot record an approval now. Run `anything2mcp doctor run` for the reason.",
            ),
        );
    };
    // The desk is asked again at the press: the approval is for the change
    // this page showed, and only while that change is the one that waits.
    let still_waits = match desk.waiting(pressed.run).await {
        Ok(waiting) => waiting.diff_hash == pressed.diff_hash,
        Err(_) => false,
    };
    if !still_waits {
        return (
            Outcome::Gone,
            answer(
                StatusCode::CONFLICT,
                "Nothing was approved",
                "The change this page showed no longer waits for an approval. Look at the run again for where it stands.",
            ),
        );
    }
    let asked = inner
        .prompt
        .ask(PromptText {
            title: format!("Approve a write by `{}`", pressed.tool),
            detail: format!("Run {}, change {}", pressed.run, pressed.diff_hash),
        })
        .await;
    let check = match asked {
        PromptAnswer::Present => PresenceCheck::OsPrompt,
        PromptAnswer::Confirmed => PresenceCheck::NotAvailable,
        PromptAnswer::Dismissed => {
            return (
                Outcome::Open,
                answer(
                    StatusCode::FORBIDDEN,
                    "Nothing was approved",
                    "The prompt on this machine was dismissed or not passed. Open the link again to answer once more.",
                ),
            );
        }
        PromptAnswer::NotAvailable => {
            return (
                Outcome::Open,
                answer(
                    StatusCode::FORBIDDEN,
                    "Nothing was approved",
                    "This machine has no way yet to ask for proof that a person is present, so it records no approval. You can decline the write from the page.",
                ),
            );
        }
    };
    let proof = PresenceProof::new(check, pressed.started_by, inner.clock.now());
    let recorded = desk
        .approve(
            pressed.run,
            pressed.diff_hash,
            PrincipalRef::person(pressed.started_by),
            ApprovalChannel::LocalPrompt,
            proof,
        )
        .await;
    match recorded {
        Ok(_) => (
            Outcome::Settled(Answered::Approved),
            answer(
                StatusCode::OK,
                "Approved",
                "The approval is recorded for the change this page showed. The program that started the run can resume it. You can close this page.",
            ),
        ),
        // The policy engine's refusal is the engine's own words.
        Err(failure) => (
            Outcome::Open,
            answer(
                StatusCode::FORBIDDEN,
                "Nothing was approved",
                failure.message(),
            ),
        ),
    }
}

async fn decline(
    State(inner): State<Arc<Inner>>,
    Path(code): Path<String>,
    body: Bytes,
) -> Response {
    let pressed = match begin(&inner, &code, &body) {
        Ok(pressed) => pressed,
        Err(response) => return *response,
    };
    let recorded = match inner.desk.get() {
        Some(desk) => desk
            .decline(
                pressed.run,
                Some(PrincipalRef::person(pressed.started_by)),
                ApprovalChannel::LocalPrompt,
            )
            .await
            .is_ok(),
        None => false,
    };
    let outcome = if recorded {
        Outcome::Settled(Answered::Declined)
    } else {
        Outcome::Open
    };
    finish(&inner, &code, outcome).await;
    if recorded {
        answer(
            StatusCode::OK,
            "Declined",
            "The write was declined and nothing was written. You can close this page.",
        )
    } else {
        answer(
            StatusCode::CONFLICT,
            "Nothing was recorded",
            "The engine could not record the decline. Nothing is written unless a person approves; the page closes at the end of its time.",
        )
    }
}

impl LocalSurface {
    /// A surface with nothing waiting and no listener open.
    #[must_use]
    pub fn new(parts: SurfaceParts) -> Self {
        let (waiting, _) = watch::channel(0);
        Self {
            inner: Arc::new(Inner {
                desk: OnceLock::new(),
                prompt: parts.prompt,
                clock: parts.clock,
                random: parts.random,
                window: parts.window,
                state: Mutex::new(Waiting::default()),
                waiting,
            }),
        }
    }

    /// Which kind of presence check this surface asks with.
    #[must_use]
    pub fn presence(&self) -> PresenceKind {
        self.inner.prompt.kind()
    }

    /// How many pages wait for a person. An engine with a page waiting is
    /// not idle.
    #[must_use]
    pub fn waiting(&self) -> usize {
        *self.inner.waiting.borrow()
    }

    /// Waits until no page waits and every listener that served one has
    /// closed.
    pub async fn settled(&self) {
        let mut waiting = self.inner.waiting.subscribe();
        loop {
            if waiting.wait_for(|count| *count == 0).await.is_err() {
                return;
            }
            let closing = std::mem::take(&mut self.inner.state().closing);
            if closing.is_empty() {
                if self.waiting() == 0 {
                    return;
                }
                continue;
            }
            for task in closing {
                let _ = task.await;
            }
        }
    }

    /// The listener's port: the one open, or a new one on the loopback
    /// address and a port the operating system picks.
    fn listen(&self, state: &mut Waiting) -> Result<u16, Failure> {
        if let Some(listening) = &state.listening {
            return Ok(listening.port);
        }
        let open = || -> std::io::Result<(tokio::net::TcpListener, u16)> {
            let listener = std::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
            listener.set_nonblocking(true)?;
            let port = listener.local_addr()?.port();
            Ok((tokio::net::TcpListener::from_std(listener)?, port))
        };
        let (listener, port) =
            open().map_err(|error| codes::unavailable("The approval page's listener", &error))?;
        let app = Router::new()
            .route("/approve/{code}", get(show))
            .route("/approve/{code}/approve", post(approve))
            .route("/approve/{code}/decline", post(decline))
            .fallback(|| async { nothing_here() })
            .layer(axum::middleware::from_fn_with_state(port, guard))
            .with_state(Arc::clone(&self.inner));
        let (stop, stopped) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = stopped.await;
                })
                .await;
        });
        state.listening = Some(Listening { port, stop, task });
        Ok(port)
    }

    fn link(port: u16, code: &str, minutes: u64) -> SurfaceLink {
        SurfaceLink {
            url: Some(format!("http://127.0.0.1:{port}/approve/{code}")),
            text: format!(
                "Open the link in your own browser on this machine to read what waits and to approve or decline it. The page closes after {minutes} minutes; no answer is a decline."
            ),
        }
    }
}

impl PersonSurface for LocalSurface {
    fn take_desk(&self, desk: ApprovalDesk) {
        // The kernel hands the desk over once. A second one is dropped.
        let _ = self.inner.desk.set(desk);
    }

    fn ask_approval(&self, ask: ApprovalAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            let inner = &self.inner;
            if inner.desk.get().is_none() {
                return Err(codes::busy(
                    codes::SURFACE_UNAVAILABLE,
                    "The engine cannot record an approval now, so no approval page is shown.",
                    "Run `anything2mcp doctor run` for the reason.",
                ));
            }
            let minutes = inner.window.as_secs().div_ceil(60).max(1);
            let code = inner.code()?;
            let mut state = inner.state();
            // A run is looked at after its answer too, before it is
            // resumed: an answered change gets no second page.
            let answered = state
                .answered
                .iter()
                .rev()
                .find(|(run, diff_hash, _)| *run == ask.run && *diff_hash == ask.diff_hash)
                .map(|(_, _, how)| *how);
            if let Some(how) = answered {
                return Ok(SurfaceLink {
                    url: None,
                    text: match how {
                        Answered::Approved => {
                            "A person approved this change. Resume the run to carry it out."
                        }
                        Answered::Declined => {
                            "This change was declined, or nobody answered in its time. Nothing is written."
                        }
                    }
                    .to_owned(),
                });
            }
            // The run is looked at many times while it waits: the page it
            // already has is the page it keeps.
            let same_ask = state
                .pages
                .iter()
                .find(|(_, page)| page.run == ask.run)
                .map(|(code, page)| (code.clone(), page.diff_hash == ask.diff_hash));
            match same_ask {
                Some((code, true)) => {
                    let port = self.listen(&mut state)?;
                    return Ok(Self::link(port, &code, minutes));
                }
                Some((stale, false)) => {
                    // The run waits with another change: the old page
                    // shows what no longer waits, and goes.
                    if let Some(gone) = state.pages.remove(&stale) {
                        gone.timer.abort();
                    }
                }
                None => {}
            }
            let port = match self.listen(&mut state) {
                Ok(port) => port,
                Err(failure) => {
                    inner.settle(&mut state);
                    return Err(failure);
                }
            };
            let timer = tokio::spawn({
                let window = inner.window;
                let inner: Weak<Inner> = Arc::downgrade(inner);
                let code = code.clone();
                async move {
                    tokio::time::sleep(window).await;
                    if let Some(inner) = inner.upgrade() {
                        inner.end_of_window(&code).await;
                    }
                }
            });
            state.pages.insert(
                code.clone(),
                Page {
                    run: ask.run,
                    tool: ask.tool,
                    diff_hash: ask.diff_hash,
                    summary: ask.summary,
                    started_by: ask.started_by,
                    once: None,
                    pressed: false,
                    ended: false,
                    timer,
                },
            );
            inner.settle(&mut state);
            Ok(Self::link(port, &code, minutes))
        })
    }

    fn ask_sign_in(&self, ask: SignInAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            Ok(SurfaceLink {
                url: None,
                text: format!(
                    "`{}` needs its person to sign in. Sign in at the window the engine opens for it on this machine, then call again.",
                    ask.target
                ),
            })
        })
    }

    fn confirm_presence(&self, ask: PresenceAsk) -> BoxFuture<'_, Result<PresenceProof, Failure>> {
        Box::pin(async move {
            let answer = self
                .inner
                .prompt
                .ask(PromptText {
                    title: format!("Confirm `{}`", ask.action),
                    detail: ask.summary,
                })
                .await;
            match answer {
                PromptAnswer::Present => Ok(PresenceProof::new(
                    PresenceCheck::OsPrompt,
                    ask.principal,
                    self.inner.clock.now(),
                )),
                PromptAnswer::Dismissed => Err(codes::policy(
                    codes::PRESENCE_FAILED,
                    "The prompt on this machine was dismissed or not passed.",
                    "Run the command again and answer the prompt yourself.",
                )),
                PromptAnswer::Confirmed | PromptAnswer::NotAvailable => Err(codes::policy(
                    codes::PRESENCE_FAILED,
                    "This machine has no way to prove that a person is present, so this cannot be done here.",
                    "Make the change in the installation's settings file, or on a machine whose operating system offers a presence prompt.",
                )),
            }
        })
    }

    fn ask_session(&self, _ask: SessionAsk) -> BoxFuture<'_, PersonAnswer> {
        // No notification and no return address are built on one machine
        // yet, so the session's person cannot be reached from here.
        Box::pin(async { PersonAnswer::Unreachable })
    }
}
