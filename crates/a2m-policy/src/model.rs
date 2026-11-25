//! What a policy request is about: who asks, which question, about what, and
//! in which context.
//!
//! Every field here is a verified fact. `a2m-actions` builds a request from
//! the caller's checked credential, the compiled target, and what the engine
//! reports; nothing in it is read from a caller's input, and so [`Context`]
//! can be written but never deserialized.

use std::fmt;

use a2m_core::{
    Digest, EffectClass, Environment, Failure, Hazard, Identity, OperationName, PrincipalId, RunId,
    Sensitivity, TargetName, WorkspaceId,
};
use serde::{Deserialize, Serialize};

use crate::failure::invalid;

/// Defines a checked name of this crate: lowercase letters, digits,
/// underscores, and dots, starting with a letter.
macro_rules! policy_name {
    ($(#[$meta:meta])* $name:ident: $kind:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// The longest such a name may be.
            pub const LONGEST: usize = 64;

            /// Checks the shape of `text` and makes the name.
            ///
            /// # Errors
            ///
            /// `input`, `schema_invalid` when the text is empty, too long, or
            /// holds anything but lowercase letters, digits, underscores, and
            /// dots after a first lowercase letter.
            pub fn new(text: &str) -> Result<Self, Failure> {
                let well_formed = text.len() <= Self::LONGEST
                    && text.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
                    && text.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.'
                    });
                if well_formed {
                    Ok(Self(text.to_owned()))
                } else {
                    Err(invalid(
                        format!("That is not a valid {} name.", $kind),
                        "Use lowercase letters, digits, underscores, and dots, starting with a letter.",
                    ))
                }
            }

            /// The name as text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = <String as Deserialize>::deserialize(deserializer)?;
                Self::new(&text).map_err(|_| serde::de::Error::custom(concat!("not a ", $kind, " name")))
            }
        }
    };
}

policy_name! {
    /// The name of an authoring or admin action, such as `grant.add`.
    ActionName: "action"
}
policy_name! {
    /// The name of a per-call option, such as pinning a mode.
    OptionName: "option"
}
policy_name! {
    /// The name of a toolset.
    ToolsetName: "toolset"
}
policy_name! {
    /// The name of a group of people, as the identity provider gives it.
    GroupName: "group"
}
policy_name! {
    /// The name of a module of a target.
    ModuleName: "module"
}
policy_name! {
    /// The name of one rule in a policy set, such as `guardrail.hazard_needs_person`.
    RuleId: "rule"
}

impl RuleId {
    /// A rule's name made by this crate, which is well formed by construction.
    pub(crate) fn known(text: &str) -> Self {
        Self(text.to_owned())
    }
}

impl Question {
    /// Whether the question is about work on a system, which a kill switch
    /// stops. Approving, administering, and masking a result are not: a
    /// switch must stay releasable, and a result already read is still masked.
    pub(crate) const fn is_work(self) -> bool {
        matches!(
            self,
            Self::UseTool | Self::ActAs | Self::WriteWithoutPerson | Self::SetOption
        )
    }
}

/// Defines a closed word of this crate with its written form.
macro_rules! word {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, ::serde::Serialize, ::serde::Deserialize)]
        pub enum $name {
            $($(#[$vmeta])* #[serde(rename = $text)] $variant),+
        }

        impl $name {
            /// Every value, in declared order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The word as it is written in files, tables, and messages.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }

            /// The value written as `text`, if there is one.
            #[must_use]
            pub fn from_text(text: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|value| value.as_str() == text)
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}
pub(crate) use word;

word! {
    /// The seven questions of spec section 212. Each is answered allow or deny.
    Question {
        /// May this caller use this tool?
        UseTool => "use_tool",
        /// May this run act as this identity?
        ActAs => "act_as",
        /// May this write proceed without a person's approval?
        WriteWithoutPerson => "write_without_person",
        /// May this person approve this run?
        Approve => "approve",
        /// May this caller see fields with this sensitivity label?
        SeeLabel => "see_label",
        /// May this caller set this per-call option?
        SetOption => "set_option",
        /// May this principal perform this authoring or admin action?
        PerformAction => "perform_action",
    }
}

word! {
    /// What kind of principal is asking.
    PrincipalKind {
        /// A person.
        Person => "person",
        /// An AI agent with an identity of its own.
        Agent => "agent",
        /// A program with a key.
        Program => "program",
    }
}

word! {
    /// The roles of spec section 214.
    Role {
        /// Calls `read` operations they are granted.
        Viewer => "viewer",
        /// Viewer, plus starts `write` runs they are granted.
        Operator => "operator",
        /// Approves write runs.
        Approver => "approver",
        /// Uses the authoring tools; cannot publish alone.
        Builder => "builder",
        /// Reviews and publishes versions.
        Publisher => "publisher",
        /// Reads audit logs, traces, and explanations of decisions.
        Auditor => "auditor",
        /// Manages targets, grants, toolsets, sessions, settings, and kill switches.
        Admin => "admin",
    }
}

word! {
    /// How the identity a run acts as is obtained (glossary, "identity kind").
    IdentityKind {
        /// The caller's own token, passed on.
        PerUserToken => "per_user_token",
        /// A session signed in as the caller.
        PerUserSession => "per_user_session",
        /// A device signed in as the caller.
        PerUserDevice => "per_user_device",
        /// A dedicated account named in the settings.
        ServiceAccount => "service_account",
    }
}

/// Who asks: a verified principal, its kind, and the groups it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrincipalRef {
    /// The principal.
    pub id: PrincipalId,
    /// What kind of principal it is.
    pub kind: PrincipalKind,
    /// The groups a verified token or the directory sync says it belongs to.
    pub groups: Vec<GroupName>,
}

impl PrincipalRef {
    /// A person who belongs to no group.
    #[must_use]
    pub const fn person(id: PrincipalId) -> Self {
        Self {
            id,
            kind: PrincipalKind::Person,
            groups: Vec::new(),
        }
    }

    /// An agent with an identity of its own.
    #[must_use]
    pub const fn agent(id: PrincipalId) -> Self {
        Self {
            id,
            kind: PrincipalKind::Agent,
            groups: Vec::new(),
        }
    }
}

/// One tool: an operation or a pipeline, in its module, in its target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolRef {
    /// The target.
    pub target: TargetName,
    /// The module of the target.
    pub module: ModuleName,
    /// The tool's full name.
    pub tool: OperationName,
}

