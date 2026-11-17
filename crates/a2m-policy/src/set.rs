//! The policy set: what administrators write, how it is checked and hashed,
//! and the table of tests a version must pass before it is published.
//!
//! A set is data. Grants, roles, and pre-approval rules are the permits; the
//! guardrails in [`crate::rules`] are forbids compiled into the binary and
//! are part of every set, because a [`CandidateSet`] is made only by
//! [`CandidateSet::compile`], which stamps the guardrails' hash into the
//! version.

use std::collections::BTreeSet;

use a2m_core::{
    CanonicalJson, Digest, EffectClass, Failure, PlanExpr, PrincipalId, Sensitivity, TargetName,
    ValueType, WorkspaceId,
};
use a2m_expr::{LibraryVersion, Program, TypeEnv};
use serde::{Deserialize, Serialize};

use crate::failure::invalid;
use crate::model::{
    GroupName, IdentityMode, ModuleName, OptionName, PrincipalRef, Request, Role, RuleId, ToolRef,
    word,
};
use crate::rules::{self, Answer};

/// The format of a stored policy set. Raised when the stored shape changes.
pub const SET_FORMAT: u32 = 1;

/// The names a pre-approval condition may read, with their types.
pub const CONDITION_NAMES: &[(&str, ValueType)] = &[
    ("effect", ValueType::Text),
    ("hazard", ValueType::Text),
    ("records", ValueType::Int),
    ("environment", ValueType::Text),
    ("toolset", ValueType::Text),
    ("target", ValueType::Text),
    ("module", ValueType::Text),
    ("tool", ValueType::Text),
    ("caller_is_agent", ValueType::Bool),
];

/// Who a grant, a role, or a rule is given to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subject {
    /// One principal.
    Principal(PrincipalId),
    /// Everyone in a group.
    Group(GroupName),
    /// Every authenticated principal.
    Anyone,
}

impl Subject {
    pub(crate) fn covers(&self, principal: &PrincipalRef) -> bool {
        match self {
            Self::Principal(id) => *id == principal.id,
            Self::Group(group) => principal.groups.contains(group),
            Self::Anyone => true,
        }
    }
}

/// Which tools a grant or a pre-approval covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Every tool of a target.
    Target(TargetName),
    /// Every tool of one module of a target.
    Module(TargetName, ModuleName),
    /// Every tool whose full name matches, where `*` stands for any run of
    /// characters, as in `example_order_*`.
    Pattern(String),
}

impl Scope {
    pub(crate) fn covers(&self, tool: &ToolRef) -> bool {
        match self {
            Self::Target(target) => *target == tool.target,
            Self::Module(target, module) => *target == tool.target && *module == tool.module,
            Self::Pattern(pattern) => wildcard(pattern.as_bytes(), tool.tool.as_str().as_bytes()),
        }
    }
}

/// Whether `name` matches `pattern`, where `*` stands for any run of bytes.
fn wildcard(pattern: &[u8], name: &[u8]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some((b'*', rest)) => (0..=name.len()).any(|skip| wildcard(rest, &name[skip..])),
        Some((first, rest)) => name
            .split_first()
            .is_some_and(|(byte, tail)| byte == first && wildcard(rest, tail)),
    }
}

/// A role given to a subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleBinding {
    /// Who holds it.
    pub who: Subject,
    /// The role.
    pub role: Role,
}

/// A grant: who may use which tools, up to which effect class, acting through
/// which identity mode. Each grant is one permit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    /// Who it is given to.
    pub who: Subject,
    /// The tools it covers.
    pub scope: Scope,
    /// The highest effect class it allows.
    pub max_effect: EffectClass,
    /// The identity mode a run under it may use.
    pub identity_mode: IdentityMode,
}

/// Who may see fields up to a sensitivity label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelGrant {
    /// Who may see them.
    pub who: Subject,
    /// The most careful label they may see.
    pub up_to: Sensitivity,
}

/// Who may set a per-call option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionGrant {
    /// Who may set it.
    pub who: Subject,
    /// The option, or every option when none is named.
    pub option: Option<OptionName>,
}

/// A rule under which a write may proceed without a person. It can never
/// cover an `irreversible` operation or any hazard other than `none`: the
/// guardrails forbid those whatever a rule says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreApproval {
    /// The rule's name within the set, such as `own_small_updates`.
    pub name: String,
    /// Whose writes it covers.
    pub who: Subject,
    /// The tools it covers.
    pub scope: Scope,
    /// The most records one run may hold.
    pub max_records: u32,
    /// A further condition over the names in [`CONDITION_NAMES`], in the
    /// expression language; it must give true or false.
    pub condition: Option<String>,
}

