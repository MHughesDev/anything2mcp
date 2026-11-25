# Architecture

How the code of Anything2MCP is put together: which crate owns what, which interface each crate offers, who calls whom, and where every piece of state lives. The [specification](../spec/README.md) says what the system does and why. The [technical reference](../reference/technical-reference.md) holds interfaces once they are settled. The [plan](../plan/roadmap.md) says in what order things are built. This folder is the bridge between them: read it before writing code in any crate.

Nothing here overrides a rule. Where this folder and a spec section differ on what the system does, the spec wins and this folder is corrected. Where they differ on how the code is arranged, this folder wins.

Every crate described here exists, and each is built in part or in full, except `a2m-runner`, which is empty. Each crate design's build order says which steps are done. The Rust in this folder is a sketch of shape and ownership: the slice that builds an interface settles its final form, and the decision record of that slice says where the code differs from the sketch.

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
  │ 3 Stores       a2m-files      target files to a compiled target     a2m-ledger   the store            │
  │ 2 Foundations  a2m-expr       a2m-vault    a2m-logs                                   │
  │ 1 Types        a2m-core                                                                    │
  └──────────────▲────────────────────────────────────────────────────────────────────────────┘
                 │ registers through the contract, and is called back
  Packs       web (browser, ui, probe) · http · mcp · desktop · command · data · file · stream · device
                 │
  Systems     whatever already exists and offers a surface the user is entitled to use
