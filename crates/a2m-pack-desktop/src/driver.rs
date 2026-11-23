//! The `desktop` driver: it finds the application the surface declares,
//! already running in the engine's own session, and carries out `goto`,
//! `wait`, `read`, `set`, `invoke`, and the discoverer's `snapshot` through
//! the back end.
//!
//! | Step | What it does |
//! | --- | --- |
//! | `goto` | Finds the one window of the application with the screen's role and title, waiting for the application to signal one until the step's deadline |
//! | `wait` | Reads the window's tree, and again each time the application signals a change, until the screen's `ready` condition holds or the deadline passes; then, for a record with a key, proves the window shows that record |
//! | `read` | Reads each field's value, or each column of the table as a list. A control that holds a secret is refused, never read |
//! | `set` | Linux only. Finds the field in a fresh walk, proves again the application's process, sets its whole text through its editable-text interface, and reads the field: a value the field does not hold as it was set stops the run before anything is saved |
//! | `invoke` | Linux only. Finds the action's element, proves the process again, refuses to act when the window already shows the action's confirmation or a refusal, does the element's named action, and waits, by the application's signals, for the confirmation, a refusal the screen declares, or the end of the application |
//! | `snapshot` | The shape of every window of the application: roles, the names that are the window's shape, the structure. Never a value |
//!
//! Nothing here starts a program, and nothing reaches another application:
//! the back end asks the system which process owns each application's
//! connection (the bus daemon on Linux) or window (Windows), or which
//! process runs the program (the process table on macOS), and asks an
//! application nothing until it is proven to be the program the surface
//! declares. A `set` or an `invoke` is done only in a posture that may act,
//! holds the driver's one desktop from its first `set` to the end of its
//! committing `invoke`, and sends no key and no pointer event. On Windows
//! and macOS a write stops before anything is sent, as `writes_not_built`
//! (decision 0218).

use std::collections::BTreeMap;
use std::sync::Arc;

