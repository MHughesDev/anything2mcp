# `a2m-actions`

Every way into the kernel, defined once. An action is a name, an input schema, an output schema, the lowest role that may use it, whether it is listed to agents, and a handler. This crate holds the registry of actions, the dispatcher that every front door calls, and the `Kernel` handle that assembles the services below it. In one process it is also the runner: a loop that claims jobs from the scheduler and hands them to the engine.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 7, Actions |
| May depend on | Layers 1 to 6 |
| Depends on | Every kernel crate below it except `a2m-expr`, `a2m-sources`, which it reaches through others. Outside: `serde`, `serde_json`, Tokio, and `toml` for settings files. Schemas are checked by `a2m-files`; no outside validator |
| Built in | Roadmap packages B5 (the first definitions), 1.4 (`Kernel::open`, the runner loop), 1.5 (generated faces), 3.2 and 3.6 (the policy gate and approvals), E5 (the scout), 5.1 and 5.2 (every authoring action, shortcuts), 6.3 (toolsets and roles) |
| Carries out | Spec sections [220](../../spec/2-kernel/220-actions-and-the-authoring-interface.md), [224](../../spec/2-kernel/224-when-the-engine-itself-fails.md), [220](../../spec/2-kernel/220-actions-and-the-authoring-interface.md), [402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md); the in-process half of [223](../../spec/2-kernel/223-the-runner-protocol.md) and [215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md); the [action catalog](../../reference/action-catalog.md) |

## Owns

