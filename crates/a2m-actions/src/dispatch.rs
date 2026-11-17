//! The dispatcher: the one path every face takes into the kernel.
//!
//! A front door authenticates its caller with [`Kernel::verify_caller`],
//! builds a [`Request`], and calls [`Kernel::dispatch`]. It decides nothing.
//! The steps, in order: find the action, validate the input against its
//! full schema, check the health state, ask policy to decide (recorded
//! whether it allows or refuses), ask for a person where the act is a
//! person's, run the handler, and shape the result, which passes the
//! redactor on its way out.

use std::path::PathBuf;

use a2m_core::{DecisionId, EffectClass, Failure, FailureClass, Hazard, Redact, Values};
use a2m_policy::{
    ActingFor, ActionRef, Context, PrincipalKind, PrincipalRef, Question, Resource, ToolsetName,
};
use serde_json::{Map, Value as Json, json};

use crate::codes;
use crate::def::{ActionDef, Effect, FieldType, Handler, HealthNeed, PersonNeed};
use crate::health::HealthState;
use crate::kernel::Kernel;
use crate::person::{PresenceAsk, PresenceProof};

/// The front door a request came through. Data for the record; nothing
/// branches on it but the presence rule of one machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrontDoor {
    /// A command.
    Command,
    /// The MCP face.
    Mcp,
    /// The REST face.
    Rest,
    /// An SDK, through the bridge.
    Sdk,
    /// The engine embedded in a program.
    Embedded,
}

impl FrontDoor {
    /// The door's name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::Mcp => "mcp",
            Self::Rest => "rest",
            Self::Sdk => "sdk",
            Self::Embedded => "embedded",
        }
    }
}

/// A caller whose credential [`Kernel::verify_caller`] checked. Only the
/// kernel makes one: a front door cannot name a principal itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub(crate) principal: PrincipalRef,
    pub(crate) workspace: a2m_core::WorkspaceId,
    pub(crate) acting_for: Option<ActingFor>,
}

impl Principal {
    /// Who it is, for policy.
    #[must_use]
    pub const fn principal(&self) -> &PrincipalRef {
        &self.principal
    }

    /// The workspace it belongs to.
    #[must_use]
    pub const fn workspace(&self) -> a2m_core::WorkspaceId {
        self.workspace
    }

    /// For whom it acts, where the credential proves it.
    #[must_use]
    pub const fn acting_for(&self) -> Option<&ActingFor> {
        self.acting_for.as_ref()
    }

    /// The person whose runs this caller may read, resume, and cancel: the
    /// person it was proved to act for, or itself when it is a person. A
    /// delegation proves nothing yet, and an agent or a program that acts
    /// for nobody owns no person's run.
    #[must_use]
    pub fn person(&self) -> Option<a2m_core::PrincipalId> {
        match &self.acting_for {
            Some(ActingFor::LocalUser(person) | ActingFor::TokenSubject(person)) => Some(*person),
            Some(ActingFor::Delegation(_)) => None,
            None => (self.principal.kind == PrincipalKind::Person).then_some(self.principal.id),
        }
    }
}

/// Who is calling, as the front door states it after the check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    /// The verified principal.
    pub principal: Principal,
    /// A label for the client, for the record. Never trusted.
    pub client: String,
    /// The front door.
    pub door: FrontDoor,
    /// The toolset the call came through.
    pub toolset: Option<ToolsetName>,
    /// Whether the caller is an AI agent, whatever principal it calls as.
    /// The MCP face always says so; a command says so unless its output is
    /// a terminal a person is at.
    pub is_agent: bool,
}

impl Caller {
    /// A caller through `door`.
    #[must_use]
    pub fn new(principal: Principal, door: FrontDoor, is_agent: bool) -> Self {
        Self {
            principal,
            client: String::new(),
            door,
            toolset: None,
            is_agent,
        }
    }
}

/// The project folder a request works in, as the mount of a front door
/// states it: the folder a local command was run in.
///
/// It is the one home of that fact for every door. Whoever mounts a door on
/// the local channel puts one on each request, as an extension beside the
/// caller, from what the client end said. The door reads it and hands it to
/// [`Request::in_project_folder`]; nothing in a request's path, query,
/// body, or tool arguments can set it. It is a statement and is not
/// trusted: the dispatcher checks it before any handler sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFolder(PathBuf);

impl ProjectFolder {
    /// The folder at `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// The folder's path, as it was stated.
    #[must_use]
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

/// What a front door hands the dispatcher.
///
/// Its debug print names the action and the fields of its input and never
/// shows a value: the input of `secret.set` holds the secret.
#[derive(Clone, PartialEq)]
pub struct Request {
    /// The action's name, as the caller wrote it.
    pub action: String,
    /// The input, as JSON. An object, or null for no input.
    pub input: Json,
    /// The caller.
    pub caller: Caller,
    /// A key that makes a repeated call the same call.
    pub idempotency_key: Option<String>,
    /// The project folder a local command was run in, as it was stated.
    /// The dispatcher checks it before any handler sees it: see
    /// [`Request::in_project_folder`].
    pub project: Option<PathBuf>,
}

/// The names of an input's fields, for a debug print that shows no value.
fn field_names(input: &Map<String, Json>) -> Vec<&str> {
    input.keys().map(String::as_str).collect()
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fields = self.input.as_object().map(field_names).unwrap_or_default();
        f.debug_struct("Request")
            .field("action", &codes::shown(&self.action))
            .field("input_fields", &fields)
            .field("door", &self.caller.door)
            .field("is_agent", &self.caller.is_agent)
            .field("project", &self.project.is_some())
            .finish_non_exhaustive()
    }
}

