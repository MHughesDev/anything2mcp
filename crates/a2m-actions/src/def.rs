//! What an action is: one definition, from which every face is generated.

use a2m_core::{EffectClass, Failure};
use a2m_files::JsonSchema;
use a2m_policy::Role;

use crate::dispatch::{Call, Output};
use crate::kernel::Kernel;

a2m_core::checked_name! {
    /// The name of an action: a noun and a verb joined by one dot, such as
    /// `target.probe`. The noun is a word of the command line; for an action
    /// a provider adds, it is the provider's name.
    pub struct ActionName: "action", 64, dotted;
}

impl ActionName {
    /// The noun: the part before the dot.
    #[must_use]
    pub fn noun(&self) -> &str {
        self.as_str().split_once('.').map_or("", |(noun, _)| noun)
    }

    /// The verb: the part after the dot.
    #[must_use]
    pub fn verb(&self) -> &str {
        self.as_str().split_once('.').map_or("", |(_, verb)| verb)
    }
}

/// The boxed future a handler returns.
pub type BoxFuture<'a, T> = a2m_auth::BoxFuture<'a, T>;

/// The kernel's own function for an action.
pub type HandlerFn = for<'a> fn(&'a Kernel, Call) -> BoxFuture<'a, Result<Output, Failure>>;

/// What an action is, in one word. An action never mixes two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shape {
    /// Reports facts about one identified thing.
    Inspect,
    /// Lists or searches.
    List,
    /// Follows what happened in a run.
    Trace,
    /// Compares two things.
    Compare,
    /// Observes or captures a system.
    Observe,
    /// Changes something.
    Change,
}

impl Shape {
    /// The word as documents write it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::List => "list",
            Self::Trace => "trace",
            Self::Compare => "compare",
            Self::Observe => "observe",
            Self::Change => "change",
        }
    }
}

/// The effect of an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Effect {
    /// Always this effect class.
    Fixed(EffectClass),
    /// The effect of the published tool called. `tool.call` only.
    OfTool,
}

/// Who may use an action: the "Who" column of the action catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Who {
    /// The lowest role that may use it.
    Role(Role),
    /// A viewer for a read, an operator for anything else. `tool.call` only.
    ByEffect,
    /// The person at this machine. On a server, an administrator.
    LocalUser,
    /// Anyone who can call at all.
    Any,
    /// The person the act belongs to, about their own delegations.
    Person,
}

impl Who {
    /// The role policy is asked about, for an action of `effect`.
    #[must_use]
    pub const fn role(self, effect: EffectClass) -> Role {
        match self {
            Self::Role(role) => role,
            Self::ByEffect => match effect {
                EffectClass::Read => Role::Viewer,
                _ => Role::Operator,
            },
            Self::LocalUser => Role::Admin,
            Self::Any | Self::Person => Role::Viewer,
        }
    }

    /// The words of the catalog's column.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Role(Role::Viewer) => "Viewer",
            Self::Role(Role::Operator) => "Operator",
            Self::Role(Role::Approver) => "Approver",
            Self::Role(Role::Builder) => "Builder",
            Self::Role(Role::Publisher) => "Publisher",
            Self::Role(Role::Auditor) => "Auditor",
            Self::Role(Role::Admin) => "Admin",
            Self::ByEffect => "Viewer or Operator, by effect",
            Self::LocalUser => "Local user",
            Self::Any => "Any",
            Self::Person => "The person",
        }
    }
}

/// Whether an action's MCP tool appears in a tool list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Listed {
    /// Reached through `action.run` only.
    No,
    /// In the `use` toolset.
    Use,
    /// In the `build` toolset.
    Build,
    /// In both.
    UseAndBuild,
}

impl Listed {
    /// The words of the catalog's column.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::No => "No",
            Self::Use => "use",
            Self::Build => "build",
            Self::UseAndBuild => "use and build",
        }
    }

    /// Whether the action is marked for `toolset`.
    #[must_use]
    pub const fn in_toolset(self, toolset: BuiltInToolset) -> bool {
        matches!(
            (self, toolset),
            (Self::Use | Self::UseAndBuild, BuiltInToolset::Use)
                | (Self::Build | Self::UseAndBuild, BuiltInToolset::Build)
        )
    }
}