use a2m_auth::SessionLease;
use a2m_core::{Step, Value, Values};
use a2m_provider::{
    BoxFuture, Cleanup, Deadline, Driver, DriverHealth, Observed, Posture, PrepareContext,
    Prepared, StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use serde_json::json;
#[cfg(target_os = "linux")]
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::backend::{App, AxError, Limits, Look};
use crate::codes::{self, shown};
use crate::lower::{
    COM_GET, GOTO, INTROSPECT, INVOKE, KEY_ARG, MANAGEMENT_GET, PROPERTY, READ, SCHEMA, SET,
    SNAPSHOT, WAIT,
};
use crate::model::{
    Cond, PAYLOAD_FORMAT, PropertyStep, ReadStep, Screen, ScreenSection, SurfaceFields,
};
use crate::screens::{PACK, SCREEN_KIND};
use crate::tree::{El, Miss, Node, Tree};

/// The format of the shapes a `snapshot` step gives.
pub(crate) const SNAPSHOT_FORMAT: &str = "1";

/// The `desktop` driver.
pub(crate) struct DesktopDriver {
    /// The session bus, when not the one the engine's environment names.
    session: Option<Arc<str>>,
    limits: Limits,
    /// The one desktop this driver changes: held by one write at a time,
    /// from its first `set` to the end of its committing `invoke`, so that
    /// two runs never type into one desktop at once. The scheduler's
    /// exclusive key (`exclusive = "screen"`) serializes writes on one
    /// surface first; this holds where two surfaces share one desktop.
    #[cfg(target_os = "linux")]
    desk: Arc<Mutex<()>>,
}

impl DesktopDriver {
    pub(crate) fn new(session: Option<Arc<str>>, limits: Limits) -> Self {
        Self {
            session,
            limits,
            #[cfg(target_os = "linux")]
            desk: Arc::new(Mutex::new(())),
        }
    }
}

/// What `prepare` acquired.
struct Held {
    look: Look,
    app: App,
    program: String,
    /// The name the surface says the application's service has on the
    /// session bus. Linux only: the bus is Linux's.
    #[cfg(target_os = "linux")]
    service: Option<String>,
    /// The management class the surface asks the scout to read, and its
    /// namespace. Windows only.
    #[cfg(windows)]
    management_class: Option<String>,
    #[cfg(windows)]
    management_namespace: Option<String>,
    sections: BTreeMap<usize, ScreenSection>,
    /// The window the last `goto` found.
    window: Option<El>,
    limits: Limits,
    posture: Posture,
    /// What proved the application's process at prepare, proved again
    /// before every act.
    #[cfg(target_os = "linux")]
    proof: crate::backend::Proof,
    #[cfg(target_os = "linux")]
    desk: Arc<Mutex<()>>,
    /// The desktop, while this binding's write holds it.
    #[cfg(target_os = "linux")]
    holding: Option<OwnedMutexGuard<()>>,
}

fn unsent(error: &AxError, program: &str) -> StepFailure {
    StepFailure::not_sent(error.failure(program))
}

fn drift(row: &a2m_core::CodeEntry, message: String, next: &str) -> StepFailure {
    StepFailure::not_sent(codes::failure(row, message, next))
}

fn missed(miss: Miss, what: &str) -> StepFailure {
    match miss {
        Miss::None => drift(
            &codes::row::LOCATOR_NOT_FOUND,
            format!("No element of the window matches the locators of {what}."),
            "Take a snapshot of the window and correct the screen's locators.",
        ),
        Miss::Many(count) => drift(
            &codes::row::LOCATOR_AMBIGUOUS,
            format!(
                "{count} elements of the window match a locator of {what}; the pack reads one only when it is the only one."
            ),
            "Give the screen a locator the application holds once, or leave the field out.",
        ),
    }
}

fn unexpected(message: String) -> StepFailure {
    StepFailure::not_sent(codes::failure(
        &codes::row::UNEXPECTED_PAGE,
        message,
        "Check that the application shows the window the screen declares, then run the tool again.",
    ))
}

impl Held {
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

    /// One walk of the window the last `goto` found, whole.
    async fn walk(&self, deadline: Deadline) -> Result<Tree, StepFailure> {
        let window = self
            .window
            .as_ref()
            .ok_or_else(|| codes::not_usable("no `goto` found the screen's window before it"))?;
        let tree = self
            .look
            .tree(window, &self.limits, deadline)
            .await
            .map_err(|error| unsent(&error, &self.program))?;
        match tree.cut {
            None => Ok(tree),
            Some("seconds") => Err(unsent(&AxError::Timeout, &self.program)),
            Some(bound) => Err(StepFailure::not_sent(codes::failure(
                &codes::row::READ_TOO_LARGE,
                format!(
                    "The window holds more than the desktop pack walks (its bound on {bound}); a locator cannot be proven to match once."
                ),
                "Raise the pack's settings `max_nodes` or `max_depth`, or read a smaller window.",
            ))),
        }
    }

    async fn text(&self, node: &Node) -> Result<String, StepFailure> {
        self.look
            .text(node, &self.limits)
            .await
            .map_err(|error| unsent(&error, &self.program))
    }

    async fn goto(&mut self, step: &Step, deadline: Deadline) -> Result<Observed, StepFailure> {
        let (name, screen) = self.screen(step)?;
        let (name, want) = (name.to_owned(), screen.window.clone());
        let mut watch = self
            .look
            .watch(&self.app)
            .await
            .map_err(|error| unsent(&error, &self.program))?;
        loop {
            let windows = self
                .look
                .windows(&self.app)
                .await
                .map_err(|error| unsent(&error, &self.program))?;
            let found: Vec<El> = windows
                .into_iter()
                .filter(|window| window.role == want.role && window.title == want.title)
                .map(|window| window.el)
                .collect();
            match found.len() {
                0 => {}
                1 => {
                    self.window = found.into_iter().next();
                    return Ok(Observed::default());
                }
                many => {
                    return Err(drift(
                        &codes::row::LOCATOR_AMBIGUOUS,
                        format!(
                            "{many} windows of the application have the role and the title of the screen `{}`.",
                            shown(&name)
                        ),
                        "Close the other windows, or tell the screen's window apart by a title of its own.",
                    ));
                }
            }
            if !watch
                .next(deadline)
                .await
                .map_err(|error| unsent(&error, &self.program))?
            {
                return Err(unexpected(format!(
                    "The application showed no window with the role and the title of the screen `{}` before the deadline.",
                    shown(&name)
                )));
            }
        }
    }

    async fn wait(&self, step: &Step, input: &StepInput<'_>) -> Result<Observed, StepFailure> {
        let deadline = input.deadline;
        let (name, screen) = self.screen(step)?;
        let ready = screen.ready.clone();
        // Listen first, then look: a change between the two is not lost.
        let mut watch = self
            .look
            .watch(&self.app)
            .await
            .map_err(|error| unsent(&error, &self.program))?;
        let what = format!("the screen `{}`'s condition", shown(name));
        loop {
            let tree = self.walk(deadline).await?;
            if self.holds(&tree, &ready, &what).await? {
                self.shows_record(step, screen, &tree, input).await?;
                return Ok(Observed::default());
            }
            if !watch
                .next(deadline)
                .await
                .map_err(|error| unsent(&error, &self.program))?
            {
                return Err(unexpected(format!(
                    "The window of the screen `{}` did not become ready before the deadline.",
                    shown(name)
                )));
            }
        }
    }

    async fn read(&self, step: &Step, deadline: Deadline) -> Result<Observed, StepFailure> {
        let (name, screen) = self.screen(step)?;
        let payload: ReadStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;
        let tree = self.walk(deadline).await?;
        let mut values = Values::new();
        for item in &payload.fields {
            let field = screen
                .fields
                .get(&item.from)
                .ok_or_else(|| codes::not_usable("it reads a field the screen does not have"))?;
            let at = tree
                .one(&field.locate)
                .map_err(|miss| missed(miss, &format!("the field `{}`", shown(&item.from))))?;
            values.insert(item.output.clone(), self.text(&tree.nodes[at]).await?);
        }
        if !payload.columns.is_empty() {
            let rows = screen.rows.as_ref().ok_or_else(|| {
                codes::not_usable(format!("the screen `{}` has no table", shown(name)))
            })?;
            let table = tree
                .one(&rows.locate)
                .map_err(|miss| missed(miss, &format!("the table of `{}`", shown(name))))?;
            let mismatch = |why: &str| {
                drift(
                    &codes::row::FINGERPRINT_MISMATCH,
                    format!("The table of the screen `{}` {why}.", shown(name)),
                    "Take a snapshot of the window and correct the screen's `header`.",
                )
            };
            let mut header = None;
            let mut body = Vec::new();
            for row in &tree.nodes[table].children {
                let row = &tree.nodes[*row];
                if row.role != "table_row" {
                    return Err(mismatch("holds something that is not a row"));
                }
                let cells: Vec<&Node> = row.children.iter().map(|at| &tree.nodes[*at]).collect();
                if cells.iter().all(|cell| cell.role == "table_column_header") {
                    if header.is_some() {
                        return Err(mismatch("has more than one row of headings"));
                    }
                    header = Some(
                        cells
                            .iter()
                            .map(|cell| cell.name.clone())
                            .collect::<Vec<_>>(),
                    );
                } else {
                    body.push(cells);
                }
            }
            if header.as_ref() != Some(&rows.header) {
                return Err(mismatch("does not show the headings it declares"));
            }
            let mut columns: BTreeMap<&str, Vec<Value>> = BTreeMap::new();
            for cells in &body {
                if cells.len() != rows.header.len()
                    || cells.iter().any(|cell| cell.role != "table_cell")
                {
                    return Err(mismatch("has a row without one cell under each heading"));
                }
                for item in &payload.columns {
                    let position = rows
                        .columns
                        .get(&item.from)
                        .and_then(|position| usize::try_from(*position).ok())
                        .ok_or_else(|| {
                            codes::not_usable("it reads a column the table does not have")
                        })?;
                    let shown = self.text(cells[position]).await?;
                    columns
                        .entry(item.output.as_str())
                        .or_default()
                        .push(Value::Text(shown));
                }
            }
            for item in &payload.columns {
                let list = columns.remove(item.output.as_str()).unwrap_or_default();
                values.insert(item.output.clone(), Value::List(list));
            }
        }
        Ok(Observed {
            values,
            ..Observed::default()
        })
    }

    /// For a step that carries the record's key: whether the window shows
    /// exactly that record, in the screen's field of each key input's
    /// name. A window that shows another record is a stop: nothing is
    /// done in it.
    async fn shows_record(
        &self,
        step: &Step,
        screen: &Screen,
        tree: &Tree,
        input: &StepInput<'_>,
    ) -> Result<(), StepFailure> {
        for arg in &step.args {
            let Some(key) = arg.name.strip_prefix(KEY_ARG) else {
                continue;
            };
            let want = value_text(input.args.get(&arg.name))
                .ok_or_else(|| codes::not_usable("a part of the record's key is not text"))?;
            let field = screen
                .fields
                .get(key)
                .ok_or_else(|| codes::not_usable("its key is shown by no field of its screen"))?;
            let at = tree
                .one(&field.locate)
                .map_err(|miss| missed(miss, &format!("the field `{}`", shown(key))))?;
            if self.text(&tree.nodes[at]).await? != want {
                return Err(unexpected(format!(
                    "The window shows another record than the one the run is for: its field `{}` does not show the record's key. Nothing was done in it.",
                    shown(key)
                )));
            }
        }
        Ok(())
    }

    /// What every act proves on the walk it acts on, just before it acts,
    /// whatever system's acting half carries it out: that the window still
    /// shows the record the run is for (the check `wait` makes, again,
    /// since the application may have moved to another record since), and
    /// that the application shows no window that no screen of the target
    /// names, such as a dialog raised since the `wait`. Either is a stop,
    /// with nothing sent (unknown means stop).
    // Only Linux has an acting half in this version; Windows and macOS
    // reach this once theirs are built, and until then it has no caller
    // there.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    async fn before_act(
        &self,
        step: &Step,
        screen: &Screen,
        tree: &Tree,
        input: &StepInput<'_>,
    ) -> Result<(), StepFailure> {
        self.shows_record(step, screen, tree, input).await?;
        let windows = self
            .look
            .windows(&self.app)
            .await
            .map_err(|error| unsent(&error, &self.program))?;
        let known = |role: &str, title: &str| {
            self.sections
                .values()
                .flat_map(|section| section.values())
                .any(|screen| screen.window.role == role && screen.window.title == title)
        };
        if let Some(stranger) = windows
            .iter()
            .find(|window| !known(window.role, &window.title))
        {
            return Err(unexpected(format!(
                "The application shows a window of the role `{}` that no screen of the target names, such as a dialog; nothing is done while one is open. Nothing was sent.",
                stranger.role
            ))
            .with_said(stranger.title.clone()));
        }
        Ok(())
    }

    /// Whether the run's posture lets a step act on the application. A
    /// field is set in a live run and against a declared test record; an
    /// action is done in a live run, and, against a test record, only when
    /// it is the commit point.
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
        Err(StepFailure::not_sent(codes::failure(
            &a2m_provider::codes::row::READ_ONLY_POSTURE,
            format!(
                "A `{}` step acts on the application, and this run may not act so.",
                step.kind
            ),
            "Run the operation live, or against a declared test record.",
        )))
    }

    /// Takes the driver's one desktop for this binding's write, waiting
    /// for another write to end until the step's deadline.
    #[cfg(target_os = "linux")]
    async fn hold(&mut self, deadline: Deadline) -> Result<(), StepFailure> {
        if self.holding.is_some() {
            return Ok(());
        }
        let wait = Arc::clone(&self.desk).lock_owned();
        match tokio::time::timeout_at(deadline.at(), wait).await {
            Ok(guard) => {
                self.holding = Some(guard);
                Ok(())
            }
            Err(_) => Err(StepFailure::not_sent(codes::failure(
                &codes::row::SESSION_IN_USE,
                "Another write held this desktop until the step's deadline; two runs never type into one desktop at once. Nothing was sent.".to_owned(),
                "Run the tool again once the other write has ended.",
            ))),
        }
    }

    /// Whether a condition holds on this walk, its texts read where it
    /// names one.
    async fn holds(&self, tree: &Tree, cond: &Cond, what: &str) -> Result<bool, StepFailure> {
        let mut texts = BTreeMap::new();
        for (at, node) in tree.nodes.iter().enumerate() {
            if node.has_text && !node.secret && condition_reads(cond, node) {
                texts.insert(at, self.text(node).await?);
            }
        }
        let mut text_of = |node: &Node| {
            if node.secret {
                return None;
            }
            tree.nodes
                .iter()
                .position(|known| known.el == node.el)
                .and_then(|at| texts.get(&at).cloned())
                .or_else(|| (!node.has_text).then(|| node.name.clone()))
        };
        tree.holds(cond, &mut text_of)
            .map_err(|miss| missed(miss, what))
    }

    /// The first variant of the screen the walk shows, with what the
    /// element its condition names says: the application's own words.
    #[cfg(target_os = "linux")]
    async fn variant_shown(
        &self,
        tree: &Tree,
        screen: &Screen,
    ) -> Result<Option<(String, Option<String>)>, StepFailure> {
        for (name, variant) in &screen.variants {
            let what = format!("the variant `{}`", shown(name));
            if self.holds(tree, &variant.when, &what).await? {
                let said = match first_locator(&variant.when)
                    .and_then(|locator| tree.one(std::slice::from_ref(locator)).ok())
                {
                    Some(at) if !tree.nodes[at].secret => self.text(&tree.nodes[at]).await.ok(),
                    _ => None,
                };
                return Ok(Some((name.clone(), said)));
            }
        }
        Ok(None)
    }

    #[cfg(not(target_os = "linux"))]
    async fn set(&mut self, step: &Step, _input: &StepInput<'_>) -> Result<Observed, StepFailure> {
        self.may_act(step)?;
        Err(codes::writes_not_built())
    }

    #[cfg(not(target_os = "linux"))]
    async fn invoke(
        &mut self,
        step: &Step,
        _input: &StepInput<'_>,
    ) -> Result<Observed, StepFailure> {
        self.may_act(step)?;
        Err(codes::writes_not_built())
    }

    #[cfg(target_os = "linux")]
    async fn set(&mut self, step: &Step, input: &StepInput<'_>) -> Result<Observed, StepFailure> {
        use crate::backend::{Acted, Settable};
        use crate::model::SetStep;

        self.may_act(step)?;
        let (name, screen) = self.screen(step)?;
        let payload: SetStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;
        let field = screen
            .fields
            .get(&payload.field)
            .ok_or_else(|| codes::not_usable("the field it sets is not on its screen"))?
            .clone();
        let name = name.to_owned();
        let text = value_text(input.args.get(crate::lower::VALUE_ARG)).ok_or_else(|| {
            codes::not_usable(format!(
                "the value for the field `{}` is not one a text control takes",
                shown(&payload.field)
            ))
        })?;
        let deadline = input.deadline;
        self.hold(deadline).await?;
        let what = format!(
            "the field `{}` of the screen `{}`",
            shown(&payload.field),
            shown(&name)
        );
        let tree = self.walk(deadline).await?;
        let at = tree
            .one(&field.locate)
            .map_err(|miss| missed(miss, &what))?;
        let node = tree.nodes[at].clone();
        let (_, screen) = self.screen(step)?;
        self.before_act(step, screen, &tree, input).await?;
        if node.secret {
            return Err(StepFailure::not_sent(codes::failure(
                &codes::row::PASSWORD_SET_REFUSED,
                format!(
                    "{what} holds a secret; it is never set from an input a caller can see. Nothing was sent."
                ),
                "Leave the field out. Signing in to a desktop application is not built in this version.",
            )));
        }
        let hands = self.look.hands(&self.proof);
        hands
            .prove(&self.app, &node)
            .await
            .map_err(|error| unsent(&error, &self.program))?;
        let refused = |row: &a2m_core::CodeEntry, why: String, next: &str| {
            StepFailure::not_sent(codes::failure(
                row,
                format!("{what} {why}. Nothing was sent."),
                next,
            ))
        };
        match hands
            .settable(&node)
            .await
            .map_err(|error| unsent(&error, &self.program))?
        {
            Settable::Yes => {}
            Settable::ReadOnly => {
                return Err(refused(
                    &codes::row::FIELD_READ_ONLY,
                    "takes no value now: the application shows it read-only".to_owned(),
                    "Look at the application; the field may be locked for this record.",
                ));
            }
            Settable::Disabled => {
                return Err(refused(
                    &codes::row::FIELD_READ_ONLY,
                    "takes no value now: the application shows it disabled".to_owned(),
                    "Look at the application; the field may be disabled for this record.",
                ));
            }
            Settable::NoInterface => {
                return Err(refused(
                    &codes::row::UNEXPECTED_CONTROL,
                    "offers no text that may be set".to_owned(),
                    "Take a snapshot of the window and correct the screen's field.",
                ));
            }
        }
        let sent_then = |error: &AxError| StepFailure::unknown(error.failure(&self.program));
        match hands.set_text(&node, &text).await {
            Ok(true) => {}
            Ok(false) => {
                return Err(StepFailure::answered(codes::failure(
                    &codes::row::FIELD_READ_ONLY,
                    format!(
                        "The application did not take the value for {what}. Nothing was saved."
                    ),
                    "Look at the application; the field may be locked for this record.",
                )));
            }
            Err(Acted::NotSent(error)) => return Err(unsent(&error, &self.program)),
            Err(Acted::Unknown(error)) => return Err(sent_then(&error)),
        }
        // The field is asked what it holds now: an application that
        // changed the value (cut it, formatted it) stops the run here,
        // before anything is saved. This proves the field took the value,
        // never that the application stored it: the operation's read-back,
        // a window reached afresh and read where it shows what is stored,
        // is that proof.
        let held = self
            .look
            .text(&node, &self.limits)
            .await
            .map_err(|error| sent_then(&error))?;
        if held != text {
            return Err(StepFailure::answered(codes::failure(
                &codes::row::VALUE_NOT_HELD,
                format!(
                    "After it was set, {what} holds another value than the one set: the application changed it. Nothing was saved."
                ),
                "Give a value the field holds as it is written, such as a shorter one.",
            )));
        }
        Ok(Observed {
            captures: vec![acting(vec![
                a2m_telemetry::Detail::new("step", "set"),
                a2m_telemetry::Detail::new("screen", name),
                a2m_telemetry::Detail::new("field", payload.field),
                a2m_telemetry::Detail::new("strategy", "editable_text"),
            ])],
            ..Observed::default()
        })
    }

    #[cfg(target_os = "linux")]
    async fn invoke(
        &mut self,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<Observed, StepFailure> {
        self.may_act(step)?;
        self.hold(input.deadline).await?;
        let result = self.invoke_held(step, input).await;
        // A write holds the desktop until its commit point is done, as it
        // ended.
        if step.commits {
            self.holding = None;
        }
        result
    }

    #[cfg(target_os = "linux")]
    async fn invoke_held(
        &self,
        step: &Step,
        input: &StepInput<'_>,
    ) -> Result<Observed, StepFailure> {
        use crate::backend::{Acted, Heard};
        use crate::model::InvokeStep;

        let deadline = input.deadline;
        let (name, screen) = self.screen(step)?;
        let payload: InvokeStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;
        let action = screen
            .actions
            .get(&payload.action)
            .ok_or_else(|| codes::not_usable("the action it does is not on its screen"))?;
        if action.commits != step.commits {
            return Err(codes::not_usable(
                "the step and its action disagree on whether the action saves",
            ));
        }
        let what = format!(
            "the action `{}` of the screen `{}`",
            shown(&payload.action),
            shown(name)
        );
        let tree = self.walk(deadline).await?;
        let at = tree
            .one(&action.locate)
            .map_err(|miss| missed(miss, &what))?;
        let node = tree.nodes[at].clone();
        let hands = self.look.hands(&self.proof);
        hands
            .prove(&self.app, &node)
            .await
            .map_err(|error| unsent(&error, &self.program))?;
        // Listen first, then look, then act: a change after the act is not
        // lost.
        let mut watch = self
            .look
            .watch(&self.app)
            .await
            .map_err(|error| unsent(&error, &self.program))?;
        // What the window shows after the action is its answer only if it
        // was not there before: a window that already shows the
        // confirmation, or a refusal, could not tell its answer from it.
        let confirm = format!("the confirmation of {what}");
        if self.holds(&tree, &action.confirm, &confirm).await? {
            return Err(unexpected(format!(
                "Before {what}, the window already shows the action's confirmation, so its answer could not be told from it. The action was not done."
            )));
        }
        if let Some((variant, said)) = self.variant_shown(&tree, screen).await? {
            let refused = StepFailure::not_sent(codes::failure(
                &codes::row::APP_VALIDATION_ERROR,
                format!(
                    "Before {what}, the window shows the variant `{}`, so its answer could not be told from it. The action was not done.",
                    shown(&variant)
                ),
                "Correct the values and run the tool again; the application refused them before the action.",
            ));
            return Err(match said {
                Some(said) => refused.with_said(said),
                None => refused,
            });
        }
        // Last before the act, on the walk it acts on: the record, and no
        // window the screens do not name.
        self.before_act(step, screen, &tree, input).await?;
        match hands.perform(&node, &action.perform).await {
            Ok(Some(true)) => {}
            Ok(None) => {
                return Err(StepFailure::not_sent(codes::failure(
                    &codes::row::UNEXPECTED_CONTROL,
                    format!(
                        "{what} offers no action named `{}` exactly once. Nothing was sent.",
                        shown(&action.perform)
                    ),
                    "Take a snapshot of the window and correct the action's `perform`.",
                )));
            }
            Ok(Some(false)) => {
                return Err(StepFailure::answered(codes::failure(
                    &codes::row::UNEXPECTED_CONTROL,
                    format!("The application said it did not do {what}."),
                    "Look at the application; the action may not be offered for this record.",
                )));
            }
            Err(Acted::NotSent(error)) => return Err(unsent(&error, &self.program)),
            Err(Acted::Unknown(_)) => {
                return Err(self.no_confirmation(step, &what, "the application did not answer it"));
            }
        }
        // From here the action reached the application. What it made of
        // it is known only when the window shows the action's confirmation,
        // or a refusal the screen declares, neither of which was there
        // before.
        loop {
            let Ok(tree) = self.walk(deadline).await else {
                return Err(self.no_confirmation(step, &what, "then the window could not be read"));
            };
            let Ok(confirmed) = self.holds(&tree, &action.confirm, &confirm).await else {
                return Err(self.no_confirmation(
                    step,
                    &what,
                    "then its confirmation could not be told",
                ));
            };
            let Ok(refused) = self.variant_shown(&tree, screen).await else {
                return Err(self.no_confirmation(
                    step,
                    &what,
                    "then the window could not be asked which state it is in",
                ));
            };
            match (confirmed, refused) {
                (true, Some((variant, _))) => {
                    return Err(self.no_confirmation(
                        step,
                        &what,
                        &format!(
                            "then the window showed both its confirmation and the variant `{}`",
                            shown(&variant)
                        ),
                    ));
                }
                (true, None) => {
                    return Ok(Observed {
                        captures: vec![acting(vec![
                            a2m_telemetry::Detail::new("step", "invoke"),
                            a2m_telemetry::Detail::new("screen", name),
                            a2m_telemetry::Detail::new("action", payload.action.as_str()),
                        ])],
                        ..Observed::default()
                    });
                }
                (false, Some((variant, said))) => {
                    // A refusal the window shows after the action, which was
                    // not there before it, is the application's answer.
                    let refused = StepFailure::answered(codes::failure(
                        &codes::row::APP_VALIDATION_ERROR,
                        format!(
                            "After {what}, the window shows the variant `{}`.",
                            shown(&variant)
                        ),
                        "Correct the values and run the tool again; the application refused them.",
                    ));
                    return Err(match said {
                        Some(said) => refused.with_said(said),
                        None => refused,
                    });
                }
                (false, None) => {}
            }
            match watch.heard(deadline).await {
                Ok(Heard::Changed) => {}
                Ok(Heard::Gone) => {
                    return Err(self.no_confirmation(
                        step,
                        &what,
                        "then the application exited before it showed its confirmation",
                    ));
                }
                Ok(Heard::Passed) => {
                    return Err(self.no_confirmation(
                        step,
                        &what,
                        "the window did not show its confirmation in the time the step allows",
                    ));
                }
                Err(_) => {
                    return Err(self.no_confirmation(
                        step,
                        &what,
                        "then the accessibility bus was lost",
                    ));
                }
            }
        }
    }

    /// An action that was sent, whose fate is not known: for one that
    /// saves, the record is in doubt.
    #[cfg(target_os = "linux")]
    fn no_confirmation(&self, step: &Step, what: &str, why: &str) -> StepFailure {
        let row = if step.commits {
            &a2m_provider::codes::row::NO_CONFIRMATION
        } else {
            &codes::row::UNEXPECTED_PAGE
        };
        StepFailure::unknown(codes::failure(
            row,
            format!("{what} was done, and {why}."),
            "Reconcile the record: read it back before anything is written again.",
        ))
    }

    /// What the surface's service says of itself: the introspection of each
    /// of its objects, asked of a service that is running and never of one
    /// that is not (decision 0316). A service no connection owns is an
    /// answer, `running: false`, and not a failure.
    #[cfg(target_os = "linux")]
    async fn introspect(&self, deadline: Deadline) -> Result<Observed, StepFailure> {
        let service = self
            .service
            .as_deref()
            .ok_or_else(|| codes::not_usable("the surface names no `service`"))?;
        let shape = match self.look.introspect(service, deadline).await {
            Ok(walk) => json!({
                "format": SNAPSHOT_FORMAT,
                "running": true,
                "cut": walk.cut,
                "objects": walk
                    .objects
                    .iter()
                    .map(|(path, xml)| json!({"path": path, "xml": xml}))
                    .collect::<Vec<_>>(),
            }),
            Err(AxError::ServiceNotRunning) => json!({
                "format": SNAPSHOT_FORMAT,
                "running": false,
                "cut": null,
                "objects": [],
            }),
            Err(error) => return Err(unsent(&error, &self.program)),
        };
        Ok(Observed {
            values: Values::new().with("shape", shape.to_string()),
            ..Observed::default()
        })
    }

    /// One property of one object of the surface's service, as text. A
    /// service that is not running is a named stop, nothing started.
    #[cfg(target_os = "linux")]
    async fn property(&self, step: &Step) -> Result<Observed, StepFailure> {
        let payload: PropertyStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;
        let service = self
            .service
            .as_deref()
            .ok_or_else(|| codes::not_usable("the surface names no `service`"))?;
        let text = self
            .look
            .property_text(
                service,
                &payload.path,
                &payload.interface,
                &payload.name,
                &self.limits,
            )
            .await
            .map_err(|error| unsent(&error, &self.program))?;
        let mut values = Values::new();
        values.insert(payload.output, text);
        Ok(Observed {
            values,
            ..Observed::default()
        })
    }

    #[cfg(not(target_os = "linux"))]
    #[allow(
        clippy::unused_async,
        reason = "the same call as on Linux, where it asks the bus"
    )]
    async fn introspect(&self, _deadline: Deadline) -> Result<Observed, StepFailure> {
        Err(codes::service_not_built())
    }

    #[cfg(not(target_os = "linux"))]
    #[allow(
        clippy::unused_async,
        reason = "the same call as on Linux, where it asks the bus"
    )]
    async fn property(&self, step: &Step) -> Result<Observed, StepFailure> {
        let _: PropertyStep = step
            .payload
            .decode()
            .map_err(|_| codes::not_usable("its payload does not decode"))?;
        Err(codes::service_not_built())
    }

    /// The definition of the management class the surface names: on Windows
    /// as the system's management service gives it, elsewhere a named stop
    /// with nothing sent (decision 0318).
    #[cfg(windows)]
    async fn schema(&self) -> Result<Observed, StepFailure> {
        let class = self
            .management_class
            .clone()
            .ok_or_else(|| codes::not_usable("the surface names no `management_class`"))?;
        let namespace = self
            .management_namespace
            .clone()
            .unwrap_or_else(|| crate::windows_kinds::DEFAULT_NAMESPACE.to_owned());
        let read = crate::backend::wmi::read(namespace.clone(), class.clone())
            .await
            .map_err(|error| unsent(&error, &self.program))?;
        let shape = json!({
            "format": SNAPSHOT_FORMAT,
            "namespace": namespace,
            "class": class,
            "singleton": read.singleton,
            "cut": read.cut,
            "properties": read.properties.iter().map(|property| json!({
                "name": property.name,
                "cim_type": property.cim_type,
                "writable": property.writable,
            })).collect::<Vec<_>>(),
            "methods": read.methods,
        });
        Ok(Observed {
            values: Values::new().with("shape", shape.to_string()),
            ..Observed::default()
        })
    }

    #[cfg(not(windows))]
    #[allow(
        clippy::unused_async,
        reason = "the same call as on Windows, where it asks the system"
    )]
    async fn schema(&self) -> Result<Observed, StepFailure> {
        Err(codes::management_not_built())
    }

    /// The shape of every window of the application.
    async fn snapshot(&self, deadline: Deadline) -> Result<Observed, StepFailure> {
        let windows = self
            .look
            .windows(&self.app)
            .await
            .map_err(|error| unsent(&error, &self.program))?;
        let mut shapes = Vec::new();
        for window in windows {
            let tree = self
                .look
                .tree(&window.el, &self.limits, deadline)
                .await
                .map_err(|error| unsent(&error, &self.program))?;
            let elements: Vec<serde_json::Value> = tree
                .nodes
                .iter()
                .map(|node| {
                    let mut one = json!({"depth": node.depth, "role": node.role});
                    if let Some(name) = Tree::shape_name(node) {
                        one["name"] = json!(name);
                    }
                    if node.name.is_empty() {
                        one["unnamed"] = json!(true);
                    }
                    if node.secret {
                        one["secret"] = json!(true);
                    }
                    one
                })
                .collect();
            shapes.push(json!({
                "role": window.role,
                "title": window.title,
                "cut": tree.cut,
                "elements": elements,
            }));
        }
        let shape = json!({
            "format": SNAPSHOT_FORMAT,
            "windows": shapes,
        });
        Ok(Observed {
            values: Values::new().with("shape", shape.to_string()),
            ..Observed::default()
        })
    }
}

