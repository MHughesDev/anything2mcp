# `a2m-policy`

The one decider. It answers every question of the form "may this happen?" with a plain allow or deny, records each answer with the rules that made it, and keeps the records that only a person can create: approvals, consent, and delegations. No other crate holds a grant or decides.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 4, Services |
| May depend on | Layers 1 to 3 |
| Depends on | `a2m-core`; `a2m-ledger` for its tables; `a2m-expr` for pre-approval conditions; `a2m-telemetry`. Outside: the Cedar crate named by [decision 0033](../../decisions/0033-policy-decisions-use-cedar-with-grants-compiled-to-it.md), its licence and size still to be verified by research brief 31 |
| Built in | Phase 3, group B and slice 3.2: the built-in local policy set, guardrails, limits, kill switches, approvals. Slice 3.6: approvals from the approval surface. Phase 6: grants, roles, published policy sets, standing approvals, chat and console channels. Phase 7: delegations |
| Carries out | Spec sections [212](../../spec/2-kernel/212-policy-and-approvals.md), [214](../../spec/2-kernel/214-access-control-toolsets-and-tenancy.md), [212](../../spec/2-kernel/212-policy-and-approvals.md), [503](../../spec/5-security/503-approval-and-sign-in-surfaces.md) (what an approval records), [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) (an agent cannot loosen its limits), [213](../../spec/2-kernel/213-effects-hazards-and-consent.md) (hazards raise the approval; consent); decisions 0033, 0034, and [0035](../../decisions/0035-a-user-s-session-is-used-only-by-that-user-and-agents-acting-for-them.md) |

## Owns

- `decide`: the seven questions of section 212, each one request in and one recorded decision out.
- The policy set: built-in guardrails, the layers written by administrators, grants and roles compiled to Cedar, the schema that every policy is validated against, and the test table that must pass before a version is published.
- The limiter beside the decision core: records per run, quotas, kill switches.
- Approvals: single-use records bound to a diff hash, and their consumption at the start of a write phase. Standing approvals.
- Consent records, and delegations from a person to an agent.
- Explaining a past decision, and answering "who can" and "what could" questions over a catalog.

It must never: let a caller supply any part of the context; treat an error as anything but a denial; let a permit outweigh a forbid; let any setting remove a guardrail; record an approval or a consent that a person did not give through a channel the caller cannot operate; reveal the content of a rule to a denied caller.

## Interface

