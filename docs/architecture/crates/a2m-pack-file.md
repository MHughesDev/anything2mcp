# `a2m-pack-file`

The file pack: the `file` driver. Values are read from and written to named places in files under declared roots. A write patches the bytes of the places named, proves nothing else changed, and replaces the whole file in one step. The pack is designed in [spec section 308](../../spec/3-packs/308-the-file-pack.md); this document says how its crate is put together.

## Facts

| | |
| --- | --- |
| Side | Pack: file |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (plan, steps, values, findings), `a2m-expr` (the normalizer of a field type), `a2m-files` (file kind, lowering, lint, and drafting registration), `a2m-sources` (the parsed form with positions, `typed`, the JSON document tree), `a2m-auth` (the lease type in the driver's signature; the pack holds no session), `a2m-provider` (`Driver`, `Discoverer`, `Describer`, `Roots`, `CheckedPath`, registration), `a2m-telemetry` (the clock for a finding's time). Outside: `tokio`, `serde`, and `serde_json`, all in the workspace already. Tests only: `a2m-ledger`, `a2m-vault`, and the stand `example-files` |
| Roadmap packages | Packages H2 and H3 of the [roadmap](../../plan/roadmap.md) |
| Carries out | Spec section [308](../../spec/3-packs/308-the-file-pack.md); [201](../../spec/2-kernel/201-archetypes-and-modes.md) (artifact); [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) (no input chooses a file); [401](../../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md), rule 4 (a file beneath an application) |
| Decisions | [0102](../../decisions/0102-the-file-mode-and-what-the-file-pack-registers.md), [0103](../../decisions/0103-the-file-pack-as-building-it-settled-it.md), [0149](../../decisions/0149-the-file-pack-after-its-safety-audit.md), [0173](../../decisions/0173-the-stream-and-file-packs-take-up-the-kernels-second-pass.md), [0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md) |

## Owns

- The mode `file` and the surface kind `file`: a folder inside a named root.
- The `documents/` file kind: its schema, and its compilation into the pack's section of the plan.
- The lowering of `read`, `list`, `update`, `create`, and `delete` into the steps `open`, `locate`, `set`, `read`, and `save`.
- Reading a file once, parsing it through `a2m-sources`, checking its layout, and finding named places.
- The patch editors for delimited text and JSON, the untouched check, and the save.
- The `file_layout` and `file_key_candidate` finding kinds, the discoverer that makes them, and the rules that draft a document and the reads over it from a layout.

It must never: resolve a path, or build one from anything but the surface's checked folder and one checked name; follow a link at a file's own name; let an input reach a file name or a place; edit a file in place; write a format it has no patch editor for; calculate a formula or open a macro; replace a file marked read-only, or one the engine's user may not write; change a value to make it safe; retry; remove, open, or write a file it did not make itself, whatever its name.

## What it registers

| Registration | Content |
| --- | --- |
| Manifest | `anything2mcp-provider.toml` in the crate, embedded. Provider `file`, kind `pack` |
| Mode | `file`, artifact; verbs `read`, `list`, `update`, `create`, `delete`; steps `open`, `locate`, `set`, `read`, `save`; `exclusive = "file"` |
| Surface kind | `file`. Address key `folder`, a relative folder with forward slashes, or `.` for the root itself. Optional `root`, a root's name, `files` when absent. In the root `project` the folder is below the project folder of the request that started the run (`CheckedPaths::project_folder`), never below the pack's root of that name, which is the folder the engine was started in; it is read and never written, as the manifest declares; a run with no project folder stops with `project_folder_missing` |
| File kind | `document`, folder `documents/`, for `file` surfaces. A document is named by its file's name without the extension |
| Finding kinds | `file_layout`, `file_key_candidate` |
| Discoverer | `file_layout`: artifact, `file` surfaces, source `live_system`, at most `discovered` |
| Drafting rules | `file_layout` to `document`; `file_layout` to `operation`: a `list` read and a read by key for each record kind of delimited text or a workbook ([decision 0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md)) |
| Effects | Reads the roots `project` and `files`; writes the root `files`. No process, no network, no session |
| Settings | `packs.file.max_bytes` (16 MiB), `max_rows` (200,000), `max_cells` (2,000,000), `max_cell_bytes` (65,536) |

### The document

