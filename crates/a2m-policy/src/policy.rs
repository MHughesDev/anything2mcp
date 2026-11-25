//! The policy engine as the crates above see it: decide, record, publish,
//! explain, and the limiter beside the decision core.

use std::sync::{Arc, Mutex, PoisonError};

use a2m_core::{
    CanonicalJson, DecisionId, Digest, Failure, FailureClass, OperationName, PrincipalId, RunId,
    TargetName, Timestamp, WorkspaceId,
};
use a2m_ledger::{Durability, PolicyKey, Store, Tx, WriteClearance};

use crate::approval::{ApprovalSurface, PresenceCheck};
use crate::failure::{self, DECISION_NOT_FOUND, POLICY_TESTS_FAILED};
use crate::model::{
    Context, PrincipalRef, Question, Request, Resource, Role, RuleId, ToolRef, ToolsetName, word,
};
use crate::rules::{self, Answer, RuleKind, Verdict};
use crate::set::{
    self, BuiltIn, CandidateSet, Compiled, DeploymentShape, Grant, PolicyLayers, PolicyTest,
    Settings, TestReport,
};
use crate::store;
use crate::toolset::Toolset;

/// The policy engine. Cheap to clone; every clone is the same engine.
#[derive(Clone)]
pub struct Policy {
    inner: Arc<Inner>,
}

struct Inner {
    store: Store,
    /// The key to this crate's tables, which the store hands out once.
    key: PolicyKey,
    profile: DeploymentShape,
    /// The installation's policy settings, from the built-in set the engine
    /// was opened with: in force under any stored set, which can only
    /// tighten them.
    floor: Settings,
    /// The set in force as this process last saw it. `decide` asks the store
    /// for the newest version on every call and loads it when it differs, so
    /// a version published by any process takes effect at the next decision.
    current: Mutex<Current>,
}

#[derive(Clone)]
enum Current {
    /// Nothing was looked at yet, or no version is stored.
    None,
    /// The newest stored version, loaded.
    Loaded(Arc<Compiled>),
    /// The newest stored version cannot be read; every decision is a denial.
    Unreadable(String),
}

impl std::fmt::Debug for Policy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Policy")
            .field("profile", &self.inner.profile)
            .finish_non_exhaustive()
    }
}

/// Whether a valid policy set is loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyHealth {
    /// A valid set is in force.
    Ok {
        /// Its version.
        version: Digest,
    },
    /// No valid set is loaded: every decision is a denial.
    NoValidSet,
}

/// One recorded decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// The decision's identifier. Its row holds the request and the rules.
    pub id: DecisionId,
    /// The question that was asked.
    pub question: Question,
    /// Allow or deny.
    pub answer: Answer,
    /// The policy version that made it; `None` when no valid set was loaded.
    pub version: Option<Digest>,
    /// The rules that determined the answer. For `explain`; never shown to a
    /// refused caller.
    pub rules: Vec<RuleId>,
    /// What a refused caller is told: a code and what to ask for.
    pub refusal: Option<Failure>,
    run: Option<RunId>,
    bound_to_diff: bool,
}

/// What a decision means for the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome<'a> {
    /// It may happen.
    Allow,
    /// The write may not proceed without a person: the run waits at
    /// `needs_approval`.
    NeedsApproval,
    /// It may not happen, and this is what the caller is told.
    Refused(&'a Failure),
}

impl Decision {
    /// Whether the answer is allow.
    #[must_use]
    pub fn allowed(&self) -> bool {
        self.answer == Answer::Allow
    }

    /// What the decision means for the call.
    #[must_use]
    pub fn outcome(&self) -> Outcome<'_> {
        match (&self.answer, &self.refusal) {
            (Answer::Allow, _) => Outcome::Allow,
            (Answer::Deny, Some(refusal)) => Outcome::Refused(refusal),
            (Answer::Deny, None) => Outcome::NeedsApproval,
        }
    }

    /// The clearance under which the run's write phase may begin without an
    /// approval: only for a decision that allowed `WriteWithoutPerson` for a
    /// run and its diff.
    #[must_use]
    pub fn clearance(&self) -> Option<WriteClearance> {
        (self.question == Question::WriteWithoutPerson && self.allowed() && self.bound_to_diff)
            .then_some(WriteClearance::PreApproved(self.id))
    }

    /// The run the decision was made for, where there was one.
    #[must_use]
    pub const fn run(&self) -> Option<RunId> {
        self.run
    }
}