/// The two toolsets every installation has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltInToolset {
    /// For calling finished tools.
    Use,
    /// For building tools.
    Build,
}

impl BuiltInToolset {
    /// The most tools the toolset lists up front (spec section 220 and
    /// section 221). The rest are reached through `action.run`.
    #[must_use]
    pub const fn cap(self) -> usize {
        match self {
            Self::Use => 12,
            Self::Build => 15,
        }
    }

    /// The toolset's name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Use => "use",
            Self::Build => "build",
        }
    }
}

/// Which health state an action can run in (spec section 224).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HealthNeed {
    /// Runs in every state, `stopped` included: it reports on the engine.
    Nothing,
    /// Needs the store and policy; runs when the engine is degraded.
    Reads,
    /// Needs everything; runs only when the engine is healthy.
    Writes,
}

/// Whether the act is a person's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PersonNeed {
    /// Anyone the role allows.
    None,
    /// Proof that a person is present, from the role's person surface.
    Presence,
    /// Proof of presence only when the input loosens something protected;
    /// the handler asks through the kernel when it finds that it does.
    PresenceWhenLoosening,
}

/// Where an action is carried out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunsIn {
    /// In the engine.
    Engine,
    /// By the client end, because it acts on the engine's process.
    Client,
}

/// The faces an action has. Today every action has all four (the open point
/// of spec section 220 about the `engine.*` actions is not settled).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Faces {
    /// `anything2mcp <noun> <verb>`.
    pub command: bool,
    /// `a2m_<noun>_<verb>`.
    pub mcp: bool,
    /// `POST /api/v1/actions/<noun>.<verb>`.
    pub rest: bool,
    /// `client.<noun>.<verb>()`.
    pub sdk: bool,
}

impl Faces {
    /// All four.
    pub const ALL: Self = Self {
        command: true,
        mcp: true,
        rest: true,
        sdk: true,
    };
}

/// A resource-shaped route generated beside `/actions/{name}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RestAlias {
    /// `GET` or `POST`.
    pub method: &'static str,
    /// The path under `/api/v1`, with `{field}` for an input field.
    pub path: &'static str,
}

/// The type of one input field. Inputs are flat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    /// Text.
    Text,
    /// A whole number within a range.
    Int {
        /// The least it may be.
        min: i64,
        /// The most it may be.
        max: i64,
    },
    /// True or false.
    Bool,
    /// One of a closed list of words.
    Word(&'static [&'static str]),
    /// A list of texts.
    TextList,
    /// A map of named values, such as a tool's own input.
    Map,
    /// A list of maps of named values, such as the rows of a source. The
    /// command face takes it as JSON.
    MapList,
    /// Whatever the schema says; for a provider's action, whose schema is
    /// its own.
    Any,
}

impl FieldType {
    fn schema(&self) -> JsonSchema {
        match self {
            Self::Text => JsonSchema::text(),
            Self::Int { min, max } => JsonSchema::int_between(*min, *max),
            Self::Bool => JsonSchema::bool(),
            Self::Word(words) => JsonSchema::words(words),
            Self::TextList => JsonSchema::list(JsonSchema::text()),
            Self::Map => JsonSchema::map_of(JsonSchema::any()),
            Self::MapList => JsonSchema::list(JsonSchema::map_of(JsonSchema::any())),
            Self::Any => JsonSchema::any(),
        }
    }

    /// The type in a word, for help text.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Int { .. } => "number",
            Self::Bool => "flag",
            Self::Word(_) => "word",
            Self::TextList => "list",
            Self::Map => "map",
            Self::MapList | Self::Any => "value",
        }
    }
}

/// One input field: an argument of the command, a property of the tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// Its name.
    pub name: String,
    /// Its type.
    pub ty: FieldType,
    /// Whether it must be given.
    pub required: bool,
    /// Whether the command face takes it without a flag. At most one field
    /// of an action.
    pub positional: bool,
    /// What it is, in a sentence.
    pub help: String,
    /// For a flag of the command line: the field that standard input
    /// fills when the flag is given. The command reads standard input to
    /// its end, removes one line ending from the end, sends the text as
    /// that field, and does not send the flag. `None` for every other
    /// field.
    pub stdin_for: Option<String>,
}

