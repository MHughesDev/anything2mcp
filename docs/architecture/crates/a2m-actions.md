# `a2m-actions`

Every way into the kernel, defined once. An action is a name, an input schema, an output schema, the lowest role that may use it, whether it is listed to agents, and a handler. This crate holds the registry of actions, the dispatcher that every front door calls, and the `Kernel` handle that assembles the services below it. In one process it is also the runner: a loop that claims jobs from the scheduler and hands them to the engine.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 7, Actions |
| May depend on | Layers 1 to 6 |
| Depends on | Every kernel crate below it except `a2m-sources`, which it reaches through others; `a2m-expr` to compile a watch's condition against the read's outputs ([decision 0291](../../decisions/0291-a-watch-ends-when-its-condition-holds-on-a-checks-own-result.md)). Outside: `serde`, `serde_json`, Tokio, and `toml` for settings files. Schemas are checked by `a2m-files`; no outside validator |
| Roadmap packages | Roadmap packages B5 (the first definitions), 1.4 (`Kernel::open`, the runner loop), 1.5 (generated faces), 3.2 and 3.6 (the policy gate and approvals), E5 (the scout), 5.1 and 5.2 (every authoring action, shortcuts), 6.3 (toolsets and roles). A second pass, after the crates above it were built, is [decision 0132](../../decisions/0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md); the safety audit is [decision 0152](../../decisions/0152-the-action-registry-and-telemetry-after-their-safety-audit.md) |
| Carries out | Spec sections [220](../../spec/2-kernel/220-actions-and-the-authoring-interface.md), [224](../../spec/2-kernel/224-when-the-engine-itself-fails.md), [220](../../spec/2-kernel/220-actions-and-the-authoring-interface.md), [402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md); the in-process half of [223](../../spec/2-kernel/223-the-runner-protocol.md) and [215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md); the [action catalog](../../reference/action-catalog.md) |

## Owns

