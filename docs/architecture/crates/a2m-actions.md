# `a2m-actions`

Every way into the kernel, defined once. An action is a name, an input schema, an output schema, the lowest role that may use it, whether it is listed to agents, and a handler. This crate holds the registry of actions, the dispatcher that every front door calls, and the `Kernel` handle that assembles the services below it. In one process it is also the runner: a loop that claims jobs from the scheduler and hands them to the engine.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 7, Actions |
| May depend on | Layers 1 to 6 |
| Depends on | Every kernel crate below it. Outside: `serde`, `serde_json`, Tokio, and a JSON Schema validator, an outside crate to be verified |
| Built in | Roadmap packages B5 (the first definitions), 1.4 (`Kernel::open`, the runner loop), 1.5 (generated faces), 3.2 and 3.6 (the policy gate and approvals), E5 (the scout), 5.1 and 5.2 (every authoring action, shortcuts), 6.3 (toolsets and roles) |
| Carries out | Spec sections [40](../../spec/40-authoring-interface.md), [52](../../spec/52-when-the-engine-itself-fails.md), [64](../../spec/64-rules-for-tools-and-capability-discovery.md), [67](../../spec/67-the-scout-and-who-does-what.md); the in-process half of [43](../../spec/43-runner-protocol.md) and [44](../../spec/44-session-keeper.md); the [action catalog](../../reference/action-catalog.md) |

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

```rust
/// One action. Every face is generated from this and nothing else.
pub struct ActionDef {
    pub name: ActionName,              // noun.verb, checked; the noun is a word of the command line
    pub summary: &'static str,         // one line; the first 240 characters choose the tool (spec section 64)
    pub input: Schema,                 // strict: no unknown fields; flat; at most about ten properties
    pub output: Schema,                // published; not sent in a tool list unless asked
    pub examples: &'static [Example],  // one to three, from recorded tests
    pub shape: Shape,                  // Inspect, List, Trace, Compare, Observe, Change: one per action
    pub effect: Effect,                // Fixed(EffectClass), or OfTool for tool.call
    pub lowest_role: Role,             // the role column of the catalog; Role is defined in a2m-policy
    pub listed: Listed,                // No, Use, Build, UseAndBuild
    pub long_running: bool,            // returns a run_id at once
    pub needs: HealthNeed,             // Nothing, Reads, Writes: which health state it can run in
    pub person: PersonNeed,            // None, Presence: the act is a person's (version.publish, protected config.set)
    pub runs_in: RunsIn,               // Engine; or Client for the engine.* actions that act on the process
    pub rest_alias: Option<RestAlias>, // a resource-shaped route generated beside /actions/{name}
    pub handler: Handler,
}

pub enum Handler {
    Kernel(fn(&Kernel, Call) -> BoxFuture<'_, Result<Outcome, Failure>>),
    Provider(ProviderActionRef),       // the handler interface is defined in a2m-provider, implemented by
                                       // packs and plug-ins; the dispatcher calls it like any other
    Client,                            // carried out by the client end; see a2m-local
}

pub struct Registry { /* ActionDef by name, in catalog order; frozen after build */ }
impl Registry {
    /// The kernel's own actions plus every provider contribution in the provider registry.
    /// Refuses a duplicate name, a provider action outside its name space, and a built-in
    /// toolset whose listed actions exceed its cap.
    pub fn build(providers: &ProviderRegistry) -> Result<Registry, Failure>;
    pub fn get(&self, name: &ActionName) -> Option<&ActionDef>;
    pub fn catalog(&self) -> Catalog;  // serializable list plus a digest; the input to every generator
}

/// What a front door hands the dispatcher after it has authenticated the caller.
pub struct Request {
    pub action: ActionName,
    pub input: serde_json::Value,
    pub caller: Caller,                // principal, client label, front door, toolset, is_agent
    pub idempotency_key: Option<String>,
    pub project: Option<ProjectRef>,   // the project folder a local command was run in; turned into
                                       // a2m-provider's checked-path type before any use
}

pub struct Kernel { /* Arc of every service below; cheap to clone */ }
impl Kernel {
    /// Opens the store and runs migrations, opens the vault, loads policy, starts the keeper,
    /// the scheduler, and the engine. Never fails for an impaired part: it opens in a lower
    /// health state and says why. Fails only for settings that do not load.
    pub async fn open(settings: KernelSettings, providers: ProviderRegistry,
                      actions: Registry, person: Arc<dyn PersonSurface>) -> Result<Kernel, Failure>;
    pub async fn dispatch(&self, request: Request) -> Response;
    /// The one place a network credential is checked: a bearer token against the configured
    /// issuer and audience, or an API key against its stored hash. Front doors extract the
    /// credential from their transport and call this; they never check one themselves.
    pub async fn verify_caller(&self, credential: CallerCredential) -> Result<Principal, Failure>;
    pub fn health(&self) -> Health;                       // Ok, Degraded(reason), Stopped(reason)
    pub fn events(&self, run: RunId) -> EventStream;      // live, from the ledger's sequence number
    pub fn approvals(&self) -> &ApprovalDesk;             // for approval surfaces; reached by no action
    pub fn start_runner_loop(&self, slots: Slots) -> RunnerLoop;
    pub async fn close(self, why: CloseReason);           // saves sessions to the vault first
}

/// What a role supplies so that a person can be reached. a2m-local implements it with a
/// notification, the loopback page, and the operating system's prompt; a2m-api with the
/// console pages behind the identity provider's sign-in.
pub trait PersonSurface: Send + Sync {
    async fn ask_approval(&self, ask: ApprovalAsk) -> Result<SurfaceLink, Failure>;
    async fn ask_sign_in(&self, ask: SignInAsk) -> Result<SurfaceLink, Failure>;
    async fn confirm_presence(&self, ask: PresenceAsk) -> Result<PresenceProof, Failure>;
    /// A plain link for the person to open in their own browser, such as an authorization
    /// page a sign-in provider needs consent on. Nothing drives that browser; the surface only
    /// notifies and shows the link, and the provider learns the outcome by its own protocol.
    async fn give_link(&self, ask: LinkAsk) -> Result<SurfaceLink, Failure>;
}

pub struct ApprovalDesk { /* records an answer through a2m-policy, given a PresenceProof */ }
pub struct Paths { /* config, data, store, blobs, profiles, journal, logs, runtime, cache */ }
impl Paths { pub fn resolve(env: &Env) -> Result<Paths, Failure>; }  // A2M_HOME wins
pub struct Shortcut { pub word: &'static str, pub steps: &'static [ShortcutStep] }
```