As in section 308, with these fields as built: `surface`, `file`, `format`, `dialect` (`delimiter`, `header`, `encoding`, `line_ending`), `fingerprint` (`sheets`, `tables`, `names`, `headers`, `keys`; a list that is absent is not compared), `may_be_open`, `lock_signal: {sibling}`, and `records`. `may_be_open` is `true`, `false`, or absent, and absent means the target has not said: it is never read as `false`. A record kind states one place: `rows: all` for delimited text; `path: [keys]` for JSON; `table: {sheet, name}`, `sheet: name`, or `cell: {name}` for a workbook. A field has `header` or `path`, a `type`, and optionally `writable`; in a workbook, `formula`; in delimited text, `may_begin_formula`. A key field is never writable. Rows, a table, or a sheet with no `key` can be listed and nothing else: the lowering refuses `read`, `update`, `create`, and `delete` over them. A field of the built-in type `untrusted_text`, which a drafted field holds until someone states its type, is never a key and never `writable`.

### The binding

`document`, `record`, `key` (a value for each key field), `fields` (a value for each field set), `limit` (for `list`), and `read_back: {record}`.

### Lint rules

| Code | Requires |
| --- | --- |
| `file.stands_undeclared` | A binding that writes is on a surface that declares `stands` |
| `file.beneath_application` | A write beneath an application is `irreversible`, and `target.yaml` names the authorization record: a reference that is not empty. What the record covers is outside the target's files, and the rule does not see it |
| `file.open_undeclared` | A write names a document that states `may_be_open`, `true` or `false` |
| `file.open_without_signal` | A write to a document with `may_be_open: true` has a `lock_signal` |
| `file.format_read_only` | A write is to delimited text or JSON |

The file kind and the lowering also report `file.document_invalid`, `file.format_not_built`, and `file.binding_invalid`, each with a file, a line, and a fix.

## Interface

```rust
pub const MANIFEST: &str;                 // the manifest's text
pub struct FilePack;                      // implements the provider contract's Pack
pub const fn pack() -> FilePack;          // the product crate hands it to the registry builder's add_pack

pub mod codes;                            // the pack's failure codes, and ALL with the class of each
pub struct FileLimits;                    // from packs.file
pub enum Format { Delimited, Json, Workbook }
pub struct LayoutFound; pub struct TableFound; pub struct KeyCandidateFound;   // what the findings hold
pub const FILE_LAYOUT, FILE_KEY_CANDIDATE: &str;
pub const FILE_DIGEST, BEFORE_DIGEST, INTENDED_DIGEST, ROWS, TRUNCATED: &str;  // names in a step's result
pub fn document_schema() -> JsonSchema;
```

Everything else is private. The driver, the lowering, the file kind, the discoverer, and the drafting rule are reached only through the registry.

### How the mode fills the loop

| Part | Step | What happens |
| --- | --- | --- |
| Prepare | | The surface's folder is asked of the pack's roots, for writing where the root allows and for reading otherwise, with the run's checked paths, so a root that stands for the project folder of each request (`Roots::add_project`) is resolved in the run's own project, with every refusal of a root, and a run with none stops with `project_folder_missing` ([decision 0194](../../decisions/0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md)); in the root `project`, of the run's project folder, for reading. The pack's section of the plan is decoded, and the field types of this surface's documents are loaded |
| Act | `open` | The file is the document's one name in the folder: the name itself, never what a link at the name points to. A link at the name, anything that is not a regular file, or more than one link on Unix: refused. The handle that is opened and the name must show one identity. With `for_write`: a document that does not state `may_be_open` is refused, as the compiler refuses it; a link in the way from the root to the folder is refused; anything at the lock signal's name, a link to nothing included, is `busy`. The bytes are read once, on a blocking thread, within `max_bytes`; digested; parsed; and the layout is compared. Returns `file_digest` for reconcile |
| | `locate` | The record kind's rows are found in the parsed form. Every key in the table is typed, so a key that is there twice is `key_not_unique`. A lookup that finds nothing is `Absent`; a `create` that finds the key is `stale` |
| | `set` | Builds the patches, applies them, parses the new bytes, runs the untouched check, and reads the named places back from the new bytes. Returns `before_digest` and `intended_digest` for reconcile |
| | `save` | The commit point. See below |
| Confirm | `read` | The file at the path has the intended digest |
| Read back | `open`, `locate`, `read` | The file is opened again from disk; the values are typed by the same `typed` a source uses |
| Release | | Nothing to remove: a save's temporary file is the contract's, removed by the save when it does not finish |