word! {
    /// Who must approve a write. `NamedApprover` is the stricter.
    #[derive(Default)]
    ApproveWrites {
        /// A person approves every run; the person whose call started the run
        /// may be that person.
        #[default]
        PersonEachRun => "person_each_run",
        /// A person other than the one whose call started the run.
        NamedApprover => "named_approver",
    }
}

/// How many calls a subject may make in each period. Counted when a call is
/// accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quota {
    /// Whose calls are counted, each caller on its own.
    pub who: Subject,
    /// The most calls one caller may make in a period.
    pub calls: u32,
    /// The length of a period, in minutes.
    pub period_minutes: u32,
}

/// The protected settings a policy set carries (spec section 226).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Who must approve a write.
    pub approve_writes: ApproveWrites,
    /// The most records one run may hold.
    pub max_records_per_run: u32,
    /// How long an approval may wait to be used, in minutes.
    pub approval_window_minutes: u32,
}

impl Settings {
    /// The longest approval window any set may state: one day.
    pub const LONGEST_WINDOW_MINUTES: u32 = 24 * 60;
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            approve_writes: ApproveWrites::PersonEachRun,
            max_records_per_run: 50,
            approval_window_minutes: 15,
        }
    }
}

/// One row of a set's own test table: a request and the answer it must get.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyTest {
    /// What the row checks, in a few words.
    pub name: String,
    /// The request.
    pub request: Request,
    /// The answer the set must give.
    pub expect: Answer,
}

/// What an administrator writes: one layer of roles, grants, and rules, with
/// its settings and its tests. Lower layers that only narrow (workspace,
/// toolset, target files) are not built yet.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PolicyLayers {
    /// Who holds which role.
    pub roles: Vec<RoleBinding>,
    /// The grants.
    pub grants: Vec<Grant>,
    /// Who may see which labels.
    pub labels: Vec<LabelGrant>,
    /// Who may set which per-call options.
    pub options: Vec<OptionGrant>,
    /// The rules under which a write needs no person.
    pub pre_approvals: Vec<PreApproval>,
    /// The quotas.
    pub quotas: Vec<Quota>,
    /// The protected settings.
    pub settings: Settings,
    /// The set's own tests. They are hashed into the version and stored
    /// beside the layers, and are never read back into a request.
    #[serde(skip)]
    pub tests: Vec<PolicyTest>,
}

/// Where a set runs, which decides what it may say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// One machine, one operating-system user.
    Local,
    /// A shared server.
    Server,
}

/// The policy set a fresh store starts with.
#[derive(Debug, Clone)]
pub struct BuiltIn {
    pub(crate) profile: Profile,
    pub(crate) layers: PolicyLayers,
}

impl BuiltIn {
    /// The set of one machine: the operating-system user holds every role and
    /// a grant on every tool, writes need a person's approval, and the
    /// guardrails apply.
    #[must_use]
    pub fn local(user: PrincipalId, workspace: WorkspaceId) -> Self {
        let who = Subject::Principal(user);
        let layers = PolicyLayers {
            roles: Role::ALL
                .iter()
                .map(|role| RoleBinding {
                    who: who.clone(),
                    role: *role,
                })
                .collect(),
            grants: IdentityMode::ALL
                .iter()
                .map(|mode| Grant {
                    who: who.clone(),
                    scope: Scope::Pattern("*".to_owned()),
                    max_effect: EffectClass::Irreversible,
                    identity_mode: *mode,
                })
                .collect(),
            labels: vec![LabelGrant {
                who: who.clone(),
                up_to: Sensitivity::Restricted,
            }],
            options: vec![OptionGrant { who, option: None }],
            pre_approvals: Vec::new(),
            quotas: Vec::new(),
            settings: Settings::default(),
            tests: rules::local_tests(user, workspace),
        };
        Self {
            profile: Profile::Local,
            layers,
        }
    }

    /// A server's first set: the guardrails and nothing else, so every
    /// decision is a denial until an administrator's set is published.
    #[must_use]
    pub fn server() -> Self {
        Self {
            profile: Profile::Server,
            layers: PolicyLayers::default(),
        }
    }

