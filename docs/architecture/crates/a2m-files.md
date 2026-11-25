# `a2m-files`

What a target is: the target files' schemas, the loader that reads them with file and line, the linter, the compiler that turns them into a hashed `CompiledTarget`, and the drafter that writes draft files from findings. It is neutral. Every file kind, verb lowering, lint rule, and drafting rule that belongs to one kind of system arrives from a pack through the registration traits defined here.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 3, Stores |
| May depend on | Layers 1 and 2 |
| Depends on | `a2m-core`, `a2m-expr`. Outside: `saphyr-parser`, an event parser for YAML that gives the place of every value ([decision 0078](../../decisions/0078-target-files-are-read-by-an-event-parser-into-our-own-located-tree-and-checked-by-our-own-schema-subset.md)); `serde` and `serde_json` for schemas and diagnostics. Schemas are checked by this crate's own code. The compiled target's encoding and hash come from `a2m-core` |
| Roadmap packages | Roadmap package B4, schemas and the loader for the kernel's file kinds; package 1.3, the compiler for reads and the registration traits; 3.4, field types; 3.8, identity strategies; 3.9, pipelines and sources; E4, the drafter |
| Carries out | Spec sections [205](../../spec/2-kernel/205-target-files.md), [206](../../spec/2-kernel/206-the-compiler-and-the-compiled-target.md), [209](../../spec/2-kernel/209-record-identity-and-dependent-records.md) (what the compiler checks), [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) (typed templates, the review summary), [202](../../spec/2-kernel/202-kernel-and-packs.md) (what a pack registers), [402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md) (the drafter); the [file formats](../../reference/file-formats.md) and the [steps](../../reference/steps-snapshots-and-diffs.md#steps); [decision 0036](../../decisions/0036-target-files-are-compiled-into-a-hashed-plan-before-anything-runs.md) |

## Owns

- The schemas of the kernel's file kinds: `target.yaml`, `auth.yaml`, `types.yaml`, `operations/`, `pipelines/`, `sources/`, `tests/`, and the fixture marker.
- The loader: reading a target folder, and only that folder, into located documents. It does not read the folder `held/` at the top, where drafts that hold an open question wait; `held_files` lists them (at most 5,000) for a message, and the compiler never sees them ([decision 0298](../../decisions/0298-a-draft-with-an-open-question-waits-in-a-holding-folder.md)).
- The nine compiler stages, and every diagnostic they produce, each with a code, a sentence, a place, and a fix.
- The kernel's lint rules, and the running of every registered one.
- Producing the `CompiledTarget`. Its types, its encoding, and the check of its hash are `a2m-core`'s; this crate is the only code that makes one.
- The registration traits a pack implements, and the `CompilerSet` that holds what every enabled pack registered.
- The drafter, and the rule for what a draft may and may not contain. It writes a draft that holds an open question under `held/`, at the same relative path, and with it each file made from the same finding that names, as a whole scalar value, something a held file defines (what a pack's `FileKind::names` gives for its kinds; the naming key of the kernel's kinds, and the entries under `types:`), taken to a fixed point inside the one finding; a file of the finding that names nothing held stays in place. It moves an unchanged draft of its own between `held/` and its place as the question comes and goes; a file a person changed is never moved or removed ([decision 0298](../../decisions/0298-a-draft-with-an-open-question-waits-in-a-holding-folder.md)).

It must never: know a mode, a locator, an endpoint, or any other knowledge of one kind of system; read outside the target folder; write a file the developer has changed; give a draft any level but `reconstructed`; run, store, or publish a compiled target (`a2m-actions` stores a published one through `a2m-ledger`); define a compiled target type, or a variant of one the engine would have to learn from this crate; be depended on by `a2m-engine`; call a model.

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
/// The same from memory, such as the sources stored beside a published compiled target.
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

A file's digest is taken of its text with every line ending a line feed and no byte-order mark, so a checkout with other line endings compiles to the same compiled target; a carriage return that stands alone is a line ending too. A fixture's recorded files are digested byte for byte.

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

/// `version` is what the compiled target is published as; a draft passes `DRAFT_VERSION`.
pub fn compile(sources: &Sources, set: &CompilerSet, mode: CompileMode, version: Version) -> Result<Compiled, Diagnostics>;
pub struct Compiled { pub compiled_target: CompiledTarget, pub encoded: EncodedCompiledTarget, pub warnings: Diagnostics, pub review: ReviewSummary, pub test_bindings: Vec<Option<usize>> }
pub struct ReviewSummary { pub items: Vec<ReviewItem>, pub sign_ins: Vec<SignInReview>, pub asserted: Vec<AssertedReview>, pub changes: Option<VersionChanges> }   // module `review`; see below
impl ReviewSummary { pub fn compare_with(&mut self, published: &CompiledTarget, compiled_target: &CompiledTarget); }

/// What changed in each tool between two versions, by the closed list of decision 0439 (module `changes`).
pub fn changes_between(before: &CompiledTarget, after: &CompiledTarget) -> VersionChanges;
/// What a tool is, exactly, as a client pins it: the SHA-256 of its name, description, schemas, and annotations in canonical JSON (decision 0460).
pub fn tool_digest(tool: &CompiledTool) -> Option<Digest>;
/// The key that marks an output of the system's words in a tool's output schema.
pub const UNTRUSTED_MARK: &str = "x-anything2mcp-untrusted";
pub struct VersionChanges { pub from: Version, pub to: Version, pub tools: Vec<ToolChange> }   // breaking, then compatible, then unchanged
pub struct ToolChange { pub tool: OperationName, pub class: ChangeClass, pub changes: Vec<Change> }   // breaking changes first
pub enum ChangeClass { Breaking, Compatible, Unchanged }
pub enum Change { ToolGone, InputGone { .. }, InputRequired { .. }, InputTypeChanged { .. }, InputWordsNarrowed { .. }, OutputGone { .. }, OutputMayBeAbsent { .. }, OutputTypeChanged { .. }, EffectRose { .. }, HazardChanged { .. }, KeyChanged { .. }, /* compatible: */ ToolAdded, InputAdded { .. }, InputLoosened { .. }, InputWordsWidened { .. }, OutputAdded { .. }, OutputAlwaysPresent { .. }, OutputWordsChanged { .. }, EffectLowered { .. }, HazardRemoved { .. }, DescriptionChanged, DefinitionChanged }

/// One edited file, for an agent's `draft.lint`: it takes the place of the file at its path, and the target is compiled as a draft.
pub fn lint_one(previous: &Sources, edited: SourceFile, set: &CompilerSet) -> Diagnostics;
```

**The review summary** ([decision 0252](../../decisions/0252-a-fixed-place-a-review-that-shows-what-runs-and-a-sign-in-before-its-probe.md)). Every field is typed, so a page renders it and prints no sentence built from a system's or an agent's words; the only sentences are the module's constants. `ReviewItem` holds the tool, effect, hazard, commit points, surfaces, destinations, uploads, downloads, `place` (the fixed place of its record, where it has one), `bindings` (for each: operation, mode, surface, the surface's declared programs, and `BindingStates::Stated { runs, environment }` or `NotStated { says: NOT_STATED }`), and `reach` (`Reach::Stated { groups, leaves_this_machine }` or `Partly { known, says: REACH_NOT_STATED }`, the groups by `AddressScope`). A run is a `ProgramRun::Program { path, digests }` or a fixed pair `Script { interpreter, interpreter_digests, arguments, script, script_digest }`, which carries `SCRIPT_NOTICE`, or a launcher `Launcher { launcher, launcher_digests, arguments, entry, entry_digest }`, which carries `LAUNCHER_NOTICE` and says that what the entry file loads is not pinned ([decision 0398](../../decisions/0398-the-review-shows-a-launched-program-as-a-launcher-and-says-what-nothing-pins.md)); a variable is `Set(value)`, `Secret`, or `Passed`. `SignInReview` holds the entry's place in the compiled target's `auth`, surfaces, provider, mode, credentials by role and reference, `ProbeReview::Declared` or `Pending`, and per environment `SentTo::NotStated`, `ProviderNotHeld`, or `Addresses { groups, leaves_this_machine }`; `AssertedReview` holds a read a pack's file asserts, from `FileKind::asserts`: the file kind, the surface, the `AssertedRead` (the entry, its method, its path, and its body's fields, each an `AssertedField` with its name, the word of its `type`, and its `enum`), and `says: ASSERTED_NOTICE`, that the person's word is the only evidence that the request changes nothing; the compiler asks every document of a pack's kind that passed its schema and stage 3, and orders the items by surface, file kind, and entry ([decision 0426](../../decisions/0426-the-publish-page-names-each-asserted-read-after-the-sign-ins-from-what-each-file-kind-says-it-asserts.md)). `ReviewSummary::state_sent_to(entry, environment, Option<Vec<SentAddress>>)` is how `a2m-actions` fills in what the session keeper says, since the compiler cannot know. Tools are ordered by hazard, effect, then exposure (a reach not stated, then one off the machine; a binding that does not say, or starts a kept script or a launcher, then a program), then name; kinds of place from unreadable, public, and a name to this machine last; sign-ins with a probe not drafted first, then by their place in the file. `ReviewSummary::map_text(&self, f)` gives the summary with every text it holds passed through `f` (each address, path, argument, variable's name and value, place, file name, credential's role, probe's name, and what a sign-in sends where, and an asserted read's entry, method, path, and fields), the fixed sentences, checked names, and digests left as they are; the registry passes the redactor before a person's page sees it ([decision 0264](../../decisions/0264-the-publish-page-renders-the-typed-review-sign-ins-first-from-one-keeper-call.md); `every_text_of_the_summary_passes_through_the_function_given`).

**What changed since the version in force** ([decision 0439](../../decisions/0439-a-tools-name-carries-no-version-and-a-breaking-change-is-named-at-publish.md), [decision 0454](../../decisions/0454-what-breaks-follows-where-it-leads-on-the-publish-page-and-is-named-in-the-answer-too.md)). `changes_between` compares two compiled targets tool by tool and classifies each change by the closed list: the tool gone; an input gone or renamed; an input that must now be given (required with no default); an input's value type changed or its words (a choice's codes) narrowed; an output gone, or one always present that may now be absent; an output's value type changed; the effect class risen; the hazard added or changed to another than `none`; the key's inputs changed. Everything else is compatible, each named. A pipeline is compared by its inputs, effect, and hazard: its result declares no fields. The compiler leaves `ReviewSummary::changes` empty, since it cannot know what is published; `a2m-actions` fills it with `compare_with`. `map_text` passes every name and word in it through the redactor with the rest. Anything else a client reads, `definition_changed`, is a changed title or a changed `tool_digest`, so a tool called unchanged keeps its digest.

**What a client pins and what it treats as data** ([decision 0460](../../decisions/0460-a-tools-digest-is-of-its-definition-as-published-and-the-untrusted-mark-sits-on-the-output-field.md), on decisions 0325 and 0439). `tool_digest` is the SHA-256 of one canonical JSON object of the tool's `name`, `description`, `input_schema`, `output_schema`, and `annotations` under the compiled target's names; the version, the target, and the title are not in it, so the same definition under another version keeps it. Stage 8 writes `UNTRUSTED_MARK: true` on the property of each output whose field type is marked `untrusted`, at the top of the property beside any `anyOf`, and nothing on any other; a pipeline's result declares no fields. `a_tools_digest_follows_its_definition_and_the_systems_words_are_marked` holds the digest of the `example` target's order read as one value on the three systems.

Stages 1 to 9 are those of [section 206](../../spec/2-kernel/206-the-compiler-and-the-compiled-target.md). This crate adds what a builder needs to know about each.

| Stage | Kernel work | Pack work, through `CompilerSet` |
| --- | --- | --- |
| 1 Parse | YAML to located documents, in `load`; a duplicate key, an anchor, an alias, and a tag are errors | None |
| 2 Validate | Kernel schemas, and the schema version; the kernel's keys of a surface and of a binding | Schemas of the pack's file kinds, of the fields its surface kind adds to a surface, and of the fields its mode adds to a binding |
| 3 Resolve | Names across files; every name defined once; controls, field types, and surfaces that schemas refer to | `FileKind::names` says which names a document defines |
| 4 Type-check | Every expression, through `a2m-expr`, with the names its place declares | `FileKind::expressions` lists the expressions inside the pack's own files |
| 5 Lower | Calls the lowering registered for the binding's mode and verb; a mode or verb nobody registered stops the compile. A lowering that declared it reads another pack's kind is handed that kind's validated entries as data | `Lowering::lower` |
| 6 Analyze | Commit points counted from the lowered steps' `commits` flag, and each step's flag held to what its pack states for the step's kind; a write saves at least once and a read never; a read holds no step of a kind its pack states can change the system; a declared effect lower than the derived one is refused; identity strategies; permission manifests, each entry one exact address or one name; no step reads a later step | `Lowering::identity` says whether each identity strategy is possible for a binding; `Lowering::commits` says whether a kind of step saves; `Lowering::changes` says whether a kind of step can change the system; `Lowering::least_effect` says what a binding is by what it does |
| 7 Lint | Kernel rules; among them, after the tools are described, the rule for a description that addresses its reader: four closed lists of patterns in the module `addressed`, read over each tool's description and its inputs' descriptions, each finding a warning (`kernel.description_instructs_the_reader`, `kernel.description_names_an_unpublished_tool`, `kernel.description_names_a_place_outside`, `kernel.description_asks_to_hide`; [section 502](../../spec/5-security/502-defending-against-a-manipulated-agent.md)) | Registered `LintRule`s |
| 8 Describe | Tool names, descriptions, schemas, annotations; two tools that cannot be told apart are an error; the whole definition is bounded text: each field description, enum member, text default, and field-type label at most 1,024 characters with no hiding character (`kernel.field_text_unbounded`), and 8,192 in all (`kernel.definition_unbounded`) ([decision 0325](../../decisions/0325-what-a-browser-can-reach-and-what-a-tools-definition-may-hold.md), point 2) | None |
| 9 Emit | The canonical encoding and the hash, both through `a2m-core` | None |

Lowering comes before analysis so that commit points are counted from the lowered steps, and the count can never disagree with what runs.

**What the compiler holds about a write, whatever a pack says** ([decision 0153](../../decisions/0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md)). An operation whose effect is `write` or `irreversible` declares at least one commit point, and each of its bindings has exactly that many committing steps, all among the steps that do the work. An operation whose effect is `read` declares none and has no committing step in any binding. A lowered step's `commits` flag agrees with what the lowering states for the step's kind: `Never`, `Always`, or `AsDeclared`, the last for a kind that saves when the pack's own files say what it refers to does. An operation declares no less than `Lowering::least_effect` gives for each binding, which is how `irreversible` is derived: the pack states which of its verbs delete, send, or cannot be undone. A lowering names a surface of a kind its own pack registered. `CompiledTarget::check` in `a2m-core` holds the same rules for a compiled target the compiler did not make. An operation whose effect is `read` holds no step, in any of its lists, of a kind for which the lowering states `StepChanges::Can`, whichever pack lowered it (`kernel.read_acts`, [decision 0307](../../decisions/0307-what-the-evidence-level-is-for-and-what-a-review-shows-about-a-tools-evidence.md), point 4). A kind is `Never` when a step of it cannot change the system, `AsDeclared` when it changes the system only where the pack's own files declare that what it refers to does and the pack's driver holds the step to that, and `Can` otherwise; a lowering that does not mark a kind leaves it `Can`, so a kind forgotten fails closed. A step marked `commits` of a kind marked `Never` is refused as `kernel.lowering_invalid`. A pack's own nearer refusal, such as the web pack's `read_acts`, comes first, at the lowering. What the compiler cannot hold: a lowering that states nothing leaves every kind `AsDeclared` for saving and claims no least effect, and for it the compiler has only the lowering's own flag. Every pack's lowering is owed all three statements, and every first-party lowering makes them. Every stage reports what it finds and the compiler carries on where the data is sound, so one run reports the problems of stages 2 to 8 together.

**The credential lint, and what a diagnostic never quotes.** Three things are a credential in a file, as `kernel.credential_in_file`: a value, other than a vault reference, under a key whose name holds `password`, `passwd`, `passphrase`, `secret`, `api_key` or `apikey` (a hyphen in a key reads as an underscore), `private_key`, `client_secret`, or one of the kinds of token named as such, `access_token`, `refresh_token`, `auth_token`, `id_token`, `session_token`, `bearer_token`; text in a credential's shape under any key, which is key material, an authorization header's value in any letter case, or a signed token; and the same shapes on a line of a fixture's recorded file. A value under a key one of whose words is `token`, `cookie`, `authorization`, `pin`, or `bearer` (a key's words are split at `_`, `-`, `.`, and a change from lower case to upper case, so `pinCode` has the word `pin` and `spinner_count` does not) is a warning, `kernel.credential_suspected`, unless it reads as a lowercase name, an expression's dotted name, or a path into an answer such as `*.pin_order` or `body.items.*.id` (names joined by dots, where a part may be `*`): such a key holds a secret as often as it holds the name of one, and an error would refuse a correct file. A number is judged only under a key that is the word itself, as in `pin: 4821`. Every value found this way, reported or not, is taken out of each diagnostic at its place, whichever rule or pack made the diagnostic; one that does not read as a name is taken out of every diagnostic. `[not shown]` stands where it was.

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
/// The payload of the compiled target's `CompiledSource`: what `sources/*.yaml` compiles to, and what `a2m-sources` loads.
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

These are data, with the serde form `a2m-sources` reads ([decision 0087](../../decisions/0087-the-source-readers-interface-as-building-it-settled-it.md)). A row field read from a bare column is data in the spec; only a row field that is an expression is in the compiled target's `mapping`. A row field's type, for a pipeline's `with`, is its field type's. The fingerprint's encoding is fixed here, so the compiler refuses a digest that is not the digest of its parts.

### The compiled target

`CompiledTarget`, `Step`, `PackSection`, `PackPayload`, `CompiledExpr`, and the rest of the compiled target's types are defined in `a2m-core`, as data, with the check of a compiled target's hash; see the [`a2m-core` design](a2m-core.md). This crate is the only code that produces a `CompiledTarget`, and what it adds is how each part is filled:

| Compiled target part | Filled in stage | How |
| --- | --- | --- |
| `header` | 9 | The compiler's version, the target, the version the caller gave, and the digest of the sorted list of source paths and file digests. No timestamp, no absolute path |
| `surfaces`, `environments` | 2 and 3 | A surface's address is the value of its kind's address key; the other fields its pack added are its payload. `production` is added when the target does not declare it |
| `field_types` | 3 | The built-in types, then the target's; each carries its normalizer as a `CompiledExpr`, built from the field type through `a2m-expr`. A target's type of a complete built-in's name is refused, `kernel.built_in_type_redefined`, at lint and at compile, so that no target can take the untrusted mark off `untrusted_text`; `money` and `choice`, which need more, are completed by a target's type of their name ([decision 0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md)) |
| `expressions` | 4 and 5 | Every expression once, as canonical text with its type; steps and bindings refer to them by index |
| `operations`, `bindings` | 5 and 6 | Bindings keep declared order. A binding's step lists are the lowering's, unchanged. A binding with no `status` is `draft`; an operation of which every binding is `draft` or `retired` is `kernel.no_binding_may_act`, a warning in every compile: an error would stop the offline test that promotes a draft binding, which runs on a publish compile ([decision 0440](../../decisions/0440-an-operation-no-binding-of-which-may-act-is-warned-of-and-a-call-of-one-fails-for-good.md)) |
| `packs` | 3 | Each registered file kind's `compile` returns one payload for each surface kind that has documents; the kernel wraps it with the owner's name, stores and hashes it, and never decodes it. Sections are in order of file kind, then surface kind |
| `tools` | 8 | Operations, then pipelines. A decimal travels as text in a tool's schema. A tool's module is the longest of the target's modules that follows the target's name in its own |
| `target` | 2 and 3 | The target's own facts. `defaults.timeouts` and `defaults.limits` are not here: a lowering reads a timeout and writes it into its steps, and a pipeline that states no limit takes the default, so the engine applies none |
| `environments` | 3 | Test records as record keys, as written; the approval rule. `production` is added when the file does not declare it, and is the default environment unless the file names one |
| `surfaces` | 2 and 3 | A surface's `programs`: what it states under the fields its kind names as program fields, and, where the path field is the address key, its address in each environment, with the surface's digest. A path that is not absolute, a path with `.` or `..` in it, a program on a network share with no digest, a digest that is not one, and a digest with no program are `program_invalid`, at the value's place |
| `auth` | 3, after the operations and the pack sections | One entry per group of surfaces. A probe is a `read` operation with the inputs `probe_inputs` gives, or a name a pack's document defines on one of the entry's surfaces; anything else is `unknown_name` or `probe_invalid`. In a draft only, a probe that names nothing, where no operation file of that name exists, is `CompiledProbe::Pending` with the warning `probe_pending` ([decision 0252](../../decisions/0252-a-fixed-place-a-review-that-shows-what-runs-and-a-sign-in-before-its-probe.md)). A credential that is not a vault reference is `credential_in_file`, and is never shown |
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
    /// The places a document declares that are each exactly one record, which no input chooses (decision 0252).
    fn fixed_places(&self, doc: &Located<Doc>) -> Vec<Located<DefinedName>> { Vec::new() }
    /// The reads a document asserts: requests by a method that may write, marked `read_asserted: true` (decision 0394, point 5).
    fn asserts(&self, doc: &Located<Doc>) -> Vec<AssertedRead> { Vec::new() }
}
pub struct FixedPlace { pub kind: FileKindName, pub name: String }
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
    fn changes(&self, kind: &StepKind) -> StepChanges { StepChanges::Can }   // whether the kind can change the system; unmarked fails closed
    fn least_effect(&self, binding: &ResolvedBinding<'_>) -> EffectClass { EffectClass::Read }   // by what the binding does
    fn binding_schema(&self) -> &JsonSchema;        // every key of a binding but mode, verb, status, precondition
    fn identity(&self, binding: &ResolvedBinding<'_>, strategy: IdentityStrategy) -> Possible;
    fn fixed_place(&self, binding: &ResolvedBinding<'_>) -> Option<FixedPlace> { None }   // the one place it names
    fn lower(&self, binding: &ResolvedBinding<'_>, cx: &LowerContext<'_>) -> Result<Lowered, Diagnostics>;
    /// What it starts, sets, and sends to, for the review summary; asked after `lower` (decision 0252).
    fn statement(&self, binding: &ResolvedBinding<'_>, lowered: &Lowered, cx: &LowerContext<'_>) -> Statement { Statement::NotStated }
}
pub enum Statement { NotStated, Stated(BindingStatement) }
pub struct BindingStatement { pub runs: Vec<ProgramRun>, pub environment: Vec<EnvironmentVariable>, pub sends: Sends }
pub enum Sends { NotStated, Only { surface: bool, addresses: Vec<String> } }   // default NotStated
pub enum Possible { Yes, No(String) }
/// Whether a step of one kind saves in the system. A lowered step marked otherwise does not compile.
pub enum StepCommits { Never, Always, AsDeclared }
/// Whether a step of one kind can change the system. A read holds no step of a kind that `Can` (decision 0307, point 4).
pub enum StepChanges { Never, AsDeclared, Can }
pub struct Lowered {
    pub surface: SurfaceName,
    pub act: Vec<Step>, pub confirm: Vec<Step>, pub read_back: Vec<Step>, pub lookup: Vec<Step>,
    pub settle_ms: u32, pub prefer_response: bool,
}
// Lowering::evidence(&self, binding, &lowered) -> WriteEvidence: what a write binding offers as evidence. Default:
// `response` where the lowering prefers the save's answer, `read_back` otherwise. `acknowledged` makes the binding
// `irreversible` by what it does, and such a binding needs no read-back (decision 0161).
// LowerContext::expr also reads `current`, what the engine's last read of the record gave, by the operation's inputs,
// each optional, and `record`, whose `key` is the record's key in the system, its parts as text (decision 0161).

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
pub struct SurfaceKindSpec { pub kind: SurfaceKind, pub address_key: String, pub fields: JsonSchema, pub programs: Vec<ProgramField>, pub notices: Vec<NoticeField> }
// with_notice(key, word, SurfaceNotice): a surface of the kind carries the notice while its own field `key` holds `word`;
// CompilerSet::surface_notices(&SurfacePlan) is what a door asks. Computed from the compiled target's surface and the registration,
// so the compiled target's shape and pinned hash are unchanged; decision 0193
// CompilerSet::address_key(&SurfaceKind) -> Option<&str>: the key a surface of the kind states its address under, as its
// pack registered it, for `target.add` to write a surface from an address with no pack's knowledge; decision 0199.
// An address of blanks states none: `kernel.missing_field`.
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
| `DraftContext` | `DraftingRule::draft` | `target()`, `existing()`, `open_question(text)`, `open_question_quoting(sentence, words)`: the product's sentence and the system's words kept apart (`Quoting`, `OpenQuestion::quoting`), the words redacted by the drafter's redactor (`Drafter::with_redact`), cleaned, at most 4 parts of 64 characters; `OpenQuestion::stored` and `StoredQuestion::read` keep them apart in the unknowns ledger's one text, and an older text reads whole (decision 0193); `not_drafted(reason)` says why the rule drafted nothing for this finding, in the pack's own words, and `left_out_as_secret(count)` how many properties it left out for a secret's name, which `DraftReport` lists and adds up ([decision 0337](../../decisions/0337-a-description-says-who-wrote-it-a-drafter-reports-what-it-left-undrafted-and-bindings-are-ordered-by-rank.md)) |

A lowering sees its own binding, the operation's inputs and key, the field types, and its own pack's compiled sections. Of another pack's kinds it sees only those it declared in `reads`, as validated entries, never as that pack's compiled section or code. It makes expressions only through the context, which type-checks them and returns the compiled target's `ExprIndex`.

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
    pub kept_failing: Vec<(RelPath, Diagnostics)>, pub open_questions: Vec<OpenQuestion>,
    pub not_drafted: Vec<NotDrafted>, pub left_out_as_secret: usize, pub not_covered: Vec<Digest>,
}
/// A finding a rule covers and drafted nothing from, with the pack and the reason the rule gave (empty where it gave none).
pub struct NotDrafted { pub finding: Digest, pub pack: String, pub reason: String }
pub struct OpenQuestion { pub file: RelPath, pub question: String, pub findings: Vec<Digest> }
```

- **What it writes.** Ordinary target files, in YAML, at `reconstructed` once they compile and pass lint; a written draft that is not in `kept_failing` is at that level. Each names the findings it was made from in comment lines, and carries the name its rule gave it from the surface's own words.
- **What it leaves out.** Any unit, key, or read-back rule the system did not state. Where one is needed, the draft holds an `open_question` marker in its place, which the compiler refuses in every mode, and the question goes to the unknowns ledger through the caller.
- **What it never overwrites.** The first line of a draft carries the digest of the rest of the file. A file is rewritten only while that still holds, which means the drafter wrote it and nobody has changed it. Otherwise the draft is listed in `conflicts` and not written; `DraftReport::conflict_failure` gives `draft_conflict` for a caller that wants a failure. A file that cannot be read as text, and a folder where the file would go, are conflicts too.
- **Where it writes.** To a regular file inside the target folder, and nowhere else. Every folder on the way is looked at without following a link, and a link is never written through: not a linked folder, not a link where the file would be, not one that leads nowhere. Such a draft is listed in `refused`. A new draft is made with create-new, which fails where anything has the name. An unchanged draft of the drafter's own is replaced by renaming a new file over it, so no file that exists is ever opened for writing.
- **Where it stops.** It drafts no write, ever, in any file kind. An operation whose effect is not plainly `read` is listed in `refused`. A document of a pack's kind is refused when its file kind's `writes` says it describes a write; a kind that does not answer is taken to describe none, so every pack whose kind can describe a write is owed the answer. Of the kernel's kinds a rule may draft `operation` and `source` only: a drafted target or sign-in file would widen what the target reaches, and a pipeline or a test stands for a person's judgment. A pack drafts its own kinds and no other pack's, and a document of a kind two packs register only for a surface of a kind its own pack registered it for. Registration refuses the rest, and the drafter checks the owner again, since a kind may be registered after its rule. A drafted binding is `draft`, never `verified`. A rule that names a file outside the folder of its file kind fails the call. The scout's other stages are `a2m-actions`' work.

### The test pack

The module `testing`, behind the feature of that name, holds a small imaginary pack called `stand`: one surface kind, the file kind `call` in `calls/`, the mode `stand` with the verbs `read` and `upsert`, one control, one lint rule, and one drafting rule. It states what the compiler asks of a pack: a `fetch` step never saves and a `send` step always does, whatever a call's file says of its effect; the verb `upsert` is a write by what it does; a `fetch` of a call that writes does not compile; and a call whose effect is `write` is a document the drafter does not write. A call whose path has no placeholder is one fixed place, and a binding names its call as its place; the pack states nothing for the review summary. `testing::stand_set()` gives a `CompilerSet` with it, `testing::register_stand(&mut builder)` adds it to a builder, and `testing::example_root()` is the `example` target in `targets/example/`, which is written for it. A crate above names the feature under its dev-dependencies. It is never enabled in the product.

A crate whose tests need a whole pack, with a driver beside the compiler side, registers the parts itself through `a2m-provider`. The objects are `testing::surface_kind_spec()`, `control()`, `file_kind()`, `lowering()`, `lint_rule()`, and `drafting_rule()`. The names its manifest declares are `PACK`, `FOLDER`, `surface_kind()`, `call_kind()`, `mode()`, `verbs()`, `step_kinds()`, `lint_code()`, and `finding_kind()`. The module's own text says what a driver meets in a compiled target compiled with it. A test in `a2m-provider` registers the pack this way and compiles the `example` target to the same compiled target as `stand_set()`. The surface kind takes an optional `helper` program with a `helper_digest`, so that a test can put a declared program in a compiled target.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-actions` | `load`, `compile`, `lint_one` for `draft.lint`, `draft.try`, `test.run`, `version.publish`, `version.diff`, `target.show`; the `Drafter` for `draft.write` and the scout |
| `a2m-provider` | `CompilerSetBuilder`, to gather what packs and plug-ins register |
| Packs | The four registration traits and their read-only views. They build `Step` and `PackPayload` from `a2m-core`; a driver later decodes its own payloads without this crate |

The engine never uses this crate; the audit refuses the dependency. It reads compiled targets through `Control`, as `a2m-core` types.

It calls `a2m-core`, for the compiled target types and their encoding, and `a2m-expr`, and the registered traits.

## State

None that persists. It reads the developer's target folder and the drafter writes into it. The published compiled target is stored by `a2m-actions` through `a2m-ledger`. `lint_one` keeps parsed files in memory, keyed by digest, for the life of the engine.

## Failures

A compile reports `Diagnostics`, not a failure; the action that called it turns them into `input` with `lint_failed`. A compiled target that fails its hash check fails in `a2m-core`, not here.

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
| The same sources and compiler give byte-identical compiled targets, on every machine | The `example` target compiled twice, from disk, from memory, from a copy in another folder, and with other line endings, gives the same bytes. CI runs it on three operating systems |
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
| What `compile` emits decodes, through `a2m-core`, to an equal `CompiledTarget` with the same hash | A round-trip test over the `example` target |
| A pack is handed another pack's entries only when it declared the read | A test with a second lowering that declared none |
| A program a surface declares reaches the compiled target, and one named by a relative path does not compile | Tests with the test pack's helper program, and with a surface kind whose address is the program, in the surface and in an environment |
| A run-time value is judged by the rules a file is, and each kind of misfit is refused with the path of its field and without the value | `schema::tests`, over named values and over JSON, and one case that compares the words with a file's |
| A record is one fixed place only where its file kind declares that place fixed in the target's document, every binding names it, and no input is its key | `a_write_of_one_fixed_place_compiles_with_no_identity_input`, `a_fixed_place_is_only_one_its_file_kind_declares_and_no_input_chooses` |
| The review summary shows what a pack states, and a pack that states nothing is shown as stating nothing, never as nothing | `the_summary_shows_what_a_tool_starts_sets_and_reaches_by_the_kind_of_place`, `a_pack_that_states_nothing_is_shown_as_stating_nothing_and_never_as_nothing`, `a_sign_in_shows_its_provider_its_credentials_by_name_and_where_it_sends`; the whole order, `the_summary_is_ordered_by_hazard_then_effect_then_exposure_then_name` |
| Each read a pack's file asserts is one item of the summary, with its request, its fields, and the fixed sentence; a target that asserts none has none | `each_asserted_read_is_one_item_with_its_request_its_fields_and_the_sentence` |
| An input of the type `url` compiles only where its operation declares a destination with a scheme, and its compiled field type keeps the name `url` | `kernel.url_without_destination` in the table; `a_url_input_compiles_with_a_destination_and_keeps_its_type_by_name` |
| A description that addresses its reader is warned of by each of four closed lists of patterns, and plain descriptions are not | `addressed::tests`, `each_rule_catches_its_patterns` and `plain_descriptions_pass`; a case of each code in `every_kernel_lint_code_has_a_case_that_fails_with_it_and_the_example_passes`; `a_description_that_addresses_its_reader_is_a_warning_and_still_compiles` |
| A probe not yet drafted compiles in a draft only, and is marked | `a_sign_in_may_name_its_probe_before_the_read_is_drafted_in_a_draft_only`; in `a2m-core`, `a_probe_not_yet_drafted_stands_only_in_a_draft` |

## Build order

1. Done: the YAML parser verified, and the schema check chosen. Kernel schemas, `load`, `Diagnostics`. Package B4.
2. Nothing here for package 1.1: its hand-written compiled target uses the compiled target types in `a2m-core`.
3. Done: the registration traits and `CompilerSet`; stages 1 to 9, proven through the test pack `stand`. Real packs now register their kinds and lowerings through the provider registry: the web pack its screen and request kinds (`crates/a2m-pack-web-ui`), and the command, data, and desktop packs theirs. Not done: a compiled target compiled from real target files through the web pack is not yet pinned by a hash on three operating systems. Package 1.3.
4. Done: field types compiled to normalizers, built in and from `types.yaml`. Package 3.4.
5. Done: identity strategies for `natural` and `composite`, commit-point counting for writes, and the write lint rules. Not done: the long form of `identity` for `stamped` and `find` has only its strategy and inputs; the stamped field and the lookup are written with the first operation that needs one. Packages 3.3 and 3.8.
6. Done: pipelines, and sources compiled to `SourceSpec`. `a2m-sources` uses these types and has no copy, and a test there compiles the `example` target and reads its fixture sheet. Package 3.9.
7. Done: the drafter and its rules for what a draft may hold, proven with the test pack's drafting rule. Not done: no pack's drafting rules exist. Package E4.
8. Done: three gaps the provider contract's builder found ([decision 0124](../../decisions/0124-the-plan-declares-a-surfaces-programs-and-a-schema-checks-run-time-values.md)): a surface kind's program fields and the compiled target's declared programs; run-time values and JSON checked against a `JsonSchema`; and the test pack's parts made public.

9. Done: what the safety audit found ([decision 0153](../../decisions/0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md)): the rules of a write the compiler holds by itself, with `StepCommits`, `Lowering::commits`, and `Lowering::least_effect`; the Scope rule's form; the drafter's writing and what it may draft, with `FileKind::writes`; the loader's bounds; the credential lint's words and what a diagnostic never quotes.

11. Done ([decision 0193](../../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)): `kernel.acknowledged_where_read_back`, a binding whose record has a key and whose read-back exists is never ended on an acknowledgement, the compiled target's check asks the same `acknowledgement_may_stand`; `open_question_quoting`; surface notices.
10. Done ([decision 0161](../../decisions/0161-the-plan-and-the-engine-after-the-packs-were-built.md)): a binding's evidence (`Lowering::evidence`), held to `irreversible` for `acknowledged` by the least-effect check; a binding's steps may read `current` and `record`; a draft compiled under any version but `DRAFT_VERSION` is refused, `kernel.draft_under_release_version`; and a tool's output schema allows the text `[redacted]` wherever it declares a number, a date, a moment, or a length of time.

**Two rules every pack is held to** (decision 0161). *A delete is `irreversible`*, unless its binding declares how the record is restored, which no binding can declare yet: a pack's `least_effect` gives `irreversible` for every verb that deletes, and the compiler's least-effect check then holds every operation of it there. Today the file pack says `write` for its `delete`; the data, command, MCP, and HTTP packs say `irreversible`; the web, device, and stream packs have no delete. *An effect left as an open question is undecided, not a write* ([decision 0178](../../decisions/0178-an-effect-left-as-an-open-question-is-undecided-not-a-write.md), which changes this half of decision 0161): `FileKind::writes` is true for an entry declared to write and false for one whose effect is an open question, so the drafter may write it; nothing can use it, because the schema checker refuses an open question wherever it stands, and the file does not compile until a person answers. A pack may be stricter where it has evidence of its own, as the HTTP pack is for an unsafe method.

11. Done ([decision 0179](../../decisions/0179-what-each-pack-drafts-so-that-a-scout-ends-with-reads-that-work.md)): the built-in field type `untrusted_text`, `text` marked untrusted, exported as `UNTRUSTED_TEXT`, so that a drafting rule marks an output of the system's text without a `types.yaml`. It is the last of the built-in types, so a target's own types still follow them in the compiled target.

12. Done ([decision 0199](../../decisions/0199-the-halves-left-for-each-other-are-joined.md)): `CompilerSet::address_key`; a surface whose address is blank is refused as stating none; and the warning `kernel.acknowledged_with_lookup`, always a warning, for a binding whose evidence is `acknowledged`, on a keyed record, with no read-back step and a lookup: the engine asks the lookup only whether the record landed, and a lookup that returns the record's values is a read-back by another name. The test pack's lowering takes `lookup: {call}`. No first-party pack's lowering makes such a binding: their compile tests pass unchanged.
13. Done ([decision 0200](../../decisions/0200-evidence-names-its-binding-and-a-recorded-fixture-says-nothing-of-the-original.md)): a test may name the binding it tests, `binding: {mode, position}` (position counted from 1 among the operation's bindings, needed where two share the mode), refused as `kernel.test_binding_unknown` where none, two, or a retired binding match, or the test is of a pipeline; `Compiled::test_bindings` holds the place for each test of the compiled target, in its order, since `a2m-core`'s `CompiledTest` has no field for it. The drafter refuses a drafted binding of any status but `draft` (`a_binding_is_drafted_as_draft_and_never_as_verified`), and, building the check [decision 0203](../../decisions/0203-two-packs-may-each-draft-a-file-kind-of-one-name-for-their-own-surfaces.md) left owed, a drafted document of a pack's kind whose surface (the one it names, else the target's first of a kind the file kind is registered for) is of a kind its rule's own pack did not register the kind for, by name, where it is placed (`a_pack_drafts_a_kind_of_a_shared_name_only_for_its_own_surfaces`). `testing::file_kind_for` and `surface_kind_spec_for` give a second pack's registration of the stand's kind.

14. Done ([decision 0252](../../decisions/0252-a-fixed-place-a-review-that-shows-what-runs-and-a-sign-in-before-its-probe.md)): a record that is one fixed place (`identity: {strategy: natural, place: fixed}`, `FileKind::fixed_places`, `Lowering::fixed_place`, `kernel.place_not_fixed`; the test pack's call with no placeholder is one); the review summary's typed fields and `Lowering::statement`, in the module `review`; a sign-in whose probe is not drafted yet, in a draft only (`CompiledProbe::Pending`, `kernel.probe_pending`). Not done: the place's own name is not in the compiled target, so the ledger keys such a record by the operation and its place in the run; no first-party pack states anything for the summary yet.

15. Done (package E7d, [decision 0426](../../decisions/0426-the-publish-page-names-each-asserted-read-after-the-sign-ins-from-what-each-file-kind-says-it-asserts.md)): `FileKind::asserts`, empty by default, and the summary's `asserted`, one `AssertedReview` for each read a pack's file asserts, with `ASSERTED_NOTICE`. The web pack's `page_request` kind and the HTTP pack's `endpoint` kind answer. Test: `each_asserted_read_is_one_item_with_its_request_its_fields_and_the_sentence`; through the product, `crates/anything2mcp/tests/asserted_publish.rs`.

Not built, in any step:

- **The rest of the Scope rule.** The compiler checks the form of what an operation's `permissions` declares: one exact address, one name, once. It does not check a destination against a list a person registered for the target, because no file holds such a list; the review summary shows every destination, and confirming a new one is the publish gate's ([section 502](../../spec/5-security/502-defending-against-a-manipulated-agent.md)). It does not check that an upload or a download names an input of the operation or a root of the installation.
- **A secret outside what the credential lint knows.** A diagnostic never quotes a value under a key the lint knows, or one in a credential's shape. A token pasted under any other key, where a name or a word is expected, is still echoed, up to 64 characters. A fixture's recorded file is looked through by line for key material, an authorization header's value, and a signed token, and for nothing else; one that is not text is not looked through.

- `lint_one` compiles the whole target again; nothing is reused by file digest.
- An operation's outputs by reference to a pack's schema, and nested records.
- The action registry's reading and rewriting of a binding by its place in the file (`binding promote`), and the proof of a live read after a promotion. The provider registry gives `CompilerSetBuilder::mode_rank` each rank its manifest states for a mode it brings a lowering for, and the compiler orders an operation's bindings by rank unless it states `binding_order: listed` ([decision 0337](../../decisions/0337-a-description-says-who-wrote-it-a-drafter-reports-what-it-left-undrafted-and-bindings-are-ordered-by-rank.md); `crates/a2m-provider/tests/ranks.rs`).
- The `stands` rule for surfaces that may sit under an application. The compiled target carries where a surface stands; refusing a write on a surface that does not say is the rule of the pack whose kind needs it, and no such pack is built.
- Lint rules that are errors only on a server.
- The count of an operation's examples below one: an operation may have none.
- A check that an auth provider knows each setting's name. The compiler holds no list of auth providers; the provider's own check refuses an unknown name when the version is published.

## Open points

- `Verb` is a checked name in `a2m-core` for now; whether it should stay a closed list is open there. Either way, a verb that no lowering registers does not compile.
- How a plug-in registers file kinds and lint rules across a process boundary: as data at start, most likely, with its lowerings answered over the plug-in protocol. `JsonSchema` and `SurfaceKindSpec` are already data; the traits are not.
- Whether a lint rule may be silenced, and where the reason is written.
- Where drafts are written inside a project, and whether they are kept apart from files a person wrote. Today they are written among them and told apart by their first line.
- Settled: everything the run time needs from the files is in the compiled target, and what only changes what the compiler emits is applied by the compiler ([decision 0121](../../decisions/0121-the-plan-carries-what-the-run-time-needs-from-target-files.md)). The step "Done" for it is this list's: `auth.yaml`, the target's own facts, an environment's test records and approval rule, a surface's `stands`, an operation's examples and decision points, a field type's format.
- The names under `defaults.timeouts` are whatever a pack's lowering asks for. No pack exists to say which; the first real pack fixes its names in its own section.
- A reader's name is checked for shape only; `a2m-sources` knows which readers exist.