**The save.** Everything is looked at again, at the name itself. No link stands in the way from the root to the folder. The target is a regular file with one link, not a link, not marked read-only. The engine's user may write the file itself, because a replace needs only the right to write the folder: the contract's `CheckedPath::may_write` asks the operating system, writing nothing and following no link. No lock signal is there.

The new bytes go to the contract's temporary file, made by `CheckedPath::create_temporary` in the same folder ([decision 0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md)): under a name of 128 random bits, so nobody can put something at the name beforehand, only where nothing stands, never through a link, and readable and writable by the engine's user alone when it is made. `Temporary::keep_owner_and_mode` gives it the old file's owner, group, and permission bits through its own handle before a byte is written, so the new bytes are never readable by anyone the old file was not, and again after the write; then it is flushed. On Unix an owner, a group, or bits that cannot be given, and a set-user, set-group, or sticky bit, which is never copied onto a file the engine wrote, are `metadata_not_kept`, unsent. On Windows the new file takes the access list its folder passes down, and the answer is yes only where the old file, opened itself, has that very owner and access list; a file with an access list of its own is `metadata_not_kept`, unsent, as on Unix, so it is never replaced with a wider one ([decision 0193](../../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md), [decision 0194](../../decisions/0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md)). The read-only mark is kept, and a file marked read-only is never replaced. The file is then read back through its own handle and must hold exactly the bytes built; otherwise the save stops, unsent.

Then, with nothing else between them: the last look at the lock signal, the digest of the file at the name compared with the digest that was read, and `Temporary::replace`, one `rename` that replaces only what stands at the name and refuses a link there, followed by `CheckedPath::flush_folder`. A flush that fails after the rename leaves the save's fate unknown, so that reconcile reads the file rather than call a name that may not outlast a power cut saved. A digest other than the one read is `stale`. What stays between the comparison and the `rename` is the time of that one call. If the rename fails, the target is digested once more: the intended digest is success; the old digest is a named failure with the request answered; anything else has an unknown fate, which the handle makes `possible_commit`.

A temporary file that is not put in place is removed by the contract when the save lets it go, on every way out, and only while its name still holds the file it made, by the identity of its own handle. A temporary file left by a save that was killed is never used and never removed by a later run, whatever its name. It stays until a person removes it.

**The editors.** A delimited field keeps its quotes and gains them only when its text would end the field or the row. A row is added at the end with the line ending the file shows, and the last line stays ended or unended as it was. A JSON token is replaced by a token of the same kind; a new element is written on one line, set apart as the last element is. A value that already agrees is not rewritten.

**Text that would run as a formula.** A spreadsheet program that opens delimited text runs a field that begins with `=`, `+`, `-`, or `@` as a formula, and quoting the field does not stop it. `set` refuses such a text value, and a new row's key too, with `value_begins_formula`, before anything is built. The full-width forms of the four signs count, and so do spaces before a sign. A value is never changed to make it safe, because what is read back must be what was written. A field declared `may_begin_formula: true` may begin with a sign: the developer says by it that the column is free text and that no spreadsheet program opens the file. What protects the person then is that declaration in the reviewed target, and the value shown in the diff they approve. A tab or a line break at the start is refused even there. A number is spelled by the pack itself and is not refused for its sign. JSON is not opened as cells, and the rule does not apply to it.

### The entry bar

| # | Item | Where |
| --- | --- | --- |
| 1 | A driver, no retries inside | `driver.rs`; one step, one attempt |
| 2 | A read-back rule for every write | `open`, `locate`, `read` on a new read of the file |
| 3 | Each identity strategy | `natural` and `composite` possible; `stamped` and `find` refused by the lowering |
| 4 | A test stand with ground truth | `targets/example-files`; `tests/stand.rs` |
| 5 | A discoverer that claims no more than it saw | Layouts `discovered`, key candidates `inferred` with two limitations |
| 6 | Failures mapped to the classes | Failures, below |
| 7 | Effects and hazards | The manifest's roots. The lowering sets no hazard; the operation declares it |
| 8 | A decision record for the mode | [0102](../../decisions/0102-the-file-mode-and-what-the-file-pack-registers.md) |

## Used by, and uses

