# `a2m-files`

What a target is: the target files' schemas, the loader that reads them with file and line, the linter, the compiler that turns them into a hashed `Plan`, and the drafter that writes draft files from findings. It is neutral. Every file kind, verb lowering, lint rule, and drafting rule that belongs to one kind of system arrives from a pack through the registration traits defined here.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 3, Stores |
| May depend on | Layers 1 and 2 |
| Depends on | `a2m-core`, `a2m-expr`. Outside: `saphyr-parser`, an event parser for YAML that gives the place of every value ([decision 0078](../../decisions/0078-target-files-are-read-by-an-event-parser-into-our-own-located-tree-and-checked-by-our-own-schema-subset.md)); `serde` and `serde_json` for schemas and diagnostics. Schemas are checked by this crate's own code. The plan's encoding and hash come from `a2m-core` |
| Built in | Roadmap package B4, schemas and the loader for the kernel's file kinds; package 1.3, the compiler for reads and the registration traits; 3.4, field types; 3.8, identity strategies; 3.9, pipelines and sources; E4, the drafter |
| Carries out | Spec sections [205](../../spec/2-kernel/205-target-files.md), [206](../../spec/2-kernel/206-the-compiler-and-the-plan.md), [209](../../spec/2-kernel/209-record-identity-and-dependent-records.md) (what the compiler checks), [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) (typed templates, the review summary), [202](../../spec/2-kernel/202-kernel-and-packs.md) (what a pack registers), [402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md) (the drafter); the [file formats](../../reference/file-formats.md) and the [steps](../../reference/steps-snapshots-and-diffs.md#steps); [decision 0036](../../decisions/0036-target-files-are-compiled-into-a-hashed-plan-before-anything-runs.md) |

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

The interface below is the code's, settled in [decision 0079](../../decisions/0079-the-compilers-registration-points-and-the-file-formats-as-the-code-settles-them.md). Everything named is exported from the crate root.

### Loading and diagnostics

```rust
/// A path inside a target folder: names joined by `/` on every operating system; never `..`, a drive, or a root.
pub struct RelPath;            // new(&str) -> Result<_, Failure>, as_str, parts, folder, file_name, stem, under(&Path) -> PathBuf, join
pub struct Place { pub file: RelPath, pub line: u32, pub column: u32 }   // both from 1
pub struct Located<T> { pub value: T, pub at: Place }
pub enum Doc { Null, Bool(bool), Int(i64), Text(String), List(Vec<Located<Doc>>), Map(Vec<Entry>) }   // no float
pub struct Entry { pub key: Located<String>, pub value: Located<Doc> }
// On Located<Doc>: get(key), entries(), items(), as_text(), as_int(), as_bool(), text(key), to_json()

pub struct TargetRoot;         // new(path), path()
pub struct SourceFile { pub path: RelPath, pub kind: FileKindName, pub digest: Digest, pub docs: Vec<Located<Doc>> }
pub struct Sources { pub root: TargetRoot, pub files: Vec<SourceFile>, pub digest: Digest }
// Sources: from_files(root, files), file(path), of_kind(kind), docs_of(kind). SourceFile: read(path, bytes, set).

/// Reads only inside `root`; refuses a link that leaves it, a link to a folder, a file no kind claims, and a file
/// over its size limit. Names that start with a dot are passed over.
pub fn load(root: &TargetRoot, set: &CompilerSet) -> Result<Sources, Diagnostics>;
pub const LARGEST_TARGET_FILE: usize;     // 1 MiB, for a YAML file
pub const LARGEST_RECORDED_FILE: usize;   // 16 MiB, for a fixture's recorded file
/// The same from memory, such as the sources stored beside a published plan.
pub fn load_files(root: &TargetRoot, files: &[(RelPath, Vec<u8>)], set: &CompilerSet) -> Result<Sources, Diagnostics>;

pub struct Diagnostic {
    pub code: LintCode,             // stable; `kernel.` or the registering pack's prefix
    pub level: Level,               // Error or Warning; drafts turn the test rules into warnings
    pub message: String,            // one plain sentence
    pub at: Place,
    pub fix: String,                // always given
    pub stage: Stage,               // Parse, Validate, Resolve, TypeCheck, Lower, Analyze, Lint, Describe, Emit
}
pub struct Diagnostics;            // sorted by file, line, column, code; to_json() for agents; From<Diagnostics> for Failure
```

A file's digest is taken of its text with every line ending a line feed and no byte-order mark, so a checkout with other line endings compiles to the same plan; a carriage return that stands alone is a line ending too. A fixture's recorded files are digested byte for byte.

**What the loader bounds.** A folder is read once, where it is: a link to a folder is refused as `kernel.path_outside_target`, whether it leads out of the target or back into it, so no link can have the tree walked again or without end. A link to a file inside the folder is read as that file. No more than one byte past a file's limit is read, and a file over it is `kernel.file_too_large`.

`Diagnostics` converts to one `Failure`: class `input`, code `lint_failed`, or `path_not_allowed` when the first error is a link that leaves the folder. The kernel's codes are the constants in the module `codes`, and `codes::KERNEL_CODES` lists each with what its rule requires.

### Schemas

```rust
/// A closed subset of JSON Schema, checked against located documents and published as JSON Schema.
pub struct JsonSchema;
// Constructors: any, bool, int, int_between(min, max), text, scalar, decimal, name, words(&[&str]), expression,
//   control(), field_type(), surface()      -- references the kernel resolves
//   list(item), non_empty_list(item), map_of(value), either(vec),
//   object().required(key, schema).optional(key, schema)   -- a key it does not list is refused
// keys(), to_json(), check(&Located<Doc>) -> Checked { diagnostics, references }
// check_values(&Values), check_value(&Value), check_json(&serde_json::Value) -> Result<(), Misfits>

/// One place where a run-time value does not fit: its path (`lines[2].quantity`; empty for the root), the code a file
/// that broke the same rule would get, what is wrong, and a fix. The message never holds the value.
pub struct Misfit { pub path: String, pub code: LintCode, pub message: String, pub fix: String }
/// At least one misfit. Converts to `Failure`: `input`, `schema_invalid`, with the first path in the message.
pub struct Misfits;   // as_slice(), paths(), Display, Error
```

An `open_question` left by the drafter is refused wherever it stands in a file, whatever the schema says, at any depth inside a field that takes any value.

**One checker, three kinds of value.** A located document, a `Value`, and JSON are walked by the same rules, so an action's input is judged as a target file is, in the same words. `to_json` gives the standard JSON Schema document of one schema, which is what a front door publishes for a tool. Three things differ for a run-time value. No message quotes the value, only its kind and its path. An open question means nothing there. A reference is taken as text, since only the compiler can resolve it. In JSON a number with a fraction fits only where any value does, and a truth value is never one of a list of words, as the published schema says. A map's members are walked in order of name, so the order of the misfits does not depend on how the JSON library was built.

### Compiling

```rust
pub enum CompileMode { Draft, Publish }     // Publish: test rules are errors

/// `version` is what the plan is published as; a draft passes `DRAFT_VERSION`.
pub fn compile(sources: &Sources, set: &CompilerSet, mode: CompileMode, version: Version) -> Result<Compiled, Diagnostics>;
pub struct Compiled { pub plan: Plan, pub encoded: EncodedPlan, pub warnings: Diagnostics, pub review: ReviewSummary }
pub struct ReviewSummary { pub items: Vec<ReviewItem> }     // every tool, by hazard, then effect, then name

/// One edited file, for an agent's `draft.lint`: it takes the place of the file at its path, and the target is compiled as a draft.
pub fn lint_one(previous: &Sources, edited: SourceFile, set: &CompilerSet) -> Diagnostics;
```

Stages 1 to 9 are those of [section 206](../../spec/2-kernel/206-the-compiler-and-the-plan.md). This crate adds what a builder needs to know about each.

| Stage | Kernel work | Pack work, through `CompilerSet` |
| --- | --- | --- |
| 1 Parse | YAML to located documents, in `load`; a duplicate key, an anchor, an alias, and a tag are errors | None |
| 2 Validate | Kernel schemas, and the schema version; the kernel's keys of a surface and of a binding | Schemas of the pack's file kinds, of the fields its surface kind adds to a surface, and of the fields its mode adds to a binding |
| 3 Resolve | Names across files; every name defined once; controls, field types, and surfaces that schemas refer to | `FileKind::names` says which names a document defines |
| 4 Type-check | Every expression, through `a2m-expr`, with the names its place declares | `FileKind::expressions` lists the expressions inside the pack's own files |
| 5 Lower | Calls the lowering registered for the binding's mode and verb; a mode or verb nobody registered stops the compile. A lowering that declared it reads another pack's kind is handed that kind's validated entries as data | `Lowering::lower` |
| 6 Analyze | Commit points counted from the lowered steps' `commits` flag, and each step's flag held to what its pack states for the step's kind; a write saves at least once and a read never; a declared effect lower than the derived one is refused; identity strategies; permission manifests, each entry one exact address or one name; no step reads a later step | `Lowering::identity` says whether each identity strategy is possible for a binding; `Lowering::commits` says whether a kind of step saves; `Lowering::least_effect` says what a binding is by what it does |
| 7 Lint | Kernel rules | Registered `LintRule`s |
| 8 Describe | Tool names, descriptions, schemas, annotations; two tools that cannot be told apart are an error | None |
| 9 Emit | The canonical encoding and the hash, both through `a2m-core` | None |

Lowering comes before analysis so that commit points are counted from the lowered steps, and the count can never disagree with what runs.

**What the compiler holds about a write, whatever a pack says** ([decision 0153](../../decisions/0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md)). An operation whose effect is `write` or `irreversible` declares at least one commit point, and each of its bindings has exactly that many committing steps, all among the steps that do the work. An operation whose effect is `read` declares none and has no committing step in any binding. A lowered step's `commits` flag agrees with what the lowering states for the step's kind: `Never`, `Always`, or `AsDeclared`, the last for a kind that saves when the pack's own files say what it refers to does. An operation declares no less than `Lowering::least_effect` gives for each binding, which is how `irreversible` is derived: the pack states which of its verbs delete, send, or cannot be undone. A lowering names a surface of a kind its own pack registered. `Plan::check` in `a2m-core` holds the same rules for a plan the compiler did not make. What the compiler cannot hold: a lowering that states nothing leaves every kind `AsDeclared` and claims no least effect, and for it the compiler has only the lowering's own flag. Every pack's lowering is owed both statements. Every stage reports what it finds and the compiler carries on where the data is sound, so one run reports the problems of stages 2 to 8 together.

**The credential lint, and what a diagnostic never quotes.** Three things are a credential in a file, as `kernel.credential_in_file`: a value, other than a vault reference, under a key whose name holds `password`, `passwd`, `passphrase`, `secret`, `api_key` or `apikey` (a hyphen in a key reads as an underscore), `private_key`, `client_secret`, or one of the kinds of token named as such, `access_token`, `refresh_token`, `auth_token`, `id_token`, `session_token`, `bearer_token`; text in a credential's shape under any key, which is key material, an authorization header's value in any letter case, or a signed token; and the same shapes on a line of a fixture's recorded file. A value under a key one of whose parts is `token`, `cookie`, `authorization`, `pin`, or `bearer` is a warning, `kernel.credential_suspected`, unless it reads as a lowercase name or an expression's dotted name: such a key holds a secret as often as it holds the name of one, and an error would refuse a correct file. A number is judged only under a key that is the word itself, as in `pin: 4821`. Every value found this way, reported or not, is taken out of each diagnostic at its place, whichever rule or pack made the diagnostic; one that does not read as a name is taken out of every diagnostic. `[not shown]` stands where it was.

**The names an expression may read.**

| Place | Names |
| --- | --- |
| A binding, and its `precondition` | `input`, a record of the operation's inputs; `env`, with `name` and `kind` |
| A source's row field `expression`, and its `rows` | `row`, the row's cells by header text, and `source`; both without a type, so the expression must end in a type of its own |
| A pipeline step's `with`, `when`, `for_each` | `input`; `env`; `steps`, with the result of each earlier step; `source`, with `rows`, when the pipeline has a source; `row`, inside a step with `for_each` |
| A step's `expect` | The same, and `result`, the step's own result |
| `success` | `input`, `env`, `steps` with every step, `source`, and `all_records_verified` |
| A pack's own file | Whatever its `ExprSite` declares |

### Source definitions

```rust
/// The payload of the plan's `SourcePlan`: what `sources/*.yaml` compiles to, and what `a2m-sources` loads.
pub struct SourceSpec { pub versions: Vec<TemplateVersion>, pub locate: Locate, pub columns: Vec<String>, pub fields: Vec<RowField>,
                        pub key: Vec<String>, pub missing: Vec<String>, pub hidden: HiddenRule, pub options: ReaderOptions }
pub struct TemplateVersion { pub version: String, pub fingerprint: Fingerprint }
pub struct Fingerprint { pub digest: Digest, pub parts: Vec<FingerprintPart> }     // of_parts(parts), is_sound()
pub struct FingerprintPart { pub kind: PartKind, pub place: String, pub value: String }
pub enum PartKind { DateSystem, Sheet, Header, Merged, Table, TableHeader, List, Name, Macros }   // NAMES, as_str(), noun()
pub struct RowField { pub name: String, pub field_type: FieldTypeIndex, pub required: bool, pub column: Option<String> }
pub enum Locate { Document, Named(String), Sheet(String) }
pub enum HiddenRule { Reject, Include }
pub struct ReaderOptions { pub delimiter: Option<char> }
pub const OPERATION_READER: &str = "operation";
```

These are data, with the serde form `a2m-sources` reads ([decision 0087](../../decisions/0087-the-source-readers-interface-as-building-it-settled-it.md)). A row field read from a bare column is data in the spec; only a row field that is an expression is in the plan's `mapping`. A row field's type, for a pipeline's `with`, is its field type's. The fingerprint's encoding is fixed here, so the compiler refuses a digest that is not the digest of its parts.

### The plan

`Plan`, `Step`, `PackSection`, `PackPayload`, `PlanExpr`, and the rest of the plan's types are defined in `a2m-core`, as data, with the check of a plan's hash; see the [`a2m-core` design](a2m-core.md). This crate is the only code that produces a `Plan`, and what it adds is how each part is filled:

| Plan part | Filled in stage | How |
| --- | --- | --- |
| `header` | 9 | The compiler's version, the target, the version the caller gave, and the digest of the sorted list of source paths and file digests. No timestamp, no absolute path |
| `surfaces`, `environments` | 2 and 3 | A surface's address is the value of its kind's address key; the other fields its pack added are its payload. `production` is added when the target does not declare it |
| `field_types` | 3 | The built-in types, then the target's; each carries its normalizer as a `PlanExpr`, built from the field type through `a2m-expr`. A target's type of a built-in's name takes its place |
| `expressions` | 4 and 5 | Every expression once, as canonical text with its type; steps and bindings refer to them by index |
| `operations`, `bindings` | 5 and 6 | Bindings keep declared order. A binding's step lists are the lowering's, unchanged |
| `packs` | 3 | Each registered file kind's `compile` returns one payload for each surface kind that has documents; the kernel wraps it with the owner's name, stores and hashes it, and never decodes it. Sections are in order of file kind, then surface kind |
| `tools` | 8 | Operations, then pipelines. A decimal travels as text in a tool's schema. A tool's module is the longest of the target's modules that follows the target's name in its own |
| `target` | 2 and 3 | The target's own facts. `defaults.timeouts` and `defaults.limits` are not here: a lowering reads a timeout and writes it into its steps, and a pipeline that states no limit takes the default, so the engine applies none |
| `environments` | 3 | Test records as record keys, as written; the approval rule. `production` is added when the file does not declare it, and is the default environment unless the file names one |
| `surfaces` | 2 and 3 | A surface's `programs`: what it states under the fields its kind names as program fields, and, where the path field is the address key, its address in each environment, with the surface's digest. A path that is not absolute, a path with `.` or `..` in it, a program on a network share with no digest, a digest that is not one, and a digest with no program are `program_invalid`, at the value's place |
| `auth` | 3, after the operations and the pack sections | One entry per group of surfaces. A probe is a `read` operation with the inputs `probe_inputs` gives, or a name a pack's document defines on one of the entry's surfaces; anything else is `unknown_name` or `probe_invalid`. A credential that is not a vault reference is `credential_in_file`, and is never shown |
| `field_types` | 3 | The normalizer and, from the same rule, the format as data: decimal mark, step, pattern, zone offset, case and whitespace |
| `pipelines` | 3 to 7 | A source's bare columns go in its payload, a `SourceSpec`; its expressions in `mapping` |
| `tests` | 7 | One for each document under `tests/` |

Encoding and hashing are `a2m-core`'s, so the compiler and the engine cannot disagree on either.

### What a pack registers

```rust
/// A file kind the pack owns, such as `screens/`, for the surface kinds it serves. One owner per
/// file kind and surface kind; a second registration for the same pair is refused (decision 0062).
pub trait FileKind: Send + Sync {
    fn name(&self) -> FileKindName;                 // checked name, such as `screen`
    fn surface_kinds(&self) -> &[SurfaceKind];      // with `name`, the keys it is registered under
    fn folder(&self) -> &str;                       // `screens`: one name, no slash
    fn schema(&self) -> &JsonSchema;                // published with the kernel's; `surface` is declared as JsonSchema::surface()
    fn names(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>>;   // what a document defines, for stage 3
    fn expressions(&self, doc: &Located<Doc>) -> Vec<ExprSite>;         // what to type-check in stage 4
    fn compile(&self, docs: &[Located<Doc>], cx: &KindContext<'_>) -> Result<CompiledSection, Diagnostics>;
    /// Whether a document describes something that changes the system. The drafter writes none that does.
    fn writes(&self, doc: &Located<Doc>) -> bool { false }
}
pub struct DefinedName { pub name: String }
pub struct ExprSite { pub text: String, pub at: Place, pub names: Vec<DeclaredName>, pub expected: Option<ValueType> }
pub struct DeclaredName { pub name: String, pub ty: ValueType }
pub struct CompiledSection { pub format: u16, pub payload: PackPayload }

/// How one mode lowers one or more verbs into steps.
pub trait Lowering: Send + Sync {
    fn mode(&self) -> Mode;
    fn verbs(&self) -> &[Verb];
    fn default_verb(&self) -> Option<Verb> { None }      // the verb of a binding of this mode that states none
    fn reads(&self) -> &[(FileKindName, SurfaceKind)];   // kinds it reads and does not own; declared, never assumed
    fn step_kinds(&self) -> &[StepKind];            // every kind its steps may carry; the driver must know each
    fn commits(&self, kind: &StepKind) -> StepCommits { StepCommits::AsDeclared }      // stated once for the kind
    fn least_effect(&self, binding: &ResolvedBinding<'_>) -> EffectClass { EffectClass::Read }   // by what the binding does
    fn binding_schema(&self) -> &JsonSchema;        // every key of a binding but mode, verb, status, precondition
    fn identity(&self, binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible;
    fn lower(&self, binding: &ResolvedBinding<'_>, cx: &LowerContext<'_>) -> Result<Lowered, Diagnostics>;
}
pub enum Possible { Yes, No(String) }
/// Whether a step of one kind saves in the system. A lowered step marked otherwise does not compile.
pub enum StepCommits { Never, Always, AsDeclared }
pub struct Lowered {
    pub surface: SurfaceName,
    pub act: Vec<Step>, pub confirm: Vec<Step>, pub read_back: Vec<Step>, pub lookup: Vec<Step>,
    pub settle_ms: u32, pub prefer_response: bool,
}

/// A lint rule with a stable code. Sees resolved files; cannot change them.
pub trait LintRule: Send + Sync {
    fn code(&self) -> LintCode;                     // the registering pack's name, a dot, the rule
    fn level(&self, mode: CompileMode) -> Level;
    fn summary(&self) -> &str;                      // what the rule requires, in one sentence
    fn check(&self, view: &ResolvedView<'_>) -> Vec<Diagnostic>;
}

/// From one kind of finding to one kind of file. Mechanical: the same finding gives the same bytes.
pub trait DraftingRule: Send + Sync {
    fn finding_kind(&self) -> FindingKind;
    fn file_kind(&self) -> FileKindName;
    fn draft(&self, finding: &Finding, cx: &DraftContext<'_>) -> Result<Vec<DraftDoc>, Failure>;
}

/// A surface kind a pack serves, as data. `programs` says which of its fields name a program the surface's driver may start.
pub struct SurfaceKindSpec { pub kind: SurfaceKind, pub address_key: String, pub fields: JsonSchema, pub programs: Vec<ProgramField> }
impl SurfaceKindSpec {
    pub fn new(kind: SurfaceKind, address_key: &str, fields: JsonSchema) -> Self;       // starts no program
    pub fn with_program(self, path_key: &str, digest_key: Option<&str>) -> Self;
}
/// The key of a surface that holds a program's absolute path, and the key that holds its digest when the kind has one.
/// The path key may be the address key: then the surface's address in each environment is a program too.
pub struct ProgramField { pub path_key: String, pub digest_key: Option<String> }

/// Everything every enabled pack registered. Built by `a2m-provider`; passed by `a2m-actions`.
pub struct CompilerSet;
impl CompilerSet {
    pub fn builder() -> CompilerSetBuilder;
    pub fn kernel_only() -> CompilerSet;
    pub fn surface_kinds(&self) -> impl Iterator<Item = &SurfaceKind>;
    pub fn modes(&self) -> Vec<&Mode>;
    pub fn verbs(&self, mode: &Mode) -> Vec<&Verb>;
    pub fn controls(&self) -> impl Iterator<Item = &ControlName>;
    pub fn file_kinds(&self) -> impl Iterator<Item = &(FileKindName, SurfaceKind)>;
    pub fn drafting_rules(&self) -> impl Iterator<Item = &(FindingKind, FileKindName)>;
    pub fn lint_codes(&self) -> Vec<(String, String)>;                    // the kernel's and the packs', with what each requires
    pub fn schemas(&self) -> BTreeMap<String, serde_json::Value>;         // every schema, as JSON Schema
}
/// Each method names the registering pack. Each refuses, with `registration_conflict`, a key registered
/// twice, a pack named `kernel`, and a name that is the kernel's or another pack's.
impl CompilerSetBuilder {
    pub fn surface_kind(&mut self, pack: &str, spec: SurfaceKindSpec) -> Result<&mut Self, Failure>;
    pub fn control(&mut self, pack: &str, control: ControlName) -> Result<&mut Self, Failure>;
    pub fn file_kind(&mut self, pack: &str, kind: Arc<dyn FileKind>) -> Result<&mut Self, Failure>;
    pub fn lowering(&mut self, pack: &str, lowering: Arc<dyn Lowering>) -> Result<&mut Self, Failure>;
    pub fn lint_rule(&mut self, pack: &str, rule: Arc<dyn LintRule>) -> Result<&mut Self, Failure>;
    pub fn drafting_rule(&mut self, pack: &str, rule: Arc<dyn DraftingRule>) -> Result<&mut Self, Failure>;
    pub fn build(self) -> CompilerSet;
}
```

A declared read of a kind no enabled pack owns is not refused when the set is built: a binding whose mode needs it does not compile.

The views are read-only.

| View | Given to | Shows |
| --- | --- | --- |
| `TargetView` | Every view, through `target()` | `name()`, `surfaces()`, `surface(name)`, `field_type(name)`, and `default_timeout_ms(name)`, a timeout from `defaults.timeouts` for a lowering to give a step that states none |
| `KindContext` | `FileKind::compile` | `target()`, `surface_kind()`, `surface_of(doc)`, and `expr(&ExprSite) -> Result<ExprIndex, Diagnostic>` |
| `ResolvedBinding` | `Lowering::identity` and `lower` | `operation`, `index`, `mode`, `verb`, `status`, `doc`, `field(key)` |
| `OperationView` | Through a binding or a `ResolvedView` | `name()`, `effect()`, `hazard()`, `strategy()`, `key()`, `inputs()`, `input(name)`, `outputs()`, `doc()` |
| `LowerContext` | `Lowering::lower` | `target()`, `operation()`, `expr(doc, expected)`, `expr_at(text, place, expected)`, `expr_type(index)`, `section(kind, surface_kind)` for the pack's own compiled sections, `entries(kind, surface_kind)` and `section_index(kind, surface_kind)` for a kind it owns or declared in `reads` |
| `ResolvedView` | `LintRule::check` | `target()`, `mode()`, `sources()`, `operations()`, `entries(kind, surface_kind)` |
| `DraftContext` | `DraftingRule::draft` | `target()`, `existing()`, `open_question(text)` |

A lowering sees its own binding, the operation's inputs and key, the field types, and its own pack's compiled sections. Of another pack's kinds it sees only those it declared in `reads`, as validated entries, never as that pack's compiled section or code. It makes expressions only through the context, which type-checks them and returns the plan's `ExprIndex`.

### The drafter

```rust
pub struct Drafter<'a>;            // new(set: &'a CompilerSet, root: &'a TargetRoot)
impl Drafter<'_> {
    /// Writes a draft for each finding a registered rule covers, then lints and compiles the folder.
    pub fn draft(&self, findings: &[Finding]) -> Result<DraftReport, Failure>;
}
pub struct DraftDoc { pub path: RelPath, pub body: DraftValue }
pub enum DraftValue { Null, Bool(bool), Int(i64), Text(String), List(Vec<DraftValue>), Map(Vec<(String, DraftValue)>), OpenQuestion(String) }
pub struct DraftReport {
    pub written: Vec<RelPath>, pub conflicts: Vec<RelPath>, pub refused: Vec<(RelPath, String)>,
    pub kept_failing: Vec<(RelPath, Diagnostics)>, pub open_questions: Vec<OpenQuestion>, pub not_covered: Vec<Digest>,
}
pub struct OpenQuestion { pub file: RelPath, pub question: String, pub findings: Vec<Digest> }
```

- **What it writes.** Ordinary target files, in YAML, at `reconstructed` once they compile and pass lint; a written draft that is not in `kept_failing` is at that level. Each names the findings it was made from in comment lines, and carries the name its rule gave it from the surface's own words.
- **What it leaves out.** Any unit, key, or read-back rule the system did not state. Where one is needed, the draft holds an `open_question` marker in its place, which the compiler refuses in every mode, and the question goes to the unknowns ledger through the caller.
- **What it never overwrites.** The first line of a draft carries the digest of the rest of the file. A file is rewritten only while that still holds, which means the drafter wrote it and nobody has changed it. Otherwise the draft is listed in `conflicts` and not written; `DraftReport::conflict_failure` gives `draft_conflict` for a caller that wants a failure. A file that cannot be read as text, and a folder where the file would go, are conflicts too.
- **Where it writes.** To a regular file inside the target folder, and nowhere else. Every folder on the way is looked at without following a link, and a link is never written through: not a linked folder, not a link where the file would be, not one that leads nowhere. Such a draft is listed in `refused`. A new draft is made with create-new, which fails where anything has the name. An unchanged draft of the drafter's own is replaced by renaming a new file over it, so no file that exists is ever opened for writing.
- **Where it stops.** It drafts no write, ever, in any file kind. An operation whose effect is not plainly `read` is listed in `refused`. A document of a pack's kind is refused when its file kind's `writes` says it describes a write; a kind that does not answer is taken to describe none, so every pack whose kind can describe a write is owed the answer. Of the kernel's kinds a rule may draft `operation` and `source` only: a drafted target or sign-in file would widen what the target reaches, and a pipeline or a test stands for a person's judgment. A pack drafts its own kinds and no other pack's. Registration refuses the rest, and the drafter checks the owner again, since a kind may be registered after its rule. A rule that names a file outside the folder of its file kind fails the call. The scout's other stages are `a2m-actions`' work.

### The test pack

The module `testing`, behind the feature of that name, holds a small imaginary pack called `stand`: one surface kind, the file kind `call` in `calls/`, the mode `stand` with the verbs `read` and `upsert`, one control, one lint rule, and one drafting rule. It states what the compiler asks of a pack: a `fetch` step never saves and a `send` step always does, whatever a call's file says of its effect; the verb `upsert` is a write by what it does; a `fetch` of a call that writes does not compile; and a call whose effect is `write` is a document the drafter does not write. `testing::stand_set()` gives a `CompilerSet` with it, `testing::register_stand(&mut builder)` adds it to a builder, and `testing::example_root()` is the `example` target in `targets/example/`, which is written for it. A crate above names the feature under its dev-dependencies. It is never enabled in the product.

A crate whose tests need a whole pack, with a driver beside the compiler side, registers the parts itself through `a2m-provider`. The objects are `testing::surface_kind_spec()`, `control()`, `file_kind()`, `lowering()`, `lint_rule()`, and `drafting_rule()`. The names its manifest declares are `PACK`, `FOLDER`, `surface_kind()`, `call_kind()`, `mode()`, `verbs()`, `step_kinds()`, `lint_code()`, and `finding_kind()`. The module's own text says what a driver meets in a plan compiled with it. A test in `a2m-provider` registers the pack this way and compiles the `example` target to the same plan as `stand_set()`. The surface kind takes an optional `helper` program with a `helper_digest`, so that a test can put a declared program in a plan.

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
| `input` | `registration_conflict` | Two packs registered the same surface kind, control, file kind and surface kind, mode and verb, lint code, or drafting pair; or a pack registered a name that is the kernel's |
| `input` | `target_unreadable` | The drafter could not write a draft into the target folder |
| `input` | `schema_invalid` | `Misfits`: a run-time value does not fit its `JsonSchema`. The message names the first path and counts the rest |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| The same sources and compiler give byte-identical plans, on every machine | The `example` target compiled twice, from disk, from memory, from a copy in another folder, and with other line endings, gives the same bytes. CI runs it on three operating systems |
| A mode, verb, step kind, surface kind, control, or file kind nobody registered does not compile | Tests with planted unknowns, and the `example` target compiled with no pack |
| The commit-point count equals the number of `commits` steps in every write binding | A test over every binding of the compiled `example` target, and a declared count that differs is refused |
| A write never compiles as a read, or with nothing recorded, whatever a target's file says of what a binding refers to | `a_write_never_compiles_as_a_read_whatever_a_target_file_says_of_its_call`, on the route the audit gave; `what_a_faulty_lowering_returns_is_refused`, for a step marked against its kind and a binding that is more than its operation declares |
| A declared effect lower than the derived one is refused | A compiler test, for a pipeline; the two tests above, for an operation |
| The loader never reads outside the target folder, reads a folder once, and reads no file past its size limit | Tests with a link out, a `..` reference, and an absolute path; `the_loader_reads_a_file_of_bounded_size_and_a_folder_once`. The link tests run where links can be made without a privilege |
| A permission names one exact address or one name | `a_permission_names_one_exact_address_or_one_name`, with a wildcard, a range, the address that stands for every address, a path, and user information |
| A value the credential lint knows is reported and is quoted by no diagnostic, a pack's included | `a_value_the_credential_lint_knows_is_reported_and_quoted_by_no_diagnostic`, with a lint rule that quotes what it finds |
| A draft is written only to a regular file inside the target folder, never through a link | `a_draft_is_never_written_through_a_link`, with a linked folder, a link that leads nowhere, and a link to an unchanged draft; `a_file_that_cannot_be_read_as_a_draft_is_not_overwritten` |
| The drafter writes no write in any file kind, and a pack drafts only its own kinds, an operation, or a source | `a_write_is_never_drafted_in_any_file_kind_and_a_pack_drafts_only_its_own_kinds` |
| Every kernel lint code has a passing and a failing test | A test that walks `codes::KERNEL_CODES` and fails on a code with no case; the `example` target is the passing case |
| A draft never carries a unit, key, or read-back the finding did not state | A test with a finding that lacks a value, for the test pack's rule; each pack adds one per drafting rule |
| What `compile` emits decodes, through `a2m-core`, to an equal `Plan` with the same hash | A round-trip test over the `example` target |
| A pack is handed another pack's entries only when it declared the read | A test with a second lowering that declared none |
| A program a surface declares reaches the plan, and one named by a relative path does not compile | Tests with the test pack's helper program, and with a surface kind whose address is the program, in the surface and in an environment |
| A run-time value is judged by the rules a file is, and each kind of misfit is refused with the path of its field and without the value | `schema::tests`, over named values and over JSON, and one case that compares the words with a file's |

## Build order

1. Done: the YAML parser verified, and the schema check chosen. Kernel schemas, `load`, `Diagnostics`. Package B4.
2. Nothing here for package 1.1: its hand-written plan uses the plan types in `a2m-core`.
3. Done: the registration traits and `CompilerSet`; stages 1 to 9, proven through the test pack `stand`. Not done: no real pack is registered yet; the web pack's screen kind and the HTTP pack's endpoint kind are their packs' packages. Package 1.3.
4. Done: field types compiled to normalizers, built in and from `types.yaml`. Package 3.4.
5. Done: identity strategies for `natural` and `composite`, commit-point counting for writes, and the write lint rules. Not done: the long form of `identity` for `stamped` and `find` has only its strategy and inputs; the stamped field and the lookup are written with the first operation that needs one. Packages 3.3 and 3.8.
6. Done: pipelines, and sources compiled to `SourceSpec`. `a2m-sources` uses these types and has no copy, and a test there compiles the `example` target and reads its fixture sheet. Package 3.9.
7. Done: the drafter and its rules for what a draft may hold, proven with the test pack's drafting rule. Not done: no pack's drafting rules exist. Package E4.
8. Done: three gaps the provider contract's builder found ([decision 0124](../../decisions/0124-the-plan-declares-a-surfaces-programs-and-a-schema-checks-run-time-values.md)): a surface kind's program fields and the plan's declared programs; run-time values and JSON checked against a `JsonSchema`; and the test pack's parts made public.

9. Done: what the safety audit found ([decision 0153](../../decisions/0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md)): the rules of a write the compiler holds by itself, with `StepCommits`, `Lowering::commits`, and `Lowering::least_effect`; the Scope rule's form; the drafter's writing and what it may draft, with `FileKind::writes`; the loader's bounds; the credential lint's words and what a diagnostic never quotes.

Not built, in any step:

- **The rest of the Scope rule.** The compiler checks the form of what an operation's `permissions` declares: one exact address, one name, once. It does not check a destination against a list a person registered for the target, because no file holds such a list; the review summary shows every destination, and confirming a new one is the publish gate's ([section 502](../../spec/5-security/502-defending-against-a-manipulated-agent.md)). It does not check that an upload or a download names an input of the operation or a root of the installation.
- **A draft's mark in the plan.** `CompileMode::Draft` turns the test rules into warnings and leaves no trace in the plan, so a draft compiled under a release version has the bytes and the hash of the published plan; a test pins this. What marks a draft today is `DRAFT_VERSION`, which the caller passes. Refusing any other version for a draft is the fix, and it waits: the test helpers of nine crates compile drafts as version 1.0.0, and one of them asserts that its fixtures give no other warning, so even a warning would break a crate that cannot know why. The plan's shape is not changed for it.
- **A secret outside what the credential lint knows.** A diagnostic never quotes a value under a key the lint knows, or one in a credential's shape. A token pasted under any other key, where a name or a word is expected, is still echoed, up to 64 characters. A fixture's recorded file is looked through by line for key material, an authorization header's value, and a signed token, and for nothing else; one that is not text is not looked through.

- `lint_one` compiles the whole target again; nothing is reused by file digest.
- An operation's outputs by reference to a pack's schema, and nested records.
- Binding order from each mode's rank in its provider's manifest; bindings are a list and keep their order.
- The `stands` rule for surfaces that may sit under an application. The plan carries where a surface stands; refusing a write on a surface that does not say is the rule of the pack whose kind needs it, and no such pack is built.
- Lint rules that are errors only on a server.
- The count of an operation's examples below one: an operation may have none.
- A check that an auth provider knows each setting's name. The compiler holds no list of auth providers; the provider's own check refuses an unknown name when the version is published.

## Open points

- `Verb` is a checked name in `a2m-core` for now; whether it should stay a closed list is open there. Either way, a verb that no lowering registers does not compile.
- How a plug-in registers file kinds and lint rules across a process boundary: as data at start, most likely, with its lowerings answered over the plug-in protocol. `JsonSchema` and `SurfaceKindSpec` are already data; the traits are not.
- Whether a lint rule may be silenced, and where the reason is written.
- Where drafts are written inside a project, and whether they are kept apart from files a person wrote. Today they are written among them and told apart by their first line.
- Settled: everything the run time needs from the files is in the plan, and what only changes what the compiler emits is applied by the compiler ([decision 0121](../../decisions/0121-the-plan-carries-what-the-run-time-needs-from-target-files.md)). The step "Done" for it is this list's: `auth.yaml`, the target's own facts, an environment's test records and approval rule, a surface's `stands`, an operation's examples and decision points, a field type's format.
- The names under `defaults.timeouts` are whatever a pack's lowering asks for. No pack exists to say which; the first real pack fixes its names in its own section.
- A reader's name is checked for shape only; `a2m-sources` knows which readers exist.
