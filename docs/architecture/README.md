# Architecture

How the code of Anything2MCP is put together: which crate owns what, which interface each crate offers, who calls whom, and where every piece of state lives. The [specification](../spec/README.md) says what the system does and why. The [technical reference](../reference/technical-reference.md) holds interfaces once they are settled. The [plan](../plan/roadmap.md) says in what order things are built. This folder is the bridge between them: read it before writing code in any crate.

Nothing here overrides a rule. Where this folder and a spec section differ on what the system does, the spec wins and this folder is corrected. Where they differ on how the code is arranged, this folder wins.

Most of what is described here is not built yet. The Rust in this folder is a sketch of shape and ownership. The slice that builds an interface settles its final form, and records the choice when it differs from the sketch.

## The system in one picture

```text
  Callers     agents over MCP · programs over REST and the SDKs · people at the command line and the console
                 │
  ┌──────────────▼──────────────────────── one engine process ───────────────────────────────┐
  │ 8 Roles        a2m-local      a2m-mcp      a2m-api      a2m-runner                         │
  │ 7 Actions      a2m-actions    every entry point is an action; holds the kernel handle     │
  │ 6 Engine       a2m-engine     the loop          a2m-sched    queues, leases, locks        │
  │ 5 Contract     a2m-provider   registry, manifests, extension points, supervisor           │
  │ 4 Services     a2m-policy     a2m-auth     a2m-sources                                     │
  │ 3 Stores       a2m-files      target files to a plan     a2m-ledger   the store            │
  │ 2 Foundations  a2m-expr       a2m-vault    a2m-telemetry                                   │
  │ 1 Types        a2m-core                                                                    │
  └──────────────▲────────────────────────────────────────────────────────────────────────────┘
                 │ registers through the contract, and is called back
  Packs       web (browser, ui, probe) · http · mcp · desktop · then command, data, file, stream, device
                 │
  Systems     whatever already exists and offers a surface the user is entitled to use
```

The kernel is layers 1 to 8. A pack sees layers 1 to 5 and nothing above. The product crate, `anything2mcp`, is the only code that names a pack: it hands each enabled pack to the registry and starts the roles.

## Layers