Used by the product crate only, which hands the pack to the registry. Uses the provider contract and the layers below it. Of the contract's save in steps it uses all of it: `may_write`, `entry_kind`, `create_temporary`, `Temporary` (`keep_owner_and_mode`, `write`, `flush`, `read_back`, `replace`), and `flush_folder`, and maps the codes `path_missing`, `path_denied`, `path_locked`, and `disk_full` to its own `file_missing`, `file_protected`, `file_locked`, and `storage_full`, so that no two of its codes mean one thing; a folder the temporary file cannot be made in is `path_not_allowed`, as before ([decision 0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md)). The folder is a `CheckedPath`, and `is_dir` looks before every use that it is still the folder that was checked. `join` checks a document's name: its form, and that a link at it does not lead out of the root. The operating-system calls are given the folder's `as_path` with that one name added, not the path `join` resolved, because `join` follows a link that stays inside the root and these calls must not: a look at the name itself, an open whose handle is then compared with the name, a create that fails where anything has the name, and a `rename`. The discoverer reads the files the kernel hands it through `CheckedPath::read`.

## State

None of its own outside a prepared driver. A prepared driver holds the checked folder, the loaded documents, the file last opened with its parsed form, the place last located, and the new bytes between `set` and `save`. No file handle is kept between steps.

## Failures

| Class | Code | When | Sent |
| --- | --- | --- | --- |
| `input` | `path_not_allowed` | A path outside the root, a link out, a link at the file's own name, a link in the way to the folder of a write, not a regular file, more than one link, a folder that cannot be written | Not sent |
| `input` | `project_folder_missing` | A surface in the root `project`, and a run whose request came with no project folder: the contract's own code | Not sent |
| `input` | `file_unavailable` | The file cannot be opened, read, or replaced for a reason with no name of its own | Not sent, or answered |
| `input` | `payload_unreadable` | The plan's section or a step's payload does not decode, or steps arrive out of order | Not sent |
| `input` | `value_unparsable`, `value_out_of_range` | A key or a field value its field type refuses | Not sent |
| `input` | `value_begins_formula` | Text for delimited text that a spreadsheet program would run as a formula, or that begins with a tab or a line break | Not sent |
| `policy` | `file_protected` | Encrypted or password-protected; marked read-only; not writable by the engine's user | Not sent |
| `policy` | `file_too_large` | Beyond the limits of `packs.file` | Not sent |
| `policy` | `effect_not_declared` | A save into a root the pack may only read | Not sent |
| `drift` | `file_missing`, `file_unreadable`, `place_not_found`, `key_not_unique`, `fingerprint_mismatch` | As section 308 | Not sent |
| `drift` | `round_trip_failed` | The untouched check fails | Not sent |
| `unknown_variant` | `formula_at_place` | A formula where a value is declared | Not sent |
| `unknown_variant` | `value_unreadable` | A place holds what its field type cannot read | Not sent |
| `unknown_variant` | `shape_unsupported` | A shape with no rule: a date to write, a nested member to add, a row to add where no line ending is shown or declared | Not sent |
| `unknown_variant` | `metadata_not_kept` | The owner, the group, or the permission bits cannot be carried over | Not sent |
| `busy` | `file_locked` | The lock signal is present; the system refuses to share the file | Not sent at `open`; answered at the replace |
| `busy` | `storage_full` | The disk or folder is full | Not sent |
| `stale` | `changed_since_preview` | The file changed after it was read; a key to create is already there; the confirm finds another file | Not sent; answered at the confirm |
| `transient` | `timeout` | The step's deadline had passed | Not sent |
| `possible_commit` | `no_confirmation` | The replace was called and the file is neither the old one nor the new one, or the folder could not be flushed after it | Unknown |

No message holds a value from a file.

## Invariants

Each is proven in `tests/` against the stand, which reports the bytes on disk through readers of its own.

