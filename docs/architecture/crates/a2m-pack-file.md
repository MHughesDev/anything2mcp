# `a2m-pack-file`

The file pack: the `file` driver. Values are read from and written to named places in files under declared roots. A write patches the bytes of the places named, proves nothing else changed, and replaces the whole file in one step. A folder is also read and written as a collection of entries, and a growing file is read from where the last read stopped ([decision 0246](../../decisions/0246-the-file-pack-reads-folders-settings-files-and-growing-files.md)). The pack is designed in [spec section 308](../../spec/3-packs/308-the-file-pack.md); this document says how its crate is put together.

## Facts

| | |
| --- | --- |
| Side | Pack: file |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (compiled target, steps, values, findings), `a2m-expr` (the normalizer of a field type), `a2m-files` (file kind, lowering, lint, and drafting registration), `a2m-sources` (the parsed form with positions, `typed`, the JSON document tree), `a2m-auth` (the lease type in the driver's signature; the pack holds no session), `a2m-provider` (`Driver`, `Discoverer`, `Describer`, `Roots`, `CheckedPath`, registration), `a2m-telemetry` (the clock for a finding's time). Outside: `tokio`, `serde`, `serde_json`, and `getrandom` (the random name of a new file's temporary file), all in the workspace already. Tests only: `a2m-ledger`, `a2m-vault`, and the stand `example-files` |
| Roadmap packages | Packages H2 and H3 of the [roadmap](../../plan/roadmap.md) |
| Carries out | Spec section [308](../../spec/3-packs/308-the-file-pack.md); [201](../../spec/2-kernel/201-archetypes-and-modes.md) (artifact); [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) (no input chooses a file); [401](../../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md), rule 4 (a file beneath an application) |
| Decisions | [0102](../../decisions/0102-the-file-mode-and-what-the-file-pack-registers.md), [0103](../../decisions/0103-the-file-pack-as-building-it-settled-it.md), [0149](../../decisions/0149-the-file-pack-after-its-safety-audit.md), [0173](../../decisions/0173-the-stream-and-file-packs-take-up-the-kernels-second-pass.md), [0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md), [0246](../../decisions/0246-the-file-pack-reads-folders-settings-files-and-growing-files.md), [0255](../../decisions/0255-the-file-pack-discovers-and-drafts-folders-settings-files-and-growing-files-as-reads.md) |

## Owns

- The mode `file` and the surface kind `file`: a folder inside a named root.
- The `documents/` file kind: its schema, and its compilation into the pack's section of the compiled target; with the formats `ini` and `property_list`, read by the pack's own readers in `settings.rs`.
- The `folders/` and `logs/` file kinds (`declare.rs`): a folder of a surface as a collection of entries, and a growing file; their lowering (`lower_folder.rs`), their steps (`drive_folder.rs`), and what they do to the file system (`folder.rs`, `log.rs`).
- The lowering of `read`, `list`, `update`, `create`, and `delete` into the steps `open`, `locate`, `set`, `read`, and `save`.
- Reading a file once, parsing it through `a2m-sources`, checking its layout, and finding named places.
- The patch editors for delimited text, JSON, INI files, and property lists, the untouched check, and the save.
- The `file_layout`, `file_key_candidate`, `file_folder`, and `file_log` finding kinds, the discoverer that makes them (`discover.rs`, with what it records of folders, settings files, and growing files in `evidence.rs`), and the rules that draft a document, a folder, a growing file, and the reads over each (`draft.rs`, `draft_kinds.rs`).

It must never: resolve a path, or build one from anything but the surface's checked folder and checked names (one name, or for a folder's read the declared depth of them); follow a link at a file's own name; let an input reach a file name or a place; edit a file in place; write a format it has no patch editor for; calculate a formula or open a macro; replace a file marked read-only, or one the engine's user may not write; change a value to make it safe; retry; remove, open, or write a temporary file it did not make itself, whatever its name; remove any other entry but by a folder's declared `delete`; follow a link at a folder's entry.

## What it registers

