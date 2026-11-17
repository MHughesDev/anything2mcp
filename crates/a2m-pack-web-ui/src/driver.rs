//! The driver of the `ui` mode: the tabs of one run, and one step at a
//! time on them, as the plan wrote it. Every run has an observing tab, in
//! which the browser fails every request of the page that could change the
//! system, and every read runs there, in any posture; a run that may act
//! also opens an acting tab, in the same profile, for the steps of a write
//! up to its commit point, which the lowering marks (decisions 0183 and
//! 0195).
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
use a2m_core::{
    AccountName, Digest, EffectClass, Failure, FailureClass, FailureCode, Identity, Secret, Step,
    Value,
};
use a2m_pack_web_browser::{
    ActTab, Address, BrowserError, Browsers, Deadline, Dispatch, Element, Input, Loaded, Look,
    ObserveTab, Origins, PageEnvironment, Primitive, ProfileKey, Query, RawValue, ReadWhat, Reason,
    RequestShape, SafeRequests, Snapshot, SnapshotLimits, Visibility, path_by_structure,
    plain_segment,
};
use a2m_provider::{
    BoxFuture, Capture, Cleanup, Driver, DriverHealth, Observed, Posture, PrepareContext, Prepared,
    StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use a2m_telemetry::{Acting, Detail};

use crate::codes::{self, shown};
use crate::lower::{GOTO, INVOKE, REACH_ARG, READ, SET, VALUE_ARG, WAIT};
use crate::model::{
    Fingerprint, GotoStep, Handling, InvokeStep, Locator, PAYLOAD_FORMAT, ReadStep, Rows, Screen,
    ScreenSection, Segment, SetStep, SurfaceFields, TabKind, WaitStep,
};
use crate::screens::{PACK, SCREEN_KIND};

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

/// How long the page is given to say that a condition already holds, when
/// it is asked before a click. A condition that holds is answered at
/// once; only one that does not hold takes this long.
const LOOK_ONCE: Duration = Duration::from_millis(250);

/// The driver of the `ui` mode.
pub(crate) struct UiDriver {
    browsers: Browsers,
    snapshots: SnapshotLimits,
    /// Whether a browser program was found when the pack registered.
    /// `health` answers from this, without looking at the machine again.
    installed: bool,
}

impl UiDriver {
    pub(crate) fn new(browsers: Browsers, max_snapshot_elements: usize) -> Self {
        Self {
            installed: browsers.availability() == a2m_core::Availability::Ready,
            browsers,
            snapshots: SnapshotLimits {
                max_elements: max_snapshot_elements,
            },
        }
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
                FailureClass::Policy,
                a2m_provider::codes::READ_ONLY_POSTURE,
                "This run may only look at the page, and its tab cannot act.".to_owned(),
                "Run the operation where it may act.",
            ))),
        }
    }
}

/// What a session keeps to open its acting tab, once a step of a write
/// asks for it.
struct ToAct {
    browsers: Browsers,
    profile: ProfileKey,
    environment: PageEnvironment,
    origins: Origins,
}

/// What one run holds: its tabs, and the screens of its plan.
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
    /// For a plan whose `goto` steps do not say which tab: each `goto` of
    /// this pack's bindings, and whether it is a step of the work of an
    /// operation that writes.
    gotos: Vec<(Step, bool)>,
    /// `scheme://host`, with a port when the surface has one.
    origin: String,
    sections: BTreeMap<usize, ScreenSection>,
    snapshots: SnapshotLimits,
    /// What the run may do. Only a live run sets a field or clicks an
    /// action that does not save; a run against a declared test record
    /// sets fields and performs the one action marked as the commit point
    /// (decision 0147).
    posture: Posture,
}