| Invariant | Test |
| --- | --- |
| A registry with only this pack builds, and its digest is stable | `pack.rs` |
| Every code the driver returns is declared with its one class | `pack.rs` |
| The fixtures compile twice to the same bytes | `pack.rs` |
| What the pack cannot carry out does not compile, with a file, a line, and a fix | `pack.rs`, eleven cases |
| An authorization key that names no record authorizes no write beneath an application | `pack.rs` |
| A value is read from a named place in delimited text, JSON, and a workbook; five reads give one outcome | `stand.rs` |
| A changed layout, a formula at a value, a protected file, a broken file, a missing file, a link out, and a repeated key each stop with their own name | `stand.rs` |
| An update changes one span and nothing else; the stand counts one write, a replace | `stand.rs` |
| A value that already agrees leaves the file byte for byte the same | `stand.rs` |
| A byte-order mark, CRLF, an unended last line, and quotes survive | `stand.rs` |
| A row and an element are added and removed, and every other byte stays | `stand.rs` |
| A file changed after it was read is `stale` and is not replaced | `stand.rs` |
| A save stopped half way leaves the old file whole, and resumes to the new one with nothing of its own left behind; the leftover it did not make is where it was | `stand.rs` |
| A lock signal, a read-only mark, a second link, and a read-only root each stop the write before anything is written | `stand.rs` |
| A link planted at a temporary file's name costs no other file: the other file, and the link, are as they were | `stand.rs`, `a_link_planted_at_a_temporary_files_name_costs_no_other_file` |
| A temporary file is the engine's user's alone when made, is used only when it holds what was built, and is removed unused, leaving the old file as it was | `save.rs`, unit test |
| A document whose file is a link is refused for a read and a write, before and at the save; a write follows no link to its folder | `stand.rs`, `a_document_whose_file_is_a_link_is_refused_and_a_write_follows_no_link` |
| A write does not compile, and does not run, when the document does not say whether its file may be open | `pack.rs`; `driver.rs`, unit test |
| Of two runs on one file, the second to save is `stale` and the first run's change stays | `stand.rs`, `of_two_runs_on_one_file_the_second_to_save_is_stale` |
| A confirm that finds another file is `stale` with the request answered | `stand.rs` |
| Text a spreadsheet program would run is refused and never altered; a declared column takes it as given; a delimiter and a quote inside a value are quoted and read back | `stand.rs`, `text_a_spreadsheet_program_would_run_is_refused_and_never_altered`; `edit.rs` and `driver.rs`, unit tests |
| A file the engine's user may not write is not replaced; the new file has the old file's owner, group, and bits | `stand.rs`, `a_file_is_replaced_only_by_one_who_may_write_it_and_keeps_its_owner_and_mode` |
| A file whose bits cannot be kept, a set-group bit, is not replaced and keeps its identity, mode, and owner; nothing is left | `stand.rs`, `a_file_whose_bits_cannot_be_kept_is_not_replaced` |
| The drafted `list` compiles, returns every row of the stand's file, column by column, as text marked untrusted, a field that begins with `=`, `+`, `-`, or `@` as the text it is; nothing is written; a read by key holds the questions, and a write over the drafted document does not compile and says its rows have no key | `stand.rs`, `the_drafted_list_reads_every_row_as_marked_text_and_a_write_waits_for_the_key` |
| A lock signal that is a link to nothing is a lock signal, before the read and at the save | `stand.rs` |
| After a refused replace the file's own bytes decide: new, old, or in doubt | `save.rs`, unit test |
| Discovery writes nothing, stores no value, and a draft holds no key and no type the file did not state: a cell of delimited text or a workbook is the text it holds, `untrusted_text`, and a JSON value's type is a question; an open question holds no text from a file | `stand.rs` |
| A document in the root `project` is read from the project folder of the run, one folder or another, and never from the folder the engine was started in; a run with no project folder stops by name | `stand.rs`, `a_project_document_is_read_from_the_project_folder_of_the_run` |
| Five reads say the same of how each step was done | `stand.rs`, `five_reads_say_the_same_of_how_each_step_was_done` |
| A delete is `irreversible`, and is still read back | `pack.rs`, `a_delete_is_irreversible_because_nothing_restores_the_row` |
| A document whose writability is an open question describes a write, and is never drafted | `pack.rs`, `a_drafting_rule_that_drafts_a_document_that_writes_is_refused` |
| The contract's codes come back as the pack's own; a temporary file is used only when it holds the bytes built | `save.rs`, unit tests |

Not proven here: a file held open without sharing, which only Windows shows. `a_file_held_without_sharing_is_busy` runs it there and ends early elsewhere. CI on Windows must show that test passing, and that the save tests pass with `rename` over an existing file.

Also not proven, and why:

- **An owner or a group the engine's user may not give.** The stop is `metadata_not_kept`. A test would need two accounts. The superuser, who can make such a file, can also give it back, and the owner test shows that; as an ordinary user the test shows the file is not replaced at all. The same stop for a bit that is never carried, a set-group bit, is shown on any Unix.
- **A replace the operating system refuses.** Nothing portable makes `rename` fail at a chosen moment. The rule that decides what the file's bytes then mean is one function with a unit test; the calls around it are not exercised.
- **The engine killed between the replace and its record,** case 3 of section 308. The test reads the file again with a new driver and finds the intended digest. The row of reconcile where the file is neither old nor new is the engine's, and is not tested here.
- **The parts that differ outside Unix.** The identity of a file there is the moment it was made and its length, a link can be made only by a user who may, and owner, group, and bits do not exist. The pack's tree compiles C through the ledger, so it cannot be linted for Windows or macOS from Linux; the stand can, and was. CI must show the tests passing on both.

Known limits, stated and not closed:

- **Extended attributes and access lists are not carried.** The new file has the old file's owner, group, and permission bits and nothing more. On Windows it takes the folder's access list, and a file whose own list differs from it is not replaced: the save stops with `metadata_not_kept`. Not run here: CI's Windows job must show the save tests passing, which means a file that holds its folder's list is told as kept. Safe code is not given either, so the pack cannot carry them or tell that a file has them.
- **A check and the call it guards are two calls.** Between the look at a name and the open, the create, or the rename, another program that may write the folder can change what is at the name. The open is caught by comparing the handle with the name; a create follows no link; a `rename` replaces a link itself and not what it points to. The folder is checked by path, not held open, so a folder swapped for a link in that moment is not caught.

## Build order