| Registration | Content |
| --- | --- |
| Manifest | `anything2mcp-provider.toml` in the crate, embedded. Provider `file`, kind `pack` |
| Mode | `file`, artifact; verbs `read`, `list`, `update`, `create`, `delete`, `rename` (a folder's entry only); steps `open`, `locate`, `set`, `read`, `save`; `exclusive = "file"` |
| Surface kind | `file`. Address key `folder`, a relative folder with forward slashes, or `.` for the root itself. Optional `root`, a root's name, `files` when absent. In the root `project` the folder is below the project folder of the request that started the run (`CheckedPaths::project_folder`), never below the pack's root of that name, which is the folder the engine was started in; it is read and never written, as the manifest declares; a run with no project folder stops with `project_folder_missing` |
| File kinds | `document`, folder `documents/`; `folder`, folder `folders/`; `log`, folder `logs/`; each for `file` surfaces, and each named by its file's name without the extension. A binding names one of them by `document:`, `folder:`, or `log:` |
| Finding kinds | `file_layout` (a settings file's sections, keys, and value shapes too), `file_key_candidate`, `file_folder`, `file_log` |
| Discoverer | `file_layout`: artifact, `file` surfaces, source `live_system`, at most `discovered`. It reads the files the kernel hands it; when it hands none, the surface's own folder, resolved from the pack's roots as the driver resolves it, for reading, and each regular file directly in it whose name the pack knows ([decision 0255](../../decisions/0255-the-file-pack-discovers-and-drafts-folders-settings-files-and-growing-files-as-reads.md)) |
| Drafting rules | `file_layout` to `document`; `file_layout` to `operation`: a `list` read and a read by key for each record kind of delimited text or a workbook ([decision 0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md)), and one `read` of each record kind of a settings file; `file_folder` to `folder` and to `operation` (a `list` and a `read` by name); `file_log` to `log` and to `operation` (a `read` from a position). None drafts a write ([decision 0255](../../decisions/0255-the-file-pack-discovers-and-drafts-folders-settings-files-and-growing-files-as-reads.md)) |
| Effects | Reads the roots `project` and `files`; writes the root `files`. No process, no network, no session |
| Settings | `packs.file.max_bytes` (16 MiB), `max_rows` (200,000), `max_cells` (2,000,000), `max_cell_bytes` (65,536) |

### The document

As in section 308, with these fields as built: `surface`, `file`, `format`, `dialect` (`delimiter`, `header`, `encoding`, `line_ending`), `fingerprint` (`sheets`, `tables`, `names`, `headers`, `keys`; a list that is absent is not compared), `may_be_open`, `lock_signal: {sibling}`, and `records`. `may_be_open` is `true`, `false`, or absent, and absent means the target has not said: it is never read as `false`. A record kind states one place: `rows: all` for delimited text; `path: [keys]` for JSON; `table: {sheet, name}`, `sheet: name`, or `cell: {name}` for a workbook. A field has `header` or `path`, a `type`, and optionally `writable` and `required`; in a workbook, `formula`; in delimited text, `may_begin_formula`. A key field is never writable. A place of a document may be empty, so every field that is not part of the key and is not declared `required: true` is an output that may be absent: the lowering writes those names into the payload of its `read` steps (`may_be_empty`), and `absent_outputs` reports them, so an empty cell reads as `null` and a `required` one that is empty stops the run with `output_unexpected` ([decision 0342](../../decisions/0342-a-documents-cell-may-be-empty-unless-it-is-a-key-or-declared-required.md)). Rows, a table, or a sheet with no `key` can be listed and nothing else: the lowering refuses `read`, `update`, `create`, and `delete` over them. A field of the built-in type `untrusted_text`, which a drafted field holds until someone states its type, is never a key and never `writable`.

### Folders, settings files, and growing files

As [decision 0246](../../decisions/0246-the-file-pack-reads-folders-settings-files-and-growing-files.md) states them, field by field and rule by rule. In short: a folder (`depth`, `max_entries`, `digest_max_bytes`, `content_max_bytes`, `writes`, `may_be_open`) has one record kind, `entry`, keyed by `name`, an entry's one name inside the folder; `list` and `read` give `name`, `kind`, `size`, `modified`, and when asked `digest` and `content`; `create` gives a temporary file a second name only where nothing has the name; `update` is the verified save held to the identity and digest that were read; `rename` gives a second name in the same folder and then removes the old; `delete` removes a file that is still the one read, by its identity and its bytes, or an empty folder. Create, rename, and delete act through the surface's folder held for them (`CheckedPath::hold_folder`), never by path, and a file is removed only while its name still holds it (`HeldFolder::remove_file_if`, [decision 0272](../../decisions/0272-the-file-packs-folder-writes-act-through-a-held-folder-after-a-second-reader.md)). A settings document names one-value places by a path of keys and is read and updated; its readers stop by name at every form whose meaning differs between programs. A growing file (`file`, `max_lines`, `max_bytes`) is read by `read` with `from:`, and gives `lines`, `position`, `outcome` (`read`, `replaced`, `truncated`, `unreadable`), and `more`. A line that is not text ends the lines a read gives; the next read tells it as `unreadable` and goes past it.

**Two shapes the drafter reads that the file does not name** ([decision 0346](../../decisions/0346-a-drafter-reads-a-sheet-with-no-table-and-types-json-by-its-values-and-a-committing-step-carries-what-it-writes.md)). A workbook sheet that defines no table, with a header row and a row below it, is a record kind placed by `sheet`: its headers are the text cells of the first row that holds anything, and its columns are typed as a table's are (`TableFound::on_sheet`, left out of the fingerprint's `tables`). A JSON list's keys carry the kind their values hold across its records (`json_column`): text is `untrusted_text`, a number `decimal`, true or false `boolean`, and a key with values of more than one kind, a list or record inside, or none set stays a question, as does the list's key.

**The step that commits carries what it writes.** The `save` of a document carries the key's and the fields' arguments (`key.<field>`, `field.<field>`), and a folder's `save` those of its `set`, so that the replay's record of what was sent holds the values an offline test's `<step>.request.json` states. The driver reads none of them at `save`. The order in which a fixture's answers are consumed is in [the file formats reference](../../reference/file-formats.md).

**A column named as a secret is never drafted as a field or an output.** The drafter asks the compiler's own test (`names_a_secret` in `a2m-files`) of each header of delimited text, a workbook's table, or a JSON document, as the file writes it and as the field it becomes. Such a header is no field of the drafted document and no output of a drafted read, and the last sentence of the description of a drafted `list` and of a read by key names the fields left out. A settings value whose key names a secret was already withheld, by the pack's own words in `evidence.rs`. The compiler's refusal of a secret in a file does not change ([decision 0302](../../decisions/0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md)).

### The binding

`document`, `record`, `key` (a value for each key field), `fields` (a value for each field set), `limit` (for `list`), and `read_back: {record}`. For a folder: `folder`, `key: {name}`, `fields: {content}` or, for `rename`, `fields: {name: input.<name>}`, `limit`, `digest`, `content`, and `read_back: {record: entry}`. For a growing file: `log` and `from`.

### Lint rules

| Code | Requires |
| --- | --- |
| `file.stands_undeclared` | A binding that writes is on a surface that declares `stands` |
| `file.beneath_application` | A write beneath an application is `irreversible`, and `target.yaml` names the authorization record: a reference that is not empty. What the record covers is outside the target's files, and the rule does not see it |
| `file.open_undeclared` | A write names a document that states `may_be_open`, `true` or `false` |
| `file.open_without_signal` | A write to a document with `may_be_open: true` has a `lock_signal` |
| `file.format_read_only` | A write is to delimited text, JSON, or a value of an INI file or a property list, and a `create` or `delete` is to delimited text or JSON |

The rules about writes cover a folder's writes too, but for `file.format_read_only`. The file kinds and the lowering also report `file.document_invalid`, `file.format_not_built`, `file.folder_invalid` (a folder or a growing file the pack cannot carry out: a folder written that does not say `may_be_open: false`, a verb named twice, a file name that is not one name), and `file.binding_invalid` (among others: a folder's verb its file does not name under `writes`, a settings value added or removed, a record of a document renamed, a growing file written), each with a file, a line, and a fix.

