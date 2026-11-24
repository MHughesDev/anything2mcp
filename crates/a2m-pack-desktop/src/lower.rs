//! The lowering of the `desktop` mode: one binding into `goto`, `wait`,
//! `read`, `set`, and `invoke` steps, each naming its screen in the pack's
//! section of the compiled target and carrying this pack's own payload.
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
    EffectClass, FailsAs, FileKindName, IdentityStrategy, Mode, PackPayload, PackRef, SectionIndex,
    Step, StepArg, StepKind, SurfaceKind, SurfaceName, ValueType, Verb,
};
use a2m_files::{
    BindingStatement, Diagnostics, Doc, JsonSchema, Located, LowerContext, Lowered, Lowering,
    Place, Possible, ResolvedBinding, Sends, Stage, Statement, StepChanges, StepCommits,
};

use crate::codes::{self, shown};
use crate::model::{
    InvokeStep, MemberStep, PAYLOAD_FORMAT, PropertyStep, ReadItem, ReadStep, Screen,
    ScreenSection, SetStep,
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
/// The step a discoverer sends: what the surface's service says of itself,
/// asked so that a service that is not running is never started.
pub const INTROSPECT: StepKind = StepKind::fixed("introspect");
/// The step that reads one property of the surface's service.
pub const PROPERTY: StepKind = StepKind::fixed("property");
/// The step a discoverer sends: the definition of the management class the
/// surface names (Windows).
pub const SCHEMA: StepKind = StepKind::fixed("schema");
/// What a drafted read of a management class's property lowers to. This
/// version carries out no such step (decision 0318).
pub const MANAGEMENT_GET: StepKind = StepKind::fixed("management_get");
/// What a drafted read of a COM property get lowers to. This version
/// carries out no such step (decision 0318).
pub const COM_GET: StepKind = StepKind::fixed("com_get");

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
                .optional("screen", JsonSchema::name())
                .optional(
                    "management",
                    JsonSchema::object()
                        .required("surface", JsonSchema::name())
                        .required("class", JsonSchema::text())
                        .required("name", JsonSchema::text()),
                )
                .optional(
                    "com",
                    JsonSchema::object()
                        .required("surface", JsonSchema::name())
                        .required("interface", JsonSchema::text())
                        .required("name", JsonSchema::text()),
                )
                .optional(
                    "property",
                    JsonSchema::object()
                        .required("surface", JsonSchema::name())
                        .required("path", JsonSchema::text())
                        .required("interface", JsonSchema::text())
                        .required("name", JsonSchema::text()),
                )
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
            step_kinds: vec![
                GOTO,
                WAIT,
                READ,
                SET,
                INVOKE,
                PROPERTY,
                MANAGEMENT_GET,
                COM_GET,
            ],
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
        FailsAs::from(&codes::row::UNEXPECTED_PAGE)
    }

    fn not_found() -> FailsAs {
        FailsAs::from(&codes::row::LOCATOR_NOT_FOUND)
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
            let read_back = self
                .cx
                .operation()
                .outputs()
                .iter()
                .any(|output| output.name == *name && !matches!(output.ty, ValueType::List(_)));
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

    /// A binding that reads one property of the surface's service: one
    /// `property` step, and nothing that could call a method of the
    /// service (decision 0316). Only the verb `read`, and only an operation
    /// declared to read.
    fn property(
        &mut self,
        binding: &ResolvedBinding<'_>,
        property: &Located<Doc>,
        verb: &str,
    ) -> Result<Lowered, Diagnostics> {
        let at = &binding.doc.at;
        let text = |key: &str| {
            property
                .get(key)
                .and_then(Located::as_text)
                .unwrap_or_default()
                .to_owned()
        };
        if verb != "read" {
            self.refuse(
                "property_reads_only",
                Stage::Analyze,
                at,
                format!(
                    "The verb `{}` is not built for a binding that names a property of a service: a property is read, and setting one is not built.",
                    shown(verb)
                ),
                "Bind the verb `read`.",
            );
        }
        if self.cx.operation().effect() != EffectClass::Read {
            self.refuse(
                "read_acts",
                Stage::Analyze,
                at,
                "A binding that reads a property of a service is a read, and the operation is declared to change something.".to_owned(),
                "Declare `effect: read`.",
            );
        }
        for key in ["screen", "fields", "action", "read_back"] {
            if let Some(stray) = binding.field(key) {
                self.refuse(
                    "read_acts",
                    Stage::Analyze,
                    &stray.at,
                    format!(
                        "A binding that reads a property of a service states `{key}`, which belongs to a window."
                    ),
                    "Remove it.",
                );
            }
        }
        let (path, interface, name) = (text("path"), text("interface"), text("name"));
        if !crate::dbus::is_object_path(&path)
            || interface.is_empty()
            || !interface
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
            || name.is_empty()
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            self.refuse(
                "property_invalid",
                Stage::Resolve,
                &property.at,
                "The binding's object path, interface, or property name is not one a bus accepts."
                    .to_owned(),
                "Write the path, interface, and name as the service's introspection gives them.",
            );
        }
        let outputs = self.cx.operation().outputs();
        let [output] = outputs else {
            self.refuse(
                "property_output",
                Stage::Analyze,
                at,
                "A read of one property has exactly one output, which is the property's value as text.".to_owned(),
                "Give the operation one output, of the type `untrusted_text`.",
            );
            return Err(std::mem::take(&mut self.problems));
        };
        let surface = SurfaceName::new(&text("surface")).map_err(|_| {
            Diagnostics::from(problem(
                "unknown_screen",
                Stage::Resolve,
                &property.at,
                "The binding's surface has no well-formed name.",
                "Name a surface the target declares.",
            ))
        })?;
        let payload = PropertyStep {
            path,
            interface,
            name,
            output: output.name.clone(),
        };
        let step = Step {
            kind: PROPERTY,
            refers_to: None,
            args: Vec::new(),
            timeout_ms: self
                .cx
                .target()
                .default_timeout_ms(PROPERTY.as_str())
                .unwrap_or(DEFAULT_TIMEOUT_MS),
            fails_as: Self::not_found(),
            commits: false,
            lookup: false,
            idempotent: true,
            when: None,
            payload: self.encoded(&payload, &property.at),
        };
        if !self.problems.is_empty() {
            return Err(std::mem::take(&mut self.problems));
        }
        Ok(Lowered {
            surface,
            act: vec![step],
            confirm: Vec::new(),
            read_back: Vec::new(),
            lookup: Vec::new(),
            settle_ms: 0,
            prefer_response: false,
        })
    }

    /// A binding that reads one member of a management class (`management`)
    /// or of a COM interface (`com`): one step, which this version of the
    /// pack does not carry out, and nothing that could change anything
    /// (decision 0318). Only the verb `read`, and only an operation declared
    /// to read.
    fn member(
        &mut self,
        binding: &ResolvedBinding<'_>,
        member: &Located<Doc>,
        verb: &str,
        key: &'static str,
        kind: StepKind,
    ) -> Result<Lowered, Diagnostics> {
        let at = &binding.doc.at;
        let text = |name: &str| {
            member
                .get(name)
                .and_then(Located::as_text)
                .unwrap_or_default()
                .to_owned()
        };
        if verb != "read" {
            self.refuse(
                "property_reads_only",
                Stage::Analyze,
                at,
                format!(
                    "The verb `{}` is not built for a binding that names a member of a {}: a member is read, and setting or calling one is not built.",
                    shown(verb),
                    if key == "com" { "COM interface" } else { "management class" }
                ),
                "Bind the verb `read`.",
            );
        }
        if self.cx.operation().effect() != EffectClass::Read {
            self.refuse(
                "read_acts",
                Stage::Analyze,
                at,
                "A binding that reads a member is a read, and the operation is declared to change something.".to_owned(),
                "Declare `effect: read`.",
            );
        }
        for other in [
            "screen",
            "fields",
            "action",
            "read_back",
            "property",
            "management",
            "com",
        ] {
            if other != key
                && let Some(stray) = binding.field(other)
            {
                self.refuse(
                    "read_acts",
                    Stage::Analyze,
                    &stray.at,
                    format!(
                        "A binding that reads a member states `{other}`, which belongs to another kind of binding."
                    ),
                    "Remove it.",
                );
            }
        }
        let owner_key = if key == "com" { "interface" } else { "class" };
        let (owner, name) = (text(owner_key), text("name"));
        let plain = |word: &str, dots: bool| {
            !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || (dots && c == '.'))
        };
        if !plain(&owner, key == "com") || !plain(&name, false) {
            self.refuse(
                "property_invalid",
                Stage::Resolve,
                &member.at,
                format!(
                    "The binding's {owner_key} or member name is not one a definition can hold."
                ),
                "Write them as the definition gives them.",
            );
        }
        let outputs = self.cx.operation().outputs();
        let [output] = outputs else {
            self.refuse(
                "property_output",
                Stage::Analyze,
                at,
                "A read of one member has exactly one output, which is its value as text."
                    .to_owned(),
                "Give the operation one output, of the type `untrusted_text`.",
            );
            return Err(std::mem::take(&mut self.problems));
        };
        let surface = SurfaceName::new(&text("surface")).map_err(|_| {
            Diagnostics::from(problem(
                "unknown_screen",
                Stage::Resolve,
                &member.at,
                "The binding's surface has no well-formed name.",
                "Name a surface the target declares.",
            ))
        })?;
        let payload = MemberStep {
            owner,
            name,
            output: output.name.clone(),
        };
        let step = Step {
            kind: kind.clone(),
            refers_to: None,
            args: Vec::new(),
            timeout_ms: self
                .cx
                .target()
                .default_timeout_ms(kind.as_str())
                .unwrap_or(DEFAULT_TIMEOUT_MS),
            fails_as: Self::not_found(),
            commits: false,
            lookup: false,
            idempotent: true,
            when: None,
            payload: self.encoded(&payload, &member.at),
        };
        if !self.problems.is_empty() {
            return Err(std::mem::take(&mut self.problems));
        }
        Ok(Lowered {
            surface,
            act: vec![step],
            confirm: Vec::new(),
            read_back: Vec::new(),
            lookup: Vec::new(),
            settle_ms: 0,
            prefer_response: false,
        })
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