/// An action's input: its fields, and the schema they make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    /// The fields, in the order they are shown.
    pub fields: Vec<Field>,
    /// The strict schema: these fields and no others.
    pub schema: JsonSchema,
}

impl Input {
    /// The input of the fields given.
    #[must_use]
    pub fn of(fields: Vec<Field>) -> Self {
        let mut schema = JsonSchema::object();
        for field in &fields {
            schema = if field.required {
                schema.required(&field.name, field.ty.schema())
            } else {
                schema.optional(&field.name, field.ty.schema())
            };
        }
        Self { fields, schema }
    }

    /// The input a provider's schema describes. Its keys become fields
    /// whose type is the schema's own business.
    #[must_use]
    pub fn of_schema(schema: JsonSchema) -> Self {
        let fields = schema
            .keys()
            .into_iter()
            .map(|name| Field {
                name: name.to_owned(),
                ty: FieldType::Any,
                required: false,
                positional: false,
                help: String::new(),
                stdin_for: None,
            })
            .collect();
        Self { fields, schema }
    }

    /// The field of that name.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.name == name)
    }
}

/// How an action is carried out.
#[derive(Clone)]
pub enum Handler {
    /// The kernel's own function.
    Kernel(HandlerFn),
    /// A provider's handler, found in the provider registry by this name.
    Provider(a2m_provider::ActionName),
    /// Carried out by the client end; see `a2m-local`. Dispatched to the
    /// engine, it answers `input`/`client_action`.
    Client,
    /// In the catalog and not built yet: every call answers
    /// `input`/`action_not_built`.
    NotBuilt,
}

impl std::fmt::Debug for Handler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Kernel(_) => "Kernel",
            Self::Provider(_) => "Provider",
            Self::Client => "Client",
            Self::NotBuilt => "NotBuilt",
        })
    }
}

/// One action. Every face is generated from this and nothing else.
#[derive(Debug, Clone)]
pub struct ActionDef {
    /// `noun.verb`.
    pub name: ActionName,
    /// One line; its first 240 characters are enough to choose the action.
    pub summary: String,
    /// The strict input.
    pub input: Input,
    /// The published output.
    pub output: JsonSchema,
    /// One to three example inputs, as JSON text.
    pub examples: Vec<&'static str>,
    /// What the action is, in one word.
    pub shape: Shape,
    /// Its effect.
    pub effect: Effect,
    /// Who may use it.
    pub who: Who,
    /// Whether its MCP tool is listed.
    pub listed: Listed,
    /// Whether it returns a `run_id` at once.
    pub long_running: bool,
    /// Which health state it can run in.
    pub needs: HealthNeed,
    /// Whether the act is a person's.
    pub person: PersonNeed,
    /// Where it is carried out.
    pub runs_in: RunsIn,
    /// The faces it has.
    pub faces: Faces,
    /// A resource-shaped route beside the generated one.
    pub rest_alias: Option<RestAlias>,
    /// The phase or stage of the plan that builds it, as the catalog says.
    pub phase: &'static str,
    /// How it is carried out.
    pub handler: Handler,
}

impl ActionDef {
    /// Whether a call can be answered with anything but
    /// `input`/`action_not_built`.
    #[must_use]
    pub const fn built(&self) -> bool {
        !matches!(self.handler, Handler::NotBuilt)
    }

    /// The role policy is asked about, for an action of `effect`.
    #[must_use]
    pub const fn role(&self, effect: EffectClass) -> Role {
        self.who.role(effect)
    }

    /// The effect class when it is fixed.
    #[must_use]
    pub const fn fixed_effect(&self) -> Option<EffectClass> {
        match self.effect {
            Effect::Fixed(effect) => Some(effect),
            Effect::OfTool => None,
        }
    }
}

/// The shape of `next` in a result: the following action, the name of its
/// tool, its input, and when to call it.
pub(crate) fn next_schema() -> JsonSchema {
    JsonSchema::object()
        .required("action", JsonSchema::text())
        .required("tool", JsonSchema::text())
        .required("args", JsonSchema::map_of(JsonSchema::any()))
        .required("when", JsonSchema::text())
}

/// Builds a definition. Used by the catalog and nowhere else.
pub(crate) struct Def(ActionDef);