word! {
    /// What a kill switch stops.
    KillScopeKind {
        /// Everything.
        Everything => "everything",
        /// One workspace.
        Workspace => "workspace",
        /// One target.
        Target => "target",
        /// One operation or pipeline.
        Operation => "operation",
    }
}

/// What a kill switch stops.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillScope {
    /// All work on every system.
    Everything,
    /// All work in one workspace.
    Workspace(WorkspaceId),
    /// All work on one target.
    Target(TargetName),
    /// One operation or pipeline.
    Operation(OperationName),
}

impl KillScope {
    fn stored(&self) -> (&'static str, String) {
        match self {
            Self::Everything => (KillScopeKind::Everything.as_str(), String::new()),
            Self::Workspace(id) => (KillScopeKind::Workspace.as_str(), id.to_string()),
            Self::Target(name) => (KillScopeKind::Target.as_str(), name.as_str().to_owned()),
            Self::Operation(name) => (KillScopeKind::Operation.as_str(), name.as_str().to_owned()),
        }
    }
}

/// A change to a kill switch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KillSwitch {
    /// What it stops.
    pub scope: KillScope,
    /// Whether it is set or released.
    pub on: bool,
    /// Who changes it.
    pub by: PrincipalId,
    /// The user-presence check the person passed, where one was made. On one
    /// machine a switch is released only with one.
    pub presence: Option<PresenceCheck>,
}

/// What `check_limits` checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitCheck {
    /// The caller.
    pub principal: PrincipalRef,
    /// The workspace.
    pub workspace: WorkspaceId,
    /// How many records the run holds, once validation knows.
    pub records: Option<u32>,
    /// Whether to count this call against the caller's quotas. True once,
    /// when a call is accepted.
    pub count_call: bool,
}

/// The proof handed in with a policy set to publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PublishProof {
    /// The user-presence check the publisher passed, where one was made. A
    /// set that loosens a protected setting is published only with one.
    pub presence: Option<PresenceCheck>,
}

/// One change to the layers in force, published as a new version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerChange {
    /// Add a grant.
    AddGrant(Grant),
    /// Remove a grant the layers hold.
    RemoveGrant(Grant),
    /// Add a toolset, or replace the one of the same name.
    SetToolset(Toolset),
    /// Remove the toolset of this name.
    RemoveToolset(ToolsetName),
}

/// One version in the history of the set, newest last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    /// The version's digest.
    pub version: Digest,
    /// Who published it; none for the built-in set.
    pub published_by: Option<String>,
    /// The user-presence check that came with it, as its word.
    pub presence: Option<String>,
    /// When it was stored.
    pub published_at: Timestamp,
    /// Whether it is the version in force.
    pub in_force: bool,
}

/// The second administrator of a publish, and who drafted the version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishReview {
    /// The administrator or publisher who reviewed the version and publishes
    /// it with the first. Never an agent, never the publisher, never the
    /// author.
    pub reviewer: PrincipalRef,
    /// The person who drafted the version, where that is known.
    pub drafted_by: Option<PrincipalId>,
}

/// Why a past decision came out as it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    /// The decision.
    pub id: DecisionId,
    /// When it was made.
    pub decided_at: Timestamp,
    /// Who asked, as written in the record.
    pub principal: String,
    /// The question, as written in the record.
    pub question: String,
    /// What it was about.
    pub resource: String,
    /// The answer.
    pub answer: Answer,
    /// The policy version that made it.
    pub version: Option<String>,
    /// The code the caller was given, when refused.
    pub refusal_code: Option<String>,
    /// The rules that determined the answer, each with what it says and the
    /// entry of the set that decided it. It holds no secret: no credential, no
    /// pre-approval condition, and no field of the request beyond what the
    /// record states.
    pub rules: Vec<RuleExplained>,
    /// The whole explanation in sentences a developer can act on.
    pub summary: String,
}