The binding table is in the [technical reference](../reference/technical-reference.md#crate-layering), and the audit enforces it. This is the reasoning behind each layer ([decision 0061](../decisions/0061-the-layers-are-redrawn-so-that-every-needed-dependency-is-allowed.md)).

| Layer | Crates | Why it sits here |
| --- | --- | --- |
| 1. Types | `a2m-core` | The words everyone shares, and nothing that does anything |
| 2. Foundations | `a2m-expr`, `a2m-vault`, `a2m-telemetry` | Each stands alone: evaluating an expression, sealing a secret, emitting a span |
| 3. Stores | `a2m-files`, `a2m-ledger` | The two durable forms of knowledge: what a target is (files compiled to a plan) and what happened (the store). The compiler type-checks expressions, so it sits above `a2m-expr`. Everything that persists goes through `a2m-ledger`, so it sits below every service |
| 4. Services | `a2m-policy`, `a2m-auth`, `a2m-sources` | Each answers one question: may this happen, who is signed in, what records does this input hold |
| 5. Contract | `a2m-provider` | The one doorway between the kernel and everything that knows a kind of system. It sits above the services because the extension points speak in their terms: a session lease, a source reader, a plan step |
| 6. Engine | `a2m-engine`, `a2m-sched` | Carrying out a plan, and deciding when and where it may be carried out. Neither depends on the other; a role joins them |
| 7. Actions | `a2m-actions` | Every way into the kernel, defined once |
| 8. Roles | `a2m-local`, `a2m-mcp`, `a2m-api`, `a2m-runner` | The shapes a process can take |
| 9, 10. Packs | `a2m-pack-*` | Knowledge of one kind of system. They implement the contract and are called back |
| 11. Product | `anything2mcp` | Names the packs, and nothing else |

## Who defines, who implements, who wires

A dependency may only point down, so an interface is defined in the lowest crate that must call it, implemented wherever the knowledge is, and joined to its caller by a crate above both. This table is the spine of the architecture. Every crate design in this folder conforms to it.

| Interface | Defined in | Implemented by | Called by | Joined by |
| --- | --- | --- | --- | --- |
| Vocabulary, `Failure`, `Secret`, identifiers, `Value`, `Finding` and its `Subject` | `a2m-core` | Not applicable | Every crate | Not applicable |
| `Redact`: text in, text with every registered secret removed | `a2m-core` | `a2m-vault` | `a2m-telemetry`, `a2m-ledger`, every front door | `a2m-actions` |
| Expressions: compile once, evaluate many times; host functions for units, numbers, dates | `a2m-expr` | `a2m-expr` | `a2m-files` to type-check; `a2m-engine`, `a2m-sources`, `a2m-policy` to evaluate | Not applicable |
| Sealing and opening; key-store back ends | `a2m-vault` | `a2m-vault`; an outside key service through a back end | `a2m-ledger`, `a2m-auth` | `a2m-actions` |
| Spans, metrics, the step trace | `a2m-telemetry` | `a2m-telemetry` | Every crate above it | `a2m-actions` |
| Target files: schemas, loader, linter, compiler, the `Plan`; the drafter | `a2m-files` | `a2m-files` | `a2m-actions` to lint, compile, draft; `a2m-engine` to read a plan | Not applicable |
| What a pack registers with the compiler: file kinds, verb lowerings, lint rules, drafting rules | `a2m-files` | Packs | `a2m-files` | `a2m-provider` gathers them into one set; `a2m-actions` passes the set to the compiler |
| The store: runs, the ledger, events, evidence blobs, findings, the audit chain; migrations | `a2m-ledger` | `a2m-ledger`, on SQLite and Postgres | Layers 4 to 8 | `a2m-actions` |
| `decide`: one request in, one recorded decision out; approvals | `a2m-policy` | `a2m-policy` | `a2m-actions` at every entry; `a2m-engine` at guard | `a2m-actions` |
| The session keeper; `SessionLease` | `a2m-auth` | `a2m-auth` | `a2m-engine` for a lease; `a2m-actions` for sign-in and status | `a2m-actions` |
| `AuthProvider`: sign in, check, renew, sign out, inject | `a2m-auth` | `a2m-auth` for the neutral ones; packs for the ones that need a surface, such as a browser | The session keeper only | `a2m-provider` registers; `a2m-actions` hands them to the keeper |
| `KeeperWork`: run a probe, a renewal, or a sign-in on a session's home runner | `a2m-auth` | `a2m-actions`, by submitting a job | The session keeper | `a2m-actions` |
| `SourceReader`: an input turned into validated records | `a2m-sources` | `a2m-sources` for the built-in formats; plug-ins | `a2m-engine` | `a2m-provider` |
| `Manifest`, the `Registry`, the supervisor, the plug-in host | `a2m-provider` | `a2m-provider` | `a2m-engine`, `a2m-actions` | The product crate fills the registry |
| `Driver`: prepare, act, health, release | `a2m-provider` | Packs; plug-ins through the plug-in host | `a2m-engine` only | `a2m-provider` |
| `Discoverer` and `Describer` | `a2m-provider` | Packs; plug-ins | `a2m-actions`, for `discover.run` and the scout | `a2m-provider` |
| The engine loop: one job in, one outcome out | `a2m-engine` | `a2m-engine` | A runner, in this process or another | A role |
| `Control`: what the engine must ask for while it runs: a lease, a record lock, a rate budget, an approval check, a place to send events | `a2m-engine` | `a2m-actions` directly in one process; `a2m-runner` over the runner link | `a2m-engine` | A role |
| Admission, queues, job leases, record locks, budgets | `a2m-sched` | `a2m-sched` | `a2m-actions` to submit; a runner to claim | `a2m-actions` |
| The action registry, the dispatcher, and the `Kernel` handle | `a2m-actions` | `a2m-actions` | Every role | A role builds the `Kernel` |
| MCP, REST, the local channel, the runner link | The role crates | The role crates | Callers | The product crate |

Four consequences are worth stating, because each closes a question an implementer would otherwise have to guess at.

- **The engine cannot name a driver, a pack, or a mode.** It asks the registry for the driver registered for a binding's mode and gets back a trait object.
- **The engine and the scheduler do not know each other.** The scheduler hands out job leases. A runner claims one and calls the engine. In one process the runner is a loop inside `a2m-actions`; across machines it is `a2m-runner`.
- **The session keeper never calls upward.** Its checks and renewals are work that must run on a session's home runner, so it is handed a `KeeperWork` and uses that.
- **Every entry is an action.** Calling a target's tool is the action `tool.call`. A front door authenticates the caller, builds a request, and dispatches. It decides nothing and holds no engine logic.

## Five flows

Each step names the crate that does it. A crate design repeats its own steps in more detail.

### A read

1. A front door (`a2m-mcp`, `a2m-api`, or a command through `a2m-local`) authenticates the caller and dispatches `tool.call`.
2. `a2m-actions` asks `a2m-policy` to decide. The decision is recorded whether it allows or refuses.
3. `a2m-actions` submits the run to `a2m-sched`, which admits or refuses it against queues, per-session limits, and budgets, and writes the run row through `a2m-ledger`.
4. A runner claims the job and calls `a2m-engine` with a `Control`.
5. The engine loads the published plan from `a2m-ledger`, guards, and chooses a binding by its declared order and health.
6. The engine asks for a `SessionLease` through `Control`; the keeper in `a2m-auth` supplies it or the run stops at `needs_login`.
7. The engine asks the `a2m-provider` registry for the driver of the binding's mode, and calls prepare and act for each step. The driver is pack code.
8. The engine normalizes the values with the field types, records events and evidence through `a2m-ledger` and `a2m-telemetry`, and returns the outcome.
9. The front door returns the result, already redacted, with a reference to the trace.

### A write

Steps 1 to 6 as for a read, then:

7. The engine reads current values and builds the diff. If policy requires approval the run rests at `needs_approval`, and the caller gets the diff's reference and nothing that could approve it.
8. A person approves through a surface the caller cannot reach. `a2m-policy` stores a single-use approval bound to the diff's hash.
9. On resume the engine checks the approval through `Control`. For each record it takes the record lock, writes the intent through `a2m-ledger`, and only then calls the driver. The ledger gives the engine a token for the recorded intent, and the driver cannot be called for a write without one.
10. The engine confirms, reads the value back, compares after normalization, and writes `verified` or stops with a failure class.
11. On any resume, the engine reconciles before it writes: for every record in `intent` or `acted`, it reads the system first.

### Building tools

1. `target.add` registers a target in the project folder.
2. `scout.run` in `a2m-actions` probes each surface and runs the discoverers the registry holds for it. Discovery runs through the engine's sandbox, where a write is impossible.
3. Findings are stored through `a2m-ledger`. What the system says about itself is compared with what was seen.
4. The drafter in `a2m-files` writes draft files wherever a pack's drafting rules say how.
5. `a2m-files` lints and compiles, using the file kinds and lowerings the packs registered.
6. Drafted reads run in the sandbox five times. The developer or their agent supplies what the report says is missing.
7. `test.run` runs the publish gates. A person publishes with `version.publish`. The plan is stored with its hash, and becomes current in stages.

### Starting a process

1. The product crate reads the settings and builds a registry: one call per enabled pack, then the plug-ins found on the machine, each checked against its manifest.
2. A role calls `Kernel::open` in `a2m-actions` with the settings and the registry. It opens the store and runs migrations, opens the vault, loads policy, and starts the keeper, the scheduler, and the engine.
3. If the store, the vault, or the policy set cannot be opened, the kernel starts in a degraded state in which nothing writes ([spec section 52](../spec/52-when-the-engine-itself-fails.md)).
4. The role starts its front doors: the local channel on one machine; the REST and MCP endpoints on a server; the runner link on a runner.

### A runner on another machine

1. `a2m-runner` opens the connection outward and says what it can serve.
2. It claims a job and receives a lease with a deadline.
3. It calls the same engine with a `Control` that speaks over the link: every lease, lock, budget, and approval check is a `need` answered by the control plane.
4. Events flow back as they happen. A lease that ends without a result is treated as `possible_commit` and reconciled before anyone else gets the job.

## Where state lives

| State | Lives in | Owned by |
| --- | --- | --- |
| Target files being written | The developer's project folder | The developer; `a2m-files` reads and the drafter writes |
| Published versions and their plans | The store | `a2m-ledger`, written only by `version.publish` |
| Runs, the ledger, events, findings, decisions, approvals, grants, sessions' records, jobs, locks | The store | `a2m-ledger` holds the tables; each service owns the meaning of its own |
| Evidence | Blobs in the data directory, by content hash, sealed | `a2m-ledger` stores; `a2m-vault` seals |
| Secrets and saved session state | The store, sealed; the root key in the operating system's key store or a key service | `a2m-vault` |
| A live session: a browser profile, tokens in memory, an open connection | The session's home runner | `a2m-auth` decides; the pack's driver holds the handle |
| Settings | The settings file and the environment | Read once at start by the product crate |
| Nothing | A front door, a command, an SDK | Thin clients hold no state |

## Process shapes

| Shape | Processes | Notes |
| --- | --- | --- |
| One machine | One engine per user, started on demand; commands and the stdio proxy are thin clients of it | [Spec section 39](../spec/39-local-engine-and-clients.md) |
| A server | A control plane and one or more runners. They may be one process | [Spec section 43](../spec/43-runner-protocol.md) |
| A plug-in | Its own process, started and stopped by the supervisor in `a2m-provider` | [Spec section 57](../spec/57-supervising-subsystems.md) |
| A browser, a database client, a device link | Owned by the pack that needs it, behind its driver | The kernel never holds a handle to one |

## Rules the arrangement enforces

A rule that depends on every author remembering it will be broken. Each of these is enforced by where something is defined.

| Rule | How the arrangement enforces it | Proof |
| --- | --- | --- |
| No AI at run time | No crate on the path of a run may depend on a model client; the engine executes only a compiled plan | A ban list in `deny.toml` once a model client could be depended on by accident; engine tests |
| Every write is verified | Only the engine calls `Driver::act`, and a write needs the token the ledger gives for a recorded intent | The nine ledger rules of spec section 42, each a test |
| Secrets never reach a caller | `Secret` cannot be printed or serialized; credentials reach a driver only through `inject`; everything leaving a front door passes `Redact` | The compile-time test in `a2m-core`; a fault test that plants a secret in a system's answer |
| The kernel is neutral | The layering table | The audit, with planted mistakes |
| One decider | Only `a2m-policy` exports `decide`; no other crate holds a grant | Review; a test that every action's dispatch records a decision |
| One renewer | `AuthProvider` is called only by the keeper; a driver receives a lease and reports signs of expiry | The keeper's ten-runs-one-renewal test |
| Unknown means stop | The compiler refuses a mode, a control, or a file kind nobody registered; a driver returns `unknown_variant` | Compiler tests; conformance tests of each driver |
| Thin clients | Commands and SDKs are generated from the action registry and contain no branch on a result | Generated code is checked in CI against the registry |
| Fail closed | The `Kernel` has three health states, and a write needs the healthy one | Fault tests that remove the store, the vault, and the policy set |

## Crate designs

One file per crate, in [crates/](crates/). The audit checks that every crate on disk has one and that every design names a crate that exists.

Each design has the same parts, in this order.

| Part | What it says |
| --- | --- |
| Purpose | One or two sentences |
| Facts | Side, layer, what it may depend on, what it does depend on, the stage or phase that builds it, the spec sections it carries out |
| Owns | What this crate is responsible for, and what it must never do |
| Interface | The public types, traits, and functions, as a Rust sketch, each with one line on its contract |
| Used by, and uses | Which crates call which of those, and what this crate calls |
| State | Tables, files, and memory it owns |
| Failures | The failure classes and codes it can produce |
| Invariants | What must always hold, and the test that proves each |
| Build order | The smallest first step, then the next, each provable alone |
| Open points | What the building slice must settle |

## The parts that are not crates

Test stands, the pip and npm packages, the pages, profiles, plug-in examples, benchmarks, and build tasks are in [other parts](other-parts.md).

## Pack designs

A pack is designed as a spec section before it is built ([section 55](../spec/55-kernel-and-packs.md)). The crate designs for the four first packs are in [crates/](crates/); the packs that have no crates yet are designed in the specification, and listed in its [index](../spec/README.md).

## Using this folder

- **To build a package from the [roadmap](../plan/roadmap.md):** read the design of each crate it touches, then the spec sections the design lists. Build the first unbuilt step of the build order.
- **To change an interface in the spine table:** it is a public contract. Write a decision record, change the table and both crate designs in the same change.
- **When code and a crate design differ:** fix one of them in the same change, as with any document.
