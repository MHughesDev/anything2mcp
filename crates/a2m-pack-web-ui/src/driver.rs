//! The driver of the `ui` mode: the tabs of one run, and one step at a time
//! on them, as the compiled target wrote it. Every run has an observing tab,
//! in which the browser fails every request of the page that could change the
//! system, and every read runs there, in any posture; a run that may act also
//! opens an acting tab, in the same profile, for the steps of a write up to
//! its commit point, which the lowering marks (decisions 0183 and 0195).
//!
//! What the driver never does: retry a step, pick the first of several
//! matches, read a password, send the tab outside the surface, sign in, or
//! renew a session. A page it has no rule for is a stop.
//!
//! The tab is opened for the origins of the run's destinations, and the
//! browser engine holds it there: it goes to no other, and looks at and
//! acts on no page of another (decision 0143). What the tab refused on the
//! way, a move of the page, a window, or a download, is heard at the start
//! and the end of every step and while a click's answer is awaited, and
//! stops the run (decision 0144).

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use a2m_auth::{ChallengeScheme, CredentialSink, SessionLease};
use a2m_core::{AccountName, Digest, EffectClass, Failure, Identity, Secret, Step, Value, Values};
use a2m_logs::{Acting, Detail};
use a2m_pack_web_browser::{
    ActTab, Address, BrowserError, BrowserProfileKey, BrowserState, Browsers, Deadline,
    DialogAnswer, Dispatch, Element, Input, Key, Loaded, Look, ObserveTab, Origins,
    PageEnvironment, Primitive, Query, RawValue, ReadWhat, Reason, RequestShape, SafeRequests,
    Snapshot, SnapshotLimits, Visibility, looks_like_an_action, path_by_structure, plain_segment,
};
use a2m_provider::{
    BoxFuture, Capture, Cleanup, Driver, DriverHealth, Observed, Posture, PrepareContext, Prepared,
    StepFailure, StepInput, StepOutcome, SurfaceRef,
};

use crate::codes::{self, shown};
use crate::lower::{CALL_TOOL, GOTO, INVOKE, REACH_ARG, READ, ROW_ARG, SET, VALUE_ARG, WAIT};
use crate::profiles::InputCommit;

mod site_tools;
mod widgets;
use crate::model::{
    Action, Cond, Field, Fingerprint, GotoStep, Handling, InvokeStep, Locator, PAYLOAD_FORMAT,
    ReadStep, Rows, Screen, ScreenSection, Segment, SetStep, SurfaceFields, TabKind, WaitStep,
};
use crate::requests::AssertedReads;
use crate::screens::{PACK, SCREEN_KIND};
use widgets::Chosen;

/// What the browser engine reports a move of the tab that it ended
/// under. A window it closed, and a download it denied, have words of
/// their own; anything the tab refused stops the run.
const NAVIGATION: &str = "NAVIGATION";

/// The account a run with no session browses as.
const ANONYMOUS: &str = "anonymous";

/// The profiles that belong to no person: the one of runs with no
/// session, and the one the web pack's discoverers look through. A lease
/// whose profile has one of these names is refused, so that a session's
/// browser is never one these share.
const RESERVED_PROFILES: &[&str] = &[ANONYMOUS, "discovery"];

/// How long a wait for what the page shows after a click runs before the
/// driver hears what the tab refused in the meantime. The wait goes on
/// until the step's own deadline; only the hearing is this often.
const HEAR_EVERY: Duration = Duration::from_millis(500);

/// The driver of the `ui` mode.
pub(crate) struct UiDriver {
    browsers: Browsers,
    snapshots: SnapshotLimits,
    /// Whether a browser program was found when the pack registered.
    /// `health` answers from this, without looking at the machine again.
    installed: bool,
    /// The surfaces the edge refused lately.
    held: Held,
}

impl UiDriver {
    pub(crate) fn new(browsers: Browsers, max_snapshot_elements: usize) -> Self {
        Self {
            installed: browsers.availability() == a2m_core::Availability::Ready,
            held: Held::default(),
            browsers,
            snapshots: SnapshotLimits {
                max_elements: max_snapshot_elements,
            },
        }
    }
}

/// How long a surface whose edge refused a visit is left alone: no run
/// asks it again in that time, so a block or a limit is never a retry storm.
const EDGE_HOLD: Duration = Duration::from_secs(60);

/// The surfaces the edge refused, by origin, with the refusal and when: the
/// pack's own memory, shared by every run of its driver, and gone when the
/// engine stops.
#[derive(Clone, Default)]
pub(crate) struct Held(
    std::sync::Arc<
        std::sync::Mutex<BTreeMap<String, (&'static a2m_core::CodeEntry, std::time::Instant)>>,
    >,
);

impl Held {
    /// The refusal that still holds a surface off, if there is one.
    fn of(&self, origin: &str) -> Option<&'static a2m_core::CodeEntry> {
        let mut held = self.0.lock().ok()?;
        match held.get(origin) {
            Some((row, at)) if at.elapsed() < EDGE_HOLD => Some(row),
            Some(_) => {
                held.remove(origin);
                None
            }
            None => None,
        }
    }

    fn hold(&self, origin: &str, row: &'static a2m_core::CodeEntry) {
        if let Ok(mut held) = self.0.lock() {
            held.insert(origin.to_owned(), (row, std::time::Instant::now()));
        }
    }
}

/// What the edge's answer to a page was, when it was a refusal.
pub(crate) fn edge_refusal(loaded: &Loaded) -> Option<&'static a2m_core::CodeEntry> {
    if loaded.challenged {
        return Some(&codes::row::CHALLENGE_PAGE);
    }
    match loaded.status {
        Some(429) => Some(&codes::row::RATE_LIMITED_BY_EDGE),
        Some(403) => Some(&codes::row::FORBIDDEN_BY_EDGE),
        Some(503) => Some(&codes::row::SURFACE_IN_MAINTENANCE),
        _ => None,
    }
}

/// The tab a step acts in: one that may act, or one that can only look.
#[derive(Clone, Copy)]
enum Tab<'a> {
    Act(&'a ActTab),
    Observe(&'a ObserveTab),
}

impl Tab<'_> {
    async fn goto(&self, address: &Address, deadline: Deadline) -> Result<Loaded, BrowserError> {
        match self {
            Self::Act(tab) => tab.goto(address, deadline).await,
            Self::Observe(tab) => tab.goto(address, deadline).await,
        }
    }

    async fn locate(&self, query: &Query, deadline: Deadline) -> Result<Element, BrowserError> {
        match self {
            Self::Act(tab) => tab.locate(query, deadline).await,
            Self::Observe(tab) => tab.locate(query, deadline).await,
        }
    }

    async fn read(
        &self,
        element: &Element,
        what: &ReadWhat,
        deadline: Deadline,
    ) -> Result<RawValue, BrowserError> {
        match self {
            Self::Act(tab) => tab.read(element, what, deadline).await,
            Self::Observe(tab) => tab.read(element, what, deadline).await,
        }
    }

    async fn wait(&self, until: &Primitive, deadline: Deadline) -> Result<(), BrowserError> {
        match self {
            Self::Act(tab) => tab.wait(until, deadline).await,
            Self::Observe(tab) => tab.wait(until, deadline).await,
        }
    }

    async fn holds(&self, condition: &Primitive, deadline: Deadline) -> Result<bool, BrowserError> {
        match self {
            Self::Act(tab) => tab.holds(condition, deadline).await,
            Self::Observe(tab) => tab.holds(condition, deadline).await,
        }
    }

    async fn snapshot(
        &self,
        limits: &SnapshotLimits,
        deadline: Deadline,
    ) -> Result<Snapshot, BrowserError> {
        match self {
            Self::Act(tab) => tab.snapshot(limits, deadline).await,
            Self::Observe(tab) => tab.snapshot(limits, deadline).await,
        }
    }

    /// One input to one element. An observing tab has no way to give one:
    /// the posture's guard refuses the step before it gets here, and this
    /// refuses it again.
    async fn input(
        &self,
        element: &Element,
        how: &Input,
        deadline: Deadline,
        what: &str,
    ) -> Result<(), StepFailure> {
        match self {
            Self::Act(tab) => tab
                .input(element, how, deadline)
                .await
                .map_err(|error| browser_failure(&error, what)),
            Self::Observe(_) => Err(StepFailure::not_sent(fail(
                &a2m_provider::codes::row::READ_ONLY_POSTURE,
                "This run may only look at the page, and its tab cannot act.".to_owned(),
                "Run the operation where it may act.",
            ))),
        }
    }
}

/// A field of a screen, the place of the locator that found it, and the
/// element found.
struct Found<'a> {
    field: &'a crate::model::Field,
    locator: usize,
    element: &'a Element,
}

/// How long a page whose platform posts a change back is given to draw
/// the form again, at most.
const REDRAWN_WITHIN: Duration = Duration::from_secs(10);

/// What a session keeps to open its acting tab, once a step of a write
/// asks for it.
struct ToAct {
    browsers: Browsers,
    profile: BrowserProfileKey,
    environment: PageEnvironment,
    origins: Origins,
}

/// What one run holds: its tabs, and the screens of its compiled target.
///
/// Every session has an observing tab, opened at prepare, in which the
/// browser fails every request of the page whose method could change the
/// system. An acting tab, in the same profile, is opened only when a
/// `goto` of an operation that writes asks for one, and only in a posture
/// that may act. Each `goto` says which of the two it reaches its screen
/// in, and the steps after it act there (decision 0195).
struct Session {
    looking: ObserveTab,
    acting: Option<ActTab>,
    /// The tab the last `goto` chose.
    on: TabKind,
    /// How to open the acting tab; `None` in a posture that may only look.
    to_act: Option<ToAct>,
    /// For a compiled target whose `goto` steps do not say which tab: each
    /// `goto` of this pack's bindings, and whether it is a step of the work
    /// of an operation that writes.
    gotos: Vec<(Step, bool)>,
    /// `scheme://host`, with a port when the surface has one.
    origin: String,
    sections: BTreeMap<usize, ScreenSection>,
    /// The screens the target declares as a sign that the session has
    /// ended, each with its `ready` (decision 0245).
    expiry: Vec<(String, Cond)>,
    snapshots: SnapshotLimits,
    /// The surfaces the edge refused lately, shared with the driver.
    held: Held,
    /// Where a declared download is written, or why that is not known.
    downloads: Result<a2m_provider::CheckedPath, Failure>,
    /// Where files for a file input are staged, or why that is not known.
    staging: Result<a2m_provider::CheckedPath, Failure>,
    /// What the run may do. Only a live run sets a field or clicks an
    /// action that does not save; a run against a declared test record
    /// sets fields and performs the one action marked as the commit point
    /// (decision 0147).
    posture: Posture,
    /// How a text control is committed, as the surface's platform profile
    /// states it; `None` sets a text control by the rule of no profile.
    commit: Option<InputCommit>,
    /// The surface's asserted reads, and whether the observing tab was
    /// handed them.
    asserted: AssertedReads,
    /// How to open the observing tab again when its browser is lost.
    reopen: Reopen,
    /// The steps that brought the observing tab's page where it is: the
    /// last `goto` in that tab, with its arguments, and each `wait` there
    /// since. Empty until such a `goto`, and after a `goto` in the acting
    /// tab.
    since_goto: Vec<(Step, Values)>,
    /// Whether the last step in the observing tab lost its page or its
    /// browser: the browser is asked for a word before the next step there
    /// uses it, since a browser can be dead before the tab sees it.
    unsure: bool,
    /// What the observing tab refused when its page made it, kept as a
    /// limitation of the run (decision 0477).
    limits: Limits,
}

/// What opens the observing tab again, in a new browser started from the
/// same profile, when the browser it was in is lost (decision 0424): the
/// browsers, and the tab's setup, with the session's state, which is placed
/// again into the new browser. Nothing here signs in: the state is the one
/// the run's lease handed over.
pub(crate) struct Reopen {
    browsers: Browsers,
    setup: TabSetup,
}

impl Reopen {
    pub(crate) fn new(browsers: Browsers, setup: TabSetup) -> Self {
        Self { browsers, setup }
    }

    /// Places the session's state into the profile's browser, which the
    /// browser engine starts afresh from the same data directory once the
    /// browser it had is lost, and opens a new observing tab there, with
    /// the same environment, origins, and asserted reads (decision 0424).
    pub(crate) async fn observing_tab(
        &self,
        safe: &SafeRequests,
        deadline: Deadline,
    ) -> Result<ObserveTab, StepFailure> {
        self.setup.place_state(&self.browsers, deadline).await?;
        self.browsers
            .observer()
            .observing_tab(
                &self.setup.profile,
                &self.setup.environment,
                safe,
                &self.setup.origins,
            )
            .await
            .map_err(|error| browser_failure(&error, "the opening of a tab in a new browser"))
    }
}

/// The origin of a `web` surface: a scheme, a host, and perhaps a port.
pub(crate) fn origin_of(address: &str) -> Result<String, Failure> {
    let unusable = |why: &str| {
        codes::failure(
            &a2m_pack_web_browser::codes::row::ADDRESS_INVALID,
            format!("The surface's address cannot be used by the web pack: {why}."),
            "Write the surface's `origin` as `https://host`, with a port if it has one, and nothing after it.",
        )
    };
    let trimmed = address.strip_suffix('/').unwrap_or(address);
    let rest = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .ok_or_else(|| unusable("it is not an http or https address"))?;
    let plain = !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'));
    if !plain {
        return Err(unusable(
            "after the scheme it holds more than a host and a port",
        ));
    }
    Address::new(trimmed)?;
    Ok(trimmed.to_owned())
}

/// The time zone of the target as a browser takes it.
pub(crate) fn time_zone(stated: Option<&str>) -> Result<String, StepFailure> {
    let Some(stated) = stated.filter(|zone| *zone != "UTC") else {
        return Ok("UTC".to_owned());
    };
    // A fixed offset of whole hours has a name in the zone database, with
    // the sign the other way round.
    let whole = stated
        .strip_suffix(":00")
        .and_then(|hours| {
            let (sign, digits) = hours.split_at_checked(1)?;
            let hours: u32 = digits.parse().ok()?;
            match sign {
                "+" => Some(('-', hours)),
                "-" => Some(('+', hours)),
                _ => None,
            }
        })
        .filter(|(_, hours)| *hours <= 14);
    match whole {
        Some((_, 0)) => Ok("UTC".to_owned()),
        Some((sign, hours)) => Ok(format!("Etc/GMT{sign}{hours}")),
        None => Err(codes::not_usable(
            "the target's time zone is not `UTC` or an offset of whole hours, and a browser cannot be given another fixed offset",
        )),
    }
}