- The action registry: one `ActionDef` per row of the action catalog, plus the actions a provider contributes under its own name space.
- The dispatcher: validate, check health, ask policy, run the handler, shape the result.
- The `Kernel` handle, `Kernel::open`, and the kernel's health state.
- The settings types the kernel needs and the loader that layers them, so that `config.show` and `config.set` speak the same rules as start-up.
- The on-disk layout of [the technical reference](../../reference/technical-reference.md#on-disk-layout), as one pure function, so that the local channel, the runner journal, and the store agree on paths.
- The in-process runner loop, and the in-process implementations of `Control` (from `a2m-engine`) and `KeeperWork` (from `a2m-auth`).
- The only caller of the compiler in `a2m-files`: lint, compile, and draft. A plan, a type in `a2m-core`, is produced there and stored by `version.publish`; the engine receives it through `Control` and never sees the compiler.
- The meaning of three tables: toolsets, the scout's stage results, and the outcomes of webhook deliveries; of two more that say what a run was called with, its input and caller, and its source; and of two the scout keeps, its scouts and what each discoverer of a discovery run reported (migration `0014`). Their migrations, like every migration, live in `a2m-ledger`.
- What a caller is shown of a value of a system: an exact number as its exact decimal text, a calendar day as text, a moment and a length of time as whole milliseconds. Inside the engine and in the store a value keeps its kind.
- The recorded catalog: the list of actions with a digest, from which every face is generated and checked.
- The scout pipeline, as the handler of `scout.run`.
- The held drafts as a person is shown them ([decision 0298](../../decisions/0298-a-draft-with-an-open-question-waits-in-a-holding-folder.md)): `draft.lint` answers `held`, each held file with its questions and their places, whatever its own `ok`; `draft.try` refuses a tool whose draft is held by name, quoting the question; `draft.read`, `draft.write`, and `draft.delete` take a path under `held/`; and when a compile has failed and the target has held drafts, the answer of `draft.lint`, `draft.try`, `version.publish`, `draft.write`, and `draft.delete` says once, in the product's words, that the target has N held drafts, one of which may define a name that was not found. No file is named and no diagnostic is changed; the compiler is given no held file.
- Shortcuts, as data: a word and a fixed list of actions.

It must never: decide whether something may happen (that is `a2m-policy`); sign in or renew (that is the keeper in `a2m-auth`); carry out a step (that is `a2m-engine`); name a pack, a mode, or a make of client; open a socket or a port; accept an approval through any action.

## Interface

As built ([decision 0094](../../decisions/0094-the-action-layers-interface-as-building-it-settled-it.md), [decision 0132](../../decisions/0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md) for the second pass, and [decision 0152](../../decisions/0152-the-action-registry-and-telemetry-after-their-safety-audit.md) for the safety audit). `ProviderRegistry` stands for the `Registry` of `a2m-provider`.

```rust
/// One action. Every face is generated from this and nothing else.
pub struct ActionDef {
    pub name: ActionName,              // noun.verb, checked
    pub summary: String,               // one line; the first 240 characters choose the tool
    pub input: Input,                  // flat fields, and the strict JsonSchema they make
    pub output: JsonSchema,            // published
    pub examples: Vec<&'static str>,   // example inputs, as JSON text; each fits the schema
    pub shape: Shape,                  // Inspect, List, Trace, Compare, Observe, Change
    pub effect: Effect,                // Fixed(EffectClass), or OfTool for tool.call
    pub who: Who,                      // the catalog's "Who" column; role(effect) gives the Role
    pub listed: Listed,                // No, Use, Build, UseAndBuild
    pub long_running: bool,
    pub needs: HealthNeed,             // Nothing, Reads, Writes
    pub person: PersonNeed,            // None, Presence, PresenceWhenLoosening
    pub runs_in: RunsIn,               // Engine, Client
    pub faces: Faces,                  // all four, today
    pub rest_alias: Option<RestAlias>,
    pub phase: &'static str,           // the catalog's phase or stage
    pub handler: Handler,              // Kernel(fn), Provider(name), Client, NotBuilt
}

pub struct ActionRegistry { /* ActionDef by name, in catalog order; frozen */ }
impl ActionRegistry {
    pub fn build(providers: &ProviderRegistry) -> Result<ActionRegistry, Failure>;
    pub fn kernel_only() -> ActionRegistry;             // for generators: no provider's actions
    pub fn get(&self, name: &ActionName) -> Option<&ActionDef>;
    pub fn find(&self, name: &str) -> Option<&ActionDef>;
    pub fn by_tool_name(&self, name: &str) -> Option<&ActionDef>;   // a2m_<noun>_<verb>
    pub fn iter(&self) -> impl Iterator<Item = &ActionDef>;
    pub fn toolset(&self, toolset: BuiltInToolset) -> Vec<&ActionDef>;   // listed, within the cap
    pub fn past_cap(&self, toolset: BuiltInToolset) -> Vec<&ActionDef>;  // marked, reached by action.run
    pub fn catalog(&self) -> Catalog;                   // serializable, with a digest
}

/// What a front door hands the dispatcher after the kernel verified the caller.
pub struct Request {
    pub action: String,                // as the caller wrote it; the dispatcher finds it
    pub input: serde_json::Value,      // an object, or null
    pub caller: Caller,                // Principal, client label, FrontDoor, toolset, is_agent
    pub idempotency_key: Option<String>,
    pub project: Option<PathBuf>,      // as stated; the dispatcher checks it before any handler
}
impl Request {
    pub fn new(action: &str, input: serde_json::Value, caller: Caller) -> Request;
    pub fn in_project(self, project: impl Into<PathBuf>) -> Request;
    /// What every door calls with the `ProjectFolder` its mount put on the request.
    pub fn in_project_folder(self, project: Option<&ProjectFolder>) -> Request;
}
/// The project folder a request works in, as a door's mount states it. A statement,
/// never trusted: see "The project folder" below.
pub struct ProjectFolder(/* a path */);
impl ProjectFolder { pub fn new(path: impl Into<PathBuf>) -> Self; pub fn as_path(&self) -> &Path; }
pub struct Response {
    pub ok: bool, pub action: String,
    pub result: Option<serde_json::Value>, pub error: Option<ErrorBody>,
    pub warnings: Vec<String>, pub next: Option<Next>,
    pub exit: ExitClass,               // Succeeded, Failed, Malformed, PersonNeeded
    pub decision: Option<DecisionId>,
}

pub struct KernelParts {
    pub settings: Settings, pub paths: Paths, pub place: StorePlace,   // Disk, Memory
    pub key_store: Arc<dyn KeyStore>, pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn RandomSource>, pub os_user: String,
}
pub struct Kernel { /* Arc of every service below; cheap to clone */ }
impl Kernel {
    /// The store with its migrations, the vault joined to it, telemetry. Never fails.
    pub async fn prepare(parts: KernelParts) -> Prepared;
    /// Policy, the keeper, the scheduler, the action registry, the tending task. Never
    /// fails. Hands the approval desk to `person` and keeps no copy.
    pub async fn open(prepared: Prepared, providers: ProviderRegistry,
                      problems: Vec<RegistryProblem>, person: Arc<dyn PersonSurface>) -> Kernel;
    pub async fn verify_caller(&self, credential: CallerCredential) -> Result<Principal, Failure>;
    pub fn status_only_caller(&self) -> Principal;       // nobody: a principal of no installation
    /// Behind the `testing` feature: a second person of this workspace, who holds nothing.
    pub fn second_person(&self, id: [u8; 16]) -> Result<Principal, Failure>;
    pub async fn dispatch(&self, request: Request) -> Response;
    pub fn health(&self) -> Health;                      // state, and every impaired part
    pub async fn recheck(&self) -> Health;
    pub async fn tend(&self);                            // sweep, keeper tick, recheck
    pub async fn start_runner_loop(&self, slots: u32) -> Result<RunnerLoop, Failure>;
    pub fn actions(&self) -> &ActionRegistry;
    pub fn providers(&self) -> &ProviderRegistry;
    pub fn settings(&self) -> &Settings;
    pub fn paths(&self) -> &Paths;
    pub fn redact(&self) -> Arc<dyn Redact>;
    pub fn telemetry(&self) -> &Telemetry;
    /// A page of a run's events after a sequence number, at most EVENTS_PAGE (200),
    /// redacted. Only for a run the caller started. Not an action.
    pub async fn events(&self, caller: &Principal, run: RunId, after: u64, limit: u32)
        -> Result<RunEvents, Failure>;
    /// The same, waiting until an event is appended, the run ends, the run comes to
    /// rest while this waits, or `wait` passes.
    pub async fn next_events(&self, caller: &Principal, run: RunId, after: u64, limit: u32,
                             wait: Duration) -> Result<RunEvents, Failure>;
    /// The same, from what the follower last saw: answered at once when whether the run
    /// is settled has changed since, although no event says so. What a follower uses.
    pub async fn next_events_from(&self, caller: &Principal, run: RunId, seen: Followed,
                                  limit: u32, wait: Duration) -> Result<RunEvents, Failure>;
    /// The scheduler's jobs waiting or leased, and the runs resting for a person.
    pub async fn runs_in_hand(&self) -> Result<RunsInHand, Failure>;
    pub async fn close(self, why: CloseReason);
}
pub struct RunsInHand { pub working: u64, pub waiting_for_person: u64 }
/// Re-exported from the ledger: an idempotency key, 1 to 200 printable
/// characters with no space, checked by `CallerKey::new`; and CALLER_KEY_LONGEST.
pub use CallerKey;
/// The last event's number a follower was given, and whether the run was settled then.
pub struct Followed { pub after: u64, pub settled: Option<bool> }
impl From<u64> for Followed {}            // nothing known of the run
impl RunEvents { pub const fn followed(&self) -> Followed; }
impl RunsInHand { pub const fn any(&self) -> bool; }
pub struct RunEvents { pub run: RunId, pub events: Vec<RunEventView>, pub last: u64,
                       pub state: RunState, pub working: bool, pub ended: bool }
impl RunEvents { pub const fn settled(&self) -> bool; }   // ended, or resting with no job
pub struct RunEventView { pub seq: u64, pub lease: u64, pub at: Timestamp, pub kind: String,
                          pub detail: serde_json::Value }
impl Prepared {
    pub fn redact(&self) -> Arc<dyn Redact>;
    pub fn telemetry(&self) -> Telemetry;
    pub fn settings(&self) -> &Settings;
    pub fn host_services(&self, supervisor: Supervisor, roots: Roots) -> HostServices;
}
pub enum CallerCredential { LocalUser(String), Bearer(Secret), ApiKey(Secret) }

/// What a role supplies so that a person can be reached. Every method returns a boxed
/// future, so the trait is a trait object.
pub trait PersonSurface: Send + Sync {
    fn take_desk(&self, desk: ApprovalDesk);             // once, from Kernel::open
    fn ask_approval(&self, ask: ApprovalAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>>;
    fn ask_sign_in(&self, ask: SignInAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>>;
    fn confirm_presence(&self, ask: PresenceAsk) -> BoxFuture<'_, Result<PresenceProof, Failure>>;
    fn ask_session(&self, ask: SessionAsk) -> BoxFuture<'_, PersonAnswer>;   // the keeper's asks
    /// Whether the surface has a page for a person's act. Default false: the act is
    /// refused `policy`/`person_required` before anything is pinned, as before.
    fn shows_person_acts(&self) -> bool { false }
    /// A person's act an agent asked for: show it on a page; the link goes into the
    /// caller's `needs_person` result. Default: refused `policy`/`person_required`.
    fn ask_person_act(&self, ask: PersonActAsk) -> BoxFuture<'_, Result<SurfaceLink, Failure>>;
    /// What the presence prompt can do here, for `doctor.run`. Raises no prompt.
    fn presence(&self) -> BoxFuture<'_, PresenceReport> { /* not_available, unknown */ }
    fn return_address(&self) -> Option<String> { None }
}
pub struct PresenceReport { pub kind: &'static str, pub reason: &'static str,
                            pub message: String, pub fix: String }
/// Proof that a person was present: the kind of check, whose it is, when, and the
/// prompt that answered. Copy. Whoever uses one checks it.
impl PresenceProof {
    pub const fn new(check: PresenceCheck, principal: PrincipalId, at: Timestamp) -> Self;
    pub const fn naming(self, prompt: &'static str) -> Self;   // a word; another shape is not kept
    pub const fn for_act(self, act: PersonActId) -> Self;      // given for one person's act
    pub const fn check / principal / at / prompt / act(&self);
    /// Its person is `person`, and it is at most PRESENCE_WINDOW_MS (120,000) old.
    pub fn checked(&self, person: PrincipalId, now: Timestamp) -> Result<(), &'static str>;
}
pub struct ApprovalDesk { /* not Clone; no public constructor; reached by no action */ }
impl ApprovalDesk {
    pub async fn waiting(&self, run: RunId) -> Result<WaitingApproval, Failure>;
    /// What the waiting run would change, from the stored diff whose hash `waiting`
    /// reports: at most PREVIEW_RECORDS (100) records from `offset`, each value cut to
    /// PREVIEW_VALUE_CHARS (400), every text redacted.
    pub async fn preview(&self, run: RunId, offset: usize) -> Result<ApprovalPreview, Failure>;
    pub async fn approve(&self, run: RunId, diff_hash: Digest, approver: PrincipalRef,
                         channel: Channel, proof: PresenceProof) -> Result<ApprovalId, Failure>;
    /// Records the decline and ends the run as `expired`, in one transaction.
    pub async fn decline(&self, run: RunId, by: Option<PrincipalRef>, channel: Channel)
                         -> Result<(), Failure>;
    /// A person's act that waits on a page (decision 0163): what will be done.
    pub fn person_act(&self, act: PersonActId) -> Result<PersonActAsk, Failure>;
    /// Carries it out once, as the person, with a proof given for it; the response
    /// is what it came to. A proof of another act, person, or time does nothing.
    pub async fn carry_out(&self, act: PersonActId, proof: PresenceProof) -> Result<Response, Failure>;
    pub fn decline_act(&self, act: PersonActId, by_person: bool);
}
pub struct PersonActId(/* 16 random bytes, shown as 32 hexadecimal characters */);
impl PersonActId { pub const fn from_bytes(b: [u8; 16]) -> Self; pub fn parse(t: &str) -> Option<Self>; }
pub struct PersonActAsk { pub act: PersonActId, pub person: PrincipalId, pub action: ActionName,
    pub summary: String, pub input: Vec<ShownField>, pub details: Vec<String>,
    pub surfaces: Vec<ActSurface>, pub warnings: Vec<ActWarning>,   // a publish's; not repeated in `details` (0199)
    pub review: Option<ReviewSummary>,   // a publish's, sign-in addresses from the keeper, every text redacted (0264)
    pub secrets: Vec<AskedSecret>, pub asked_through: String, pub expires_at: Timestamp }
pub struct ShownField { pub name: String, pub value: String, pub cut: bool }
pub const ACTS_WAITING: usize;   // 16
pub const ACTS_KEPT: usize;      // 64
pub const SHOWN_VALUE_CHARS: usize; // 400
/// What `init` writes (section 220 gives it no action): the project file, AGENTS.md,
/// .gitignore, and targets/, each new and never through a link; what is there is kept.
pub mod init { pub fn init_project(folder: &Path) -> Result<InitReport, Failure>;
               pub struct InitReport { pub written: Vec<&'static str>, pub kept: Vec<&'static str> }
               pub const PROJECT_FILE_TEXT, GUIDANCE_TEXT, GITIGNORE_TEXT: &str; }
pub struct ApprovalPreview { pub run: RunId, pub diff_hash: Digest, pub tool: String,
    pub target: String, pub environment: String, pub create: u32, pub update: u32,
    pub unchanged: u32, pub total: usize, pub offset: usize, pub records: Vec<PreviewRecord> }
impl ApprovalPreview { pub const fn complete(&self) -> bool; }
pub struct PreviewRecord { pub operation: String, pub key: String, pub action: &'static str,
    pub fields: Vec<PreviewField> }
pub struct PreviewField { pub name: String, pub current: Option<String>, pub proposed: String,
    pub changes: bool, pub cut: bool }

pub struct Paths { /* installation_config, config, data, store, blobs, profiles, journal, logs, runtime, cache */ }
impl Paths { pub fn resolve(env: &Env) -> Result<Paths, Failure>; pub fn under(home: &Path) -> Paths; }

pub struct Settings { /* every key, its value, the layer that set it */ }
impl Settings {
    pub fn load(sources: &Sources) -> Result<Settings, Failure>;          // start-up: any problem stops
    pub fn load_all(sources: &Sources) -> (Settings, Vec<Failure>);       // doctor: every problem
    pub fn defaults() -> Settings;
    pub fn get / text / int / flag / list(&self, key: &str);
    pub fn entry(&self, key: &str) -> Option<&Entry>;                    // value, layer, origin, overridden
    pub fn provider_settings(&self) -> ProviderSettings;                 // for RegistryBuilder::new
    pub fn show(&self, key: Option<&str>) -> Result<serde_json::Value, Failure>;
    pub fn digest(&self) -> Digest;
    pub fn project_file(&self) -> Option<&Path>;                          // layer 6's file: the start's
    pub fn project_values(&self) -> BTreeMap<String, Value>;              // what layer 6 wrote
}
pub struct Sources { /* installation, installation_keeper, presets, user, project, environment, flags, serve, packs */ }
// Who may change the installation's file: only Administrator may loosen or lock (decision 0241).
pub enum InstallationKeeper { Administrator, Open { path, why }, NotChecked(&'static str) }
impl InstallationKeeper { pub fn may_loosen(&self) -> bool; pub fn why_not(&self) -> Option<String>; }
// Sources::read checks and reads the installation's file through one handle (decision 0273);
// Sources::with_installation_keeper_for_tests is the hidden route a test of the product takes.
// Each SettingDef says whether its part is built; a value for one not built is refused by name.
pub enum Built { Yes, No { instead: &'static str, arrives_with: &'static str } }
pub const PROJECT_SETTINGS_DIFFER: &str;                                  // the warning's code
impl Sources {
    pub fn read(paths: &Paths, project: Option<&Path>, environment: BTreeMap<String, String>)
        -> Result<Sources, Failure>;
    pub fn with_pack(self, manifest: Arc<Manifest>) -> Sources;
}

/// The faces, as data.
pub fn command_faces(registry: &ActionRegistry) -> Vec<CommandFace>;
pub fn mcp_tools(registry: &ActionRegistry, toolset: BuiltInToolset) -> Vec<McpToolFace>;
pub fn mcp_tool(def: &ActionDef) -> McpToolFace;
pub fn rest_routes(registry: &ActionRegistry) -> Vec<RestRouteFace>;
pub fn sdk_methods(registry: &ActionRegistry) -> Vec<SdkMethodFace>;
impl McpResultFace {
    pub fn of(response: &Response) -> McpResultFace;
    /// For a caller on `toolset`: `next` names `a2m_action_run` where the toolset
    /// does not list the following action's own tool.
    pub fn of_in(response: &Response, registry: &ActionRegistry, toolset: BuiltInToolset) -> McpResultFace;
}
pub const SHORTCUTS: &[Shortcut];
impl Shortcut {
    /// The steps to carry out on this engine: those before the first action that is
    /// not built, that action, and the sentence to print when it stops early.
    pub fn on(&self, registry: &ActionRegistry) -> ShortcutRun;
}
pub struct ShortcutRun { pub steps: Vec<ShortcutStep>, pub stops_before: Option<&'static str>,
                         pub note: Option<String> }
pub const PROCESS_COMMANDS: &[&str];
```

A `CommandArg` carries `stdin_for`: for a flag, the field that standard input fills when the flag is given. The command then reads standard input to its end, removes one line ending from the end, sends the text as that field, and does not send the flag. Today only `--stdin` of `secret.set` has it, for `value`. The kernel never reads standard input.

### The dispatcher

A front door authenticates and builds a `Request`; it decides nothing. `dispatch` then does, in this order, and every refusal is an ordinary `Failure`:

1. **Find.** An unknown name is `input` / `schema_invalid`, with the nearest names as the fix.
2. **Validate.** The input against the action's full schema, never the compact one a tool list showed. Malformed input and a valid input that names nothing are different failures (spec section 220): the second is `input` / `not_found`.
3. **Health.** The action's `needs` against the kernel's state. A write in `degraded`, or anything but a `Nothing` action in `stopped`, returns `busy` / `engine_degraded` with the reason.
4. **Decide.** One `decide` call to `a2m-policy`, with the action, its effect (for `tool.call`, the published tool's effect class), the caller, and the context the engine supplies. Policy records the decision, allowed or refused, and returns its identifier. The dispatcher refuses to go on without one. The one exception: a `Nothing` action in a `stopped` kernel, which has no store to record a decision in; it reports on the engine and shows no data of any system.
5. **Person.** For an action with `PersonNeed::Presence`, and for one with `PresenceWhenLoosening` when the call loosens: a caller marked as an agent is handed to the person surface's page for a person's act (below); any other caller is asked `confirm_presence` on the role's surface. Releasing a kill switch loosens; an action that has not said how its loosening is told (`config.set`, `provider.enable`, neither built) is asked for on every call. A caller marked as an agent never satisfies it, and on one machine the proof comes only from the operating system's prompt. The proof that comes back is checked: it must be this caller's, and at most two minutes old by the kernel's clock. A handler is given the proof in `Call::presence` and never asks itself.
6. **Run.** The handler. It is given the effect class and hazard the decision was made for (`Call::decided`); `tool.call` refuses, with `busy`, `tool_changed`, a tool that has another by the time it looks. A long-running action submits to `a2m-sched` and returns the `run_id` at once.
7. **Shape.** Pass every outgoing string through `Redact`, the echoed action name included; cut to the result size cap and say which limit cut it; add `next`. Every strict output schema admits `next`, and the MCP rendering of a success puts it into the structured content as `action`, `tool`, `args`, and `when`. Not built: marking text that came from a target as untrusted outside the scout's report, which keeps it under `untrusted`; and keeping a large result as a blob with its reference.

### A person's act an agent asks for

[Decision 0163](../../decisions/0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md). Every caller of a served engine is an agent, so a person's act is shown to the person instead of refused:

- **The ask.** The dispatcher hands the act to `Kernel::ask_person` (module `acts`): the caller must act for a person (`Principal::person`), or it is `policy`, `person_required`; an act not built is `action_not_built`. The act waits in memory under sixteen random bytes, with its input, the caller, and the project folder as it was checked, for `policy.approval_window_minutes`, at most `ACTS_WAITING` at once. For `version.publish` the drafts are compiled at the version they will be stored as, and `version` and `plan_hash` are pinned into the input; a lint failure is answered then, with no page. `PersonSurface::ask_person_act` is given exactly what will be done (`PersonActAsk`, every value redacted and cut) and gives the link.
- **The answer.** `exit` person needed; a result `state: needs_person`, `act`, `action`, `link`, `person`, `expires_at`; `next` is the same call. Every output schema of a person's act, and of `publish.request`, admits that result: the output is either the action's own or this. The same call while the act waits gives the same page; once it was answered or its window ended, what became of it is given back, for the window, the newest `ACTS_KEPT`.
- **The press.** `ApprovalDesk::carry_out` checks that the proof was given for this act (`PresenceProof::for_act`), is the person's the act waits for, and is of now; marks the act as being carried out, so a second press finds nothing; decides the act again, recorded, for the person (the same principal, not an agent, client `person page`); looks at the project folder again; runs the handler with the proof; and keeps what it came to. `version.publish` refuses `stale`, `drafts_changed`, when the drafts compile to another plan than the one pinned. A proof that fails changes nothing, and the act goes on waiting.
- **Decline and silence.** `ApprovalDesk::decline_act` keeps the act as declined (`policy`, `approval_refused`); nothing is done.

### Watches

[Decision 0283](../../decisions/0283-watches-reach-an-agent-through-the-registry-and-their-person-confirms-each-on-the-page.md), over the ledger's and the scheduler's halves ([decision 0262](../../decisions/0262-watches-are-held-to-their-end-their-budget-and-their-runs-own-result.md)). Module `handlers/watches.rs`.

- **The ask.** `watch.create` takes a published read, its input, an environment, an interval, an end, outputs to leave out, and a condition ([decision 0291](../../decisions/0291-a-watch-ends-when-its-condition-holds-on-a-checks-own-result.md)): an expression compiled against `result`, a record of the read's outputs typed as the plan types them, that must give true or false, and kept in the plan's form; the ledger answers it on each check's own result, and the confirmation page shows it. The person it acts for is `Principal::person` of the caller. The surface, the requests a check sends, and the shape compared come from the plan: the first verified binding's surface, its act and confirm steps, and the whole result, since no plan declares a list of records with a key. It keeps the run's call under that person, asks through `Scheduler::ask_watch`, and hands `watch.confirm` to `Kernel::ask_person`.
- **The confirmation.** A person's act (`PersonNeed::Presence`): an agent's call goes to the page, whose lines `Kernel::watch_confirmation_lines` gives; the handler passes the current plan hash and the surface's rate limit to `Scheduler::confirm_watch`.
- **The tending.** `Kernel::tend` calls `tend_watches` after the sweep: the scheduler's `due_watches` ends every watch past its end and cancels the runs left to cancel in its own transaction (decision 0291), then the tending admits due checks (after setting the surface's declared budget), ends each check whose run ended with that run's own stored result, cancels a resting check run after the last check on it ended, and resumes a watch paused for a sign-in once its person's session is valid. The tending task waits on the earlier of `next_due` and `next_watch_due`; the runner wakes it when a check's run finishes. `version.publish` calls `watches_after_publish`.
- **A shared run.** Two watches of one read and input for one person share a run under ledger rule 6 ([decision 0285](../../decisions/0285-two-watches-of-the-same-read-for-one-person-share-a-run-and-ending-one-never-cancels-a-run-another-rests-on.md)). Wherever a check's run would be cancelled (`watch.end`, the runs a watch's end closed, which the scheduler cancels itself, a resting run, and `watches_after_publish`), it is cancelled only when `Watches::run_left_to_cancel` answers that a watch's check opened it and no other check under way names it, each run once ([decision 0287](../../decisions/0287-the-last-check-under-way-on-a-run-a-check-opened-cancels-it.md)). So ending one watch never cancels the run another rests on, the last check on a run its watch's check opened cancels it, every watch on a shared run that waits for a sign-in pauses for it, and a run no check opened, such as the person's own call a check joined, is never cancelled for a watch. A person's own call that joins a run a check opened is not protected: the ledger keeps no mark of it (decision 0285, point 5).
- **Whose.** Every action but `watch.create` serves only a watch of the caller's person; `watch.end` also its asker. A record a change names is resolved only from a run of that person.
- **Idle.** `Kernel::watches_hold_the_engine` is true while a watch is confirmed or running; the product crate's idle holds read it.