## Interface

```rust
pub const MANIFEST: &str;                 // the manifest's text
pub struct FilePack;                      // implements the provider contract's Pack
pub const fn pack() -> FilePack;          // the product crate hands it to the registry builder's add_pack

pub mod codes;                            // the pack's failure codes, and ALL with the class of each
pub struct FileLimits;                    // from packs.file
pub enum Format { Delimited, Json, Workbook, Ini, PropertyList }
pub struct FolderKind; pub struct LogKind;  // the `folders/` and `logs/` file kinds
pub const FOLDER_KIND, LOG_KIND: FileKindName;
pub const FOLDER_INVALID: LintCode;
pub const ENTRY_NAME, ENTRY_CONTENT, SKIPPED, LOG_FROM, LOG_LINES, LOG_POSITION, LOG_OUTCOME, LOG_MORE: &str;
pub fn folder_schema() -> JsonSchema; pub fn log_schema() -> JsonSchema;
pub struct LayoutFound; pub struct TableFound; pub struct KeyCandidateFound;   // what the findings hold
pub struct FolderFound; pub struct EntryFound; pub enum EntryKindFound;     // a surface's folder
pub struct SettingFound; pub enum ValueShape; pub enum Withheld;             // a settings value's shape
pub struct LogFound; pub enum LineForm;                                       // a growing file
pub const FILE_LAYOUT, FILE_KEY_CANDIDATE, FILE_FOLDER, FILE_LOG: &str;
pub const FILE_DIGEST, BEFORE_DIGEST, INTENDED_DIGEST, ROWS, TRUNCATED: &str;  // names in a step's result
pub fn document_schema() -> JsonSchema;
```

