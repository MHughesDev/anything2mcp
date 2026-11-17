# `a2m-files`

What a target is: the target files' schemas, the loader that reads them with file and line, the linter, the compiler that turns them into a hashed `Plan`, and the drafter that writes draft files from findings. It is neutral. Every file kind, verb lowering, lint rule, and drafting rule that belongs to one kind of system arrives from a pack through the registration traits defined here.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 3, Stores |
| May depend on | Layers 1 and 2 |
| Depends on | `a2m-core`, `a2m-expr`. Outside, each to be verified and recorded: a YAML parser that keeps the place of every value; a JSON Schema validator; `serde_json` for schemas and diagnostics. The plan's encoding and hash come from `a2m-core` |
| Built in | Roadmap package B4, schemas and the loader for the kernel's file kinds; package 1.3, the compiler for reads and the registration traits; 3.4, field types; 3.8, identity strategies; 3.9, pipelines and sources; E4, the drafter |
| Carries out | Spec sections [7](../../spec/07-files-and-formats.md), [46](../../spec/46-target-compiler-and-plan.md), [50](../../spec/50-record-identity-and-dependent-records.md) (what the compiler checks), [51](../../spec/51-defending-against-a-manipulated-agent.md) (typed templates, the review summary), [55](../../spec/55-kernel-and-packs.md) (what a pack registers), [67](../../spec/67-the-scout-and-who-does-what.md) (the drafter); the [file formats](../../reference/file-formats.md) and the [steps](../../reference/steps-snapshots-and-diffs.md#steps); [decision 0036](../../decisions/0036-target-files-are-compiled-into-a-hashed-plan-before-anything-runs.md) |

## Owns

- The schemas of the kernel's file kinds: `target.yaml`, `auth.yaml`, `types.yaml`, `operations/`, `pipelines/`, `sources/`, `tests/`, and the fixture marker.
- The loader: reading a target folder, and only that folder, into located documents.
- The nine compiler stages, and every diagnostic they produce, each with a code, a sentence, a place, and a fix.
- The kernel's lint rules, and the running of every registered one.
- Producing the `Plan`. Its types, its encoding, and the check of its hash are `a2m-core`'s; this crate is the only code that makes one.
- The registration traits a pack implements, and the `CompilerSet` that holds what every enabled pack registered.
- The drafter, and the rule for what a draft may and may not contain.

It must never: know a mode, a locator, an endpoint, or any other knowledge of one kind of system; read outside the target folder; write a file the developer has changed; give a draft any level but `reconstructed`; run, store, or publish a plan (`a2m-actions` stores a published one through `a2m-ledger`); define a plan type, or a variant of one the engine would have to learn from this crate; be depended on by `a2m-engine`; call a model.

## Interface

### Loading and diagnostics

```rust
/// A target folder, read. Every value keeps its place.
pub struct Sources { pub root: TargetRoot, pub files: Vec<SourceFile>, pub digest: Digest }
pub struct SourceFile { pub path: RelPath, pub kind: FileKindName, pub digest: Digest, pub docs: Vec<Located<Doc>> }
pub struct Place { pub file: RelPath, pub line: u32, pub column: u32 }
pub struct Located<T> { pub value: T, pub at: Place }

/// Reads only inside `root`; refuses a link that leaves it, an unknown folder, and a file no kind claims.
pub fn load(root: &TargetRoot, set: &CompilerSet) -> Result<Sources, Diagnostics>;

pub struct Diagnostic {
    pub code: LintCode,             // stable; `kernel.` or the registering pack's prefix
    pub level: Level,               // error or warning; drafts turn some errors into warnings
    pub message: String,            // one plain sentence
    pub at: Place,
    pub fix: Option<String>,
    pub stage: Stage,
}
pub struct Diagnostics(Vec<Diagnostic>);   // sorted by file, line, column, code; serializes to JSON for agents
```

### Compiling

```rust
pub enum CompileMode { Draft, Publish }     // Publish: test rules are errors; no `Dyn` expression; clean and deterministic

pub fn compile(sources: &Sources, set: &CompilerSet, mode: CompileMode) -> Result<Compiled, Diagnostics>;
pub struct Compiled { pub plan: Plan, pub warnings: Diagnostics, pub review: ReviewSummary }

/// One edited file, for an agent's `draft.lint`. Reuses everything unchanged, by file digest.
pub fn lint_one(previous: &Sources, edited: SourceFile, set: &CompilerSet) -> Diagnostics;
```

Stages 1 to 9 are those of [section 46](../../spec/46-target-compiler-and-plan.md). This crate adds what a builder needs to know about each.

| Stage | Kernel work | Pack work, through `CompilerSet` |
| --- | --- | --- |
| 1 Parse | YAML to located documents; a duplicate key is an error | None |
| 2 Validate | Kernel schemas, and the schema version | Schemas of the pack's file kinds, and of the fields its bindings add |
| 3 Resolve | Names across files; placeholders to inputs; every name defined once | `FileKind::names` says which names a file of the kind defines, so the kernel can resolve to them without reading them |
| 4 Type-check | Every expression, through `a2m-expr`, with the right `TypeEnv` for its place | `FileKind::expressions` lists the expressions inside the pack's own files |
| 5 Analyze | Commit points counted from the lowered steps' `commits` flag; effect classes raised, never lowered; identity strategies; permission manifests; no cycles | `Lowering::identity` says whether each identity strategy is possible for a binding |
| 6 Lint | Kernel rules | Registered `LintRule`s |
| 7 Lower | Calls the lowering registered for the binding's mode and verb; a mode or verb nobody registered stops the compile | `Lowering::lower` |
| 8 Describe | Tool names, descriptions, schemas, annotations; two tools that cannot be told apart are an error | None |
| 9 Emit | The canonical encoding and the hash, both through `a2m-core` | None |

Stage 7 runs before stage 5 counts commit points, although section 46 lists them the other way: the count is taken from the lowered steps, so it can never disagree with what runs.

### The plan

`Plan`, `Step`, `PackSection`, `PackPayload`, `PlanExpr`, and the rest of the plan's types are defined in `a2m-core`, as data, with the check of a plan's hash; see the [`a2m-core` design](a2m-core.md). This crate is the only code that produces a `Plan`, and what it adds is how each part is filled:

| Plan part | Filled in stage | How |
| --- | --- | --- |
| `header` | 9 | The compiler's version, the target, and the digest of the sorted list of source paths and file digests. No timestamp, no absolute path |
| `surfaces`, `field_types` | 3 | Field types carry their normalizer as a `PlanExpr`, built from the field type through `a2m-expr` |
| `expressions` | 4 | Every expression once, as canonical text with its type; steps and bindings refer to them by index |
| `operations`, `bindings` | 5 and 7 | Bindings keep declared order. A binding's `act`, `confirm`, and `read_back` are the lowering's three step lists, unchanged |
| `packs` | 3 | Each registered file kind's `compile` returns one `PackSection`; the kernel stores and hashes it and never decodes it |
| `tools`, `manifests` | 8 and 5 | As section 46 says |

Encoding and hashing are `a2m-core`'s, so the compiler and the engine cannot disagree on either.

### What a pack registers

```rust
/// A file kind the pack owns, such as `screens/`. One owner per kind; a second is refused.
pub trait FileKind: Send + Sync {
    fn name(&self) -> FileKindName;                 // checked name, such as `screen`
    fn folder(&self) -> &str;                       // `screens/`
    fn schema(&self) -> &JsonSchema;                // published with the kernel's
    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>>;   // what a document defines, for stage 3
    fn expressions(&self, doc: &Located<Doc>) -> Vec<ExprSite>;         // what to type-check in stage 4
    fn compile(&self, docs: &[Located<Doc>], cx: &KindContext<'_>) -> Result<PackSection, Diagnostics>;
}

/// How one mode lowers one or more verbs into steps.
pub trait Lowering: Send + Sync {
    fn mode(&self) -> Mode;
    fn verbs(&self) -> &[Verb];
    fn step_kinds(&self) -> &[StepKind];            // every kind its steps may carry; the driver must know each
    fn binding_schema(&self) -> &JsonSchema;        // the fields this mode adds to a binding
    fn identity(&self, binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible;
    fn lower(&self, binding: &ResolvedBinding<'_>, cx: &LowerContext<'_>) -> Result<Lowered, Diagnostics>;
}
pub struct Lowered { pub act: Vec<Step>, pub confirm: Vec<Step>, pub read_back: Vec<Step> }

/// A lint rule with a stable code. Sees resolved files; cannot change them.
pub trait LintRule: Send + Sync {
    fn code(&self) -> LintCode;
    fn level(&self, mode: CompileMode) -> Level;
    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic>;
}

/// From one kind of finding to one kind of file. Mechanical: the same finding gives the same bytes.
pub trait DraftingRule: Send + Sync {
    fn finding_kind(&self) -> FindingKind;
    fn file_kind(&self) -> FileKindName;
    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure>;
}

/// Everything every enabled pack registered. Built by `a2m-provider`; passed by `a2m-actions`.
pub struct CompilerSet { /* file kinds, lowerings by (mode, verb), lint rules, drafting rules */ }
impl CompilerSet {
    pub fn builder() -> CompilerSetBuilder;         // refuses a kind, a (mode, verb), a lint code, or a
}                                                   // (finding kind, file kind) registered twice
```

`ResolvedBinding`, `LowerContext`, `KindContext`, `ResolvedView`, and `DraftContext` are read-only views. A lowering sees its own binding, the operation's inputs and key, the field types, and its own pack's compiled sections; it cannot see another pack's. It makes expressions only through the context, which type-checks them and returns an `ExprRef`.

### The drafter

```rust
pub struct Drafter<'a> { set: &'a CompilerSet, root: &'a TargetRoot }
impl Drafter<'_> {
    /// Writes a draft for each finding a registered rule covers, then lints and compiles them.
    pub fn draft(&self, findings: &[Finding]) -> Result<DraftReport, Failure>;
}
pub struct DraftReport { pub written: Vec<RelPath>, pub kept_failing: Vec<(RelPath, Diagnostics)>,
                         pub open_questions: Vec<OpenQuestion>, pub not_covered: Vec<FindingId> }
```

- **What it writes.** Ordinary target files, in YAML, at `reconstructed` once they compile and pass lint. Each names the findings it was made from and carries a mechanical, provisional name from the surface's own words.
- **What it leaves out.** Any unit, key, or read-back rule the system did not state. Where one is needed, the draft holds an `open_question` marker in its place, which the compiler refuses in every mode, and the question goes to the unknowns ledger through the caller.
- **What it never overwrites.** A file is rewritten only if the drafter wrote it and its digest is still the one the drafter recorded. Otherwise the draft is reported as a conflict and not written.
- **Where it stops.** It drafts no write operation, ever. The scout's other stages are `a2m-actions`' work.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-actions` | `load`, `compile`, `lint_one` for `draft.lint`, `draft.try`, `test.run`, `version.publish`, `version.diff`, `plan.show`; the `Drafter` for `draft.write` and the scout |
| `a2m-provider` | `CompilerSetBuilder`, to gather what packs and plug-ins register |
| Packs | The four registration traits and their read-only views. They build `Step` and `PackPayload` from `a2m-core`; a driver later decodes its own payloads without this crate |

The engine never uses this crate; the audit refuses the dependency. It reads plans through `Control`, as `a2m-core` types.

It calls `a2m-core`, for the plan types and their encoding, and `a2m-expr`, and the registered traits.

## State

None that persists. It reads the developer's target folder and the drafter writes into it. The published plan is stored by `a2m-actions` through `a2m-ledger`. `lint_one` keeps parsed files in memory, keyed by digest, for the life of the engine.

## Failures

A compile reports `Diagnostics`, not a failure; the action that called it turns them into `input` with `lint_failed`. A plan that fails its hash check fails in `a2m-core`, not here.

| Class | Code | When |
| --- | --- | --- |
| `input` | `lint_failed` | Any error diagnostic |
| `input` | `path_not_allowed` | The loader met a link or a reference outside the target folder |
| `input` | `draft_conflict` | The drafter would overwrite a file the developer changed |
| `input` | `registration_conflict` | Two packs registered the same kind, mode and verb, lint code, or drafting pair |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| The same sources and compiler give byte-identical plans, on every machine | Compile the `example` target twice on each of three operating systems and compare hashes; package 1.3 |
| A mode, verb, step kind, or file kind nobody registered does not compile | Compiler tests with planted unknowns |
| The commit-point count equals the number of `commits` steps in every write binding | A property test over lowered operations |
| A declared effect lower than the derived one is refused | A compiler test |
| The loader never reads outside the target folder | Tests with a link out, a `..` reference, and an absolute path |
| Every lint rule has a passing and a failing test | A test that walks `CompilerSet` and fails on a code with none; Phase 1, group A |
| A draft never carries a unit, key, or read-back the finding did not state | A test per drafting rule, with findings that lack each; package E4 |
| What `compile` emits decodes, through `a2m-core`, to an equal `Plan` with the same hash | A round-trip test over the `example` target |

## Build order

1. Verify the YAML parser for places, and the schema validator. Kernel schemas, `load`, `Diagnostics`. Package B4.
2. Nothing here for package 1.1: its hand-written plan uses the plan types in `a2m-core`.
3. The registration traits and `CompilerSet`; stages 1 to 9 for read operations, with the web pack's screen kind and `read` lowering as the first registered. Package 1.3.
4. Field types compiled to normalizers. Package 3.4.
5. Identity strategies, commit-point counting for writes, and the write lint rules. Packages 3.3 and 3.8.
6. Pipelines and sources. Package 3.9.
7. The drafter and the first drafting rules. Package E4.

## Open points

- `Verb` is a checked name in `a2m-core` for now; whether it should stay a closed list is open there. Either way, a verb that no lowering registers does not compile.
- The read-only views a lowering sees: how much of the operation and the field types they expose, settled with the first lowering in package 1.3.
- How a plug-in registers file kinds and lint rules across a process boundary: as data at start, most likely, with its lowerings answered over the plug-in protocol.
- Whether a lint rule may be silenced, and where the reason is written.
- Where drafts are written inside a project, and whether they are kept apart from files a person wrote.