/// A sink that takes one thing: the session's browser state, which the
/// web pack's own provider places (decision 0245). Anything else a
/// provider would place is a credential this pack does not put into a
/// browser.
#[derive(Default)]
struct StatePlace {
    state: Option<Secret>,
    placed: bool,
}

impl CredentialSink for StatePlace {
    fn header(&mut self, name: &str, value: &Secret) {
        if name == crate::sign_in::BROWSER_STATE && self.state.is_none() {
            self.state = Some(Secret::new(value.expose()));
        } else {
            self.placed = true;
        }
    }

    fn cookie(&mut self, _origin: &str, _name: &str, _value: &Secret) {
        self.placed = true;
    }

    fn tls_identity(&mut self, _certificate: &[u8], _key: &Secret) {
        self.placed = true;
    }

    fn challenge_credential(&mut self, _scheme: ChallengeScheme, _user: &str, _secret: &Secret) {
        self.placed = true;
    }
}

/// When a step must be done: the earlier of its own timeout and the
/// deadline it was handed.
pub(crate) fn until(step: &Step, input: &StepInput<'_>) -> Deadline {
    let own = Duration::from_millis(u64::from(step.timeout_ms));
    Deadline::after(own.min(input.deadline.remaining()))
}

pub(crate) fn fail(row: &a2m_core::CodeEntry, message: String, next: &str) -> Failure {
    codes::failure(row, message, next)
}

/// Whether the request an error belongs to may have reached the page.
fn sent(error: &BrowserError, failure: Failure) -> StepFailure {
    match error.dispatch() {
        Dispatch::NotSent => StepFailure::not_sent(failure),
        Dispatch::Sent | Dispatch::Unknown => StepFailure::unknown(failure),
    }
}

/// Whether a step's failure says it lost its page or its browser: the
/// browser it ran in may be dead before its tab has seen it.
pub(crate) fn lost_page(failed: &StepFailure) -> bool {
    *failed.failure.code() == a2m_provider::codes::CONNECTION_LOST
}

/// The failure of a step for an error of the base, where the step has no
/// meaning of its own for it. `what` names the thing looked for or acted
/// on, from the target's files.
pub(crate) fn browser_failure(error: &BrowserError, what: &str) -> StepFailure {
    if let Some(failure) = error.failure() {
        return sent(error, failure);
    }
    let again = "Run the tool again.";
    let failure = match error {
        BrowserError::NotFound => fail(
            &codes::row::LOCATOR_NOT_FOUND,
            format!("No locator of {what} matches an element of the page."),
            "Look at the page and correct the screen's locators.",
        ),
        BrowserError::Ambiguous { count } => fail(
            &codes::row::LOCATOR_AMBIGUOUS,
            format!("A locator of {what} matches {count} elements, and exactly one must match."),
            "Make the locator more exact; the first of several is never picked.",
        ),
        BrowserError::NotActionable(
            reason @ (Reason::WrongControl | Reason::NoSuchOption | Reason::NotEditable),
        ) => fail(
            &codes::row::UNEXPECTED_CONTROL,
            format!(
                "The element found for {what} cannot be operated as its control says: {reason}."
            ),
            "Look at the page; correct the field's `control`, or declare the state the page is in.",
        ),
        BrowserError::NotActionable(reason) => fail(
            &codes::row::UNEXPECTED_PAGE,
            format!("The element found for {what} cannot be used now: {reason}."),
            "Look at the page; declare the state it is in as a variant of the screen.",
        ),
        BrowserError::Timeout { .. } => fail(
            &a2m_provider::codes::row::TIMEOUT,
            format!("The page did not answer in the time the step allows, at {what}."),
            again,
        ),
        BrowserError::Crashed { .. } | BrowserError::Disconnected { .. } => fail(
            &a2m_provider::codes::row::CONNECTION_LOST,
            format!("The browser was lost at {what}."),
            again,
        ),
        BrowserError::NotLoaded(_) => fail(
            &a2m_provider::codes::row::CONNECTION_LOST,
            format!("The page did not load, at {what}."),
            "Check that the application is reachable, then run the tool again.",
        ),
        BrowserError::RequestFailed(_) => fail(
            &a2m_provider::codes::row::CONNECTION_LOST,
            format!("A request of the page did not complete, at {what}."),
            again,
        ),
        // The application answered, with a redirect a read follows nowhere:
        // a session that has expired is often answered so. It is not a lost
        // connection, and the read is not run again on its own.
        BrowserError::Redirected => fail(
            &codes::row::UNEXPECTED_STATUS,
            format!(
                "The answer to {what} was a redirect, which a read from inside the page does not follow."
            ),
            "Look at where the application sends the read; if the session has expired, sign in again before the tool is run.",
        ),
        BrowserError::UnexpectedDialog { .. } => fail(
            &codes::row::UNEXPECTED_DIALOG,
            format!("The page opened a dialog that no screen declares, at {what}."),
            "Look at the page; a dialog is declared before a run may answer it.",
        ),
        BrowserError::TooLarge { limit } => fail(
            &codes::row::READ_TOO_LARGE,
            format!("What {what} shows is longer than the limit of {limit} characters."),
            "Raise `packs.web.max_read_chars`, or read a smaller part of the page.",
        ),
        // The base classes this one itself, above; kept here so that the
        // match names every error.
        BrowserError::ForeignOrigin(origin) => fail(
            &codes::row::SURFACE_NOT_DECLARED,
            format!(
                "The page is on `{}`, which is not an origin this run may reach, at {what}. Nothing was read or sent there.",
                shown(origin)
            ),
            "Declare the surface in the target's files if the system really lives there; otherwise look at why the page moved.",
        ),
        // A write from inside the page, before anything was sent: the page
        // does not hold its token where the entry says (decision 0206).
        BrowserError::Token(a2m_pack_web_browser::TokenProblem::Missing) => fail(
            &codes::row::ANTI_FORGERY_NOT_FOUND,
            format!(
                "The page holds no anti-forgery token where the entry says, at {what}. Nothing was sent."
            ),
            "Look at where the page holds its token now, and correct the entry's `anti_forgery`.",
        ),
        BrowserError::Token(problem) => fail(
            &codes::row::ANTI_FORGERY_UNREADABLE,
            format!("At {what}, {problem}. Nothing was sent."),
            "Look at the page; a token is read from one place that holds one text.",
        ),
        BrowserError::Unexpected(_)
        | BrowserError::WriteRefused(_)
        | BrowserError::PasswordField
        | BrowserError::LaunchFailed(_) => fail(
            &codes::row::UNEXPECTED_PAGE,
            format!("The browser answered in a way no rule covers, at {what}."),
            "Report this to the web pack's author.",
        ),
    };
    match error {
        // The page was read, and what it holds is the answer.
        BrowserError::TooLarge { .. } | BrowserError::Unexpected(_) => {
            StepFailure::answered(failure)
        }
        // A page that did not load: whether the request left is not known.
        BrowserError::NotLoaded(_) => StepFailure::unknown(failure),
        _ => sent(error, failure),
    }
}

pub(crate) fn acting(means: Vec<Detail>, measured: Vec<Detail>) -> Capture {
    Capture::acting(&Acting { means, measured })
}

/// The value of one placeholder as one segment of an address. The form of
/// every placeholder is the same: one plain segment, of letters, digits,
/// `-`, `.`, `_`, and `~`, not made of dots only. A value of any other form
/// is refused before the tab is sent anywhere, never encoded: an input
/// cannot change where a request goes (decision 0187).
pub(crate) fn segment(name: &str, value: Option<&Value>) -> Result<String, StepFailure> {
    let refused = |why: &str| {
        StepFailure::not_sent(fail(
            &a2m_pack_web_browser::codes::row::ADDRESS_VALUE_REFUSED,
            format!("The value for `{name}` cannot stand in the screen's address: {why}."),
            "Give a value that is one segment of an address: letters, digits, `-`, `.`, `_`, and `~`.",
        ))
    };
    let text = match value {
        Some(Value::Text(text)) => text.clone(),
        Some(Value::Int(number)) => number.to_string(),
        Some(_) => return Err(refused("it is not text or a whole number")),
        None => return Err(refused("it was not given")),
    };
    if text.is_empty() || text.chars().all(|c| c == '.') {
        return Err(refused("it is empty, or made of dots only"));
    }
    if text.len() > a2m_pack_web_browser::MAX_SEGMENT_BYTES {
        return Err(refused("it is longer than 512 bytes"));
    }
    // One rule for every placeholder, in the browser engine: a `goto` and a
    // read from inside the page hold a value to the same form.
    if !plain_segment(&text) {
        return Err(refused(
            "it holds a character that is not a letter, a digit, `-`, `.`, `_`, or `~`",
        ));
    }
    // A key that is a word of an action could reach a sibling route that
    // acts on a visit, as `/orders/purge` beside `/orders/{order_id}`: the
    // drivers hold a key to the list discovery holds every address to
    // (decision 0195).
    if looks_like_an_action(&text) {
        return Err(refused(
            "it holds a word that names an action, and an address that may act on a visit is never asked for",
        ));
    }
    Ok(text)
}

/// Whether a made path is the screen's own pattern: one segment for each
/// of the pattern's, each literal as written, each placeholder one plain
/// segment.
pub(crate) fn within_pattern(path: &str, reach: &[Segment]) -> bool {
    let Some(rest) = path.strip_prefix('/') else {
        return false;
    };
    let made: Vec<&str> = if rest.is_empty() {
        Vec::new()
    } else {
        rest.split('/').collect()
    };
    made.len() == reach.len()
        && made.iter().zip(reach).all(|(made, part)| match part {
            Segment::Literal(text) => made == text,
            Segment::Hole(_) => plain_segment(made),
        })
}

/// A value as a text control takes it. Only the forms that need no field
/// type to write them.
fn widget_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Text(text) => Some(text.clone()),
        Value::Int(number) => Some(number.to_string()),
        Value::Decimal(number) => Some(number.to_string()),
        _ => None,
    }
}

/// The rows of a table, from the text the page shows for it: one line for
/// each row, one tab between two cells. The heading row must be the one
/// the screen declares; a line above it is the caption.
fn parse_rows(text: &str, rows: &Rows) -> Option<Vec<Vec<String>>> {
    let cells = |line: &str| -> Vec<String> {
        line.split('\t')
            .map(|cell| cell.trim().to_owned())
            .collect()
    };
    let mut lines = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line));
    // A heading is compared as a person reads it: a table library's sort
    // words are not its own (decision 0474, point 2).
    let heading = |line: &str| -> Option<Vec<String>> {
        cells(line)
            .iter()
            .map(|cell| crate::heading::words(cell))
            .collect()
    };
    loop {
        let line = lines.next()?;
        if heading(line).as_ref() == Some(&rows.header) {
            break;
        }
        if line.contains('\t') {
            return None;
        }
    }
    let mut out = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let row = cells(line);
        if row.len() != rows.header.len() {
            return None;
        }
        out.push(row);
    }
    Some(out)
}

impl Session {
    /// The screen a step names.
    fn screen<'a>(&'a self, step: &Step) -> Result<(&'a str, &'a Screen), StepFailure> {
        let named = step
            .refers_to
            .as_ref()
            .ok_or_else(|| codes::not_usable("it names no screen"))?;
        self.sections
            .get(&named.section.get())
            .and_then(|section| section.get_key_value(&named.entry))
            .map(|(name, screen)| (name.as_str(), screen))
            .ok_or_else(|| codes::not_usable("the screen it names is not in the compiled target"))
    }

    /// Whether the step may act on the page in this run's posture. A
    /// field is set in a live run and in a run against a declared test
    /// record: typing saves nothing until the committing action. A click
    /// is made in a live run, and, against a test record, only for the
    /// step marked as the commit point: an action that is not declared to
    /// save is one whose effect nobody stated.
    fn may_act(&self, step: &Step) -> Result<(), StepFailure> {
        let allowed = match self.posture {
            Posture::Live => true,
            Posture::Sandbox { test_records: true } => step.kind == SET || step.commits,
            Posture::Sandbox {
                test_records: false,
            }
            | Posture::Discovery => false,
        };
        if allowed {
            return Ok(());
        }
        Err(StepFailure::not_sent(fail(
            &a2m_provider::codes::row::READ_ONLY_POSTURE,
            format!(
                "A `{}` step acts on the page, and this run may not act so.",
                step.kind
            ),
            "Run the operation live, or against a declared test record, where only the action marked as the commit point is performed.",
        )))
    }

    /// What the tab refused since it was last asked, as a failure: a move
    /// of the page, which the browser ended, is `policy`,
    /// `surface_not_declared`; a window the page opened, which was closed,
    /// or a download, which was denied, is a state no screen declares.
    /// `None` when it refused nothing.
    fn refused(&self, what: &str) -> Option<Failure> {
        let mut refused = self.limits.keep(self.looking.blocked());
        if let Some(acting) = &self.acting {
            refused.extend(acting.blocked());
        }
        refusal(&refused, what, &self.asserted)
    }

    /// The tab the steps act in now: the acting tab only after a `goto`
    /// that chose it, and the observing tab in every other case.
    fn tab(&self) -> Tab<'_> {
        match (self.on, &self.acting) {
            (TabKind::Act, Some(acting)) => Tab::Act(acting),
            _ => Tab::Observe(&self.looking),
        }
    }

    /// Whether `step` runs in the observing tab: a `goto` that reaches its
    /// screen there, or any other step after one.
    fn in_observing_tab(&self, step: &Step) -> Result<bool, StepFailure> {
        if step.kind != GOTO {
            return Ok(self.on == TabKind::Observe);
        }
        Ok(self.to_act.is_none() || tab_for(step, &self.gotos)? != TabKind::Act)
    }

    /// Keeps what brought the observing tab's page where it is, after a
    /// step that was done: a `goto` there starts the list, a `goto` in the
    /// acting tab empties it, and a `wait` in the observing tab is added.
    fn remember(&mut self, step: &Step, input: &StepInput<'_>) {
        if step.kind == GOTO {
            self.since_goto.clear();
            if self.on == TabKind::Observe {
                self.since_goto.push((step.clone(), input.args.clone()));
            }
        } else if step.kind == WAIT && self.on == TabKind::Observe && !self.since_goto.is_empty() {
            self.since_goto.push((step.clone(), input.args.clone()));
        }
    }

    /// When the browser of the observing tab is lost and `step` runs in
    /// that tab: lost by what the tab has seen, or, after a step there that
    /// lost its page or its browser, by giving no answer when it is asked
    /// for one, since a browser can be dead before the tab sees it. Then it
    /// places the session's state into a new browser started from
    /// the same profile, opens the observing tab there, and, for a step that
    /// is not a `goto`, brings its page back where it was by the `goto` and
    /// the `wait` steps that brought it there, done again (decision 0424).
    /// The tab can only read, so doing them again changes nothing in the
    /// system. Nothing here signs in: the state placed is the one the run's
    /// lease handed over, and a page that shows a declared sign of expiry
    /// stops as it would have the first time. A step in the acting tab is
    /// never helped: that tab was in the lost browser too, and a write is
    /// never sent again.
    async fn recover(&mut self, step: &Step, input: &StepInput<'_>) -> Result<(), StepFailure> {
        let mut lost = self.looking.browser_lost();
        if !lost && self.unsure && self.in_observing_tab(step)? {
            self.unsure = false;
            lost = !self.looking.browser_answers(input.deadline).await;
        }
        if !lost || !self.in_observing_tab(step)? {
            return Ok(());
        }
        self.unsure = false;
        self.looking = self
            .reopen
            .observing_tab(&self.asserted.safe_requests(&self.origin), input.deadline)
            .await?;
        if step.kind == GOTO {
            return Ok(());
        }
        let steps = self.since_goto.clone();
        for (done, args) in &steps {
            let again = StepInput::new(args, input.deadline);
            let outcome = if done.kind == GOTO {
                self.goto(done, &again).await?
            } else {
                self.wait(done, &again).await?
            };
            if let StepOutcome::Absent(_) = outcome {
                return Err(StepFailure::not_sent(fail(
                    &codes::row::UNEXPECTED_PAGE,
                    format!(
                        "The browser was lost, and the page reached again in a new browser shows a state that says the record is absent, which it did not show before, at the `{}` step done again.",
                        done.kind
                    ),
                    "Run the tool again; the page changed while the browser was started again.",
                )));
            }
        }
        Ok(())
    }

    /// The tab for a `goto`, opened when it is the acting tab and not open
    /// yet. In a posture that may only look, every tab is the observing
    /// one, whatever the step says.
    async fn choose(&mut self, step: &Step) -> Result<(), StepFailure> {
        let wanted = tab_for(step, &self.gotos)?;
        let Some(to_act) = self.to_act.as_ref().filter(|_| wanted == TabKind::Act) else {
            self.on = TabKind::Observe;
            return Ok(());
        };
        if self.acting.is_none() {
            let browser = to_act
                .browsers
                .open(&to_act.profile, Visibility::Background)
                .await
                .map_err(|error| browser_failure(&error, "the start of the browser"))?;
            let tab = browser
                .acting_tab(&to_act.environment, &to_act.origins)
                .await
                .map_err(|error| browser_failure(&error, "the opening of a tab"))?;
            self.acting = Some(tab);
        }
        self.on = TabKind::Act;
        Ok(())
    }
}