    /// Where this set runs.
    #[must_use]
    pub const fn profile(&self) -> Profile {
        self.profile
    }

    /// The layers of this set, to start a tighter one from.
    #[must_use]
    pub const fn layers(&self) -> &PolicyLayers {
        &self.layers
    }
}

/// A checked policy set in memory: what `decide` evaluates.
#[derive(Debug, Clone)]
pub(crate) struct Compiled {
    pub(crate) version: Digest,
    pub(crate) layers: PolicyLayers,
    /// One program per pre-approval, in order; `None` where it has no condition.
    pub(crate) conditions: Vec<Option<Program>>,
}

/// A checked policy set, ready to be tested and published. It can be made
/// only by [`CandidateSet::compile`], so none exists without the guardrails.
#[derive(Debug, Clone)]
pub struct CandidateSet {
    pub(crate) compiled: Compiled,
    /// The canonical text of the layers, as stored.
    pub(crate) layers_text: String,
    /// The canonical text of the tests, as stored.
    pub(crate) tests_text: String,
}

#[derive(Serialize)]
struct Stamp {
    format: u32,
    guardrails: Digest,
    layers: Digest,
    tests: Digest,
}

/// The version of a set: a digest over the format, the guardrails compiled
/// into this binary, the layers, and the tests.
pub(crate) fn version_of(layers_text: &str, tests_text: &str) -> Result<Digest, Failure> {
    let stamp = Stamp {
        format: SET_FORMAT,
        guardrails: rules::guardrails_digest(),
        layers: Digest::of(layers_text.as_bytes()),
        tests: Digest::of(tests_text.as_bytes()),
    };
    Ok(CanonicalJson::encode(&stamp)?.digest())
}

impl CandidateSet {
    /// Checks `layers` and makes a candidate. The guardrails are added here
    /// and nowhere else.
    ///
    /// # Errors
    ///
    /// `input`, `schema_invalid` when a setting is out of range, a rule is
    /// badly formed, a condition does not compile to true or false, or the
    /// set says something its profile does not allow.
    pub fn compile(layers: &PolicyLayers, profile: Profile) -> Result<Self, Failure> {
        let settings = &layers.settings;
        if settings.max_records_per_run == 0 {
            return Err(invalid(
                "The record limit of a policy set must be at least 1.",
                "Set `max_records_per_run` to 1 or more.",
            ));
        }
        if settings.approval_window_minutes == 0
            || settings.approval_window_minutes > Settings::LONGEST_WINDOW_MINUTES
        {
            return Err(invalid(
                "The approval window must be between 1 minute and one day.",
                "Set `approval_window_minutes` between 1 and 1440.",
            ));
        }
        if profile == Profile::Local && settings.approve_writes != ApproveWrites::PersonEachRun {
            return Err(invalid(
                "On one machine a write is approved by the person at it; no other approver can be named.",
                "Set `approve_writes` to `person_each_run`.",
            ));
        }
        for grant in &layers.grants {
            check_scope(&grant.scope)?;
        }
        if layers
            .quotas
            .iter()
            .any(|quota| quota.calls == 0 || quota.period_minutes == 0)
        {
            return Err(invalid(
                "A quota needs at least one call and a period of at least one minute.",
                "Set the quota's `calls` and `period_minutes` to 1 or more.",
            ));
        }
        let mut names = BTreeSet::new();
        let mut conditions = Vec::with_capacity(layers.pre_approvals.len());
        for rule in &layers.pre_approvals {
            RuleId::new(&rule.name).map_err(|_| {
                invalid(
                    "A pre-approval rule's name is not a valid name.",
                    "Name the rule in lowercase, such as `own_small_updates`.",
                )
            })?;
            if !names.insert(rule.name.as_str()) {
                return Err(invalid(
                    format!("Two pre-approval rules are named `{}`.", rule.name),
                    "Give each pre-approval rule its own name.",
                ));
            }
            check_scope(&rule.scope)?;
            if rule.max_records == 0 || rule.max_records > settings.max_records_per_run {
                return Err(invalid(
                    format!(
                        "The pre-approval rule `{}` covers more records than a run may hold.",
                        rule.name
                    ),
                    "Set its `max_records` between 1 and the set's record limit.",
                ));
            }
            conditions.push(match &rule.condition {
                None => None,
                Some(source) => Some(compile_condition(source)?),
            });
        }
        let layers_text = text_of(&CanonicalJson::encode(layers)?)?;
        let tests_text = text_of(&CanonicalJson::encode(&layers.tests)?)?;
        let version = version_of(&layers_text, &tests_text)?;
        Ok(Self {
            compiled: Compiled {
                version,
                layers: layers.clone(),
                conditions,
            },
            layers_text,
            tests_text,
        })
    }

