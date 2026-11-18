//! The lowering of the `desktop` mode: one binding into `goto`, `wait`,
//! `read`, `set`, and `invoke` steps, each naming its screen in the pack's
//! section of the plan and carrying this pack's own payload.
//!
//! | Verb | Act | Read back |
//! | --- | --- | --- |
//! | `read`, `list` | `goto` (find the screen's window among the application's), `wait` (its `ready` condition, and the record's key), `read` (fields, or a table's columns as lists) | |
//! | `update`, `upsert` | `goto`, `wait`, one `set` for each field, `invoke` of the action, marked as the commit point | `goto`, `wait`, `read` on the screen `read_back` names: the window reached afresh, read where it shows what is stored |
//!
//! There is no separate `locate` step: each `read`, `set`, and `invoke`
//! finds its own element in a fresh walk of the window, as for `ui`
//! (decision 0112). The committing `invoke` is not done until the window
//! shows the action's `confirm`, so the confirm list is empty. The window
//! has no state for a record that is not there in this version, so an
//! `upsert` changes the record the window shows, as `update` does
//! (decision 0218).
//!
//! When the operation has a key, the `wait` step carries it, and the
//! driver stops unless the window shows exactly that record in the screen's
//! field of each key input's name.

use a2m_core::{
    EffectClass, FailsAs, FailureClass, FileKindName, IdentityStrategy, Mode, PackPayload, PackRef,
    SectionIndex, Step, StepArg, StepKind, SurfaceKind, SurfaceName, ValueType, Verb,
};
use a2m_files::{
    Diagnostics, Doc, JsonSchema, Located, LowerContext, Lowered, Lowering, Place, Possible,
    ResolvedBinding, Stage, StepCommits,
};

use crate::codes::{self, shown};
use crate::model::{
    InvokeStep, PAYLOAD_FORMAT, ReadItem, ReadStep, Screen, ScreenSection, SetStep,
};
use crate::screens::{DESKTOP_SURFACE, SCREEN_KIND, problem};

/// The step that finds a screen's window among the application's windows.
pub const GOTO: StepKind = StepKind::fixed("goto");
/// The step that waits until a screen's window can be used.
pub const WAIT: StepKind = StepKind::fixed("wait");
/// The step that reads fields, or the rows of a table.
pub const READ: StepKind = StepKind::fixed("read");
/// The step that sets one field's text.
pub const SET: StepKind = StepKind::fixed("set");
/// The step that does an action and waits for what the window shows.
pub const INVOKE: StepKind = StepKind::fixed("invoke");
/// The step a discoverer sends: the shape of the application's windows.
pub const SNAPSHOT: StepKind = StepKind::fixed("snapshot");

const READ_VERBS: &[&str] = &["read", "list"];
const WRITE_VERBS: &[&str] = &["update", "upsert"];

/// The name of the argument of a `set` step.
pub(crate) const VALUE_ARG: &str = "value";
/// What the name of a `wait` step's argument for a part of the record's
/// key starts with.
pub(crate) const KEY_ARG: &str = "key.";

/// How long a step may take when the target states no timeout under the
/// step kind's name.
const DEFAULT_TIMEOUT_MS: u32 = 30_000;

/// The lowering of the `desktop` mode.
pub(crate) struct DesktopLowering {
    mode: Mode,
    schema: JsonSchema,
    verbs: Vec<Verb>,
    step_kinds: Vec<StepKind>,
}

impl DesktopLowering {
    pub(crate) fn new(mode: Mode) -> Self {
        Self {
            mode,
            schema: JsonSchema::object()
                .required("screen", JsonSchema::name())
                .optional("fields", JsonSchema::map_of(JsonSchema::expression()))
                .optional("action", JsonSchema::name())
                .optional(
                    "read_back",
                    JsonSchema::object().required("screen", JsonSchema::name()),
                ),
            verbs: READ_VERBS
                .iter()
                .chain(WRITE_VERBS)
                .map(|verb| Verb::fixed(verb))
                .collect(),
            step_kinds: vec![GOTO, WAIT, READ, SET, INVOKE],
        }
    }
}

struct Found<'a> {
    name: &'a str,
    at: &'a Place,
    section: SectionIndex,
    screen: Screen,
}

struct Lower<'a, 'b> {
    cx: &'a LowerContext<'b>,
    section: Option<(SectionIndex, ScreenSection)>,
    problems: Diagnostics,
}

