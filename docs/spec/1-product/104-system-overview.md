# 104. System overview

The system has five layers of responsibility between the callers and the system being wrapped. In the smallest setup they all run inside one process on a laptop; in the largest, the control plane and the runners are separate services on separate machines. The layers and their contracts are the same either way. The overview is of the design: on one machine every layer is built in part, and separate control-plane and runner machines are not built ([section 704](../7-project/704-readiness.md)).

| Layer | Responsibility | Knows about a specific kind of system? | Knows about a specific target? |
| --- | --- | --- | --- |
| Front doors | Speak MCP, REST, and the local channel; authenticate callers; serve the sign-in and approval pages | No | No |
| Control plane | Decide whether a call may run, queue it, record it, pick a runner, keep sessions alive | No | Only through published target files |
| Runners | Run the engine loop on a job; hold the live sessions: a browser, an HTTP client, a desktop handle, a database connection, a device link | No | No |
| Drivers, in packs | Carry out one step in one mode | Yes: each pack knows one kind | No |
| Target files | Describe one target: its surfaces, the pack's own file kinds, field types, operations, pipelines | Through the file kinds its packs bring | Yes; this is the only place |

## Kernel and packs in the picture

Everything above the drivers is the kernel. A pack sits beside the kernel, not inside it: it registers its file kinds, lowerings, lint rules, driver, discoverers, and drafting rules through the provider contract, and the kernel calls it back ([section 202](../2-kernel/202-kernel-and-packs.md)).

```text
  Callers      agents over MCP · programs over REST and the SDKs · people at the command line and the console
                  │
  ┌───────────────▼─────────────────────── kernel ──────────────────────────────────────┐
  │ Front doors     MCP, REST, the local channel, the sign-in and approval pages        │
  │ Control plane   policy · scheduler · ledger · session keeper · registry · compiler  │
  │ Runners         the engine loop, on this machine or another                          │
  │ Provider contract                                                                    │
  └───────────────▲──────────────────────────────────────────────────────────────────────┘
                  │ registers through the contract, and is called back
  Packs        web · http · mcp · desktop · command · data · file · stream · device
                  │ drivers reach the system through its own surfaces
  Systems      whatever already exists and offers a surface the user is entitled to use
```

The crates that carry each layer, and who defines, implements, and joins each interface between them, are in [the architecture](../../architecture/README.md). A live session lives on its home runner, held by the pack's driver; the session keeper decides about it from the control plane. The engine never touches the store: everything the engine needs while it runs a job, it asks of the control plane through one interface, so the same engine runs unchanged on a machine that has no store.

## Runners

**Runners are the key to deployment flexibility.** A runner is the same engine code started in a worker role. It opens its connection outward to the control plane, claims jobs under a lease, and reports results ([section 223](../2-kernel/223-the-runner-protocol.md)). In one process the runner is a loop inside the kernel; across machines it is a separate process. A job that needs a live session goes to the session's home runner, the one that holds it.

| Kind | Lives | Handles |
| --- | --- | --- |
| Server runner | Beside the control plane, or on any machine the control plane trusts | Work that needs no interactive desktop: browser, HTTP, MCP, and the other packs' work on that machine |
| Desktop runner | In an interactive session of an operating system | Desktop applications |
| User-device runner | On a person's own computer | Work that must use the sign-in that device already has, which is what lets a company server act as each person even when sessions are bound to managed devices. It is given only jobs that act as that person |

Work that must happen on a particular machine, such as starting a program installed there, goes to a runner on that machine. Nothing reaches another machine by a remote login ([section 306](../3-packs/306-the-command-pack.md)).

## A read, end to end

A caller asks for one order on the `example` target through the tool `example_orders_order_read`.

1. A front door authenticates the caller and turns the call into the action `tool.call`.
2. The policy engine decides whether this caller may run this tool on this target. The decision is recorded whether it allows or refuses.
3. The scheduler admits the run against queues, per-session limits, and budgets, and the run is recorded.
4. A runner claims the job and starts the engine loop.
5. The engine fetches the published plan by its hash and checks the hash, guards with the recorded decision, and chooses a binding by its declared order and health.
6. The engine asks for a session lease. The session keeper supplies one, or the run stops at `needs_login`.
7. The engine asks the registry for the driver of the binding's mode, which is pack code, and has it carry out each step: in the web pack, open the order's screen and read its fields; in the HTTP pack, send the matching read request.
8. The engine normalizes the values with their field types, sends the step trace to be stored, and returns the outcome.
9. The front door returns the result, already redacted, with a reference to the trace.

## A write, end to end

A caller asks to enter a sheet of orders on the `example` target through the pipeline tool `example_order_sheet_enter`, with a sheet and an order number.

1. The front door authenticates the caller; the policy engine checks the grants, and the source reader validates every row of the sheet before anything runs.
2. The scheduler queues the run against the right session and a runner claims it.
3. The engine performs the read steps, reads the current values, and builds a diff of current against proposed values, record by record.
4. Policy requires approval, so the run rests at `needs_approval`. The caller gets a reference to the diff and nothing that could approve it.
5. A person approves the diff through a surface the caller cannot reach. The approval is stored once, bound to the diff's hash.
6. On resume, the engine checks the approval. For each record it takes the record lock and records the intent in the ledger, and only then has the driver write, through the binding chosen by declared order and health.
7. The engine confirms the write, reads the value back from the system, compares after normalization, and records the record `verified`, or stops with a failure class.
8. The caller receives verified counts, the totals read back from the system, and a reference to the trace.

If the run is interrupted at any point, the next resume reconciles first: every record left in `intent` or `acted` is read from the system before anything is written again ([section 208](../2-kernel/208-runs-the-ledger-and-resume.md)).

Interfaces, the data model, protocols, and catalogs are in the [technical reference](../../reference/technical-reference.md).
