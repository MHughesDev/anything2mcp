# `a2m-actions`

Every way into the kernel, defined once. An action is a name, an input schema, an output schema, the lowest role that may use it, whether it is listed to agents, and a handler. This crate holds the registry of actions, the dispatcher that every front door calls, and the `Kernel` handle that assembles the services below it. In one process it is also the runner: a loop that claims jobs from the scheduler and hands them to the engine.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 7, Actions |
| May depend on | Layers 1 to 6 |
| Depends on | Every kernel crate below it except `a2m-expr`, `a2m-sources`, which it reaches through others. Outside: `serde`, `serde_json`, Tokio, and `toml` for settings files. Schemas are checked by `a2m-files`; no outside validator |
| Built in | Roadmap packages B5 (the first definitions), 1.4 (`Kernel::open`, the runner loop), 1.5 (generated faces), 3.2 and 3.6 (the policy gate and approvals), E5 (the scout), 5.1 and 5.2 (every authoring action, shortcuts), 6.3 (toolsets and roles). A second pass, after the crates above it were built, is [decision 0132](../../decisions/0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md); the safety audit is [decision 0152](../../decisions/0152-the-action-registry-and-telemetry-after-their-safety-audit.md) |
| Carries out | Spec sections [220](../../spec/2-kernel/220-actions-and-the-authoring-interface.md), [224](../../spec/2-kernel/224-when-the-engine-itself-fails.md), [220](../../spec/2-kernel/220-actions-and-the-authoring-interface.md), [402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md); the in-process half of [223](../../spec/2-kernel/223-the-runner-protocol.md) and [215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md); the [action catalog](../../reference/action-catalog.md) |

## Owns

- The action registry: one `ActionDef` per row of the action catalog, plus the actions a provider contributes under its own name space.
- The dispatcher: validate, check health, ask policy, run the handler, shape the result.
- The `Kernel` handle, `Kernel::open`, and the kernel's health state.
- The settings types the kernel needs and the loader that layers them, so that `config.show` and `config.set` speak the same rules as start-up.
- The on-disk layout of [the technical reference](../../reference/technical-reference.md#on-disk-layout), as one pure function, so that the local channel, the runner journal, and the store agree on paths.
- The in-process runner loop, and the in-process implementations of `Control` (from `a2m-engine`) and `KeeperWork` (from `a2m-auth`).
- The only caller of the compiler in `a2m-files`: lint, compile, and draft. A plan, a type in `a2m-core`, is produced there and stored by `version.publish`; the engine receives it through `Control` and never sees the compiler.
- The meaning of three tables: toolsets, the scout's stage results, and the outcomes of webhook deliveries; and of two more that say what a run was called with, its input and caller, and its source. Their migrations, like every migration, live in `a2m-ledger`.
- What a caller is shown of a value of a system: an exact number as its exact decimal text, a calendar day as text, a moment and a length of time as whole milliseconds. Inside the engine and in the store a value keeps its kind.
- The recorded catalog: the list of actions with a digest, from which every face is generated and checked.
- The scout pipeline, as the handler of `scout.run`.
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
    pub const fn check / principal / at / prompt(&self);
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
}
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
}
pub struct Sources { /* installation, presets, user, project, environment, flags, serve, packs */ }
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
5. **Person.** For an action with `PersonNeed::Presence`, and for one with `PresenceWhenLoosening` when the call loosens, `confirm_presence` on the role's surface. Releasing a kill switch loosens; an action that has not said how its loosening is told (`config.set`, `provider.enable`, neither built) is asked for on every call. A caller marked as an agent never satisfies it, and on one machine the proof comes only from the operating system's prompt. The proof that comes back is checked: it must be this caller's, and at most two minutes old by the kernel's clock. A handler is given the proof in `Call::presence` and never asks itself.
6. **Run.** The handler. It is given the effect class and hazard the decision was made for (`Call::decided`); `tool.call` refuses, with `busy`, `tool_changed`, a tool that has another by the time it looks. A long-running action submits to `a2m-sched` and returns the `run_id` at once.
7. **Shape.** Pass every outgoing string through `Redact`, the echoed action name included; cut to the result size cap and say which limit cut it; add `next`. Every strict output schema admits `next`, and the MCP rendering of a success puts it into the structured content as `action`, `tool`, `args`, and `when`. Not built: marking text that came from a target as untrusted, and keeping a large result as a blob with its reference.

