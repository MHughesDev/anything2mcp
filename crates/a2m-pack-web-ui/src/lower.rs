//! The lowering of the `ui` mode: one binding into `goto`, `wait`, `read`,
//! `set`, and `invoke` steps, each naming its screen in the pack's section
//! of the plan and carrying this pack's own payload.
//!
//! | Verb | Act | Confirm | Read back |
//! | --- | --- | --- | --- |
//! | `read`, `list` | `goto` in an observing tab, `wait`, `read` | | |
//! | the write verbs | `goto` in an acting tab, `wait`, one `set` for each field, `invoke`, marked as the commit point | | `goto` in an observing tab, `wait`, `read` on the screen `read_back` names |
//!
//! There is no separate `locate` step: each `read`, `set`, and `invoke`
//! finds its own element and checks its fingerprint, because an element is
//! good for one page generation only. The committing `invoke` is not done
//! until the page shows the action's `confirm`, so the confirm list is
//! empty (decision 0112).

use a2m_core::{
    EffectClass, FailsAs, FileKindName, IdentityStrategy, Mode, PackPayload, PackRef, SectionIndex,
    Step, StepArg, StepKind, SurfaceKind, SurfaceName, ValueType, Verb,
};
use a2m_files::{
    Diagnostics, Doc, JsonSchema, Located, LowerContext, Lowered, Lowering, Place, Possible,
    ResolvedBinding, Stage, Statement, StepChanges, StepCommits,
};

use crate::codes::{self, shown};
use crate::model::{
    GotoStep, InvokeStep, PAYLOAD_FORMAT, ReadItem, ReadStep, Screen, ScreenSection, SetStep,
    TabKind, WaitStep,
};
use crate::screens::{SCREEN_KIND, WEB_SURFACE, problem};

/// The step that sends the tab to a screen's address.
pub const GOTO: StepKind = StepKind::fixed("goto");
/// The step that waits until a screen can be used.
pub const WAIT: StepKind = StepKind::fixed("wait");
/// The step that reads fields, or the rows of a table.
pub const READ: StepKind = StepKind::fixed("read");
/// The step that sets one field through its control's rule.
pub const SET: StepKind = StepKind::fixed("set");
/// The step that performs an action and waits for what the page shows.
pub const INVOKE: StepKind = StepKind::fixed("invoke");

const READ_VERBS: &[&str] = &["read", "list"];
const WRITE_VERBS: &[&str] = &["create", "update", "upsert", "submit"];
/// The verbs whose write cannot be taken back.
const IRREVERSIBLE_VERBS: &[&str] = &["submit"];

/// How long a step may take when the target states no timeout under the
/// step kind's name.
const DEFAULT_TIMEOUT_MS: u32 = 30_000;

/// The name of the argument of a `set` step.
pub(crate) const VALUE_ARG: &str = "value";
/// What the name of a grid field's row key argument starts with, followed by
/// the field's name.
pub(crate) const ROW_ARG: &str = "row.";
/// What the name of a `goto` step's argument starts with.
pub(crate) const REACH_ARG: &str = "reach.";

/// The lowering of the `ui` mode.
pub(crate) struct UiLowering {
    mode: Mode,
    schema: JsonSchema,
    verbs: Vec<Verb>,
    step_kinds: Vec<StepKind>,
}

