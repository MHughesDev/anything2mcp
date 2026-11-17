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
//! A person's act an agent asked for, such as publishing a version, is
//! shown the same way on a page of its own (decision 0163): exactly what
//! will be done, every value as text; a press of Carry out raises the
//! presence prompt, and only a proof from that press, given for that act,
//! reaches the desk, which carries the act out once.
//!
//! The listener is not a front door: it answers its pages and their
//! presses, and nothing else.

use std::collections::{BTreeMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::Duration;

use a2m_actions::{
    ApprovalAsk, ApprovalDesk, ApprovalPreview, BINDING_LATER, BoxFuture, MAY_NOT_BE_READ_BACK,
    PersonActAsk, PersonActId, PersonSurface, PresenceAsk, PresenceProof, PresenceReport,
    PreviewRecord, SessionAsk, SignInAsk, SurfaceLink,
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
use crate::presence::{
    Availability, PresenceKind, PresencePrompt, PromptAnswer, PromptName, PromptText,
};

const APPROVAL_PAGE: &str = include_str!("pages/approval.html");
const ANSWER_PAGE: &str = include_str!("pages/answer.html");
const ACT_PAGE: &str = include_str!("pages/act.html");
/// The most characters of what an act came to that its answer page shows.
const RESULT_SHOWN: usize = 2_000;

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

/// One person's act that waits on a page.
struct ActPage {
    act: PersonActId,
    action: String,
    person: PrincipalId,
    /// The one-time value of the page served last. A press takes it.
    once: Option<String>,
    /// A press is being carried out.
    pressed: bool,
    /// The window ended while a press was being carried out.
    ended: bool,
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
    /// The person's acts that wait on a page, by the code in its address.
    acts: BTreeMap<String, ActPage>,
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

/// The check an approval is recorded with: the policy engine has one word
/// for each prompt an operating system owns, and this is the one place that
/// maps them (decision 0145).
const fn recorded_check(prompt: PromptName) -> PresenceCheck {
    match prompt {
        PromptName::Polkit => PresenceCheck::Polkit,
        PromptName::LocalAuthentication => PresenceCheck::LocalAuthentication,
        PromptName::WindowsHello => PresenceCheck::WindowsHello,
    }
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
            // Not `no-referrer`: under it a browser sends `Origin: null` on
            // the page's own form post, which the guard refuses, so no real
            // press could work. `same-origin` keeps the page's origin on a
            // press and sends nothing to any other origin.
            (header::REFERRER_POLICY, fixed("same-origin")),
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
            && state.acts.is_empty()
            && let Some(listening) = state.listening.take()
        {
            let _ = listening.stop.send(());
            state.closing.push(listening.task);
        }
        self.waiting
            .send_replace(state.pages.len() + state.acts.len());
    }

    /// The window of an act's page ended: silence is a decline.
    fn end_of_act_window(&self, code: &str) {
        let act = {
            let mut state = self.state();
            let Some(page) = state.acts.get_mut(code) else {
                return;
            };
            if page.pressed {
                page.ended = true;
                return;
            }
            let act = page.act;
            state.acts.remove(code);
            self.settle(&mut state);
            act
        };
        if let Some(desk) = self.desk.get() {
            desk.decline_act(act, false);
        }
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

/// A waiting change as the page shows it: what the desk read back from
/// the stored diff, and nothing rebuilt.
struct Change {
    /// The first reading: the tool, the target, the counts.
    head: ApprovalPreview,
    /// The records, in the order the run would write them.
    records: Vec<PreviewRecord>,
    /// Whether these are all the records of the change.
    whole: bool,
}

/// Reads the change `run` waits with, record by record, and only if it is
/// the change with `diff_hash`, the one the page was opened for. `None`
/// when the run no longer waits with that change.
async fn read_change(desk: &ApprovalDesk, run: RunId, diff_hash: Digest) -> Option<Change> {
    let mut head = desk.preview(run, 0).await.ok()?;
    if head.diff_hash != diff_hash {
        return None;
    }
    let mut records = std::mem::take(&mut head.records);
    let mut whole = records.len() >= head.total;
    while !whole && records.len() < RECORDS_SHOWN {
        let next = desk.preview(run, records.len()).await.ok()?;
        if next.diff_hash != diff_hash || next.records.is_empty() {
            return None;
        }
        whole = next.complete();
        records.extend(next.records);
    }
    Some(Change {
        head,
        records,
        whole,
    })
}

/// The change as markup. Every value in it comes from a system or a
/// caller, and is placed as text.
fn change_html(change: &Change) -> String {
    use std::fmt::Write as _;
    let head = &change.head;
    let mut out = String::new();
    let _ = write!(
        out,
        "<p>Target <code>{}</code>, environment <code>{}</code>. {} to create, {} to change, {} already as intended: {} records in all.</p>",
        escape(&head.target),
        escape(&head.environment),
        head.create,
        head.update,
        head.unchanged,
        head.total,
    );
    if !change.whole {
        let _ = write!(
            out,
            "<p><strong>This page shows the first {} records only.</strong> A change that cannot be read in full here cannot be approved here.</p>",
            change.records.len(),
        );
    }
    for (index, record) in change.records.iter().enumerate() {
        let _ = write!(
            out,
            "<section><h3>{}. <code>{}</code> {}: {}</h3>",
            index + 1,
            escape(&record.operation),
            escape(&record.key),
            escape(record.action),
        );
        out.push_str(&written_by_html(record));
        out.push_str("<table><thead><tr><th scope=\"col\">Field</th><th scope=\"col\">Now</th><th scope=\"col\">After the write</th><th scope=\"col\">Changes</th></tr></thead><tbody>");
        for field in &record.fields {
            let now = field.current.as_ref().map_or_else(
                || "<em>does not exist</em>".to_owned(),
                |current| escape(current),
            );
            let _ = write!(
                out,
                "<tr><th scope=\"row\">{}</th><td>{}</td><td>{}</td><td>{}{}</td></tr>",
                escape(&field.name),
                now,
                escape(&field.proposed),
                if field.changes { "yes" } else { "no" },
                if field.cut {
                    "; a value is cut short here"
                } else {
                    ""
                },
            );
        }
        out.push_str("</tbody></table></section>");
    }
    out
}

/// What one binding's evidence means for whether the write is read back,
/// in the page's words.
fn evidence_words(evidence: &str) -> &'static str {
    match evidence {
        "read_back" => "it is read back and compared with what was intended after it is sent",
        "response" => {
            "the answer the system gives to the write is compared with what was intended; it is not read back apart"
        }
        "acknowledged" => {
            "it is accepted by the system and not read back: nothing compares it after it is sent"
        }
        // A kind of evidence this page does not know is told as the
        // weakest.
        _ => "this page does not know its evidence: take it as not read back",
    }
}

/// What the page says, beside a record and before its fields, about how
/// it will be written: a write the system only acknowledges, said to be
/// one before the person approves it (decision 0181), and the binding the
/// approval binds the write to, or every binding it could take when that
/// is chosen later (decision 0185). Each word is escaped.
pub(crate) fn written_by_html(record: &PreviewRecord) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    match record.not_read_back {
        Some(words) if words == MAY_NOT_BE_READ_BACK => {
            let _ = write!(
                out,
                "<p><strong>This write {}.</strong> A binding that is only acknowledged reads nothing back and compares nothing after it is sent.</p>",
                escape(words),
            );
        }
        Some(words) => {
            let _ = write!(
                out,
                "<p><strong>This write is {}.</strong> Nothing will read it back or compare it after it is sent.</p>",
                escape(words),
            );
        }
        None => {}
    }
    match (record.binding_later, record.bindings.as_slice()) {
        (_, []) => {}
        (None, [binding]) => {
            let _ = write!(
                out,
                "<p>Written by the binding <code>{}</code>, and by no other: {}.</p>",
                escape(&binding.mode),
                escape(evidence_words(binding.evidence)),
            );
        }
        (later, bindings) => {
            let _ = write!(
                out,
                "<p>This record is {}. It is written by one of these bindings, and by no other:</p><ul>",
                escape(later.unwrap_or(BINDING_LATER)),
            );
            for binding in bindings {
                let _ = write!(
                    out,
                    "<li><code>{}</code>: {}.</li>",
                    escape(&binding.mode),
                    escape(evidence_words(binding.evidence)),
                );
            }
            out.push_str("</ul>");
        }
    }
    out
}

/// What the page says about the presence prompt of this machine.
fn presence_html(presence: &Availability) -> String {
    if presence.proves_presence() {
        format!("<p>{}</p>", escape(&presence.message))
    } else {
        format!(
            "<p><strong>No presence prompt is available on this machine, so nothing here can approve this write.</strong> {} {} You can still decline it.</p>",
            escape(&presence.message),
            escape(&presence.remedy),
        )
    }
}

fn gone() -> Response {
    answer(
        StatusCode::CONFLICT,
        "Nothing to approve here",
        "The change this page was opened for no longer waits for an approval. Look at the run again for where it stands.",
    )
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
    let asked = inner
        .state()
        .pages
        .get(&code)
        .map(|found| (found.run, found.diff_hash));
    let Some((run, diff_hash)) = asked else {
        return nothing_here();
    };
    let Some(desk) = inner.desk.get() else {
        return gone();
    };
    // The page shows the stored change, and only the one it was opened
    // for: the desk reads the diff back and computes its hash again.
    let Some(change) = read_change(desk, run, diff_hash).await else {
        return gone();
    };
    let presence = inner.prompt.available().await;
    let mut state = inner.state();
    let Some(found) = state.pages.get_mut(&code) else {
        return nothing_here();
    };
    // Only the page served last can press.
    found.once = Some(once.clone());
    found.whole = change.whole;
    let minutes = inner.window.as_secs().div_ceil(60).max(1);
    page(
        StatusCode::OK,
        fill(
            APPROVAL_PAGE,
            &[
                ("summary", &escape(&found.summary)),
                ("tool", &escape(&found.tool)),
                ("run", &escape(&found.run.to_string())),
                ("started_by", &escape(&found.started_by.to_string())),
                ("diff_hash", &escape(&found.diff_hash.to_string())),
                ("short_hash", &escape(&short(found.diff_hash))),
                ("change", &change_html(&change)),
                ("presence", &presence_html(&presence)),
                ("minutes", &minutes.to_string()),
                ("code", &escape(&code)),
                ("once", &escape(&once)),
            ],
        ),
    )
}

/// What a press is about, copied out of the page while the state is held.
struct Pressed {
    run: RunId,
    tool: String,
    diff_hash: Digest,
    started_by: PrincipalId,
    whole: bool,
    deadline: Instant,
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
        whole: found.whole,
        deadline: found.deadline,
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

/// What a press answers when the task that carried it out did not end.
fn not_carried_out() -> Response {
    answer(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Nothing was recorded",
        "The engine could not carry out the press. Nothing is written unless a person approves; open the link again.",
    )
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
    // The press is carried out in a task of its own: a prompt waits for a
    // person, and a browser that goes away meanwhile must not leave the
    // page marked as pressed for ever.
    let carried = tokio::spawn(async move {
        let (outcome, response) = approve_pressed(&inner, &pressed).await;
        finish(&inner, &code, outcome).await;
        response
    });
    carried.await.unwrap_or_else(|_| not_carried_out())
}

fn nothing_approved(status: StatusCode, detail: &str) -> (Outcome, Response) {
    (
        Outcome::Open,
        answer(status, "Nothing was approved", detail),
    )
}

/// Carries out a press of Approve, and says how it ended.
async fn approve_pressed(inner: &Inner, pressed: &Pressed) -> (Outcome, Response) {
    let Some(desk) = inner.desk.get() else {
        return nothing_approved(
            StatusCode::SERVICE_UNAVAILABLE,
            "The engine cannot record an approval now. Run `anything2mcp doctor run` for the reason.",
        );
    };
    if !pressed.whole {
        return nothing_approved(
            StatusCode::CONFLICT,
            "This change has more records than its page can show, and a change that was not shown in full is not approved here. You can decline it from the page.",
        );
    }
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
    // One prompt at a time. A second press is told so and raises nothing.
    let Ok(_up) = inner.prompting.try_lock() else {
        return nothing_approved(
            StatusCode::SERVICE_UNAVAILABLE,
            "Another presence prompt is up on this machine. Answer or close it first, then open the link again and press once more.",
        );
    };
    let asked = inner
        .prompt
        .ask(
            PromptText {
                title: format!("Approve a write by `{}`", pressed.tool),
                detail: format!("Change {}", short(pressed.diff_hash)),
            },
            pressed.deadline,
        )
        .await;
    // The window is the surface's own to keep: an answer that comes back
    // after it closed is a dismissal, whatever the prompt said, so that no
    // approval is recorded late by a prompt that did not keep its deadline.
    let asked = if Instant::now() >= pressed.deadline {
        PromptAnswer::Dismissed
    } else {
        asked
    };
    let (check, named, how) = match asked {
        PromptAnswer::Present(prompt) => (
            recorded_check(prompt),
            Some(prompt.as_str()),
            format!("the presence prompt was passed ({})", prompt.as_str()),
        ),
        PromptAnswer::Confirmed => (
            PresenceCheck::NotAvailable,
            None,
            "a confirmation dialog was answered, which proves no person".to_owned(),
        ),
        PromptAnswer::Dismissed => {
            return nothing_approved(
                StatusCode::FORBIDDEN,
                "The prompt on this machine was closed, or its time ended. Open the link again to answer once more.",
            );
        }
        PromptAnswer::Denied => {
            return nothing_approved(
                StatusCode::FORBIDDEN,
                "The prompt on this machine was answered and not passed. Open the link again to answer once more.",
            );
        }
        PromptAnswer::NotAvailable(why) => {
            return nothing_approved(
                StatusCode::FORBIDDEN,
                &format!(
                    "No presence prompt is available on this machine, so no approval is recorded. {} {} You can decline the write from the page.",
                    why.message, why.remedy
                ),
            );
        }
    };
    let mut proof = PresenceProof::new(check, pressed.started_by, inner.clock.now());
    if let Some(name) = named {
        proof = proof.naming(name);
    }
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
                &format!(
                    "The approval is recorded for the change this page showed: {how}. The program that started the run can resume it. You can close this page."
                ),
            ),
        ),
        // The policy engine's refusal is the engine's own words.
        Err(failure) => nothing_approved(StatusCode::FORBIDDEN, failure.message()),
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
    // In a task of its own, as a press of Approve is.
    let carried = tokio::spawn(async move {
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
        recorded
    });
    match carried.await {
        Ok(true) => answer(
            StatusCode::OK,
            "Declined",
            "The write was declined and nothing was written. You can close this page.",
        ),
        Ok(false) => answer(
            StatusCode::CONFLICT,
            "Nothing was recorded",
            "The engine could not record the decline. Nothing is written unless a person approves; the page closes at the end of its time.",
        ),
        Err(_) => not_carried_out(),
    }
}

/// Whether the page shows every value of the act's input whole: none was
/// cut to the length a page shows.
fn whole_input(ask: &PersonActAsk) -> bool {
    ask.input.iter().all(|field| !field.cut)
}

/// The input of an act as a table, and the lines to read beside it. Every
/// name and value is placed as text.
fn act_html(ask: &PersonActAsk) -> (String, String) {
    use std::fmt::Write as _;
    let mut input = String::new();
    if !whole_input(ask) {
        // What is pressed is what was shown (decision 0181): an act whose
        // input the page cannot show whole is not carried out from it.
        input.push_str("<p><strong>A value of this input is longer than this page can show, so this act cannot be carried out here.</strong> You can decline it, and ask the program for an act whose input can be read whole.</p>");
    }
    input.push_str(
        "<table><thead><tr><th scope=\"col\">Field</th><th scope=\"col\">Value</th></tr></thead><tbody>",
    );
    for field in &ask.input {
        let _ = write!(
            input,
            "<tr><th scope=\"row\">{}</th><td>{}{}</td></tr>",
            escape(&field.name),
            escape(&field.value),
            if field.cut { " (cut short here)" } else { "" },
        );
    }
    input.push_str("</tbody></table>");
    let mut details = String::new();
    if !ask.details.is_empty() {
        details.push_str("<h2>What it may do</h2><p>The most dangerous first.</p><ul>");
        for line in &ask.details {
            let _ = write!(details, "<li>{}</li>", escape(line));
        }
        details.push_str("</ul>");
    }
    (input, details)
}

/// The first characters of an act's identifier, as the page and a prompt
/// that carries words show them.
fn short_act(act: PersonActId) -> String {
    act.to_string().chars().take(SHORT_HASH).collect()
}

async fn act_show(State(inner): State<Arc<Inner>>, Path(code): Path<String>) -> Response {
    let Ok(once) = inner.code() else {
        return answer(
            StatusCode::SERVICE_UNAVAILABLE,
            "The page cannot be shown",
            "This machine gave no random value for the page. Nothing was done. Open the link again.",
        );
    };
    let Some(act) = inner.state().acts.get(&code).map(|page| page.act) else {
        return nothing_here();
    };
    let Some(desk) = inner.desk.get() else {
        return gone();
    };
    // The page shows what the desk holds for the act, and nothing kept
    // from the ask.
    let Ok(ask) = desk.person_act(act) else {
        return answer(
            StatusCode::CONFLICT,
            "Nothing waits here",
            "The act this page was opened for no longer waits: it was carried out, declined, or its time ended.",
        );
    };
    let presence = inner.prompt.available().await;
    let mut state = inner.state();
    let Some(found) = state.acts.get_mut(&code) else {
        return nothing_here();
    };
    found.once = Some(once.clone());
    let minutes = inner.window.as_secs().div_ceil(60).max(1);
    let (input, details) = act_html(&ask);
    page(
        StatusCode::OK,
        fill(
            ACT_PAGE,
            &[
                ("action", &escape(ask.action.as_str())),
                ("summary", &escape(&ask.summary)),
                ("asked_through", &escape(&ask.asked_through)),
                ("act", &escape(&act.to_string())),
                ("short", &escape(&short_act(act))),
                ("input", &input),
                ("details", &details),
                ("presence", &presence_html(&presence)),
                ("minutes", &minutes.to_string()),
                ("code", &escape(&code)),
                ("once", &escape(&once)),
            ],
        ),
    )
}

/// What a press on an act's page is about.
struct ActPressed {
    act: PersonActId,
    action: String,
    person: PrincipalId,
    deadline: Instant,
}

/// Takes the act page's one-time value and marks it as pressed, as
/// [`begin`] does for an approval.
fn act_begin(inner: &Inner, code: &str, body: &Bytes) -> Result<ActPressed, Box<Response>> {
    let mut state = inner.state();
    let Some(found) = state.acts.get_mut(code) else {
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
            "Nothing was done",
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
    Ok(ActPressed {
        act: found.act,
        action: found.action.clone(),
        person: found.person,
        deadline: found.deadline,
    })
}

/// Ends a press on an act's page: an act that was answered, or no longer
/// waits, takes its page with it; any other waits on, unless its window
/// ended meanwhile, which is a decline.
fn act_finish(inner: &Inner, code: &str, settled: bool) {
    let declined = {
        let mut state = inner.state();
        let Some(found) = state.acts.get_mut(code) else {
            return;
        };
        found.pressed = false;
        let (act, ended) = (found.act, found.ended);
        if settled || ended {
            if let Some(gone) = state.acts.remove(code) {
                gone.timer.abort();
            }
            inner.settle(&mut state);
        }
        (!settled && ended).then_some(act)
    };
    if let (Some(act), Some(desk)) = (declined, inner.desk.get()) {
        desk.decline_act(act, false);
    }
}

fn nothing_done(status: StatusCode, detail: &str) -> (bool, Response) {
    (false, answer(status, "Nothing was done", detail))
}

/// What the act came to, as its answer page says it: the engine's own
/// words, and the result as text.
fn act_answer(done: &a2m_actions::Response) -> Response {
    if done.ok {
        let result = done
            .result
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        let result: String = result.chars().take(RESULT_SHOWN).collect();
        answer(
            StatusCode::OK,
            "Done",
            &format!(
                "`{}` was carried out as you, once. What it gave: {result}. The program that asked can call it again to be told. You can close this page.",
                done.action
            ),
        )
    } else {
        let why = done
            .error
            .as_ref()
            .map_or("The engine did not say why.", |error| {
                error.message.as_str()
            });
        answer(
            StatusCode::CONFLICT,
            "Carried out, and it did not succeed",
            &format!(
                "`{}` was tried once, as you, and failed: {why}",
                done.action
            ),
        )
    }
}

async fn act_confirm_pressed(inner: &Inner, pressed: &ActPressed) -> (bool, Response) {
    let Some(desk) = inner.desk.get() else {
        return nothing_done(
            StatusCode::SERVICE_UNAVAILABLE,
            "The engine cannot carry out an act now. Run `anything2mcp doctor run` for the reason.",
        );
    };
    // Asked again at the press: only an act that still waits is prompted
    // for, and only one whose input the page showed whole.
    let Ok(ask) = desk.person_act(pressed.act) else {
        return (
            true,
            answer(
                StatusCode::CONFLICT,
                "Nothing was done",
                "The act this page showed no longer waits. Ask the program that asked for it.",
            ),
        );
    };
    if !whole_input(&ask) {
        return nothing_done(
            StatusCode::CONFLICT,
            "A value of this act's input is longer than its page can show, and an act that was not shown whole is not carried out here. You can decline it from the page.",
        );
    }
    let Ok(_up) = inner.prompting.try_lock() else {
        return nothing_done(
            StatusCode::SERVICE_UNAVAILABLE,
            "Another presence prompt is up on this machine. Answer or close it first, then open the link again and press once more.",
        );
    };
    let asked = inner
        .prompt
        .ask(
            PromptText {
                title: format!("Carry out `{}`", pressed.action),
                detail: format!("Act {}", short_act(pressed.act)),
            },
            pressed.deadline,
        )
        .await;
    let asked = if Instant::now() >= pressed.deadline {
        PromptAnswer::Dismissed
    } else {
        asked
    };
    let prompt = match asked {
        PromptAnswer::Present(prompt) => prompt,
        PromptAnswer::Confirmed => {
            return nothing_done(
                StatusCode::FORBIDDEN,
                "A confirmation dialog was answered, which proves no person, so nothing was done.",
            );
        }
        PromptAnswer::Dismissed => {
            return nothing_done(
                StatusCode::FORBIDDEN,
                "The prompt on this machine was closed, or its time ended. Open the link again to answer once more.",
            );
        }
        PromptAnswer::Denied => {
            return nothing_done(
                StatusCode::FORBIDDEN,
                "The prompt on this machine was answered and not passed. Open the link again to answer once more.",
            );
        }
        PromptAnswer::NotAvailable(why) => {
            return nothing_done(
                StatusCode::FORBIDDEN,
                &format!(
                    "No presence prompt is available on this machine, so nothing is done here. {} {} You can decline the act from the page.",
                    why.message, why.remedy
                ),
            );
        }
    };
    let proof = PresenceProof::new(recorded_check(prompt), pressed.person, inner.clock.now())
        .naming(prompt.as_str())
        .for_act(pressed.act);
    match desk.carry_out(pressed.act, proof).await {
        Ok(done) => (true, act_answer(&done)),
        Err(failure) => nothing_done(StatusCode::FORBIDDEN, failure.message()),
    }
}

async fn act_confirm(
    State(inner): State<Arc<Inner>>,
    Path(code): Path<String>,
    body: Bytes,
) -> Response {
    let pressed = match act_begin(&inner, &code, &body) {
        Ok(pressed) => pressed,
        Err(response) => return *response,
    };
    // In a task of its own, as a press of Approve is.
    let carried = tokio::spawn(async move {
        let (settled, response) = act_confirm_pressed(&inner, &pressed).await;
        act_finish(&inner, &code, settled);
        response
    });
    carried.await.unwrap_or_else(|_| not_carried_out())
}

async fn act_decline(
    State(inner): State<Arc<Inner>>,
    Path(code): Path<String>,
    body: Bytes,
) -> Response {
    let pressed = match act_begin(&inner, &code, &body) {
        Ok(pressed) => pressed,
        Err(response) => return *response,
    };
    if let Some(desk) = inner.desk.get() {
        desk.decline_act(pressed.act, true);
    }
    act_finish(&inner, &code, true);
    answer(
        StatusCode::OK,
        "Declined",
        "The act was declined and nothing was done. You can close this page.",
    )
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
                prompting: tokio::sync::Mutex::new(()),
            }),
        }
    }

    /// What this surface's presence prompt can do on this machine now:
    /// the kind, the reason, and the remedy. `doctor.run` reports it.
    pub async fn presence(&self) -> Availability {
        self.inner.prompt.available().await
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
            .route("/act/{code}", get(act_show))
            .route("/act/{code}/confirm", post(act_confirm))
            .route("/act/{code}/decline", post(act_decline))
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

    fn presence(&self) -> BoxFuture<'_, PresenceReport> {
        Box::pin(async move {
            let found = LocalSurface::presence(self).await;
            // Only a prompt the operating system owns proves a person; a
            // dialog alone is reported as no prompt.
            let kind = match (found.kind, found.prompt) {
                (PresenceKind::OsPrompt, Some(prompt)) => prompt.as_str(),
                _ => PresenceReport::NOT_AVAILABLE,
            };
            PresenceReport {
                kind,
                reason: found.reason.as_str(),
                message: found.message,
                fix: found.remedy,
            }
        })
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
            let deadline = Instant::now() + inner.window;
            let timer = tokio::spawn({
                let inner: Weak<Inner> = Arc::downgrade(inner);
                let code = code.clone();
                async move {
                    tokio::time::sleep_until(deadline).await;
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
                    whole: false,
                    deadline,
                    timer,
                },
            );
            inner.settle(&mut state);
            Ok(Self::link(port, &code, minutes))
        })
    }

    fn shows_person_acts(&self) -> bool {
        self.inner.desk.get().is_some()
    }

    fn ask_person_act(&self, ask: PersonActAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>> {
        Box::pin(async move {
            let inner = &self.inner;
            if inner.desk.get().is_none() {
                return Err(codes::busy(
                    codes::SURFACE_UNAVAILABLE,
                    "The engine cannot carry out a person's act now, so no page is shown.",
                    "Run `anything2mcp doctor run` for the reason.",
                ));
            }
            let minutes = inner.window.as_secs().div_ceil(60).max(1);
            let code = inner.code()?;
            let mut state = inner.state();
            let port = match self.listen(&mut state) {
                Ok(port) => port,
                Err(failure) => {
                    inner.settle(&mut state);
                    return Err(failure);
                }
            };
            let deadline = Instant::now() + inner.window;
            let timer = tokio::spawn({
                let inner: Weak<Inner> = Arc::downgrade(inner);
                let code = code.clone();
                async move {
                    tokio::time::sleep_until(deadline).await;
                    if let Some(inner) = inner.upgrade() {
                        inner.end_of_act_window(&code);
                    }
                }
            });
            state.acts.insert(
                code.clone(),
                ActPage {
                    act: ask.act,
                    action: ask.action.to_string(),
                    person: ask.person,
                    once: None,
                    pressed: false,
                    ended: false,
                    deadline,
                    timer,
                },
            );
            inner.settle(&mut state);
            Ok(SurfaceLink {
                url: Some(format!("http://127.0.0.1:{port}/act/{code}")),
                text: format!(
                    "`{}` is a person's act. Open the link in your own browser on this machine to read exactly what will be done and to carry it out or decline it. The page closes after {minutes} minutes; no answer is a decline.",
                    ask.action
                ),
            })
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
            let refused = |message: &str, next: &str| {
                Err(codes::policy(codes::PRESENCE_FAILED, message, next))
            };
            // One prompt at a time, as for an approval.
            let Ok(_up) = self.inner.prompting.try_lock() else {
                return Err(codes::busy(
                    codes::PROMPT_BUSY,
                    "Another presence prompt is up on this machine.",
                    "Answer or close that prompt, then run the command again.",
                ));
            };
            // A prompt that no page asked for is not raised while a page
            // waits for its person: on a system whose dialog shows fixed
            // text, the person reading that page would take it for theirs.
            if self.waiting() > 0 {
                return Err(codes::busy(
                    codes::PROMPT_BUSY,
                    "An approval page waits for its person on this machine, and no other presence prompt is raised meanwhile.",
                    "Answer or let that page close, then run the command again.",
                ));
            }
            let deadline = Instant::now() + self.inner.window;
            let answer = self
                .inner
                .prompt
                .ask(
                    PromptText {
                        title: format!("Confirm `{}`", ask.action),
                        detail: ask.summary,
                    },
                    // A prompt nobody answers ends as an approval's does.
                    deadline,
                )
                .await;
            // An answer after the window is a dismissal, whatever it says.
            let answer = if Instant::now() >= deadline {
                PromptAnswer::Dismissed
            } else {
                answer
            };
            match answer {
                PromptAnswer::Present(prompt) => Ok(PresenceProof::new(
                    recorded_check(prompt),
                    ask.principal,
                    self.inner.clock.now(),
                )
                .naming(prompt.as_str())),
                PromptAnswer::Dismissed => refused(
                    "The prompt on this machine was closed, or its time ended.",
                    "Run the command again and answer the prompt yourself.",
                ),
                PromptAnswer::Denied => refused(
                    "The prompt on this machine was answered and not passed.",
                    "Run the command again and answer the prompt yourself.",
                ),
                PromptAnswer::Confirmed => refused(
                    "A confirmation dialog proves no person, so this cannot be done here.",
                    "Make the change in the installation's settings file, or on a machine whose operating system offers a presence prompt.",
                ),
                PromptAnswer::NotAvailable(why) => refused(
                    &format!(
                        "No presence prompt is available on this machine, so this cannot be done here. {}",
                        why.message
                    ),
                    &if why.remedy.is_empty() {
                        "Make the change in the installation's settings file, or on a machine whose operating system offers a presence prompt.".to_owned()
                    } else {
                        why.remedy
                    },
                ),
            }
        })
    }

    fn ask_session(&self, _ask: SessionAsk) -> BoxFuture<'_, PersonAnswer> {
        // No notification and no return address are built on one machine
        // yet, so the session's person cannot be reached from here.
        Box::pin(async { PersonAnswer::Unreachable })
    }
}
