# `a2m-policy`

The one decider. It answers every question of the form "may this happen?" with a plain allow or deny, records each answer with the rules that made it, and keeps the records that only a person can create: approvals, consent, and delegations. No other crate holds a grant or decides.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 4, Services |
| May depend on | Layers 1 to 3 |
| Depends on | `a2m-core`; `a2m-ledger` for its tables and for the run an approval is about; `a2m-expr` for pre-approval conditions. No outside crate beyond `serde` and `serde_json`: the decision core is this crate's own evaluator ([decision 0082](../../decisions/0082-the-decision-core-is-the-policy-crates-own-evaluator.md)) |
| Roadmap packages | Phase 3, group B and slice 3.2: the built-in local policy set, guardrails, limits, kill switches, approvals. Slice 3.6: approvals from the approval surface. Phase 6: grants, roles, published policy sets, standing approvals, chat and console channels. Phase 7: delegations |
| Carries out | Spec sections [212](../../spec/2-kernel/212-policy-and-approvals.md), [214](../../spec/2-kernel/214-access-control-toolsets-and-tenancy.md), [226](../../spec/2-kernel/226-configuration.md) (the protected policy settings), [503](../../spec/5-security/503-approval-and-sign-in-surfaces.md) (what an approval records), [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) (an agent cannot loosen its limits), [213](../../spec/2-kernel/213-effects-hazards-and-consent.md) (hazards raise the approval; consent); decisions 0082, 0083, 0034, and [0035](../../decisions/0035-a-user-s-session-is-used-only-by-that-user-and-agents-acting-for-them.md) |

## Owns

- `decide`: the seven questions of section 212, each one request in and one recorded decision out.
- The policy set: built-in guardrails, the layer written by administrators (roles, grants, label and option rules, pre-approval rules, quotas, the protected settings), and the test table that must pass before a version is published.
- The limiter beside the decision core: records per run, quotas, kill switches.
- Approvals: single-use records bound to a diff hash, and their consumption at the start of a write phase.
- Explaining a past decision, and answering "who can" and "what could" questions without recording.
- Toolsets and role grants: a toolset is a lower layer that only narrows, a role grant is a grant given to a role, and a change to one grant or toolset is published as a new version ([decision 0328](../../decisions/0328-a-toolset-only-narrows-a-role-is-a-bundle-of-grants-and-a-second-administrator-is-a-named-check.md)).
- Not built yet: standing approvals, consent records, and delegations from a person to an agent.

It must never: let a caller supply any part of the context; treat an error as anything but a denial; let a permit outweigh a forbid; let any setting remove a guardrail; record an approval that a person did not give through a channel the caller cannot operate; reveal the content of a rule to a denied caller.

## Interface

The store hands a transaction to a closure on its own thread, so every call that reads or writes a table is a plain function that takes `&mut Tx`. The two `async` conveniences run a transaction of their own.