impl UiLowering {
    pub(crate) fn new(mode: Mode) -> Self {
        Self {
            mode,
            schema: JsonSchema::object()
                .required("screen", JsonSchema::name())
                .optional("fields", JsonSchema::map_of(JsonSchema::expression()))
                .optional("rows", JsonSchema::map_of(JsonSchema::expression()))
                .optional("action", JsonSchema::name())
                .optional(
                    "read_back",
                    JsonSchema::object()
                        .required("screen", JsonSchema::name())
                        .optional("rows", JsonSchema::map_of(JsonSchema::expression())),
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

/// One screen as a lowering uses it.
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
        let named = part.get("screen")?;
        let name = named.as_text()?;
        let found = self.section.as_ref().and_then(|(index, section)| {
            section.get(name).map(|screen| Found {
                name,
                at: &named.at,
                section: *index,
                screen: screen.clone(),
            })
        });
        if found.is_none() {
            self.refuse(
                "unknown_screen",
                Stage::Resolve,
                &named.at,
                format!(
                    "No file under `screens/` defines a screen named `{name}` on a surface of the kind `web`."
                ),
                "Name a screen that `screens/` defines on a `web` surface.",
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
            idempotent: false,
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
                    "Report this to the web pack's author.",
                );
                PackPayload::none()
            }
        }
    }

    /// The `goto` step: a value for every placeholder of the screen's
    /// address, each the operation's input of the same name, and the tab
    /// it is reached in: one that cannot write, unless the step is of a
    /// write before its read-back (decision 0195).
    fn goto(&mut self, found: &Found<'_>, tab: TabKind) -> Step {
        let mut args = Vec::new();
        for hole in found.screen.holes() {
            if self.cx.operation().input(hole).is_none() {
                self.refuse(
                    "placeholder_unbound",
                    Stage::Resolve,
                    found.at,
                    format!(
                        "The placeholder `{hole}` of the screen `{}` matches no input of the operation.",
                        found.name
                    ),
                    "Add an input of that name.",
                );
                continue;
            }
            match self.cx.expr_at(&format!("input.{hole}"), found.at, None) {
                Ok(index) => {
                    let fits = self.cx.expr_type(index).is_some_and(|ty| {
                        matches!(ty, ValueType::Text | ValueType::Int | ValueType::Choice)
                    });
                    if !fits {
                        self.refuse(
                            "position_type",
                            Stage::TypeCheck,
                            found.at,
                            format!("The input `{hole}` fills a segment of the screen's address and is not text or a whole number."),
                            "Give the input a field type that is one segment of an address.",
                        );
                    }
                    args.push(StepArg {
                        name: format!("{REACH_ARG}{hole}"),
                        value: index,
                    });
                }
                Err(wrong) => self.problems.push(wrong),
            }
        }
        let mut step = self.step(GOTO, found, FailsAs::from(&codes::row::UNEXPECTED_PAGE));
        step.args = args;
        step.payload = self.encoded(&GotoStep { tab }, found.at);
        step
    }

    fn wait(&mut self, found: &Found<'_>, snapshot: bool) -> Step {
        let mut step = self.step(WAIT, found, FailsAs::from(&codes::row::UNEXPECTED_PAGE));
        step.payload = self.encoded(&WaitStep { snapshot }, found.at);
        step
    }

    /// The `read` step: one value for each output of the operation, from
    /// the screen's field of the same name, or, for a list, from the column
    /// of that name of the screen's table.
    fn read(&mut self, found: &Found<'_>, binding: &Located<Doc>) -> Step {
        let mut payload = ReadStep::default();
        let mut args = Vec::new();
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
                let (what, fix) = if listed {
                    (
                        "column of its table",
                        "Add the column under the screen's `rows`, or rename the output.",
                    )
                } else {
                    (
                        "field",
                        "Add the field to the screen, or rename the output.",
                    )
                };
                self.refuse(
                    "unknown_field",
                    Stage::Resolve,
                    found.at,
                    format!(
                        "The output `{}` matches no {what} on the screen `{}`.",
                        output.name, found.name
                    ),
                    fix,
                );
                continue;
            }
            let item = ReadItem {
                output: output.name.clone(),
                from: output.name.clone(),
            };
            if !listed
                && found
                    .screen
                    .fields
                    .get(&output.name)
                    .is_some_and(|field| field.control == "grid")
            {
                match self.row_key(&output.name, found, binding) {
                    Some(arg) => args.push(arg),
                    None => continue,
                }
            }
            if listed {
                payload.columns.push(item);
            } else {
                payload.fields.push(item);
            }
        }
        let mut step = self.step(READ, found, FailsAs::from(&codes::row::LOCATOR_NOT_FOUND));
        step.args = args;
        step.payload = self.encoded(&payload, found.at);
        step
    }

    /// The argument that carries the key of the row a grid field is read
    /// or set in: the expression the binding's `rows` gives under the
    /// field's name, which must be text or a whole number.
    fn row_key(
        &mut self,
        name: &str,
        found: &Found<'_>,
        binding: &Located<Doc>,
    ) -> Option<StepArg> {
        let Some(entry) = binding
            .get("rows")
            .map(Located::entries)
            .unwrap_or_default()
            .iter()
            .find(|entry| entry.key.value == name)
        else {
            self.refuse(
                "row_missing",
                Stage::Resolve,
                &binding.at,
                format!(
                    "The field `{name}` of the screen `{}` is a cell of a grid, and the binding does not say which row.",
                    found.name
                ),
                "Add `rows:` with the field's name and an expression for the key of the row, which is the text its first cell shows.",
            );
            return None;
        };
        let index = match self.cx.expr(&entry.value, None) {
            Ok(index) => index,
            Err(wrong) => {
                self.problems.push(wrong);
                return None;
            }
        };
        let fits = self
            .cx
            .expr_type(index)
            .is_some_and(|ty| matches!(ty, ValueType::Text | ValueType::Choice | ValueType::Int));
        if !fits {
            self.refuse(
                "value_type",
                Stage::TypeCheck,
                &entry.value.at,
                format!("The key of the row for the field `{name}` is not text, a code, or a whole number."),
                "Give the key a field type that is one of those.",
            );
            return None;
        }
        Some(StepArg {
            name: format!("{ROW_ARG}{name}"),
            value: index,
        })
    }

    /// A `rows` entry that names a field that is not a grid cell, or none.
    fn stray_rows(&mut self, found: &Found<'_>, binding: &Located<Doc>) {
        for entry in binding
            .get("rows")
            .map(Located::entries)
            .unwrap_or_default()
        {
            let grid = found
                .screen
                .fields
                .get(&entry.key.value)
                .is_some_and(|field| field.control == "grid");
            if !grid {
                self.refuse(
                    "rows_not_fitting",
                    Stage::Resolve,
                    &entry.key.at,
                    format!(
                        "`rows` names `{}`, which is not a `grid` field of the screen `{}`.",
                        shown(&entry.key.value),
                        found.name
                    ),
                    "Name only the grid cells a binding sets or reads.",
                );
            }
        }
    }

    /// One `set` step for each entry of the binding's `fields`, in the
    /// order they are written.
    fn sets(&mut self, found: &Found<'_>, binding: &Located<Doc>) -> Vec<Step> {
        let mut steps = Vec::new();
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
            let index = match self.cx.expr(&entry.value, None) {
                Ok(index) => index,
                Err(wrong) => {
                    self.problems.push(wrong);
                    continue;
                }
            };
            let fits = self.cx.expr_type(index).is_some_and(|ty| {
                if field.control == "checkbox" || field.control == "toggle" {
                    ty == ValueType::Bool
                } else {
                    matches!(
                        ty,
                        ValueType::Text | ValueType::Choice | ValueType::Int | ValueType::Decimal
                    )
                }
            });
            if !fits {
                self.refuse(
                    "value_type",
                    Stage::TypeCheck,
                    &entry.value.at,
                    format!(
                        "The value for the field `{name}` cannot be given to a `{}` control: a `checkbox` and a `toggle` take a truth value, and every other control takes text, a code, a whole number, or an exact number.",
                        shown(&field.control)
                    ),
                    "Give a value of such a type. Formatting a date or a quantity for a control is not built yet.",
                );
            }
            let mut step = self.step(SET, found, FailsAs::from(&codes::row::LOCATOR_NOT_FOUND));
            step.args = vec![StepArg {
                name: VALUE_ARG.to_owned(),
                value: index,
            }];
            if field.control == "grid" {
                match self.row_key(name, found, binding) {
                    Some(arg) => step.args.push(arg),
                    None => continue,
                }
            }
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
                "A binding that writes through the interface names the `action` that saves."
                    .to_owned(),
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
                format!("The screen `{}` has no action named `{name}`.", found.name),
                "Add the action to the screen, or correct its name.",
            );
            return None;
        };
        let mut step = self.step(INVOKE, found, FailsAs::from(&codes::row::LOCATOR_NOT_FOUND));
        step.commits = action.commits;
        step.payload = self.encoded(
            &InvokeStep {
                action: name.to_owned(),
            },
            &named.at,
        );
        Some(step)
    }
}

