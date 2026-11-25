# 206. The compiler and the compiled target

Target files are written for people and agents to read. The engine never reads them at call time. A compiler turns a target's files into one checked, hashed compiled target, and the engine runs only compiled targets. This section defines the compiler's stages, what the compiled target contains, the lint rules, and how versions stay compatible. Built today: the compiler and the compiled target; the same compiled target bytes on three operating systems is not yet shown ([section 704](../7-project/704-readiness.md)).

## Why compile

- **Speed.** Nothing is parsed, resolved, or type-checked during a call.
- **Safety.** Every mistake a file can contain is found before publishing, all at once, with a file and line.
- **Reproducibility.** The same files and the same compiler version give a byte-identical compiled target, so its hash identifies exactly what ran.

## Stages

| # | Stage | What it does | Fails on |
| --- | --- | --- | --- |
| 1 | Parse | Reads every YAML file, keeping the file, line, and column of each value | Malformed YAML |
| 2 | Validate | Checks each file against its JSON Schema and the target's schema version | Unknown or missing fields |
| 3 | Resolve | Links operations to screens, endpoints, and field types; pipelines to operations and sources; placeholders to inputs | A name that does not exist; a name defined twice |
| 4 | Type-check | Checks every expression against the input and output schemas and field types | A type error in any expression |
| 5 | Lower | Turns each binding's verb into the fixed list of [steps](../../reference/steps-snapshots-and-diffs.md) its driver will run, through the lowering its pack registered for the mode and verb | A mode or verb nobody registered |
| 6 | Analyze | Works out each operation's commit points from the lowered steps, each pipeline's effect class, and each operation's permission manifest | A write with no key that is not marked irreversible; a declared effect lower than the derived one |
| 7 | Lint | Applies the rules below, the kernel's and those packs register | Any rule marked error |
| 8 | Describe | Generates each tool's name, description, input and output schemas, and annotations | A tool that would be indistinguishable from another |
| 9 | Emit | Writes the compiled target and its hash | Nothing |

All problems from stages 2 to 8 are reported together, each with a code, a plain sentence, the place, and a suggested fix, and as JSON for agents.

**Commit points are counted, not assumed.** The ledger keeps one row per save in the system ([section 208](208-runs-the-ledger-and-resume.md)). Lowering runs before analysis, so the compiler counts the steps marked `commits` in each write binding's lowered steps, and the count can never disagree with what runs: in the web pack, actions marked `commits` in a UI binding; in the HTTP pack, write requests. An operation whose bindings disagree on the count, or whose count differs from what its author declared, does not compile. A write saves at least once and a read never: an operation whose effect is `write` or `irreversible` has at least one commit point in each binding, and one whose effect is `read` has none. Each pack states, once for each kind of step its lowering emits, whether a step of that kind never saves, always saves, or saves when the pack's own files say what it refers to does; a lowered step that is marked otherwise does not compile, and a kind stated as always saving can therefore never run under a read. A lowering also states the least effect a binding has by what it does, so that a verb that deletes or sends is `irreversible` whatever the operation's file declares. The compiled target's own check holds the same rules when a compiled target is loaded, so a compiled target the compiler did not make cannot run a write as a read, unrecorded, or unread ([decision 0153](../../decisions/0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md)).

**Every write can be found again.** A write operation must declare an identity strategy that is possible in each of its bindings ([section 209](209-record-identity-and-dependent-records.md)). Steps that use another step's result must not form a cycle.

**Templates are typed.** Every placeholder in a request template is given a position and encoded for it, so an input cannot change where a request goes ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)).

**Numbers are decimals.** An expression has no float type. The type checker refuses a double literal, a double-typed name, and any function that returns one. A decimal is added, compared, and rounded only through the expression library's host functions, and text becomes a decimal only through `parse_decimal` with the field type's places and rounding ([`a2m-expr`](../../architecture/crates/a2m-expr.md)).