### Runs that wait for a person

- **Whose a run is.** `run.get`, `run.resume`, and `run.cancel` answer for a run only to the person who started it, or to a caller proved to act for that person (`Principal::person`). Anyone else is told `input`, `not_found`, whatever role they hold: a run acts under its person's session. `trace.show` is bound to the workspace alone, because reading a run's evidence is what the auditor's role is for and the dispatcher's decision was that the caller holds it. A resume never changes what was kept of the run's caller: the guard asks about the caller that started the run, and `run.resume` first asks the policy engine, and records the answer, whether whoever resumes may use the run's tool.
- **A run whose call cannot be read.** A run whose row in `actions_run_calls` is missing, or whose caller does not parse, is never run with an empty input and a caller made up for it. It ends as `failed` with `busy`, `run_call_unreadable`, or as `cancelled` where the ledger allows no other end; with a record in doubt it stops, and no reconcile is queued, which would end the same way.
- **Waiting for a run.** `tool.call` and `run.resume` wait until the run has ended, or rests or has stopped with no job of the scheduler waiting or leased for it. The run's row and its unfinished job are read in one transaction: a job is unfinished from the transaction that queues it to the one that records how it ended, so nothing a runner does between its claim and the end of its work is taken for rest. After an approval, one `run.resume` completes the write.
- **Telling the person.** Only `tool.call` and `run.resume` tell a run's person that it waits, and once for one diff of one run: the link is kept in memory and given again. `run.get` asks nobody anything; it shows the kept link. A run that a person approved at the desk and nobody has resumed is reported with `approved: true`, no link, and `run.resume` as the next step, and is not marked as needing a person.
- **A decline.** `ApprovalDesk::decline` records the decline through the policy engine and ends the run as `expired` in the same transaction, with the failure `policy`, `approval_refused`. `run.get` reports it; nobody is asked again; the same call again is a new run.
- **The refusal to write without a person** is a recorded decision. A `need` event of the run carries its identifier, the word `approval`, and the diff's hash.
- **A kill switch** met when work is submitted or resumed is answered `policy`, `kill_switch_on`, by the policy engine, which records the refusal. The scheduler's `busy`, `on_hold` is not passed on.

### Idempotency keys and the run list

What a caller needs after a lost answer ([decision 0160](../../decisions/0160-an-idempotency-key-gives-back-its-run-and-run-list-shows-a-callers-own-runs.md)).