impl Request {
    /// A request for `action` with `input`.
    #[must_use]
    pub fn new(action: &str, input: Json, caller: Caller) -> Self {
        Self {
            action: action.to_owned(),
            input,
            caller,
            idempotency_key: None,
            project: None,
        }
    }

    /// The same request, run in a project folder.
    #[must_use]
    pub fn in_project(mut self, project: impl Into<PathBuf>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// The same request, run in the project folder its door's mount
    /// stated, when it stated one. Every door calls this with what it
    /// found in the request's extensions.
    ///
    /// The dispatcher takes the folder only on one machine, and only when
    /// it is an absolute path to a folder that exists and that neither
    /// holds nor lies inside one of the engine's own folders. A folder
    /// that fails is not used, and an action that needs a project answers
    /// `input`/`path_not_allowed` with the reason; every other action is
    /// served as if none had been stated.
    #[must_use]
    pub fn in_project_folder(mut self, project: Option<&ProjectFolder>) -> Self {
        self.project = project.map(|folder| folder.0.clone());
        self
    }
}

/// How a response ends, the same for every face: the command's exit code,
/// the status of a REST answer, whether an MCP result is an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExitClass {
    /// The action did what was asked.
    Succeeded,
    /// The action failed; the error says why.
    Failed,
    /// The request was malformed: no such action, or input that breaks the
    /// schema.
    Malformed,
    /// A person is needed: a sign-in, an approval, or presence.
    PersonNeeded,
}

impl ExitClass {
    /// The command's exit code (spec section 220).
    #[must_use]
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Succeeded => 0,
            Self::Failed => 1,
            Self::Malformed => 2,
            Self::PersonNeeded => 3,
        }
    }

    /// The word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Malformed => "malformed",
            Self::PersonNeeded => "person_needed",
        }
    }
}

/// The most useful following action, so that a caller that has read nothing
/// else can still go on.
#[derive(Debug, Clone, PartialEq)]
pub struct Next {
    /// The action to call.
    pub action: String,
    /// Its input.
    pub args: Json,
    /// When to call it, in a few words.
    pub when: String,
}

impl Next {
    /// A following action.
    #[must_use]
    pub fn new(action: &str, args: Json, when: &str) -> Self {
        Self {
            action: action.to_owned(),
            args,
            when: when.to_owned(),
        }
    }

    fn to_json(&self) -> Json {
        json!({"action": self.action, "args": self.args, "when": self.when})
    }

    /// `next` as a result carries it: the action, the name of its tool,
    /// its input, and when to call it.
    pub(crate) fn to_result_json(&self) -> Json {
        let tool = match self.action.split_once('.') {
            Some((noun, verb)) => format!("{}{noun}_{verb}", crate::faces::TOOL_PREFIX),
            None => self.action.clone(),
        };
        json!({"action": self.action, "tool": tool, "args": self.args, "when": self.when})
    }

    /// `next` for a caller whose tool list does not hold the action's own
    /// tool: the same step through `action.run`, which every toolset
    /// lists, with the action as its argument.
    pub(crate) fn through_action_run(&self) -> Json {
        json!({
            "action": "action.run",
            "tool": format!("{}action_run", crate::faces::TOOL_PREFIX),
            "args": {"action": self.action, "input": self.args},
            "when": self.when,
        })
    }
}

/// A failure as a response carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorBody {
    /// The failure class.
    pub class: FailureClass,
    /// The stable code.
    pub code: String,
    /// A plain sentence.
    pub message: String,
    /// The next step: an instruction, not a description.
    pub fix: String,
    /// Whether calling again with the same input is safe.
    pub retry_safe: bool,
}

impl ErrorBody {
    fn of(failure: &Failure, effect: EffectClass) -> Self {
        // A retry is safe when nothing was or could have been sent: every
        // class but the one that says a write may have gone through. A
        // refusal of a write before it started is as safe to repeat as a
        // read.
        let retry_safe = match failure.class() {
            FailureClass::PossibleCommit => false,
            FailureClass::Mismatch | FailureClass::Rejected => effect == EffectClass::Read,
            _ => true,
        };
        Self {
            class: failure.class(),
            code: failure.code().to_string(),
            message: failure.message().to_owned(),
            fix: failure.next_step().to_owned(),
            retry_safe,
        }
    }

    /// The error as one compact JSON object: class, code, message, fix, and
    /// whether a retry is safe.
    #[must_use]
    pub fn to_json(&self) -> Json {
        json!({
            "class": self.class.to_string(),
            "code": self.code,
            "message": self.message,
            "fix": self.fix,
            "retry_safe": self.retry_safe,
        })
    }
}