Everything else is private. The driver, the lowering, the file kind, the discoverer, and the drafting rule are reached only through the registry.

### How the mode fills the loop

| Part | Step | What happens |
| --- | --- | --- |
| Prepare | | The surface's folder is asked of the pack's roots, for writing where the root allows and for reading otherwise, with the run's checked paths, so a root that stands for the project folder of each request (`Roots::add_project`) is resolved in the run's own project, with every refusal of a root, and a run with none stops with `project_folder_missing` ([decision 0194](../../decisions/0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md)); in the root `project`, of the run's project folder, for reading. The pack's section of the compiled target is decoded, and the field types of this surface's documents are loaded |
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

Used by the product crate only, which hands the pack to the registry. Uses the provider contract and the layers below it. Of the contract's save in steps it uses all of it: `may_write`, `entry_kind`, `create_temporary`, `Temporary` (`keep_owner_and_mode`, `write`, `flush`, `read_back`, `replace`), and `flush_folder`, and maps the codes `path_missing`, `path_denied`, `path_locked`, and `disk_full` to its own `file_missing`, `file_protected`, `file_locked`, and `storage_full`, so that no two of its codes mean one thing; a folder the temporary file cannot be made in is `path_not_allowed`, as before ([decision 0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md)). The folder is a `CheckedPath`, and `is_dir` looks before every use that it is still the folder that was checked. `join` checks a document's name: its form, and that a link at it does not lead out of the root. The operating-system calls are given the folder's `as_path` with that one name added, not the path `join` resolved, because `join` follows a link that stays inside the root and these calls must not: a look at the name itself, an open whose handle is then compared with the name, a create that fails where anything has the name, and a `rename`. The discoverer reads the files the kernel hands it through `CheckedPath::read`; when it hands none, the surface's folder through the pack's own `Folder`, as the driver does.

## State

None of its own outside a prepared driver. A prepared driver holds the checked folder, the loaded documents, the file last opened with its parsed form, the place last located, and the new bytes between `set` and `save`. No file handle is kept between steps.

## Failures