```

The kernel is layers 1 to 8. A pack sees layers 1 to 5 and nothing above. The product crate, `anything2mcp`, is the only code that names a pack: it hands each enabled pack to the registry and starts the roles.

## Layers

The binding table is in the [technical reference](../reference/technical-reference.md#crate-layering), and the audit enforces it. This is the reasoning behind each layer ([decision 0061](../decisions/0061-the-layers-are-redrawn-so-that-every-needed-dependency-is-allowed.md)).

| Layer | Crates | Why it sits here |
| --- | --- | --- |
| 1. Types | `a2m-core` | The words everyone shares, and nothing that does anything |
| 2. Foundations | `a2m-expr`, `a2m-vault`, `a2m-logs`, `a2m-netstate` | Each stands alone: evaluating an expression, sealing a secret, emitting a span, reading what the machine knows of its own networks |
| 3. Stores | `a2m-files`, `a2m-ledger` | The two durable forms of knowledge: what a target is (files compiled into a compiled target) and what happened (the store). The compiler type-checks expressions, so it sits above `a2m-expr`. Everything that persists goes through `a2m-ledger`, so it sits below every service |
| 4. Services | `a2m-policy`, `a2m-auth`, `a2m-sources` | Each answers one question: may this happen, who is signed in, what records does this input hold |
| 5. Contract | `a2m-provider` | The one doorway between the kernel and everything that knows a kind of system. It sits above the services because the extension points speak in their terms: a session lease, a source reader, a compiled target step |
| 6. Engine | `a2m-engine`, `a2m-sched` | Carrying out a compiled target, and deciding when and where it may be carried out. Neither depends on the other; a role joins them |
| 7. Actions | `a2m-actions` | Every way into the kernel, defined once |
| 8. Roles | `a2m-local`, `a2m-mcp`, `a2m-api`, `a2m-runner` | The shapes a process can take |
| 9, 10. Packs | `a2m-pack-*` | Knowledge of one kind of system: the web, HTTP, MCP, and desktop packs, and the command, data, file, stream, and device packs, each with its own crates. They implement the contract and are called back |
| 11. Product | `anything2mcp` | Names the packs, and nothing else |
| 12. Copy test harness | `a2m-copy-test-harness` | Test equipment in `tools/copy-test-harness/`, never packaged: the product's own `serve` with a presence prompt only a copy test's runner answers ([decision 0310](../decisions/0310-the-field-harness-runs-in-a-closed-network-and-says-in-the-ledger-what-it-is.md)) |

## Who defines, who implements, who wires

A dependency may only point down, so an interface is defined in the lowest crate that must call it, implemented wherever the knowledge is, and joined to its caller by a crate above both. This table is the spine of the architecture. Every crate design in this folder conforms to it.

| Interface | Defined in | Implemented by | Called by | Joined by |
| --- | --- | --- | --- | --- |
| Vocabulary, `Failure`, `Secret`, identifiers, `Value`, `Finding` and its `Subject` | `a2m-core` | Not applicable | Every crate | Not applicable |
| The `CompiledTarget` and its steps, as data, with the check of a compiled target's hash | `a2m-core` | Produced only by the compiler in `a2m-files` | `a2m-engine`, `a2m-provider`, packs | Not applicable |
| `Redact`: text in, text with every registered secret removed; and a keyed digest for naming things without revealing them | `a2m-core` | `a2m-vault` | `a2m-logs`, `a2m-ledger`, every front door | `a2m-actions` |
| Expressions: compile once, evaluate many times; host functions for units, numbers, dates | `a2m-expr` | `a2m-expr` | `a2m-files` to type-check; `a2m-engine`, `a2m-sources`, `a2m-policy` to evaluate | Not applicable |
| Sealing and opening, as methods of the `Vault` itself, with no trait between; key-store back ends | `a2m-vault` | `a2m-vault`; an outside key service through a back end that a plug-in may supply | `a2m-ledger`, which is handed the vault whole through `Store::attach`; `a2m-auth`, which is handed the same vault | `a2m-actions` |
| `KeyRecords`: where the vault's wrapped keys are kept, one row per key and generation | `a2m-vault` | `a2m-ledger`, which gives it out through `Store::key_records` and holds the store weakly | `a2m-vault` | `a2m-actions` |
| Spans, metrics, the step trace | `a2m-logs` | `a2m-logs` | Every crate above it. The finished trace goes back to the engine as redacted bytes, and the engine sends it to the store | `a2m-actions` |
| Target files: schemas, loader, linter, compiler; the drafter | `a2m-files` | `a2m-files` | `a2m-actions` to lint, compile, draft. Never the engine | Not applicable |
| What a pack registers with the compiler: file kinds, verb lowerings, lint rules, drafting rules | `a2m-files` | Packs | `a2m-files` | `a2m-provider` gathers them into one set; `a2m-actions` passes the set to the compiler |
| The store: runs, the ledger with its record locks, events, evidence blobs, findings, the audit chain; migrations | `a2m-ledger` | `a2m-ledger`, on SQLite and Postgres | Layers 4 to 8 | `a2m-actions` |
| `decide`: one request in, one recorded decision out; approvals | `a2m-policy` | `a2m-policy` | `a2m-actions` at every entry; the engine at guard, through `Control` | `a2m-actions` |
| The session keeper; `SessionLease` | `a2m-auth` | `a2m-auth` | The engine for a lease, through `Control`; `a2m-actions` for sign-in and status | `a2m-actions` |
| `AuthProvider`: sign in, check, renew, sign out, inject | `a2m-auth` | `a2m-auth` for those that only hold or exchange a credential; packs for those that need a surface of the system, such as a browser | The session keeper only | `a2m-provider` registers; `a2m-actions` hands them to the keeper |
| `KeeperWork`: run a probe, a renewal, or a sign-in on a session's home runner | `a2m-auth` | `a2m-actions`, by submitting a job | The session keeper | `a2m-actions` |
| `SourceReader`: an input turned into validated records. It takes bytes, never a path | `a2m-sources` | `a2m-sources` for the built-in formats; plug-ins | `a2m-engine` | `a2m-provider` |
| `Manifest`, the `Registry`, the plug-in host | `a2m-provider` | `a2m-provider` | `a2m-engine`, `a2m-actions` | The product crate fills the registry |
| The supervisor: the only way anything starts a process | `a2m-provider` | `a2m-provider` | The plug-in host; packs' drivers, each through a supervisor scoped to the programs a published compiled target declares | `a2m-provider` |
| `Driver`: prepare, act, health, release | `a2m-provider` | Packs; plug-ins through the plug-in host | `a2m-engine` only, and only through a handle that checks the intent token on every step that commits | `a2m-provider` |
| `Discoverer`, with its describer | `a2m-provider` | Packs; plug-ins | A runner, inside the engine's sandbox, for a discovery job that `a2m-actions` submits for `discover.run` and the scout | `a2m-provider` |
| `SandboxPort`: what a discoverer may ask the engine for, with writing impossible | `a2m-provider` | `a2m-engine` | Discoverers | `a2m-actions` |
| File access for packs: a path that can only be made by the check against the allowed roots | `a2m-provider` | `a2m-provider` | Packs, which receive checked paths and never resolve one; `a2m-actions` for the kernel's own reads | `a2m-provider` |
| An action a provider adds | `a2m-provider` | Packs; plug-ins | The dispatcher in `a2m-actions` | `a2m-provider` |
| The engine loop: one job in, one outcome out | `a2m-engine` | `a2m-engine` | A runner, in this process or another | A role |
| `Control`: everything the engine needs from the control plane while it runs a job: the compiled target by its hash, the policy decision at guard, a session lease, a record lock, a rate budget, the approval check, recording an intent and getting its token, sending events and evidence | `a2m-engine` | `a2m-actions` directly in one process; `a2m-runner` over the runner link | `a2m-engine` | A role |
| Admission, queues, job leases, waiting for record locks and other exclusive resources, budgets | `a2m-sched` | `a2m-sched` | `a2m-actions` to submit; a runner to claim | `a2m-actions` |
| The action registry, the dispatcher, the `Kernel` handle, and the check of a caller's credential | `a2m-actions` | `a2m-actions` | Every role | A role builds the `Kernel` |
| `PersonSurface`: asking a person to approve, to sign in, or to confirm they are present | `a2m-actions` | `a2m-local` on one machine; `a2m-api` on a server | `a2m-actions` | The product crate hands it to `Kernel::open` |
| MCP, REST, the local channel, the runner link | The role crates | The role crates | Callers | The product crate. Role crates share a layer and never use each other |

Nine consequences are worth stating, because each closes a question an implementer would otherwise have to guess at.

- **The engine cannot name a driver, a pack, or a mode.** It asks the registry for the driver registered for a binding's mode and gets back a handle.
- **The engine never touches the store, the policy engine, or the compiler.** Everything it needs while running a job comes through `Control`, so the same engine runs unchanged on a machine that has no store. The audit refuses a dependency from `a2m-engine` on `a2m-files` or `a2m-policy`; it may name a type from `a2m-ledger`, such as the intent token, and never opens a store.
- **The engine and the scheduler do not know each other.** The scheduler hands out job leases. A runner claims one and calls the engine. In one process the runner is a loop inside `a2m-actions`; across machines it is `a2m-runner`.
- **The session keeper never calls upward.** Its checks and renewals are work that must run on a session's home runner, so it is handed a `KeeperWork` and uses that.
- **Every entry is an action, with one exception.** Calling a target's tool is the action `tool.call`. A front door reads the caller's credential, has `a2m-actions` check it, builds a request, and dispatches. It decides nothing and holds no engine logic. The exception is recording a person's approval or refusal: it is deliberately not an action and has no generated face, so that no command, tool, or route can approve. The approval surface is given a handle that can do those two things and nothing else.
- **A table has one owner.** The store owns the connection, the transactions, and the migrations; each crate owns the meaning of its own tables, and no crate's SQL names another's. A transaction is a closure run on the store's own thread, `store.write(durability, |tx| ...)`, and a crate above reaches its own tables through `tx.tables(Owner::X)` ([decision 0080](../decisions/0080-the-store-is-one-sqlite-file-and-a-crate-reaches-its-own-tables-through-the-transaction.md)). The one declared exception: when writes begin, the ledger checks the approval that policy recorded, in the same transaction.
- **Words that arrive with packs are registered names, not enums.** A mode, a surface kind, a step kind, a file kind, a lint code, a finding kind, a control name, and a failure code are checked names that a provider registers or a crate defines. A step kind is a name within its mode. The kernel's enums are the words that are the same for every kind of system.
- **Packs share data shapes and never code.** A file kind or a finding kind has one owning pack, which registers its schema for a surface kind. Another pack may declare that it reads the kind, and the compiler hands it the validated entries as data ([decision 0062](../decisions/0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md)).
- **A pack is handed a context when it registers:** a supervisor scoped to what it may start, its folders as checked paths, the redactor, and the logs and traces. The product crate joins a pack's own crates where there are several, as it does for the web pack's shared browser handle, and does nothing else with a pack.

## Five flows

Each step names the crate that does it. A crate design repeats its own steps in more detail.

### A read

1. A front door (`a2m-mcp`, `a2m-api`, or a command through `a2m-local`) authenticates the caller and dispatches `tool.call`.
2. `a2m-actions` asks `a2m-policy` to decide. The decision is recorded whether it allows or refuses.
3. `a2m-actions` submits the run to `a2m-sched`, which admits or refuses it against queues, per-session limits, and budgets, and writes the run row through `a2m-ledger`.
4. A runner claims the job and calls `a2m-engine` with a `Control`.
5. The engine fetches the published compiled target by its hash through `Control`, checks the hash, guards with the policy decision `Control` returns, and chooses a binding by its declared order and health.
6. The engine asks for a `SessionLease` through `Control`; the keeper in `a2m-auth` supplies it or the run stops at `needs_login`.
7. The engine asks the `a2m-provider` registry for the driver of the binding's mode, and calls prepare and act for each step. The driver is pack code.
8. The engine normalizes the values with the field types, sends events and the finished trace through `Control`, which in one process writes them through `a2m-ledger`, and returns the outcome.
9. The front door returns the result, already redacted, with a reference to the trace.

### A write

Steps 1 to 6 as for a read, then:

7. The engine reads current values and builds the diff. If policy requires approval the run rests at `needs_approval`, and the caller gets the diff's reference and nothing that could approve it.
8. A person approves through a surface the caller cannot reach. `a2m-policy` stores a single-use approval bound to the diff's hash.
9. On resume the engine checks the approval through `Control`. For each record it takes the record lock and records the intent, both through `Control`, and only then calls the driver. The ledger answers a recorded intent with a token, and the driver's handle refuses a step that commits without one.
10. The engine confirms, reads the value back, compares after normalization, and writes `verified` or stops with a failure class.
11. On any resume, the engine reconciles before it writes: for every record in `intent` or `acted`, it reads the system first.

### Building tools

1. `target.add` registers a target in the project folder.
2. `scout.run` in `a2m-actions` submits discovery jobs. A runner runs the discoverers the registry holds for each surface inside the engine's sandbox, which gives a discoverer only what `SandboxPort` offers. What "cannot write" means is stated per access kind by each pack: a request that is not a read is never sent, a program is started only in its declared help and version forms, a queue is never consumed.
3. Findings are stored through `a2m-ledger`. What the system says about itself is compared with what was seen.
4. The drafter in `a2m-files` writes draft files wherever a pack's drafting rules say how.
5. `a2m-files` lints and compiles, using the file kinds and lowerings the packs registered.
6. Drafted reads run in the sandbox five times. The developer or their agent supplies what the report says is missing.
7. `test.run` runs the publish gates. A person publishes with `version.publish`. The compiled target is stored with its hash, and becomes current in stages.

### Starting a process

1. The product crate reads the settings and builds a registry: one call per enabled pack, then the plug-ins found on the machine, each checked against its manifest.
2. A role calls `Kernel::open` in `a2m-actions` with the settings and the registry. It opens the store and runs migrations, opens the vault, loads policy, and starts the keeper, the scheduler, and the engine.
3. The kernel has three health states ([spec section 224](../spec/2-kernel/224-when-the-engine-itself-fails.md)). If the store cannot be opened or no valid policy version exists, it is stopped and serves nothing but its own status. If the vault or the space for evidence is unavailable, it is degraded: reads that need no secret still run, and nothing writes, because nothing could be sealed or verified.
4. The product crate joins the role crates, which never use each other: the local channel on one machine; the REST and MCP endpoints on a server; the runner link on a runner.

### A runner on another machine

1. `a2m-runner` opens the connection outward and says what it can serve.
2. It claims a job and receives a lease with a deadline.
3. It calls the same engine with a `Control` that speaks over the link: every lease, lock, budget, and approval check is a `need` answered by the control plane.
4. Events flow back as they happen. A lease that ends without a result is treated as `possible_commit` and reconciled before anyone else gets the job.

## Where state lives

| State | Lives in | Owned by |
| --- | --- | --- |
| Target files being written | The developer's project folder | The developer; `a2m-files` reads and the drafter writes |
| Published versions and their compiled targets | The store | `a2m-ledger`, written only by `version.publish` |
| Runs, the ledger, record locks, events, findings, the audit chain, evidence blobs | The store and the data directory | `a2m-ledger` |
| Decisions, approvals, grants, consents, delegations, kill switches, quota counters | The store | `a2m-policy` |
| Sessions' records, their events, and saved session state, sealed | The store | `a2m-auth` |
| Jobs, leases, and budgets, in the tables `jobs`, `runners`, `resource_holds`, `caller_turns`, `scheduler_holds`, and `budgets` | The store | `a2m-sched` |
| The health of each binding | The store | `a2m-engine`, written through `Control` |
| Toolsets, and the scout's stage results | The store | `a2m-actions` |
| The vault's wrapped keys | The store, through `KeyRecords`; the root key in the operating system's key store or a key service | `a2m-vault` |
| A live session: a browser profile, tokens in memory, an open connection or link | The session's home runner | `a2m-auth` decides; the pack's driver holds the handle |
| Settings | The settings file and the environment | Read once at start; the loader is in `a2m-actions` |
| Nothing | A front door, a command, an SDK | Thin clients hold no state |

One numbered sequence of migrations, in `a2m-ledger`, covers the whole schema. Each migration names the crate that owns its tables.

## Process shapes

| Shape | Processes | Notes |
| --- | --- | --- |
| One machine | One engine per user, started on demand; commands and the stdio proxy are thin clients of it | [Spec section 222](../spec/2-kernel/222-front-doors-and-the-local-engine.md) |
| A server | A control plane and one or more runners. They may be one process | [Spec section 223](../spec/2-kernel/223-the-runner-protocol.md) |
| A plug-in | Its own process, started and stopped by the supervisor in `a2m-provider` | [Spec section 204](../spec/2-kernel/204-supervising-subsystems.md) |
| A browser, a database client, a device link | Owned by the pack that needs it, behind its driver | The kernel never holds a handle to one |

## Rules the arrangement enforces

A rule that depends on every author remembering it will be broken. Each of these is enforced by where something is defined.

| Rule | How the arrangement enforces it | Proof |
| --- | --- | --- |
| No AI at run time | No crate on the path of a run may depend on a model client; the engine executes only a compiled target | A ban list in `deny.toml` once a model client could be depended on by accident; engine tests |
| Every write is recorded first and proven after | Only the engine reaches a driver, through a handle that refuses a committing step without the token the ledger gives for a recorded intent | The nine ledger rules of spec section 208, each a test |
| Secrets never reach a caller | `Secret` cannot be printed or serialized; credentials reach a driver only through `inject`; everything leaving a front door passes `Redact` | The compile-time test in `a2m-core`; a fault test that plants a secret in a system's answer |
| The kernel is neutral | The layering table | The audit, with planted mistakes |
| One decider | Only `a2m-policy` exports `decide`; no other crate holds a grant | Review; a test that every action's dispatch records a decision |
| Nothing but a person approves | Recording an approval is not an action and has no generated face | A test that the registry holds no action able to approve, on any face |
| The engine runs only compiled targets | The engine cannot depend on the compiler | The audit, with a planted dependency |
| One renewer | `AuthProvider` is called only by the keeper; a driver receives a lease and reports signs of expiry | The keeper's ten-runs-one-renewal test |
| Unknown means stop | The compiler refuses a mode, a control, or a file kind nobody registered; a driver returns `unknown_variant` | Compiler tests; conformance tests of each driver |
| Thin clients | Commands and SDKs are generated from the action registry and contain no branch on a result | Generated code is checked in CI against the registry |
| Fail closed | The `Kernel` has three health states, and a write needs the healthy one | Fault tests that remove the store, the vault, and the policy set |

## Crate designs

One file per crate, in [crates/](crates). The audit checks that every crate on disk has one and that every design names a crate that exists.

Each design has the same parts, in this order.

| Part | What it says |
| --- | --- |
| Purpose | One or two sentences |
| Facts | Side, layer, what it may depend on, what it does depend on, the roadmap packages that build it (row "Roadmap packages", whether or not they are done), the spec sections it carries out |
| Owns | What this crate is responsible for, and what it must never do |
| Interface | The public types, traits, and functions, as a Rust sketch, each with one line on its contract |
| Used by, and uses | Which crates call which of those, and what this crate calls |
| State | Tables, files, and memory it owns |
| Failures | The failure classes and codes it can produce |
| Invariants | What must always hold, and the test that proves each |
| Build order | The smallest first step, then the next, each provable alone. Each step is marked Done, Done in part, or Not built: this is where a design says what of its crate exists |
| Open points | What the building slice must settle |

## The parts that are not crates

Test stands, the pip and npm packages, the pages, profiles, plug-in examples, benchmarks, and build tasks are in [other parts](other-parts.md).

## Pack designs

A pack is designed as a spec section before it is built ([section 202](../spec/2-kernel/202-kernel-and-packs.md)). Every pack crate has a full design in [crates/](crates), next to the kernel's: the web pack's three crates and those of the HTTP, MCP, desktop, command, data, file, stream, and device packs. Every pack is designed in the specification, and listed in its [index](../spec/README.md). To add a pack, read [writing a pack](writing-a-pack.md): the parts of a pack as they are in the code, the entry bar with where the file pack clears each item, and what is not yet possible.

## Using this folder

- **To build a package from the [roadmap](../plan/roadmap.md):** read the design of each crate it touches, then the spec sections the design lists. Build the first unbuilt step of the build order.
- **To change an interface in the spine table:** it is a public contract. Write a decision record, change the table and both crate designs in the same change.
- **When code and a crate design differ:** fix one of them in the same change, as with any document.