/// What every face renders, without branching on its content.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    /// Whether the action did what was asked.
    pub ok: bool,
    /// The action's name, as it was asked for.
    pub action: String,
    /// The result, redacted. Present when `ok`, and for a run that rests.
    pub result: Option<Json>,
    /// The failure. Present when not `ok`.
    pub error: Option<ErrorBody>,
    /// What the caller should know beside the result.
    pub warnings: Vec<String>,
    /// The most useful following action.
    pub next: Option<Next>,
    /// How the response ends.
    pub exit: ExitClass,
    /// The recorded policy decision, when one was made. `policy.explain`
    /// takes it.
    pub decision: Option<DecisionId>,
}

impl Response {
    /// The response as the one JSON object of the command output contract:
    /// `ok`, `action`, then `result`, `warnings`, and `next`, or `error`.
    #[must_use]
    pub fn to_json(&self) -> Json {
        let mut out = Map::new();
        out.insert("ok".to_owned(), Json::Bool(self.ok));
        out.insert("action".to_owned(), Json::String(self.action.clone()));
        if let Some(result) = &self.result {
            out.insert("result".to_owned(), result.clone());
        }
        if let Some(error) = &self.error {
            out.insert("error".to_owned(), error.to_json());
        }
        out.insert(
            "warnings".to_owned(),
            Json::Array(self.warnings.iter().cloned().map(Json::String).collect()),
        );
        if let Some(next) = &self.next {
            out.insert("next".to_owned(), next.to_json());
        }
        if let Some(decision) = &self.decision {
            out.insert("decision".to_owned(), Json::String(decision.to_string()));
        }
        Json::Object(out)
    }

    /// The command's exit code.
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        self.exit.exit_code()
    }
}

/// What a handler is given: the validated input and the checked caller.
///
/// Its debug print names the fields of the input and never shows a value.
#[derive(Clone)]
pub struct Call {
    /// The input. It passed the action's schema.
    pub input: Map<String, Json>,
    /// The caller.
    pub caller: Caller,
    /// The recorded decision that allowed the call.
    pub decision: DecisionId,
    /// The idempotency key, if the caller sent one.
    pub idempotency_key: Option<String>,
    /// The project folder the request stated, after the dispatcher's
    /// check. `None` when none was stated or the one stated was refused.
    /// A handler still reaches every file through a checked path under it.
    pub project: Option<PathBuf>,
    /// Why the stated project folder was refused, when it was.
    pub project_refused: Option<Failure>,
    /// Proof that a person is present, when the action needed it.
    pub presence: Option<PresenceProof>,
    /// The effect class and hazard the dispatcher's decision was made for.
    /// For `tool.call` these are the published tool's, as it stood when
    /// the decision was made; the handler refuses a tool that has another
    /// by the time it looks. `None` for `tool.call` of a tool that was not
    /// found.
    pub decided: Option<(EffectClass, Hazard)>,
}

impl std::fmt::Debug for Call {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Call")
            .field("input_fields", &field_names(&self.input))
            .field("door", &self.caller.door)
            .field("is_agent", &self.caller.is_agent)
            .field("decision", &self.decision)
            .field("project", &self.project.is_some())
            .field("presence", &self.presence.is_some())
            .field("decided", &self.decided)
            .finish_non_exhaustive()
    }
}

impl Call {
    /// The project folder of the call.
    ///
    /// # Errors
    ///
    /// `input`/`path_not_allowed`: the reason the stated folder was
    /// refused, or that the call states none.
    pub fn project_folder(&self) -> Result<&std::path::Path, Failure> {
        match (&self.project, &self.project_refused) {
            (Some(folder), _) => Ok(folder),
            (None, Some(refused)) => Err(refused.clone()),
            (None, None) => Err(codes::input(
                codes::PATH_NOT_ALLOWED,
                "This action works on a project folder, and the call names none.",
                "Run it inside a project folder, or pass `--project` with the folder's full path.",
            )),
        }
    }

    /// A text field, when it was given.
    #[must_use]
    pub fn text(&self, name: &str) -> Option<&str> {
        self.input.get(name).and_then(Json::as_str)
    }

    /// A text field the schema requires.
    ///
    /// # Errors
    ///
    /// `input`/`schema_invalid` when it is missing, which the dispatcher's
    /// validation prevents.
    pub fn required(&self, name: &str) -> Result<&str, Failure> {
        self.text(name).ok_or_else(|| {
            codes::input(
                codes::SCHEMA_INVALID,
                format!("The field `{name}` is missing."),
                format!("Give `{name}`."),
            )
        })
    }

    /// A whole-number field, when it was given.
    #[must_use]
    pub fn int(&self, name: &str) -> Option<i64> {
        self.input.get(name).and_then(Json::as_i64)
    }

    /// A true-or-false field, when it was given.
    #[must_use]
    pub fn flag(&self, name: &str) -> Option<bool> {
        self.input.get(name).and_then(Json::as_bool)
    }
}

/// What a handler returns.
#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    /// The result, which fits the action's output schema.
    pub result: Json,
    /// What the caller should know beside it.
    pub warnings: Vec<String>,
    /// The most useful following action.
    pub next: Option<Next>,
    /// How the response ends. `PersonNeeded` for a run that rests waiting
    /// for a person: the result still says where it stands.
    pub exit: ExitClass,
    /// The failure a run ended with, when the action started a run that
    /// failed: the response then carries both the result, which names the
    /// run, and the error.
    pub failure: Option<Failure>,
}