/// Whether a condition reads the text of this element.
fn condition_reads(cond: &Cond, node: &Node) -> bool {
    match cond {
        Cond::Visible(_) => false,
        Cond::Text { locate, .. } => node.role == locate.role && node.name == locate.name,
        Cond::All(all) => all.iter().any(|one| condition_reads(one, node)),
    }
}

/// The first locator a condition names: where a variant's words are.
#[cfg(target_os = "linux")]
fn first_locator(cond: &Cond) -> Option<&crate::model::Locator> {
    match cond {
        Cond::Visible(locator)
        | Cond::Text {
            locate: locator, ..
        } => Some(locator),
        Cond::All(all) => all.iter().find_map(first_locator),
    }
}

/// A value as the text a field takes: text, a whole number, or an exact
/// number in its own written form. Nothing else is set.
fn value_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Text(text) => Some(text.clone()),
        Value::Int(number) => Some(number.to_string()),
        Value::Decimal(number) => Some(number.to_string()),
        _ => None,
    }
}

/// How a step was done, for its trace.
#[cfg(target_os = "linux")]
fn acting(means: Vec<a2m_telemetry::Detail>) -> a2m_provider::Capture {
    a2m_provider::Capture::acting(&a2m_telemetry::Acting {
        means,
        measured: Vec::new(),
    })
}