/// Which tab a `goto` reaches its screen in. Its payload says; a compiled
/// target compiled before it did is answered from the compiled target's
/// `gotos`: an acting tab only when every binding that holds this very step
/// is the work of an operation that writes, and otherwise a stop by name,
/// never an acting tab for what may be a read.
fn tab_for(step: &Step, gotos: &[(Step, bool)]) -> Result<TabKind, StepFailure> {
    if !step.payload.is_none() {
        return step
            .payload
            .decode::<GotoStep>()
            .map(|goto| goto.tab)
            .map_err(|_| codes::not_usable("its payload does not decode"));
    }
    let mut holders = gotos.iter().filter(|(known, _)| known == step);
    let first = holders.next();
    if first.is_some_and(|(_, writes)| *writes) && holders.all(|(_, writes)| *writes) {
        Ok(TabKind::Act)
    } else {
        Err(codes::not_usable(
            "its `goto` does not say which tab it reaches its screen in, and the compiled target does not show it to be a step of an operation that writes",
        ))
    }
}

/// What the browser engine reports a window it closed, and a download it
/// denied, under.
const LEFT_THE_PAGE: &[&str] = &["POPUP", "DOWNLOAD"];

/// What the browser engine reports a socket, a shared worker, or a service
/// worker under, which the page's lock refused when the page made it, before
/// anything was sent; with the words a person reads for each.
const WHEN_MADE_WORDS: &[(&str, &str)] = &[
    ("WEBSOCKET", "a WebSocket"),
    ("WEBTRANSPORT", "a WebTransport session"),
    ("PEERCONNECTION", "a peer connection"),
    ("SHAREDWORKER", "a shared worker"),
    ("SERVICEWORKER", "a service worker"),
];

/// The words a person reads for what a page's lock refused when the page
/// made it; the base's word when it has none here.
fn made_words(refused: &RequestShape) -> &str {
    WHEN_MADE_WORDS
        .iter()
        .find(|(method, _)| *method == refused.method)
        .map_or(refused.method.as_str(), |(_, made)| made)
}

/// Where a refusal led, as a person may be shown it: the origin, and the
/// path by structure, so that a segment that may be a token is not shown. A
/// socket's address, `ws:` or `wss:`, is not a page address, so it is cut
/// at its first `/` after the host by hand; one with no `://` shows nothing.
fn refused_place(refused: &RequestShape) -> String {
    let (place, sent) = match Address::new(&refused.address) {
        Ok(address) => {
            let origin = address.origin();
            let sent = address
                .as_str()
                .strip_prefix(&origin)
                .unwrap_or_default()
                .to_owned();
            (origin, sent)
        }
        Err(_) => match refused.address.split_once("://") {
            Some((scheme, rest)) => {
                let (host, path) = rest.find('/').map_or((rest, ""), |at| rest.split_at(at));
                (format!("{scheme}://{host}"), path.to_owned())
            }
            None => (String::new(), String::new()),
        },
    };
    shown(&format!("{place}{}", path_by_structure(&sent)))
}

/// The stops that say a page did not show what its screen declares, which
/// may be because it went on without what the observing tab refused when
/// the page made it.
const NOT_SHOWN: [&a2m_core::CodeEntry; 3] = [
    &codes::row::UNEXPECTED_PAGE,
    &codes::row::LOCATOR_NOT_FOUND,
    &a2m_provider::codes::row::TIMEOUT,
];

/// What an observing tab's lock refused when the page made it, kept as a
/// limitation of the run rather than a stop (decision 0477): a socket, a
/// shared worker, or a service worker, through which nothing was sent. A
/// read goes on past it where the page goes on without it, and the run says
/// what was refused: each one, once, as a `refused` detail of what the step
/// measured, of the step during or before which the tab heard of it. What
/// the acting tab refused is never kept here: it stops the run.
#[derive(Debug, Default)]
pub(crate) struct Limits(std::sync::Mutex<Kept>);

#[derive(Debug, Default)]
struct Kept {
    /// Each refusal kept, once, oldest first.
    refused: Vec<RequestShape>,
    /// How many of them a step's outcome already says.
    told: usize,
}

impl Limits {
    fn kept(&self) -> std::sync::MutexGuard<'_, Kept> {
        // Only this type writes what it holds, and every write leaves it
        // whole, so a lock poisoned elsewhere still holds a true list.
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Keeps what the observing tab refused when its page made it, and
    /// hands back everything else it refused, in its order, for
    /// [`refusal`].
    pub(crate) fn keep(&self, refused: Vec<RequestShape>) -> Vec<RequestShape> {
        let (made, rest): (Vec<_>, Vec<_>) = refused
            .into_iter()
            .partition(RequestShape::refused_when_made);
        let mut kept = self.kept();
        for one in made {
            if !kept.refused.contains(&one) {
                kept.refused.push(one);
            }
        }
        rest
    }

    /// The first refusal kept, if any.
    pub(crate) fn first(&self) -> Option<RequestShape> {
        self.kept().refused.first().cloned()
    }

    /// Adds to the step's capture of how it was done each refusal kept that
    /// no step's outcome says yet, as a `refused` detail of what was
    /// measured: the base's word and where it led. Two runs of the same
    /// page may hear it at different steps, so it is not in the signature.
    pub(crate) fn tell(&self, observed: &mut Observed) {
        let mut kept = self.kept();
        if kept.told == kept.refused.len() {
            return;
        }
        let details: Vec<Detail> = kept.refused[kept.told..]
            .iter()
            .map(|one| Detail::new("refused", format!("{} {}", one.method, refused_place(one))))
            .collect();
        kept.told = kept.refused.len();
        let at = observed.captures.iter().position(Capture::is_acting);
        let mut acting_now = at
            .and_then(|at| serde_json::from_slice::<Acting>(&observed.captures[at].bytes).ok())
            .unwrap_or(Acting {
                means: Vec::new(),
                measured: Vec::new(),
            });
        acting_now.measured.extend(details);
        let capture = Capture::acting(&acting_now);
        match at {
            Some(at) => observed.captures[at] = capture,
            None => observed.captures.insert(0, capture),
        }
    }

    /// A step's outcome, given what was kept. In a driver whose read is
    /// what the page shows (`shown_by_page`), a page that shows the record
    /// absent after a refusal kept is a state no rule covers, and stops:
    /// the page may say so only because it went without what it made. And a
    /// step there that did not find what its screen declares, or waited for
    /// it in vain, says what was refused before it, since the page may need
    /// it to show its record. Every other outcome is the step's own, with
    /// what was kept told.
    pub(crate) fn after(
        &self,
        done: Result<StepOutcome, StepFailure>,
        what: &str,
        shown_by_page: bool,
    ) -> Result<StepOutcome, StepFailure> {
        let first = self.first();
        match (done, first) {
            (Ok(StepOutcome::Absent(_)), Some(first)) if shown_by_page => {
                Err(StepFailure::not_sent(fail(
                    &codes::row::UNEXPECTED_PAGE,
                    format!(
                        "The page shows that the record is absent, at {what}, after the tab, which may only read, refused {} (`{}`) when the page made it. Whether the record is absent is not known: the page may say so only because it went on without it.",
                        made_words(&first),
                        refused_place(&first)
                    ),
                    &format!(
                        "Look at what the page does when it loads. A page that needs {} to show its record cannot be read by the web pack; read the record from a page that does not.",
                        made_words(&first)
                    ),
                )))
            }
            (Err(failed), Some(first))
                if shown_by_page && failed.sent == a2m_provider::Sent::NotSent =>
            {
                // Only a stop that says the page did not show what its
                // screen declares is said again, from its own row.
                let Some(row) = NOT_SHOWN
                    .iter()
                    .find(|row| row.failure_code() == *failed.failure.code())
                else {
                    return Err(failed);
                };
                let made = made_words(&first);
                Err(StepFailure::not_sent(fail(
                    row,
                    format!(
                        "{} Before it, the tab, which may only read, refused {made} (`{}`) when the page made it, and the page may need it to show the screen.",
                        failed.failure.message(),
                        refused_place(&first)
                    ),
                    &format!(
                        "{} A page that needs {made} to show its record cannot be read by the web pack; read the record from a page that does not.",
                        failed.failure.next_step()
                    ),
                )))
            }
            (Ok(StepOutcome::Done(mut observed)), _) => {
                self.tell(&mut observed);
                Ok(StepOutcome::Done(observed))
            }
            (Ok(StepOutcome::Absent(mut observed)), _) => {
                self.tell(&mut observed);
                Ok(StepOutcome::Absent(observed))
            }
            (failed, _) => failed,
        }
    }
}

/// What a tab refused since it was last asked, as a failure: a move of the
/// page, which the browser ended, is `policy`, `surface_not_declared`; a
/// window the page opened, which was closed, and a download, which was
/// denied, are a state no screen declares; a request of the page that could
/// change the system, which an observing tab failed, is `policy`,
/// `page_write_refused`: what was read is not read where the page may
/// write, and a read that needs such a request cannot be made (decision
/// 0195). When an asserted read of the surface names the request's method
/// and path, the next step names that page request (decision 0394, point
/// 4). A socket, a shared worker, or a service worker the acting tab
/// refused when the page made it is `page_write_refused` too; one the
/// observing tab refused is kept by [`Limits::keep`] before this is asked,
/// and is no stop (decision 0477). `None` when it refused nothing.
pub(crate) fn refusal(
    refused: &[RequestShape],
    what: &str,
    asserted: &AssertedReads,
) -> Option<Failure> {
    let first = refused.first()?;
    let address = Address::new(&first.address).ok();
    let place = shown(&address.as_ref().map(Address::origin).unwrap_or_default());
    Some(if first.method == a2m_pack_web_browser::OUTSIDE {
        fail(
            &codes::row::ORIGIN_NOT_DECLARED,
            format!(
                "The page sent a request to `{place}`, at {what}, which is neither the surface's origin nor a destination the operation declares. The browser failed it before it left, and the run stops: what the page does without it is not known."
            ),
            "Declare the origin in the operation's `permissions.destinations`, with its scheme, if the system's pages really load from it; otherwise look at why the page sends there.",
        )
    } else if first.method == NAVIGATION {
        fail(
            &codes::row::SURFACE_NOT_DECLARED,
            format!(
                "The page tried to move to `{place}`, at {what}. The browser stopped it there, and the run stops: nothing is set, clicked, or read on a page that would leave."
            ),
            "Look at why the page moves. Declare the surface in the target's files if the system really lives there.",
        )
    } else if first.refused_when_made() {
        // Nothing was sent: the lock refused it when the page made it. It
        // is not a request an asserted read could name, so the next step
        // does not send the person to one (field test 11, finding 63). Only
        // the acting tab's refusals come here: a write is not made on a
        // page that goes on without what it made.
        let made = made_words(first);
        fail(
            &codes::row::PAGE_WRITE_REFUSED,
            format!(
                "The page opened {made} (`{}`), at {what}, in the tab a write is made in. The tab refused it when the page made it, so nothing was sent through it, and the run stops: a write is not made on a page that goes on without what it made, since what the page then does is not known.",
                refused_place(first),
            ),
            &format!(
                "Look at what the page does when it loads. A write through a page that opens {made} as it loads cannot be made by the web pack yet; a read of such a page goes on without it."
            ),
        )
    } else if LEFT_THE_PAGE.contains(&first.method.as_str()) {
        fail(
            &codes::row::UNEXPECTED_PAGE,
            format!(
                "The page tried something no screen declares, at {what}: a new window or a download (`{}`). The browser refused it.",
                shown(&first.method)
            ),
            "Look at the page; a window or a download a step expects is declared on it, which this version cannot do yet.",
        )
    } else {
        let sent = address
            .as_ref()
            .and_then(|address| address.as_str().strip_prefix(&address.origin()))
            .unwrap_or_default();
        // The path by structure: a segment that may be a token is not
        // shown.
        let path = path_by_structure(sent);
        let next = asserted.next_step(&first.method, sent).unwrap_or_else(|| {
            "Look at what the page does when it loads. A read of a page that writes by itself cannot be made by the web pack; if the request only reads, a person may say so with a page request marked `read_asserted: true`, and otherwise read the record another way.".to_owned()
        });
        fail(
            &codes::row::PAGE_WRITE_REFUSED,
            format!(
                "The page sent a request that could change the system (`{}` to `{}`), at {what}, in a tab that may only read. The browser failed it before it left, and the run stops: a read is never made where the page may write.",
                shown(&first.method),
                shown(&format!("{place}{path}")),
            ),
            &next,
        )
    })
}