```rust
#[derive(Clone)]
pub struct Policy { /* the store; the profile; the set in force as last read */ }
impl Policy {
    /// A fresh store gets `built_in` as its first version. The second value is the one
    /// handle that can record an approval or a decline.
    pub async fn open(store: &Store, built_in: BuiltIn) -> Result<(Policy, ApprovalSurface), Failure>;

    /// Never fails: an error while deciding is a recorded denial. The decision row is written
    /// in `tx`; the caller lets it commit whether the answer was allow or deny.
    pub fn decide(&self, tx: &mut Tx<'_>, req: &Request) -> Decision;
    /// The same, in a transaction of its own, which always commits.
    pub async fn decide_alone(&self, req: Request) -> Decision;
    /// The answer a request would get, recording nothing.
    pub fn would(&self, tx: &mut Tx<'_>, req: &Request) -> Answer;

    pub fn health(&self, tx: &mut Tx<'_>) -> PolicyHealth;            // Ok { version } | NoValidSet
    pub fn kill_epoch(&self, tx: &mut Tx<'_>) -> Result<u64, Failure>; // rises with every kill switch change
}

pub struct Request {
    pub principal: PrincipalRef, pub question: Question, pub resource: Resource, pub context: Context,
    pub run: Option<RunId>, pub diff_hash: Option<Digest>,
}
pub struct PrincipalRef { pub id: PrincipalId, pub kind: PrincipalKind, pub groups: Vec<GroupName> }
pub enum PrincipalKind { Person, Agent, Program }
pub enum Question { UseTool, ActAs, WriteWithoutPerson, Approve, SeeLabel, SetOption, PerformAction }
pub enum Resource {
    Tool(ToolRef),          // target, module, tool name
    Action(ActionRef),      // the action's name and the role its registry entry says it needs
    Identity(Identity),     // a person's identity is that person's session
    Label(Sensitivity),
    Run(RunRef),            // the run and who started it
    Option(OptionName),
}
/// Built only by a2m-actions, from verified facts. It can be written and never deserialized.
pub struct Context {
    pub effect: EffectClass, pub hazard: Hazard, pub records: Option<u32>,
    pub workspace: WorkspaceId, pub toolset: Option<ToolsetName>, pub environment: Environment,
    pub identity_mode: Option<IdentityMode>, pub acting_for: Option<ActingFor>,
    pub caller_is_agent: bool, // true for every caller of the local channel, MCP, and an SDK; in one
                               // process the process's view of its terminal: a claim, never proof of a
                               // person, which only the presence prompt gives (decision 0158)
    pub test_records: bool, // every record of the run is a declared test record of a test environment
}
pub enum ActingFor { LocalUser(PrincipalId), TokenSubject(PrincipalId), Delegation(PrincipalId) }

pub struct Decision {
    pub id: DecisionId, pub question: Question, pub answer: Answer,
    pub version: Option<Digest>, pub rules: Vec<RuleId>, pub refusal: Option<Failure>,
}
pub enum Answer { Allow, Deny }
pub enum Outcome<'a> { Allow, NeedsApproval, Refused(&'a Failure) }
impl Decision {
    pub fn outcome(&self) -> Outcome<'_>;
    /// `WriteClearance::PreApproved`, only for an allowed `WriteWithoutPerson` bound to a run and a diff.
    pub fn clearance(&self) -> Option<WriteClearance>;
}

// Approvals. Recorded only through the handle `Policy::open` returns.
pub struct ApprovalSurface { /* not Clone; no public constructor */ }
pub struct Approved {
    pub run: RunId, pub diff_hash: Digest, pub tool_version: Digest,
    pub approver: PrincipalRef, pub channel: Channel, pub presence: PresenceCheck,
    pub proved_at: Timestamp, pub prompt: Option<String>,
}
pub struct Declined { pub run: RunId, pub by: Option<PrincipalRef>, pub channel: Channel }
pub enum Channel { LocalPrompt, ApprovalPage, ChatCard, UrlElicitation }
pub enum PresenceCheck { OsPrompt, Polkit, LocalAuthentication, WindowsHello, SignedIn, NotAvailable }
impl ApprovalSurface {
    pub fn record_approval(&self, tx: &mut Tx<'_>, a: &Approved) -> Result<ApprovalId, Failure>;
    pub fn record_decline(&self, tx: &mut Tx<'_>, d: &Declined) -> Result<(), Failure>;
    pub async fn approve(&self, a: Approved) -> Result<ApprovalId, Failure>;   // own transaction
    pub async fn decline(&self, d: Declined) -> Result<(), Failure>;           // own transaction
}
impl Policy {
    /// Marks the approval used and returns the clearance a2m-ledger's begin_writes checks, in the same tx.
    pub fn consume(&self, tx: &mut Tx<'_>, run: RunId, diff_hash: &Digest) -> Result<WriteClearance, Failure>;
    pub fn declined(&self, tx: &mut Tx<'_>, run: RunId) -> Result<bool, Failure>;
}

// Policy sets, limits, kill switches.
pub struct PolicyLayers {
    pub roles: Vec<RoleBinding>, pub grants: Vec<Grant>, pub labels: Vec<LabelGrant>,
    pub options: Vec<OptionGrant>, pub pre_approvals: Vec<PreApproval>, pub quotas: Vec<Quota>,
    pub settings: Settings, pub tests: Vec<PolicyTest>,
}
pub enum Subject { Principal(PrincipalId), Group(GroupName), Anyone }
pub struct Grant { pub who: Subject, pub scope: Scope, pub max_effect: EffectClass, pub identity_mode: IdentityMode }
pub enum Scope { Target(TargetName), Module(TargetName, ModuleName), Pattern(String) }
/// A role is a bundle of grants: everyone who holds `role` holds this permit.
pub struct RoleGrant { pub role: Role, pub scope: Scope, pub max_effect: EffectClass, pub identity_mode: IdentityMode }
/// A lower layer that only narrows. It holds no permit; once a set defines toolsets, a call through
/// a toolset the set does not define is refused. `PolicyLayers` also carries `role_grants`,
/// `toolsets`, and `second_admin_publish` (all absent from the stored text when unused).
pub struct Toolset {
    pub name: ToolsetName, pub who: Vec<Subject>, pub tools: Vec<Scope>,
    pub max_effect: EffectClass, pub identity_mode: IdentityMode, pub allow_pre_approval: bool,
}
pub struct Settings {
    pub approve_writes: ApproveWrites, pub max_records_per_run: u32, pub approval_window_minutes: u32,
    pub agent_effect_ceiling: EffectClass, pub writes_outside_tests: bool,
    pub two_person_irreversible: bool, pub pre_approval: bool,
}
impl Settings { pub fn tighter(&self, other: &Settings) -> Settings; pub fn looser_than(&self, old: &Settings) -> bool; }
impl BuiltIn {
    pub fn local(user: PrincipalId, workspace: WorkspaceId) -> BuiltIn; pub fn server() -> BuiltIn;
    pub fn with_settings(self, s: Settings) -> BuiltIn; // the installation's settings, still the built-in set
}
impl CandidateSet {
    pub fn compile(layers: &PolicyLayers, profile: Profile) -> Result<CandidateSet, Failure>;
    pub fn test(&self) -> TestReport;
    pub fn loosens(&self, current: &PolicyLayers) -> bool;
}
impl Policy {
    pub fn compile(&self, layers: &PolicyLayers) -> Result<CandidateSet, Failure>;
    pub fn test(&self, c: &CandidateSet) -> TestReport;                                   // policy.test
    pub fn publish(&self, tx: &mut Tx<'_>, c: &CandidateSet, by: &PrincipalRef, proof: PublishProof) -> Result<Digest, Failure>;
    pub fn explain(&self, tx: &mut Tx<'_>, id: DecisionId, workspace: WorkspaceId) -> Result<Explanation, Failure>; // policy.explain
    /// `publish` with a second administrator, where the set in force asks for one.
    pub fn publish_reviewed(&self, tx: &mut Tx<'_>, c: &CandidateSet, by: &PrincipalRef, proof: PublishProof, review: Option<&PublishReview>) -> Result<Digest, Failure>;
    /// One grant or one toolset added or removed, published as a new version with the tests the caller brings.
    pub fn publish_change(&self, tx: &mut Tx<'_>, change: LayerChange, tests: Vec<PolicyTest>, by: &PrincipalRef, proof: PublishProof, review: Option<&PublishReview>) -> Result<Digest, Failure>;
    pub fn layers_in_force(&self, tx: &mut Tx<'_>) -> Option<PolicyLayers>;           // without its tests
    pub fn versions(&self, tx: &mut Tx<'_>) -> Result<Vec<VersionInfo>, Failure>;     // the history, oldest first
    /// Questions over a catalog the caller supplies; nothing is recorded.
    pub fn tools_allowed(&self, tx: &mut Tx<'_>, who: &PrincipalRef, catalog: &[(ToolRef, Context)]) -> Vec<ToolRef>;
    pub fn who_can(&self, tx: &mut Tx<'_>, tool: &ToolRef, context: &Context, candidates: &[PrincipalRef]) -> Vec<PrincipalId>;
    pub fn toolset_tools(&self, tx: &mut Tx<'_>, name: &ToolsetName, catalog: &[ToolRef]) -> Option<Vec<ToolRef>>;
    pub fn check_limits(&self, tx: &mut Tx<'_>, l: &LimitCheck) -> Result<(), Failure>;
    pub fn set_kill_switch(&self, tx: &mut Tx<'_>, k: &KillSwitch) -> Result<u64, Failure>;
}
pub enum KillScope { Everything, Workspace(WorkspaceId), Target(TargetName), Operation(OperationName) }
```