### Runs that wait for a person

- **Whose a run is.** `run.get`, `run.resume`, and `run.cancel` answer for a run only to the person who started it, or to a caller proved to act for that person (`Principal::person`). Anyone else is told `input`, `not_found`, whatever role they hold: a run acts under its person's session. `trace.show` is bound to the workspace alone, because reading a run's evidence is what the auditor's role is for and the dispatcher's decision was that the caller holds it. A resume never changes what was kept of the run's caller: the guard asks about the caller that started the run, and `run.resume` first asks the policy engine, and records the answer, whether whoever resumes may use the run's tool.
- **A run whose call cannot be read.** A run whose row in `actions_run_calls` is missing, or whose caller does not parse, is never run with an empty input and a caller made up for it. It ends as `failed` with `busy`, `run_call_unreadable`, or as `cancelled` where the ledger allows no other end; with a record in doubt it stops, and no reconcile is queued, which would end the same way.
- **Waiting for a run.** `tool.call` and `run.resume` wait until the run has ended, or rests or has stopped with no job of the scheduler waiting or leased for it. The run's row and its unfinished job are read in one transaction: a job is unfinished from the transaction that queues it to the one that records how it ended, so nothing a runner does between its claim and the end of its work is taken for rest. After an approval, one `run.resume` completes the write.
- **Telling the person.** Only `tool.call` and `run.resume` tell a run's person that it waits, and once for one diff of one run: the link is kept in memory and given again. `run.get` asks nobody anything; it shows the kept link. A run that a person approved at the desk and nobody has resumed is reported with `approved: true`, no link, and `run.resume` as the next step, and is not marked as needing a person.
- **A decline.** `ApprovalDesk::decline` records the decline through the policy engine and ends the run as `expired` in the same transaction, with the failure `policy`, `approval_refused`. `run.get` reports it; nobody is asked again; the same call again is a new run.
- **The refusal to write without a person** is a recorded decision. A `need` event of the run carries its identifier, the word `approval`, and the diff's hash.
- **A kill switch** met when work is submitted or resumed is answered `policy`, `kill_switch_on`, by the policy engine, which records the refusal. The scheduler's `busy`, `on_hold` is not passed on.

### The project folder

A local command works in a project folder. The mount of a front door puts a `ProjectFolder`, a type of this crate, on each request, as an extension beside the caller, from what the client end stated; the door hands it to `Request::in_project_folder`. Nothing a caller writes in a path, a query, a body, or a tool's arguments sets it, so a caller of an MCP tool cannot state one. The client end of the local channel can, and it runs as the person: whoever can run it can state any folder. So before the handler runs the dispatcher takes the folder only when all of this holds, with every link followed:

- this engine serves one machine, the path is a full one, and it names a folder that exists;
- the folder neither holds nor lies inside the engine's own settings, data, runtime, or cache folders;
- it is not the home folder or a folder above it, does not lie inside a hidden folder of the home folder, and neither holds nor lies inside a folder the operating system gives programs for their data (the `XDG_*` folders, `Library` on macOS, `APPDATA`, `LOCALAPPDATA`, and `PROGRAMDATA` on Windows). The folder for temporary files is excepted from the last two, because on one operating system it lies inside such a folder and is nobody's own. This is what keeps a browser's profile and a key folder from being a project (rule 5);
- it holds a project file, `anything2mcp.toml`, that is a file and not a link, is at most 1 MiB, and loads as a settings file by itself;
- it is inside what the setting `files.allowed_roots` allows. The word `<project>`, the default, allows any folder that passed the checks above: the local engine serves commands run in many projects, so the folder it was started in would be the wrong bound. A full path allows the folders inside it; a list without the word allows only those; an entry that is neither allows nothing.

