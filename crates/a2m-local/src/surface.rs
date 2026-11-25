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
    NOW_CANNOT_BE_READ, PersonActAsk, PersonActId, PersonSurface, PresenceAsk, PresenceProof,
    PresenceReport, PreviewRecord, SessionAsk, SignInAsk, SurfaceLink,
};
use a2m_auth::{PersonAnswer, PersonAsk};
use a2m_core::{Digest, Failure, FailureCode, Identity, PrincipalId, RunId};
use a2m_logs::Clock;
use a2m_policy::{Channel as ApprovalChannel, PresenceCheck, PrincipalRef};
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
    Availability, PresenceKind, PresencePrompt, PromptAnswer, PromptFor, PromptName, PromptText,
};

const APPROVAL_PAGE: &str = include_str!("pages/approval.html");
const ANSWER_PAGE: &str = include_str!("pages/answer.html");
const ACT_PAGE: &str = include_str!("pages/act.html");
const CODE_PAGE: &str = include_str!("pages/code.html");
const CAPTURE_PAGE: &str = include_str!("pages/capture.html");
/// How long past a capture's own bound its page stays, so that a person
/// who presses Stop recording late is told it had ended.
const CAPTURE_PAGE_MARGIN: Duration = Duration::from_secs(60);
/// The most characters of what an act came to that its answer page shows.
const RESULT_SHOWN: usize = 2_000;

/// The random bytes in the address's code and in the page's one-time
/// value.
const CODE_BYTES: usize = 16;
/// The most a press may send: one short field.
const PRESS_LIMIT: usize = 256;
/// The most a press of Carry out may send: the one-time value and, for a
/// sign-in, the credentials its page asked for (decision 0205).
const ACT_PRESS_LIMIT: usize = 16 * 1024;
/// The name a field of a secret has on an act's page: this, then the
/// secret's role.
const SECRET_FIELD: &str = "secret.";
/// The most records of one change a page shows. A change with more cannot
/// be read in full here, so it cannot be approved here either.
const RECORDS_SHOWN: usize = 2_000;
/// How many characters of the hash of a change the page shows beside
/// Approve, and a prompt that carries words repeats.
const SHORT_HASH: usize = 12;
/// How many answered changes are remembered. An older one that is asked
/// about again gets a new page, which the desk then refuses to act on.
const ANSWERED_KEPT: usize = 256;
/// How each field of a sign-in's secret is served (decision 0999): a text
/// field, so that no browser takes it for a password to keep; masked by the
/// act page's style; with no completion, spelling check, or change of case,
/// so that what is typed is neither remembered as form history nor sent to
/// a spelling service.
const SECRET_INPUT: &str = "type=\"text\" class=\"secret\" autocomplete=\"off\" autocapitalize=\"off\" autocorrect=\"off\" spellcheck=\"false\"";

/// The sentence beside each field of a sign-in's secret.
const SECRET_KEEP: &str = "If your browser offers to save or remember what you typed here, decline: it belongs in the engine's vault only.";

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
    /// The effect class of the operation the write carries out.
    effect: a2m_core::EffectClass,
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
    /// The presses of Carry out being carried out, each with the way a
    /// request of the session keeper for a code reaches its page.
    presses: Vec<InFlight>,
    /// The code pages that wait for a person's one-time code, by the code
    /// in their address.
    codes: BTreeMap<String, CodePage>,
    /// The pages of captures that record, by the code in their address:
    /// where their person ends them (decision 0392, point 2).
    captures: BTreeMap<String, CapturePage>,
}

/// The page of one capture its person began: it says what is kept, and
/// its one press ends the recording.
struct CapturePage {
    capture: String,
    person: PrincipalId,
    target: String,
    surface: String,
    bound_seconds: u64,
    /// The one-time value of the page served last. A press takes it.
    once: Option<String>,
    timer: JoinHandle<()>,
}

/// A press of Carry out being carried out. While it is, the session
/// keeper's request for a one-time code of the same person is answered on
/// that press's own page (decision 0205): the press's answer is the page
/// that asks for the code.
struct InFlight {
    id: u64,
    person: PrincipalId,
    /// Whether the act is a sign-in: only a sign-in's press is asked for a
    /// code.
    signs_in: bool,
    /// Told the code of the page that asks for a code, once.
    tell: Option<oneshot::Sender<String>>,
}