| Class | Code | When | Sent |
| --- | --- | --- | --- |
| `input` | `path_not_allowed` | A path outside the root, a link out, a link at the file's own name, a link in the way to the folder of a write, not a regular file, more than one link, a folder that cannot be written | Not sent |
| `input` | `project_folder_missing` | A surface in the root `project`, and a run whose request came with no project folder: the contract's own code | Not sent |
| `input` | `file_unavailable` | The file cannot be opened, read, or replaced for a reason with no name of its own | Not sent, or answered |
| `input` | `payload_unreadable` | The compiled target's section or a step's payload does not decode, or steps arrive out of order | Not sent |
| `input` | `value_out_of_range` | A key or a field value its field type refuses | Not sent |
| `input` | `name_case_conflict` | A folder holds the entry's name only in another case, or a name to create or rename to in another case | Not sent |
| `input` | `value_unparsable` | A key or a field value its field type refuses; an entry's name or content that is not text; a growing file's position no read gave | Not sent |
| `policy` | `folder_not_empty` | A folder to remove holds an entry | Not sent |
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
| Discovery writes nothing, stores no value, and a draft holds no key and no type the file did not state: a cell of delimited text or a workbook is the text it holds, `untrusted_text`, and a JSON field has the type its values hold, with only a key of mixed, nested, or no values a question; an open question holds no text from a file | `stand.rs` |
| A drafted list of delimited text reads every row to the end of the data, an empty cell as no value | `stand.rs`, `a_drafted_list_of_delimited_text_ends_at_the_end_of_the_data_not_at_an_empty_cell` |
| A workbook saved with no table is drafted as the rows under its header row, listed, and read as stored | `stand.rs`, `a_workbook_saved_with_no_table_is_drafted_as_the_rows_under_its_header_row` |
| A JSON field is drafted with the type its values hold; only the untypable is asked | `stand.rs`, `the_fields_of_a_json_list_are_typed_by_their_json_type_and_only_the_untypable_is_asked` |
| The step that commits carries the key and the values written, for an offline test | `stand.rs`, `the_committing_step_carries_the_key_and_the_values_it_writes_for_an_offline_test` |
| A document in the root `project` is read from the project folder of the run, one folder or another, and never from the folder the engine was started in; a run with no project folder stops by name | `stand.rs`, `a_project_document_is_read_from_the_project_folder_of_the_run` |
| Five reads say the same of how each step was done | `stand.rs`, `five_reads_say_the_same_of_how_each_step_was_done` |
| A settings value is read by its path from an INI file and a property list; each form with two meanings stops by name and only its own read; a changed layout is drift; a binary property list is not built | `entries.rs`, `a_settings_value_is_read_by_its_path_and_a_form_with_two_meanings_stops`; `settings.rs`, unit tests |
| A settings write leaves the file byte for byte the seed with the value's bytes alone changed: the byte-order mark, CRLF, comments, blank lines, the unended last line, quotes, tabs, the comment and the document type of a property list | `entries.rs`, `a_settings_write_changes_its_value_and_keeps_every_other_byte` |
| A settings value the file cannot hold as written is refused unsent, and the file is as it was | `entries.rs` |
| A property list reads no document type of its own declarations, no entity, no deeper than 32 | `settings.rs`, `a_property_list_reads_no_document_type_and_no_entity` |
| A folder lists its entries in the order of their names' bytes, to its depth, with sizes and digests the stand confirms; one entry is read by its name with its content; a name another case holds, a name out of the folder, a device name, a folder's content, and bytes that are not text each stop by name; a link out is listed as a link and never followed | `entries.rs`, `a_folder_lists_its_entries_in_order_and_reads_one_by_its_name` |
| A file is created only where no entry has its name, in any case; one another program puts there meanwhile is never replaced; nothing of the temporary file is left | `entries.rs`, `a_file_is_created_only_where_no_entry_has_its_name` |
| A file is replaced only while it is the one that was read | `entries.rs` |
| A rename keeps the file's identity and digest, the old name gone; a new name the folder holds is never taken | `entries.rs`, `an_entry_is_renamed_in_its_folder_and_keeps_its_identity` |
| A delete is `irreversible`; a folder is removed only when empty | `entries.rs` |
| A growing file gives its complete lines after the position, within its bounds, never a partial line, and writes nothing; a replaced file, a file cut short, a line that is not text, and a position no read gave are each told by name | `entries.rs`, two tests; `log.rs`, unit test |
| A folder, a settings file, or a growing file the pack cannot carry out does not compile, ten cases | `pack.rs`, `a_folder_a_settings_file_or_a_growing_file_the_pack_cannot_carry_out_does_not_compile` |
| A delete is `irreversible`, and is still read back | `pack.rs`, `a_delete_is_irreversible_because_nothing_restores_the_row` |
| A document whose writability is an open question describes a write, and is never drafted | `pack.rs`, `a_drafting_rule_that_drafts_a_document_that_writes_is_refused` |
| The contract's codes come back as the pack's own; a temporary file is used only when it holds the bytes built | `save.rs`, unit tests |
| A surface's folder is discovered to its depth with each entry's kind and size and no content; the drafted folder's file names no write, its `list` and `read` compile and give what the stand holds, and a write over it does not compile, saying why | `stand.rs`, `a_folder_is_discovered_to_its_depth_and_its_drafted_reads_list_what_the_stand_holds` |
| A folder deeper than the bound is cut and says so, in the finding, the report, and the drafted read; a link to a file and to a folder outside the root are entries of the kind `link`, never opened or looked into | `stand.rs`, `a_folder_deeper_than_the_bound_is_cut_and_a_link_out_of_the_root_is_never_followed` |
| An INI file and a property list are discovered by section, key, and shape; a value whose key names a secret or that has the form of a credential is withheld and appears in no finding, description, or drafted file; one read per record kind compiles and gives what the stand holds | `stand.rs`, `settings_files_are_discovered_by_key_and_shape_drafted_as_reads_and_no_secret_reaches_evidence` |
| A growing file is discovered by its name, `inferred`, with no line kept; its drafted read compiles and gives the stand's complete lines | `stand.rs`, `a_growing_file_is_discovered_drafted_and_read_from_the_start_through_the_draft` |
| A line form is named only where exactly one fits; a key naming a secret and a value shaped as a credential are told | `evidence.rs`, unit tests |
| The drafter refuses a folder, a document, and an operation that write, each with its reason | `pack.rs`, `a_drafting_rule_that_drafts_a_folder_or_an_operation_that_writes_is_refused`, `a_drafting_rule_that_drafts_a_document_that_writes_is_refused` |