    /// The version this set will have once published.
    #[must_use]
    pub const fn version(&self) -> Digest {
        self.compiled.version
    }

    /// The layers the set was compiled from.
    #[must_use]
    pub const fn layers(&self) -> &PolicyLayers {
        &self.compiled.layers
    }

    /// Runs the set's own test table.
    #[must_use]
    pub fn test(&self) -> TestReport {
        let mut report = TestReport::default();
        for test in &self.compiled.layers.tests {
            let got = rules::evaluate(&self.compiled, &test.request).answer;
            if got == test.expect {
                report.passed += 1;
            } else {
                report.failed.push(TestFailure {
                    name: test.name.clone(),
                    expected: test.expect,
                    got,
                });
            }
        }
        report
    }

    /// Whether this set is looser than `current` on a protected setting: a
    /// higher record limit, a longer approval window, a weaker approver, or a
    /// pre-approval rule `current` does not hold.
    #[must_use]
    pub fn loosens(&self, current: &PolicyLayers) -> bool {
        let (new, old) = (&self.compiled.layers.settings, &current.settings);
        new.max_records_per_run > old.max_records_per_run
            || new.approval_window_minutes > old.approval_window_minutes
            || new.approve_writes < old.approve_writes
            || self
                .compiled
                .layers
                .pre_approvals
                .iter()
                .any(|rule| !current.pre_approvals.contains(rule))
    }
}

pub(crate) fn text_of(json: &CanonicalJson) -> Result<String, Failure> {
    String::from_utf8(json.as_bytes().to_vec()).map_err(|_| {
        invalid(
            "The policy set does not encode as text.",
            "Report this; it is a fault in the policy engine.",
        )
    })
}

fn check_scope(scope: &Scope) -> Result<(), Failure> {
    let Scope::Pattern(pattern) = scope else {
        return Ok(());
    };
    let well_formed = !pattern.is_empty()
        && pattern.len() <= 64
        && pattern
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'*');
    if well_formed {
        Ok(())
    } else {
        Err(invalid(
            "A tool-name pattern holds lowercase letters, digits, underscores, and `*` only.",
            "Write the pattern like `example_order_*`.",
        ))
    }
}

fn compile_condition(source: &str) -> Result<Program, Failure> {
    let mut env = TypeEnv::new(LibraryVersion::CURRENT);
    for (name, ty) in CONDITION_NAMES {
        env.declare(name, ty.clone())?;
    }
    let compiled = a2m_expr::compile(source, &env, Some(&ValueType::Bool))?;
    let expr: PlanExpr = compiled.to_plan_expr()?;
    Program::load(&expr)
}

/// A stored set, read back: the same checks as [`CandidateSet::compile`],
/// and the stored version must be the one these bytes give under this
/// binary's guardrails.
pub(crate) fn load(
    version: Digest,
    layers_text: &str,
    tests_text: &str,
    profile: Profile,
) -> Result<Compiled, Failure> {
    let unreadable = || {
        invalid(
            "The stored policy set cannot be read.",
            "Publish a policy set again.",
        )
    };
    if version_of(layers_text, tests_text)? != version {
        return Err(unreadable());
    }
    let layers: PolicyLayers = serde_json::from_str(layers_text).map_err(|_| unreadable())?;
    let set = CandidateSet::compile(&layers, profile)?;
    if set.layers_text != layers_text {
        return Err(unreadable());
    }
    // The tests were hashed into the stored version and are not read back, so
    // the version is the stored one, not the one of a set with no tests.
    Ok(Compiled {
        version,
        ..set.compiled
    })
}

/// The result of running a set's test table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TestReport {
    /// How many rows got the answer they expect.
    pub passed: usize,
    /// The rows that did not.
    pub failed: Vec<TestFailure>,
}

impl TestReport {
    /// Whether every row passed.
    #[must_use]
    pub const fn ok(&self) -> bool {
        self.failed.is_empty()
    }
}

/// One row of a test table that got the wrong answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFailure {
    /// The row's name.
    pub name: String,
    /// The answer it expects.
    pub expected: Answer,
    /// The answer the set gave.
    pub got: Answer,
}