impl<'a> Lower<'a, '_> {
    fn refuse(&mut self, rule: &'static str, stage: Stage, at: &Place, message: String, fix: &str) {
        self.problems.push(problem(rule, stage, at, message, fix));
    }

    /// The screen a binding, or its `read_back`, names.
    fn screen(&mut self, part: &'a Located<Doc>) -> Option<Found<'a>> {
        let named = part.get("screen");
        let name = named.and_then(|named| named.as_text()).unwrap_or_default();
        let at = named.map_or(&part.at, |named| &named.at);
        let found = self.section.as_ref().and_then(|(index, section)| {
            section.get(name).map(|screen| Found {
                name,
                at,
                section: *index,
                screen: screen.clone(),
            })
        });
        if found.is_none() {
            self.refuse(
                "unknown_screen",
                Stage::Resolve,
                at,
                format!(
                    "No file under `screens/` defines a screen named `{}` on a surface of the kind `desktop`.",
                    shown(name)
                ),
                "Name a screen that `screens/` defines on a `desktop` surface.",
            );
        }
        found
    }

    fn step(&self, kind: StepKind, found: &Found<'_>, fails_as: FailsAs) -> Step {
        Step {
            timeout_ms: self
                .cx
                .target()
                .default_timeout_ms(kind.as_str())
                .unwrap_or(DEFAULT_TIMEOUT_MS),
            kind,
            refers_to: Some(PackRef {
                section: found.section,
                entry: found.name.to_owned(),
            }),
            args: Vec::new(),
            fails_as,
            commits: false,
            lookup: false,
            idempotent: true,
            when: None,
            payload: PackPayload::none(),
        }
    }

    fn encoded<T: serde::Serialize>(&mut self, value: &T, at: &Place) -> PackPayload {
        match PackPayload::encode(value) {
            Ok(payload) => payload,
            Err(failure) => {
                self.refuse(
                    "not_encodable",
                    Stage::Lower,
                    at,
                    failure.message().to_owned(),
                    "Report this to the desktop pack's author.",
                );
                PackPayload::none()
            }
        }
    }

    fn unexpected() -> FailsAs {
        FailsAs {
            class: FailureClass::UnknownVariant,
            code: codes::UNEXPECTED_PAGE,
        }
    }

    fn not_found() -> FailsAs {
        FailsAs {
            class: FailureClass::Drift,
            code: codes::LOCATOR_NOT_FOUND,
        }
    }

    fn goto(&self, found: &Found<'_>) -> Step {
        self.step(GOTO, found, Self::unexpected())
    }

    /// The `wait` step, with one argument for each part of the record's
    /// key: the window must show that record in the screen's field of the
    /// key input's name, which the screen must have.
    fn wait(&mut self, found: &Found<'_>) -> Step {
        let mut args = Vec::new();
        for key in self.cx.operation().key() {
            if !found.screen.fields.contains_key(key) {
                self.refuse(
                    "key_unshown",
                    Stage::Resolve,
                    found.at,
                    format!(
                        "The record's key `{key}` is shown by no field of the screen `{}`, so the driver could not prove the window shows the record the run is for.",
                        found.name
                    ),
                    "Add a field of that name to the screen, where the window shows the record's key.",
                );
                continue;
            }
            match self.cx.expr_at(&format!("input.{key}"), found.at, None) {
                Ok(index) => args.push(StepArg {
                    name: format!("{KEY_ARG}{key}"),
                    value: index,
                }),
                Err(wrong) => self.problems.push(wrong),
            }
        }
        let mut step = self.step(WAIT, found, Self::unexpected());
        step.args = args;
        step
    }

    /// The `read` step: one value for each output of the operation, from
    /// the screen's field of the same name, or, for a list, from its
    /// table's column of that name.
    fn read(&mut self, found: &Found<'_>) -> Step {
        let mut payload = ReadStep::default();
        for output in self.cx.operation().outputs() {
            let listed = matches!(output.ty, ValueType::List(_));
            let known = if listed {
                found
                    .screen
                    .rows
                    .as_ref()
                    .is_some_and(|rows| rows.columns.contains_key(&output.name))
            } else {
                found.screen.fields.contains_key(&output.name)
            };
            if !known {
                self.refuse(
                    "unknown_field",
                    Stage::Resolve,
                    found.at,
                    format!(
                        "The output `{}` matches no {} on the screen `{}`.",
                        output.name,
                        if listed {
                            "column of its table"
                        } else {
                            "field"
                        },
                        found.name
                    ),
                    "Add it to the screen, or rename the output.",
                );
                continue;
            }
            let item = ReadItem {
                output: output.name.clone(),
                from: output.name.clone(),
            };
            if listed {
                payload.columns.push(item);
            } else {
                payload.fields.push(item);
            }
        }
        let mut step = self.step(READ, found, Self::not_found());
        step.payload = self.encoded(&payload, found.at);
        step
    }

    /// One `set` step for each entry of the binding's `fields`, in the
    /// order they are written.
    fn sets(&mut self, found: &Found<'_>, binding: &Located<Doc>) -> Vec<Step> {
        let mut steps = Vec::new();
        let irreversible = self.cx.operation().effect() == EffectClass::Irreversible;
        for entry in binding
            .get("fields")
            .map(Located::entries)
            .unwrap_or_default()
        {
            let name = &entry.key.value;
            let Some(field) = found.screen.fields.get(name) else {
                self.refuse(
                    "unknown_field",
                    Stage::Resolve,
                    &entry.key.at,
                    format!(
                        "The screen `{}` has no field named `{}`.",
                        found.name,
                        shown(name)
                    ),
                    "Add the field to the screen, or correct its name.",
                );
                continue;
            };
            if field
                .locate
                .iter()
                .any(|locator| locator.role == "password_text")
            {
                self.refuse(
                    "password_set",
                    Stage::Analyze,
                    &entry.key.at,
                    format!(
                        "The field `{}` is a password field, and a password is never set from an input a caller can see.",
                        shown(name)
                    ),
                    "Leave the field out. Signing in to a desktop application is not built in this version.",
                );
                continue;
            }
            // Entry bar item 2: every value written is read back, or the
            // operation says it cannot be.
            let read_back = self.cx.operation().outputs().iter().any(|output| {
                output.name == *name && !matches!(output.ty, ValueType::List(_))
            });
            if !read_back && !irreversible {
                self.refuse(
                    "unreadable_write",
                    Stage::Analyze,
                    &entry.key.at,
                    format!(
                        "The field `{}` is written and is no output of the operation, so its value is never read back.",
                        shown(name)
                    ),
                    "Add an output of that name, read from the read-back screen, or declare `effect: irreversible`.",
                );
            }
            let index = match self.cx.expr(&entry.value, None) {
                Ok(index) => index,
                Err(wrong) => {
                    self.problems.push(wrong);
                    continue;
                }
            };
            let fits = self.cx.expr_type(index).is_some_and(|ty| {
                matches!(
                    ty,
                    ValueType::Text | ValueType::Choice | ValueType::Int | ValueType::Decimal
                )
            });
            if !fits {
                self.refuse(
                    "value_type",
                    Stage::TypeCheck,
                    &entry.value.at,
                    format!(
                        "The value for the field `{}` cannot be given to a `{}` control, which takes text, a code, a whole number, or an exact number.",
                        shown(name),
                        shown(&field.control)
                    ),
                    "Give a value of such a type.",
                );
            }
            let mut step = self.step(SET, found, Self::not_found());
            step.args = vec![StepArg {
                name: VALUE_ARG.to_owned(),
                value: index,
            }];
            step.payload = self.encoded(
                &SetStep {
                    field: name.clone(),
                },
                &entry.key.at,
            );
            steps.push(step);
        }
        steps
    }

    fn invoke(&mut self, found: &Found<'_>, binding: &Located<Doc>) -> Option<Step> {
        let Some(named) = binding.get("action") else {
            self.refuse(
                "action_missing",
                Stage::Resolve,
                &binding.at,
                "A binding that writes through a window names the `action` that saves.".to_owned(),
                "Add `action:` with an action of the screen.",
            );
            return None;
        };
        let name = named.as_text()?;
        let Some(action) = found.screen.actions.get(name) else {
            self.refuse(
                "unknown_action",
                Stage::Resolve,
                &named.at,
                format!(
                    "The screen `{}` has no action named `{}`.",
                    found.name,
                    shown(name)
                ),
                "Add the action to the screen, or correct its name.",
            );
            return None;
        };
        let mut step = self.step(INVOKE, found, Self::not_found());
        step.commits = action.commits;
        step.idempotent = !action.commits;
        step.payload = self.encoded(
            &InvokeStep {
                action: name.to_owned(),
            },
            &named.at,
        );
        Some(step)
    }
}