- **A key.** `tool.call` takes one in its field `idempotency_key`, or beside its input in `Request::idempotency_key`, where the REST door puts its `Idempotency-Key` header and the MCP door the `anything2mcp/idempotency_key` of a call's `_meta`. Both may be given only when they are the same key. The dispatcher checks it before anything is decided: 1 to 200 printable ASCII characters, no space (`input`, `idempotency_key_invalid`). Another action given a key answers with a warning that it was not used.
- **What a key means.** A call that carries a key is the same call as the first one made with it by the same person in the same workspace: it is given back the run that call started, unfinished or finished, its stored result shown and redacted as any result is, marked `repeated: true`, and no other run is started. The same key with another tool, environment, input, or source is `input`, `idempotency_key_reused`, and starts nothing. The version of the tool is not part of what is compared: the same call after a publish still gives back its run. A key holds for as long as its run is kept; it leaves with the run when retention purges it.
- **How it is kept.** The ledger's table `caller_idempotency` (migration `0013`) holds the workspace's keyed digest of the key, never the key: a key is the caller's own text and may hold anything. The key is held, with the digest of the call, in the transaction that keeps the run's call, before the run is admitted; it is bound to the run once the scheduler admits it; it is let go when nothing was admitted. A key held with no run, left by a call that stopped in between, is bound to the run that call was admitted as, when there is one, before another is started. Two calls with one key are one at a time in memory from the look at the key to the binding.
- **`run.list`.** A page of the caller's own runs, newest first: only the person who started a run, or a caller proved to act for that person, sees it, whatever role is held; an auditor reads other people's runs through `trace.show` and the audit log. Filters: `state` (a list), `tool`, `target`, `since` (whole milliseconds, by the store's clock, when the run was opened), `idempotency_key`. At most 100 a page, 20 by default; `after`, the last run of the previous page, starts the next, and the result says `complete`. Each entry: `run_id`, `tool`, `target`, `status`, `created_at`, `started_at`, `ended_at`, `in_doubt` (read from the ledger, yes when it cannot be read), `waits_for_person`, the failure's class and code, and `next`. REST serves it at `GET /api/v1/runs`.

### The project folder

A local command works in a project folder. The mount of a front door puts a `ProjectFolder`, a type of this crate, on each request, as an extension beside the caller, from what the client end stated; the door hands it to `Request::in_project_folder`. Nothing a caller writes in a path, a query, a body, or a tool's arguments sets it, so a caller of an MCP tool cannot state one. The client end of the local channel can, and it runs as the person: whoever can run it can state any folder. So before the handler runs the dispatcher takes the folder only when all of this holds, with every link followed:

- this engine serves one machine, the path is a full one, and it names a folder that exists;
- the folder neither holds nor lies inside the engine's own settings, data, runtime, or cache folders;
- it is not the home folder or a folder above it, does not lie inside a hidden folder of the home folder, and neither holds nor lies inside a folder the operating system gives programs for their data (the `XDG_*` folders, `Library` on macOS, `APPDATA`, `LOCALAPPDATA`, and `PROGRAMDATA` on Windows). The folder for temporary files is excepted from the last two, because on one operating system it lies inside such a folder and is nobody's own. This is what keeps a browser's profile and a key folder from being a project (rule 5);
- it holds a project file, `anything2mcp.toml`, that is a file and not a link, is at most 1 MiB, and loads as a settings file by itself;
- nobody but the engine's user or the administrator may change the folder or its project file: the owner of each, and of what a link at its path leads to, is one of the two; the mode lets no other account write; and a group may write only when it is the user's primary group. Inside the folder for temporary files, every folder between it and the project is held to the same rule, which is what keeps the excuse from the rule above safe. This is a copy of the client's rule (`kept_by` of `a2m-local`), which this crate cannot depend on. On Windows the owner and the access list are not read yet, and the rule refuses nothing there;
- it is inside what the setting `files.allowed_roots` allows. The word `<project>`, the default, allows any folder that passed the checks above: the local engine serves commands run in many projects, so the folder it was started in would be the wrong bound. A full path allows the folders inside it; a list without the word allows only those; an entry that is neither allows nothing.

A folder that fails is not handed on: `Call::project_folder` then answers `input`, `path_not_allowed` with the reason, and an action that needs no project is served all the same. What is handed on is the folder as it was resolved, never the path as it was stated. Every check compares paths in one form, the one the operating system gives a resolved path (a place that does not exist yet is resolved up to its nearest folder that does). On Windows that form is the verbatim one, `\\?\C:\...`, and the folder is handed on in the ordinary form, `C:\...` (`\\?\UNC\server\share\...` as `\\server\share\...`), wherever that names the same folder; a path with a part the ordinary form would read otherwise, such as a device name or a trailing dot, is handed on as resolved. That is the form the provider's `resolved_path` gives, the one in which every root holds its folder; the rule for it is written once, there. A handler reaches every file through a checked path under the folder, as before. What remains: a program that runs as the person and writes a project file into a folder of theirs makes that folder a project; such a program can read the folder itself.

### A source for a pipeline

`tool.call` takes `source_file`, a path inside the project folder of the request, or `source_rows`, a list of records for a small input. A source file is read only from the project's `sources` folder, as in `sources/items.csv`: not from its target files, its project file, another folder, or a path with a hidden part. The dispatcher resolves the path through a checked root made from the project folder, which follows a link only while it stays inside the project; what stands at the name must be a file, not a link, a folder, or a device. It is read once, off the async threads, at most the smaller of `files.max_upload_bytes` and the engine's source limit, and its bytes are stored as a blob; rows are stored as the bytes of a JSON document, at most 256 KiB. The blob's name is part of the digest that makes a run the same run. The runner hands the stored bytes to every job of the run, so the engine is never given a path and a resume reads the same bytes. A tool that reads a source and is given none, or the other way about, is `input`, `schema_invalid`. Not built: rows for a source that another reader than `json` reads, which needs `a2m-engine` to take rows.

### The protected settings and what is in force

Whether something may happen is the policy engine's to decide, so a setting about it is in force only as far as the policy engine decides by it. `Kernel::open` opens the policy engine with the built-in set of the profile, `BuiltIn::local(user, workspace)` or `BuiltIn::server()`, with the installation's policy settings carried into it by `BuiltIn::with_settings`; the policy engine keeps them in force under any set published later, which can only tighten them ([decision 0145](../../decisions/0145-only-the-policy-engine-clears-a-write-and-the-ledger-reads-the-whole-clearance.md)). The module `bounds` says how each protected setting is put in force:

| Setting | In force through | A value other than the default |
| --- | --- | --- |
| `policy.approve_writes`, `policy.max_records_per_run`, `policy.approval_window_minutes`, `policy.agent_effect_ceiling`, `policy.writes_outside_tests`, `policy.two_person_irreversible` | The policy engine's settings | In force |
| `policy.approve_writes` = `named_approver` on one machine, or a word the policy engine has not | Nothing | Stops the kernel |
| `policy.pre_approval` | The policy engine's settings, and the guard: with it off, a write is never begun on a pre-approval | In force |
| `policy.preview_freshness_seconds` | The engine's freshness window | In force |
| `files.allowed_roots` | The dispatcher's check of a stated project folder | In force |

"Stops the kernel" means: the policy part is impaired with `policy`, `setting_not_in_force`, which names the setting, the value it was given, where it was set, and the value in force; the kernel is `stopped`; `engine.status` and `doctor.run` give the reason; and `config.show` marks the setting `in_force: false` with a warning. A limit is never reported that is not kept: one test per setting changes it and sees the policy engine's decision change. The presets `cautious` and `ci` run. The question "may this write proceed without a person" also says whether every record of the run is a declared test record of its environment, when that environment is a test one. The question "may this write proceed without a person" carries the run's record count, from the ledger, and the tool's hazard, from the plan.

The `Response` is the same structure for every face: `ok`, `action`, `result`, `error`, `warnings`, `next`, the recorded decision, and an exit class (succeeded, failed, malformed, person needed). A run that failed carries both a result, which names the run, and an error; a run that rests waiting for a person carries a result and the exit class `person_needed`. Each face renders it without branching on its content: the command face as the [output contract](../../reference/technical-reference.md#command-output-contract), the MCP face as structured content, REST as JSON with a status code.

### `Kernel::open` and health

| Part | Opened by | If it cannot be opened | State |
| --- | --- | --- | --- |
| Telemetry | `a2m-telemetry` | Logs to standard error only | `ok` |
| Store and migrations | `a2m-ledger` | Half-migrated, corrupt, or unreachable | `stopped` |
| Vault | `a2m-vault` | No session can be restored or saved; no evidence can be sealed, so nothing can be marked verified | `degraded` |
| Evidence space | `a2m-ledger` | Under the configured free-space floor | `degraded` |
| Policy set | `a2m-policy` | The previous valid version stays in force; with none at all every decision denies | `stopped` if none |
| Plans | `a2m-ledger` | A plan that fails its hash check is unavailable alone | `ok` |
| Keeper, scheduler, engine | `a2m-auth`, `a2m-sched`, `a2m-engine` | Not expected to fail; a failure is `stopped` | |

The kernel is always returned, so that the front door can answer `engine.status` and `doctor.run` with the reason instead of failing to start. Opening is two calls, `prepare` and `open`, because the provider registry is built between them from the redactor and telemetry the first one makes. The evidence-space row is not built. The state is re-evaluated whenever a part reports a change, and a move to a lower state is an event in the audit log.

Before anything else on open, the store's runs left `running` under a lease nobody holds are set to `stopped` with `possible_commit` / `runner_lost` ([spec section 222](../../spec/2-kernel/222-front-doors-and-the-local-engine.md)). Their reconcile is a job like any other. Not built at open: today such a run is stopped by the scheduler's sweep once its lease has ended, so an engine started within the lease of one that was killed sees the run as `running` until then.

### The in-process runner loop

```rust
sched.hello(offer).await?;                           // once, before the first claim
loop {
    for lease in sched.claim(runner, free_slots, wait).await? {   // waits on work arriving
        // one thread per job in hand, driven by the same runtime
        let control = InProcessControl::new(kernel, lease.fence(), lease.lease_ref(), run, plan, caller);
        let outcome = engine.run_job(&job_spec, &control).await;  // a task sends heartbeats meanwhile
        control.apply(outcome.run_state(), outcome.failure()).await?;   // under the job's lease
        sched.finish(&lease.lease_ref(), job_end).await?;
    }
}
```

Slots are a count given to `start_runner_loop`, offered for every registered mode. The keeper's work does not take a slot in one process: it runs on the keeper's own worker (`InProcessWork` of `a2m-auth`), so a run waiting for a renewal can never hold the slot the renewal needs. In one process the lease is at least ten minutes: the engine sends a save only when the lease leaves time for the whole binding, each step at its time limit, and counts a step that states none as thirty seconds, so ten minutes covers a binding of nineteen such steps. A job the engine gives back with a full lease left stops with `busy` / `lease_too_short` ([decision 0095](../../decisions/0095-the-settings-loader-and-the-in-process-runner-as-built.md)). The lease rules of spec section 223 apply unchanged: the lease is counted on a steady clock, heartbeats are sent by a task per held job, and the engine stops acting before the lease runs out. Keeping them on one machine is what lets hypothesis H-21 be measured.

A job of a tool that writes names what it must hold alone: one key for every surface a write of the tool may be made on, of the kind its mode declares, from `Engine::exclusive_needs` and `ResourceKey::held_alone`. The scheduler takes all of a job's keys in the transaction that leases it, or none, so two jobs never wait for each other; a second write on the same surface stays queued until the first job has ended. A read names none. A system job that reaches this loop is ended as stopped with `busy`, `system_job_not_carried`, never as done: in one process the keeper's work, a sign-out and a forget included, is a call on the keeper's own worker that is waited for to its answer, so none is queued and none can be lost on the way.

After a claim the store could not answer, the loop waits for the kernel's wake notice or thirty seconds and tries again. That is an interval between tries and not a wait for a condition: the store gives no notice that it can be asked again.

The list is the spine's, in its order. The engine opens nothing itself: no store, no policy engine, no compiler.

The engine gives up a `Control` call that does not answer in time and drops its future. Every call below is one transaction of the store, which commits or not as a whole whoever still waits for it. What follows a commit is a notice in memory, given by a guard as the call ends, however it ends. `Moved` to the state the run is already in is no move. The runner fills a job for a run that was worked on before from the ledger alone: its records, their stored values, and, from the run's `need` events, what was recorded before each commit point.

| `Control` asks for | In process, answered by |
| --- | --- |
| The plan by its hash | `a2m-ledger`; the hash is checked with the check in `a2m-core` before the plan is handed over |
| The policy decision at guard | `a2m-policy` `decide`, recorded like every decision |
| A session lease | The keeper in `a2m-auth` |
| A record lock | `a2m-ledger`, which owns the lock table and its rules, under the job's lease number; when the lock is held, `a2m-sched` queues the wait |
| A rate budget | `a2m-sched` |
| The approval check for a diff hash | `a2m-policy`. A refusal of "write without a person", a kill switch or a policy engine that could not decide, stops the run; only a plain denial goes on to a person's approval |
| Recording an intent, and its token | `a2m-ledger`, in one transaction that also checks the approval policy recorded; the reply is the token the driver's handle requires |
| Sending events and evidence, including the finished trace and a binding's health | `a2m-ledger`. After every write the kernel's notice that a run changed is given, which wakes whoever waits in `next_events` or for a run to rest. A verified record is marked through `Store::store_verified`, so its event is `verified`. `trace.show` reads the trace signature from the last line of the stored trace |

In one process `KeeperWork` is `a2m-auth`'s `InProcessWork` over the keeper's own worker; this crate supplies its two halves, the way to a person and the probe. The probe, `ProbeBridge`, submits a run of the tool that runs the entry's `probe` operation with the plan's `probe_inputs`, as the session's person, as a `session_keepalive` job; the runner loop hands the engine an ordinary `execute` job whose control leases the session for purpose `Probe`, and the probe's answer is how the run ended. The probe of a session is that of the published version a run last leased it under, noted in memory as the lease is given. The runner loop serves one slot kept for keeper jobs with a claim loop of its own ([decision 0353](../../decisions/0353-a-session-is-probed-by-a-keeper-job-run-through-the-engine-in-a-slot-kept-for-it.md)). Submitting a keeper job addressed to a session's home runner is for a runner on another machine and is not built. When the keeper needs a person, to sign in or to give consent on a link, it asks through the keeper's own interface in `a2m-auth`, and this crate answers by calling the role's `PersonSurface::ask_session` with the keeper's own request: a link to open in the person's own browser, a code to enter, or a notice. `ask_sign_in` gives the link that goes into the result of a run that rests at `needs_login`.

### Generated faces

Rust faces are derived at run time from the `Registry`. Code in other languages is generated at build time from the recorded catalog. Nothing is written by hand.

| Face | Generated by | From |
| --- | --- | --- |
| Command | The product crate builds the command tree when the program starts | `Registry`: noun, verb, input schema to flags, the declared positional field |
| MCP tool | `a2m-mcp`, when a tool list is asked for | `Registry`: `a2m_<noun>_<verb>`, listed per toolset, annotations from the effect |
| REST route | `a2m-api`, when its router is built | `Registry`: `POST /api/v1/actions/<noun>.<verb>`, and the `rest_alias` routes |
| OpenAPI document | `a2m-api` | `Registry` |
| SDK methods | A code-generation task in `xtask`, written to `bindings/` | The recorded catalog |
| Reference pages for `docs.get` | The same task | The recorded catalog and the documents. The guides in `guides/*.md` are built into the binary and served first, by the name on their first line; each ends with whether every action it names is built, read from the registry as it is asked for ([decision 0159](../../decisions/0159-a-lost-answer-is-a-write-in-doubt-at-the-doors-and-in-the-clients.md)) |

Two checks hold this. **Parity:** a test enumerates the live command tree, MCP tool list, router, and SDK method list, and compares each with the registry, failing on anything extra or missing; process commands and shortcuts are the only words allowed beside the nouns. **Recorded catalog:** the build writes the catalog with its digest; the engine compares the digest at start, and CI fails when the checked-in SDKs were generated from another digest.

### The scout

As built ([decision 0170](../../decisions/0170-the-scout-identifies-each-surface-with-one-discovery-job-and-stores-each-stage.md) for identify and the stages, [decision 0171](../../decisions/0171-the-scouts-report-is-hashed-data-with-the-systems-words-kept-apart.md) for the report). The handlers are in `handlers/scout.rs` and `handlers/scout_report.rs`.

- **Identify** (`target.probe`, and the first stage of a scout). For each declared surface: what the registry says (kind, archetype, provider, whether its kind keeps a session, each discoverer that suits it with its declared kinds, sources, highest level, and bounds), then one `Discover` job of a run of its own, submitted as `discover.run` submits it. The engine runs every discoverer that suits the surface in that one job, within its declared bounds. A surface with no provider or no suitable discoverer is reported as `no_provider` or `no_discoverer`, and nothing runs for it. The run's control keeps, beside the findings, what each discoverer reported (`actions_discovery_results`), so that a run's findings and the bounds each discoverer reached are known.
- **`auth.detect`.** The same identify, and per surface claims for a person to confirm: whether its kind keeps a session, the mode and provider `auth.yaml` declares for it, and what its probe met. A `recommendation` object stands beside what the files declare: the fixed rules of `a2m-auth` over the `sign_in_method` findings of the surface's discovery, marked `inferred`, with `choices` (mode, provider, other providers that suit it, a reason), `questions`, and `unknown`. What the files declare is kept as declared, and the surface's `level` is `inferred` only when they declare nothing and a choice was found ([decision 0367](../../decisions/0367-auth-detect-recommends-a-mode-and-a-provider-by-fixed-rules-and-marks-it-inferred.md)); with no finding, the method and identity mode are named in `unknown`. The scout's report carries the same entries.
- **A scout** is a row of `actions_scouts`, by person and target, not a ledger run; its system work is ordinary runs. It is kept by its project folder too: the digest of the folder it was made in is stored with its first stage, and `scout.run` continues, and `scout.report` reads, only a scout of the call's project (audit finding A3; a column of `actions_scouts` would be the cleaner home, and is the ledger's migration to make). `scout.run` continues the newest unfinished scout of the target by the same person in the same project, or starts one (`fresh: true` always starts one). Each stage is stored in `actions_scout_stages`, already redacted, with the hash of the stage before it as its input, before the next begins. The scout waits for a discovery to its end, as long as its discoverers may take by their manifests, and for a trial to its end or `TRIAL_BOUND` (two minutes from its submission), whatever `wait_seconds` says: a call never leaves a run it started in hand behind it, so an engine that runs inside a command and ends with it leaves no job claimed for a lease to wait out. `wait_seconds` is looked at between trials, after one new trial at least; when it has passed, the try stage is not stored and the call answers `state: working`, with `next` the same call ([decision 0189](../../decisions/0189-a-developers-first-hour-runs-without-hand-steps.md)).

| # | Stage | As built | Touches the target |
| --- | --- | --- | --- |
| 0 | Consent and budget | The project the scout is kept by; the files the call handed it, by name; what is true of consent: none is recorded, and none is recorded on a person's behalf, with who asked, an agent or a person (audit finding A4); the dispatcher's recorded decision. The budget settings are not kept, and the stage says so | No |
| 1 | Identify | As above. Drafts that do not compile are reported, and the scout goes on | Yes, in the sandbox, read only |
| 2 | Discover | The findings of stage 1's runs, by discoverer, with what each reported; each text the system gave under `untrusted` | No: the job of stage 1 ran them |
| 3 | Compare | The drafter over a copy of the target's folder in the engine's cache folder, read through checked paths, so no link is followed, at most 4,096 files and 64 MiB, and removed after; what a stopped engine left there is removed when the next one opens (audit finding A5); a file it would write where the target holds one a person wrote is a difference and an open question. The comparison of each pack's rule (section 218, roadmap E3) is not built | No |
| 4 | Draft | The drafter in the target's folder: new files and its own unchanged drafts only, never a write. Its open questions go to the unknowns ledger, each asked once. A draft with an open question, and each file of its finding that names something the held draft defines, is written under `held/`, which the loader does not read; `held_back` lists those files ([decision 0298](../../decisions/0298-a-draft-with-an-open-question-waits-in-a-holding-folder.md)) | No |
| 5 | Check | The drafts compiled in draft mode | No |
| 6 | Try | Each drafted read operation, with its first example or with no input when it needs none, five times one after another, as runs of its own whose digest names the scout, the plan, and the try, so that each is submitted once and a continuation finds it again, ended or not. A try that does not complete ends the read's tries: the read is `failed`, tried and not proven, with the failure's class and code; one that does not end within `TRIAL_BOUND` is cancelled, and the read is `not_ended`. `proven` when all five completed with one result, at `executed`. A read that states no example of its one key input is tried last, with the first item of the one list output of the key's name that a read proven earlier in the same scout, on the same surface, answered (`key_from_list`); the key is stored only in the trial's own stored call, and the report says `input_from`, never the value. Anything else that needs an input it gives no example of is not tried. Off when `authoring.scout.try_reads` is off. The target is compiled as the loader reads it, so held drafts are not in it; `left_out` lists them, and any file outside `held/` that holds an open question | Yes, in the sandbox, read only |
| 7 | Report | `content`, hashed; the runs beside it; a plain rendering that quotes nothing the system said | No |

Stages 3 to 6 need a project that holds the target's folder, and say so without one. `authoring.scout.run` set to `off` refuses `scout.run` with `policy`, `scout_off`. `scout.report` gives the newest finished scout's report by the caller's person, and warns when a newer one has not finished. Neither action answers with the whole report: `scout_report::shown` cuts each list named in `PARTS` to its first 20 entries, adds `counts`, `shown_per_list` and `cut_lists`, and bounds the text; the stored stage 7 and its hash are not touched. `scout.report` with `part`, `limit` and `offset` (the registry's own paging arguments, as `finding.list` has them) reads one of those lists whole from the stored report, in its stored order; `part` is a registered word, so an unknown name is `schema_invalid`. No stage has a branch that a setting can turn into a write: a `Discover` job and a `Try` job run in the engine's sandbox, where a discoverer is given only what `SandboxPort` in `a2m-provider` offers and no intent token exists; a trial is of a read only.