/// What a binding of the `ui` or the `page_http` mode states for the
/// review: it sends only to its surface and its operation's declared
/// destinations, starts nothing, and sets nothing (decision 0292).
pub(crate) fn web_statement() -> Statement {
    use a2m_files::{BindingStatement, Sends};
    Statement::Stated(BindingStatement {
        sends: Sends::Only {
            surface: true,
            addresses: Vec::new(),
        },
        ..BindingStatement::default()
    })
}

impl Lowering for UiLowering {
    fn mode(&self) -> Mode {
        self.mode.clone()
    }

    /// Where the tool sends, which the browser holds it to (decision 0292):
    /// its surface, and the destinations its operation declares, which the
    /// review adds as it does for every pack. A tab's own document and
    /// every look and act are on the surface's origin alone; every other
    /// request of its page, its frames, and its workers to an origin on
    /// neither list is failed in the browser before it leaves
    /// (`a2m-pack-web-browser`, `Page::judge_request`). It starts no
    /// program and sets no variable: the browser is the engine's own, held
    /// to the digests the person reviewed, and the scripts a page brings
    /// run inside it, from the reach stated.
    fn statement(
        &self,
        _binding: &ResolvedBinding<'_>,
        _lowered: &Lowered,
        _cx: &LowerContext<'_>,
    ) -> Statement {
        web_statement()
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

    /// A `goto`, `wait`, or `read` only looks at a page. A `set` types into
    /// one and an `invoke` presses one of its actions, and either can change
    /// the system whatever the screen says, so a read holds neither.
    fn changes(&self, kind: &StepKind) -> StepChanges {
        if *kind == SET || *kind == INVOKE {
            StepChanges::Can
        } else {
            StepChanges::Never
        }
    }

    /// The HTTP pack's rule, for one meaning of a verb across the modes
    /// (decision 0144): a verb that reads is a read; `submit` cannot be
    /// taken back; any other write is a write when the binding names how
    /// the record is read back, and `irreversible` when it names nothing,
    /// since what was written could never be found again.
    fn least_effect(&self, binding: &ResolvedBinding<'_>) -> EffectClass {
        let verb = binding.verb.as_str();
        if READ_VERBS.contains(&verb) {
            EffectClass::Read
        } else if IRREVERSIBLE_VERBS.contains(&verb) || binding.field("read_back").is_none() {
            EffectClass::Irreversible
        } else {
            EffectClass::Write
        }
    }

    fn binding_schema(&self) -> &JsonSchema {
        &self.schema
    }

    fn identity(&self, binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible {
        match strategy {
            IdentityStrategy::None => Possible::Yes,
            IdentityStrategy::Natural | IdentityStrategy::Composite => {
                let reads = READ_VERBS.contains(&binding.verb.as_str());
                if reads || binding.field("read_back").is_some() {
                    Possible::Yes
                } else {
                    Possible::No(
                        "the binding names no `read_back` screen that is reached from the record's key."
                            .to_owned(),
                    )
                }
            }
            IdentityStrategy::Stamped | IdentityStrategy::Find => Possible::No(
                "finding a record by a stamp or a search is not built in this version of the web pack."
                    .to_owned(),
            ),
        }
    }

    fn lower(
        &self,
        binding: &ResolvedBinding<'_>,
        cx: &LowerContext<'_>,
    ) -> Result<Lowered, Diagnostics> {
        let mut lower = Lower {
            cx,
            section: None,
            problems: Diagnostics::new(),
        };
        if let Some((index, section)) = cx.section(&SCREEN_KIND, &WEB_SURFACE) {
            let decoded = if section.format == PAYLOAD_FORMAT {
                section.payload.decode::<ScreenSection>()
            } else {
                Err(codes::row::STEP_NOT_USABLE.failure(
                    "The screens were compiled in another format.",
                    "Compile the target again.",
                ))
            };
            match decoded {
                Ok(decoded) => lower.section = Some((index, decoded)),
                Err(failure) => lower.refuse(
                    "not_encodable",
                    Stage::Lower,
                    &binding.doc.at,
                    failure.message().to_owned(),
                    "Report this to the web pack's author.",
                ),
            }
        }
        let reads = READ_VERBS.contains(&binding.verb.as_str());
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
            for key in ["fields", "action", "read_back"] {
                if let Some(stray) = binding.field(key) {
                    lower.refuse(
                        "read_acts",
                        Stage::Analyze,
                        &stray.at,
                        format!(
                            "The verb `{}` reads, and the binding states `{key}`, which belongs to a write.",
                            binding.verb
                        ),
                        "Remove it, or bind a verb that writes.",
                    );
                }
            }
            // A read of one record may find none, when the screen declares
            // how that looks; a list answers with the rows there are.
            let lookup = binding.verb.as_str() == "read" && found.screen.can_be_absent();
            lower.stray_rows(&found, binding.doc);
            let mut steps = vec![
                lower.goto(&found, TabKind::Observe),
                lower.wait(&found, false),
                lower.read(&found, binding.doc),
            ];
            for step in &mut steps {
                step.lookup = lookup;
            }
            lowered.act = steps;
        } else {
            lower.stray_rows(&found, binding.doc);
            let mut act = vec![lower.goto(&found, TabKind::Act), lower.wait(&found, true)];
            act.extend(lower.sets(&found, binding.doc));
            act.extend(lower.invoke(&found, binding.doc));
            lowered.act = act;
            if let Some(read_back) = binding.field("read_back")
                && let Some(reader) = lower.screen(read_back)
            {
                if !reader.screen.can_be_absent() {
                    lower.refuse(
                        "read_back_never_absent",
                        Stage::Analyze,
                        reader.at,
                        format!(
                            "The record is read back through the screen `{}`, which declares no variant for a record that does not exist.",
                            reader.name
                        ),
                        "Add a variant with `handling: absent` to the screen, with what the page shows then.",
                    );
                }
                // The read-back is a new visit, in a tab that cannot
                // write: it, and the preview and the freshness check that
                // run it on their own, are reads.
                lower.stray_rows(&reader, read_back);
                let mut steps = vec![
                    lower.goto(&reader, TabKind::Observe),
                    lower.wait(&reader, false),
                    lower.read(&reader, read_back),
                ];
                for step in &mut steps {
                    step.lookup = true;
                }
                lowered.read_back = steps;
            }
        }
        if lower.problems.is_empty() {
            Ok(lowered)
        } else {
            Err(lower.problems)
        }
    }
}