/// The origin of a `web` surface: a scheme, a host, and perhaps a port.
pub(crate) fn origin_of(address: &str) -> Result<String, Failure> {
    let unusable = |why: &str| {
        codes::failure(
            FailureClass::Input,
            codes::ADDRESS_INVALID,
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

/// A sink that takes nothing: a browser's session is its own profile, and
/// this version has no way to place a credential into it.
#[derive(Default)]
struct NoPlace {
    placed: bool,
}

impl CredentialSink for NoPlace {
    fn header(&mut self, _name: &str, _value: &Secret) {
        self.placed = true;
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

pub(crate) fn fail(class: FailureClass, code: FailureCode, message: String, next: &str) -> Failure {
    codes::failure(class, code, message, next)
}

/// Whether the request an error belongs to may have reached the page.
fn sent(error: &BrowserError, failure: Failure) -> StepFailure {
    match error.dispatch() {
        Dispatch::NotSent => StepFailure::not_sent(failure),
        Dispatch::Sent | Dispatch::Unknown => StepFailure::unknown(failure),
    }
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
            FailureClass::Drift,
            codes::LOCATOR_NOT_FOUND,
            format!("No locator of {what} matches an element of the page."),
            "Look at the page and correct the screen's locators.",
        ),
        BrowserError::Ambiguous { count } => fail(
            FailureClass::Drift,
            codes::LOCATOR_AMBIGUOUS,
            format!("A locator of {what} matches {count} elements, and exactly one must match."),
            "Make the locator more exact; the first of several is never picked.",
        ),
        BrowserError::NotActionable(
            reason @ (Reason::WrongControl | Reason::NoSuchOption | Reason::NotEditable),
        ) => fail(
            FailureClass::UnknownVariant,
            codes::UNEXPECTED_CONTROL,
            format!(
                "The element found for {what} cannot be operated as its control says: {reason}."
            ),
            "Look at the page; correct the field's `control`, or declare the state the page is in.",
        ),
        BrowserError::NotActionable(reason) => fail(
            FailureClass::UnknownVariant,
            codes::UNEXPECTED_PAGE,
            format!("The element found for {what} cannot be used now: {reason}."),
            "Look at the page; declare the state it is in as a variant of the screen.",
        ),
        BrowserError::Timeout { .. } => fail(
            FailureClass::Transient,
            a2m_provider::codes::TIMEOUT,
            format!("The page did not answer in the time the step allows, at {what}."),
            again,
        ),
        BrowserError::Crashed { .. } | BrowserError::Disconnected { .. } => fail(
            FailureClass::Transient,
            a2m_provider::codes::CONNECTION_LOST,
            format!("The browser was lost at {what}."),
            again,
        ),
        BrowserError::NotLoaded(_) => fail(
            FailureClass::Transient,
            a2m_provider::codes::CONNECTION_LOST,
            format!("The page did not load, at {what}."),
            "Check that the application is reachable, then run the tool again.",
        ),
        BrowserError::RequestFailed(_) => fail(
            FailureClass::Transient,
            a2m_provider::codes::CONNECTION_LOST,
            format!("A request of the page did not complete, at {what}."),
            again,
        ),
        BrowserError::UnexpectedDialog { .. } => fail(
            FailureClass::UnknownVariant,
            codes::UNEXPECTED_DIALOG,
            format!("The page opened a dialog that no screen declares, at {what}."),
            "Look at the page; a dialog is declared before a run may answer it.",
        ),
        BrowserError::TooLarge { limit } => fail(
            FailureClass::UnknownVariant,
            codes::READ_TOO_LARGE,
            format!("What {what} shows is longer than the limit of {limit} characters."),
            "Raise `packs.web.max_read_chars`, or read a smaller part of the page.",
        ),
        // The base classes this one itself, above; kept here so that the
        // match names every error.
        BrowserError::ForeignOrigin(origin) => fail(
            FailureClass::Policy,
            codes::SURFACE_NOT_DECLARED,
            format!(
                "The page is on `{}`, which is not an origin this run may reach, at {what}. Nothing was read or sent there.",
                shown(origin)
            ),
            "Declare the surface in the target's files if the system really lives there; otherwise look at why the page moved.",
        ),
        BrowserError::Unexpected(_)
        | BrowserError::PasswordField
        | BrowserError::LaunchFailed(_) => fail(
            FailureClass::UnknownVariant,
            codes::UNEXPECTED_PAGE,
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
            FailureClass::Input,
            codes::ADDRESS_VALUE_REFUSED,
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
    loop {
        let line = lines.next()?;
        if cells(line) == rows.header {
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
            .ok_or_else(|| codes::not_usable("the screen it names is not in the plan"))
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
            FailureClass::Policy,
            a2m_provider::codes::READ_ONLY_POSTURE,
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
        let mut refused = self.looking.blocked();
        if let Some(acting) = &self.acting {
            refused.extend(acting.blocked());
        }
        refusal(&refused, what)
    }

    /// The tab the steps act in now: the acting tab only after a `goto`
    /// that chose it, and the observing tab in every other case.
    fn tab(&self) -> Tab<'_> {
        match (self.on, &self.acting) {
            (TabKind::Act, Some(acting)) => Tab::Act(acting),
            _ => Tab::Observe(&self.looking),
        }
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

/// Which tab a `goto` reaches its screen in. Its payload says; a plan
/// compiled before it did is answered from the plan's `gotos`: an acting
/// tab only when every binding that holds this very step is the work of an
/// operation that writes, and otherwise a stop by name, never an acting tab
/// for what may be a read.
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
            "its `goto` does not say which tab it reaches its screen in, and the plan does not show it to be a step of an operation that writes",
        ))
    }
}

/// What the browser engine reports a window it closed, and a download it
/// denied, under.
const LEFT_THE_PAGE: &[&str] = &["POPUP", "DOWNLOAD"];

/// What a tab refused since it was last asked, as a failure: a move of the
/// page, which the browser ended, is `policy`, `surface_not_declared`; a
/// window the page opened, which was closed, and a download, which was
/// denied, are a state no screen declares; a request of the page that could
/// change the system, which an observing tab failed, is `policy`,
/// `page_write_refused`: what was read is not read where the page may
/// write, and a read that needs such a request cannot be made (decision
/// 0195). `None` when it refused nothing.
pub(crate) fn refusal(refused: &[RequestShape], what: &str) -> Option<Failure> {
    let first = refused.first()?;
    let address = Address::new(&first.address).ok();
    let place = shown(&address.as_ref().map(Address::origin).unwrap_or_default());
    Some(if first.method == NAVIGATION {
        fail(
            FailureClass::Policy,
            codes::SURFACE_NOT_DECLARED,
            format!(
                "The page tried to move to `{place}`, at {what}. The browser stopped it there, and the run stops: nothing is set, clicked, or read on a page that would leave."
            ),
            "Look at why the page moves. Declare the surface in the target's files if the system really lives there.",
        )
    } else if LEFT_THE_PAGE.contains(&first.method.as_str()) {
        fail(
            FailureClass::UnknownVariant,
            codes::UNEXPECTED_PAGE,
            format!(
                "The page tried something no screen declares, at {what}: a new window or a download (`{}`). The browser refused it.",
                shown(&first.method)
            ),
            "Look at the page; a window or a download a step expects is declared on it, which this version cannot do yet.",
        )
    } else {
        // The path by structure: a segment that may be a token is not
        // shown.
        let path = address
            .as_ref()
            .and_then(|address| address.as_str().strip_prefix(&address.origin()))
            .map(path_by_structure)
            .unwrap_or_default();
        fail(
            FailureClass::Policy,
            codes::PAGE_WRITE_REFUSED,
            format!(
                "The page sent a request that could change the system (`{}` to `{}`), at {what}, in a tab that may only read. The browser failed it before it left, and the run stops: a read is never made where the page may write.",
                shown(&first.method),
                shown(&format!("{place}{path}")),
            ),
            "Look at what the page does when it loads. A read of a page that writes by itself cannot be made by the web pack; read the record another way.",
        )
    })
}

impl Session {
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

    /// Whether a condition holds on the page now: it is asked once, and
    /// given [`LOOK_ONCE`] to answer. `true` only on the page's own word
    /// that it holds; a page that has not said so within that time is
    /// taken as not showing it.
    async fn holds_now(
        &self,
        condition: &Primitive,
        deadline: Deadline,
    ) -> Result<bool, StepFailure> {
        let once = Deadline::after(LOOK_ONCE.min(deadline.remaining()));
        match self.tab().wait(condition, once).await {
            Ok(()) => Ok(true),
            Err(BrowserError::Timeout { .. }) => Ok(false),
            Err(error) => Err(browser_failure(&error, "the page's state")),
        }
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
            // The check below looks for the locator inside the section,
            // which drops a scope of its own. The compiler refuses the
            // pair; a plan that holds it all the same is not acted on.
            if fingerprint.is_some() && locator.within.is_some() {
                return Err(codes::not_usable(
                    "a locator with a scope of its own also has a fingerprint",
                ));
            }
            let element = match self.tab().locate(&locator.query(), deadline).await {
                Ok(element) => element,
                Err(BrowserError::NotFound) => continue,
                Err(error) => return Err(browser_failure(&error, what)),
            };
            if let Some(fingerprint) = fingerprint {
                let mismatch = |why: &str| {
                    StepFailure::not_sent(fail(
                        FailureClass::Drift,
                        codes::FINGERPRINT_MISMATCH,
                        format!("The element found for {what} fails its fingerprint: {why}."),
                        "Look at the page and correct the screen; the element may have moved, or another may have taken its name.",
                    ))
                };
                // The locator matched exactly one element of the page, so a
                // match inside the section is that element.
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
    async fn said_by(&self, screen: &Screen, variant: &str) -> Option<String> {
        let variant = screen.variants.get(variant)?;
        let deadline = Deadline::after(LOOK_ONCE);
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
        let loaded = self
            .tab()
            .goto(&address, until(step, input))
            .await
            .map_err(|error| browser_failure(&error, &what))?;
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
        // The screen is ready, or it is in a state it declares instead.
        let mut any = vec![screen.ready.primitive()];
        any.extend(
            screen
                .variants
                .values()
                .map(|variant| Primitive::Visible(variant.when.query())),
        );
        match self.tab().wait(&Primitive::Any(any), deadline).await {
            Ok(()) => {}
            Err(BrowserError::Timeout { .. }) => {
                return Err(StepFailure::not_sent(fail(
                    FailureClass::UnknownVariant,
                    codes::UNEXPECTED_PAGE,
                    format!(
                        "The page did not become {what} in the time the step allows: neither its `ready` condition nor one of its variants held."
                    ),
                    "Look at the page; correct the screen's `ready`, or declare the state the page is in as a variant.",
                )));
            }
            Err(error) => return Err(browser_failure(&error, &what)),
        }
        let variant = self
            .variant(screen, deadline)
            .await
            .map_err(|error| browser_failure(&error, &what))?;
        let mut means = vec![Detail::new("step", "wait"), Detail::new("screen", name)];
        match variant {
            Some((variant, Handling::Absent)) => {
                means.push(Detail::new("variant", variant));
                Ok(StepOutcome::Absent(Observed {
                    captures: vec![acting(means, Vec::new())],
                    ..Observed::default()
                }))
            }
            Some((variant, Handling::Stop)) => Err(StepFailure::answered(fail(
                FailureClass::Rejected,
                codes::APP_VALIDATION_ERROR,
                format!("The page shows the variant `{variant}` of {what}, which stops a run."),
                "Look at the page; the application refused.",
            ))),
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
            known_control(&field.control)?;
            let what = format!("the field `{}` of the screen `{name}`", item.from);
            let (element, locator) = self
                .find(&what, &field.locate, field.fingerprint.as_ref(), deadline)
                .await?;
            let value = match self.tab().read(&element, &field.read.what(), deadline).await {
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
            let text = match self.tab().read(&element, &ReadWhat::Text, deadline).await {
                Ok(RawValue::Text(text)) => text,
                Ok(_) => {
                    return Err(browser_failure(
                        &BrowserError::NotActionable(Reason::WrongControl),
                        &what,
                    ));
                }
                Err(error) => return Err(browser_failure(&error, &what)),
            };
            let table = parse_rows(&text, rows).ok_or_else(|| {
                StepFailure::answered(fail(
                    FailureClass::Drift,
                    codes::FINGERPRINT_MISMATCH,
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
            Rule::Select => {
                let code = widget_text(value).ok_or_else(wrong_value)?;
                self.tab()
                    .input(&element, &Input::SelectByValue(code), deadline, &what)
                    .await?;
                "select"
            }
            Rule::Text => {
                let text = widget_text(value).ok_or_else(wrong_value)?;
                self.set_text(&element, &text, &what, deadline).await?
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
            FailureClass::UnknownVariant,
            codes::UNEXPECTED_CONTROL,
            format!(
                "{what} does not hold the text it was given, by filling, by typing, or by its own setter."
            ),
            "Look at the page; the control changes what is typed into it, and its field needs another `control`.",
        )))
    }

    async fn invoke(&self, step: &Step, input: &StepInput<'_>) -> Result<StepOutcome, StepFailure> {
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
        let before = self
            .variants_now(screen, deadline)
            .await
            .map_err(|error| browser_failure(&error, &what))?;
        if let Some(shown_before) = before.iter().next() {
            let handling = screen
                .variants
                .get(shown_before)
                .map_or(Handling::Stop, |variant| variant.handling);
            let (class, code, next) = match handling {
                Handling::Stop => (
                    FailureClass::Rejected,
                    codes::APP_VALIDATION_ERROR,
                    "Correct the values and run the tool again; the page refused them before the action was performed.",
                ),
                Handling::Absent => (
                    FailureClass::UnknownVariant,
                    codes::UNEXPECTED_PAGE,
                    "Look at the page; the record is not where the action would act on it.",
                ),
            };
            let refused = StepFailure::not_sent(fail(
                class,
                code,
                format!(
                    "Before {what}, the page shows the variant `{shown_before}`, so its answer to the action could not be told from it. The action was not performed."
                ),
                next,
            ));
            return Err(match handling {
                Handling::Stop => match self.said_by(screen, shown_before).await {
                    Some(said) => refused.with_said(said),
                    None => refused,
                },
                Handling::Absent => refused,
            });
        }
        if self
            .holds_now(&action.confirm.primitive(), deadline)
            .await?
        {
            return Err(StepFailure::not_sent(fail(
                FailureClass::UnknownVariant,
                codes::UNEXPECTED_PAGE,
                format!(
                    "Before {what}, the page already shows the action's confirmation, so its answer to the action could not be told from it. The action was not performed."
                ),
                "Correct the action's `confirm` in the screen so that it names what the page shows only once the action is done.",
            )));
        }
        self.tab()
            .input(&element, &Input::Click, deadline, &what)
            .await?;

        // From here the click has reached the page. What the application
        // made of it is known only when the page shows the action's
        // confirmation, or a state the screen declares instead, neither of
        // which was there before the click.
        let no_confirmation = |why: String| {
            let (class, code) = if step.commits {
                (
                    FailureClass::PossibleCommit,
                    a2m_provider::codes::NO_CONFIRMATION,
                )
            } else {
                (FailureClass::UnknownVariant, codes::UNEXPECTED_PAGE)
            };
            StepFailure::unknown(fail(
                class,
                code,
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
                    "the page did not show its confirmation in the time the step allows".to_owned(),
                ));
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
                        FailureClass::Rejected,
                        codes::APP_VALIDATION_ERROR,
                        format!("After {what}, the page shows the variant `{variant}`."),
                        "Correct the values and run the tool again; the application refused them.",
                    ));
                    // The page's own words, apart from the product's.
                    match self.said_by(screen, variant).await {
                        Some(said) => refused.with_said(said),
                        None => refused,
                    }
                }
                // A record that is not where the save left it says nothing
                // of whether the save landed.
                (Handling::Absent, true) => no_confirmation(format!(
                    "then the page showed the variant `{variant}`, which does not say whether the save landed"
                )),
                (Handling::Absent, false) => StepFailure::answered(fail(
                    FailureClass::UnknownVariant,
                    codes::UNEXPECTED_PAGE,
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
}

/// The rule for a control's name. A name with no rule stops the step: the
/// plan was compiled by a pack that knew a control this driver does not.
fn known_control(control: &str) -> Result<Rule, StepFailure> {
    match control {
        "text" | "textarea" | "number" | "currency" | "measurement" => Ok(Rule::Text),
        "select" => Ok(Rule::Select),
        "checkbox" => Ok(Rule::Check),
        other => Err(StepFailure::not_sent(fail(
            FailureClass::UnknownVariant,
            codes::UNKNOWN_CONTROL,
            format!(
                "The plan names the control `{}`, and this driver has no rule for it.",
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
    pub profile: ProfileKey,
    pub environment: PageEnvironment,
    pub origins: Origins,
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
            FailureClass::Transient,
            a2m_provider::codes::TIMEOUT,
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
    if let Some(locale) = &cx.plan.target.locale {
        environment.locale.clone_from(locale);
    }
    environment.time_zone = time_zone(cx.plan.target.timezone.as_deref())?;

    // Whose browser: the session's, or the one of a surface that needs
    // no sign-in. A browser's session lives in its own profile; a
    // credential handed over to be placed cannot be, yet.
    let account = match lease {
        None => ANONYMOUS.to_owned(),
        Some(lease) => {
            let mut sink = NoPlace::default();
            lease.apply(&mut sink).map_err(unsent)?;
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
    let profile = ProfileKey {
        target: cx.surface.target.clone(),
        environment: cx.surface.environment.clone(),
        identity: Identity::Account(AccountName::new(&account).map_err(|_| {
            codes::not_usable("the session's profile has no name an account may have")
        })?),
    };
    // The tab may be on the surface's own origin and on no other. The
    // surface's address is one of the run's destinations, checked
    // above; another destination is another surface, whose screens
    // this run does not hold, and a page of it is not acted on.
    let own = Address::new(&surface.address).map_err(unsent)?;
    let origins = Origins::of([&own]);
    Ok(TabSetup {
        origin,
        profile,
        environment,
        origins,
    })
}

impl UiDriver {
    async fn prepared(
        &self,
        cx: &PrepareContext<'_>,
        lease: Option<&SessionLease>,
    ) -> Result<Session, StepFailure> {
        let TabSetup {
            origin,
            profile,
            environment,
            origins,
        } = tab_setup(cx, lease)?;
        let surface = &cx.surface.surface;
        let mut sections = BTreeMap::new();
        for (position, section) in cx.plan.packs.iter().enumerate() {
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
        // the system, and none is marked safe. A read never runs anywhere
        // else, in any posture (decision 0195).
        let looking = self
            .browsers
            .observer()
            .observing_tab(&profile, &environment, &SafeRequests::default(), &origins)
            .await
            .map_err(|error| browser_failure(&error, "the opening of a tab"))?;
        let may_act = matches!(
            cx.posture,
            Posture::Live | Posture::Sandbox { test_records: true }
        );
        // For a plan compiled before a `goto` said which tab it is in:
        // each `goto` of a binding over this pack's screens, and whether it
        // is the work of an operation that writes.
        let mut gotos = Vec::new();
        for operation in &cx.plan.operations {
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
        Ok(Session {
            looking,
            acting: None,
            on: TabKind::Observe,
            to_act: may_act.then(|| ToAct {
                browsers: self.browsers.clone(),
                profile,
                environment,
                origins,
            }),
            gotos,
            origin,
            sections,
            snapshots: self.snapshots.clone(),
            posture: cx.posture,
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
            } else {
                return Err(codes::not_usable(format!(
                    "the interface driver has no step of the kind `{}`",
                    step.kind
                )));
            };
            // And what it refused during this step undoes the step's
            // outcome: what was read may be of a page on its way out, and
            // a save it followed is in doubt.
            let what = format!("the end of a `{}` step", step.kind);
            let Some(failure) = session.refused(&what) else {
                return done;
            };
            Err(if step.commits {
                StepFailure::unknown(fail(
                    FailureClass::PossibleCommit,
                    a2m_provider::codes::NO_CONFIRMATION,
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

    fn health(&self, _surface: &SurfaceRef) -> DriverHealth {
        if self.installed {
            DriverHealth::Ready
        } else {
            DriverHealth::Unavailable(codes::BROWSER_UNAVAILABLE)
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
    fn a_goto_that_does_not_say_its_tab_acts_only_when_the_plan_shows_a_write() {
        let goto = |entry: &str| Step {
            kind: GOTO,
            refers_to: Some(a2m_core::PackRef {
                section: a2m_core::SectionIndex::new(0),
                entry: entry.to_owned(),
            }),
            args: Vec::new(),
            timeout_ms: 1000,
            fails_as: a2m_core::FailsAs {
                class: FailureClass::UnknownVariant,
                code: codes::UNEXPECTED_PAGE,
            },
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
        said.payload = a2m_core::PackPayload::encode(&GotoStep { tab: TabKind::Observe }).unwrap();
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
    }
}