**Files a person hands the scout** ([decision 0263](../../decisions/0263-a-person-hands-the-scout-files-of-the-projects-sources-folder-by-names-the-discoverers-declare.md), "The registry's half, as built"). `scout.run` and `discover.run` take `files`, each name a discoverer reads to a file's path from the project folder, such as `{"description": "sources/example-openapi.json"}`. Each path is checked when asked, by the code that checks a source file of `tool.call` (`source_file` in `handlers/tools.rs`): inside `sources/`, no hidden part, through a checked root of the project folder, a regular file, at most the smaller of `files.max_upload_bytes` and the engine's source limit; nothing is read. A name that no discoverer suiting an observed surface reads (`Registry::files_taken`) is `input`, `discovery_file_not_taken`, and the fix lists the names that are read. Both refusals come before anything is stored or submitted. Each surface's discovery is handed only the files whose names its discoverers read; they are a record `files` of the discovery run's stored call, so part of its input hash, and a call that hands none stores none, as before. The runner decodes them and `InProcessControl::discovery_files` hands them to the engine, which checks each again. The scout keeps them in stage 0, `files`, and its report shows them: an unfinished scout goes on with the files it began with, checked again; a call that names other files starts a new scout. `spec.import` is `scout.run` with `fresh` and `files = {description: file}`, and no logic of its own.

A read whose last trial rests at `needs_login` is `waiting` in the try stage, with the surface its binding acts on and `session.login` as `next`; the stage's `counts` add up (`drafted` is `proven`, `waiting`, `failed`, `not_tried`, and `held`), and `draft.try` names the same `next` and takes up again, once, a trial it is given back that was made before the call and waits for a sign-in. `target.add` writes a surface's `fields` beside its address, and the draft compile checks them against the kind ([decision 0354](../../decisions/0354-a-read-that-waits-for-a-sign-in-as-built-and-a-surface-added-with-its-own-fields.md)).

Not built: a probe at an address with no declared surface, which needs a discovery job that names an address and no plan; a budget for a whole scout; stage 6 comparing trace signatures as well as results; any action that fills the discovery focus, so the HTTP pack's `description_path` is still unreachable.

### Promoting a first binding, and removing a draft

[Decision 0189](../../decisions/0189-a-developers-first-hour-runs-without-hand-steps.md), in `handlers/promote.rs` and `handlers/drafts.rs`.