/// A page that waits for a person's one-time code for one sign-in.
struct CodePage {
    /// The target and the environment of the sign-in, as the page names
    /// them.
    target: String,
    environment: String,
    /// The one-time value of the page. A post takes it.
    once: Option<String>,
    /// Where the code goes. Taken by the post that carries it.
    answer: Option<oneshot::Sender<a2m_core::Secret>>,
    /// The press the code is for, which ends with the act's answer: the
    /// post that carries the code answers with it.
    carried: Option<JoinHandle<Response>>,
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
pub(crate) const fn recorded_check(prompt: PromptName) -> PresenceCheck {
    match prompt {
        PromptName::Polkit => PresenceCheck::Polkit,
        PromptName::LocalAuthentication => PresenceCheck::LocalAuthentication,
        PromptName::WindowsHello => PresenceCheck::WindowsHello,
        PromptName::CopyTestHarness => PresenceCheck::CopyTestHarness,
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

/// The header every refusal of the loopback listener names its failure
/// code in, so that whatever sent the request, a person's browser or a
/// program, is told by name why nothing was recorded. The page's own words
/// say the same to a person.
pub const FAILURE_CODE_HEADER: &str = "a2m-failure-code";

/// A refusal: the answer page, with the failure code in
/// [`FAILURE_CODE_HEADER`].
fn refused(status: StatusCode, code: &FailureCode, title: &str, detail: &str) -> Response {
    let mut response = answer(status, title, detail);
    if let Ok(value) = HeaderValue::from_str(code.as_str()) {
        response.headers_mut().insert(FAILURE_CODE_HEADER, value);
    }
    response
}

fn nothing_here() -> Response {
    refused(
        StatusCode::NOT_FOUND,
        &codes::PAGE_NOT_FOUND,
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
            && state.captures.is_empty()
            && let Some(listening) = state.listening.take()
        {
            let _ = listening.stop.send(());
            state.closing.push(listening.task);
        }
        self.waiting
            .send_replace(state.pages.len() + state.acts.len() + state.captures.len());
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
        return refused(
            StatusCode::FORBIDDEN,
            &codes::REQUEST_NOT_FROM_PAGE,
            "Not from this machine's own page",
            "This page answers only the browser that opened its link on this machine.",
        );
    }
    next.run(request).await
}

/// The fields of a form a press sent, `application/x-www-form-urlencoded`,
/// each name and value decoded. `None` for a body longer than `limit`, or
/// one that is not such a form of text.
fn form_of(body: &Bytes, limit: usize) -> Option<Vec<(String, String)>> {
    fn decoded(part: &str) -> Option<String> {
        let bytes = part.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut at = 0;
        while at < bytes.len() {
            match bytes[at] {
                b'+' => out.push(b' '),
                b'%' => {
                    let hex = part.get(at + 1..at + 3)?;
                    out.push(u8::from_str_radix(hex, 16).ok()?);
                    at += 2;
                }
                other => out.push(other),
            }
            at += 1;
        }
        String::from_utf8(out).ok()
    }
    if body.len() > limit {
        return None;
    }
    std::str::from_utf8(body)
        .ok()?
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            Some((decoded(name)?, decoded(value)?))
        })
        .collect()
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
pub(crate) struct Change {
    /// The first reading: the tool, the target, the counts.
    pub(crate) head: ApprovalPreview,
    /// The records, in the order the run would write them.
    pub(crate) records: Vec<PreviewRecord>,
    /// Whether these are all the records of the change.
    pub(crate) whole: bool,
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
pub(crate) fn change_html(change: &Change) -> String {
    use std::fmt::Write as _;
    let head = &change.head;
    let mut out = String::new();
    let _ = write!(
        out,
        "<p>Target <code>{}</code>, environment <code>{}</code>. {} to create, {} to change, {} already as intended{}: {} records in all.</p>",
        escape(&head.target),
        escape(&head.environment),
        head.create,
        head.update,
        head.unchanged,
        if head.write == 0 {
            String::new()
        } else {
            format!(
                ", {} to write, whose current values cannot be read",
                head.write
            )
        },
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
        out.push_str(&version_shown::notices_html(&record.notices));
        out.push_str("<table><thead><tr><th scope=\"col\">Field</th><th scope=\"col\">Now</th><th scope=\"col\">After the write</th><th scope=\"col\">Changes</th></tr></thead><tbody>");
        for field in &record.fields {
            // What the record holds now: its value; the product's own
            // words, set apart, where the binding cannot read it, never as
            // a value and never as "does not exist" (decision 0196).
            let now = if field.now_cannot_be_read {
                format!(
                    "<em class=\"product\">This program's words: {}</em>",
                    escape(NOW_CANNOT_BE_READ)
                )
            } else {
                field.current.as_ref().map_or_else(
                    || "<em>does not exist</em>".to_owned(),
                    |current| escape(current),
                )
            };
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

/// What a page says of a version's surfaces and of a record's surface:
/// each surface's kind and where it leads in every environment, each
/// notice a pack registers for it in the notice's fixed words, and every
/// lint warning of the version with its code and words (decisions 0193,
/// 0194, and 0199; AC-1 of the third audit). It prints what the action
/// registry hands the page (`PreviewRecord::notices`,
/// `PersonActAsk::surfaces`, and `PersonActAsk::warnings`). Every text may
/// quote a target file: each is shown with the characters that reorder,
/// hide, or restyle text written as their escapes, and then escaped for
/// the page.
pub(crate) mod version_shown {
    use std::fmt::Write as _;

    use a2m_actions::{ActSurface, ActWarning, NEW_PLACE, PreviewNotice};

    use super::escape;

    /// Text from a target file, as the page places it: every character
    /// that can reorder or hide what a person reads written as its escape,
    /// so an address reads as it is, then escaped as markup.
    fn shown(text: &str) -> String {
        escape(&a2m_core::said_neutral(text))
    }

    /// The notices of a record's surfaces, beside the record, each in its
    /// own strong sentence. Nothing for a record whose surfaces carry none.
    pub(crate) fn notices_html(notices: &[PreviewNotice]) -> String {
        let mut out = String::new();
        for notice in notices {
            let _ = write!(
                out,
                "<p class=\"notice\"><strong>{}</strong> (<code>{}</code>)</p>",
                shown(notice.words),
                shown(notice.code),
            );
        }
        out
    }

    /// The mark beside an address no person registered for the target, in
    /// the kernel's fixed words (decision 0413); nothing beside any other.
    fn new_place(new: bool) -> String {
        if new {
            format!(" <strong class=\"new-place\">({NEW_PLACE})</strong>")
        } else {
            String::new()
        }
    }

    /// The surfaces of a version and its lint warnings, for the page a
    /// person publishes from. Every surface is listed with its kind and its
    /// address in each environment, named, since a version runs against
    /// each, so a person sees where each leads, not only its name.
    pub(crate) fn version_html(surfaces: &[ActSurface], warnings: &[ActWarning]) -> String {
        let mut out = String::new();
        if !surfaces.is_empty() {
            out.push_str("<h2>Where it leads</h2><ul>");
            for surface in surfaces {
                let _ = write!(
                    out,
                    "<li><code>{}</code>, a surface of the kind <code>{}</code>, leads to ",
                    shown(&surface.name),
                    shown(&surface.kind),
                );
                if surface.addresses.is_empty() {
                    let _ = write!(
                        out,
                        "<code>{}</code>{}.",
                        shown(&surface.address),
                        new_place(surface.new_here),
                    );
                } else {
                    let at: Vec<String> = surface
                        .addresses
                        .iter()
                        .map(|(environment, address)| {
                            format!(
                                "<code>{}</code> in the environment <code>{}</code>{}",
                                shown(address),
                                shown(environment),
                                new_place(surface.new_in.contains(environment)),
                            )
                        })
                        .collect();
                    let _ = write!(out, "{}.", at.join("; "));
                }
                out.push_str(&notices_html(&surface.notices));
                out.push_str("</li>");
            }
            out.push_str("</ul>");
        }
        if !warnings.is_empty() {
            out.push_str("<h2>What the checks warn of</h2><ul>");
            for warning in warnings {
                let _ = write!(
                    out,
                    "<li><code>{}</code>: {}</li>",
                    shown(&warning.code),
                    shown(&warning.message),
                );
            }
            out.push_str("</ul>");
        }
        out
    }
}

/// What the page a person publishes from says of each sign-in and each
/// tool of the version, from the typed review summary the action registry
/// hands it (`PersonActAsk::review`; decisions 0252 and 0264). Sign-ins
/// come first: a credential sent to an address puts every tool of the
/// surfaces it covers at risk. Then the tools, in the summary's own order,
/// the most dangerous first. Every text came from a target file or a
/// provider and passed the redactor; each is shown with the characters
/// that reorder or hide text written as their escapes, and then escaped.
/// The only sentences are the page's own and the summary's constants. A
/// secret variable's value is never in the summary, and the page says so.
pub(crate) mod review_shown {
    use std::fmt::Write as _;

    use a2m_core::AddressScope;
    use a2m_files::{
        AssertedReview, BindingStates, Change, ProgramRun, Reach, ReachGroup, ReviewItem,
        ReviewSummary, SentTo, SessionCheckReview, SignInReview, ToolChange, VariableValue,
        VersionChanges,
    };

    use super::escape;

    fn shown(text: &str) -> String {
        escape(&a2m_core::said_neutral(text))
    }

    fn code(text: &str) -> String {
        format!("<code>{}</code>", shown(text))
    }

    fn codes<T: ToString>(items: &[T]) -> String {
        if items.is_empty() {
            return "none".to_owned();
        }
        items
            .iter()
            .map(|item| code(&item.to_string()))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// What a kind of place is, in words a person reads.
    fn scope_words(scope: AddressScope) -> &'static str {
        match scope {
            AddressScope::Unreadable => "an address that cannot be read as any kind of place",
            AddressScope::Public => "public addresses, which anyone can reach",
            AddressScope::Name => "names, which may lead to any address",
            AddressScope::Unspecified => "the unspecified address",
            AddressScope::Multicast => "multicast addresses",
            AddressScope::CarrierGradeNat => "addresses of a carrier's shared range",
            AddressScope::PrivateUse => "addresses of a private network",
            AddressScope::UniqueLocal => "unique local addresses",
            AddressScope::LinkLocal => "link-local addresses",
            AddressScope::Reserved => "reserved addresses",
            AddressScope::Documentation => "addresses kept for documentation",
            AddressScope::ThisMachine => "this machine",
        }
    }

    fn groups_html(groups: &[ReachGroup]) -> String {
        let mut out = String::from("<ul>");
        for group in groups {
            let _ = write!(
                out,
                "<li>{} (<code>{}</code>): {}</li>",
                escape(scope_words(group.scope)),
                group.scope,
                codes(&group.addresses),
            );
        }
        out.push_str("</ul>");
        out
    }

    fn sign_in_html(sign_in: &SignInReview) -> String {
        let mut out = String::from("<li>");
        if let SessionCheckReview::Pending { name, says } = &sign_in.probe {
            let _ = write!(
                out,
                "<p><strong>Not confirmed: {}</strong> (the read {})</p>",
                escape(says),
                code(name),
            );
        }
        let _ = write!(
            out,
            "<p>Signs in to {} through the provider {}, {}. ",
            codes(&sign_in.surfaces),
            code(sign_in.provider.as_str()),
            code(sign_in.mode.as_str()),
        );
        if sign_in.credentials.is_empty() {
            out.push_str("It uses no stored credential.");
        } else {
            let credentials: Vec<String> = sign_in
                .credentials
                .iter()
                .map(|credential| {
                    format!(
                        "{} kept at {}",
                        code(&credential.role),
                        code(&credential.reference.to_string())
                    )
                })
                .collect();
            let _ = write!(
                out,
                "Its credentials, by name, never by value: {}.",
                credentials.join("; ")
            );
        }
        if let SessionCheckReview::Declared { name } = &sign_in.probe {
            let _ = write!(out, " The read {} checks the session.", code(name));
        }
        out.push_str("</p><ul>");
        for sent in &sign_in.sent_to {
            let _ = write!(
                out,
                "<li>In the environment {}, a credential or a token of it is sent to: ",
                code(sent.environment.as_str())
            );
            match &sent.sent_to {
                SentTo::NotStated { says } | SentTo::ProviderNotHeld { says } => {
                    let _ = write!(out, "<strong>{}</strong>.", escape(says));
                }
                SentTo::Addresses {
                    groups,
                    leaves_this_machine,
                } => {
                    if *leaves_this_machine {
                        out.push_str("<strong>it may leave this machine</strong>.");
                    } else {
                        out.push_str("this machine only.");
                    }
                    out.push_str("<ul>");
                    for group in groups {
                        let addresses: Vec<String> = group
                            .addresses
                            .iter()
                            .map(|sent| format!("{}, {}", code(&sent.address), shown(&sent.what)))
                            .collect();
                        let _ = write!(
                            out,
                            "<li>{} (<code>{}</code>): {}</li>",
                            escape(scope_words(group.scope)),
                            group.scope,
                            addresses.join("; "),
                        );
                    }
                    out.push_str("</ul>It is sent to no other address.");
                }
            }
            out.push_str("</li>");
        }
        out.push_str("</ul></li>");
        out
    }

    fn run_html(run: &ProgramRun, notice: Option<&str>) -> String {
        let digests = |digests: &[a2m_core::Digest]| {
            if digests.is_empty() {
                "pinned to no digest".to_owned()
            } else {
                format!("pinned to the digest {}", codes(digests))
            }
        };
        let mut out = match run {
            ProgramRun::Program {
                path,
                digests: pins,
            } => {
                format!("Starts the program {}, {}.", code(path), digests(pins))
            }
            ProgramRun::Script {
                interpreter,
                interpreter_digests,
                arguments,
                script,
                script_digest,
            } => format!(
                "Starts the interpreter {}, {}, with the fixed arguments {}, on the script {} the target keeps, pinned to the digest {}.",
                code(interpreter),
                digests(interpreter_digests),
                codes(arguments),
                code(script),
                code(&script_digest.to_string()),
            ),
            ProgramRun::Launcher {
                launcher,
                launcher_digests,
                arguments,
                entry,
                entry_digest,
            } => format!(
                "Starts the launcher {}, {}, with the fixed arguments {}, on the entry file {}, pinned to the digest {}.",
                code(launcher),
                digests(launcher_digests),
                codes(arguments),
                code(entry),
                code(&entry_digest.to_string()),
            ),
        };
        if let Some(notice) = notice {
            let _ = write!(out, " <strong>{}</strong>.", escape(notice));
        }
        out
    }

    /// One change to a tool, in the page's own words, from its typed
    /// fields (decision 0439).
    fn change_words(change: &Change) -> String {
        match change {
            Change::ToolGone => "the tool is gone: a call to it is refused".to_owned(),
            Change::InputGone { input } => {
                format!(
                    "the input {} is gone or renamed: a call that gives it is refused",
                    code(input)
                )
            }
            Change::InputRequired { input } => format!(
                "the input {} must now be given: a call without it is refused",
                code(input)
            ),
            Change::InputTypeChanged { input, from, to } => format!(
                "the input {} was a {} and is now a {}",
                code(input),
                code(from),
                code(to)
            ),
            Change::InputWordsNarrowed { input, removed } => {
                if removed.is_empty() {
                    format!(
                        "the input {} now takes only a fixed list of words",
                        code(input)
                    )
                } else {
                    format!(
                        "the input {} no longer takes {}",
                        code(input),
                        codes(removed)
                    )
                }
            }
            Change::OutputGone { output } => {
                format!("the output {} is gone", code(output))
            }
            Change::OutputMayBeAbsent { output } => format!(
                "the output {} was always present and may now be absent",
                code(output)
            ),
            Change::OutputTypeChanged { output, from, to } => format!(
                "the output {} was a {} and is now a {}",
                code(output),
                code(from),
                code(to)
            ),
            Change::EffectRose { from, to } => format!(
                "its effect rose from {} to {}",
                code(from.as_str()),
                code(to.as_str())
            ),
            Change::HazardChanged { from, to } => format!(
                "its hazard was {} and is now {}",
                code(from.as_str()),
                code(to.as_str())
            ),
            Change::KeyChanged { from, to } => format!(
                "the inputs that name its record were {} and are now {}",
                codes(from),
                codes(to)
            ),
            Change::ToolAdded => "the tool is new".to_owned(),
            Change::InputAdded { input } => {
                format!("a new input {}, which a call may leave out", code(input))
            }
            Change::InputLoosened { input } => {
                format!("the input {} may now be left out", code(input))
            }
            Change::InputWordsWidened { input, added } => {
                format!("the input {} also takes {}", code(input), codes(added))
            }
            Change::OutputAdded { output } => format!("a new output {}", code(output)),
            Change::OutputAlwaysPresent { output } => {
                format!("the output {} is now always present", code(output))
            }
            Change::OutputWordsChanged {
                output,
                added,
                removed,
            } => format!(
                "the words the output {} may hold changed: added {}, removed {}",
                code(output),
                codes(added),
                codes(removed)
            ),
            Change::EffectLowered { from, to } => format!(
                "its effect fell from {} to {}",
                code(from.as_str()),
                code(to.as_str())
            ),
            Change::HazardRemoved { from } => {
                format!("its hazard {} is gone", code(from.as_str()))
            }
            Change::DescriptionChanged => "its description changed".to_owned(),
            Change::DefinitionChanged => {
                "something else a client reads in it changed, such as its title".to_owned()
            }
        }
    }

    fn changes_list(changes: &[Change]) -> String {
        let mut out = String::from("<ul>");
        for change in changes {
            let _ = write!(out, "<li>{}.</li>", change_words(change));
        }
        out.push_str("</ul>");
        out
    }

    /// Each tool whose change breaks an agent that calls it as it was,
    /// with why: on the page a person publishes from, right after where
    /// each surface leads and before the sign-ins (decisions 0439 and
    /// 0454). The publish is not refused.
    pub(crate) fn breaking_html(changes: &VersionChanges) -> String {
        let breaking: Vec<&ToolChange> = changes.breaking().collect();
        if breaking.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        let _ = write!(
            out,
            "<h2>What breaks</h2><p>Read these first. Compared with version {}, the version in force, each of these tools changes in a way that breaks an agent that calls it as it was. You may still publish.</p><ul>",
            code(&changes.from.to_string()),
        );
        for tool in breaking {
            let _ = write!(
                out,
                "<li><p><strong>{}</strong>:</p>",
                code(tool.tool.as_str())
            );
            out.push_str(&changes_list(&tool.changes));
            out.push_str("</li>");
        }
        out.push_str("</ul>");
        out
    }

    fn tool_html(item: &ReviewItem, changes: Option<&VersionChanges>) -> String {
        let mut out = String::new();
        let _ = write!(
            out,
            "<li><p><strong>{}</strong>: effect {}, hazard {}, {} commit points; surfaces {}.</p>",
            code(item.tool.as_str()),
            code(item.effect.as_str()),
            code(item.hazard.as_str()),
            item.commit_points,
            codes(&item.surfaces),
        );
        // What changed since the version in force, where nothing breaks:
        // a break is named in its own section, first.
        if let Some((changes, tool)) = changes
            .and_then(|changes| changes.of(&item.tool).map(|tool| (changes, tool)))
            .filter(|(_, tool)| tool.class == a2m_files::ChangeClass::Compatible)
        {
            let _ = write!(
                out,
                "<p>Changed since version {}, and nothing that breaks:</p>",
                code(&changes.from.to_string())
            );
            out.push_str(&changes_list(&tool.changes));
        }
        // Where it sends, first.
        match &item.reach {
            Reach::Partly { known, says } => {
                let _ = write!(out, "<p><strong>{}</strong>.", escape(says));
                if known.is_empty() {
                    out.push_str(" No address is known.</p>");
                } else {
                    out.push_str(" Known:</p>");
                    out.push_str(&groups_html(known));
                }
            }
            Reach::Stated {
                groups,
                leaves_this_machine,
            } => {
                if groups.is_empty() {
                    out.push_str("<p>It sends no request to any address.</p>");
                } else {
                    if *leaves_this_machine {
                        out.push_str("<p><strong>It may send to an address off this machine.</strong> It sends to these, and to no other address:</p>");
                    } else {
                        out.push_str(
                            "<p>It sends to these, all on this machine, and to no other address:</p>",
                        );
                    }
                    out.push_str(&groups_html(groups));
                }
            }
        }
        if !item.uploads.is_empty() {
            let _ = write!(
                out,
                "<p>It may send the files {}.</p>",
                codes(&item.uploads)
            );
        }
        if !item.downloads.is_empty() {
            let _ = write!(
                out,
                "<p>It may receive the files {}.</p>",
                codes(&item.downloads)
            );
        }
        // What it starts and sets, binding by binding.
        if !item.bindings.is_empty() {
            out.push_str("<ul>");
        }
        for binding in &item.bindings {
            let _ = write!(
                out,
                "<li>Through the mode {} on the surface {}, for {}: ",
                code(binding.mode.as_str()),
                code(binding.surface.as_str()),
                code(binding.operation.as_str()),
            );
            match &binding.states {
                BindingStates::NotStated { says } => {
                    let _ = write!(out, "<strong>{}</strong>.", escape(says));
                }
                BindingStates::Stated { runs, environment } => {
                    if runs.is_empty() {
                        out.push_str("it starts no program.");
                    }
                    for run in runs {
                        out.push(' ');
                        out.push_str(&run_html(&run.run, run.notice));
                    }
                    if environment.is_empty() {
                        out.push_str(" It sets no variable.");
                    } else {
                        out.push_str(" Its variables:<ul>");
                        for variable in environment {
                            let value = match &variable.value {
                                VariableValue::Set(value) => format!("set to {}", code(value)),
                                VariableValue::Secret => "secret, not shown".to_owned(),
                                VariableValue::Passed => {
                                    "passed from the engine's own environment when it is set there"
                                        .to_owned()
                                }
                            };
                            let _ = write!(out, "<li>{}: {value}</li>", code(&variable.name));
                        }
                        out.push_str("</ul>");
                    }
                }
            }
            if !binding.surface_programs.is_empty() {
                let programs: Vec<String> = binding
                    .surface_programs
                    .iter()
                    .map(|program| code(&program.path))
                    .collect();
                let _ = write!(
                    out,
                    " The surface declares the programs {}.",
                    programs.join(", ")
                );
            }
            out.push_str("</li>");
        }
        if !item.bindings.is_empty() {
            out.push_str("</ul>");
        }
        if let Some(place) = &item.place {
            let _ = write!(
                out,
                "<p>Its record is one fixed place: {}, of the kind {}, on the surface {}.</p>",
                code(&place.name),
                code(place.kind.as_str()),
                code(place.surface.as_str()),
            );
        }
        out.push_str("</li>");
        out
    }

    /// One asserted read: the request, the entry that asserts it, its
    /// body's fields, and the summary's fixed sentence in bold.
    fn asserted_html(asserted: &AssertedReview) -> String {
        let mut out = String::from("<li>");
        let _ = write!(
            out,
            "<p>{} {} on the surface {}, asserted by the {} {}: <strong>{}</strong>.</p>",
            code(&asserted.read.method),
            code(&asserted.read.path),
            code(asserted.surface.as_str()),
            code(asserted.kind.as_str()),
            code(&asserted.read.entry),
            escape(asserted.says),
        );
        if asserted.read.fields.is_empty() {
            out.push_str("<p>It sends no field.</p>");
        } else {
            out.push_str("<p>The fields it may send, and no other:</p><ul>");
            for field in &asserted.read.fields {
                let _ = write!(out, "<li>{}, a {}", code(&field.name), code(&field.kind));
                if !field.one_of.is_empty() {
                    let _ = write!(out, ", one of {}", codes(&field.one_of));
                }
                out.push_str("</li>");
            }
            out.push_str("</ul>");
        }
        out.push_str("</li>");
        out
    }

    /// The sign-ins of a version, then the reads a person asserted, then
    /// its tools. The surfaces come before all of them, from
    /// `version_shown` (decision 0418).
    pub(crate) fn review_html(review: &ReviewSummary) -> String {
        let mut out = String::new();
        if !review.sign_ins.is_empty() {
            out.push_str("<h2>How it signs in</h2><p>Read these first: a credential sent to an address puts every tool of the surfaces it covers at risk.</p><ul>");
            for sign_in in &review.sign_ins {
                out.push_str(&sign_in_html(sign_in));
            }
            out.push_str("</ul>");
        }
        if !review.asserted.is_empty() {
            out.push_str("<h2>The reads a person asserted</h2><p>Each of these requests is sent by a method that may change the system, and the browser lets it through for every read of its surface, whichever tool sends it.</p><ul>");
            for asserted in &review.asserted {
                out.push_str(&asserted_html(asserted));
            }
            out.push_str("</ul>");
        }
        if !review.items.is_empty() {
            out.push_str("<h2>What each tool may do</h2><p>The most dangerous first.</p><ul>");
            for item in &review.items {
                out.push_str(&tool_html(item, review.changes.as_ref()));
            }
            out.push_str("</ul>");
        }
        out
    }
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

const GONE_DETAIL: &str = "The change this page was opened for no longer waits for an approval. Look at the run again for where it stands.";

fn gone() -> Response {
    refused(
        StatusCode::CONFLICT,
        &a2m_policy::APPROVAL_STALE,
        "Nothing to approve here",
        GONE_DETAIL,
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
    effect: a2m_core::EffectClass,
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
        return Err(Box::new(refused(
            StatusCode::FORBIDDEN,
            &codes::REQUEST_NOT_FROM_PAGE,
            "Nothing was recorded",
            "The press did not come from the page as it was last shown. Open the link again and press there.",
        )));
    }
    if found.pressed {
        return Err(Box::new(refused(
            StatusCode::CONFLICT,
            &codes::PROMPT_BUSY,
            "A press is already being answered",
            "Answer the prompt on this machine, or open the link again when it has closed.",
        )));
    }
    found.pressed = true;
    Ok(Pressed {
        run: found.run,
        tool: found.tool.clone(),
        diff_hash: found.diff_hash,
        effect: found.effect,
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

fn nothing_approved(status: StatusCode, code: &FailureCode, detail: &str) -> (Outcome, Response) {
    (
        Outcome::Open,
        refused(status, code, "Nothing was approved", detail),
    )
}

/// Carries out a press of Approve, and says how it ended.
async fn approve_pressed(inner: &Inner, pressed: &Pressed) -> (Outcome, Response) {
    let Some(desk) = inner.desk.get() else {
        return nothing_approved(
            StatusCode::SERVICE_UNAVAILABLE,
            &codes::SURFACE_UNAVAILABLE,
            "The engine cannot record an approval now. Run `anything2mcp doctor run` for the reason.",
        );
    };
    if !pressed.whole {
        return nothing_approved(
            StatusCode::CONFLICT,
            &codes::CHANGE_NOT_SHOWN_WHOLE,
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
            refused(
                StatusCode::CONFLICT,
                &a2m_policy::APPROVAL_STALE,
                "Nothing was approved",
                "The change this page showed no longer waits for an approval. Look at the run again for where it stands.",
            ),
        );
    }
    // One prompt at a time. A second press is told so and raises nothing.
    let Ok(_up) = inner.prompting.try_lock() else {
        return nothing_approved(
            StatusCode::SERVICE_UNAVAILABLE,
            &codes::PROMPT_BUSY,
            "Another presence prompt is up on this machine. Answer or close it first, then open the link again and press once more.",
        );
    };
    let asked = inner
        .prompt
        .ask(
            PromptText {
                title: format!("Approve a write by `{}`", pressed.tool),
                detail: format!("Change {}", short(pressed.diff_hash)),
                purpose: PromptFor::Approval(Some(pressed.effect)),
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
                &codes::PRESENCE_FAILED,
                "The prompt on this machine was closed, or its time ended. Open the link again to answer once more.",
            );
        }
        PromptAnswer::Denied => {
            return nothing_approved(
                StatusCode::FORBIDDEN,
                &codes::PRESENCE_FAILED,
                "The prompt on this machine was answered and not passed. Open the link again to answer once more.",
            );
        }
        PromptAnswer::NotAvailable(why) => {
            return nothing_approved(
                StatusCode::FORBIDDEN,
                &codes::PRESENCE_FAILED,
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
        Err(failure) => nothing_approved(StatusCode::FORBIDDEN, failure.code(), failure.message()),
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
    let lines = |heading: &str| {
        let mut out = String::new();
        if !ask.details.is_empty() {
            out.push_str(heading);
            out.push_str("<ul>");
            for line in &ask.details {
                let _ = write!(out, "<li>{}</li>", escape(line));
            }
            out.push_str("</ul>");
        }
        out
    };
    let details = match &ask.review {
        // A publish: where each surface leads first, each new place marked
        // there; then the sign-ins and the tools; then the kernel's own
        // lines and the checks' warnings (decisions 0264 and 0413).
        Some(review) => {
            // Where it can reach first, the page's safety question
            // (decision 0418); then what breaks an agent that calls a tool
            // as it was, before the sign-ins (decision 0454).
            let mut details = version_shown::version_html(&ask.surfaces, &[]);
            if let Some(changes) = &review.changes {
                details.push_str(&review_shown::breaking_html(changes));
            }
            details.push_str(&review_shown::review_html(review));
            details.push_str(&lines("<h2>Before you publish</h2>"));
            details.push_str(&version_shown::version_html(&[], &ask.warnings));
            details
        }
        None => {
            let mut details = lines("<h2>What it may do</h2>");
            details.push_str(&version_shown::version_html(&ask.surfaces, &ask.warnings));
            details
        }
    };
    (input, details)
}

/// The fields of the secrets a sign-in's page asks for, inside the form
/// of Carry out. Each is empty when served and is never filled in again:
/// what the person types goes with their press and is shown nowhere.
///
/// A field is never of type `password` (decision 0999): a browser offers
/// to keep what is typed in a password field whatever the page asks, in
/// its own store and its sync, outside the vault. Each is a text field the
/// page's style masks, that asks for no completion, spelling check, or
/// change of case, and says beside it not to let the browser keep it.
fn secrets_html(ask: &PersonActAsk) -> String {
    use std::fmt::Write as _;
    if ask.secrets.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "<fieldset><legend>Sign-in</legend><p>Type what the system's sign-in needs. It goes with your press to this machine's engine only, is sealed in its vault for you alone, and is never shown to the program that asked. Your browser may show what you type in the clear: type it where nobody can read your screen.</p>",
    );
    for (at, secret) in ask.secrets.iter().enumerate() {
        let _ = write!(
            out,
            "<p><label for=\"secret-{at}\">{}</label> <input id=\"secret-{at}\" name=\"{SECRET_FIELD}{}\" {SECRET_INPUT}{}> {SECRET_KEEP}{}</p>",
            escape(&secret.role),
            escape(&secret.role),
            if secret.stored { "" } else { " required" },
            if secret.stored {
                " Stored already: leave it empty to use the stored one."
            } else {
                ""
            },
        );
    }
    out.push_str("</fieldset>");
    out
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
        return refused(
            StatusCode::CONFLICT,
            &codes::SURFACE_UNAVAILABLE,
            "Nothing to approve here",
            GONE_DETAIL,
        );
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
                ("secrets", &secrets_html(&ask)),
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
    /// What the person typed into the page's fields of secrets, by role.
    /// It goes to the desk with the proof, and nowhere else.
    given: a2m_auth::Returned,
}

/// Takes the act page's one-time value and marks it as pressed, as
/// [`begin`] does for an approval.
fn act_begin(inner: &Inner, code: &str, body: &Bytes) -> Result<ActPressed, Box<Response>> {
    let mut state = inner.state();
    let Some(found) = state.acts.get_mut(code) else {
        return Err(Box::new(nothing_here()));
    };
    let expected = found.once.take();
    let fields = form_of(body, ACT_PRESS_LIMIT).unwrap_or_default();
    let once = fields
        .iter()
        .find(|(name, _)| name == "once")
        .map(|(_, value)| value.as_str());
    let fits = match (expected.as_deref(), once) {
        (Some(expected), Some(given)) => same(expected, given),
        _ => false,
    };
    // Each secret is held as one from here on; the desk registers it with
    // the redactor before it does anything else.
    let mut given = a2m_auth::Returned::new();
    for (name, value) in fields {
        if let Some(role) = name.strip_prefix(SECRET_FIELD) {
            given = given.with(role, a2m_core::Secret::new(value));
        }
    }
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
        given,
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

/// As [`nothing_done`], naming the failure code, as a refusal of an
/// approval does, so that whatever pressed, a person's browser or a
/// program, is told by name why nothing was done.
fn nothing_done_coded(status: StatusCode, code: &FailureCode, detail: &str) -> (bool, Response) {
    (false, refused(status, code, "Nothing was done", detail))
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

async fn act_confirm_pressed(inner: &Arc<Inner>, pressed: &mut ActPressed) -> (bool, Response) {
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
        return nothing_done_coded(
            StatusCode::SERVICE_UNAVAILABLE,
            &codes::PROMPT_BUSY,
            "Another presence prompt is up on this machine. Answer or close it first, then open the link again and press once more.",
        );
    };
    let asked = inner
        .prompt
        .ask(
            PromptText {
                title: format!("Carry out `{}`", pressed.action),
                detail: format!("Act {}", short_act(pressed.act)),
                purpose: PromptFor::Act,
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
            return nothing_done_coded(
                StatusCode::FORBIDDEN,
                &codes::PRESENCE_FAILED,
                "A confirmation dialog was answered, which proves no person, so nothing was done.",
            );
        }
        PromptAnswer::Dismissed => {
            return nothing_done_coded(
                StatusCode::FORBIDDEN,
                &codes::PRESENCE_FAILED,
                "The prompt on this machine was closed, or its time ended. Open the link again to answer once more.",
            );
        }
        PromptAnswer::Denied => {
            return nothing_done_coded(
                StatusCode::FORBIDDEN,
                &codes::PRESENCE_FAILED,
                "The prompt on this machine was answered and not passed. Open the link again to answer once more.",
            );
        }
        PromptAnswer::NotAvailable(why) => {
            return nothing_done_coded(
                StatusCode::FORBIDDEN,
                &codes::PRESENCE_FAILED,
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
    let given = std::mem::take(&mut pressed.given);
    let carried = if given.is_empty() {
        desk.carry_out(pressed.act, proof).await
    } else {
        desk.carry_out_with(pressed.act, proof, given).await
    };
    match carried {
        Ok(done) => match recording(&done) {
            // A capture its person has just begun: the answer is its page,
            // where they end it (decision 0392, point 2).
            Some(begun) => (true, capture_page_opened(inner, pressed.person, begun)),
            None => (true, act_answer(&done)),
        },
        Err(failure) => nothing_done(StatusCode::FORBIDDEN, failure.message()),
    }
}

/// A capture an act has just begun, as the engine answered it: its
/// identifier, target, surface, and bound.
struct Begun {
    capture: String,
    target: String,
    surface: String,
    bound_seconds: u64,
}

fn recording(done: &a2m_actions::Response) -> Option<Begun> {
    if !done.ok || done.action != "capture.start" {
        return None;
    }
    let result = done.result.as_ref()?;
    if result.get("state")?.as_str()? != "recording" {
        return None;
    }
    let text = |name: &str| {
        result
            .get(name)
            .and_then(|value| value.as_str())
            .map(str::to_owned)
    };
    Some(Begun {
        capture: text("capture_id")?,
        target: text("target")?,
        surface: text("surface")?,
        bound_seconds: result
            .get("bound_seconds")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
    })
}

/// Serves a capture's page, with a new one-time value.
fn capture_html(code: &str, page: &CapturePage, once: &str) -> Response {
    page_html(fill(
        CAPTURE_PAGE,
        &[
            ("capture", &escape(&page.capture)),
            ("target", &escape(&page.target)),
            ("surface", &escape(&page.surface)),
            ("bound", &page.bound_seconds.to_string()),
            ("code", &escape(code)),
            ("once", &escape(once)),
        ],
    ))
}

fn page_html(html: String) -> Response {
    page(StatusCode::OK, html)
}

/// Opens the page of a capture its person has just begun, for as long as
/// the capture may record and a margin.
fn capture_page_opened(inner: &Arc<Inner>, person: PrincipalId, begun: Begun) -> Response {
    let (Ok(code), Ok(once)) = (inner.code(), inner.code()) else {
        return answer(
            StatusCode::OK,
            "Recording",
            "The capture records. This machine gave no random value for its page, so it cannot be ended here: the program that asked ends it, or it ends at its bound.",
        );
    };
    let life = Duration::from_secs(begun.bound_seconds).saturating_add(CAPTURE_PAGE_MARGIN);
    let ending = Arc::downgrade(inner);
    let at = code.clone();
    let timer = tokio::spawn(async move {
        tokio::time::sleep(life).await;
        if let Some(inner) = ending.upgrade() {
            let mut state = inner.state();
            state.captures.remove(&at);
            inner.settle(&mut state);
        }
    });
    let page = CapturePage {
        capture: begun.capture,
        person,
        target: begun.target,
        surface: begun.surface,
        bound_seconds: begun.bound_seconds,
        once: Some(once.clone()),
        timer,
    };
    let shown = capture_html(&code, &page, &once);
    let mut state = inner.state();
    state.captures.insert(code, page);
    inner.settle(&mut state);
    shown
}

async fn capture_show(State(inner): State<Arc<Inner>>, Path(code): Path<String>) -> Response {
    let Ok(once) = inner.code() else {
        return answer(
            StatusCode::SERVICE_UNAVAILABLE,
            "The page cannot be shown",
            "This machine gave no random value for the page. Nothing was done. Open the link again.",
        );
    };
    let mut state = inner.state();
    let Some(found) = state.captures.get_mut(&code) else {
        return nothing_here();
    };
    found.once = Some(once.clone());
    capture_html(&code, found, &once)
}

/// A press of Stop recording: the page's one-time value, then the end of
/// the capture, once.
async fn capture_stop(
    State(inner): State<Arc<Inner>>,
    Path(code): Path<String>,
    body: Bytes,
) -> Response {
    let (capture, person) = {
        let mut state = inner.state();
        let Some(found) = state.captures.get_mut(&code) else {
            return nothing_here();
        };
        let expected = found.once.take();
        let fits = match (expected.as_deref(), once_of(&body)) {
            (Some(expected), Some(given)) => same(expected, given),
            _ => false,
        };
        if !fits {
            return refused(
                StatusCode::FORBIDDEN,
                &codes::REQUEST_NOT_FROM_PAGE,
                "Nothing was done",
                "The press did not come from the page as it was last shown. Open the page again and press there.",
            );
        }
        let (capture, person) = (found.capture.clone(), found.person);
        if let Some(gone) = state.captures.remove(&code) {
            gone.timer.abort();
        }
        inner.settle(&mut state);
        (capture, person)
    };
    let Some(desk) = inner.desk.get() else {
        return refused(
            StatusCode::SERVICE_UNAVAILABLE,
            &codes::SURFACE_UNAVAILABLE,
            "Nothing was done",
            "The engine cannot end the capture now. It ends at its bound.",
        );
    };
    match desk.end_capture(&capture, person) {
        Ok(()) => answer(
            StatusCode::OK,
            "Recording ended",
            "The capture ended, and what it found is stored as findings. The program that asked can call `capture.stop` to be told what it found. You can close this page.",
        ),
        Err(failure) => refused(
            StatusCode::CONFLICT,
            failure.code(),
            "Nothing was done",
            failure.message(),
        ),
    }
}

async fn act_confirm(
    State(inner): State<Arc<Inner>>,
    Path(code): Path<String>,
    body: Bytes,
) -> Response {
    let mut pressed = match act_begin(&inner, &code, &body) {
        Ok(pressed) => pressed,
        Err(response) => return *response,
    };
    drop(body);
    // While this press is carried out, a request of the session keeper for
    // this person's one-time code is answered on this press's page.
    let (tell, told) = oneshot::channel::<String>();
    let id = {
        let mut state = inner.state();
        let id = state
            .presses
            .iter()
            .map(|press| press.id)
            .max()
            .unwrap_or(0)
            + 1;
        state.presses.push(InFlight {
            id,
            person: pressed.person,
            signs_in: pressed.action == "session.login",
            tell: Some(tell),
        });
        id
    };
    // In a task of its own, as a press of Approve is.
    let task = Arc::clone(&inner);
    let mut carried = tokio::spawn(async move {
        let (settled, response) = act_confirm_pressed(&task, &mut pressed).await;
        task.state().presses.retain(|press| press.id != id);
        act_finish(&task, &code, settled);
        response
    });
    let asked = tokio::select! {
        done = &mut carried => return done.unwrap_or_else(|_| not_carried_out()),
        asked = told => asked.ok(),
    };
    let Some(page) = asked else {
        return carried.await.unwrap_or_else(|_| not_carried_out());
    };
    // The sign-in asks for a code: the press's answer is the page that
    // takes it, and the press goes on in its task until the code comes.
    let shown = {
        let mut state = inner.state();
        match state.codes.get_mut(&page) {
            Some(found) => {
                found.carried = Some(carried);
                found
                    .once
                    .clone()
                    .map(|once| (once, found.target.clone(), found.environment.clone()))
            }
            None => None,
        }
    };
    match shown {
        Some((once, target, environment)) => {
            page_of_code(&inner, &page, &once, &target, &environment)
        }
        None => answer(
            StatusCode::CONFLICT,
            "Nothing waits here",
            "The sign-in asked for a code, and its time ended before the page could be shown. Nothing was signed in.",
        ),
    }
}

/// The page that asks for a one-time code.
fn page_of_code(
    inner: &Inner,
    code: &str,
    once: &str,
    target: &str,
    environment: &str,
) -> Response {
    let minutes = inner.window.as_secs().div_ceil(60).max(1);
    page(
        StatusCode::OK,
        fill(
            CODE_PAGE,
            &[
                ("target", &escape(target)),
                ("environment", &escape(environment)),
                ("code", &escape(code)),
                ("once", &escape(once)),
                ("minutes", &minutes.to_string()),
            ],
        ),
    )
}

/// A person's one-time code, posted from its page: it goes to the sign-in
/// that asked for it, once, and the answer is what the act came to.
async fn code_given(
    State(inner): State<Arc<Inner>>,
    Path(code): Path<String>,
    body: Bytes,
) -> Response {
    let fields = form_of(&body, PRESS_LIMIT).unwrap_or_default();
    drop(body);
    let value = |name: &str| {
        fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.clone())
    };
    let (sent, carried) = {
        let mut state = inner.state();
        let Some(found) = state.codes.get_mut(&code) else {
            return nothing_here();
        };
        let expected = found.once.take();
        let fits = match (expected.as_deref(), value("once")) {
            (Some(expected), Some(given)) => same(expected, &given),
            _ => false,
        };
        // Taken out either way: a post that is not from the page ends the
        // sign-in, which then signs nothing in, and its press ends.
        let Some(found) = state.codes.remove(&code) else {
            return nothing_here();
        };
        if !fits {
            return answer(
                StatusCode::FORBIDDEN,
                "Nothing was signed in",
                "The code did not come from the page as it was shown, so the sign-in ended. Ask for it again.",
            );
        }
        let sent = match (found.answer, value("code")) {
            (Some(answer), Some(given)) if !given.is_empty() => {
                answer.send(a2m_core::Secret::new(given)).is_ok()
            }
            _ => false,
        };
        (sent, found.carried)
    };
    if !sent {
        return answer(
            StatusCode::CONFLICT,
            "Nothing was signed in",
            "No code was given, or the sign-in had ended. Ask for the sign-in again.",
        );
    }
    match carried {
        Some(carried) => carried.await.unwrap_or_else(|_| not_carried_out()),
        None => answer(
            StatusCode::OK,
            "The code was handed on",
            "The sign-in goes on. The program that asked can call it again to be told how it ended.",
        ),
    }
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
            .route("/code/{code}", post(code_given))
            .route("/capture/{code}", get(capture_show))
            .route("/capture/{code}/stop", post(capture_stop))
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
                return Err(codes::failure(
                    &codes::row::SURFACE_UNAVAILABLE,
                    "The engine cannot record an approval now, so no approval page is shown.",
                    "Run `anything2mcp doctor run` for the reason.",
                ));
            }
            let minutes = inner.window.as_secs().div_ceil(60).max(1);
            let code = inner.code()?;
            let mut state = inner.state();
            // A run is looked at after its answer too, before it is
            // resumed: a declined change gets no second page. An approved
            // one is asked about again only when its approval cleared
            // nothing, because it expired or was revoked (decision 0388):
            // until it is used, the kernel itself says the run is approved
            // and asks no surface. That ask gets a new page.
            let answered = state
                .answered
                .iter()
                .rev()
                .find(|(run, diff_hash, _)| *run == ask.run && *diff_hash == ask.diff_hash)
                .map(|(_, _, how)| *how);
            if answered == Some(Answered::Declined) {
                return Ok(SurfaceLink {
                    url: None,
                    text: "This change was declined, or nobody answered in its time. Nothing is written."
                        .to_owned(),
                });
            }
            if answered == Some(Answered::Approved) {
                state
                    .answered
                    .retain(|(run, diff_hash, _)| *run != ask.run || *diff_hash != ask.diff_hash);
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
                    effect: ask.effect,
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
                return Err(codes::failure(
                    &codes::row::SURFACE_UNAVAILABLE,
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
                    "`{}` needs its person to sign in. Ask for it with `session.login`: the person carries the sign-in out on its page on this machine. Then resume the run.",
                    ask.target
                ),
            })
        })
    }

    fn confirm_presence(&self, ask: PresenceAsk) -> BoxFuture<'_, Result<PresenceProof, Failure>> {
        Box::pin(async move {
            let refused = |message: &str, next: &str| {
                Err(codes::failure(&codes::row::PRESENCE_FAILED, message, next))
            };
            // One prompt at a time, as for an approval.
            let Ok(_up) = self.inner.prompting.try_lock() else {
                return Err(codes::failure(
                    &codes::row::PROMPT_BUSY,
                    "Another presence prompt is up on this machine.",
                    "Answer or close that prompt, then run the command again.",
                ));
            };
            // A prompt that no page asked for is not raised while a page
            // waits for its person: on a system whose dialog shows fixed
            // text, the person reading that page would take it for theirs.
            if self.waiting() > 0 {
                return Err(codes::failure(
                    &codes::row::PROMPT_BUSY,
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
                        purpose: PromptFor::Act,
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

    fn ask_session(&self, ask: SessionAsk) -> BoxFuture<'_, PersonAnswer> {
        Box::pin(async move {
            // A request reaches the person only through the page of their own
            // press of Carry out on a sign-in, while it is carried out
            // (decision 0205): the person is at the page then, and nobody
            // else is asked. No notification and no return address are built
            // on one machine yet.
            let Identity::Person(person) = &ask.identity else {
                return PersonAnswer::Unreachable;
            };
            if matches!(ask.ask, PersonAsk::Notify { .. }) {
                // The person who pressed is the person told: a sign-in in a
                // window is theirs to carry out, and the engine types nothing
                // in it. Nothing is asked of them, and no press is used up.
                let there = self
                    .inner
                    .state()
                    .presses
                    .iter()
                    .any(|press| press.signs_in && press.person == *person);
                return if there {
                    PersonAnswer::Done(a2m_auth::Returned::new())
                } else {
                    PersonAnswer::Unreachable
                };
            }
            // A one-time code is asked for on that page.
            let PersonAsk::EnterCode { .. } = &ask.ask else {
                return PersonAnswer::Unreachable;
            };
            let inner = &self.inner;
            let (Ok(code), Ok(once)) = (inner.code(), inner.code()) else {
                return PersonAnswer::Unreachable;
            };
            let (given, got) = oneshot::channel::<a2m_core::Secret>();
            let tell = {
                let mut state = inner.state();
                let Some(tell) = state
                    .presses
                    .iter_mut()
                    .find(|press| press.signs_in && press.person == *person)
                    .and_then(|press| press.tell.take())
                else {
                    return PersonAnswer::Unreachable;
                };
                state.codes.insert(
                    code.clone(),
                    CodePage {
                        target: ask.target.to_string(),
                        environment: ask.environment.to_string(),
                        once: Some(once),
                        answer: Some(given),
                        carried: None,
                    },
                );
                tell
            };
            if tell.send(code.clone()).is_err() {
                inner.state().codes.remove(&code);
                return PersonAnswer::Unreachable;
            }
            let answered = tokio::time::timeout(inner.window, got).await;
            // The page goes whatever came of it: a code is taken once.
            inner.state().codes.remove(&code);
            match answered {
                Ok(Ok(secret)) => {
                    PersonAnswer::Done(a2m_auth::Returned::new().with("code", secret))
                }
                _ => PersonAnswer::NoAnswer,
            }
        })
    }
}