/// One determining rule and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleExplained {
    /// The rule's name in its policy version.
    pub id: String,
    /// Where it comes from: a guardrail, a grant, a toolset, and so on.
    pub kind: RuleKind,
    /// What it says, in words.
    pub says: String,
}

impl Policy {
    /// Opens the policy engine over `store`. A fresh store gets `built_in`
    /// as its first version; otherwise the newest stored version is loaded.
    /// With no valid version every decision is a denial and
    /// [`Policy::health`] says so.
    ///
    /// The second value is the one handle that can record an approval or a
    /// decline. It cannot be cloned or made any other way: hand it to the
    /// approval surface and to nothing else.
    ///
    /// # Errors
    ///
    /// The store's failures; `input`/`schema_invalid` or
    /// `input`/`policy_tests_failed` when `built_in` itself is not a valid
    /// set.
    pub async fn open(
        store: &Store,
        built_in: BuiltIn,
    ) -> Result<(Self, ApprovalSurface), Failure> {
        let profile = built_in.profile();
        let floor = built_in.layers().settings.clone();
        let candidate = CandidateSet::compile(built_in.layers(), profile)?;
        tests_must_pass(&candidate.test())?;
        // The key to this crate's tables comes from the store once: a
        // second opening on the same store is refused, so there is one
        // approval handle and one policy engine per store (decision 0145).
        let key = store.policy_key()?;
        let policy = Self {
            inner: Arc::new(Inner {
                store: store.clone(),
                key,
                profile,
                floor,
                current: Mutex::new(Current::None),
            }),
        };
        let opened = policy.clone();
        store
            .write(Durability::Synced, move |tx| {
                // The built-in set is stored when the store holds none, and
                // again when the newest one is an earlier built-in set: the
                // binary's guardrails or the local user may have changed.
                let newest = match store::newest_version(tx, opened.key())? {
                    Some(version) => store::version(tx, opened.key(), &version)?,
                    None => None,
                };
                if newest.is_none_or(|row| row.built_in) {
                    store::insert_version(
                        tx,
                        opened.key(),
                        &candidate,
                        rules::guardrails_digest(),
                        None,
                        None,
                    )?;
                }
                opened.current(tx);
                Ok(())
            })
            .await?;
        let surface = ApprovalSurface::new(policy.clone());
        Ok((policy, surface))
    }

    pub(crate) fn store(&self) -> &Store {
        &self.inner.store
    }

    pub(crate) fn key(&self) -> &PolicyKey {
        &self.inner.key
    }