### The dispatcher

A front door authenticates and builds a `Request`; it decides nothing. `dispatch` then does, in this order, and every refusal is an ordinary `Failure`:

1. **Find.** An unknown name is `input` / `schema_invalid`, with the nearest names as the fix.
2. **Validate.** The input against the action's full schema, never the compact one a tool list showed. Malformed input and a valid input that names nothing are different failures (spec section 64).
3. **Health.** The action's `needs` against the kernel's state. A write in `degraded`, or anything but a `Nothing` action in `stopped`, returns `busy` / `engine_degraded` with the reason.
4. **Decide.** One `decide` call to `a2m-policy`, with the action, its effect (for `tool.call`, the published tool's effect class), the caller, and the context the engine supplies. Policy records the decision, allowed or refused, and returns its identifier. The dispatcher refuses to go on without one.
5. **Person.** For an action with `PersonNeed::Presence`, `confirm_presence` on the role's surface. A caller marked as an agent never satisfies it, and on one machine the proof comes only from the operating system's prompt.
6. **Run.** The handler. A long-running action submits to `a2m-sched` and returns the `run_id` at once.
7. **Shape.** Pass every outgoing string through `Redact`; mark text that came from a target as untrusted; cut to the result size cap and say which limit cut it; keep a large result as a blob and return its reference with a size and a summary; add `next`.

The `Response` is the same structure for every face: `ok`, `action`, `result` or `error`, `warnings`, `next`, and an exit class (succeeded, failed, malformed, person needed). Each face renders it without branching on its content: the command face as the [output contract](../../reference/technical-reference.md#command-output-contract), the MCP face as structured content, REST as JSON with a status code.

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

The kernel is always returned, so that the front door can answer `engine.status` and `doctor.run` with the reason instead of failing to start. The state is re-evaluated whenever a part reports a change, and a move to a lower state is an event in the audit log.

Before anything else on open, the store's runs left `running` under a lease nobody holds are set to `stopped` with `possible_commit` / `runner_lost` ([spec section 39](../../spec/39-local-engine-and-clients.md)). Their reconcile is a job like any other.

### The in-process runner loop

```rust
loop {
    let lease = sched.claim(offer, wait).await?;     // long wait on a named condition, never a sleep
    let control = InProcessControl::new(&kernel, &lease);
    let outcome = match lease.job.kind {
        JobKind::Run       => engine.run(&lease.job, &control).await,
        JobKind::Reconcile => engine.reconcile(&lease.job, &control).await,
        JobKind::Keeper(w) => keeper.perform(w, &control).await,     // the keeper's work half
        JobKind::Discover  => engine.discover(&lease.job, &control).await,  // the sandbox
    };
    sched.complete(lease, outcome).await?;
}
```

Slots come from settings per mode, as a runner on another machine states them in `hello`. Keeper jobs have a reserved slot, so a run waiting for a renewal can never hold the only slot the renewal needs. The lease rules of spec section 43 apply unchanged: the lease is counted on a steady clock, heartbeats are sent by a task per held job, and the engine stops acting before the lease runs out. Keeping them on one machine is what lets hypothesis H-21 be measured.

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
| Sending events and evidence, including the finished trace and a binding's health | `a2m-ledger`. Each event is also published to `events()` subscribers |

`KeeperWork` is implemented by submitting a `Keeper` job to `a2m-sched` addressed to the session's home runner, and waiting for its outcome. In one process the home runner is this loop. When the keeper needs a person, to sign in or to give consent on a link, it asks through the keeper's own interface in `a2m-auth`, and this crate answers by calling the role's `PersonSurface`: `ask_sign_in` for a sign-in window a pack's provider opens, `give_link` for a provider outside the web pack, such as authorization-code sign-in or an upstream MCP server's, that needs the person's consent in their own browser.

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
| `input` | `path_not_allowed` | A project or file path that fails the check behind `a2m-provider`'s checked-path type |
| `policy` | Any code `decide` returns | Refused by policy, passed through unchanged |
| `policy` | `person_required` (new) | A person's act was asked for by a caller that cannot give one |
| `busy` | `engine_degraded` | The health state forbids the action |
| `busy` | `queue_full` | Passed through from `a2m-sched` |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Every dispatch records exactly one decision before the handler runs | A test that dispatches every action with an allowing and a refusing policy and counts decisions |
| No action, route, tool, or command reaches `ApprovalDesk` | A test that walks every generated face; a review rule on the desk's callers |
| The four faces of an action give the same result for the same input | The parity test of spec section 64 |
| Nothing leaves `dispatch` without passing `Redact` | The planted-secret fault test, through every face |
| A write never runs in `degraded` or `stopped` | Fault tests that remove the store, the vault, and the policy set |
| The registry equals the action catalog, in names, roles, and listing | A test that reads the catalog document |
| The scout never obtains a write token | A test that plants a discoverer and a drafted operation that would write |
| A keeper job always has a slot | A test that fills every run slot with runs waiting for a renewal |

## Build order

1. `ActionDef`, `Registry::build`, and the catalog test, with `provider.list` and `provider.describe` defined and not served. Package B5.
2. `Kernel::open` with the store and the engine, the runner loop, `InProcessControl`, health with two parts, `Paths`. Package 1.4.
3. The dispatcher with validation and shaping; the command and MCP faces; the parity test. Package 1.5.
4. `decide` in the dispatcher, approval checks in `Control`, `PersonSurface`, `ApprovalDesk`, the vault and policy rows of health. Packages 3.2, 3.6, 3.10.
5. `KeeperWork` and the reserved slot, with the keeper. Phase 2.
6. The scout, stage by stage, each resumable. Package E5.
7. Every authoring action, shortcuts, the recorded catalog and its generators. Packages 5.1, 5.2.

## Open points

- Where large results go when the caller is a remote program: a blob reference and a fetch route, most likely.
- Whether `engine.*` actions keep an MCP tool and a REST route at all, since they act on a process a remote caller cannot see. Today the rule is four faces for every action.
- How a shortcut threads one action's output into the next without the client holding logic: declared field mappings in the `Shortcut` data is the proposal.
- The JSON Schema validator crate, and whether schemas are written by hand or derived from Rust types.