- **`binding.promote`** takes a `draft` binding that is its operation's first, with no `verified` binding beside it, named by `mode` and, where two share the mode, `position` ([decision 0200](../../decisions/0200-evidence-names-its-binding-and-a-recorded-fixture-says-nothing-of-the-original.md)). It compiles the drafts; takes a binding in `shadow` by the branch below; refuses a retired one, one beside a verified binding, and a mode with two bindings and no position (`binding_not_promotable`); runs the offline tests with the binding being promoted standing as the one that would act, and refuses, listing them, unless every test of the operation runs and passes, at least one ran this binding with the digest it has now, and, for a write, one of those sent its save through it (`promotion_unproven`); and, for a read, refuses unless the caller's completed trial of this binding, made from this project, with its digest now, is named by `run_id`, or the newest finished scout of the target in this project proved this binding, with that digest, at `executed`. Evidence earned by another binding, or by this one before it changed, is never taken. It rewrites the binding's `status` in the operation's file to `verified`, the value on its line or a new line after `mode`, checks by compiling that the operation is otherwise the same plan, writes it through the checked path as `draft.write` does, refused where the file changed since it was read, and records an audit entry with the evidence, which names the binding. A binding already verified is answered `promoted: false`. For an operation that writes it is a person's act: the dispatcher asks nothing (`loosens` is false for it, since the effect is known only once compiled), and the handler hands an agent to `ask_person` and asks a caller at a terminal for presence; the person's press runs it again with the proof.
- **Which binding a test or a trial runs** ([decision 0200](../../decisions/0200-evidence-names-its-binding-and-a-recorded-fixture-says-nothing-of-the-original.md)): the one it names, by place and mode, else the one that would act live (`default_binding` of `a2m-engine`: the first `verified`, else the first not retired), handed to the engine as `JobSpec::binding`, which runs it alone. An offline test names it with `binding:` (`Compiled::test_bindings`); `draft.try` and `fixture.record` with `mode` and `position`; the scout tries the default. Every test outcome, a trial's stored call (`CallerFacts::binding`), the scout's `tried` entry, and `fixture.record`'s result name the binding and its digest. The publish gate asks, for every `verified` binding and the one being promoted, a passing test that ran it (`binding_untested`), and for a write one that sent its save through it (`no_save_sent`), each named by tool and binding.
- **A binding is never written `verified` by hand through the registry** (audit finding PM-2): `draft.write` refuses, `binding_not_promotable`, a text in which a binding is `verified` where the target's files do not hold it, at that place of that operation, `verified` and written the same; a changed binding is written `draft` and promoted again. A person's own editor is outside this action: there, the publish page and the gate hold. `draft.write` and `draft.delete` refuse a path through a link below the target's folder (DD-1).
- **The publish page** lists each binding of the plan that is `verified` where the current version holds none verified for the same operation and mode (`Kernel::first_to_act`), and, where it holds one of the mode that does something else (by `binding_digest`), says the binding acts as changed.
- **`binding.promote` of a binding in `shadow`** ([decision 0219](../../decisions/0219-shadow-bindings-reach-the-product-and-the-first-hour-is-mended.md), on [decision 0207](../../decisions/0207-a-shadow-binding-reads-beside-the-verified-one-and-a-person-promotes-it-on-a-run-of-exact-matches.md)) is a person's act: an agent is refused `policy`/`person_required`. The digest is `BindingPin::of` over the compiled drafts, which a test holds equal to the published plan's while the binding's files are unchanged. The ledger's `Shadows::promote`, with the threshold of `engine.shadow.matches_to_promote.read`/`.write`, records the promotion first (`policy`/`shadow_not_promotable` with the reason otherwise); the status is then rewritten as for a first binding; a promotion recorded whose rewrite did not happen is finished by the next call. `InProcessControl` keeps every comparison the engine makes (`keeps_shadow`, `shadow`); the engine's bound is `engine.shadow.time_limit_ms`. A shadow read is a request and takes the surface's rate budget.
- **`shadow.report`** gives, for each binding compared, by operation and digest: matches, mismatches, not compared, matches in a row, the threshold, whether it is promotable now and why not, the last mismatch's moment, run, and the outputs that differed by name and kind (never a value), and the promotion when there is one; `next` names `binding.promote` for one that earned it.
- **The access actions and the reach** ([decision 0339](../../decisions/0339-the-access-actions-the-built-in-toolsets-and-where-a-reach-is-kept.md)). `toolset.create`, `toolset.update`, `toolset.remove`, `grant.add`, `grant.remove`, and `policy.publish` are a person's act (`PersonNeed::Presence`) that publishes one new version through `Policy::publish_change` or `publish_reviewed`, with the presence check as `PublishProof.presence`, an audit entry, and the caller's own `tests` rows; a `reviewer` is asked for proof of presence of their own through the person surface. `policy.test` and `policy.publish` read a JSON policy file inside the call's checked project folder. `policy.explain` is asked of the policy engine for the auditor role or the administrator role. `toolset.list`, `grant.list`, `policy.versions`, `policy.tools_allowed`, and `policy.who_can` only read. `Kernel::toolset_for_decision` leaves `use`, `build`, and `admin` out of a decision's context when the set in force does not define a toolset of that name. The reaches are the store fact `net.reaches`, written only by `reach.name` and `reach.withdraw` (a person's act, refused to an agent again in the handler) and read by `reach.list` and `Kernel::reaches_and_network`, which gives the reaches and the fingerprint now for `NetworkPlace::of_in_reach`; `net.current` shows the facts of the network now and no digest.
- **`client.connect`** ([decision 0219](../../decisions/0219-shadow-bindings-reach-the-product-and-the-first-hour-is-mended.md)) gives the server entry an MCP client needs to start `anything2mcp --project <folder> bridge`: printed for `generic`, the default, with no file read or written; or set in the file a client profile names. A profile is data the local user keeps in `<config>/clients/<name>.toml` (`format`, `key`, a path per system); none ships. Writing is a local person's act (an agent is refused `person_required`), shown first and written only with `yes`, setting only the entry of its name, through a file written whole beside and renamed; a file that is not a JSON object is refused `client_config_refused`.
- **Hints that read the gates** ([decision 0219](../../decisions/0219-shadow-bindings-reach-the-product-and-the-first-hour-is-mended.md)). `draft.lint` and `target.status` point at the publish only when the publish compile and the offline tests pass (`publish_next`); otherwise at `test.run`, naming the operations with no test. `test.run` that did not pass ends `failed`, exit code 1, with the publish's refusal. The degraded refusal carries its cause's own fix; a vault that never attached because its passphrase was not given refuses every action that needs the store with that failure, `input`/`vault_passphrase_missing`; a key store that does not answer leaves the reads that need no secret running. The publish page says when `production` is implied rather than declared.
- **`draft.delete`** removes one file (`file`), or several (`files`, every one checked before any is removed, all or none), of the target's folder by a checked `RelPath` with no hidden part: never a top-level file of the target (`target.yaml`, `auth.yaml`, `types.yaml`), a name no file kind claims, a link, a folder, or a device. A published version's sources are held in the store with its plan, and no path reaches them. It answers with the removed file's digest and the lint of what is left.
- **The gate stays whole.** `version.publish` does not publish only the operations that have tests: an operation with no offline test refuses the publish with `publish_gates_not_passed`, naming the files, with `test.run` and `draft.delete` as the fix; `test.run` lists each under `missing` with both as `next`.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-local` | `Kernel::open`, `dispatch`, `health`, `runs_in_hand` for its idle stop, `Paths`, `PersonSurface` to implement, and the `ApprovalDesk` it is handed: `waiting`, `preview`, `approve`, `decline` |
| `a2m-mcp` | `Registry` for tool lists, `dispatch` (`tool.search` with `full` for the published tools), `next_events` for progress |
| `a2m-api` | `Registry` for routes and OpenAPI, `dispatch`, `next_events` for an event stream, `PersonSurface` to implement |
| `a2m-runner` | `Paths`, `KernelSettings` for its own group; not the `Kernel`, which a runner on another machine does not open |
| `anything2mcp` | `Registry::build`, the settings loader, `Kernel::open`, `start_runner_loop`, `Shortcut`s for the command line |

| This crate calls | For |
| --- | --- |
| `a2m-ledger` | Opening the store, plans, runs, record locks, intents, findings, scout stages, the catalog digest |
| `a2m-vault` | Opening the vault; the `Redact` it implements, handed to every front door through the `Kernel` |
| `a2m-policy` | `decide`, approval checks, `ApprovalDesk` answers |
| `a2m-auth` | The keeper: status, sign-in requests, `perform` on the loop |
| `a2m-files` | Lint, compile, the drafter; this crate is their only caller |
| `a2m-provider` | The provider registry, contributed actions' handlers, the checked-path type, the comparison |
| `a2m-sched` | `submit`, `claim`, `finish`, `unfinished_job` for whoever waits for a run, waits for a held lock, budgets |
| `a2m-engine` | `run`, `reconcile`, `discover` |
| `a2m-netstate` | The seven readers of the machine's own networks, each run on a blocking thread; their serialized form is the result as it stands ([decision 0260](../../decisions/0260-the-machines-network-facts-are-served-as-reads-of-the-local-user-one-listed.md)) |

## State

| State | Where |
| --- | --- |
| The registry and the recorded catalog | Memory, frozen at build; the digest in the store |
| Health and its cause | Memory; each change an audit event |
| Toolsets | A table in the store, owned by this crate |
| Scout stage results | Tables in the store, owned by this crate: each scout (`actions_scouts`), each finished stage with its input hash and its result (`actions_scout_stages`), and what each discoverer of a discovery run reported (`actions_discovery_results`) |
| Webhook delivery outcomes | A table in the store, owned by this crate, beside the outbox the events are written to |
| What a run was called with | Two tables in the store, owned by this crate: the stored input and the caller's verified facts (`actions_run_calls`), and the stored source (`actions_run_sources`) |
| Which runs a person approved and nobody resumed yet; the link a run's person was last given | Memory, the newest 1024 runs of each; gone with the process. They shape what a reader is told and decide nothing |
| Large results | Blobs in the store, by content hash; on one machine also written under the project's `.anything2mcp/out/` by the engine, with a name the engine chose |

## Failures

The crate's own codes are declared once, each with its class, meaning, next step, and, for a `busy` code, whether it passes by itself, in its `failure_codes!` table, the module `codes` ([decision 0258](../../decisions/0258-every-crate-declares-its-failure-codes-in-one-table.md)); `schema_invalid`, `not_found`, `engine_degraded`, and `idempotency_key_invalid` stay the core's, the store's, and the scheduler's constants. Of its `busy` codes only `tool_changed` passes by itself: the call made again is decided anew. A test holds this table to the crate's.

| Class | Code | When |
| --- | --- | --- |
| `input` | `schema_invalid` | No such action; input breaks the schema |
| `input` | `not_found` | A valid input names something that does not exist: a tool, a run, a provider, a setting, a topic |
| `input` | `action_not_built` | The action is in the catalog and has no handler yet |
| `input` | `client_action` | An action the client end carries out reached the engine |
| `input` | `path_not_allowed` | No project folder, or a path that fails the check behind `a2m-provider`'s checked-path type |
| `input` | `settings_invalid` | An unknown key, a wrong type, or a file that is not TOML, with the file and line |
| `input` | `action_registry_invalid` | A duplicate action name, or a provider's action outside its name space |
| `policy` | Any code `decide` returns | Refused by policy, passed through unchanged |
| `input` | `network_not_recognizable` | The network the machine is on states too few facts to be recognized again, so no reach is named on it |
| `policy` | `person_required` | A person's act was asked for by a caller that acts for no person, or the surface has no page for it |
| `stale` | `drafts_changed` | The drafts of a publish a person was shown compile to another plan by the time they press |
| `policy` | `scout_off` | `scout.run` was called and `authoring.scout.run` is `off` |
| `input` | `discovery_file_not_taken` | `scout.run`, `discover.run`, or `spec.import` names a file under a name no discoverer suiting the observed surfaces reads; nothing starts |
| `input` | `binding_not_promotable` | `binding.promote` was asked for a binding that is not a drafted first binding, or whose status cannot be rewritten by rule |
| `input` | `promotion_unproven` | `binding.promote` found an offline test of the operation missing or not passing, or no trial that proved the read |
| `input` | `publish_gates_not_passed` | A publish whose offline tests did not all run and pass, or an operation with no passing offline test |
| `policy` | `offline_writes_nothing` | An offline test asked for something that would store, lease, or clear a write |
| `input` | `fixture_no_scrub_rule` | `fixture.record` met an observed value no scrubbing rule covers; nothing is written |
| `input` | `fixture_read_failed` | The read `fixture.record` runs did not complete in the sandbox, or gave nothing to record |
| `input` | `fixture_not_replayed` | The test `fixture.record` made does not pass against its scrubbed fixture, or an observed value would be in a file |
| `input` | `client_config_refused` | `client.connect` cannot set a client's configuration; nothing is written |
| `rejected` | `run_not_completed` | `run.get` or a tool call reports a run that ended in a state other than completed and carried no failure of its own |
| `policy` | `approval_refused` | What `run.get` reports for a run a person declined, or whose approval window ended; and what the desk answers to a proof of presence that is not the approver's or not of now |
| `policy` | `kill_switch_on` | A kill switch covers the work a call would start or resume |
| `policy` | `approval_stale` | The desk was asked to show a stored diff that is not the one the run waits on |
| `input` | `file_unavailable` | A source file, or a file handed to a discovery, that is missing, unreadable, or larger than a source may be |
| `policy` | `caller_not_verified` | A credential this engine does not accept |
| `policy` | `setting_protected` | A layer loosened a protected setting, or set a key it may not |
| `policy` | `setting_locked` | A higher layer changed a key the installation file locks |
| `policy` | `setting_not_in_force` | A protected setting is set to a value the policy engine does not work by; the kernel is stopped |
| `policy` | `presence_required` | A proof of presence that is another person's, or not of now, handed to the dispatcher |
| `input` | `idempotency_key_invalid` | A key that is not 1 to 200 printable characters, or two different keys on one request |
| `input` | `idempotency_key_reused` | A key the caller gave before with another call |
| `busy` | `run_call_unreadable` | What a run was called with is missing from the store or cannot be read; the run is not carried out |
| `busy` | `tool_changed` | A tool was published again, with another effect, between the decision about a call and the call |
| `busy` | `system_job_not_carried` | The in-process runner loop was handed a job that is not a run's |
| `busy` | `store_unavailable` | A change was made and its audit entry could not be written |
| `busy` | `engine_degraded` | The health state forbids the action |
| `busy` | `lease_too_short` | A step may take longer than a job's lease lasts |
| `busy` | `queue_full` and the scheduler's other codes | Passed through from `a2m-sched` |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Every dispatch records one decision before the handler runs | `every_dispatch_records_one_decision_and_a_refusal_carries_its_four_parts`: a missing tool, an action not built, and a refused publish each carry a decision. Not yet proven with a refusing policy set, or for every action |
| No action reaches `ApprovalDesk` | The kernel holds no desk: it is moved to the person surface. `no_action_can_approve`; the write test approves at the desk and through nothing else |
| The four faces of an action share its name and its schema | `every_action_has_all_four_faces_and_they_share_its_name`. The parity test through live faces belongs to the role crates |
| Nothing leaves `dispatch` without passing `Redact` | `a_planted_secret_appears_in_no_result`, through the dispatcher |
| A write never runs in `degraded` or `stopped` | `a_stopped_store_serves_status_and_nothing_else`, `without_a_key_store_the_kernel_is_degraded_and_writes_nothing` |
| The registry equals the action catalog, in names, roles, and listing | `the_registry_holds_exactly_the_actions_of_the_catalog`, which reads the catalog document |
| An empty file is a complete configuration; a layer that may only tighten never loosens; every value names its layer | `tests/settings.rs` |
| The scout never obtains a write token, and no step that saves reaches a system | `a_scout_drafts_reads_by_rule_tries_them_and_never_writes`, which asks the stand's driver what reached it; `a_scout_identifies_reports_the_same_twice_and_writes_nothing`, which asks the test application |
| The same system in the same state gives the same report, by hash; a stopped scout continues where it stopped | The same two tests |
| Each trial of a scout is submitted once; a call never leaves a trial it started in hand; a trial that fails ends its read's tries, by name; a read by key takes its key from a list proven in the same scout, and the key is in no report and no draft | `a_scouts_trials_are_made_once_end_by_name_and_take_a_key_from_a_list`; through the product, `first_hour.rs`, with an engine served and with one inside each command |
| A scout is kept by its project; no consent is recorded on a person's behalf | `a_scout_is_kept_by_its_project_and_records_no_consent_for_a_person` |
| The compare stage's copy follows no link, is bounded, and does not outlive its engine | `the_compare_copy_follows_no_link_out_of_the_project`, `a_link_swapped_in_after_the_listing_is_not_read`, `the_compare_copy_is_bounded`, `a_scouts_copy_left_in_the_cache_is_removed_when_the_next_engine_opens` |
| A first binding is made verified only on its evidence, by a rewrite of its status alone; a write's promote is a person's act; a draft is removed only by its checked path | `a_first_binding_is_promoted_on_its_evidence_and_a_draft_is_removed_by_its_path` |
| A keeper job always has a slot | `a_keeper_job_takes_the_slot_kept_for_it_and_no_other_job_ever_does` in `a2m-sched`; the runner loop serves the kept slot with a claim loop of its own |
| A session is probed through the engine at half its idle timeout, only while wanted, and a probe that cannot run changes nothing | `a_probe_runs_once_per_half_idle_and_stops_after_the_keep_alive_window`, `a_probe_that_finds_the_session_ended_moves_it_to_needs_person_and_the_next_call_returns_needs_login`, `a_probe_that_cannot_run_changes_nothing` |
| A decline ends the run, and the caller is told | `a_decline_ends_the_run_as_expired_and_the_caller_is_told` |
| After an approval, one `run.resume` completes the write | `one_resume_after_an_approval_completes_the_write_every_time_under_load`: three hundred runs, with readers running beside them |
| Reading a run asks nobody anything | `reading_a_run_asks_nobody_and_an_approved_run_says_so` |
| The desk shows only the diff a waiting run carries, bounded and redacted | `the_desk_shows_the_change_a_waiting_run_would_make_and_no_other` |
| A source reaches the engine as stored bytes, never as a path, and a resume reads the same bytes | `a_source_file_reaches_a_pipeline_and_a_resume_reads_the_same_bytes`, which asks the test application what was written |
| A write refused for want of approval is a recorded decision | `a_write_refused_for_want_of_approval_is_recorded_as_a_decision` |
| A kill switch is one refusal wherever it is met | `a_kill_switch_is_one_refusal_wherever_it_is_met` |
| A result that carries `next` fits its action's output schema | `a_result_shows_exact_numbers_as_text_and_carries_its_next_step` |
| A pipeline over a source runs from a file of the project to verified records | `a_sheet_is_entered_row_by_row_from_a_file_of_the_project` |
| What was recorded before a commit point is handed back on resume | `a_key_assigned_before_the_commit_point_is_handed_back_when_the_run_is_resumed`: after a lost answer the record is read by the assigned code, not looked up again |
| A record still `planned` can be recorded as failed | `lines_whose_header_failed_are_recorded_as_blocked_and_written_once_it_is_settled` |
| A stated project folder is checked before any handler sees it | `a_stated_project_folder_is_checked_before_any_handler_sees_it` |
| The count of work in hand follows a run from queued to its end, and a run that rests for a person | `the_kernel_says_how_much_work_it_has_in_hand` |
| A run is read, resumed, and cancelled only by its person, and a resume keeps what was kept of its caller | `another_persons_run_is_not_read_resumed_or_cancelled_and_a_resume_keeps_its_caller` |
| A run whose stored call cannot be read is never run | `a_run_whose_stored_call_cannot_be_read_is_ended_and_never_run`, which asks the test application |
| The desk and the dispatcher take a proof of presence only of the person it is used for, and of now; an approval's audit entry says what the proof said | `the_desk_takes_a_proof_only_of_the_approver_and_of_now_and_records_what_it_says`, `the_dispatcher_takes_a_proof_of_presence_only_of_its_caller_and_of_now` |
| A protected setting is in force or the kernel is stopped with its name | `a_protected_setting_that_is_not_in_force_stops_the_kernel_and_is_never_shown_as_in_force`, `the_limits_the_settings_show_are_the_limits_the_policy_engine_keeps`, and the tests of `bounds` |
| A pre-approved write needs no person only where `policy.pre_approval` allows it | `a_pre_approved_write_needs_no_person_only_where_the_setting_allows_it` |
| A write the policy engine cannot decide is stopped | `a_write_the_policy_engine_cannot_decide_is_stopped_and_no_approval_carries_it` |
| A project folder holds a project file, is inside the allowed roots, and is no program's own folder; a source file is read only from `sources` | The tests of `project`; `a_stated_project_folder_is_checked_before_any_handler_sees_it`; `a_source_file_reaches_a_pipeline_and_a_resume_reads_the_same_bytes` |
| An act that is a person's when it loosens is asked for by the dispatcher | `an_act_that_is_a_persons_when_it_loosens_is_asked_for_by_the_dispatcher` |
| The tool that is run is the one the decision was made for; a debug print of a call shows no value | `a_tool_that_is_not_the_one_the_decision_was_made_for_is_not_run` |
| A caller made without a credential holds nothing | `the_caller_a_stopped_engine_answers_to_is_nobody_and_holds_nothing` |
| Two writes on one surface are one at a time | `two_writes_on_one_surface_are_one_at_a_time_by_the_scheduler` |
| A write binding of six steps at the default limit fits the lease | `a_write_whose_binding_has_six_steps_at_the_default_limit_fits_the_lease` |
| A follower is woken when its run comes to rest | `a_follower_is_woken_when_its_run_comes_to_rest_and_not_when_it_already_rests` |
| The same key gives back one run, unfinished or finished, and one write | `the_same_key_gives_back_one_run_and_one_write_finished_or_not`, which asks the test application |
| A key given with another call is refused; another person's key is theirs; a key is checked | `a_key_given_with_another_call_is_refused_and_another_persons_key_is_theirs` |
| `run.list` shows only the caller's runs, filtered, paged, and marked | `the_run_list_shows_only_the_callers_runs_filtered_paged_and_marked`, `a_run_whose_write_is_in_doubt_is_marked_in_the_list` |
| A project folder or file another account may change is refused | `a_project_folder_or_file_that_others_may_change_is_refused` (Linux and macOS) |
| A person's act an agent asks for is carried out once, from its page, by a proof given for it, as the person; a proof of another ask, another person, or another time does nothing; a publish rests on the plan the person was shown | `a_publish_an_agent_asks_for_is_carried_out_once_on_its_page_by_the_persons_proof`, `a_requested_publish_rests_on_the_drafts_shown_and_ends_unanswered_with_its_window`; through the real page in `a2m-local` |
| A draft is read and written only inside its target's folder, never through a link, and a file that does not read as its kind is not written; a trial and a test publish nothing | `an_agent_writes_reads_tries_and_tests_drafts_inside_the_project_alone`, which asks the file system |
| A discovery runs in the sandbox, stores what it finds, and writes nothing | `a_discovery_runs_in_the_sandbox_stores_its_findings_and_writes_nothing`, which asks the test application |
| A file handed to a discovery is one of the project's `sources` folder, under a name a discoverer reads, or nothing starts; a scout handed one drafts from it | The product's `scout_files.rs`: `a_description_given_as_a_file_is_imported_by_the_scout_through_the_command`, `a_register_map_given_as_a_file_is_read_by_the_device_scout`, `spec_import_drafts_from_a_file_as_the_scout_does`, `a_file_outside_the_sources_folder_a_hidden_file_or_a_link_out_is_refused_and_no_run_starts`, `a_name_no_discoverer_takes_is_refused_with_the_names_taken` |
| `init` writes a project that loads, and keeps what is there | `init_writes_a_project_that_loads_and_keeps_what_is_there` |
| A `Control` call dropped at a wait leaves nothing half-done | By construction: one transaction a call, and a guard for the notice. Not tested with a dropped call |

## Build order

1. Done: `ActionDef`, `ActionRegistry::build`, and the catalog test. Every action of the catalog is defined as data; one that is not built answers `input` / `action_not_built`.
2. Done: `Kernel::prepare` and `Kernel::open` with the store, the vault, policy, the keeper, the scheduler, and the engine; the runner loop; `InProcessControl`; the three health states; `Paths`; the settings loader (package 1.4a). Not built: the evidence-space check; the keeper's work for a runner on another machine. A session's probe is built ([decision 0353](../../decisions/0353-a-session-is-probed-by-a-keeper-job-run-through-the-engine-in-a-slot-kept-for-it.md)).
3. Done: the dispatcher with validation, the health check, the decision, the person need, and redaction; every face as data (`command_faces`, `mcp_tools`, `rest_routes`, `sdk_methods`, `McpResultFace`). Not built: the live parity test, which needs the role crates; the untrusted mark; large results as blobs.
4. Done: `decide` in the dispatcher and at guard, the approval check and `consume` in the transaction of `begin_writes`, `PersonSurface`, `ApprovalDesk` with its preview and a decline that ends the run, the vault and policy rows of health.
5. Not built: the keeper's work as a scheduled job, and bearer tokens and API keys in `verify_caller`.
6. Done ([decision 0164](../../decisions/0164-the-authoring-actions-served-through-the-engine.md)): `discover.run`, a `Discover` job per surface whose stored call names the surface and the focus, which the runner turns into the job's tool and input; `finding.list`, `finding.describe`, `unknown.list`; `test.run` as far as the schema and lint gate, with every offline test listed as not run, because no replay of a fixture through a driver exists below this crate. Done since ([decision 0170](../../decisions/0170-the-scout-identifies-each-surface-with-one-discovery-job-and-stores-each-stage.md), [decision 0171](../../decisions/0171-the-scouts-report-is-hashed-data-with-the-systems-words-kept-apart.md)): `target.probe`, `auth.detect`, `scout.run`, `scout.report`, and the `add` shortcut through to the report. Not built: what "The scout" names as not built.
7. Built, each with a test through the dispatcher: `action.search`, `action.describe`, `action.run`, `docs.get`, `engine.status`, `config.show`, `doctor.run`, `provider.list`, `provider.describe`, `target.add`, `target.probe`, `auth.detect`, `scout.run`, `scout.report`, `target.list` (the project's targets and the published ones), `target.status`, `draft.read`, `draft.write`, `draft.delete`, `draft.lint`, `draft.try` (drafted reads), `test.run` (the lint gate and the offline tests of reads), `publish.request`, `version.publish`, `binding.promote` (a drafted first binding, and a binding in shadow), `shadow.report`, `client.connect`, `plan.show`, `tool.search`, `tool.describe`, `tool.call`, `run.get`, `run.list`, `run.resume`, `run.cancel`, `trace.show`, `session.status`, `session.login`, `session.revoke`, `session.list`, `session.end`, `secret.set`, `killswitch.set`, `policy.explain`, `audit.query`. The four `engine.*` process actions and `self.update` are the client's. Every other action of the catalog is defined and not built. Shortcuts are data, and `Shortcut::on` says where one stops on this engine. The recorded catalog has a digest, kept in the store at each start; the generators that read it are not built.
9. Done (the safety audit, [decision 0152](../../decisions/0152-the-action-registry-and-telemetry-after-their-safety-audit.md)): whose a run is; the protected settings and what is in force; the bounds of a project folder and of a source file; the checked proof of presence; a run whose call cannot be read; presence when an act loosens; what a job holds alone; the in-process lease; `next_events` at rest; `tool.search` by kind; `McpResultFace::of_in`; `PersonSurface::presence`. Done since (decision 0159): `next_events_from` and `Followed`, so that a run that came to rest between a follower's two calls is told at once; the guides served by `docs.get`. Done since (decision 0145): every policy setting is carried into the policy engine; `policy.explain` answers only in the caller's workspace; a record's mode is read from the ledger's row. Done since (decision 0160): `run.list`; idempotency keys on `tool.call`; the owner rule of a project folder. Done since ([decision 0163](../../decisions/0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md)): the page on which a person carries out a person's act an agent asked for (sections 222 and 503), with `ApprovalDesk::person_act`, `carry_out`, `decline_act`, and `PersonSurface::ask_person_act`.
10. Done ([decision 0176](../../decisions/0176-the-action-registry-takes-up-the-kernels-second-pass.md)), `src/tests/pass_two.rs`: `Control::acknowledged` through `Store::store_acknowledged`, held to the diff the engine writes; `Control::keep_capture`, a blob of the short class and a `need` event of the run that names it; `Control::project_folder`, kept with the run's stored call; a run's failure rebuilt with `run_failure`, `said` shown as `{"by": "system", "text": ...}` apart from the sentence in the result, in the MCP face's error, and through `said_line`; `not_read_back` from the ledger's tally; `PreviewRecord::not_read_back`; a call given an unfinished earlier run back by rule 6 marked `earlier_run`; a provider whose manifest was refused listed by `provider.list` as `refused`.
11. Done ([decision 0177](../../decisions/0177-a-targets-offline-tests-run-against-a-replay-of-their-fixtures.md)): `test.run` runs each offline test of a read through `Engine::run_offline` against a `Replay` of its fixture (`src/offline.rs`), and compares the result with the test's after normalization. Done ([decision 0180](../../decisions/0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md)): the offline test of a write, in a throwaway ledger of its own with the test's one clearance, compared with the request its fixture recorded; a pipeline's source from its fixture folder; the publish gate in `version.publish` and `publish.request` (`offline::run_gates`, `GATES`, `publish_gates_not_passed`), with the gates' evidence stored with the version and listed on the person's page.
12. Done ([decision 0180](../../decisions/0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md)): the scout's try stage and `plan_to_observe` compile what can compile (`compile_what_can`). Drafts held in `held/` are not loaded, so they are not in the compile at all ([decision 0298](../../decisions/0298-a-draft-with-an-open-question-waits-in-a-holding-folder.md)); a file the loader does read that holds an open question (a draft written before that decision, or a person's file) is left out by this step and named, as before; a tried read keeps its result's shape, never its values; a question and the compiler's messages about drafts are carried under `untrusted` in the report and in `unknown.list`. A key for a try taken from a list read in the same run: built since (decision 0189, step 14), kept only in the trial's own stored call.
13. Done ([decision 0185](../../decisions/0185-a-record-with-no-key-is-never-proved-absent-by-one-look.md)), `src/tests/pass_two.rs`: the runner's snapshot gives a record the values recorded before its save only from `need` events after its current intent (`RecordRow::intent_seq`), so a reconcile is given the values of the attempt in doubt; `RecordSnapshot::intent_at` and `sent_by`, the time the ledger recorded what followed the send; `RunSnapshot::approved`, the stored diff read back; a job stopped with `possible_commit`, `cannot_reconcile` ends as resting, so the scheduler does not queue another reconcile at once and read the system again and again: the run rests `stopped` for a person; `PreviewRecord::bindings` and `binding_later` (`BINDING_LATER`, and `MAY_NOT_BE_READ_BACK` for a record that may be written by an acknowledged binding); what the system said, kept whole by the failure, cut for display only in `said_json` (with `cut`) and `said_line`, at `SAID_LONGEST` characters and never inside a redaction mark.
14. Done ([decision 0189](../../decisions/0189-a-developers-first-hour-runs-without-hand-steps.md)): the scout's try stage, each trial made once and waited for to its end; a key for a try from a list read in the same scout; a scout kept by its project and stage 0 without consent made up (audit findings A3, A4); the compare copy through checked paths, bounded, and cleared at open (A5); `binding.promote` for a drafted first binding; `draft.delete`; the bindings that act first, on the publish page; `missing` with `draft.delete` for an operation with no test. Promoting a `shadow` binding was not built here; it is now ([decision 0219](../../decisions/0219-shadow-bindings-reach-the-product-and-the-first-hour-is-mended.md)). Adopted from decision 0193: the scout drafts with the redactor (`Drafter::with_redact`) and stores each question with the system's words apart (`open.stored()`), and the report and `unknown.list` show the pack's sentence as `text` and only the quoted words under `untrusted` (`StoredQuestion::read`); `said_line` is `said_quoted` of `a2m-core`; `trace.show` passes every `said` through its `said_neutral`.
15. Done ([decision 0196](../../decisions/0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md)): `fixture.record` for reads (`src/fixture.rs`: a trial recorded through `Control::step_observed`, every value redacted and scrubbed by `Scrubber`'s fixed rules, the test run in memory and again from the files written, nothing written where a type has no rule); the publish gate passes a write only by a test that sent its save against a recorded request, and `sent: 0` names a test of the path where nothing is sent (AC-2); the gate tests the bytes the plan was compiled from, held once and tied to the plan's source digest, and its evidence names both (AC-3); the publish page carries each surface's kind, addresses, and notices and every lint warning (`PersonActAsk::surfaces`, `warnings`; AC-1), the approval page each record's surface notices (`PreviewRecord::notices`), `ApprovalPreview::write`, and `PreviewField::now_cannot_be_read`; the page's gate line counts offline tests only and says live ones are excused (AC-4); fixture values marked `untrusted` in `test.run` (A-5); a fixture folder reached part by part, never through a link, at most 256 entries and 64 MiB (A-2); a run's project folder checked again at every lease (A-4); `InProcessControl::other_records` over the store, counting only records whose write may have landed; `ActionContext::paths`, the call's own project folder; a scout kept by its row's `project` (migration 0018); `network.proxy` refused at load while no proxy is built. `target.add` stating a surface from an address: built since (step 16).
16. Done ([decision 0199](../../decisions/0199-the-halves-left-for-each-other-are-joined.md)): `target.add` given `address` and `kind` writes the target file with one surface of the kind, under the key `CompilerSet::address_key` gives, and one test environment, compiled as a draft before anything is written, a refusal naming the compiler's errors; an address with no kind is refused with the kinds the enabled packs offer, never guessed. The first hour has no hand step. A publish's surfaces and warnings are handed in `PersonActAsk::surfaces` and `warnings` alone, no longer also as lines of `details`. `fixture.record` scrubs each item of a list by the list's field type, so a list of choices keeps declared codes; a code the read observed is not looked for in the files written, since another code may stand for it. Tests: `a_target_is_added_to_a_project_and_its_first_file_is_linted`, `a_fixture_is_recorded_from_a_list_read_and_a_read_of_choices` (the stand's list read and read of choices), `a_records_preview_carries_the_notices_its_surface_kind_registers`. Not proven: a candidate whose recorded value was masked answering `CannotTell` through the real store: a masked value is never sent as an argument (`argument_masked`), so the test application's steps cannot land one, and a step that records a value it does not send needs a hook in the engine's test helper.
17. Done ([decision 0200](../../decisions/0200-evidence-names-its-binding-and-a-recorded-fixture-says-nothing-of-the-original.md)), the kernel's fourth safety pass: each test and trial runs, and its evidence names, one binding (AC-6, PM-3, PM-4: `evidence_earned_by_one_binding_is_never_taken_for_another`); `fixture.record` keeps as structure only the operation's declared names and the names the target's files state, puts a made-up key in place of every other, writes a choice as its first code, chooses every made-up value without looking at the original but where the replay needs it, checks every original value and key of 3 characters or more against what is written and counts the shorter, makes every file new and removes what it made on any failure, keeps its trial short and cancels one past its wait, and names its binding, its trial, and its limits (FX-1 to FX-6; `fixture.rs`'s tests, the product's `other_packs.rs` through the data and web packs); `draft.write` refuses a binding written `verified` (PM-2: `a_binding_is_never_written_verified_by_hand`) and a path through a link (DD-1); the answer to a driver counts only the attempt that may have landed and names no other person's run (OR-1); `run.resume` calls `Scheduler::resume` and says what a reconcile found holding the run says. Not built: a test of the other-person case through the store (a write by a second person needs that person's policy and approval in the test world; `told` is tested alone). Stated, not held: a driver may ask about any recorded name of the operation's records, not only those its own binding records; only in-process first-party drivers ask today, and the answer never gives a value back.
18. Done ([decision 0205](../../decisions/0205-a-sign-in-an-agent-asks-for-is-carried-out-by-its-person-on-the-engines-page.md)), `src/handlers/sessions.rs`: `session.login` from the sign-in the target's plan declares, with the keeper's entry built as a run's lease builds it (`control::entry_of`); an agent's ask handed to the person surface with `Kernel::ask_person` when the provider needs a person or a credential is not stored; `PersonActAsk::secrets` (`AskedSecret`), the credentials the page asks for, pinned with the surface and environment; `ApprovalDesk::carry_out_with` and `Call::given`, what the person typed, registered with the redactor first and sealed for the person; a sign-in's outcome never given back from the kept acts; `session.revoke`, `session.list`, `session.end`; the scout's `needs_login` for `credential_missing`, its stop before drafting (`state: needs_sign_in`, `next: session.login`, no stage stored), and `session.needed` per surface in the report. Tests: `a_sign_in_is_asked_for_by_name_and_a_session_is_ended_only_by_whose_it_is`, `a2m-local`'s `a_sign_in_that_asks_for_a_code_asks_on_the_persons_own_page_and_keeps_it_nowhere`, the product's `first_hour_signed_in.rs`. Not built: the 40 actions decision 0205 lists as unserved, each with what it waits for.
19. Done ([decision 0248](../../decisions/0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md)), after audit 5: `control::entry_of(plan, auth, environment, published)` fills `AuthEntry::surfaces` from the plan, marked published only for a plan that is not a draft the kernel keeps; `session.login` tells a published version's declaration from the drafts' by one digest of everything that decides what is sent where, asks the person for every drafts' sign-in, shows on the page every address a credential or a token goes to with whether it is published, pins the digest so a press after the drafts changed is refused (`drafts_changed`), and marks the person's press `shown_to_person`; a session holds for the caller only where `Keeper::allows` says so. The publish review lists each sign-in's provider, references, and addresses in each environment, and refuses one the keeper would refuse. Tests: `a_drafted_sign_in_sends_no_stored_credential_until_its_person_sees_every_address`, `the_publish_act_carries_the_typed_review_with_sign_in_addresses_from_the_keeper` (step 24).
20. Done ([decision 0273](../../decisions/0273-the-settings-keeper-and-the-registrys-owed-lines-after-a-second-reader.md)), after audit 6: the installation's settings file is checked and read through one handle, opened without following a link at its end, its device and number held to the entry the walk found; no group's write counts as the administrator's for it, the administrators' group of macOS included; the access lists on the way are read by `a2m-provider`'s `check_program`; the rule tests read each product file to its end but its final unit-test module; `doctor.run`'s `home` check names a folder that is not this user's, which the engine could not close; the policy engine is told a run's records are test records only from a published version's environment; `version.publish` refuses `0.0.0`. Tests: `keeper::tests`, `the_scan_of_product_sources_cuts_only_a_files_final_test_module`, `paths::tests::a_folder_of_another_account_or_a_link_in_its_place_is_named`, `control::told_tests::a_drafts_test_environment_gives_no_test_records`, and `drafts_are_linted_a_person_publishes_them_and_the_tools_are_found`. Not built: the `Sources` field stays public while the product crate assigns it.
21. Done ([decision 0260](../../decisions/0260-the-machines-network-facts-are-served-as-reads-of-the-local-user-one-listed.md)), `src/handlers/machine.rs`, `src/tests/net.rs`: `net.interfaces`, `net.routes`, `net.resolvers`, `net.neighbours`, `net.listeners`, `net.proxy`, and `net.standing`, each a read for the local user with the default health need, its reader run off the async threads, its output schema held to every word `a2m-netstate` writes; `net.standing` is listed in the `use` toolset and names `net.interfaces` through `action.run` as what to call next. Not built: an input that narrows a reading, and any reading of a network the machine is not on.
22. Done ([decision 0263](../../decisions/0263-a-person-hands-the-scout-files-of-the-projects-sources-folder-by-names-the-discoverers-declare.md), "The registry's half, as built"): `files` on `scout.run` and `discover.run`, checked when asked (`discovery_files`, `source_file`, `files_by_surface` in `handlers/tools.rs`); `input`, `discovery_file_not_taken`; the files in each discovery run's stored call (`runner::DISCOVERY_FILES`) and in the scout's stage 0; `InProcessControl::discovery_files`; `spec.import`, served, as a fresh `scout.run` handed its one file under `description`. Tests: the product's `scout_files.rs`. Not built: an action that fills the discovery focus.
23. Done ([decision 0283](../../decisions/0283-watches-reach-an-agent-through-the-registry-and-their-person-confirms-each-on-the-page.md)): `watch.create`, `watch.confirm`, `watch.get`, `watch.list`, `watch.changes`, `watch.pause`, `watch.resume`, `watch.end`; the watches' tending in `Kernel::tend`; the five protected `watch.*` settings read into `SchedSettings::watch`; `Kernel::watches_hold_the_engine`. Tests: `src/tests/watches.rs`, `tests/settings.rs`'s `a_settings_file_of_the_person_cannot_lower_the_watch_floor`, and the product's `tests/watch.rs`. Not built: what decision 0283 lists, among them the MCP resource, the REST stream, records compared one by one, and removal; the condition came with step 26.
24. Done ([decision 0264](../../decisions/0264-the-publish-page-renders-the-typed-review-sign-ins-first-from-one-keeper-call.md)), `src/acts.rs`: `PersonActAsk::review`, the compiler's `ReviewSummary` (re-exported at the root of `a2m-actions`) with each sign-in's addresses in each environment stated by `Keeper::sign_in_addresses` for the published entry (`ReviewSummary::state_sent_to`; a provider not held is stated as not held, any other refusal of the keeper's refuses the publish), and every text passed through the redactor by `ReviewSummary::map_text`. The text lines built from the review items and the sign-in lines (`sign_ins_shown`) are gone; `details` keeps the kernel's own lines: first to act, the implied `production`, the gates. Test: `the_publish_act_carries_the_typed_review_with_sign_in_addresses_from_the_keeper` in `tests/build.rs`.
25. Done ([decision 0285](../../decisions/0285-two-watches-of-the-same-read-for-one-person-share-a-run-and-ending-one-never-cancels-a-run-another-rests-on.md), [decision 0287](../../decisions/0287-the-last-check-under-way-on-a-run-a-check-opened-cancels-it.md)): a check's run is cancelled only by `run_left_to_cancel`, in `watch.end`, the tending's two cancels, and `watches_after_publish`; `Kernel::check_submission`, the submission of a check, which a test also submits as the person's own call. Tests in `src/tests/watches.rs`: `ending_one_watch_leaves_the_run_another_rests_on_and_it_ends_with_its_result`, `ending_the_last_watch_on_a_run_its_check_opened_cancels_the_run`, `a_shared_run_that_waits_for_a_sign_in_pauses_every_watch_resting_on_it`, `a_check_that_joined_a_run_it_did_not_open_never_cancels_it`. Not built: point 5 of decision 0285, a person's own call that joined a run a check opened.
26. Done ([decision 0291](../../decisions/0291-a-watch-ends-when-its-condition-holds-on-a-checks-own-result.md)): `watch.create`'s `condition`, compiled by `checked_condition` in `handlers/watches.rs`; `condition` in every watch result and on the confirmation page; `condition_met` among the kinds of `watch.changes`. The rule of decision 0287 moved to `Watches::run_left_to_cancel`; the tending no longer cancels the runs a watch's end closed, which `Scheduler::due_watches` cancels in its own transaction, so a failed read can no longer lose them. Test: `a_condition_is_checked_against_the_reads_outputs_and_ends_the_watch_when_it_holds` in `src/tests/watches.rs`, and the product's `a_watch_ends_at_the_first_check_on_which_its_condition_holds_of_what_the_stand_changed`. Not built: package W2d, what decision 0283 left but the condition.
8. Done (the second pass): a source through `tool.call` and the runner; `tool.search` in full; `Kernel::events` and `next_events`; values shown as themselves. Not built: inline rows for a source that is not read as JSON; a call by a published tool's own name in one dispatch, which the MCP endpoint can have by dispatching `tool.call` and reading `input`, `not_found` with no result as an unknown tool.

## Open points

- Where large results go when the caller is a remote program: a blob reference and a fetch route, most likely.
- Whether `engine.*` actions keep an MCP tool and a REST route at all, since they act on a process a remote caller cannot see. Today the rule is four faces for every action.
- How a shortcut threads one action's output into the next without the client holding logic. Today a shortcut's step names the field that takes the shortcut's own argument, and nothing more.
- An action a provider adds has no summary, role, or listing of its own: the provider contract gives a name, two schemas, and an effect. Such an action is given the role `Builder` for a read and `Admin` otherwise, and is not listed.
- `browser.install` is a row of the catalog and so an action of the kernel, which names a kind of program. It belongs to the pack that drives one, as an action that pack adds.
- A word may be both a noun and a shortcut or process command (`run`, `test`, `docs`, `doctor`, `publish`, `runner`). The command face must take the bare word as the shortcut or process command and the word with a verb as the action.
- The run's caller at guard is the caller who started it, kept by this crate when the run was made and never rewritten. A reconcile job the scheduler queues by itself is decided as that caller.
- That a run was approved and not yet resumed is known to a reader only while the engine that recorded the approval runs. After a restart `run.get` shows the run as waiting, and a resume completes it all the same.
- `Kernel::events`, `run.get`, `run.resume`, and `run.cancel` serve a run only to its person. A console that follows other people's runs needs a decision of its own; `trace.show` already serves an auditor.
- The installation's settings folder is one the user can write when `A2M_HOME` is set, which puts everything under one directory, and on Windows when `PROGRAMDATA` is not set. The installation file may loosen and lock, so on those two layouts whoever can write the user's files sets the bounds the other layers are held to. A real installation sets neither.
- `policy.explain` explains a decision by its identifier and does not check its workspace: `a2m-policy` gives the explanation no workspace to compare. One installation is one workspace today.
- A presence proof can be made by any crate, because the crate that raises the prompt is above this one. The desk and the dispatcher check whose it is and when it was made; what it is worth rests on the person surface the product crate mounts.
- A caller's `is_agent` is what the front door states. Every caller of the local channel, and every MCP and SDK caller, is an agent (`caller_is_agent` true): the product no longer takes a header's word that a person is at a terminal ([decision 0158](../../decisions/0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md)). In one process the flag is the process's own view of its terminal: a claim, not a proof. A person is established only by the presence prompt. So a person's act (publishing a version, changing a protected setting) asked for through a served engine goes to the person surface's page, where a press raises the presence prompt and carries the act out once ([decision 0163](../../decisions/0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md)). In this crate the flag changes four things: a caller marked as an agent is never asked for presence, and its person's act is shown to its person on a page instead; it is written into the caller's facts of a run and from there into every policy request about the run (`caller_is_agent`), where the policy engine's guardrail on approving and a pre-approval rule's condition read it; and it is part of the decision recorded for each dispatch. `is_agent: false` is never taken as proof of a person: every act that needs one asks the person surface for a proof and checks it.
- A response to a call refused by a kill switch carries the decision about the action, an allow, and not the policy engine's recorded refusal of the work.
- The mode each intent was stored for is read from the record's row in the ledger, so a record in doubt is read through the binding that wrote it after a restart too (decision 0145). What was recorded before a commit point is read from the ledger and survives one.
- A record that reconcile verified has no outputs and no assigned key in the ledger; the engine reads it once more on resume ([decision 0132](../../decisions/0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md), point 18).