/// Whether a management namespace begins with two separators,
/// `\\host\root` or `//host/root`: another machine's. The statement is
/// withdrawn for one, and the driver refuses to ask one.
pub(crate) fn names_another_machine(namespace: &str) -> bool {
    let mut chars = namespace.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some('/' | '\\'), Some('/' | '\\'))
    )
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

    /// A `goto`, `wait`, or `read` only looks at a window. A `set` types
    /// into one and an `invoke` performs one of its actions, and either can
    /// change the system whatever the window model says, so a read holds
    /// neither.
    fn changes(&self, kind: &StepKind) -> StepChanges {
        if *kind == SET || *kind == INVOKE {
            StepChanges::Can
        } else {
            StepChanges::Never
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

    /// A binding of this pack starts no program and sets no variable: it
    /// attaches to an application that is already running. The driver sends
    /// nothing over a network: it asks the platform's own accessibility
    /// interface (the session's accessibility bus, the window system, the
    /// process table) on this machine, and the one other thing it reads, a
    /// management class's definition, it asks of this machine's management
    /// service. So it sends to no address. What the application does in
    /// answer to an action is its own, and no request the engine sends is
    /// addressed to it by a name or an address (decision 0336). The one
    /// surface field that could name another machine, a management
    /// namespace written `\\host\...` or `//host/...`, makes the statement
    /// not stated.
    fn statement(
        &self,
        _binding: &ResolvedBinding<'_>,
        lowered: &Lowered,
        cx: &LowerContext<'_>,
    ) -> Statement {
        let Some(surface) = cx.target().surface(lowered.surface.as_str()) else {
            return Statement::NotStated;
        };
        if surface
            .doc
            .text("management_namespace")
            .is_some_and(names_another_machine)
        {
            return Statement::NotStated;
        }
        Statement::Stated(BindingStatement {
            sends: Sends::Only {
                surface: false,
                addresses: Vec::new(),
            },
            ..BindingStatement::default()
        })
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
        if let Some(property) = binding.field("property") {
            return lower.property(binding, property, verb);
        }
        for (key, kind) in [("management", MANAGEMENT_GET), ("com", COM_GET)] {
            if let Some(member) = binding.field(key) {
                return lower.member(binding, member, verb, key, kind);
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
            let wait = lower.wait(&found);
            // Every act carries the record's key the `wait` proves, so that
            // it proves it again on the walk it acts on, just before it
            // acts: the window may have moved to another record since.
            let keys = wait.args.clone();
            let mut act = vec![lower.goto(&found), wait];
            for mut step in lower
                .sets(&found, binding.doc)
                .into_iter()
                .chain(lower.invoke(&found, binding.doc))
            {
                step.args.extend(keys.iter().cloned());
                act.push(step);
            }
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