- The action registry: one `ActionDef` per row of the action catalog, plus the actions a provider contributes under its own name space.
- The dispatcher: validate, check health, ask policy, run the handler, shape the result.
- The `Kernel` handle, `Kernel::open`, and the kernel's health state.
- The settings types the kernel needs and the loader that layers them, so that `config.show` and `config.set` speak the same rules as start-up.
- The on-disk layout of [the technical reference](../../reference/technical-reference.md#on-disk-layout), as one pure function, so that the local channel, the runner journal, and the store agree on paths.
- The in-process runner loop, and the in-process implementations of `Control` (from `a2m-engine`) and `KeeperWork` (from `a2m-auth`).
- The only caller of the compiler in `a2m-files`: lint, compile, and draft. A plan, a type in `a2m-core`, is produced there and stored by `version.publish`; the engine receives it through `Control` and never sees the compiler.
- The meaning of three tables: toolsets, the scout's stage results, and the outcomes of webhook deliveries. Their migrations, like every migration, live in `a2m-ledger`.
- The recorded catalog: the list of actions with a digest, from which every face is generated and checked.
- The scout pipeline, as the handler of `scout.run`.
- Shortcuts, as data: a word and a fixed list of actions.

It must never: decide whether something may happen (that is `a2m-policy`); sign in or renew (that is the keeper in `a2m-auth`); carry out a step (that is `a2m-engine`); name a pack, a mode, or a make of client; open a socket or a port; accept an approval through any action.

## Interface

As built ([decision 0094](../../decisions/0094-the-action-layers-interface-as-building-it-settled-it.md)). `ProviderRegistry` stands for the `Registry` of `a2m-provider`.

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
    pub project: Option<PathBuf>,      // checked against the allowed roots before any use
}
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
    pub fn status_only_caller(&self) -> Principal;       // for a stopped engine's status
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
    pub async fn close(self, why: CloseReason);
}
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
    fn return_address(&self) -> Option<String> { None }
}
pub struct ApprovalDesk { /* not Clone; no public constructor; reached by no action */ }
impl ApprovalDesk {
    pub async fn waiting(&self, run: RunId) -> Result<WaitingApproval, Failure>;
    pub async fn approve(&self, run: RunId, diff_hash: Digest, approver: PrincipalRef,
                         channel: Channel, proof: PresenceProof) -> Result<ApprovalId, Failure>;
    pub async fn decline(&self, run: RunId, by: Option<PrincipalRef>, channel: Channel)
                         -> Result<(), Failure>;
}

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
impl McpResultFace { pub fn of(response: &Response) -> McpResultFace; }
pub const SHORTCUTS: &[Shortcut];
pub const PROCESS_COMMANDS: &[&str];
```

### The dispatcher

A front door authenticates and builds a `Request`; it decides nothing. `dispatch` then does, in this order, and every refusal is an ordinary `Failure`:

1. **Find.** An unknown name is `input` / `schema_invalid`, with the nearest names as the fix.
2. **Validate.** The input against the action's full schema, never the compact one a tool list showed. Malformed input and a valid input that names nothing are different failures (spec section 220): the second is `input` / `not_found`.
3. **Health.** The action's `needs` against the kernel's state. A write in `degraded`, or anything but a `Nothing` action in `stopped`, returns `busy` / `engine_degraded` with the reason.
4. **Decide.** One `decide` call to `a2m-policy`, with the action, its effect (for `tool.call`, the published tool's effect class), the caller, and the context the engine supplies. Policy records the decision, allowed or refused, and returns its identifier. The dispatcher refuses to go on without one. The one exception: a `Nothing` action in a `stopped` kernel, which has no store to record a decision in; it reports on the engine and shows no data of any system.
5. **Person.** For an action with `PersonNeed::Presence`, `confirm_presence` on the role's surface. A caller marked as an agent never satisfies it, and on one machine the proof comes only from the operating system's prompt.
6. **Run.** The handler. A long-running action submits to `a2m-sched` and returns the `run_id` at once.
7. **Shape.** Pass every outgoing string through `Redact`, the echoed action name included; cut to the result size cap and say which limit cut it; add `next`. Not built: marking text that came from a target as untrusted, and keeping a large result as a blob with its reference.

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

Slots are a count given to `start_runner_loop`, offered for every registered mode. The keeper's work does not take a slot in one process: it runs on the keeper's own worker (`InProcessWork` of `a2m-auth`), so a run waiting for a renewal can never hold the slot the renewal needs. In one process the lease is at least 100 seconds, and a job the engine gives back with a full lease left stops with `busy` / `lease_too_short` ([decision 0095](../../decisions/0095-the-settings-loader-and-the-in-process-runner-as-built.md)). The lease rules of spec section 223 apply unchanged: the lease is counted on a steady clock, heartbeats are sent by a task per held job, and the engine stops acting before the lease runs out. Keeping them on one machine is what lets hypothesis H-21 be measured.

The list is the spine's, in its order. The engine opens nothing itself: no store, no policy engine, no compiler.

| `Control` asks for | In process, answered by |
| --- | --- |
| The plan by its hash | `a2m-ledger`; the hash is checked with the check in `a2m-core` before the plan is handed over |
| The policy decision at guard | `a2m-policy` `decide`, recorded like every decision |
| A session lease | The keeper in `a2m-auth` |
| A record lock | `a2m-ledger`, which owns the lock table and its rules, under the job's lease number; when the lock is held, `a2m-sched` queues the wait |
| A rate budget | `a2m-sched` |
| The approval check for a diff hash | `a2m-policy` |
| Recording an intent, and its token | `a2m-ledger`, in one transaction that also checks the approval policy recorded; the reply is the token the driver's handle requires |
| Sending events and evidence, including the finished trace and a binding's health | `a2m-ledger`. Each event is also published to `events()` subscribers. A verified record is marked through `Store::store_verified`, so its event is `verified`. `trace.show` reads the trace signature from the last line of the stored trace |

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
| Reference pages for `docs.get` | The same task | The recorded catalog and the documents |

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
| `a2m-local` | `Kernel::open`, `dispatch`, `health`, `events`, `approvals`, `Paths`, `PersonSurface` to implement |
| `a2m-mcp` | `Registry` for tool lists, `dispatch`, `events` for progress |
| `a2m-api` | `Registry` for routes and OpenAPI, `dispatch`, `events`, `approvals`, `PersonSurface` to implement |
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
| `a2m-sched` | `submit`, `claim`, `complete`, waits for a held lock, budgets |
| `a2m-engine` | `run`, `reconcile`, `discover` |

## State

| State | Where |
| --- | --- |
| The registry and the recorded catalog | Memory, frozen at build; the digest in the store |
| Health and its cause | Memory; each change an audit event |
| Toolsets | A table in the store, owned by this crate |
| Scout stage results | A table in the store, owned by this crate: run, stage, input hash, result reference |
| Webhook delivery outcomes | A table in the store, owned by this crate, beside the outbox the events are written to |
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
| `policy` | `caller_not_verified` | A credential this engine does not accept |
| `policy` | `setting_protected` | A layer loosened a protected setting, or set a key it may not |
| `policy` | `setting_locked` | A higher layer changed a key the installation file locks |
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

## Build order

1. Done: `ActionDef`, `ActionRegistry::build`, and the catalog test. Every action of the catalog is defined as data; one that is not built answers `input` / `action_not_built`.
2. Done: `Kernel::prepare` and `Kernel::open` with the store, the vault, policy, the keeper, the scheduler, and the engine; the runner loop; `InProcessControl`; the three health states; `Paths`; the settings loader (package 1.4a). Not built: the evidence-space check; a session's probe; the keeper's work for a runner on another machine.
3. Done: the dispatcher with validation, the health check, the decision, the person need, and redaction; every face as data (`command_faces`, `mcp_tools`, `rest_routes`, `sdk_methods`, `McpResultFace`). Not built: the live parity test, which needs the role crates; the untrusted mark; large results as blobs.
4. Done: `decide` in the dispatcher and at guard, the approval check and `consume` in the transaction of `begin_writes`, `PersonSurface`, `ApprovalDesk`, the vault and policy rows of health.
5. Not built: the keeper's work as a scheduled job, and bearer tokens and API keys in `verify_caller`.
6. Not built: the scout, `discover.run`, `test.run`.
7. Built, each with a test through the dispatcher: `action.search`, `action.describe`, `action.run`, `docs.get`, `engine.status`, `config.show`, `doctor.run`, `provider.list`, `provider.describe`, `target.add`, `target.list`, `draft.lint`, `version.publish`, `plan.show`, `tool.search`, `tool.describe`, `tool.call`, `run.get`, `run.resume`, `run.cancel`, `trace.show`, `session.status`, `secret.set`, `killswitch.set`, `policy.explain`, `audit.query`. The four `engine.*` process actions and `self.update` are the client's. Every other action of the catalog is defined and not built. Shortcuts are data. The recorded catalog has a digest, kept in the store at each start; the generators that read it are not built.

## Open points

- Where large results go when the caller is a remote program: a blob reference and a fetch route, most likely.
- Whether `engine.*` actions keep an MCP tool and a REST route at all, since they act on a process a remote caller cannot see. Today the rule is four faces for every action.
- How a shortcut threads one action's output into the next without the client holding logic. Today a shortcut's step names the field that takes the shortcut's own argument, and nothing more.
- An action a provider adds has no summary, role, or listing of its own: the provider contract gives a name, two schemas, and an effect. Such an action is given the role `Builder` for a read and `Admin` otherwise, and is not listed.
- `browser.install` is a row of the catalog and so an action of the kernel, which names a kind of program. It belongs to the pack that drives one, as an action that pack adds.
- A word may be both a noun and a shortcut or process command (`run`, `test`, `docs`, `doctor`, `publish`, `runner`). The command face must take the bare word as the shortcut or process command and the word with a verb as the action.
- The run's caller at guard is the caller who started or last resumed it, kept by this crate. A reconcile job the scheduler queues by itself is decided as the person who started the run.