**Effect classes can only go up.** A pipeline's class is the highest of its operations'. An author can declare a higher class than the compiler derives, never a lower one.

## What the compiled target contains

| Part | Contents |
| --- | --- |
| Header | Compiled target format version, minimum engine version, target, version, compiler version, hash of the source files |
| Target | What holds for the target as a whole: its title, modules, default environment, locale and time zone, capture level, whether it allows one session per account, its drift risk, and the reference to its authorization record |
| Surfaces | Each surface with its registered surface kind; how they share sessions; where a surface stands to an application that owns its data |
| Environments | Each environment's kind and address overrides, its declared test records, and its approval rule |
| Sign-in | For each group of surfaces that share a sign-in: the mode, the auth provider, the probe, the expiry signals, declared timeouts, the provider's settings, and credential references. Never a credential |
| Field types | Normalizers and comparison rules, as host-function calls; beside each, as data, how the system shows a value: decimal mark, step, pattern, zone offset, case and whitespace rules |
| Pack sections | What each pack's file kinds compiled to, such as the web pack's screens or the HTTP pack's endpoints, as one opaque payload per kind |
| Operations | Inputs, outputs, key, effect, commit-point count, example inputs, declared decision points, and one step list per binding, in the order the bindings are tried |
| Pipelines | The step graph, checkpoints, limits, success checks |
| Expressions | Every expression, already parsed and type-checked |
| Tools | Names, descriptions, schemas, annotations, as the front doors will list them |
| Manifests | Per operation: surfaces, uploads, downloads, and destinations it may touch |
| Tests | Which fixtures and expected results belong to which operation |

The compiled target's types live in `a2m-core`, with the check of a compiled target's hash; the compiler in `a2m-files` is the only code that makes a compiled target. The engine depends on the first and never on the second. Knowledge of one kind of system, such as locators or request templates, is held in a pack section as bytes that the kernel stores and hashes and never decodes; the pack's driver decodes its own.

**Binding order.** An operation's bindings are tried in the order its files declare. Where an operation states none, the compiler sets it from each mode's rank in its provider's manifest ([section 203](203-the-provider-contract.md)), so the engine walks the compiled target's order and never knows which mode is cheap. An operation states an order with `binding_order: listed`, which keeps the file's. The compiler takes a mode's rank from the compiler set (`CompilerSetBuilder::mode_rank`), which the provider registry fills from each manifest; a mode given none has the manifest's default of 100, so a set given no ranks orders as the file does ([decision 0337](../../decisions/0337-a-description-says-who-wrote-it-a-drafter-reports-what-it-left-undrafted-and-bindings-are-ordered-by-rank.md)). The provider registry's call is not built yet.

**What packs register.** A pack registers its file kinds, the lowering for each mode and verb it serves, its lint rules, and its drafting rules ([section 202](202-kernel-and-packs.md)). A file kind has one owning pack, and registration is keyed by the file kind and the surface kind, so two packs can each register `screens/` for their own surfaces. Another pack may declare that it reads a kind; the compiler then hands that pack's lowering the validated entries as data, never the owning pack's code. If the owning pack is not enabled, a binding that needs its kind does not compile ([decision 0062](../../decisions/0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md)).

**Encoding.** Canonical JSON, with no spaces, every object's keys sorted, and whole numbers only, after one first line that names the compiled target format ([decision 0069](../../decisions/0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md)). The compiled target's hash is the digest of all its bytes. A binary format is considered only if compiled target loading misses its budget, and would be the next format number. `anything2mcp target show` prints any compiled target as readable text for review and comparison.

## Lint rules

Rules have stable codes so they can be cited, silenced with a reason where allowed, and tested.