impl Lowering for DesktopLowering {
    fn mode(&self) -> Mode {
        self.mode.clone()
    }

    fn verbs(&self) -> &[Verb] {
        &self.verbs
    }

    fn reads(&self) -> &[(FileKindName, SurfaceKind)] {
        &[]
    }

    fn step_kinds(&self) -> &[StepKind] {
        &self.step_kinds
    }

    /// Only an `invoke` may save, and then as its action declares; a
    /// `goto`, `wait`, `read`, or `set` never does.
    fn commits(&self, kind: &StepKind) -> StepCommits {
        if *kind == INVOKE {
            StepCommits::AsDeclared
        } else {
            StepCommits::Never
        }
    }

    /// A verb that reads is a read. A write is a write when the binding
    /// names how the record is read back, and `irreversible` when it
    /// names nothing, since what was written could never be read again
    /// (entry bar item 2).
    fn least_effect(&self, binding: &ResolvedBinding<'_>) -> EffectClass {
        if READ_VERBS.contains(&binding.verb.as_str()) {
            EffectClass::Read
        } else if binding.field("read_back").is_none() {
            EffectClass::Irreversible
        } else {
            EffectClass::Write
        }
    }

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn identity(&self, binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        let reads = READ_VERBS.contains(&binding.verb.as_str());
        match strategy {
            IdentityStrategy::None => Possible::Yes,
            IdentityStrategy::Natural | IdentityStrategy::Composite
                if reads || binding.field("read_back").is_some() =>
            {
                Possible::Yes
            }
            IdentityStrategy::Natural | IdentityStrategy::Composite => Possible::No(
                "the binding names no `read_back` screen, where the window shows the record by its key."
                    .to_owned(),
            ),
            IdentityStrategy::Stamped | IdentityStrategy::Find => Possible::No(
                "finding a record by a stamp or a search is not built in this version of the desktop pack."
                    .to_owned(),
            ),
        }
    }

    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let at = &binding.doc.at;
        let verb = binding.verb.as_str();
        let reads = READ_VERBS.contains(&verb);
        if !reads && !WRITE_VERBS.contains(&verb) {
            return Err(Diagnostics::from(problem(
                "writes_not_built",
                Stage::Analyze,
                at,
                format!(
                    "The verb `{}` is not built for a `desktop` binding in this version: it reads, updates, and upserts the record a window shows.",
                    shown(verb)
                ),
                "Bind `read`, `list`, `update`, or `upsert`.",
            )));
        }
        let mut lower = Lower {
            cx,
            section: None,
            problems: Diagnostics::new(),
        };
        if let Some((index, section)) = cx.section(&SCREEN_KIND, &DESKTOP_SURFACE) {
            let decoded = if section.format == PAYLOAD_FORMAT {
                section.payload.decode::<ScreenSection>().ok()
            } else {
                None
            };
            match decoded {
                Some(decoded) => lower.section = Some((index, decoded)),
                None => lower.refuse(
                    "not_encodable",
                    Stage::Lower,
                    at,
                    "The screens were compiled in another format.".to_owned(),
                    "Report this to the desktop pack's author.",
                ),
            }
        }
        let Some(found) = lower.screen(binding.doc) else {
            return Err(lower.problems);
        };
        let surface = SurfaceName::new(&found.screen.surface).map_err(|_| {
            Diagnostics::from(problem(
                "unknown_screen",
                Stage::Resolve,
                found.at,
                "The screen's surface has no well-formed name.",
                "Name a surface the target declares.",
            ))
        })?;
        let mut lowered = Lowered {
            surface,
            act: Vec::new(),
            confirm: Vec::new(),
            read_back: Vec::new(),
            lookup: Vec::new(),
            settle_ms: 0,
            prefer_response: false,
        };
        if reads {
            if cx.operation().effect() != EffectClass::Read {
                lower.refuse(
                    "read_acts",
                    Stage::Analyze,
                    at,
                    format!(
                        "The verb `{verb}` reads, and the operation is declared to change something."
                    ),
                    "Declare `effect: read`, or bind `update` or `upsert` with the action that saves.",
                );
            }
            for key in ["fields", "action", "read_back"] {
                if let Some(stray) = binding.field(key) {
                    lower.refuse(
                        "read_acts",
                        Stage::Analyze,
                        &stray.at,
                        format!(
                            "The verb `{verb}` reads, and the binding states `{key}`, which belongs to a write."
                        ),
                        "Remove it, or bind a verb that writes.",
                    );
                }
            }
            lowered.act = vec![lower.goto(&found), lower.wait(&found), lower.read(&found)];
        } else {
            let mut act = vec![lower.goto(&found), lower.wait(&found)];
            act.extend(lower.sets(&found, binding.doc));
            act.extend(lower.invoke(&found, binding.doc));
            lowered.act = act;
            // The read-back is a new visit: the window found again, waited
            // for again, and read where it shows what is stored, never in
            // the fields the values were typed into.
            if let Some(read_back) = binding.field("read_back")
                && let Some(reader) = lower.screen(read_back)
            {
                lowered.read_back = vec![
                    lower.goto(&reader),
                    lower.wait(&reader),
                    lower.read(&reader),
                ];
            }
        }
        if lower.problems.is_empty() {
            Ok(lowered)
        } else {
            Err(lower.problems)
        }
    }
}
