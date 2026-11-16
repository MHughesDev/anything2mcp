# 46. The target compiler and the compiled plan

Target files are written for people and agents to read. The engine never reads them at call time. A compiler turns a target's files into one checked, hashed plan, and the engine runs only plans. This section defines the compiler's stages, what the plan contains, the lint rules, and how versions stay compatible.

## Why compile

- **Speed.** Nothing is parsed, resolved, or type-checked during a call.
- **Safety.** Every mistake a file can contain is found before publishing, all at once, with a file and line.
- **Reproducibility.** The same files and the same compiler version give a byte-identical plan, so its hash identifies exactly what ran.

## Stages

| # | Stage | What it does | Fails on |
| --- | --- | --- | --- |
| 1 | Parse | Reads every YAML file, keeping the file, line, and column of each value | Malformed YAML |
| 2 | Validate | Checks each file against its JSON Schema and the target's schema version | Unknown or missing fields |
| 3 | Resolve | Links operations to screens, endpoints, and field types; pipelines to operations; sources to mappings | A name that does not exist; a name defined twice |
| 4 | Type-check | Checks every expression against the input and output schemas and field types | A type error in any expression |
| 5 | Analyze | Works out each operation's commit points, each pipeline's effect class, and each operation's permission manifest | A write with no key that is not marked irreversible; a declared effect lower than the derived one |
| 6 | Lint | Applies the rules below | Any rule marked error |
| 7 | Lower | Turns each binding's verb into the fixed list of [steps](../reference/steps-snapshots-and-diffs.md) its driver will run | A verb the mode does not support |
| 8 | Describe | Generates each tool's name, description, input and output schemas, and annotations | A tool that would be indistinguishable from another |
| 9 | Emit | Writes the plan and its hash | Nothing |

All problems from stages 2 to 8 are reported together, each with a code, a plain sentence, the place, and a suggested fix, and as JSON for agents.

**Commit points are counted, not assumed.** The ledger keeps one row per save in the application ([section 42](42-run-ledger-design.md)). The compiler counts the saves in each write binding: confirmed actions in a UI binding, write requests in an HTTP binding. An operation whose bindings disagree on the count, or whose count differs from what its author declared, does not compile.

**Every write can be found again.** A write operation must declare an identity strategy that is possible in each of its bindings ([section 50](50-record-identity-and-dependent-records.md)). Steps that use another step's result must not form a cycle.

**Templates are typed.** Every placeholder in a request template is given a position and encoded for it, so an input cannot change where a request goes ([section 51](51-defending-against-a-manipulated-agent.md)).

**Effect classes can only go up.** A pipeline's class is the highest of its operations'. An author can declare a higher class than the compiler derives, never a lower one.

## What the plan contains

| Part | Contents |
| --- | --- |
| Header | Plan format version, minimum engine version, target, version, compiler version, hash of the source files |
| Surfaces | Origins, base addresses, windows; how they share sessions |
| Field types | Normalizers and comparison rules, as host-function calls |
| Screens | Ordered locators, fingerprints, readiness conditions |
| Endpoints | Method, path template, compiled request and response schemas |
| Operations | Inputs, outputs, key, effect, commit-point count, and one step list per binding |
| Pipelines | The step graph, checkpoints, limits, success checks |
| Expressions | Every expression, already parsed and type-checked |
| Tools | Names, descriptions, schemas, annotations, as the front doors will list them |
| Manifests | Per operation: surfaces, uploads, downloads, and destinations it may touch |
| Tests | Which fixtures and expected results belong to which operation |

The plan's shared types live in `w2m-core`; the compiler lives in `w2m-files`. The engine depends on the first and never on the second.

**Encoding.** A compact binary encoding through Rust's standard serialization framework, inside a small versioned envelope. A zero-copy format is considered only if plan loading misses its budget. `web2mcp plan show` prints any plan as readable text for review and comparison.

## Lint rules

Rules have stable codes so they can be cited, silenced with a reason where allowed, and tested.

| Group | Rules | Level |
| --- | --- | --- |
| Determinism | No fixed waits; every wait names a condition; no unordered iteration over source rows | Error |
| Writes | Every write operation has a key or is marked irreversible; every write binding has a read-back; every `for_each` over writes has a checkpoint; write pipelines declare a record limit | Error |
| Locating | Every UI field has at least one locator and a fingerprint; no locator relies on position alone | Error |
| Contracts | Every endpoint has a response schema; every operation has an output schema | Error |
| Scope | No surface, upload, download, or destination outside those declared | Error |
| Secrets | No value that looks like a credential in any file; credentials are vault references only | Error |
| Layering | Pipelines name operations only; operations name screens and endpoints only through bindings | Error |
| Tests | Every operation has a fixture-based test; fixtures carry the scrubbed marker | Error for publish, warning for drafts |
| Naming | Tool names follow the naming scheme; no two tools differ only by a suffix | Error |
| Descriptions | Starts with what the tool does; says when to prefer another tool; within length limits | Warning |
| Data | Free-text outputs are marked untrusted; fields that hold personal or restricted data carry a sensitivity label | Warning, error on servers |

## Drafts and published versions

| | Draft | Published |
| --- | --- | --- |
| Compiled | In memory, on demand, by `lint`, `try`, and `test` | Once, in a clean deterministic mode, by `publish` |
| Test rules | Warnings | Errors |
| Marked | As a draft; refused by normal callers | Signed with the plan hash |
| Stored | Nowhere | The plan, the source files, and the gate evidence, together and immutable |

On publish the compiler also writes a review summary ordered by risk, which is what the reviewing person sees first ([section 51](51-defending-against-a-manipulated-agent.md)). A published version takes over in stages ([section 52](52-when-the-engine-itself-fails.md)).

The source files are always stored beside the plan. A plan can therefore always be rebuilt, compared, or migrated.

## Compatibility

- **Plan format.** The engine loads the current and the previous plan format. `web2mcp migrate` recompiles stored sources for a newer engine.
- **File schema.** Each target states its schema version. A newer compiler reads older versions and can rewrite files forward.
- **Tool contracts.** `web2mcp diff` compares two versions of a target and classifies each change to a tool: compatible, such as a new optional input, or breaking, such as a removed input, a changed type, or a raised effect class. A breaking change requires a new major number in the tool's name, as the gap review's tool-versioning rule requires ([section 32](32-components-added-by-the-gap-review.md)).

## Speed targets

Compiling a target with 500 operations: under 2 seconds. Loading its plan: under 20 ms. Checking a single edited file for an agent's `lint` call: under 100 ms. All are targets until measured.

## Open points

- The exact encoding crate, chosen in Phase 1 by measuring load time and size.
- Whether lint rules should be extensible by plug-ins or stay built in.
- How much of the description lint can be automatic before the agent-usability gate has to judge it.