A folder that fails is not handed on: `Call::project_folder` then answers `input`, `path_not_allowed` with the reason, and an action that needs no project is served all the same. What is handed on is the folder as it was resolved, never the path as it was stated. A handler reaches every file through a checked path under the folder, as before. What remains: a program that runs as the person and writes a project file into a folder of theirs makes that folder a project; such a program can read the folder itself.

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

In one process `KeeperWork` is `a2m-auth`'s `InProcessWork` over the keeper's own worker; this crate supplies its two halves, the way to a person and the probe. Submitting a keeper job to `a2m-sched` addressed to a session's home runner is for a runner on another machine and is not built. When the keeper needs a person, to sign in or to give consent on a link, it asks through the keeper's own interface in `a2m-auth`, and this crate answers by calling the role's `PersonSurface::ask_session` with the keeper's own request: a link to open in the person's own browser, a code to enter, or a notice. `ask_sign_in` gives the link that goes into the result of a run that rests at `needs_login`.

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

`scout.run` is long-running. It is a run of its own kind, with a `run_id`, and each stage stores its result before the next begins, so a stopped scout continues from the last finished stage.

| # | Stage | Done by | Touches the target |
| --- | --- | --- | --- |
| 0 | Consent and budget | `decide` with the target's consent record; `authoring.scout.budget` split across stages | No |
| 1 | Identify | One `Discover` job per declared surface, running the probe the registry holds for it and `auth.detect` | Yes, on a runner, read only |
| 2 | Discover | One `Discover` job per surface and suitable discoverer, within its bounds | Yes, on a runner, in the sandbox |
| 3 | Compare | The comparison in `a2m-provider`; each difference is a finding or an open question in `a2m-ledger` | No |
| 4 | Draft | The drafter in `a2m-files`, with the drafting rules the registry gathered | No |
| 5 | Check | `a2m-files` lints and compiles each draft; a failure is kept and marked | No |
| 6 | Try | For each drafted read that is safe by enforcement or by the system's own contract, five `draft.try` runs in the sandbox; the trace signatures and results compared | Yes, on a runner, read only |
| 7 | Report | Found, drafted, proven, open, and what to look at first, in a fixed order, hashed | No |

A surface that needs a sign-in is reported as `needs_login`; the others carry on, and the next `scout.run` on the target resumes that surface from where it stopped. A stage that hits a limit records which. No stage has a branch that a setting can turn into a write: a `Discover` job runs inside the engine's sandbox, where a discoverer is given only what `SandboxPort` in `a2m-provider` offers and no intent token exists.

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

## State

| State | Where |
| --- | --- |
| The registry and the recorded catalog | Memory, frozen at build; the digest in the store |
| Health and its cause | Memory; each change an audit event |
| Toolsets | A table in the store, owned by this crate |
| Scout stage results | A table in the store, owned by this crate: run, stage, input hash, result reference |
| Webhook delivery outcomes | A table in the store, owned by this crate, beside the outbox the events are written to |
| What a run was called with | Two tables in the store, owned by this crate: the stored input and the caller's verified facts (`actions_run_calls`), and the stored source (`actions_run_sources`) |
| Which runs a person approved and nobody resumed yet; the link a run's person was last given | Memory, the newest 1024 runs of each; gone with the process. They shape what a reader is told and decide nothing |
| Large results | Blobs in the store, by content hash; on one machine also written under the project's `.anything2mcp/out/` by the engine, with a name the engine chose |

## Failures

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
| `policy` | `person_required` | A person's act was asked for by a caller that cannot give one |
| `policy` | `approval_refused` | What `run.get` reports for a run a person declined, or whose approval window ended; and what the desk answers to a proof of presence that is not the approver's or not of now |
| `policy` | `kill_switch_on` | A kill switch covers the work a call would start or resume |
| `policy` | `approval_stale` | The desk was asked to show a stored diff that is not the one the run waits on |
| `input` | `file_unavailable` | A source file that is missing, unreadable, or larger than a source may be |
| `policy` | `caller_not_verified` | A credential this engine does not accept |
| `policy` | `setting_protected` | A layer loosened a protected setting, or set a key it may not |
| `policy` | `setting_locked` | A higher layer changed a key the installation file locks |
| `policy` | `setting_not_in_force` | A protected setting is set to a value the policy engine does not work by; the kernel is stopped |
| `policy` | `presence_required` | A proof of presence that is another person's, or not of now, handed to the dispatcher |
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
| The scout never obtains a write token | Not built |
| A keeper job always has a slot | Holds by construction in one process: the keeper's work takes no slot. Not tested |
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
| A `Control` call dropped at a wait leaves nothing half-done | By construction: one transaction a call, and a guard for the notice. Not tested with a dropped call |