```rust
pub struct Policy { /* the current set, swapped atomically; the limiter; the store */ }
impl Policy {
    /// Loads the newest valid version. With none, every decision is a denial and health says so.
    pub async fn open(store: &Store, built_in: BuiltIn) -> Result<Policy, Failure>;

    /// Never fails: an error while deciding is a recorded denial. The decision row is written in
    /// `tx`; the caller commits it whether the answer was allow or deny.
    pub async fn decide(&self, tx: &mut Tx<'_>, req: Request) -> Decision;
    /// The same, in a transaction of its own, for callers with nothing else to commit.
    pub async fn decide_alone(&self, req: Request) -> Decision;

    pub fn health(&self) -> PolicyHealth;                 // Ok { version } | NoValidSet
    pub fn kill_epoch(&self) -> u64;                       // rises with every kill switch change; sent to runners
}

pub struct Request { pub principal: PrincipalRef, pub question: Question, pub resource: Resource, pub context: Context }
pub enum Question { UseTool, ActAs, WriteWithoutPerson, Approve, SeeLabel, SetOption, PerformAction }
pub enum Resource { Tool(ToolRef), Action(ActionName), Identity(IdentityRef), Label(Sensitivity), Run(RunId), Option(OptionName) }
/// Built only by a2m-actions, from verified facts and from what the engine reports through Control. Never deserialized from a caller.
pub struct Context {
    pub effect: EffectClass, pub hazard: Hazard, pub records: Option<u32>,
    pub workspace: WorkspaceId, pub toolset: Option<ToolsetRef>, pub environment: Environment,
    pub identity_mode: Option<IdentityMode>, pub session_owner: Option<PersonRef>,
    pub caller_is_agent: bool, pub acting_for: Option<ActingFor>, pub client_label: String, pub at: Instant,
}
pub enum ActingFor { LocalUser(PersonRef), TokenSubject(PersonRef), Delegation(DelegationId) }

pub struct Decision { pub id: DecisionId, pub answer: Answer, pub version: Digest, pub rules: Vec<RuleId>, pub refusal: Option<Failure> }
pub enum Answer { Allow, Deny }

// Approvals. Recorded only by an approval surface, after the person has proved presence.
pub struct Approved {
    pub run: RunId, pub diff_hash: Digest, pub tool_version: Digest,
    pub approver: PersonRef, pub channel: Channel, pub presence: PresenceCheck,
}
pub enum Channel { LocalPrompt, ApprovalPage, ChatCard, UrlElicitation }
impl Policy {
    pub async fn record_approval(&self, tx: &mut Tx<'_>, a: Approved) -> Result<ApprovalId, Failure>;
    pub async fn record_decline(&self, tx: &mut Tx<'_>, run: RunId, by: PersonRef, channel: Channel) -> Result<(), Failure>;
    /// Marks the approval used and returns the clearance a2m-ledger's begin_writes checks, in the same tx.
    pub async fn consume(&self, tx: &mut Tx<'_>, run: RunId, diff_hash: &Digest) -> Result<WriteClearance, Failure>;
    pub async fn grant_standing(&self, tx: &mut Tx<'_>, s: StandingApproval) -> Result<ApprovalId, Failure>;
}

// Consent and delegation: statements by a person, never inferred, never on a person's behalf.
pub enum ConsentAct { InspectTarget, Capture, InstallBridge, AttachProcess, RunTargetCode, OpenDeviceLink, EnableDeclaredProvider }
impl Policy {
    pub async fn record_consent(&self, tx: &mut Tx<'_>, c: Consent) -> Result<ConsentId, Failure>;
    pub async fn consent_for(&self, tx: &mut Tx<'_>, act: ConsentAct, subject: &ConsentSubject) -> Result<Option<Consent>, Failure>;
    pub async fn delegate(&self, tx: &mut Tx<'_>, d: Delegation) -> Result<DelegationId, Failure>;
    pub async fn revoke_delegation(&self, tx: &mut Tx<'_>, id: DelegationId) -> Result<(), Failure>;
}

// Policy sets, grants, limits.
pub struct Grant { pub principal: PrincipalRef, pub scope: Scope, pub max_effect: EffectClass, pub identity_mode: IdentityMode }
pub enum Scope { Target(TargetName), Module(TargetName, String), Pattern(String) }
impl Policy {
    pub fn compile(&self, layers: &PolicyLayers) -> Result<CandidateSet, Failure>;   // grants to permits; guardrails added
    pub fn test(&self, c: &CandidateSet) -> TestReport;                                // policy.test
    pub async fn publish(&self, tx: &mut Tx<'_>, c: CandidateSet, by: PrincipalRef, proof: PublishProof) -> Result<Digest, Failure>;
    pub async fn explain(&self, id: DecisionId) -> Result<Explanation, Failure>;         // policy.explain
    pub async fn check_limits(&self, tx: &mut Tx<'_>, l: LimitCheck) -> Result<(), Failure>;
    pub async fn set_kill_switch(&self, tx: &mut Tx<'_>, k: KillSwitch) -> Result<(), Failure>;
}
```

Contracts that the signatures do not show:

- **The order of evaluation is section 212's, and kill switches come first.** `decide` consults the kill switches before the decision core on every question, so a switch needs no separate call.
- **Guardrails are in every set.** `compile` appends the built-in forbids, which are Cedar text compiled into the binary with their own hash. A `CandidateSet` cannot be built without them, so no published version lacks them. The guardrails are the seven of section 212, plus the hazard rules of section 213: any hazard other than `none` forbids `WriteWithoutPerson`; `irreversible`, `device_function`, and `physical` forbid the use of a standing approval.
- **Cedar's skipped policy is a denial.** The decision core runs the Cedar validator before publishing, and at decision time any error reported with an answer turns it into a deny with the code `policy_evaluation_failed`. If research brief 31 rules Cedar out, an in-house matcher with the same two semantics sits behind the same `decide`, and nothing outside this crate changes.
- **Approval binds and is used once.** `record_approval` refuses unless the approver is a person and not an agent identity, the question `Approve` allows them for this run, the diff hash equals the one stored on the run, and the run is waiting at `needs_approval`. Where policy names an approver, the caller's own confirmation is refused. `consume` refuses an expired, used, or revoked approval, or one for another diff, and marks it used in the caller's `tx`, which also carries `a2m-ledger`'s `begin_writes`. One transaction, so rule 3 of section 208 holds.
- **There is no approve action.** `record_approval` is called by an approval surface: the loopback listener in `a2m-local` after the operating system's prompt, or a server's console page after the approver signs in. `a2m-actions` passes the surface a handle that can reach these calls and nothing else. No command, MCP tool, REST route, or SDK method is generated for them.
- **Silence is a decline.** An approval window that ends is recorded as a decline by nobody, and the run ends `expired`.
- **A session belongs to its owner.** `ActAs` on a resource whose `session_owner` is a person is denied unless the principal is that person or `acting_for` names them through one of the three cases of section 215. A delegation counts only while it is unrevoked and unexpired, and only for the toolset it names.
- **Policy only narrows.** A lower layer's policy is compiled as a further restriction; `compile` refuses a target file, toolset, or workspace layer that contains a permit wider than its parent's.
- **On one machine,** the built-in set gives the operating-system user every role, requires approval for writes, and holds the guardrails. `publish` of a tighter local set needs a `PublishProof` carrying the user-presence check, because the approval rules and record limits are protected settings. A looser local set cannot be compiled.
- **A denial says what to ask for, not why.** The `refusal` carries a code from the table below and a next step; the determining rules are kept for `policy.explain`, which only an auditor or administrator may run.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-actions` | `decide` at every dispatch; approvals, consent, delegations, grants, publishing, explaining, for the actions `grant.add`, `policy.publish`, `policy.test`, `policy.explain`, `killswitch.set`, `delegation.grant`, and the approval surface |
| `a2m-actions` as the in-process `Control`, and `a2m-runner` on the control-plane side of the runner link | On the engine's behalf: `decide` at guard, before the write phase and on resume; `consume`; `check_limits`. The engine never names this crate |
| `a2m-sched` | `kill_epoch`, to stop handing out work and to tell runners |
| `a2m-local`, `a2m-api` | `record_approval`, `record_decline`, through the handle `a2m-actions` gives the approval surface; the person is reached through `PersonSurface` |

It uses `a2m-ledger` for its tables, `a2m-expr` to evaluate the conditions of pre-approval rules over the context, and `a2m-telemetry` for decision timing.

## State

Tables, through `a2m-ledger`: `principals`, `grants`, `policy_versions`, `decisions`, `approvals`, `consents`, `delegations`, `kill_switches`, `quota_counters`. In memory: the current compiled set behind an atomic swap, and a small cache of principals' groups with the bound of section 212.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `policy` | `not_granted` | No permit applies to the caller, the tool, or the action |
| `policy` | `identity_not_allowed` | `ActAs` is denied, including every attempt to use another person's session |
| `policy` | `effect_too_high` | A grant exists, with a lower highest effect class |
| `policy` | `approval_missing` | A write reaches the write phase with no usable approval |
| `policy` | `approval_stale` | The approval is expired, used, or for another diff |
| `policy` | `limit_exceeded` | Records per run or a quota |
| `policy` | `kill_switch_on` (new) | A kill switch covers the request |
| `policy` | `policy_evaluation_failed` (new) | Cedar reported an error, or no valid set is loaded |
| `input` | `schema_invalid` | A candidate set fails validation, or a lower layer widens a higher one |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Every action's dispatch records a decision, allow or deny | A test in `a2m-actions` over the whole registry |
| No setting, file, or layer removes a guardrail | A test that tries each through `compile` and through the local protected settings |
| An agent identity can never approve | A guardrail test, and a test that calls `record_approval` with one |
| An approval is used once, and only for its diff | Rule 3 of section 208, tested together with `a2m-ledger` |
| An evaluation error is a denial | A test with a policy that raises an error at evaluation, planted past the validator |
| The same request on the same version gives the same answer | A property test over random requests |
| A version that fails its test table is not published | A test that publishes one that lets a viewer write |
| A decision is under 100 microseconds on a warm cache | A benchmark, from slice 3.2; a target until measured, hypothesis H-11 |

## Build order

1. The built-in local set: guardrails and "writes need approval", `decide` for `UseTool`, `WriteWithoutPerson`, and `PerformAction`, recorded decisions, with a stub approval. Slice 3.2. Cedar or the fallback is chosen first, by research brief 31.
2. `check_limits` and kill switches. Phase 3, group B.
3. `record_approval`, `consume`, and `record_decline`, joined with `a2m-ledger`'s rule 3. Slices 3.3 and 3.6.
4. Protected settings on one machine.
5. Grants, roles, layers, publishing with tests, `policy.explain`. Phase 6.
6. Standing approvals; the server channels; delegations and `ActAs` with a delegation. Phases 6 and 7.
7. Consent records, with the first act that needs one: inspecting a target on a server, before `scout.run` there.

## Open points

- Cedar's binary size, decision time, and whether its partial evaluation can list the tools a caller may use. Research brief 31; hypothesis H-11.
- Whether every read's decision is stored in full, or summarized at scale as decision 0034 allows.
- Whether consent records belong here or with the scout. They are here because only the decider should ask whether one exists.
- The shape of the presence proof that `a2m-local` hands in, once research brief 27 says which prompts each operating system offers.