impl Output {
    /// A result that succeeded.
    #[must_use]
    pub const fn ok(result: Json) -> Self {
        Self {
            result,
            warnings: Vec::new(),
            next: None,
            exit: ExitClass::Succeeded,
            failure: None,
        }
    }

    /// The same, ending as failed with the failure a run ended with.
    #[must_use]
    pub fn failed(mut self, failure: Failure) -> Self {
        self.exit = ExitClass::Failed;
        self.failure = Some(failure);
        self
    }

    /// The same, naming what to call next.
    #[must_use]
    pub fn then(mut self, next: Next) -> Self {
        self.next = Some(next);
        self
    }

    /// The same, with a warning.
    #[must_use]
    pub fn warn(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// The same, ending as `exit`.
    #[must_use]
    pub const fn exit(mut self, exit: ExitClass) -> Self {
        self.exit = exit;
        self
    }
}

impl Kernel {
    /// Carries out one request. Never fails: a refusal is a response.
    pub async fn dispatch(&self, request: Request) -> Response {
        let redact = self.redact();
        // The name is echoed as it was asked for, and so passes the
        // redactor like everything else that leaves.
        let asked = redact.redact(&codes::shown(&request.action));
        let outcome = self.dispatch_inner(request).await;
        self.shaped(asked, outcome)
    }

    /// A response from what the dispatcher, or a person's act carried out
    /// on its page, came to: every string through the redactor.
    pub(crate) fn shaped(
        &self,
        asked: String,
        outcome: Result<(Output, DecisionId), Box<Refused>>,
    ) -> Response {
        let redact = self.redact();
        match outcome {
            Ok((output, decision)) => {
                let exit = output.exit;
                Response {
                    ok: exit == ExitClass::Succeeded,
                    action: asked,
                    result: Some(redact_json(redact.as_ref(), &output.result)),
                    error: output.failure.as_ref().map(|failure| {
                        ErrorBody::of(&redact.redact_failure(failure), EffectClass::Write)
                    }),
                    warnings: output
                        .warnings
                        .iter()
                        .map(|warning| redact.redact(warning))
                        .collect(),
                    next: output.next.map(|next| Next {
                        action: next.action,
                        args: redact_json(redact.as_ref(), &next.args),
                        when: next.when,
                    }),
                    exit,
                    // A stopped engine has no store to record a decision in.
                    decision: (decision != DecisionId::from_bytes([0; 16])).then_some(decision),
                }
            }
            Err(refused) => {
                let failure = redact.redact_failure(&refused.failure);
                let exit = if refused.malformed {
                    ExitClass::Malformed
                } else if person_is_needed(&failure) {
                    ExitClass::PersonNeeded
                } else {
                    ExitClass::Failed
                };
                Response {
                    ok: false,
                    action: asked,
                    result: None,
                    error: Some(ErrorBody::of(&failure, refused.effect)),
                    warnings: Vec::new(),
                    next: refused.next,
                    exit,
                    decision: refused.decision,
                }
            }
        }
    }