impl Session {
    /// Answers a declared variant of a screen with the action it names:
    /// one click, not a commit point. Nothing is retried.
    async fn answer_variant(
        &self,
        screen_name: &str,
        screen: &Screen,
        variant: &str,
        deadline: Deadline,
    ) -> Result<(), StepFailure> {
        let with = screen
            .variants
            .get(variant)
            .and_then(|declared| declared.with.as_deref())
            .and_then(|action| screen.actions.get(action).map(|found| (action, found)));
        let Some((action_name, action)) = with else {
            return Err(codes::not_usable(
                "a variant it must answer names no action of its screen",
            ));
        };
        let what = format!(
            "the action `{action_name}` that answers the variant `{variant}` of the screen `{screen_name}`"
        );
        let (element, _) = self
            .find(&what, &action.locate, action.fingerprint.as_ref(), deadline)
            .await?;
        self.tab()
            .input(&element, &Input::Click, deadline, &what)
            .await?;
        Ok(())
    }

    /// The radio button of one choice of a group the field's locator found:
    /// the one of that label inside the group.
    async fn radio(
        &self,
        what: &str,
        field: &Field,
        position: usize,
        label: &str,
        deadline: Deadline,
    ) -> Result<Element, StepFailure> {
        let frame = field
            .fingerprint
            .as_ref()
            .and_then(|print| print.frame.as_ref());
        let group = field.locate[position].query_in(frame);
        self.tab()
            .locate(&Query::role("radio", label).within(group), deadline)
            .await
            .map_err(|error| browser_failure(&error, what))
    }

    /// Answers the dialog of the browser that the action declares, if the
    /// page has one open and it says what the action says it will. One that
    /// says anything else is not answered.
    async fn answer_declared_dialog(
        &self,
        action: &Action,
        deadline: Deadline,
        what: &str,
    ) -> Result<bool, StepFailure> {
        let (Tab::Act(tab), Some(declared)) = (self.tab(), action.dialog.as_ref()) else {
            return Ok(false);
        };
        if tab
            .open_dialog()
            .is_none_or(|open| open.message != declared.says)
        {
            return Ok(false);
        }
        let how = if declared.accept {
            DialogAnswer::Accept
        } else {
            DialogAnswer::Dismiss
        };
        tab.answer_dialog(&how, deadline)
            .await
            .map_err(|error| browser_failure(&error, what))?;
        Ok(true)
    }