| Group | Rules | Level |
| --- | --- | --- |
| Determinism | No fixed waits; every wait names a condition; no unordered iteration over source rows | Error |
| Writes | Every write operation has a key or is marked irreversible; every write binding has a read-back; every `for_each` over writes has a checkpoint; write pipelines declare a record limit | Error |
| Locating, registered by the web pack | Every UI field has at least one locator and a fingerprint; no locator relies on position alone | Error |
| Contracts | Every endpoint, in the HTTP pack's catalog, has a response schema; every operation has an output schema | Error |
| Scope | No surface, upload, download, or destination outside those declared. Built in part: a binding's surface is one the target declares and of a kind its mode's pack serves, and each destination an operation declares is one exact address, with no wildcard, range, path, or user information, and each upload and download one name. Not built: checking a destination against the origins a person registered for the target, for which no file has a place yet; until then the review summary and the publish gate are the check ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)) | Error |
| Secrets | No value that looks like a credential in any file, a fixture's recorded files included; credentials are vault references only. No diagnostic quotes a value the rule knows | Error; a warning for a value under a key that names a token, a cookie, an authorization, or a PIN and may be the name of one |
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
| Marked | As a draft, by the draft version, which no published compiled target carries: a draft compiled under any other version is refused ([decision 0161](../../decisions/0161-the-plan-and-the-engine-after-the-packs-were-built.md)); refused by normal callers | Signed with the compiled target hash |
| Stored | Nowhere | The compiled target, the source files, and the gate evidence, together and immutable |

On publish the compiler also writes a review summary ordered by risk, which is what the reviewing person sees first ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)). A published version takes over in stages ([section 224](224-when-the-engine-itself-fails.md)).

The source files are always stored beside the compiled target. A compiled target can therefore always be rebuilt, compared, or migrated.

## Compatibility

- **Compiled target format.** The engine loads the current and the previous compiled target format. A change to the compiled target's shape raises the format. `anything2mcp migrate` recompiles stored sources for a newer engine. A compiled target kept from each release must still load, with its hash, in every later build ([decision 0997](../../decisions/0997-what-a-release-publishes-is-kept-and-every-later-build-is-checked-against-it.md); package YL4); today only format 1 exists, and nothing yet holds a shape change to a new format.
- **File schema.** Each target states its schema version: one number for every file kind the program carries, the kernel's and every pack's. Removing or renaming a field or a word, or changing a field's meaning, raises it; a new optional field, file kind, or word does not. A compiler reads its own number and the one before, and `migrate` rewrites files forward; a new refusal of a file that compiled before arrives as a warning first ([decision 0996](../../decisions/0996-one-schema-number-covers-every-file-kind-the-program-carries-and-says-what-raises-it.md)). Today the compiler reads number 1, the only one.
- **Tool contracts.** A tool's name never carries a version: the version is in its `_meta`, and its digest says exactly what it is. `anything2mcp diff` compares two versions of a target and classifies each change to a tool by a closed list: breaking, such as a removed input, a new required input, a changed type, a removed output, or a raised effect class, or else compatible, such as a new optional input. The review a person publishes from lists each breaking change first; it does not refuse the publish ([decision 0999](../../decisions/0999-a-tools-name-carries-no-version-and-a-breaking-change-is-named-at-publish.md)). Neither `diff` nor that part of the review is built (package YL1).

## Speed targets

Compiling a target with 500 operations: under 2 seconds. Loading its compiled target: under 20 ms. Checking a single edited file for an agent's `lint` call: under 100 ms. All are targets until measured.

## Open points

- Whether loading a compiled target as canonical JSON meets the budget below, measured in Phase 1 on a compiled target of real size.
- How a plug-in registers file kinds and lint rules across a process boundary: as data at start, most likely, with its lowerings answered over the plug-in protocol.
- Whether a lint rule may be silenced, and where the reason is written.
- `Verb` is a checked name ([decision 0068](../../decisions/0068-values-names-and-identifiers-as-the-core-crate-settles-them.md)); a verb that no lowering registers does not compile.
- How much of the description lint can be automatic before the agent-usability gate has to judge it.