    async fn dispatch_inner(&self, request: Request) -> Result<(Output, DecisionId), Box<Refused>> {
        // 1. Find.
        let Some(def) = self.actions().find(&request.action).cloned() else {
            let near = self.actions().nearest(&request.action);
            let fix = if near.is_empty() {
                "Call `action.search` to list the actions.".to_owned()
            } else {
                format!("Use one of: {}.", near.join(", "))
            };
            return Err(Refused::malformed(codes::input(
                codes::SCHEMA_INVALID,
                format!(
                    "There is no action named `{}`.",
                    codes::shown(&request.action)
                ),
                fix,
            )));
        };

        // 2. Validate, against the full schema.
        let input = validate(&def, request.input).map_err(|failure| {
            Refused::malformed(failure).then(Next::new(
                "action.describe",
                json!({"action": def.name.as_str()}),
                "to read the action's schema",
            ))
        })?;

        // The idempotency key: from the request, as a door carried it, or,
        // for `tool.call`, from its own field. Two different keys on one
        // request are refused; a key is checked before anything is decided.
        let (input, idempotency_key) =
            taken_key(&def, input, request.idempotency_key).map_err(Refused::malformed)?;

        // 3. Health.
        let health = self.health();
        let effect = self.effect_of(&def, &input).await;
        let effect_class = effect.as_ref().map_or(EffectClass::Read, |found| found.0);
        if !health.allows(def.needs, effect_class) {
            return Err(Refused::new(
                codes::busy(
                    codes::ENGINE_DEGRADED,
                    format!(
                        "The engine is {}: {}.",
                        health.state.as_str(),
                        health.cause()
                    ),
                    "Call `doctor.run` for the fix, then try again.",
                ),
                effect_class,
            )
            .then(Next::new("doctor.run", json!({}), "now")));
        }

        // The `Nothing` actions report on the engine and must answer when
        // there is no store to record a decision in. They change nothing and
        // show no data of any system.
        let decided = effect;
        let decision = if def.needs == HealthNeed::Nothing && health.state == HealthState::Stopped {
            DecisionId::from_bytes([0; 16])
        } else {
            // 4. Decide: one decision, recorded whether it allows or refuses.
            let (effect_class, hazard) = effect.unwrap_or((EffectClass::Read, Hazard::None));
            let decision = self
                .decide_action(&def, &request.caller, effect_class, hazard)
                .await;
            if let Some(refusal) = decision.refusal {
                return Err(Refused::after(refusal, effect_class, decision.id));
            }
            if !decision.allowed {
                return Err(Refused::after(
                    codes::policy(
                        a2m_policy::NOT_GRANTED,
                        "Policy did not allow this action.",
                        "Ask an administrator for the role the action needs.",
                    ),
                    effect_class,
                    decision.id,
                ));
            }
            decision.id
        };

        // 5. Person. An act that is a person's whenever it loosens is asked
        // for here as well, so that no handler has to remember to ask.
        let asks = match def.person {
            PersonNeed::Presence => true,
            PersonNeed::PresenceWhenLoosening => loosens(&def, &input),
            PersonNeed::None => false,
        };
        if asks && request.caller.is_agent {
            // A person's act asked for by an agent is shown to the person
            // on a page of the person surface; a press there, and the
            // presence prompt it raises, carry it out (decision 0163).
            let project = match request.project {
                Some(stated) => match self.checked_project(&stated).await {
                    Ok(folder) => (Some(folder), None),
                    Err(refused) => (None, Some(refused)),
                },
                None => (None, None),
            };
            let output = self
                .ask_person(&def, &request.caller, input, project)
                .await
                .map_err(|failure| Refused::after(failure, effect_class, decision))?;
            return Ok((output, decision));
        }
        let presence = if asks {
            Some(
                self.presence(&request.caller, &def, effect_class)
                    .await
                    .map_err(|failure| Refused::after(failure, effect_class, decision))?,
            )
        } else {
            None
        };

        // 6. Run. The project folder a door stated is checked first; one
        // that fails is not handed on.
        // What is handed on is the folder as the check resolved it, with
        // every link followed, never the path as it was stated: a link in
        // the stated path that is changed after the check moves nothing.
        let (project, project_refused) = match request.project {
            Some(stated) => match self.checked_project(&stated).await {
                Ok(folder) => (Some(folder), None),
                Err(refused) => (None, Some(refused)),
            },
            None => (None, None),
        };
        let call = Call {
            input,
            caller: request.caller,
            decision,
            idempotency_key: idempotency_key.clone(),
            project,
            project_refused,
            presence,
            decided,
        };
        let mut output = self
            .run_handler(&def, call)
            .await
            .map_err(|failure| Refused::after(failure, effect_class, decision))?;
        if idempotency_key.is_some() && !READS_KEY.contains(&def.name.as_str()) {
            output = output.warn(format!(
                "`{}` starts no run, so the idempotency key was not used.",
                def.name
            ));
        }

        // 7. Shape: the cap. Redaction is the caller's last act, above.
        Ok((self.capped(&def, output), decision))
    }

    /// Checks a project folder a front door stated, and gives the folder
    /// with every link followed. It is taken only on one machine, where
    /// the caller is the user the engine runs as; it must be an absolute
    /// path to a folder that exists; it may neither hold nor lie inside one
    /// of the engine's own folders, or a folder in which another program
    /// keeps its data; it must hold a project file; and it must be inside
    /// what the setting `files.allowed_roots` allows. So no action can be
    /// pointed at the store, the vault's files, a browser's profile, or a
    /// key folder through it.
    ///
    /// # Errors
    ///
    /// `input`/`path_not_allowed`, with the reason.
    pub(crate) async fn checked_project(
        &self,
        stated: &std::path::Path,
    ) -> Result<PathBuf, Failure> {
        let refuse = |why: &str| {
            codes::input(
                codes::PATH_NOT_ALLOWED,
                format!("The project folder of this call is not used: {why}."),
                "Run the command inside a project folder of your own, one that holds `anything2mcp.toml`, or pass `--project` with its full path.",
            )
        };
        if !self.is_local() {
            return Err(refuse(
                "a project folder belongs to one machine, and this engine is a server",
            ));
        }
        if !stated.is_absolute() {
            return Err(refuse("its path is not a full path"));
        }
        let stated = stated.to_path_buf();
        let kernel = self.clone();
        // Looking at the disk waits: off the async threads.
        tokio::task::spawn_blocking(move || {
            crate::project::check(
                &stated,
                kernel.paths(),
                kernel.settings(),
                &kernel.inner.places,
            )
        })
        .await
        .unwrap_or(Err("it could not be looked at"))
        .map_err(refuse)
    }