## Build order

1. Done: `ActionDef`, `ActionRegistry::build`, and the catalog test. Every action of the catalog is defined as data; one that is not built answers `input` / `action_not_built`.
2. Done: `Kernel::prepare` and `Kernel::open` with the store, the vault, policy, the keeper, the scheduler, and the engine; the runner loop; `InProcessControl`; the three health states; `Paths`; the settings loader (package 1.4a). Not built: the evidence-space check; a session's probe; the keeper's work for a runner on another machine.
3. Done: the dispatcher with validation, the health check, the decision, the person need, and redaction; every face as data (`command_faces`, `mcp_tools`, `rest_routes`, `sdk_methods`, `McpResultFace`). Not built: the live parity test, which needs the role crates; the untrusted mark; large results as blobs.
4. Done: `decide` in the dispatcher and at guard, the approval check and `consume` in the transaction of `begin_writes`, `PersonSurface`, `ApprovalDesk` with its preview and a decline that ends the run, the vault and policy rows of health.
5. Not built: the keeper's work as a scheduled job, and bearer tokens and API keys in `verify_caller`.
6. Not built: the scout, `discover.run`, `test.run`.
7. Built, each with a test through the dispatcher: `action.search`, `action.describe`, `action.run`, `docs.get`, `engine.status`, `config.show`, `doctor.run`, `provider.list`, `provider.describe`, `target.add`, `target.list`, `draft.lint`, `version.publish`, `plan.show`, `tool.search`, `tool.describe`, `tool.call`, `run.get`, `run.resume`, `run.cancel`, `trace.show`, `session.status`, `secret.set`, `killswitch.set`, `policy.explain`, `audit.query`. The four `engine.*` process actions and `self.update` are the client's. Every other action of the catalog is defined and not built. Shortcuts are data, and `Shortcut::on` says where one stops on this engine. The recorded catalog has a digest, kept in the store at each start; the generators that read it are not built.
9. Done (the safety audit, [decision 0152](../../decisions/0152-the-action-registry-and-telemetry-after-their-safety-audit.md)): whose a run is; the protected settings and what is in force; the bounds of a project folder and of a source file; the checked proof of presence; a run whose call cannot be read; presence when an act loosens; what a job holds alone; the in-process lease; `next_events` at rest; `tool.search` by kind; `McpResultFace::of_in`; `PersonSurface::presence`. Done since (decision 0159): `next_events_from` and `Followed`, so that a run that came to rest between a follower's two calls is told at once; the guides served by `docs.get`. Done since (decision 0145): every policy setting is carried into the policy engine; `policy.explain` answers only in the caller's workspace; a record's mode is read from the ledger's row.
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
- A caller's `is_agent` is what the front door states. In this crate it changes four things: a caller marked as an agent is never asked for presence and is refused a person's act; it is written into the caller's facts of a run and from there into every policy request about the run (`caller_is_agent`), where the policy engine's guardrail on approving and a pre-approval rule's condition read it; and it is part of the decision recorded for each dispatch. `is_agent: false` is never taken as proof of a person: every act that needs one asks the person surface for a proof and checks it.
- A response to a call refused by a kill switch carries the decision about the action, an allow, and not the policy engine's recorded refusal of the work.
- The mode each intent was stored for is read from the record's row in the ledger, so a record in doubt is read through the binding that wrote it after a restart too (decision 0145). What was recorded before a commit point is read from the ledger and survives one.
- A record that reconcile verified has no outputs and no assigned key in the ledger; the engine reads it once more on resume ([decision 0132](../../decisions/0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md), point 18).