    fn cached(&self) -> Current {
        self.inner
            .current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The set in force, read through `tx`: the newest stored version,
    /// loaded once and kept until another is the newest.
    pub(crate) fn current(&self, tx: &mut Tx<'_>) -> Option<Arc<Compiled>> {
        let found = match store::newest_version(tx, self.key()) {
            Ok(Some(newest)) => match self.cached() {
                Current::Loaded(set) if set.version.to_string() == newest => Current::Loaded(set),
                Current::Unreadable(version) if version == newest => Current::Unreadable(version),
                _ => self.load(tx, &newest),
            },
            Ok(None) => Current::None,
            // A store that cannot say which version is in force: no decision
            // may rest on a remembered one.
            Err(_) => return None,
        };
        let loaded = match &found {
            Current::Loaded(set) => Some(Arc::clone(set)),
            Current::None | Current::Unreadable(_) => None,
        };
        *self
            .inner
            .current
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = found;
        loaded
    }

    fn load(&self, tx: &mut Tx<'_>, version: &str) -> Current {
        let loaded = store::version(tx, self.key(), version)
            .ok()
            .flatten()
            .and_then(|row| {
                let digest: Digest = row.version.parse().ok()?;
                let mut set =
                    set::load(digest, &row.layers, &row.tests, self.inner.profile).ok()?;
                // The installation's settings stay in force under a set
                // published later: on each setting, the tighter one holds.
                set.layers.settings = set.layers.settings.tighter(&self.inner.floor);
                Some(set)
            });
        loaded.map_or_else(
            || Current::Unreadable(version.to_owned()),
            |set| Current::Loaded(Arc::new(set)),
        )
    }

    /// Whether a valid set is in force: the newest stored version, read
    /// through `tx`.
    pub fn health(&self, tx: &mut Tx<'_>) -> PolicyHealth {
        self.current(tx)
            .map_or(PolicyHealth::NoValidSet, |set| PolicyHealth::Ok {
                version: set.version,
            })
    }

    /// Decides one request and records the decision in `tx`.
    ///
    /// It never fails: an error while deciding is a recorded denial. The
    /// caller must let `tx` commit whether the answer was allow or deny, by
    /// returning `Ok` from its closure, or the record of a refusal is lost
    /// with it. If the record itself cannot be written, the answer is a
    /// denial with the code `policy_evaluation_failed`.
    pub fn decide(&self, tx: &mut Tx<'_>, request: &Request) -> Decision {
        let (verdict, version) = self.verdict(tx, request);
        let bound_to_diff = request.run.is_some() && request.diff_hash.is_some();
        let recorded = (|| {
            let id = DecisionId::from_parts(tx.now(), tx.random()?)?;
            let text = set::text_of(&CanonicalJson::encode(request)?)?;
            store::insert_decision(
                tx,
                self.key(),
                &id.to_string(),
                request,
                &text,
                &verdict,
                version,
            )?;
            Ok::<_, Failure>(id)
        })();
        let (id, verdict) = match recorded {
            Ok(id) => (id, verdict),
            Err(_) => (
                DecisionId::from_bytes([0; 16]),
                Verdict::deny(rules::EVALUATION_ERROR, failure::evaluation_failed()),
            ),
        };
        Decision {
            id,
            question: request.question,
            answer: verdict.answer,
            version,
            rules: verdict
                .rules
                .iter()
                .map(|rule| RuleId::known(rule))
                .collect(),
            refusal: verdict.refusal,
            run: request.run,
            bound_to_diff,
        }
    }

    /// How hard a decision about `request` may be pushed to disk when its
    /// caller asks for `asked`: relaxed only for a request that reads, is
    /// bound to no diff, and is not an action only an administrator may
    /// perform, and synced otherwise. An administrator's action, such as
    /// setting the kill switch, is synced even where its effect is labelled
    /// a read, so that no caller's request can relax it (decision 0386).
    #[must_use]
    pub fn durability_for(request: &Request, asked: Durability) -> Durability {
        let administrators = matches!(
            &request.resource,
            Resource::Action(action) if action.needs == Role::Admin
        );
        if asked == Durability::Relaxed
            && request.context.effect == a2m_core::EffectClass::Read
            && request.diff_hash.is_none()
            && !administrators
        {
            Durability::Relaxed
        } else {
            Durability::Synced
        }
    }

    /// Decides one request in a transaction of its own, which commits
    /// whether the answer was allow or deny, and is synced to disk before
    /// it returns.
    pub async fn decide_alone(&self, request: Request) -> Decision {
        self.decide_alone_as(request, Durability::Synced).await
    }

    /// [`Policy::decide_alone`], with the commit pushed to disk as hard as
    /// `durability` says. `Durability::Relaxed` is for the decision about a
    /// call of a tool that only reads, which is part of a read's record:
    /// the ledger keeps a read without a disk sync of its own (spec section
    /// 208, Reads). The decision still commits before the answer is given,
    /// survives a crash of the process, and reaches the disk with the next
    /// synced commit or checkpoint, which the write-ahead log keeps in
    /// order. A decision a write rests on is never relaxed: a request whose
    /// effect is not `read`, or that is bound to a diff, is synced whatever
    /// `durability` says.
    pub async fn decide_alone_as(&self, request: Request, durability: Durability) -> Decision {
        let durability = Self::durability_for(&request, durability);
        let policy = self.clone();
        let asked = request.clone();
        let decided = self
            .inner
            .store
            .write(durability, move |tx| Ok(policy.decide(tx, &asked)))
            .await;
        decided.unwrap_or_else(|_| Decision {
            id: DecisionId::from_bytes([0; 16]),
            question: request.question,
            answer: Answer::Deny,
            version: None,
            rules: vec![RuleId::known(rules::EVALUATION_ERROR)],
            refusal: Some(failure::evaluation_failed()),
            run: request.run,
            bound_to_diff: false,
        })
    }

    /// The answer a request would get now, without recording anything: for
    /// "what could this person do?" and "who can call this tool?" asked over
    /// a catalog. Kill switches are consulted as `decide` consults them.
    pub fn would(&self, tx: &mut Tx<'_>, request: &Request) -> Answer {
        self.verdict(tx, request).0.answer
    }

    fn verdict(&self, tx: &mut Tx<'_>, request: &Request) -> (Verdict, Option<Digest>) {
        // Kill switches come first, before any rule.
        let stopped = if request.question.is_work() {
            store::switches_on(tx, self.key()).map(|on| covers(&on, request))
        } else {
            Ok(false)
        };
        let set = self.current(tx);
        let version = set.as_ref().map(|set| set.version);
        let verdict = match (stopped, set) {
            (Ok(true), _) => Verdict::deny(rules::KILL_SWITCH, failure::kill_switch_on()),
            (Ok(false), Some(set)) => rules::evaluate(&set, request),
            (Err(_), _) | (_, None) => {
                Verdict::deny(rules::EVALUATION_ERROR, failure::evaluation_failed())
            }
        };
        (verdict, version)
    }

    /// Checks `layers` and makes a candidate set for this engine's profile.
    ///
    /// # Errors
    ///
    /// As [`CandidateSet::compile`].
    pub fn compile(&self, layers: &PolicyLayers) -> Result<CandidateSet, Failure> {
        CandidateSet::compile(layers, self.inner.profile)
    }

    /// Runs a candidate's own test table.
    #[must_use]
    pub fn test(&self, candidate: &CandidateSet) -> TestReport {
        candidate.test()
    }

    /// Publishes a candidate as the version in force. It takes effect, for
    /// every process on the store, at the first decision after `tx` commits.
    ///
    /// # Errors
    ///
    /// `input`/`policy_tests_failed` when the candidate's test table does not
    /// pass; `policy`/`presence_required` when it loosens a protected setting
    /// and `proof` carries no user-presence check, or no valid set is in
    /// force to compare it with; the store's failures.
    pub fn publish(
        &self,
        tx: &mut Tx<'_>,
        candidate: &CandidateSet,
        by: &PrincipalRef,
        proof: PublishProof,
    ) -> Result<Digest, Failure> {
        self.publish_reviewed(tx, candidate, by, proof, None)
    }

    /// As [`Policy::publish`], with the second administrator a set may need.
    /// A set needs one when the set in force says `second_admin_publish`, so
    /// a candidate cannot switch the need off for itself, and the first set
    /// that turns it on needs none. The reviewer must be a person other than the publisher and
    /// the author, who holds the role of administrator or publisher in the
    /// set in force.
    ///
    /// # Errors
    ///
    /// As [`Policy::publish`]; `policy`/`second_admin_required` when a second
    /// administrator is needed and none, or one who may not, was given.
    pub fn publish_reviewed(
        &self,
        tx: &mut Tx<'_>,
        candidate: &CandidateSet,
        by: &PrincipalRef,
        proof: PublishProof,
        review: Option<&PublishReview>,
    ) -> Result<Digest, Failure> {
        tests_must_pass(&candidate.test())?;
        let in_force = self.current(tx);
        let needs_second = in_force
            .as_ref()
            .is_some_and(|set| set.layers.second_admin_publish);
        if needs_second {
            let roles = in_force.as_ref().map(|set| &set.layers);
            check_review(by, review, roles)?;
        }
        let present = proof.presence.is_some_and(PresenceCheck::proves_presence);
        let loosens = in_force
            .as_ref()
            .is_none_or(|current| candidate.loosens(&current.layers));
        if loosens && !present {
            return Err(failure::presence_required(
                "publishing a policy set that loosens a protected setting",
            ));
        }
        store::insert_version(
            tx,
            self.key(),
            candidate,
            rules::guardrails_digest(),
            Some(by.id),
            proof.presence.map(PresenceCheck::as_str),
        )?;
        Ok(candidate.version())
    }

    /// The layers of the set in force, without its test table, which is
    /// stored and hashed but never read back. `None` when no valid set is in
    /// force.
    pub fn layers_in_force(&self, tx: &mut Tx<'_>) -> Option<PolicyLayers> {
        self.current(tx).map(|set| set.layers.clone())
    }

    /// The history of published versions, oldest first.
    ///
    /// # Errors
    ///
    /// The store's failures.
    pub fn versions(&self, tx: &mut Tx<'_>) -> Result<Vec<VersionInfo>, Failure> {
        let in_force = self.current(tx).map(|set| set.version);
        store::versions(tx, self.key())?
            .into_iter()
            .map(|row| {
                let version: Digest = row.version.parse().map_err(|_| {
                    failure::invalid(
                        "A stored policy version has no readable name.",
                        "Publish a policy set again.",
                    )
                })?;
                Ok(VersionInfo {
                    in_force: in_force == Some(version),
                    version,
                    published_by: row.published_by,
                    presence: row.presence,
                    published_at: row.published_at,
                })
            })
            .collect()
    }

    /// Changes one grant or one toolset of the set in force and publishes the
    /// result as a new version, with `tests` as its test table (the table of
    /// the set in force is never read back, so the caller brings the one the
    /// new version must pass). The rules of [`Policy::publish_reviewed`]
    /// apply.
    ///
    /// # Errors
    ///
    /// `input`/`schema_invalid` when no valid set is in force, the grant or
    /// toolset to remove is not there, or the result does not compile; the
    /// failures of [`Policy::publish_reviewed`].
    pub fn publish_change(
        &self,
        tx: &mut Tx<'_>,
        change: LayerChange,
        tests: Vec<PolicyTest>,
        by: &PrincipalRef,
        proof: PublishProof,
        review: Option<&PublishReview>,
    ) -> Result<Digest, Failure> {
        let Some(mut layers) = self.layers_in_force(tx) else {
            return Err(failure::evaluation_failed());
        };
        let missing = |what: &str| {
            failure::invalid(
                format!("The {what} to remove is not in the set in force."),
                "List what the set holds, and name one that is there.",
            )
        };
        layers = match change {
            LayerChange::AddGrant(grant) => layers.with_grant(grant),
            LayerChange::RemoveGrant(grant) => {
                let (next, removed) = layers.without_grant(&grant);
                if !removed {
                    return Err(missing("grant"));
                }
                next
            }
            LayerChange::SetToolset(toolset) => layers.with_toolset(toolset),
            LayerChange::RemoveToolset(name) => {
                let (next, removed) = layers.without_toolset(&name);
                if !removed {
                    return Err(missing("toolset"));
                }
                next
            }
        };
        layers.tests = tests;
        let candidate = self.compile(&layers)?;
        self.publish_reviewed(tx, &candidate, by, proof, review)
    }

    /// The tools of `catalog` that `principal` may use, each asked as
    /// [`Policy::would`] asks it, with the context the tool brings. The
    /// catalog is the caller's; nothing is recorded.
    pub fn tools_allowed(
        &self,
        tx: &mut Tx<'_>,
        principal: &PrincipalRef,
        catalog: &[(ToolRef, Context)],
    ) -> Vec<ToolRef> {
        catalog
            .iter()
            .filter(|(tool, context)| {
                let request = Request {
                    principal: principal.clone(),
                    question: Question::UseTool,
                    resource: Resource::Tool(tool.clone()),
                    context: context.clone(),
                    run: None,
                    diff_hash: None,
                };
                self.would(tx, &request) == Answer::Allow
            })
            .map(|(tool, _)| tool.clone())
            .collect()
    }

    /// Which of `candidates` may use `tool`: "who can call this tool?".
    pub fn who_can(
        &self,
        tx: &mut Tx<'_>,
        tool: &ToolRef,
        context: &Context,
        candidates: &[PrincipalRef],
    ) -> Vec<PrincipalId> {
        candidates
            .iter()
            .filter(|principal| {
                let request = Request {
                    principal: (*principal).clone(),
                    question: Question::UseTool,
                    resource: Resource::Tool(tool.clone()),
                    context: context.clone(),
                    run: None,
                    diff_hash: None,
                };
                self.would(tx, &request) == Answer::Allow
            })
            .map(|principal| principal.id)
            .collect()
    }

    /// The tools of `catalog` that the toolset `name` selects, for the one
    /// list every caller of the toolset sees. `None` when the set in force
    /// does not define the toolset.
    pub fn toolset_tools(
        &self,
        tx: &mut Tx<'_>,
        name: &ToolsetName,
        catalog: &[ToolRef],
    ) -> Option<Vec<ToolRef>> {
        let set = self.current(tx)?;
        let toolset = set.layers.toolsets.iter().find(|set| set.name == *name)?;
        Some(
            catalog
                .iter()
                .filter(|tool| toolset.selects(tool))
                .cloned()
                .collect(),
        )
    }

    /// Why a past decision came out as it did. Only an auditor or an
    /// administrator may ask; `a2m-actions` decides that before calling.
    ///
    /// # Errors
    ///
    /// `input`/`decision_not_found` when no such decision is recorded in
    /// the caller's `workspace`: a decision of another workspace is not
    /// told apart from one that does not exist. The store's failures.
    pub fn explain(
        &self,
        tx: &mut Tx<'_>,
        id: DecisionId,
        workspace: WorkspaceId,
    ) -> Result<Explanation, Failure> {
        let found = store::decision(tx, self.key(), &id.to_string())?
            .filter(|row| row.workspace == workspace.to_string());
        let Some(row) = found else {
            return Err(Failure::new(
                FailureClass::Input,
                DECISION_NOT_FOUND,
                "No decision with that identifier is recorded.",
                "Check the identifier; it starts with `d_`.",
            ));
        };
        let set = row.version.as_deref().and_then(|version| {
            let stored = store::version(tx, self.key(), version).ok().flatten()?;
            let digest: Digest = stored.version.parse().ok()?;
            set::load(digest, &stored.layers, &stored.tests, self.inner.profile).ok()
        });
        let answer = if row.allowed {
            Answer::Allow
        } else {
            Answer::Deny
        };
        let explained: Vec<RuleExplained> = row
            .reasons
            .iter()
            .map(|rule| RuleExplained {
                id: rule.clone(),
                kind: rules::kind_of(rule),
                says: rules::describe(set.as_ref(), rule),
            })
            .collect();
        let resource = row.resource.unwrap_or_default();
        let mut summary = format!(
            "The question `{}` about `{resource}` was answered `{answer}`",
            row.question
        );
        match &row.version {
            Some(version) => summary.push_str(&format!(" under policy version {version}.")),
            None => summary.push_str(" with no valid policy set loaded."),
        }
        for rule in &explained {
            summary.push_str(&format!(" `{}`: {}", rule.id, rule.says));
        }
        if !row.allowed && row.refusal_code.is_none() {
            summary.push_str(" This is not a refusal: the run waits for a person's approval.");
        }
        Ok(Explanation {
            id,
            decided_at: row.decided_at,
            principal: row.principal,
            question: row.question,
            resource,
            answer,
            version: row.version,
            refusal_code: row.refusal_code,
            rules: explained,
            summary,
        })
    }

    /// Checks the limits beside the decision core: records per run, and the
    /// caller's quotas when `count_call` is set.
    ///
    /// # Errors
    ///
    /// `policy`/`limit_exceeded` over a limit; `policy`/
    /// `policy_evaluation_failed` when no valid set is loaded; the store's
    /// failures.
    pub fn check_limits(&self, tx: &mut Tx<'_>, check: &LimitCheck) -> Result<(), Failure> {
        let Some(set) = self.current(tx) else {
            return Err(failure::evaluation_failed());
        };
        let limit = set.layers.settings.max_records_per_run;
        if check.records.is_some_and(|records| records > limit) {
            return Err(failure::limit_exceeded(&format!(
                "a run may hold at most {limit} records"
            )));
        }
        if !check.count_call {
            return Ok(());
        }
        for quota in &set.layers.quotas {
            if !quota.who.covers(&check.principal) {
                continue;
            }
            let calls = store::count_call(
                tx,
                self.key(),
                check.workspace.to_string(),
                check.principal.id.to_string(),
                quota.period_minutes,
            )?;
            if calls > i64::from(quota.calls) {
                return Err(failure::limit_exceeded(&format!(
                    "the caller's quota is {} calls every {} minutes",
                    quota.calls, quota.period_minutes
                )));
            }
        }
        Ok(())
    }

    /// Sets or releases a kill switch and gives the new kill epoch. A switch
    /// that is set stops, from the next decision, every question about work
    /// it covers.
    ///
    /// # Errors
    ///
    /// `policy`/`presence_required` when a switch is released on one machine
    /// without a user-presence check; the store's failures.
    pub fn set_kill_switch(&self, tx: &mut Tx<'_>, switch: &KillSwitch) -> Result<u64, Failure> {
        let present = switch.presence.is_some_and(PresenceCheck::proves_presence);
        if !switch.on && self.inner.profile == DeploymentShape::Local && !present {
            return Err(failure::presence_required("releasing a kill switch"));
        }
        let (scope, name) = switch.scope.stored();
        store::set_switch(
            tx,
            self.key(),
            scope,
            name,
            switch.on,
            switch.by,
            switch.presence.map(PresenceCheck::as_str),
        )
    }

    /// The kill epoch: a number that rises with every change to a kill
    /// switch, sent to runners so they can tell that one happened.
    ///
    /// # Errors
    ///
    /// The store's failures.
    pub fn kill_epoch(&self, tx: &mut Tx<'_>) -> Result<u64, Failure> {
        store::kill_epoch(tx, self.key())
    }
}

fn tests_must_pass(report: &TestReport) -> Result<(), Failure> {
    if report.ok() {
        return Ok(());
    }
    let names: Vec<&str> = report
        .failed
        .iter()
        .map(|test| test.name.as_str())
        .collect();
    Err(Failure::new(
        FailureClass::Input,
        POLICY_TESTS_FAILED,
        format!(
            "The policy set fails {} of its own tests: {}.",
            report.failed.len(),
            names.join("; ")
        ),
        "Correct the set or its tests; a version is published only when its test table passes.",
    ))
}

/// Whether a kill switch that is on covers the request.
fn covers(on: &[(String, String)], request: &Request) -> bool {
    let tool = match &request.resource {
        crate::model::Resource::Tool(tool) => Some((tool.target.as_str(), tool.tool.as_str())),
        _ => None,
    };
    covers_parts(on, &request.context.workspace.to_string(), tool)
}

/// Whether a switch that is on covers work in `workspace` on a tool, named
/// by its target and its full name.
pub(crate) fn covers_parts(
    on: &[(String, String)],
    workspace: &str,
    tool: Option<(&str, &str)>,
) -> bool {
    on.iter()
        .any(|(scope, name)| match KillScopeKind::from_text(scope) {
            Some(KillScopeKind::Everything) => true,
            Some(KillScopeKind::Workspace) => name == workspace,
            Some(KillScopeKind::Target) => tool.is_some_and(|(target, _)| target == name),
            Some(KillScopeKind::Operation) => tool.is_some_and(|(_, tool)| tool == name),
            // A switch this engine cannot read stops everything.
            None => true,
        })
}

/// Whether `review` lets `by` publish: a person other than `by` and the
/// author, who holds the role of administrator or publisher in the set in
/// force. An agent never publishes or reviews a publish.
fn check_review(
    by: &PrincipalRef,
    review: Option<&PublishReview>,
    in_force: Option<&PolicyLayers>,
) -> Result<(), Failure> {
    use crate::model::{PrincipalKind, Role};
    let Some(review) = review else {
        return Err(failure::second_admin_required("none was given"));
    };
    let reviewer = &review.reviewer;
    if by.kind == PrincipalKind::Agent || reviewer.kind != PrincipalKind::Person {
        return Err(failure::second_admin_required(
            "an agent can neither publish nor review a publish",
        ));
    }
    if reviewer.id == by.id {
        return Err(failure::second_admin_required(
            "the reviewer is the publisher",
        ));
    }
    if review.drafted_by == Some(reviewer.id) {
        return Err(failure::second_admin_required("the reviewer drafted it"));
    }
    let may_review = in_force.is_some_and(|layers| {
        layers.roles.iter().any(|binding| {
            matches!(binding.role, Role::Admin | Role::Publisher) && binding.who.covers(reviewer)
        })
    });
    if may_review {
        Ok(())
    } else {
        Err(failure::second_admin_required(
            "the reviewer holds neither the administrator nor the publisher role",
        ))
    }
}