    /// Asks the role's person surface for proof that a person is present.
    ///
    /// # Errors
    ///
    /// `policy`/`person_required` for a caller marked as an agent, which
    /// never satisfies it; what the surface answers otherwise.
    pub(crate) async fn presence(
        &self,
        caller: &Caller,
        def: &ActionDef,
        effect: EffectClass,
    ) -> Result<PresenceProof, Failure> {
        if caller.is_agent || caller.principal.principal.kind != PrincipalKind::Person {
            return Err(codes::policy(
                codes::PERSON_REQUIRED,
                format!(
                    "`{}` is a person's act, and this caller is not a person at this machine.",
                    def.name
                ),
                "Ask the person to run it themselves, at a terminal.",
            ));
        }
        let asked = caller.principal.principal.id;
        let proof = self
            .person()
            .confirm_presence(PresenceAsk {
                principal: asked,
                action: def.name.clone(),
                effect,
                summary: def.summary.clone(),
            })
            .await?;
        // The proof is checked where it is used: it must be this caller's,
        // and it must be of now. A surface that hands back a proof it kept
        // from another person's prompt, or from an earlier one, proves
        // nothing about this call.
        proof
            .checked(asked, self.inner.clock.now())
            .map_err(|why| {
                codes::policy(
                    a2m_policy::PRESENCE_REQUIRED,
                    format!(
                        "`{}` is a person's act, and the proof of presence was not taken: {why}.",
                        def.name
                    ),
                    "Run the command again and answer the prompt yourself.",
                )
            })?;
        Ok(proof)
    }

    pub(crate) async fn run_handler(&self, def: &ActionDef, call: Call) -> Result<Output, Failure> {
        match &def.handler {
            Handler::Kernel(handler) => handler(self, call).await,
            Handler::Provider(name) => {
                let Some((action, _)) = self.providers().action(name) else {
                    return Err(not_built(def));
                };
                let provider = a2m_provider::ProviderName::new(def.name.noun())?;
                let roots = self
                    .providers()
                    .provider_roots(&provider)
                    .cloned()
                    .unwrap_or_else(a2m_provider::Roots::none);
                let input: Values =
                    serde_json::from_value(Json::Object(call.input)).map_err(|_| {
                        codes::input(
                            codes::SCHEMA_INVALID,
                            "The input holds a value the action cannot take.",
                            "Use text, whole numbers, true or false, lists, and maps.",
                        )
                    })?;
                let redact = self.redact();
                let cx = a2m_provider::ActionContext {
                    roots: &roots,
                    registry: self.providers(),
                    redact: redact.as_ref(),
                };
                let values = action.handle(&input, &cx).await?;
                let result = serde_json::to_value(&values).map_err(|_| {
                    codes::input(
                        codes::SCHEMA_INVALID,
                        "The provider's result could not be written as JSON.",
                        "Report this to the provider's publisher.",
                    )
                })?;
                Ok(Output::ok(result))
            }
            Handler::Client => Err(codes::input(
                codes::CLIENT_ACTION,
                format!(
                    "`{}` acts on the engine's own process and is carried out by the client end.",
                    def.name
                ),
                "Run it as a command on the machine the engine runs on.",
            )),
            Handler::NotBuilt => Err(not_built(def)),
        }
    }

    /// Cuts a result to the configured size cap, and says which limit cut
    /// it.
    pub(crate) fn capped(&self, def: &ActionDef, mut output: Output) -> Output {
        let cap = self.settings().int("tools.result_cap_bytes").unwrap_or(0);
        let Ok(cap) = usize::try_from(cap) else {
            return output;
        };
        if cap == 0 {
            return output;
        }
        let size = output.result.to_string().len();
        if size > cap {
            output.result = json!({
                "truncated": true,
                "limit": "tools.result_cap_bytes",
                "limit_bytes": cap,
                "size_bytes": size,
                "summary": format!("The result of `{}` is larger than the cap and was not returned.", def.name),
            });
            output.warnings.push(format!(
                "The result was cut by the setting `tools.result_cap_bytes` ({cap} bytes). Ask for less with `limit` and `offset`."
            ));
        }
        output
    }

    /// The decision for an action, recorded in a transaction of its own.
    pub(crate) async fn decide_action(
        &self,
        def: &ActionDef,
        caller: &Caller,
        effect: EffectClass,
        hazard: Hazard,
    ) -> Decided {
        let Some(policy) = self.policy() else {
            return Decided {
                id: DecisionId::from_bytes([0; 16]),
                allowed: false,
                refusal: Some(codes::busy(
                    codes::ENGINE_DEGRADED,
                    "No policy set is loaded, so nothing can be decided.",
                    "Call `doctor.run` for the fix.",
                )),
            };
        };
        let mut context = Context::plain(caller.principal.workspace, self.environment());
        context.effect = effect;
        context.hazard = hazard;
        context.toolset = caller.toolset.clone();
        context.caller_is_agent = caller.is_agent;
        context.acting_for = caller.principal.acting_for.clone();
        let name = match a2m_policy::ActionName::new(def.name.as_str()) {
            Ok(name) => name,
            Err(failure) => {
                return Decided {
                    id: DecisionId::from_bytes([0; 16]),
                    allowed: false,
                    refusal: Some(failure),
                };
            }
        };
        let request = a2m_policy::Request {
            principal: caller.principal.principal.clone(),
            question: Question::PerformAction,
            resource: Resource::Action(ActionRef {
                name,
                needs: def.role(effect),
            }),
            context,
            run: None,
            diff_hash: None,
        };
        let decision = policy.decide_alone(request).await;
        Decided {
            id: decision.id,
            allowed: decision.allowed(),
            refusal: decision.refusal,
        }
    }