/// One authoring or admin action, and the role its registry entry says may
/// perform it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionRef {
    /// The action.
    pub name: ActionName,
    /// The role that may perform it.
    pub needs: Role,
}

/// One run, and the principal whose call started it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunRef {
    /// The run.
    pub run: RunId,
    /// Who started it.
    pub started_by: PrincipalId,
}

/// What a request is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Resource {
    /// A tool, for `UseTool` and `WriteWithoutPerson`.
    Tool(ToolRef),
    /// An action, for `PerformAction`.
    Action(ActionRef),
    /// The identity a run would act as, for `ActAs`. A person's identity is
    /// that person's session.
    Identity(Identity),
    /// A sensitivity label, for `SeeLabel`.
    Label(Sensitivity),
    /// A run, for `Approve`.
    Run(RunRef),
    /// A per-call option, for `SetOption`.
    Option(OptionName),
}

impl Resource {
    /// The target the resource belongs to, where it has one.
    #[must_use]
    pub const fn target(&self) -> Option<&TargetName> {
        match self {
            Self::Tool(tool) => Some(&tool.target),
            _ => None,
        }
    }

    /// A short name for the record of a decision. Never a value from a system.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Tool(tool) => format!("tool:{}", tool.tool),
            Self::Action(action) => format!("action:{}", action.name),
            Self::Identity(identity) => format!("identity:{identity}"),
            Self::Label(label) => format!("label:{label}"),
            Self::Run(run) => format!("run:{}", run.run),
            Self::Option(option) => format!("option:{option}"),
        }
    }
}

/// For whom an agent acts: the three cases of spec section 215.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActingFor {
    /// The agent runs on the person's own machine, as that operating-system user.
    LocalUser(PrincipalId),
    /// The agent's token was issued to the person and names them as its subject.
    TokenSubject(PrincipalId),
    /// The person delegated to the agent. Not built yet: a delegation never
    /// counts, so the answer is a denial.
    Delegation(PrincipalId),
}

impl ActingFor {
    /// The person the caller acts for, when the link is one this crate can
    /// check today.
    pub(crate) const fn proven_person(&self) -> Option<&PrincipalId> {
        match self {
            Self::LocalUser(person) | Self::TokenSubject(person) => Some(person),
            Self::Delegation(_) => None,
        }
    }
}

/// The verified facts around a request.
///
/// Built only by `a2m-actions`. It can be written to the record of a
/// decision and never read from text, so no caller can supply any of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Context {
    /// The effect class of the operation or pipeline.
    pub effect: EffectClass,
    /// Its hazard.
    pub hazard: Hazard,
    /// How many records the run holds, once that is known.
    pub records: Option<u32>,
    /// The workspace.
    pub workspace: WorkspaceId,
    /// The toolset the call came through.
    pub toolset: Option<ToolsetName>,
    /// The environment.
    pub environment: Environment,
    /// How the run's identity is obtained, once resolved from the toolset.
    pub identity_mode: Option<IdentityKind>,
    /// Whether the caller is an AI agent, whatever principal it calls as.
    pub caller_is_agent: bool,
    /// For whom the caller acts, where that was proved.
    pub acting_for: Option<ActingFor>,
    /// Whether every record the run would write is a declared test record
    /// of a test environment, as the caller found it. False when it is not
    /// known; with `writes_outside_tests` off, a write is then refused.
    pub test_records: bool,
}

impl Context {
    /// The context of a call that changes nothing and names no tool: an
    /// authoring or admin action, in `workspace` and `environment`.
    #[must_use]
    pub const fn plain(workspace: WorkspaceId, environment: Environment) -> Self {
        Self {
            effect: EffectClass::Read,
            hazard: Hazard::None,
            records: None,
            workspace,
            toolset: None,
            environment,
            identity_mode: None,
            caller_is_agent: false,
            acting_for: None,
            test_records: false,
        }
    }
}

/// One question to the policy engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Request {
    /// Who asks.
    pub principal: PrincipalRef,
    /// Which question.
    pub question: Question,
    /// About what.
    pub resource: Resource,
    /// The verified facts around it.
    pub context: Context,
    /// The run the question belongs to, where there is one.
    pub run: Option<RunId>,
    /// The hash of the run's diff, once the preview made one. A decision that
    /// lets a write proceed without a person is bound to this run and this
    /// diff, and to no other.
    pub diff_hash: Option<Digest>,
}