    /// The variants of the screen the page shows now, by name.
    async fn variants_now(
        &self,
        screen: &Screen,
        deadline: Deadline,
    ) -> Result<BTreeSet<String>, BrowserError> {
        let mut shown = BTreeSet::new();
        for (name, variant) in &screen.variants {
            match self.tab().locate(&variant.when.query(), deadline).await {
                Ok(_) | Err(BrowserError::Ambiguous { .. }) => {
                    shown.insert(name.clone());
                }
                Err(BrowserError::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(shown)
    }

    /// Waits until the condition holds or the deadline passes, and hears
    /// what the tab refused every [`HEAR_EVERY`] meanwhile. `Ok(Some(_))`
    /// is a refusal heard before the condition held.
    async fn wait_hearing(
        &self,
        until: &Primitive,
        deadline: Deadline,
        what: &str,
    ) -> Result<Option<Failure>, BrowserError> {
        loop {
            let slice = Deadline::after(HEAR_EVERY.min(deadline.remaining()));
            match self.tab().wait(until, slice).await {
                Ok(()) => return Ok(None),
                Err(BrowserError::Timeout { .. }) if !deadline.passed() => {
                    if let Some(failure) = self.refused(what) {
                        return Ok(Some(failure));
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Whether a condition holds on the page now, on the page's own word:
    /// it is asked once and answers at once. A page that gives no answer
    /// by the step's deadline is a failure, never "no": a slow machine is
    /// not read as a page that does not show it.
    async fn holds_now(
        &self,
        condition: &Primitive,
        deadline: Deadline,
    ) -> Result<bool, StepFailure> {
        self.tab()
            .holds(condition, deadline)
            .await
            .map_err(|error| browser_failure(&error, "the page's state"))
    }

    /// The one element a list of locators finds, and which of them found
    /// it. A locator that matches several elements, and an element that
    /// fails its fingerprint, stop the step: a later locator is tried only
    /// when an earlier one matches nothing.
    async fn find(
        &self,
        what: &str,
        locators: &[Locator],
        fingerprint: Option<&Fingerprint>,
        deadline: Deadline,
    ) -> Result<(Element, usize), StepFailure> {
        for (position, locator) in locators.iter().enumerate() {
            // The check below looks for the locator inside the section, which
            // drops a scope of its own. The compiler refuses the pair; a
            // compiled target that holds it all the same is not acted on.
            if fingerprint.is_some() && locator.within.is_some() {
                return Err(codes::not_usable(
                    "a locator with a scope of its own also has a fingerprint",
                ));
            }
            let frame = fingerprint.and_then(|print| print.frame.as_ref());
            // An element the page gives no name, or a heading found by its
            // level, is looked for only inside its section, where it must
            // be the one; and an element that is its section is that one
            // element of the page (decision 0473, point 3).
            let own_section = fingerprint.is_some_and(|print| print.section == *locator);
            let nameless = fingerprint.is_some()
                && !own_section
                && match &locator.by {
                    crate::model::LocateBy::Role { name, .. } => name.is_empty(),
                    crate::model::LocateBy::Heading(_) => true,
                    _ => false,
                };
            let query = match fingerprint {
                Some(print) if nameless => locator.query_inside(&print.section),
                _ => locator.query_in(frame),
            };
            let element = match self.tab().locate(&query, deadline).await {
                Ok(element) => element,
                Err(BrowserError::NotFound) => continue,
                Err(error) => return Err(browser_failure(&error, what)),
            };
            if let Some(fingerprint) = fingerprint {
                let mismatch = |why: &str| {
                    StepFailure::not_sent(fail(
                        &codes::row::FINGERPRINT_MISMATCH,
                        format!("The element found for {what} fails its fingerprint: {why}."),
                        "Look at the page and correct the screen; the element may have moved, or another may have taken its name.",
                    ))
                };
                // The locator matched exactly one element of the page, so a
                // match inside the section is that element.
                if !nameless && !own_section {
                    match self
                        .tab()
                        .locate(&locator.query_inside(&fingerprint.section), deadline)
                        .await
                    {
                        Ok(_) => {}
                        Err(BrowserError::NotFound | BrowserError::Ambiguous { .. }) => {
                            return Err(mismatch("it is not inside its section"));
                        }
                        Err(error) => return Err(browser_failure(&error, what)),
                    }
                }
                if let Some(text) = &fingerprint.near_text {
                    let near = Locator {
                        by: crate::model::LocateBy::Text {
                            text: text.clone(),
                            contains: false,
                        },
                        within: None,
                    };
                    match self
                        .tab()
                        .locate(&near.query_inside(&fingerprint.section), deadline)
                        .await
                    {
                        Ok(_) | Err(BrowserError::Ambiguous { .. }) => {}
                        Err(BrowserError::NotFound) => {
                            return Err(mismatch("its section does not show the text it names"));
                        }
                        Err(error) => return Err(browser_failure(&error, what)),
                    }
                }
            }
            return Ok((element, position));
        }
        Err(browser_failure(&BrowserError::NotFound, what))
    }

    /// The variant of the screen the page shows now, if it shows one.
    async fn variant<'a>(
        &self,
        screen: &'a Screen,
        deadline: Deadline,
    ) -> Result<Option<(&'a str, Handling)>, BrowserError> {
        for (name, variant) in &screen.variants {
            match self.tab().locate(&variant.when.query(), deadline).await {
                Ok(_) | Err(BrowserError::Ambiguous { .. }) => {
                    return Ok(Some((name.as_str(), variant.handling)));
                }
                Err(BrowserError::NotFound) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    /// What the page itself says in the element of a declared variant: its
    /// validation message. The base redacts every text it reads, and the
    /// failure bounds it again. `None` when the variant's element is not
    /// one element, or has no text to read.
    async fn said_by(&self, screen: &Screen, variant: &str, deadline: Deadline) -> Option<String> {
        let variant = screen.variants.get(variant)?;
        let element = self
            .tab()
            .locate(&variant.when.query(), deadline)
            .await
            .ok()?;
        match self.tab().read(&element, &ReadWhat::Text, deadline).await {
            Ok(RawValue::Text(text)) => Some(text),
            _ => None,
        }
    }

    /// A snapshot of the page as evidence. It left the base already
    /// redacted, with every password masked.
    async fn snapshot(&self, deadline: Deadline) -> Option<Capture> {
        let snapshot = self.tab().snapshot(&self.snapshots, deadline).await.ok()?;
        Some(Capture {
            name: "snapshot".to_owned(),
            media_type: "text/plain".to_owned(),
            bytes: snapshot.to_text().into_bytes(),
        })
    }

    async fn goto(
        &mut self,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        self.choose(step).await?;
        let (name, screen) = self.screen(step)?;
        let mut path = String::new();
        for part in &screen.reach {
            path.push('/');
            match part {
                Segment::Literal(text) => path.push_str(text),
                Segment::Hole(hole) => {
                    let value = input.args.get(&format!("{REACH_ARG}{hole}"));
                    path.push_str(&segment(hole, value)?);
                }
            }
        }
        if path.is_empty() {
            path.push('/');
        }
        // The made address is held to the surface's origin and the
        // screen's own pattern before the tab hears of it.
        let made = format!("{}{path}", self.origin);
        if !within_pattern(&path, &screen.reach) || !made.starts_with(&format!("{}/", self.origin))
        {
            return Err(StepFailure::not_sent(codes::surface_not_declared(
                "The address made for the screen",
            )));
        }
        let address = Address::new(&made).map_err(StepFailure::not_sent)?;
        let what = format!("the screen `{name}`");
        // A surface the edge refused lately is not asked again.
        if let Some(row) = self.held.of(&self.origin) {
            return Err(StepFailure::not_sent(fail(
                row,
                format!(
                    "The edge of {} refused a visit a moment ago, and the surface is left alone for a while; nothing was sent.",
                    self.origin
                ),
                row.next,
            )));
        }
        let loaded = self
            .tab()
            .goto(&address, until(step, input))
            .await
            .map_err(|error| browser_failure(&error, &what))?;
        // An answer of the edge is a stop that keeps what it was: the
        // status, and no more. It is never retried and a challenge is never
        // answered (rule 5); the surface is held off for a while.
        if let Some(row) = edge_refusal(&loaded) {
            self.held.hold(&self.origin, row);
            return Err(StepFailure::answered(fail(
                row,
                format!(
                    "{what} was answered by the edge of the site with status {}, and not by the application.",
                    loaded
                        .status
                        .map_or_else(|| "none".to_owned(), |s| s.to_string())
                ),
                row.next,
            )));
        }
        // Where the page ended is checked: a move to another origin is not
        // followed by anything.
        let inside = loaded
            .address
            .strip_prefix(&self.origin)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '?', '#']));
        if !inside {
            return Err(StepFailure::answered(codes::surface_not_declared(
                "The address the page moved to",
            )));
        }
        let status = loaded
            .status
            .map_or_else(|| "none".to_owned(), |status| status.to_string());
        Ok(StepOutcome::Done(Observed {
            captures: vec![acting(
                vec![
                    Detail::new("step", "goto"),
                    Detail::new("screen", name),
                    Detail::new("path", screen.template()),
                    Detail::new("status", status),
                ],
                Vec::new(),
            )],
            ..Observed::default()
        }))
    }

    async fn wait(&self, step: &Step, input: &StepInput<'_>) -> Result<StepOutcome, StepFailure> {
        let (name, screen) = self.screen(step)?;
        let payload: WaitStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;
        let deadline = until(step, input);
        let what = format!("the screen `{name}`");
        // The screen is ready, or it is in a state it declares instead, or
        // the page is one the target declares as a sign that the session
        // has ended.
        let mut any = vec![screen.ready.primitive()];
        any.extend(
            screen
                .variants
                .values()
                .map(|variant| Primitive::Visible(variant.when.query())),
        );
        any.extend(self.expiry.iter().map(|(_, ready)| ready.primitive()));
        match self.tab().wait(&Primitive::Any(any), deadline).await {
            Ok(()) => {}
            Err(BrowserError::Timeout { .. }) => {
                return Err(StepFailure::not_sent(fail(
                    &codes::row::UNEXPECTED_PAGE,
                    format!(
                        "The page did not become {what} in the time the step allows: neither its `ready` condition nor one of its variants held."
                    ),
                    "Look at the page; correct the screen's `ready`, or declare the state the page is in as a variant.",
                )));
            }
            Err(error) => return Err(browser_failure(&error, &what)),
        }
        // A sign-in page in place of the screen: the session has ended. The
        // driver stops; the engine tells the keeper, which alone renews.
        if !self.expiry.is_empty() && !self.holds_now(&screen.ready.primitive(), deadline).await? {
            for (sign, ready) in &self.expiry {
                if sign != name && self.holds_now(&ready.primitive(), deadline).await? {
                    return Err(StepFailure::not_sent(crate::sign_in::session_ended(
                        format!(
                            "The page shows the screen `{}` in place of {what}",
                            shown(sign)
                        ),
                    )));
                }
            }
        }
        let mut variant = self
            .variant(screen, deadline)
            .await
            .map_err(|error| browser_failure(&error, &what))?;
        let mut means = vec![Detail::new("step", "wait"), Detail::new("screen", name)];
        // A notice over the screen that the screen declares as one to
        // dismiss is closed by the action it names, once; a notice that is
        // still there after that stops the run.
        if let Some((shown_name, Handling::Dismiss)) = variant {
            self.answer_variant(name, screen, shown_name, deadline)
                .await?;
            means.push(Detail::new("dismissed", shown_name));
            variant = self
                .variant(screen, deadline)
                .await
                .map_err(|error| browser_failure(&error, &what))?;
        }
        match variant {
            Some((variant, Handling::Absent)) => {
                means.push(Detail::new("variant", variant));
                Ok(StepOutcome::Absent(Observed {
                    captures: vec![acting(means, Vec::new())],
                    ..Observed::default()
                }))
            }
            Some((variant, Handling::Stop)) => Err(StepFailure::answered(fail(
                &codes::row::APP_VALIDATION_ERROR,
                format!("The page shows the variant `{variant}` of {what}, which stops a run."),
                "Look at the page; the application refused.",
            ))),
            Some((variant, Handling::Confirm | Handling::Dismiss)) => {
                Err(StepFailure::not_sent(fail(
                    &codes::row::UNEXPECTED_PAGE,
                    format!(
                        "The page shows the variant `{variant}` of {what}, which is not one that can be answered at this point, or which the answer did not close."
                    ),
                    "Look at the page; the screen declares a state it cannot leave by its own action.",
                )))
            }
            None => {
                let mut captures = Vec::new();
                let mut measured = Vec::new();
                if payload.snapshot {
                    match self.snapshot(deadline).await {
                        Some(snapshot) => captures.push(snapshot),
                        None => measured.push(Detail::new("snapshot", "not_taken")),
                    }
                }
                captures.insert(0, acting(means, measured));
                Ok(StepOutcome::Done(Observed {
                    captures,
                    ..Observed::default()
                }))
            }
        }
    }

    async fn read(&self, step: &Step, input: &StepInput<'_>) -> Result<StepOutcome, StepFailure> {
        let (name, screen) = self.screen(step)?;
        let payload: ReadStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;
        let deadline = until(step, input);
        let mut observed = Observed::default();
        let mut means = vec![Detail::new("step", "read"), Detail::new("screen", name)];
        for item in &payload.fields {
            let field = screen
                .fields
                .get(&item.from)
                .ok_or_else(|| codes::not_usable("a field it reads is not on its screen"))?;
            let rule = known_control(&field.control)?;
            let what = format!("the field `{}` of the screen `{name}`", item.from);
            let (element, locator) = self
                .find(&what, &field.locate, field.fingerprint.as_ref(), deadline)
                .await?;
            if rule == Rule::Radio {
                // The choice whose radio button is on, by the code it stores.
                let mut chosen = None;
                for (code, label) in &field.options {
                    let radio = self.radio(&what, field, locator, label, deadline).await?;
                    match self.tab().read(&radio, &ReadWhat::Checked, deadline).await {
                        Ok(RawValue::Flag(true)) => {
                            chosen = Some(code.clone());
                            break;
                        }
                        Ok(_) => {}
                        Err(error) => return Err(browser_failure(&error, &what)),
                    }
                }
                let Some(code) = chosen else {
                    return Err(StepFailure::answered(fail(
                        &codes::row::UNEXPECTED_CONTROL,
                        format!("No choice of {what} is on."),
                        "Look at the page; the group shows no choice, or one the field's `options` do not list.",
                    )));
                };
                means.push(Detail::new(
                    format!("locator.{}", item.from),
                    locator.to_string(),
                ));
                observed
                    .values
                    .insert(item.output.clone(), Value::Text(code));
                continue;
            }
            if !matches!(rule, Rule::Text | Rule::Select | Rule::Check) {
                let row = input.args.get(&format!("{ROW_ARG}{}", item.from));
                let value = self
                    .read_widget(
                        rule,
                        &Chosen {
                            field,
                            element: &element,
                            locator,
                            row,
                            what: &what,
                        },
                        deadline,
                    )
                    .await?;
                means.push(Detail::new(
                    format!("locator.{}", item.from),
                    locator.to_string(),
                ));
                observed.values.insert(item.output.clone(), value);
                continue;
            }
            let value = match self
                .tab()
                .read(&element, &field.read.what(), deadline)
                .await
            {
                Ok(RawValue::Text(text)) => Value::Text(text),
                Ok(RawValue::Flag(flag)) => Value::Bool(flag),
                Ok(RawValue::Absent) => {
                    return Err(browser_failure(
                        &BrowserError::NotActionable(Reason::WrongControl),
                        &what,
                    ));
                }
                // A password is never read; the base's own failure says so.
                Err(error) => return Err(browser_failure(&error, &what)),
            };
            means.push(Detail::new(
                format!("locator.{}", item.from),
                locator.to_string(),
            ));
            observed.values.insert(item.output.clone(), value);
        }
        if !payload.columns.is_empty() {
            let rows = screen
                .rows
                .as_ref()
                .ok_or_else(|| codes::not_usable("its screen has no table"))?;
            let what = format!("the table of the screen `{name}`");
            let (element, locator) = self
                .find(&what, &rows.locate, rows.fingerprint.as_ref(), deadline)
                .await?;
            let reading = match &rows.items {
                Some(role) => ReadWhat::Items { role: role.clone() },
                None => ReadWhat::Text,
            };
            let text = match self.tab().read(&element, &reading, deadline).await {
                Ok(RawValue::Text(text)) => text,
                Ok(_) => {
                    return Err(browser_failure(
                        &BrowserError::NotActionable(Reason::WrongControl),
                        &what,
                    ));
                }
                Err(error) => return Err(browser_failure(&error, &what)),
            };
            let table = if rows.items.is_some() {
                // Each item is one line, its text, and one row of one cell.
                Some(
                    text.split('\n')
                        .filter(|line| !line.trim().is_empty())
                        .map(|line| vec![line.trim().to_owned()])
                        .collect(),
                )
            } else {
                parse_rows(&text, rows)
            };
            let table = table.ok_or_else(|| {
                StepFailure::answered(fail(
                    &codes::row::FINGERPRINT_MISMATCH,
                    format!(
                        "The headings or the rows of {what} are not the ones the screen declares."
                    ),
                    "Look at the page and correct the `header` of the screen's `rows`.",
                ))
            })?;
            means.push(Detail::new("locator.rows", locator.to_string()));
            for item in &payload.columns {
                let position = rows
                    .columns
                    .get(&item.from)
                    .and_then(|position| usize::try_from(*position).ok())
                    .ok_or_else(|| codes::not_usable("a column it reads is not in its table"))?;
                let column: Vec<Value> = table
                    .iter()
                    .filter_map(|row| row.get(position))
                    .map(|cell| Value::Text(cell.clone()))
                    .collect();
                observed
                    .values
                    .insert(item.output.clone(), Value::List(column));
            }
        }
        observed.captures.push(acting(means, Vec::new()));
        Ok(StepOutcome::Done(observed))
    }

    async fn set(&self, step: &Step, input: &StepInput<'_>) -> Result<StepOutcome, StepFailure> {
        self.may_act(step)?;
        let (name, screen) = self.screen(step)?;
        let payload: SetStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;
        let field = screen
            .fields
            .get(&payload.field)
            .ok_or_else(|| codes::not_usable("the field it sets is not on its screen"))?;
        let rule = known_control(&field.control)?;
        let value = input.args.get(VALUE_ARG);
        let wrong_value = || {
            codes::not_usable(format!(
                "the value for the field `{}` is not one a `{}` control takes",
                payload.field,
                shown(&field.control)
            ))
        };
        let deadline = until(step, input);
        let what = format!("the field `{}` of the screen `{name}`", payload.field);
        let (element, locator) = self
            .find(&what, &field.locate, field.fingerprint.as_ref(), deadline)
            .await?;
        let strategy = match rule {
            Rule::Check => {
                let on = value.and_then(Value::as_bool).ok_or_else(wrong_value)?;
                self.tab()
                    .input(&element, &Input::Check(on), deadline, &what)
                    .await?;
                "check"
            }
            Rule::Radio => {
                let code = widget_text(value).ok_or_else(wrong_value)?;
                let label = field.options.get(&code).ok_or_else(wrong_value)?;
                let radio = self.radio(&what, field, locator, label, deadline).await?;
                self.tab()
                    .input(&radio, &Input::Check(true), deadline, &what)
                    .await?;
                "check"
            }
            Rule::Select => {
                let code = widget_text(value).ok_or_else(wrong_value)?;
                self.tab()
                    .input(&element, &Input::SelectByValue(code), deadline, &what)
                    .await?;
                "select"
            }
            Rule::Text => {
                let text = widget_text(value).ok_or_else(wrong_value)?;
                match self.commit {
                    Some(commit) => {
                        let found = Found {
                            field,
                            locator,
                            element: &element,
                        };
                        self.commit_text(commit, &found, &text, &what, deadline)
                            .await?
                    }
                    None => self.set_text(&element, &text, &what, deadline).await?,
                }
            }
            rule => {
                let text = widget_text(value).ok_or_else(wrong_value)?;
                let row = input.args.get(&format!("{ROW_ARG}{}", payload.field));
                self.set_widget(
                    rule,
                    &Chosen {
                        field,
                        element: &element,
                        locator,
                        row,
                        what: &what,
                    },
                    &text,
                    &wrong_value,
                    deadline,
                )
                .await?
            }
        };
        Ok(StepOutcome::Done(Observed {
            captures: vec![acting(
                vec![
                    Detail::new("step", "set"),
                    Detail::new("screen", name),
                    Detail::new("field", payload.field.as_str()),
                    Detail::new("locator", locator.to_string()),
                    Detail::new("strategy", strategy),
                ],
                Vec::new(),
            )],
            ..Observed::default()
        }))
    }

    /// The rule of a text control: fill; when the control does not hold
    /// the text, type it key by key; when it still does not, set it through
    /// the element's own setter. One strategy in one step, not a retry: the
    /// control is asked what it holds after each, and the first that holds
    /// ends it. A password input is filled once and never asked.
    async fn set_text(
        &self,
        element: &Element,
        text: &str,
        what: &str,
        deadline: Deadline,
    ) -> Result<&'static str, StepFailure> {
        let ways = [
            ("fill", Input::Fill(text.to_owned())),
            ("type", Input::Type(text.to_owned())),
            ("native_set", Input::NativeSet(text.to_owned())),
        ];
        for (name, way) in ways {
            self.tab().input(element, &way, deadline, what).await?;
            match self.tab().read(element, &ReadWhat::Value, deadline).await {
                Ok(RawValue::Text(held)) if held == text => return Ok(name),
                Ok(_) => {}
                Err(BrowserError::PasswordField) => return Ok(name),
                Err(error) => {
                    // Input reached the page before this read failed.
                    return Err(StepFailure::unknown(browser_failure(&error, what).failure));
                }
            }
        }
        Err(StepFailure::unknown(fail(
            &codes::row::UNEXPECTED_CONTROL,
            format!(
                "{what} does not hold the text it was given, by filling, by typing, or by its own setter."
            ),
            "Look at the page; the control changes what is typed into it, and its field needs another `control`.",
        )))
    }

    /// The rule of a text control on a surface whose platform profile states
    /// how a value is committed: the one way the profile names, and nothing
    /// else tried after it (decision 0385, the input commit record). The
    /// control is asked what it holds afterwards; for a commit that waits
    /// for the page to draw the form again, the control drawn anew is.
    async fn commit_text(
        &self,
        commit: InputCommit,
        found: &Found<'_>,
        text: &str,
        what: &str,
        deadline: Deadline,
    ) -> Result<&'static str, StepFailure> {
        let seen = match self.tab() {
            Tab::Act(tab) => tab.changes(),
            Tab::Observe(_) => 0,
        };
        let first = if commit == InputCommit::TypeThenBlur {
            Input::Type(text.to_owned())
        } else {
            Input::Fill(text.to_owned())
        };
        self.tab()
            .input(found.element, &first, deadline, what)
            .await?;
        let then = if commit == InputCommit::Enter {
            Input::Press(Key::Enter)
        } else {
            Input::Blur
        };
        // The text reached the page: whatever fails from here leaves its
        // fate unknown.
        self.tab()
            .input(found.element, &then, deadline, what)
            .await
            .map_err(|failure| StepFailure::unknown(failure.failure))?;
        let holder = if commit == InputCommit::BlurThenWaitReady {
            self.redrawn(found, seen, what, deadline).await?
        } else {
            found.element.clone()
        };
        match self.tab().read(&holder, &ReadWhat::Value, deadline).await {
            Ok(RawValue::Text(held)) if held == text => Ok(commit.name()),
            Err(BrowserError::PasswordField) => Ok(commit.name()),
            Ok(_) => Err(StepFailure::unknown(fail(
                &codes::row::UNEXPECTED_CONTROL,
                format!(
                    "{what} does not hold the text it was given, committed as the surface's platform profile says (`{}`).",
                    commit.name()
                ),
                "Look at the page; the surface may name the wrong platform profile, or the control changes what is put into it.",
            ))),
            Err(error) => Err(StepFailure::unknown(browser_failure(&error, what).failure)),
        }
    }

    /// The control the page drew anew after a commit that sends the change
    /// and draws the form again: the wait is for the old control to leave
    /// its document, or a new document to load, on the page's own word; a
    /// page that does not draw again in the time given is a stop with the
    /// fate of the value unknown (rule 7), never read as committed.
    async fn redrawn(
        &self,
        found: &Found<'_>,
        seen: u64,
        what: &str,
        deadline: Deadline,
    ) -> Result<Element, StepFailure> {
        let unknown =
            |error: &BrowserError| StepFailure::unknown(browser_failure(error, what).failure);
        let Tab::Act(tab) = self.tab() else {
            return Err(codes::not_usable(
                "a value is committed in a tab that cannot act",
            ));
        };
        let within = deadline.remaining().min(REDRAWN_WITHIN);
        let drew = tab
            .wait_redrawn(found.element, seen, within, deadline)
            .await
            .map_err(|error| unknown(&error))?;
        if !drew {
            return Err(StepFailure::unknown(fail(
                &codes::row::COMMIT_NOT_REDRAWN,
                format!(
                    "{what} was committed as the surface's platform profile says (`{}`), and the page did not draw the form again within {} seconds.",
                    InputCommit::BlurThenWaitReady.name(),
                    within.as_secs()
                ),
                "Look at the page: its change may not have reached the application. Find out what it holds before running the tool again.",
            )));
        }
        let field = found.field;
        let frame = field
            .fingerprint
            .as_ref()
            .and_then(|print| print.frame.as_ref());
        let query = field.locate[found.locator].query_in(frame);
        self.tab()
            .wait(&Primitive::Visible(query), deadline)
            .await
            .map_err(|error| unknown(&error))?;
        self.find(what, &field.locate, field.fingerprint.as_ref(), deadline)
            .await
            .map(|(element, _)| element)
            .map_err(|failure| StepFailure::unknown(failure.failure))
    }

    /// An action, and the download it declares, if it does: the refusal of
    /// downloads is lifted for this step alone, for one file of at most the
    /// size declared, into the folder of the profile; and put back, whatever
    /// the step came to (decision 0362, the download record).
    async fn invoke(&self, step: &Step, input: &StepInput<'_>) -> Result<StepOutcome, StepFailure> {
        let declared = {
            let (_, screen) = self.screen(step)?;
            let payload: InvokeStep = step
                .payload
                .decode()
                .map_err(|_| codes::not_usable("its payload does not decode"))?;
            screen
                .actions
                .get(&payload.action)
                .and_then(|action| action.download.clone())
        };
        let Some(download) = declared else {
            return self.invoke_action(step, input).await;
        };
        self.may_act(step)?;
        let acting_tab = self
            .acting
            .as_ref()
            .filter(|_| self.on == TabKind::Act)
            .ok_or_else(|| {
                codes::not_usable("a download is declared for an action of a tab that may act")
            })?;
        let deadline = until(step, input);
        let folder = self
            .downloads
            .as_ref()
            .map_err(|failure| StepFailure::not_sent(failure.clone()))?;
        let what = "the download of the action".to_owned();
        acting_tab
            .allow_download(folder.as_path(), download.most_bytes, deadline)
            .await
            .map_err(|error| browser_failure(&error, &what))?;
        let performed = self.invoke_action(step, input).await;
        let got = match &performed {
            Ok(_) => Some(acting_tab.downloaded(deadline).await),
            Err(_) => None,
        };
        // The refusal goes back before anything else is decided.
        let back = acting_tab
            .forbid_downloads(Deadline::after(Duration::from_secs(5)))
            .await;
        let mut outcome = performed?;
        back.map_err(|error| StepFailure::unknown(browser_failure(&error, &what).failure))?;
        let Some(got) = got else {
            return Err(codes::not_usable("the download was not waited for"));
        };
        let file = match got {
            Ok(file) => file,
            Err(error) => {
                let (row, why) = match error {
                    a2m_pack_web_browser::DownloadError::NoneBegan => (
                        &codes::row::DOWNLOAD_NOT_OFFERED,
                        "The page offered no file.".to_owned(),
                    ),
                    a2m_pack_web_browser::DownloadError::TooLarge { limit } => (
                        &codes::row::DOWNLOAD_TOO_LARGE,
                        format!("The file is larger than the {limit} bytes the action allows."),
                    ),
                    a2m_pack_web_browser::DownloadError::Cancelled => (
                        &codes::row::DOWNLOAD_INCOMPLETE,
                        "The download did not end whole.".to_owned(),
                    ),
                    a2m_pack_web_browser::DownloadError::Browser(error) => {
                        return Err(StepFailure::unknown(browser_failure(&error, &what).failure));
                    }
                };
                // The page answered the click; what it offered is not what
                // the action declares.
                return Err(StepFailure::answered(fail(row, why, row.next)));
            }
        };
        let StepOutcome::Done(observed) = &mut outcome else {
            return Ok(outcome);
        };
        let name = file
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(into) = &download.into {
            observed
                .values
                .insert(into.clone(), Value::Text(name.clone()));
        }
        if let Some(size_into) = &download.size_into {
            // The size is bounded by `most_bytes`, at most 64 MiB, so it
            // is always a whole number the value holds.
            let bytes = i64::try_from(file.bytes).map_err(|_| {
                StepFailure::unknown(
                    codes::not_usable("the file's size is too large to give").failure,
                )
            })?;
            observed.values.insert(size_into.clone(), Value::Int(bytes));
        }
        observed.captures.push(acting(
            vec![
                Detail::new("step", "download"),
                Detail::new("file", name),
                Detail::new("bytes", file.bytes.to_string()),
                Detail::new("suggested", shown(&file.suggested)),
            ],
            Vec::new(),
        ));
        Ok(outcome)
    }

    async fn invoke_action(
        &self,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<StepOutcome, StepFailure> {
        self.may_act(step)?;
        let (name, screen) = self.screen(step)?;
        let payload: InvokeStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;
        let action = screen
            .actions
            .get(&payload.action)
            .ok_or_else(|| codes::not_usable("the action it performs is not on its screen"))?;
        if action.commits != step.commits {
            return Err(codes::not_usable(
                "the step and its action disagree on whether the action saves",
            ));
        }
        let deadline = until(step, input);
        let what = format!("the action `{}` of the screen `{name}`", payload.action);
        let (element, locator) = self
            .find(&what, &action.locate, action.fingerprint.as_ref(), deadline)
            .await?;
        // What the page shows after the click is its answer only if it was
        // not there before. A page that already shows a state the screen
        // declares, or the action's confirmation, could not tell its answer
        // from what it showed: the action is not performed (decision 0144).
        let mut before = self
            .variants_now(screen, deadline)
            .await
            .map_err(|error| browser_failure(&error, &what))?;
        // A notice the screen declares as one to dismiss is in the way of
        // the click, and is closed first; the click has not been made.
        let dismissable: Vec<String> = before
            .iter()
            .filter(|shown| {
                screen
                    .variants
                    .get(*shown)
                    .is_some_and(|variant| variant.handling == Handling::Dismiss)
            })
            .cloned()
            .collect();
        if !dismissable.is_empty() {
            for shown in &dismissable {
                self.answer_variant(name, screen, shown, deadline).await?;
            }
            before = self
                .variants_now(screen, deadline)
                .await
                .map_err(|error| browser_failure(&error, &what))?;
        }
        if let Some(shown_before) = before.iter().next() {
            let handling = screen
                .variants
                .get(shown_before)
                .map_or(Handling::Stop, |variant| variant.handling);
            let (row, next) = match handling {
                Handling::Stop => (
                    &codes::row::APP_VALIDATION_ERROR,
                    "Correct the values and run the tool again; the page refused them before the action was performed.",
                ),
                Handling::Absent => (
                    &codes::row::UNEXPECTED_PAGE,
                    "Look at the page; the record is not where the action would act on it.",
                ),
                Handling::Confirm | Handling::Dismiss => (
                    &codes::row::UNEXPECTED_PAGE,
                    "Look at the page; a state the screen declares as one to answer was shown before the action, or was not closed by its answer.",
                ),
            };
            let refused = StepFailure::not_sent(fail(
                row,
                format!(
                    "Before {what}, the page shows the variant `{shown_before}`, so its answer to the action could not be told from it. The action was not performed."
                ),
                next,
            ));
            return Err(match handling {
                Handling::Stop => match self.said_by(screen, shown_before, deadline).await {
                    Some(said) => refused.with_said(said),
                    None => refused,
                },
                Handling::Absent | Handling::Confirm | Handling::Dismiss => refused,
            });
        }
        if self
            .holds_now(&action.confirm.primitive(), deadline)
            .await?
        {
            return Err(StepFailure::not_sent(fail(
                &codes::row::UNEXPECTED_PAGE,
                format!(
                    "Before {what}, the page already shows the action's confirmation, so its answer to the action could not be told from it. The action was not performed."
                ),
                "Correct the action's `confirm` in the screen so that it names what the page shows only once the action is done.",
            )));
        }
        // A click of an action that declares a dialog may be answered by
        // the dialog at once, the page being held in its handler.
        let mut dialog_answered = false;
        match self.tab() {
            Tab::Act(tab) => match tab.input(&element, &Input::Click, deadline).await {
                Ok(()) => {}
                Err(BrowserError::UnexpectedDialog { .. })
                    if self.answer_declared_dialog(action, deadline, &what).await? =>
                {
                    dialog_answered = true;
                }
                Err(error) => return Err(browser_failure(&error, &what)),
            },
            Tab::Observe(_) => {
                return Err(StepFailure::not_sent(fail(
                    &a2m_provider::codes::row::READ_ONLY_POSTURE,
                    "This run may only look at the page, and its tab cannot act.".to_owned(),
                    "Run the operation where it may act.",
                )));
            }
        }

        // From here the click has reached the page. What the application
        // made of it is known only when the page shows the action's
        // confirmation, or a state the screen declares instead, neither of
        // which was there before the click.
        let no_confirmation = |why: String| {
            let row = if step.commits {
                &a2m_provider::codes::row::NO_CONFIRMATION
            } else {
                &codes::row::UNEXPECTED_PAGE
            };
            StepFailure::unknown(fail(
                row,
                format!("{what} was performed, and {why}."),
                "Reconcile the record: read it back before anything is written again.",
            ))
        };
        // A page that tried to leave, or did something no screen declares,
        // after the click: a write in doubt for a click that saves, and a
        // stop for one that does not.
        let after_click = |failure: Failure| {
            if step.commits {
                no_confirmation(format!(
                    "then the page did something no rule covers: {}",
                    failure.message()
                ))
            } else {
                StepFailure::unknown(failure)
            }
        };
        let mut answered = false;
        let now = loop {
            let mut any = vec![action.confirm.primitive()];
            any.extend(
                screen
                    .variants
                    .values()
                    .map(|variant| Primitive::Visible(variant.when.query())),
            );
            match self
                .wait_hearing(&Primitive::Any(any), deadline, &what)
                .await
            {
                Ok(None) => {}
                Ok(Some(refused)) => return Err(after_click(refused)),
                Err(BrowserError::Timeout { .. }) => {
                    return Err(no_confirmation(
                        "the page did not show its confirmation in the time the step allows"
                            .to_owned(),
                    ));
                }
                Err(BrowserError::UnexpectedDialog { .. })
                    if !dialog_answered
                        && self.answer_declared_dialog(action, deadline, &what).await? =>
                {
                    dialog_answered = true;
                    continue;
                }
                Err(BrowserError::UnexpectedDialog { .. }) => {
                    return Err(StepFailure::unknown(
                        browser_failure(
                            &BrowserError::UnexpectedDialog {
                                dispatch: Dispatch::Sent,
                            },
                            &what,
                        )
                        .failure,
                    ));
                }
                Err(error @ BrowserError::ForeignOrigin(_)) => {
                    return Err(after_click(browser_failure(&error, &what).failure));
                }
                Err(_) => {
                    return Err(no_confirmation(
                        "the browser was lost before the page showed its confirmation".to_owned(),
                    ));
                }
            }
            let now = self.variants_now(screen, deadline).await.map_err(|_| {
                no_confirmation("the page could not be asked which state it is in".to_owned())
            })?;
            // A question the screen declares for this action is answered once,
            // by the action it names, and the page's answer is waited for
            // again. A second question stops the run below.
            let question = now
                .iter()
                .find(|shown| {
                    screen.variants.get(*shown).is_some_and(|variant| {
                        matches!(variant.handling, Handling::Confirm | Handling::Dismiss)
                    })
                })
                .cloned();
            if let (false, Some(question)) = (answered, question) {
                answered = true;
                if let Err(failure) = self.answer_variant(name, screen, &question, deadline).await {
                    return Err(if step.commits {
                        no_confirmation(format!(
                            "then the page asked `{question}`, which could not be answered: {}",
                            failure.failure.message()
                        ))
                    } else {
                        failure
                    });
                }
                continue;
            }
            break now;
        };
        if let Some(variant) = now.iter().next() {
            let handling = screen
                .variants
                .get(variant)
                .map_or(Handling::Stop, |variant| variant.handling);
            // A page that shows the confirmation and a declared state at
            // once has given no one answer.
            let confirmed_too = self
                .holds_now(&action.confirm.primitive(), deadline)
                .await
                .unwrap_or(true);
            return Err(match (handling, step.commits) {
                (_, true) if confirmed_too => no_confirmation(format!(
                    "then the page showed both its confirmation and the variant `{variant}`"
                )),
                // A refusal the page shows after the click, which was not
                // there before it, is the application's answer to it.
                (Handling::Stop, _) => {
                    let refused = StepFailure::answered(fail(
                        &codes::row::APP_VALIDATION_ERROR,
                        format!("After {what}, the page shows the variant `{variant}`."),
                        "Correct the values and run the tool again; the application refused them.",
                    ));
                    // The page's own words, apart from the product's.
                    match self.said_by(screen, variant, deadline).await {
                        Some(said) => refused.with_said(said),
                        None => refused,
                    }
                }
                // A record that is not where the save left it says nothing
                // of whether the save landed.
                (Handling::Absent, true) => no_confirmation(format!(
                    "then the page showed the variant `{variant}`, which does not say whether the save landed"
                )),
                (Handling::Confirm | Handling::Dismiss, true) => no_confirmation(format!(
                    "then the page showed the variant `{variant}` again after it was answered"
                )),
                (Handling::Confirm | Handling::Dismiss, false) => StepFailure::answered(fail(
                    &codes::row::UNEXPECTED_PAGE,
                    format!(
                        "After {what}, the page shows the variant `{variant}` again after it was answered."
                    ),
                    "Look at the page; the screen declares a question the answer does not close.",
                )),
                (Handling::Absent, false) => StepFailure::answered(fail(
                    &codes::row::UNEXPECTED_PAGE,
                    format!("After {what}, the page shows the variant `{variant}`."),
                    "Look at the page; the record is not where the action left it.",
                )),
            });
        }
        // No variant shows, so the confirmation held when the wait ended.
        // It is asked again: one that showed for a moment and is gone is
        // not an answer.
        match self
            .wait_hearing(&action.confirm.primitive(), deadline, &what)
            .await
        {
            Ok(None) => {}
            Ok(Some(refused)) => return Err(after_click(refused)),
            Err(error @ BrowserError::ForeignOrigin(_)) => {
                return Err(after_click(browser_failure(&error, &what).failure));
            }
            Err(_) => {
                return Err(no_confirmation(
                    "its confirmation did not stay on the page".to_owned(),
                ));
            }
        }
        let mut measured = Vec::new();
        let mut captures = Vec::new();
        match self.snapshot(deadline).await {
            Some(snapshot) => captures.push(snapshot),
            None => measured.push(Detail::new("snapshot", "not_taken")),
        }
        captures.insert(
            0,
            acting(
                vec![
                    Detail::new("step", "invoke"),
                    Detail::new("screen", name),
                    Detail::new("action", payload.action.as_str()),
                    Detail::new("locator", locator.to_string()),
                ],
                measured,
            ),
        );
        Ok(StepOutcome::Done(Observed {
            captures,
            ..Observed::default()
        }))
    }
}

/// How a control is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rule {
    Text,
    Select,
    Check,
    Radio,
    Combo,
    Slider,
    File,
    Grid,
    Tree,
    Tabs,
    Accordion,
}

/// The rule for a control's name. A name with no rule stops the step: the
/// compiled target was compiled by a pack that knew a control this driver
/// does not.
fn known_control(control: &str) -> Result<Rule, StepFailure> {
    match control {
        "text" | "textarea" | "number" | "currency" | "measurement" => Ok(Rule::Text),
        "select" => Ok(Rule::Select),
        "checkbox" | "toggle" => Ok(Rule::Check),
        "radio" => Ok(Rule::Radio),
        // A date, a date and time, and a time are typed, or set through the
        // field's own setter, in the form the browser's field takes:
        // `2026-12-01`, `2026-12-01T09:30`, `09:30`. A rich-text editor is
        // an editable element whose text is set as any text is.
        "date" | "datetime" | "time" | "rich_text" => Ok(Rule::Text),
        "combobox" | "autocomplete" => Ok(Rule::Combo),
        "slider" => Ok(Rule::Slider),
        "file" => Ok(Rule::File),
        "grid" => Ok(Rule::Grid),
        "tree" => Ok(Rule::Tree),
        "tabs" => Ok(Rule::Tabs),
        "accordion" => Ok(Rule::Accordion),
        other => Err(StepFailure::not_sent(fail(
            &codes::row::UNKNOWN_CONTROL,
            format!(
                "The compiled target names the control `{}`, and this driver has no rule for it.",
                shown(other)
            ),
            "Compile the target again with the version of the web pack that is enabled.",
        ))),
    }
}

/// What every tab of a `web` surface is opened with: the surface's origin,
/// whose profile, the target's fixed environment, and the one origin the
/// tab may be on. The `ui` and the `page_http` drivers open their tabs
/// alike.
pub(crate) struct TabSetup {
    /// `scheme://host`, with a port when the surface has one.
    pub origin: String,
    pub profile: BrowserProfileKey,
    pub environment: PageEnvironment,
    pub origins: Origins,
    /// The session's browser state, with the mark of its session and
    /// epoch, when its provider placed one.
    pub state: Option<(String, BrowserState)>,
    /// How a text control is committed, as the surface's platform profile
    /// states it; `None` for a surface that names no profile, or `auto`.
    pub commit: Option<InputCommit>,
}

impl TabSetup {
    /// Places the session's state into the profile's browser before any
    /// tab of the run is opened, once for each epoch of the session.
    pub(crate) async fn place_state(
        &self,
        browsers: &Browsers,
        deadline: Deadline,
    ) -> Result<(), StepFailure> {
        let Some((mark, state)) = &self.state else {
            return Ok(());
        };
        let browser = browsers
            .open(&self.profile, Visibility::Background)
            .await
            .map_err(|error| browser_failure(&error, "the start of the session's browser"))?;
        browser
            .import_state(mark, state, &self.origins, deadline)
            .await
            .map(|_| ())
            .map_err(|error| browser_failure(&error, "the placing of the session's state"))
    }
}

/// Every address of a surface of the compiled target, in every environment.
fn surface_addresses(compiled_target: &a2m_core::CompiledTarget) -> BTreeSet<&str> {
    compiled_target
        .surfaces
        .iter()
        .map(|surface| surface.address.as_str())
        .chain(
            compiled_target
                .environments
                .iter()
                .flat_map(|environment| environment.addresses.iter())
                .map(|address| address.address.as_str()),
        )
        .collect()
}

/// The origins a surface declares as refused without a stop, as addresses.
fn quiet_origins(fields: &SurfaceFields, own: &Address) -> Result<Vec<Address>, StepFailure> {
    let mut origins = Vec::new();
    for text in &fields.refuse_quietly {
        let address = Address::new(text).map_err(|_| {
            codes::not_usable(
                "an origin the surface refuses quietly is not an address with a scheme",
            )
        })?;
        if address.origin() != *text {
            return Err(codes::not_usable(
                "an origin the surface refuses quietly is written with more than an origin: a scheme, a host, and a port at most",
            ));
        }
        if address.origin() == own.origin() {
            return Err(codes::not_usable(
                "the surface refuses quietly its own origin",
            ));
        }
        origins.push(address);
    }
    Ok(origins)
}

/// The tab's setup, from the run's context and its session.
pub(crate) fn tab_setup(
    cx: &PrepareContext<'_>,
    lease: Option<&SessionLease>,
) -> Result<TabSetup, StepFailure> {
    let unsent = StepFailure::not_sent;
    let surface = &cx.surface.surface;
    let origin = origin_of(&surface.address).map_err(unsent)?;
    if !cx.destinations.contains(&surface.address) {
        return Err(unsent(codes::surface_not_declared(&format!(
            "The address of the surface `{}`",
            surface.name
        ))));
    }
    if cx.deadline.passed() {
        return Err(unsent(fail(
            &a2m_provider::codes::row::TIMEOUT,
            "The time for preparing a tab had passed before it began.".to_owned(),
            "Run the tool again.",
        )));
    }

    // The fixed environment: set once for the tab, from the target.
    let fields = if surface.payload.is_none() {
        SurfaceFields::default()
    } else {
        surface
            .payload
            .decode::<SurfaceFields>()
            .map_err(|_| codes::not_usable("the surface's fields do not decode"))?
    };
    let mut environment = PageEnvironment::default();
    if let Some(viewport) = fields.viewport {
        environment.viewport = (viewport.width, viewport.height);
    }
    if let Some(scheme) = &fields.color_scheme {
        environment.dark = scheme == "dark";
    }
    if let Some(reduced) = fields.reduced_motion {
        environment.reduced_motion = reduced;
    }
    if let Some(locale) = &cx.compiled_target.target.locale {
        environment.locale.clone_from(locale);
    }
    environment.time_zone = time_zone(cx.compiled_target.target.timezone.as_deref())?;

    // Whose browser: the session's, or the one of a surface that needs
    // no sign-in. A browser's session lives in its own profile; a
    // credential handed over to be placed cannot be, yet.
    let mut state = None;
    let account = match lease {
        None => ANONYMOUS.to_owned(),
        Some(lease) => {
            let mut sink = StatePlace::default();
            lease.apply(&mut sink).map_err(unsent)?;
            // The state is taken only from a provider of this pack: another
            // provider's header of the same name is not a browser's state
            // (audit finding A7).
            let ours = [
                crate::sign_in::FORM_LOGIN,
                crate::window::WINDOW_LOGIN,
                crate::window::SSO_OIDC,
                crate::window::SSO_SAML,
            ]
            .contains(&lease.grant().provider.as_str());
            if sink.state.is_some() && !ours {
                return Err(codes::not_usable(
                    "the session's provider is not one of the web pack's, and only those hand over a browser's state",
                ));
            }
            if let Some(placed) = &sink.state {
                let parsed = crate::sign_in::state_from(placed).ok_or_else(|| {
                    codes::not_usable(
                        "the session's browser state is not in the form this pack places",
                    )
                })?;
                state = Some((format!("{}:{}", lease.session(), lease.epoch()), parsed));
            }
            if sink.placed {
                return Err(codes::not_usable(
                    "the session's provider hands over a credential to place, and this version of the web pack uses only a session that lives in the browser's own profile",
                ));
            }
            match lease.profile() {
                // The names of the profiles a run with no session and
                // a discovery use are never a session's.
                Some(profile) if RESERVED_PROFILES.contains(&profile.as_str()) => {
                    return Err(codes::not_usable(
                        "the session's profile has the name of a profile a run with no session, or a discovery, uses",
                    ));
                }
                Some(profile) => profile.as_str().to_owned(),
                None => {
                    let digest = Digest::of(lease.session().to_string().as_bytes()).to_string();
                    let short: String = digest
                        .chars()
                        .filter(char::is_ascii_hexdigit)
                        .take(32)
                        .collect();
                    format!("s{short}")
                }
            }
        }
    };
    let profile = BrowserProfileKey {
        target: cx.surface.target.clone(),
        environment: cx.surface.environment.clone(),
        identity: Identity::Account(AccountName::new(&account).map_err(|_| {
            codes::not_usable("the session's profile has no name an account may have")
        })?),
    };
    // The tab may be on the surface's own origin and on no other. The
    // surface's address is one of the run's destinations, checked
    // above; another surface's address is not this run's to load from or
    // act on. Its page may send requests also to each destination the
    // operation declares that reads as an origin (decision 0292); one
    // written without a scheme names none, and gives the tab nothing.
    let own = Address::new(&surface.address).map_err(unsent)?;
    let declared: Vec<Address> = cx
        .destinations
        .iter()
        .filter(|address| !surface_addresses(cx.compiled_target).contains(*address))
        .filter(|address| address.starts_with("https://") || address.starts_with("http://"))
        .filter_map(|address| Address::new(address).ok())
        .collect();
    // The origins the surface declares as refused without a stop. Each is
    // one origin with its scheme, and none is the surface's own: anything
    // else is a stop by name, never a guess (rule 7).
    let quiet = quiet_origins(&fields, &own)?;
    let origins = Origins::of([&own])
        .and_requests_to(declared.iter())
        .and_refused_quietly(quiet.iter());
    // The input commit of the surface's platform. `auto` is settled by the
    // probe's detection (package E2), which is not built: until it is, a
    // surface of `auto` commits as one that names no profile. A name this
    // version does not hold is a stop, never a guess (rule 7).
    let commit = match fields.profile.as_deref() {
        None | Some("auto") => None,
        Some(name) => Some(
            crate::profiles::named(name)
                .ok_or_else(|| {
                    codes::not_usable(format!(
                        "the surface names the platform profile `{}`, which this version of the web pack does not hold",
                        shown(name)
                    ))
                })?
                .input_commit,
        ),
    };
    Ok(TabSetup {
        origin,
        profile,
        environment,
        origins,
        state,
        commit,
    })
}

impl UiDriver {
    async fn prepared(
        &self,
        cx: &PrepareContext<'_>,
        lease: Option<&SessionLease>,
    ) -> Result<Session, StepFailure> {
        let setup = tab_setup(cx, lease)?;
        setup.place_state(&self.browsers, cx.deadline).await?;
        let origin = setup.origin.clone();
        let commit = setup.commit;
        let surface = &cx.surface.surface;
        let mut sections = BTreeMap::new();
        for (position, section) in cx.compiled_target.packs.iter().enumerate() {
            let own = section.pack == PACK
                && section.kind == SCREEN_KIND
                && section.surface_kind == surface.kind;
            if !own {
                continue;
            }
            if section.format != PAYLOAD_FORMAT {
                return Err(codes::not_usable(format!(
                    "its screens are in format {}, and this pack reads format {PAYLOAD_FORMAT}",
                    section.format
                )));
            }
            sections.insert(
                position,
                section
                    .payload
                    .decode::<ScreenSection>()
                    .map_err(|_| codes::not_usable("its screens do not decode"))?,
            );
        }

        // Every session looks through an observing tab, in which the
        // browser fails every request of its page whose method could change
        // the system, but the asserted reads of a published version, live or
        // in an environment of kind `test` (decision 0394, point 4). A read
        // never runs anywhere else, in any posture (decision 0195).
        let asserted = AssertedReads::of(cx)?;
        let looking = self
            .browsers
            .observer()
            .observing_tab(
                &setup.profile,
                &setup.environment,
                &asserted.safe_requests(&origin),
                &setup.origins,
            )
            .await
            .map_err(|error| browser_failure(&error, "the opening of a tab"))?;
        let may_act = matches!(
            cx.posture,
            Posture::Live | Posture::Sandbox { test_records: true }
        );
        // For a compiled target compiled before a `goto` said which tab it is
        // in: each `goto` of a binding over this pack's screens, and whether
        // it is the work of an operation that writes.
        let mut gotos = Vec::new();
        for operation in &cx.compiled_target.operations {
            let writes = operation.effect != EffectClass::Read;
            for binding in &operation.bindings {
                let own = |step: &Step| {
                    step.kind == GOTO
                        && step
                            .refers_to
                            .as_ref()
                            .is_some_and(|named| sections.contains_key(&named.section.get()))
                };
                for step in binding.act.iter().filter(|step| own(step)) {
                    gotos.push((step.clone(), writes));
                }
                for step in binding
                    .confirm
                    .iter()
                    .chain(&binding.read_back)
                    .chain(&binding.lookup)
                    .filter(|step| own(step))
                {
                    gotos.push((step.clone(), false));
                }
            }
        }
        // The `ui` driver reads pages: a sign of expiry that is a status of
        // a page request's answer is `page_http`'s.
        let expiry =
            crate::sign_in::expiry_signs(cx.compiled_target, surface.name.as_str(), &sections)
                .map_err(codes::not_usable)?
                .screens;
        Ok(Session {
            expiry,
            looking,
            acting: None,
            on: TabKind::Observe,
            to_act: may_act.then(|| ToAct {
                browsers: self.browsers.clone(),
                profile: setup.profile.clone(),
                environment: setup.environment.clone(),
                origins: setup.origins.clone(),
            }),
            reopen: Reopen::new(self.browsers.clone(), setup),
            since_goto: Vec::new(),
            unsure: false,
            limits: Limits::default(),
            gotos,
            origin,
            sections,
            snapshots: self.snapshots.clone(),
            held: self.held.clone(),
            downloads: self.browsers.downloads_folder(),
            staging: self.browsers.staging_folder(),
            posture: cx.posture,
            commit,
            asserted,
        })
    }
}

impl Driver for UiDriver {
    fn prepare<'a>(
        &'a self,
        cx: &'a PrepareContext<'a>,
        lease: Option<&'a SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        Box::pin(async move { self.prepared(cx, lease).await.map(Prepared::new) })
    }

    fn act<'a>(
        &'a self,
        prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            let session = prepared
                .state::<Session>()
                .ok_or_else(|| codes::not_usable("it was prepared by another driver"))?;
            // What the tab refused since the last step stops this one
            // before it acts: the page tried to leave, or did something
            // no screen declares.
            let what = format!("the start of a `{}` step", step.kind);
            if let Some(failure) = session.refused(&what) {
                return Err(StepFailure::not_sent(failure));
            }
            // A read goes on in a new browser of the same profile when the
            // observing tab's browser was lost (decision 0424).
            if let Err(failed) = session.recover(step, input).await {
                session.unsure |= lost_page(&failed);
                return Err(failed);
            }
            let done = if step.kind == GOTO {
                session.goto(step, input).await
            } else if step.kind == WAIT {
                session.wait(step, input).await
            } else if step.kind == READ {
                session.read(step, input).await
            } else if step.kind == SET {
                session.set(step, input).await
            } else if step.kind == INVOKE {
                session.invoke(step, input).await
            } else if step.kind == CALL_TOOL {
                session.call_tool(step, input).await
            } else {
                return Err(codes::not_usable(format!(
                    "the interface driver has no step of the kind `{}`",
                    step.kind
                )));
            };
            if matches!(&done, Err(failed) if lost_page(failed))
                && session.in_observing_tab(step).unwrap_or(false)
            {
                session.unsure = true;
            }
            // And what it refused during this step undoes the step's
            // outcome: what was read may be of a page on its way out, and
            // a save it followed is in doubt.
            let what = format!("the end of a `{}` step", step.kind);
            let Some(failure) = session.refused(&what) else {
                // What the observing tab refused when its page made it is a
                // limitation: the step's outcome says it (decision 0477).
                let done = session.limits.after(done, &what, true);
                if let Ok(StepOutcome::Done(_)) = &done {
                    session.remember(step, input);
                }
                return done;
            };
            Err(if step.commits {
                StepFailure::unknown(fail(
                    &a2m_provider::codes::row::NO_CONFIRMATION,
                    format!(
                        "The action was performed, and then the page did something no rule covers: {}",
                        failure.message()
                    ),
                    "Reconcile the record: read it back before anything is written again.",
                ))
            } else if step.kind == SET || step.kind == INVOKE {
                StepFailure::unknown(failure)
            } else {
                StepFailure::not_sent(failure)
            })
        })
    }

    fn health(&self, surface: &SurfaceRef) -> DriverHealth {
        if !self.installed {
            return DriverHealth::Unavailable(codes::BROWSER_UNAVAILABLE);
        }
        // A surface whose edge refused a visit lately is not ready: it is
        // named by the refusal until the hold ends.
        match origin_of(&surface.surface.address)
            .ok()
            .and_then(|origin| self.held.of(&origin))
        {
            Some(row) => DriverHealth::Unavailable(row.failure_code()),
            None => DriverHealth::Ready,
        }
    }

    fn release<'a>(&'a self, prepared: Prepared, _poisoned: bool) -> BoxFuture<'a, Cleanup> {
        Box::pin(async move {
            // The tab is the run's own and closes when dropped; nothing of a
            // run is kept for the next, poisoned or not.
            drop(prepared);
            Cleanup::complete()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(method: &str, address: &str) -> RequestShape {
        RequestShape {
            method: method.to_owned(),
            address: address.to_owned(),
        }
    }

    fn measured(observed: &Observed) -> Vec<Detail> {
        observed
            .acting()
            .map(|acting| acting.measured)
            .unwrap_or_default()
    }

    fn done() -> Result<StepOutcome, StepFailure> {
        Ok(StepOutcome::Done(Observed {
            captures: vec![acting(vec![Detail::new("step", "wait")], Vec::new())],
            ..Observed::default()
        }))
    }

    /// What an observing tab refused when its page made it is kept, once,
    /// and told once, on the step's measured details and not in how it was
    /// done; everything else it refused is handed back for a stop, in its
    /// order (decision 0477).
    #[test]
    fn a_refusal_made_when_the_page_made_it_is_kept_and_told_once() {
        let limits = Limits::default();
        let worker = shape(
            "SHAREDWORKER",
            "http://127.0.0.1:9/escapes/shared-worker.js",
        );
        let rest = limits.keep(vec![
            worker.clone(),
            shape("POST", "http://127.0.0.1:9/traps/write"),
            worker.clone(),
            shape("WORKER", "http://127.0.0.1:9/w.js"),
            shape("REFUSED", "http://127.0.0.1:9/x"),
        ]);
        let methods: Vec<_> = rest.iter().map(|one| one.method.as_str()).collect();
        assert_eq!(methods, ["POST", "WORKER", "REFUSED"]);
        let Ok(StepOutcome::Done(first)) = limits.after(done(), "a step", true) else {
            panic!("a step that was done stays done");
        };
        let told = measured(&first);
        assert_eq!(told.len(), 1, "{told:?}");
        assert_eq!(told[0].name, "refused");
        assert_eq!(
            told[0].value,
            "SHAREDWORKER http://127.0.0.1:9/escapes/shared-worker.js"
        );
        assert_eq!(first.acting().unwrap().means.len(), 1);
        // The same refusal heard again is not told again.
        assert!(limits.keep(vec![worker]).is_empty());
        let Ok(StepOutcome::Done(second)) = limits.after(done(), "a step", true) else {
            panic!("a step that was done stays done");
        };
        assert!(measured(&second).is_empty());
    }

    /// After a refusal kept: a page that shows the record absent stops, in
    /// the `ui` driver; a step that did not find its screen says what was
    /// refused before it; any other failure is the step's own. In the
    /// `page_http` driver, whose read is its own request, the outcome is
    /// the step's own.
    #[test]
    fn after_a_refusal_kept_an_absent_record_stops_and_a_screen_not_found_names_it() {
        let limits = Limits::default();
        limits.keep(vec![shape("WEBSOCKET", "ws://127.0.0.1:9/events")]);
        let absent = || Ok(StepOutcome::Absent(Observed::default()));
        let stopped = limits.after(absent(), "a step", true).unwrap_err();
        assert_eq!(stopped.failure.code().as_str(), "unexpected_page");
        assert_eq!(stopped.sent, a2m_provider::Sent::NotSent);
        assert!(
            stopped.failure.message().contains("a WebSocket"),
            "{stopped:?}"
        );
        assert!(
            matches!(
                limits.after(absent(), "a step", false),
                Ok(StepOutcome::Absent(_))
            ),
            "a page request's answer is its own"
        );

        let not_shown = || {
            Err(StepFailure::not_sent(fail(
                &codes::row::UNEXPECTED_PAGE,
                "The page did not become the screen.".to_owned(),
                "Look at the page.",
            )))
        };
        let named = limits.after(not_shown(), "a step", true).unwrap_err();
        assert_eq!(named.failure.code().as_str(), "unexpected_page");
        assert!(
            named
                .failure
                .message()
                .starts_with("The page did not become the screen.")
        );
        assert!(
            named
                .failure
                .message()
                .contains("`ws://127.0.0.1:9/events`"),
            "{named:?}"
        );
        assert!(named.failure.next_step().starts_with("Look at the page."));
        let own = limits.after(not_shown(), "a step", false).unwrap_err();
        assert_eq!(own.failure.message(), "The page did not become the screen.");

        let ended = || {
            Err(StepFailure::not_sent(fail(
                &codes::row::APP_VALIDATION_ERROR,
                "The page refused.".to_owned(),
                "Correct the values.",
            )))
        };
        let kept_own = limits.after(ended(), "a step", true).unwrap_err();
        assert_eq!(kept_own.failure.message(), "The page refused.");

        // With nothing kept, every outcome is the step's own.
        let none = Limits::default();
        assert!(matches!(
            none.after(absent(), "a step", true),
            Ok(StepOutcome::Absent(_))
        ));
        let plain = none.after(not_shown(), "a step", true).unwrap_err();
        assert_eq!(
            plain.failure.message(),
            "The page did not become the screen."
        );
    }

    /// Every control the pack registers has a rule, and a control it did
    /// not register is a stop with its own code, never a guess at a text
    /// box (rule 7).
    #[test]
    fn every_registered_control_has_a_rule_and_any_other_is_a_stop() {
        for control in crate::screens::CONTROLS {
            assert!(known_control(control).is_ok(), "{control}");
        }
        let stopped = known_control("signature_pad").unwrap_err();
        assert_eq!(stopped.failure.code().as_str(), "unknown_control");
        assert_eq!(stopped.sent, a2m_provider::Sent::NotSent);
    }

    #[test]
    fn an_origin_is_a_scheme_a_host_and_a_port() {
        assert_eq!(
            origin_of("http://127.0.0.1:4010").unwrap(),
            "http://127.0.0.1:4010"
        );
        assert_eq!(
            origin_of("https://app.example.com/").unwrap(),
            "https://app.example.com"
        );
        for refused in [
            "app.example.com",
            "ftp://app.example.com",
            "https://app.example.com/orders",
            "https://user@app.example.com",
            "https://app.example.com?x=1",
            "https://",
        ] {
            let failure = origin_of(refused).unwrap_err();
            assert_eq!(failure.code(), &codes::ADDRESS_INVALID, "{refused}");
        }
    }

    #[test]
    fn a_value_fills_one_segment_and_cannot_leave_it() {
        let text = |text: &str| Value::Text(text.to_owned());
        assert_eq!(segment("id", Some(&text("A-100"))).unwrap(), "A-100");
        assert_eq!(segment("id", Some(&Value::Int(7))).unwrap(), "7");
        for refused in [
            Some(text("")),
            Some(text("..")),
            Some(text("...")),
            Some(text("a/b")),
            Some(text("a\\b")),
            Some(text("a?b=1")),
            Some(text("a#b")),
            Some(text("%2e%2e")),
            Some(text("a%2Fb")),
            Some(text("https:x")),
            Some(text("a b")),
            Some(text("a\u{0}b")),
            Some(text("a\nb")),
            Some(text("\u{e9}")),
            // A word of an action, which a sibling route could act on.
            Some(text("purge")),
            Some(text("export-cancel")),
            Some(Value::Bool(true)),
            None,
        ] {
            let failure = segment("id", refused.as_ref()).unwrap_err();
            assert_eq!(failure.failure.code(), &codes::ADDRESS_VALUE_REFUSED);
            assert_eq!(failure.sent, a2m_provider::Sent::NotSent);
        }
        let reach = [
            Segment::Literal("orders".to_owned()),
            Segment::Hole("order_id".to_owned()),
        ];
        assert!(within_pattern("/orders/A-100", &reach));
        for outside in [
            "/orders",
            "/orders/A-100/x",
            "/other/A-100",
            "/orders/..",
            "orders/A",
        ] {
            assert!(!within_pattern(outside, &reach), "{outside}");
        }
    }

    #[test]
    fn a_goto_that_does_not_say_its_tab_acts_only_when_the_compiled_target_shows_a_write() {
        let goto = |entry: &str| Step {
            kind: GOTO,
            refers_to: Some(a2m_core::PackRef {
                section: a2m_core::SectionIndex::new(0),
                entry: entry.to_owned(),
            }),
            args: Vec::new(),
            timeout_ms: 1000,
            fails_as: a2m_core::FailsAs::from(&codes::row::UNEXPECTED_PAGE),
            commits: false,
            lookup: false,
            idempotent: false,
            when: None,
            payload: a2m_core::PackPayload::none(),
        };
        let gotos = vec![
            (goto("form"), true),
            (goto("shared"), true),
            (goto("shared"), false),
            (goto("list"), false),
        ];
        assert!(matches!(tab_for(&goto("form"), &gotos), Ok(TabKind::Act)));
        for stops in ["shared", "list", "unknown"] {
            let stopped = tab_for(&goto(stops), &gotos).err().unwrap();
            assert_eq!(*stopped.failure.code(), codes::STEP_NOT_USABLE, "{stops}");
        }
        // A step that says is taken at its word.
        let mut said = goto("list");
        said.payload = a2m_core::PackPayload::encode(&GotoStep {
            tab: TabKind::Observe,
        })
        .unwrap();
        assert!(matches!(tab_for(&said, &gotos), Ok(TabKind::Observe)));
    }

    #[test]
    fn a_fixed_offset_becomes_a_zone_a_browser_knows() {
        assert_eq!(time_zone(None).unwrap(), "UTC");
        assert_eq!(time_zone(Some("UTC")).unwrap(), "UTC");
        assert_eq!(time_zone(Some("+02:00")).unwrap(), "Etc/GMT-2");
        assert_eq!(time_zone(Some("-05:00")).unwrap(), "Etc/GMT+5");
        assert_eq!(time_zone(Some("+00:00")).unwrap(), "UTC");
        assert!(time_zone(Some("+05:30")).is_err());
        assert!(time_zone(Some("Europe/Paris")).is_err());
    }

    #[test]
    fn a_table_is_read_by_its_declared_headings() {
        let rows = Rows {
            locate: Vec::new(),
            fingerprint: None,
            header: vec!["Order".to_owned(), "Customer".to_owned()],
            columns: BTreeMap::new(),
            items: None,
        };
        let cell = |text: &str| text.to_owned();
        assert_eq!(
            parse_rows(
                "All orders\nOrder\tCustomer\nA-100\tAcme\nA-101\t Beta \n",
                &rows
            ),
            Some(vec![
                vec![cell("A-100"), cell("Acme")],
                vec![cell("A-101"), cell("Beta")],
            ])
        );
        assert_eq!(parse_rows("Order\tCustomer\n", &rows), Some(Vec::new()));
        // Other headings, a row with another number of cells, and no
        // heading at all are not a table the screen declares.
        assert_eq!(parse_rows("Order\tClient\nA-100\tAcme\n", &rows), None);
        assert_eq!(parse_rows("Order\tCustomer\nA-100\n", &rows), None);
        assert_eq!(parse_rows("All orders\n", &rows), None);
        // A table library's sort words on a heading are not the heading's
        // (decision 0474, point 2); a heading with no words left is none.
        assert_eq!(
            parse_rows(
                "Order Order: Activate to sort\tCustomer: activate to sort column ascending\nA-100\tAcme\n",
                &rows
            ),
            Some(vec![vec![cell("A-100"), cell("Acme")]])
        );
        assert_eq!(
            parse_rows("Order\tActivate to sort\nA-100\tAcme\n", &rows),
            None
        );
    }
}