/// A new definition with the defaults of a read that needs the store.
pub(crate) fn act(
    name: &'static str,
    summary: &'static str,
    who: Who,
    listed: Listed,
    phase: &'static str,
    shape: Shape,
) -> Def {
    Def(ActionDef {
        name: ActionName::fixed(name),
        summary: summary.to_owned(),
        input: Input::of(Vec::new()),
        output: JsonSchema::map_of(JsonSchema::any()),
        examples: Vec::new(),
        shape,
        effect: Effect::Fixed(EffectClass::Read),
        who,
        listed,
        long_running: false,
        needs: HealthNeed::Reads,
        person: PersonNeed::None,
        runs_in: RunsIn::Engine,
        faces: Faces::ALL,
        rest_alias: None,
        phase,
        handler: Handler::NotBuilt,
    })
}

impl Def {
    fn push(
        mut self,
        name: &'static str,
        ty: FieldType,
        required: bool,
        help: &'static str,
    ) -> Self {
        let mut fields = std::mem::take(&mut self.0.input.fields);
        fields.push(Field {
            name: name.to_owned(),
            ty,
            required,
            positional: false,
            help: help.to_owned(),
            stdin_for: None,
        });
        self.0.input = Input::of(fields);
        self
    }

    /// A flag of the command line that says: read standard input and send
    /// it as `field`. Other faces have no standard input; a call that
    /// carries the flag itself is answered by the handler.
    pub(crate) fn stdin_flag(
        mut self,
        name: &'static str,
        field: &'static str,
        help: &'static str,
    ) -> Self {
        self = self.push(name, FieldType::Bool, false, help);
        if let Some(flag) = self.0.input.fields.last_mut() {
            flag.stdin_for = Some(field.to_owned());
        }
        self
    }

    /// A field that must be given.
    pub(crate) fn req(self, name: &'static str, ty: FieldType, help: &'static str) -> Self {
        self.push(name, ty, true, help)
    }

    /// A field that may be given.
    pub(crate) fn opt(self, name: &'static str, ty: FieldType, help: &'static str) -> Self {
        self.push(name, ty, false, help)
    }

    /// A required field the command face takes without a flag.
    pub(crate) fn pos(mut self, name: &'static str, ty: FieldType, help: &'static str) -> Self {
        self = self.push(name, ty, true, help);
        if let Some(field) = self.0.input.fields.last_mut() {
            field.positional = true;
        }
        self
    }

    /// An optional field the command face takes without a flag.
    pub(crate) fn opt_pos(mut self, name: &'static str, ty: FieldType, help: &'static str) -> Self {
        self = self.push(name, ty, false, help);
        if let Some(field) = self.0.input.fields.last_mut() {
            field.positional = true;
        }
        self
    }

    pub(crate) fn output(mut self, schema: JsonSchema) -> Self {
        self.0.output = schema;
        self
    }

    pub(crate) fn example(mut self, json: &'static str) -> Self {
        self.0.examples.push(json);
        self
    }

    /// A change: it writes the engine's own state and needs the healthy
    /// state.
    pub(crate) fn writes(mut self) -> Self {
        self.0.effect = Effect::Fixed(EffectClass::Write);
        self.0.needs = HealthNeed::Writes;
        self
    }

    pub(crate) fn effect(mut self, effect: Effect) -> Self {
        self.0.effect = effect;
        self
    }

    pub(crate) fn needs(mut self, needs: HealthNeed) -> Self {
        self.0.needs = needs;
        self
    }

    pub(crate) fn long_running(mut self) -> Self {
        self.0.long_running = true;
        self
    }

    pub(crate) fn person(mut self, person: PersonNeed) -> Self {
        self.0.person = person;
        self
    }

    pub(crate) fn client(mut self) -> Self {
        self.0.runs_in = RunsIn::Client;
        self.0.needs = HealthNeed::Nothing;
        self.0.handler = Handler::Client;
        self
    }

    pub(crate) fn alias(mut self, method: &'static str, path: &'static str) -> Self {
        self.0.rest_alias = Some(RestAlias { method, path });
        self
    }

    pub(crate) fn done(mut self) -> ActionDef {
        // Every result may name the most useful following call (spec
        // section 221). An output that is a map of anything admits it
        // already; a strict object is given the key here.
        self.0.output = self.0.output.optional("next", next_schema());
        self.0
    }
}