    /// A caller of this installation's workspace who is not its user: a
    /// second person, who holds no role and no grant under the built-in
    /// set. For a test of a crate above this one that must prove that a
    /// caller who did not start a run cannot follow, read, or resume it.
    /// Never part of the product: a caller is named by
    /// [`Kernel::verify_caller`] and by nothing else.
    ///
    /// # Errors
    ///
    /// `busy`/`engine_degraded` when the store is not open.
    #[cfg(any(test, feature = "testing"))]
    pub fn second_person(&self, id: [u8; 16]) -> Result<Principal, Failure> {
        Ok(Principal {
            principal: PrincipalRef::person(a2m_core::PrincipalId::from_bytes(id)),
            workspace: self.installation()?.workspace,
            acting_for: None,
        })
    }

    /// The effect class and hazard a call would have: the action's own, or,
    /// for `tool.call`, the published tool's.
    async fn effect_of(
        &self,
        def: &ActionDef,
        input: &Map<String, Json>,
    ) -> Option<(EffectClass, Hazard)> {
        match def.effect {
            Effect::Fixed(effect) => Some((effect, Hazard::None)),
            Effect::OfTool => {
                let tool = input.get("tool").and_then(Json::as_str)?;
                self.tool_effect(tool).await
            }
        }
    }
}

/// Whether a call of an action that is a person's act whenever it loosens
/// does loosen. Releasing a kill switch does. An action that has not said
/// how its loosening is told is asked for on every call, which is the safe
/// side: `config.set` and `provider.enable`, until they are built.
fn loosens(def: &ActionDef, input: &Map<String, Json>) -> bool {
    match def.name.as_str() {
        "killswitch.set" => input.get("on").and_then(Json::as_bool) == Some(false),
        _ => true,
    }
}

/// A decision, as the dispatcher needs it.
pub(crate) struct Decided {
    pub(crate) id: DecisionId,
    pub(crate) allowed: bool,
    pub(crate) refusal: Option<Failure>,
}

pub(crate) struct Refused {
    failure: Failure,
    malformed: bool,
    effect: EffectClass,
    next: Option<Next>,
    decision: Option<DecisionId>,
}

impl Refused {
    fn new(failure: Failure, effect: EffectClass) -> Box<Self> {
        Box::new(Self {
            failure,
            malformed: false,
            effect,
            next: None,
            decision: None,
        })
    }

    fn malformed(failure: Failure) -> Box<Self> {
        Box::new(Self {
            failure,
            malformed: true,
            effect: EffectClass::Read,
            next: None,
            decision: None,
        })
    }

    pub(crate) fn after(failure: Failure, effect: EffectClass, decision: DecisionId) -> Box<Self> {
        Box::new(Self {
            failure,
            malformed: false,
            effect,
            next: None,
            decision: Some(decision),
        })
    }

