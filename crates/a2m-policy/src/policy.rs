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
use crate::model::{PrincipalRef, Question, Request, RuleId, word};
use crate::rules::{self, Answer, Verdict};
use crate::set::{
    self, BuiltIn, CandidateSet, Compiled, PolicyLayers, Profile, Settings, TestReport,
};
use crate::store;

/// The policy engine. Cheap to clone; every clone is the same engine.
#[derive(Clone)]
pub struct Policy {
    inner: Arc<Inner>,
}

struct Inner {
    store: Store,
    /// The key to this crate's tables, which the store hands out once.
    key: PolicyKey,
    profile: Profile,
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
    /// The rules that determined the answer, each with what it says.
    pub rules: Vec<RuleExplained>,
    /// The whole explanation in sentences a developer can act on.
    pub summary: String,
}

/// One determining rule and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleExplained {
    /// The rule's name in its policy version.
    pub id: String,
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

    /// Decides one request in a transaction of its own, which commits
    /// whether the answer was allow or deny.
    pub async fn decide_alone(&self, request: Request) -> Decision {
        let policy = self.clone();
        let asked = request.clone();
        let decided = self
            .inner
            .store
            .write(Durability::Synced, move |tx| Ok(policy.decide(tx, &asked)))
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
        tests_must_pass(&candidate.test())?;
        let present = proof.presence.is_some_and(PresenceCheck::proves_presence);
        let loosens = self
            .current(tx)
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
        if !switch.on && self.inner.profile == Profile::Local && !present {
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