Not proven here: a file held open without sharing, which only Windows shows. `a_file_held_without_sharing_is_busy` runs it there and ends early elsewhere. CI on Windows must show that test passing, and that the save tests pass with `rename` over an existing file.

Also not proven, and why:

- **An owner or a group the engine's user may not give.** The stop is `metadata_not_kept`. A test would need two accounts. The superuser, who can make such a file, can also give it back, and the owner test shows that; as an ordinary user the test shows the file is not replaced at all. The same stop for a bit that is never carried, a set-group bit, is shown on any Unix.
- **A replace the operating system refuses.** Nothing portable makes `rename` fail at a chosen moment. The rule that decides what the file's bytes then mean is one function with a unit test; the calls around it are not exercised.
- **The engine killed between the replace and its record,** case 3 of section 308. The test reads the file again with a new driver and finds the intended digest. The row of reconcile where the file is neither old nor new is the engine's, and is not tested here.
- **The parts that differ outside Unix.** On Windows the identity of a file is the volume's serial number and its file identifier, from a handle; the folders from the root down are held open while a folder is written; a file is removed through its own handle; a link can be made only by a user who may; and owner, group, and bits do not exist. The pack and the provider are linted for Windows and macOS from Linux with a stand-in C compiler. CI must show the tests passing on both, and the macOS test of a name the file system folds (`a_name_the_file_system_folds_into_a_listed_entry_is_a_named_stop`) runs only there.

Known limits, stated and not closed:

- **Extended attributes and access lists are not carried.** The new file has the old file's owner, group, and permission bits and nothing more. On Windows it takes the folder's access list, and a file whose own list differs from it is not replaced: the save stops with `metadata_not_kept`. Not run here: CI's Windows job must show the save tests passing, which means a file that holds its folder's list is told as kept. Safe code is not given either, so the pack cannot carry them or tell that a file has them.
- **A check and the call it guards are two calls.** Between the look at a name and the open, the create, or the rename, another program that may write the folder can change what is at the name. The open is caught by comparing the handle with the name; a create follows no link; a second name is never given where a name stands; a removal moves the file aside (Linux, macOS) or opens it (Windows) and looks again before it removes. The folder itself is held, so a folder swapped for a link in that moment takes nothing ([decision 0272](../../decisions/0272-the-file-packs-folder-writes-act-through-a-held-folder-after-a-second-reader.md)). What is left: on Linux and macOS a held folder moved away whole is still the one written, which `still_at_its_path` tells before the act (unsent) and after it (unknown); on Windows a held folder made a mount point while held is followed; a process ended in the moment a removal looks leaves the file under a name beginning `.a2m-old-`.

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
5e. Done ([decision 0246](../../decisions/0246-the-file-pack-reads-folders-settings-files-and-growing-files.md)): a folder as a collection of entries, read and written; INI files and property lists in their XML form, read and updated one value at a time; a growing file read from a position the caller hands back. The save takes the identity read (`SaveJob::expected`). Not proven here: Windows and macOS, which CI must show (the decision's table of three systems). Not built: a declared restore for a folder's delete; a settings value added or removed.
5f. Done ([decision 0255](../../decisions/0255-the-file-pack-discovers-and-drafts-folders-settings-files-and-growing-files-as-reads.md)): discovering and drafting folders, settings files, and growing files, reads only. With no files handed, the discoverer reads the surface's own folder (`driver::surface_folder`, for reading) to the bound's depth (`folder::survey`), then each regular file directly in it whose name the pack knows. Not built: a folder handed by the kernel, a growing file not named `.log`, and a line form carried into the draft as fields.
5g. Done ([decision 0272](../../decisions/0272-the-file-packs-folder-writes-act-through-a-held-folder-after-a-second-reader.md)): section C of the sixth safety audit. Create, rename, and delete act through the held folder; the delete is held to the digest of the bytes read too; identity is the provider's `FileIdentity`; a repeated INI section header is `shape_unsupported`; a written INI value refuses every control character and U+0085, U+2028, U+2029, and a property list value also U+FFFE and U+FFFF; a log's bad line is told as `unreadable` and passed, and a regrown cut is `truncated`; a name the file system folds into a listed entry is `name_case_conflict`. The fixed place of decision 0252: `FileKind::fixed_places` lists each record of a document with no `key` that is one place, `Lowering::fixed_place` names `document.record` for a binding with no `key:`, and `Lowering::statement` says a binding sends nothing where the surface's folder is a local path in form, and is not stated for one that names another machine. Proven by the three `a_folder_swapped_for_a_link_after_the_look_gets_no_…_outside_the_root` tests, `a_file_put_at_the_name_after_the_look_is_not_the_one_deleted`, `a_settings_write_declares_its_record_one_fixed_place`, and `the_review_states_what_the_file_pack_runs_and_where_it_sends`.
6. Not built: `upsert`; a list of rows in one save; workbook writes with the formula rule and the declared second place; YAML, TOML, XML, and text documents; writing dates, times, and measurements; removing a temporary file a killed save left, which no later run can prove it made; carrying, or refusing, extended attributes and access lists.
7. No auth: the pack holds no session.

## Open points

- **What the kernel lacked**, for hypothesis H-26: nothing in the compiled target names the files of a surface, so `PrepareContext.paths` is not used; a manifest lists roots by fixed name; `Observed` had no place for the trace's `means` and `measured` (it now has the `acting` capture); no public function spells a value as its field type shows it; `CheckedPath` hides the kind of an input-output error; it now makes a temporary file the save uses whole (`create_temporary`, `Temporary`, and `FileIdentity::of_open`, [decision 0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md)); it now asks whether the engine's user may write a file (`may_write`), which the pack takes. So the pack adds one checked name to the folder's `as_path` itself ([decision 0149](../../decisions/0149-the-file-pack-after-its-safety-audit.md)). While that was built, the contract gained `entry_kind`, which says what stands at a name and follows nothing, and an `open` that follows no link and reaches the folder from the root down; the pack has taken `entry_kind` where it can (a document's own name, a file handed to discovery); the contract now holds a folder for its writes (`HeldFolder`, [decision 0272](../../decisions/0272-the-file-packs-folder-writes-act-through-a-held-folder-after-a-second-reader.md)); `TargetView` does not show the authorization record; the JSON parser refuses a duplicate key and so cannot report one.
- **A write to one fixed place** is now declared `identity: {strategy: natural, place: fixed}` with no identity input, as [decision 0252](../../decisions/0252-a-fixed-place-a-review-that-shows-what-runs-and-a-sign-in-before-its-probe.md) settles it: a settings record, a JSON record at a path with no key, and a workbook's named cell are each one place. Two operations that write one place still take different locks, as that record says.
- **What discovery is handed** ([decision 0255](../../decisions/0255-the-file-pack-discovers-and-drafts-folders-settings-files-and-growing-files-as-reads.md)): the engine hands a discovery job no files of the surface, so the discoverer resolves the surface's folder from the pack's roots itself. And the words that make a key name a secret are the kernel's credential lint's, which `a2m-files` keeps private: the pack repeats them in `evidence.rs` until the kernel exports them.
- **A growing file's position between runs** is the caller's, as an output handed back as an input: nothing in the contract hands a step a value from an earlier run.
- **`key_not_unique` has two classes in the product:** `input` in `a2m-sources`, for a source, and `drift` here, for a target, as section 308 states.
- **The window between the last digest check and the replace** stays, as section 308 says. It is one call wide. Nothing in the engine or the scheduler reads a mode's `exclusive` setting yet, so `exclusive = "file"` is a statement and two runs on one file are held apart by the digest alone: the second to save is `stale`, and a second save that falls inside the window is not caught.
- **Whether `file` fitted the artifact archetype**, for H-29: the five steps were enough. The one bend is that `open` belongs to act and not to prepare.