    fn then(mut self: Box<Self>, next: Next) -> Box<Self> {
        self.next = Some(next);
        self
    }
}

fn not_built(def: &ActionDef) -> Failure {
    codes::input(
        codes::ACTION_NOT_BUILT,
        format!(
            "`{}` is in the catalog and is not built yet (it arrives with {}).",
            def.name,
            if def.phase.starts_with("Stage") || def.phase == "provider" {
                def.phase.to_owned()
            } else {
                format!("Phase {}", def.phase)
            }
        ),
        "Call `action.search` to see which actions are built.",
    )
}

/// Whether a failure means that a person must act before anything goes on.
fn person_is_needed(failure: &Failure) -> bool {
    failure.class() == FailureClass::SessionExpired
        || failure.code() == &codes::PERSON_REQUIRED
        || failure.code() == &a2m_policy::PRESENCE_REQUIRED
        || failure.code() == &a2m_policy::APPROVAL_MISSING
}

/// Checks `input` against the action's schema: an object with the action's
/// fields and no others, each of its declared type.
///
/// # Errors
///
/// `input`/`schema_invalid`, naming the field and what it must be.
/// The actions that read a request's idempotency key: `tool.call`, which
/// starts a run, and `action.run`, which hands it to the action it runs.
const READS_KEY: &[&str] = &["tool.call", "action.run"];

/// The field of `tool.call` that carries the idempotency key, for a door
/// that has no other place for one.
pub(crate) const KEY_FIELD: &str = "idempotency_key";

/// Takes the idempotency key of a request: the one the door carried, or,
/// for `tool.call`, the one in its own field, which is taken out of the
/// input. Both may be given only when they are the same key.
///
/// # Errors
///
/// `input`/`idempotency_key_invalid` for a key that is not 1 to 200
/// printable characters, or for two different keys.
fn taken_key(
    def: &ActionDef,
    mut input: Map<String, Json>,
    stated: Option<String>,
) -> Result<(Map<String, Json>, Option<String>), Failure> {
    if def.name.as_str() == "run.list" {
        // A key beside a listing is the key to look for: a door or a
        // command line with one place for a key carries it there. It is
        // no key of the listing's own, which starts no run.
        if let Some(door) = stated {
            match input.get(KEY_FIELD) {
                Some(Json::String(field)) if *field != door => {
                    return Err(codes::input(
                        codes::IDEMPOTENCY_KEY_INVALID,
                        "The call carries two different idempotency keys: one in its input and one beside it.",
                        "Send one key, in one place.",
                    ));
                }
                Some(_) => {}
                None => {
                    a2m_ledger::CallerKey::new(&door)?;
                    input.insert(KEY_FIELD.to_owned(), Json::String(door));
                }
            }
        }
        return Ok((input, None));
    }
    let given = if def.name.as_str() == "tool.call" {
        match input.remove(KEY_FIELD) {
            Some(Json::String(text)) => Some(text),
            _ => None,
        }
    } else {
        None
    };
    let key = match (given, stated) {
        (Some(field), Some(door)) if field != door => {
            return Err(codes::input(
                codes::IDEMPOTENCY_KEY_INVALID,
                "The call carries two different idempotency keys: one in its input and one beside it.",
                "Send one key, in one place.",
            ));
        }
        (field, door) => field.or(door),
    };
    if let Some(key) = &key {
        a2m_ledger::CallerKey::new(key)?;
    }
    Ok((input, key))
}

pub(crate) fn validate(def: &ActionDef, input: Json) -> Result<Map<String, Json>, Failure> {
    let map = match input {
        Json::Null => Map::new(),
        Json::Object(map) => map,
        _ => {
            return Err(invalid(
                "The input must be an object of named fields.".to_owned(),
                "Send a JSON object.".to_owned(),
            ));
        }
    };
    let known = || def.input.fields.iter().map(|field| field.name.as_str());
    for name in map.keys() {
        if def.input.field(name).is_none() {
            let near = codes::nearest(name, known());
            return Err(invalid(
                format!(
                    "`{}` has no field named `{}`.",
                    def.name,
                    codes::shown(name)
                ),
                if near.is_empty() {
                    format!(
                        "Its fields are: {}.",
                        known().collect::<Vec<_>>().join(", ")
                    )
                } else {
                    format!("Use one of: {}.", near.join(", "))
                },
            ));
        }
    }
    for field in &def.input.fields {
        match map.get(&field.name) {
            None | Some(Json::Null) if field.required => {
                return Err(invalid(
                    format!("`{}` needs the field `{}`.", def.name, field.name),
                    format!("Give `{}`: {}", field.name, field.help),
                ));
            }
            None | Some(Json::Null) => {}
            Some(value) => check_type(def, &field.name, &field.ty, value)?,
        }
    }
    // The schema itself is checked as well, for a provider's action, whose
    // fields the list above cannot type.
    let map: Map<String, Json> = map
        .into_iter()
        .filter(|(_, value)| !value.is_null())
        .collect();
    let whole = Json::Object(map);
    def.input.schema.check_json(&whole).map_err(Failure::from)?;
    match whole {
        Json::Object(map) => Ok(map),
        _ => Ok(Map::new()),
    }
}

fn check_type(def: &ActionDef, name: &str, ty: &FieldType, value: &Json) -> Result<(), Failure> {
    let wrong = |must: String| {
        invalid(
            format!("The field `{name}` of `{}` must be {must}.", def.name),
            format!("Send `{name}` as {must}."),
        )
    };
    match ty {
        FieldType::Text => value
            .as_str()
            .map(|_| ())
            .ok_or_else(|| wrong("text".to_owned())),
        FieldType::Int { min, max } => match value.as_i64() {
            Some(number) if number >= *min && number <= *max => Ok(()),
            _ => Err(wrong(format!("a whole number from {min} to {max}"))),
        },
        FieldType::Bool => value
            .as_bool()
            .map(|_| ())
            .ok_or_else(|| wrong("true or false".to_owned())),
        FieldType::Word(words) => match value.as_str() {
            Some(word) if words.contains(&word) => Ok(()),
            _ => Err(wrong(format!("one of: {}", words.join(", ")))),
        },
        FieldType::TextList => match value.as_array() {
            Some(items) if items.iter().all(Json::is_string) => Ok(()),
            _ => Err(wrong("a list of texts".to_owned())),
        },
        FieldType::Map => value
            .as_object()
            .map(|_| ())
            .ok_or_else(|| wrong("a map of named values".to_owned())),
        FieldType::MapList => match value.as_array() {
            Some(items) if items.iter().all(Json::is_object) => Ok(()),
            _ => Err(wrong("a list of maps of named values".to_owned())),
        },
        FieldType::Any => Ok(()),
    }
}

fn invalid(message: String, next: String) -> Failure {
    codes::input(codes::SCHEMA_INVALID, message, next)
}

/// Every text in `value`, at any depth, passed through the redactor. A
/// field's name is redacted too, because a system chooses the names it
/// returns.
pub(crate) fn redact_json(redact: &dyn Redact, value: &Json) -> Json {
    match value {
        Json::String(text) => Json::String(redact.redact(text)),
        Json::Array(items) => {
            Json::Array(items.iter().map(|item| redact_json(redact, item)).collect())
        }
        Json::Object(map) => Json::Object(
            map.iter()
                .map(|(name, value)| (redact.redact(name), redact_json(redact, value)))
                .collect(),
        ),
        other => other.clone(),
    }
}