Contracts that the signatures do not show:

- **The order of evaluation is section 212's, and kill switches come first.** `decide` consults the kill switches before the decision core, so a switch needs no separate call. A switch stops the questions about work on a system: `UseTool`, `ActAs`, `WriteWithoutPerson`, and `SetOption`. It does not stop `PerformAction`, `Approve`, or `SeeLabel`: a switch must stay releasable, an approval is checked again before the write, and a result already read is still masked.
- **Two semantics, and nothing else.** A request is denied unless a permit applies, and one matching forbid denies it whatever permits apply. The forbids are the guardrails; the permits are a set's grants, roles, label and option rules, and pre-approval rules.
- **Guardrails are in every set.** They are functions over typed facts, compiled into the binary, with a hash over their names, their wording, and a revision number that is raised when their logic changes. A `CandidateSet` is made only by `compile`, which stamps that hash into the version, and `decide` consults the guardrails before any permit whatever the set holds. A set's data cannot name a guardrail, so no setting, file, or layer can remove one. Built: only a person approves; where an approver is named, the caller's own confirmation does not count; an `irreversible` operation and any hazard other than `none` forbid `WriteWithoutPerson`; a person's session is used only by that person or a caller proved to act for them.
- **An error is a denial.** The only thing that can fail while deciding is a pre-approval rule's condition, an expression. A condition is checked when the set is compiled; one that still fails when it runs denies the request with the code `policy_evaluation_failed`. So does a store that cannot say which version is in force, a newest version that cannot be read, and a decision that cannot be recorded. A new version that fails to compile or to pass its tests is never stored, so the version before stays in force; a newest stored version that cannot be read is not passed over for an older one, which may be looser ([decision 0352](../../decisions/0352-the-faults-of-the-kernels-own-parts-as-proven-and-what-a-verified-record-keeps.md)).
- **The version in force is the newest one stored.** `decide` asks the store for the newest version on every call and loads it when it differs from the one in memory, so a version published by any process takes effect at the next decision, and a publish that was rolled back never does.
- **A pre-approval is bound to one run and one diff.** `WriteWithoutPerson` is allowed only for a request that names a run and its diff hash. The decision row carries both, and the ledger checks them when writes begin.
- **Approval binds and is used once.** `record_approval` reads the run from the ledger, decides and records `Approve` for the approver, and refuses unless the approver is a person, that decision allows, the run is waiting at `needs_approval`, the diff hash and the tool version equal the run's, the proof of presence was made within two minutes of the store's clock and not ahead of it, and the check proves presence wherever the channel is not the engine's own prompt. Where `two_person_irreversible` holds and the run is irreversible, a second approval by the same person is refused. `consume` refuses under a kill switch that covers the run and for a run that was declined, whatever was decided before it; refuses an expired, used, or revoked approval, or one for another diff; needs two approvals of two people where two are asked for; and marks what it uses in the caller's `tx`, which also carries `a2m-ledger`'s `begin_writes`. The ledger reads the approval again and refuses one that was not marked used in its own transaction, so rule 3 of section 208 holds (decision 0145).
- **Only the policy engine writes its tables.** `Policy::open` takes the store's `PolicyKey`, which the store hands out once, and every statement of this crate goes through `Tx::policy_tables` with it. A second `Policy::open` on the same store is refused, so there is one approval handle per store and no second opening can put another built-in set in force. What remains: a crate that depends on the ledger and is handed the key could still write these tables; the kernel hands it to this crate alone, and a test in `a2m-ledger` holds that no other crate's source names it (decision 0145).
- **The installation's settings stay in force.** `BuiltIn::with_settings` carries the installation's policy settings into the built-in set. The policy engine keeps them as a floor: under any set published later, each setting in force is the tighter of the set's and the installation's. `agent_effect_ceiling`, `writes_outside_tests`, and `pre_approval` are forbids that follow the settings in force (`limit.agent_effect_ceiling`, `limit.writes_only_to_test_records`, `limit.no_pre_approval`), hashed with the guardrails; `two_person_irreversible` is kept by `record_approval` and `consume`. A pre-approval decision begins writes only in the transaction it was made in, which the ledger checks.
- **There is no approve action, and the types say so.** `record_approval` and `record_decline` are methods of `ApprovalSurface` and of nothing else. The only `ApprovalSurface` comes out of `Policy::open`; it is not `Clone` and has no public constructor. `a2m-actions`, which opens the policy engine, hands it to the approval surface: the loopback listener in `a2m-local` after the operating system's prompt, or a server's console page after the approver signs in. Nothing in the action registry can hold it, so no command, MCP tool, REST route, or SDK method can approve.
- **Silence is a decline.** An approval window that ends is recorded with `record_decline` by nobody, any unused approval of the run is revoked, and the caller ends the run `expired`.
- **A session belongs to its owner.** `ActAs` on a person's identity is denied unless the principal is that person, or `acting_for` names them as the local user or the token's subject. A delegation never counts yet, because delegations are not built.
- **On one machine,** the built-in set gives the operating-system user every role and a grant on every tool, requires a person's approval for writes, and holds the guardrails. A local set cannot name another approver.
- **A toolset only narrows, and a second administrator is a check of `publish`.** The forbids of a toolset (`toolset.<name>.not_connectable`, `not_selected`, `effect_ceiling`, `identity_mode`, `no_pre_approval`, and `toolset.unknown`) are consulted after the guardrails and before any permit, so a toolset can refuse what a grant admits and never admit what no grant does. While the set in force says `second_admin_publish`, `publish` refuses with `second_admin_required` unless `publish_reviewed` is given a reviewer who is a person, not the publisher, not the author, and an administrator or publisher in the set in force (decision 0313).
- **Protected settings loosen only with a person present.** The record limit, the approval window, who approves, and the pre-approval rules are the protected settings of section 226. `publish` refuses a set that is looser than the one in force on any of them unless the proof carries a user-presence check that proves presence. Tightening needs no proof. Releasing a kill switch on one machine needs the same check.
- **A denial says what to ask for, not why.** The `refusal` carries a code from the table below and a next step; the determining rules are kept for `policy.explain`, which only an auditor or administrator may run. `a2m-actions` decides that before it calls `explain`, and `explain` answers only for a decision of the caller's workspace.
- **What the approval path cannot see.** The approver's kind is what the surface says, and `caller_is_agent` is not known on that path: what keeps an agent out of an approval is that it cannot operate the surface. A presence check is a word the surface hands in; nothing in the types stops a crate that holds the handle from writing one, which is why the handle goes to the person surface alone. The approver role is not scoped to a workspace or a target yet: on a server, an approver approves runs in every workspace.
- **What `PerformAction` needs is the registry's to say.** Each action's entry names the role that may perform it; the request carries it, and the set says who holds which role.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-actions` | `Policy::open`; `decide` at every dispatch; `compile`, `test`, `publish`, `explain`, `set_kill_switch`, for the actions `grant.add`, `policy.publish`, `policy.test`, `policy.explain`, `killswitch.set`; passes the `ApprovalSurface` to the approval surface |
| `a2m-actions` as the in-process `Control`, and `a2m-runner` on the control-plane side of the runner link | On the engine's behalf: `decide` at guard, before the write phase and on resume; `consume`; `check_limits`. The engine never names this crate |
| `a2m-sched` | `kill_epoch`, to stop handing out work and to tell runners |
| `a2m-local`, `a2m-api` | `ApprovalSurface::approve` and `decline`, through the handle `a2m-actions` gives the approval surface; the person is reached through `PersonSurface` |

It uses `a2m-ledger` for its tables and to read the run an approval is about, and `a2m-expr` to compile and evaluate the conditions of pre-approval rules.

## State

Tables, through `a2m-ledger`: `decisions`, `approvals`, `approval_declines`, `policy_versions`, `kill_switches`, `quota_counters`. Their migrations are the two files of the ledger's sequence that name this crate as owner. Not created yet: `principals`, `grants` as rows of their own (grants are part of a version's layers), `consents`, `delegations`. In memory: the compiled set in force as last read.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `policy` | `not_granted` | No permit applies to the caller, the tool, or the action |
| `policy` | `identity_not_allowed` | `ActAs` is denied, including every attempt to use another person's session |
| `policy` | `effect_too_high` | A grant exists, with a lower highest effect class |
| `policy` | `approval_missing` | A write reaches the write phase with no approval |
| `policy` | `approval_stale` | The approval is expired, used, revoked, or for another diff |
| `policy` | `approval_refused` | The approver is not a person, may not approve this run, or the run is not waiting for approval |
| `policy` | `limit_exceeded` | Records per run or a quota |
| `policy` | `kill_switch_on` | A kill switch covers the request |
| `policy` | `policy_evaluation_failed` | An error while deciding, or no valid set is loaded |
| `policy` | `presence_required` | A set that loosens a protected setting, or the release of a kill switch on one machine, without a user-presence check |
| `policy` | `second_admin_required` | A set that needs a second administrator to publish it was offered without one, or the second one may not review it |
| `input` | `schema_invalid` | A candidate set fails validation |
| `input` | `policy_tests_failed` | A candidate set fails its own test table |
| `input` | `decision_not_found` | `explain` of an identifier that names no decision |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Nothing is allowed unless a rule permits it | A test that asks all seven questions of an empty set |
| Every decision is recorded, allow or deny | A test that reads the `decisions` table through its own connection. The test over the whole action registry belongs to `a2m-actions` |
| No policy input removes a guardrail | A test that builds the loosest set that compiles and asks what each guardrail forbids, and a property test that a matching guardrail always denies |
| An agent identity can never approve | A guardrail test, and a test that hands `record_approval` an agent identity |
| Only the approval handle records an approval | The types: the two methods exist on `ApprovalSurface` alone, and a test that fails to compile if the handle becomes `Clone` |
| An approval is used once, and only for its diff, its run, and within its window | Tests with `a2m-ledger`: another diff, a second use, an expired one, a declined one, one recorded after the decline, one under a kill switch, and one consumed in an earlier transaction than `begin_writes` |
| One policy engine, and one approval handle, per store | A test that opens it twice |
| The installation's settings are in force under a published set | Tests of the record limit and of two approvers under a looser published set; one test per setting in `a2m-actions` |
| An evaluation error is a denial | A test with a condition that passes the checker and fails when it runs |
| The same request on the same version gives the same answer | A property test over 4,000 generated requests |
| A version that fails its test table is not published | A test that publishes one that lets a stranger write |
| A kill switch refuses everything it covers, and stays releasable | A test over the target, operation, and everything scopes |
| Decisions, versions, and switches survive a restart | A test that closes and reopens the store |
| With no valid set every decision is a recorded denial | A test that plants an unreadable newest version |
| A decision is under 100 microseconds on a warm cache | Not a benchmark yet. Measured by hand: decision 0082 has the numbers; hypothesis H-11 |

## Build order

1. Done: the decision core, chosen by evidence (decision 0082). The built-in local set: guardrails and "writes need approval", `decide` for all seven questions, recorded decisions. Slice 3.2.
2. Done: `check_limits` (records per run, quotas) and kill switches. Phase 3, group B.
3. Done: `record_approval`, `consume`, and `record_decline`, joined with `a2m-ledger`'s rule 3. Slices 3.3 and 3.6, the policy half.
4. Done: protected settings on one machine: a looser set, and the release of a kill switch, need a user-presence check.
5. Done in part: roles, grants, role grants, label and option rules, pre-approval rules with conditions, toolsets as a lower layer that only narrows, versioned sets with their own tests and a history, `publish`, `publish_reviewed` (a second administrator), `publish_change` (one grant or toolset), `explain` (naming the deciding grant, role grant, toolset, or rule), `would`, and the catalog questions (decision 0313). Not built: the lower layers of a workspace and of target files; the request budget per surface; a benchmark in `bench/`; decision timing through `a2m-telemetry`. The actions that carry these (`toolset.create`, `toolset.update`, `toolset.list`, `grant.add`, `grant.remove`, `grant.list`, `policy.publish`, `policy.test`, `policy.explain`) are `a2m-actions`' half of package 6.3.
6. Not built: standing approvals; delegations and `ActAs` with a delegation. The guardrails that the author of a version cannot be its only publisher and that an agent cannot approve a publish are held by `publish_reviewed` when the set asks for a second administrator; whether they should hold when it does not is open.
7. Not built: consent records, with the first act that needs one.
8. Done (the safety audit, decision 0145): the store's key, taken once; the installation's settings through `BuiltIn::with_settings`, kept as a floor; the forbids for an agent's ceiling, writes outside test records, and pre-approval off; two approvers of an irreversible run; `consume` at the point of action; the proof's time, the tool version, and the presence words checked when an approval is recorded; `explain` bound to the caller's workspace.

## Open points

- Whether every read's decision is stored in full, or summarized at scale as decision 0034 allows. Today every decision is stored in full.
- Whether consent records belong here or with the scout. They are here because only the decider should ask whether one exists.
- The shape of the presence proof that `a2m-local` hands in. Settled in part (decision 0145): each operating system's prompt has its word, `polkit`, `local_authentication`, `windows_hello`, and, for a field test only, `field_harness` (decision 0310), beside `os_prompt` and `signed_in`, and `not_available` records an approval only by the engine's own prompt and proves nothing: it does not loosen a protected setting or release a kill switch. The proof's time and the prompt's name come with the approval; the time is checked, and the name is kept in the audit entry the desk writes, not in the approval's row.
- A set published before the binary's guardrails change is not read by the newer binary, because its version was stamped with the old guardrails' hash. The built-in set is stored again at start; a person's set must be published again. Whether to republish it automatically is open.
- `caller_confirms`, the third value of `policy.approve_writes` in section 226, has no meaning here yet: a person approves every write through the surface, and the set says only whether that person may be the caller.
- Listing the tools a caller may use is `would` asked tool by tool. Whether that is fast enough for a large catalog is not measured.