1. Done: the manifest and registration.
2. Done: the `document` file kind, the lowering of five verbs, four lint rules, and the fixtures in `tests/fixtures/example/`.
3. Done: the driver's reads, from delimited text, JSON, and workbooks.
4. Done: writes to delimited text and JSON: `update`, `create`, `delete`, the untouched check, the save, the confirm, the read-back, and digests for reconcile.
5. Done: the discoverer, the describer, and the drafting rule for documents.
5a. Done: what the kernel holds the pack to after the safety audit ([decision 0155](../../decisions/0155-four-packs-state-what-saves-what-writes-and-when-their-guard-lifts.md)). `Lowering::commits`: `save` is `Always`, and `open`, `locate`, `set`, and `read` are `Never`. The manifest lists `commit_steps = ["save"]`. `Lowering::least_effect`: `update`, `create`, and `delete` are `write`; `read` and `list` are `read`. `FileKind::writes`: a document describes a write when a field is marked `writable` or the document states `may_be_open` or `lock_signal`; the drafter writes none of these. The driver now keeps the run's posture and refuses a `save` unless the posture is live, or a sandbox with test records, and the step is marked `commits`. `FileDriver` and `DocumentKind` are exported so that a test can reach the driver below the handle and build a drafting rule. The pack takes the provider's `CheckedPath::entry_kind` for a document's own name and for a file handed to discovery. It keeps its own look at a lock signal, because the roots' check cannot join a name that is a link to nothing, and that name is a signal.
5b. Done: the kernel's second pass ([decision 0173](../../decisions/0173-the-stream-and-file-packs-take-up-the-kernels-second-pass.md)). Step inputs by `StepInput::new` and `for_record` in the tests; an `acting` capture on every step (`means`: the format and the part of the work; `measured`: the file's size). The save asks `CheckedPath::may_write` and flushes the folder with `CheckedPath::flush_folder`, reads its temporary file back before the last digest comparison, and maps the contract's codes to its own. A surface in the root `project` is the run's project folder. `Lowering::least_effect`: `delete` is `irreversible`; the fixtures say so. Not taken up, with the reason: `Prepared::recorded` (the read-back reads the places afresh and the engine compares their values with the intended and the earlier ones, which settles the three rows of section 308's reconcile table; the recorded digests would add nothing that decides), `with_said` (no message holds text from a file), keys (no step reads `StepInput::key`), `acknowledged` evidence (every write is read back), and expressions (no mapping key stands in for `current` or `record.key`).
5c. Done ([decision 0194](../../decisions/0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md)): the root of each request's project: `FileDriver::folder` resolves through `Roots::folder_in` and `check_in` with the run's checked paths; one registry serves two project folders and each run reads and writes its own, and a link from one into the other is refused (`a_root_of_each_requests_project_resolves_in_that_project_and_never_another`). The save stops with `metadata_not_kept` on Windows too where the target's owner and access list cannot be kept. The save's last looks are held to the file the save began with: the target's identity, by path, is taken before the temporary file is made and must be the same just before the replace, and after it the path must lead to the file the save wrote; otherwise the save stops as changed, unsent, or is unknown after the replace (FI-1; `a_folder_swapped_before_the_last_look_stops_the_save_unsent`). The lock signal, the digest, and the folder's flush still go by path: a folder swapped in the few calls between the last look and the replace is told by the identity after it, and the flush may then reach the folder at the path, not the one renamed in; closing that needs the contract to digest and flush through the temporary file's folder handle.
5c. Done: the first drafted reads ([decision 0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md)). A delimited or workbook cell is drafted `untrusted_text` with no key; beside the document, a `list` read per record kind that compiles, and a read by key that holds the questions. A record kind of rows may state no key, and is then only listed; `untrusted_text` is never a key and never written. The `list` step gives each field as a list beside `rows` and `truncated`. The save moved onto the contract's `Temporary`, and the pack's own temporary file, its token, and its removal at release are gone. Not proven here: a drafted workbook list over a table with formulas stops by name, `formula_at_place`, since the drafter does not know which column holds them; the drafted document names no surface, and lands on the first file surface of the target.
5d. Done: the drafted document names the surface its file was observed on, which the `file_layout` finding now carries (`surface`), and a drafted workbook is typed by what its cells hold ([decision 0188](../../decisions/0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md)). Tried against the stand, the drafted workbook list had stopped at `value_unreadable`, not `formula_at_place`: a workbook's number is not text. Discovery now records, for each column of a workbook's table (`TableFound::columns`, a `ColumnFound` each), the kind its cells hold (`CellKind`: `text`, `number`, `boolean`, or `mixed`, a formula counted as its kept result) and whether one holds a formula. The draft types the column `untrusted_text`, `decimal`, or `boolean`, marks a formula column `formula: true`, so that the list returns the result the workbook kept as section 308 says, and leaves a `mixed` column out of the document and the list, asking its type in the read by key. A formula another writer later puts where the document declares a value still stops with `formula_at_place`. Proven by `the_drafted_document_names_the_surface_its_file_was_observed_on` and `a_drafted_workbook_list_reads_each_cell_as_stored_and_a_formula_as_its_kept_result`. Not proven against the stand: a `mixed` column, which its seed does not hold. The tests' `outputs_of` stays, since a pack cannot call the engine; it normalizes a list item by item and keeps a `null` item, as the engine's does.
6. Not built: `upsert`; a list of rows in one save; workbook writes with the formula rule and the declared second place; YAML, TOML, XML, and text documents; writing dates, times, and measurements; removing a temporary file a killed save left, which no later run can prove it made; carrying, or refusing, extended attributes and access lists.
7. No auth: the pack holds no session.

## Open points

- **What the kernel lacked**, for hypothesis H-26: nothing in the plan names the files of a surface, so `PrepareContext.paths` is not used; a manifest lists roots by fixed name; `Observed` had no place for the trace's `means` and `measured` (it now has the `acting` capture); no public function spells a value as its field type shows it; `CheckedPath` hides the kind of an input-output error; it now makes a temporary file the save uses whole (`create_temporary`, `Temporary`, and `FileIdentity::of_open`, [decision 0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md)); it now asks whether the engine's user may write a file (`may_write`), which the pack takes. So the pack adds one checked name to the folder's `as_path` itself ([decision 0149](../../decisions/0149-the-file-pack-after-its-safety-audit.md)). While that was built, the contract gained `entry_kind`, which says what stands at a name and follows nothing, and an `open` that follows no link and reaches the folder from the root down; the pack has taken `entry_kind` where it can (a document's own name, a file handed to discovery); it should also open by path, which would also close the swap of a folder between a look and an act on Linux and macOS; `TargetView` does not show the authorization record; the JSON parser refuses a duplicate key and so cannot report one.
- **`key_not_unique` has two classes in the product:** `input` in `a2m-sources`, for a source, and `drift` here, for a target, as section 308 states.
- **The window between the last digest check and the replace** stays, as section 308 says. It is one call wide. Nothing in the engine or the scheduler reads a mode's `exclusive` setting yet, so `exclusive = "file"` is a statement and two runs on one file are held apart by the digest alone: the second to save is `stale`, and a second save that falls inside the window is not caught.
- **Whether `file` fitted the artifact archetype**, for H-29: the five steps were enough. The one bend is that `open` belongs to act and not to prepare.
