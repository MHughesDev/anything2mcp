//! The `desktop` driver, for reads: it finds the application the surface
//! declares, already running in the engine's own session, and carries out
//! `goto`, `wait`, `read`, and the discoverer's `snapshot` through the
//! look-only back end.
//!
//! | Step | What it does |
//! | --- | --- |
//! | `goto` | Finds the one window of the application with the screen's role and title, waiting for the application to signal one until the step's deadline |
//! | `wait` | Reads the window's tree, and again each time the application signals a change, until the screen's `ready` condition holds or the deadline passes |
//! | `read` | Reads each field's value, or each column of the table as a list. A control that holds a secret is refused, never read |
//! | `snapshot` | The shape of every window of the application: roles, the names that are the window's shape, the structure. Never a value |
//!
//! Nothing here starts a program, and nothing reaches another application:
//! the back end asks the bus daemon which process owns each application's
//! connection and asks an application nothing until it is proven to be
//! the program the surface declares.

use std::collections::BTreeMap;
use std::sync::Arc;

use a2m_auth::SessionLease;
use a2m_core::{FailureClass, Step, Value, Values};
use a2m_provider::{
    BoxFuture, Cleanup, Deadline, Driver, DriverHealth, Observed, PrepareContext, Prepared,
    StepFailure, StepInput, StepOutcome, SurfaceRef,
};
use serde_json::json;

use crate::backend::{App, AxError, Limits, Look};
use crate::codes::{self, shown};
use crate::lower::{GOTO, READ, SNAPSHOT, WAIT};
use crate::model::{PAYLOAD_FORMAT, ReadStep, Screen, ScreenSection, SurfaceFields};
use crate::screens::{PACK, SCREEN_KIND};
use crate::tree::{El, Miss, Node, Tree};

/// The format of the shapes a `snapshot` step gives.
pub(crate) const SNAPSHOT_FORMAT: &str = "1";

/// The `desktop` driver.
pub(crate) struct DesktopDriver {
    /// The session bus, when not the one the engine's environment names.
    session: Option<Arc<str>>,
    limits: Limits,
}

impl DesktopDriver {
    pub(crate) fn new(session: Option<Arc<str>>, limits: Limits) -> Self {
        Self { session, limits }
    }
}

/// What `prepare` acquired.
struct Held {
    look: Look,
    app: App,
    program: String,
    sections: BTreeMap<usize, ScreenSection>,
    /// The window the last `goto` found.
    window: Option<El>,
    limits: Limits,
}

fn unsent(error: &AxError, program: &str) -> StepFailure {
    StepFailure::not_sent(error.failure(program))
}

fn drift(code: a2m_core::FailureCode, message: String, next: &str) -> StepFailure {
    StepFailure::not_sent(codes::failure(FailureClass::Drift, code, message, next))
}

fn missed(miss: Miss, what: &str) -> StepFailure {
    match miss {
        Miss::None => drift(
            codes::LOCATOR_NOT_FOUND,
            format!("No element of the window matches the locators of {what}."),
            "Take a snapshot of the window and correct the screen's locators.",
        ),
        Miss::Many(count) => drift(
            codes::LOCATOR_AMBIGUOUS,
            format!(
                "{count} elements of the window match a locator of {what}; the pack reads one only when it is the only one."
            ),
            "Give the screen a locator the application holds once, or leave the field out.",
        ),
    }
}

fn unexpected(message: String) -> StepFailure {
    StepFailure::not_sent(codes::failure(
        FailureClass::UnknownVariant,
        codes::UNEXPECTED_PAGE,
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
                FailureClass::UnknownVariant,
                codes::READ_TOO_LARGE,
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
                        codes::LOCATOR_AMBIGUOUS,
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

    async fn wait(&self, step: &Step, deadline: Deadline) -> Result<Observed, StepFailure> {
        let (name, screen) = self.screen(step)?;
        let ready = screen.ready.clone();
        // Listen first, then look: a change between the two is not lost.
        let mut watch = self
            .look
            .watch(&self.app)
            .await
            .map_err(|error| unsent(&error, &self.program))?;
        loop {
            let tree = self.walk(deadline).await?;
            let mut texts = BTreeMap::new();
            for (at, node) in tree.nodes.iter().enumerate() {
                if node.has_text && !node.secret && condition_reads(&ready, node) {
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
            match tree.holds(&ready, &mut text_of) {
                Ok(true) => return Ok(Observed::default()),
                Ok(false) => {}
                Err(miss) => return Err(missed(miss, &format!("the screen `{}`'s condition", shown(name)))),
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
                    codes::FINGERPRINT_MISMATCH,
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
                    header = Some(cells.iter().map(|cell| cell.name.clone()).collect::<Vec<_>>());
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
                        .ok_or_else(|| codes::not_usable("it reads a column the table does not have"))?;
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
fn condition_reads(cond: &crate::model::Cond, node: &Node) -> bool {
    match cond {
        crate::model::Cond::Visible(_) => false,
        crate::model::Cond::Text { locate, .. } => {
            node.role == locate.role && node.name == locate.name
        }
        crate::model::Cond::All(all) => all.iter().any(|one| condition_reads(one, node)),
    }
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
            Ok(Prepared::new(Held {
                look,
                app,
                program,
                sections,
                window: None,
                limits: self.limits,
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
                held.wait(step, deadline).await?
            } else if step.kind == READ {
                held.read(step, deadline).await?
            } else if step.kind == SNAPSHOT {
                held.snapshot(deadline).await?
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
        if cfg!(target_os = "linux") {
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