fn sections_of(cx: &PrepareContext<'_>) -> Result<BTreeMap<usize, ScreenSection>, StepFailure> {
    let mut sections = BTreeMap::new();
    for (position, section) in cx.plan.packs.iter().enumerate() {
        let own = section.pack == PACK
            && section.kind == SCREEN_KIND
            && section.surface_kind == cx.surface.surface.kind;
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
    Ok(sections)
}

impl Driver for DesktopDriver {
    fn prepare<'a>(
        &'a self,
        cx: &'a PrepareContext<'a>,
        _lease: Option<&'a SessionLease>,
    ) -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        Box::pin(async move {
            let surface = &cx.surface.surface;
            let program = surface.address.clone();
            let fields = if surface.payload.is_none() {
                SurfaceFields::default()
            } else {
                surface
                    .payload
                    .decode::<SurfaceFields>()
                    .map_err(|_| codes::not_usable("the surface's fields do not decode"))?
            };
            if fields.application.is_empty() {
                return Err(codes::not_usable("the surface names no application"));
            }
            let sections = sections_of(cx)?;
            let look = Look::connect(self.session.as_deref())
                .await
                .map_err(|error| unsent(&error, &program))?;
            let app = match look
                .application(&program, &fields.application, fields.digest.as_deref())
                .await
            {
                Ok(app) => app,
                Err(error) => {
                    look.close().await;
                    return Err(unsent(&error, &program));
                }
            };
            #[cfg(target_os = "linux")]
            let proof = match look.proof(&app, fields.digest.as_deref()) {
                Ok(proof) => proof,
                Err(error) => {
                    look.close().await;
                    return Err(unsent(&error, &program));
                }
            };
            Ok(Prepared::new(Held {
                look,
                app,
                program,
                #[cfg(target_os = "linux")]
                service: fields.service.clone(),
                #[cfg(windows)]
                management_class: fields.management_class.clone(),
                #[cfg(windows)]
                management_namespace: fields.management_namespace.clone(),
                sections,
                window: None,
                limits: self.limits,
                posture: cx.posture,
                #[cfg(target_os = "linux")]
                proof,
                #[cfg(target_os = "linux")]
                desk: Arc::clone(&self.desk),
                #[cfg(target_os = "linux")]
                holding: None,
            }))
        })
    }

    fn act<'a>(
        &'a self,
        prepared: &'a mut Prepared,
        step: &'a Step,
        input: &'a StepInput<'a>,
    ) -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            let held = prepared
                .state::<Held>()
                .ok_or_else(|| codes::not_usable("it was prepared by another driver"))?;
            let deadline = input.deadline;
            let observed = if step.kind == GOTO {
                held.goto(step, deadline).await?
            } else if step.kind == WAIT {
                held.wait(step, input).await?
            } else if step.kind == READ {
                held.read(step, deadline).await?
            } else if step.kind == SNAPSHOT {
                held.snapshot(deadline).await?
            } else if step.kind == INTROSPECT {
                held.introspect(deadline).await?
            } else if step.kind == PROPERTY {
                held.property(step).await?
            } else if step.kind == SCHEMA {
                held.schema().await?
            } else if step.kind == MANAGEMENT_GET || step.kind == COM_GET {
                return Err(codes::step_not_built(step.kind.as_str()));
            } else if step.kind == SET {
                held.set(step, input).await?
            } else if step.kind == INVOKE {
                held.invoke(step, input).await?
            } else {
                return Err(codes::not_usable(format!(
                    "this version has no `{}` step",
                    shown(step.kind.as_str())
                )));
            };
            Ok(StepOutcome::Done(observed))
        })
    }

    fn health(&self, _surface: &SurfaceRef) -> DriverHealth {
        if cfg!(any(target_os = "linux", windows, target_os = "macos")) {
            DriverHealth::Ready
        } else {
            DriverHealth::Unavailable(codes::SYSTEM_NOT_BUILT)
        }
    }

    fn release<'a>(&'a self, prepared: Prepared, _poisoned: bool) -> BoxFuture<'a, Cleanup> {
        Box::pin(async move {
            if let Ok(held) = prepared.into_state::<Held>() {
                held.look.close().await;
            }
            Cleanup::complete()
        })
    }
}
