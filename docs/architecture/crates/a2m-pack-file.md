# `a2m-pack-file`

The file pack: the `file` driver. Values are read from and written to named places in files under declared roots. A write patches the bytes of the places named, proves nothing else changed, and replaces the whole file in one step. The pack is designed in [spec section 308](../../spec/3-packs/308-the-file-pack.md); this document says how its crate is put together.

## Facts

| | |
| --- | --- |
| Side | Pack: file |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5. It has no base crate and depends on no other pack crate |
| Depends on | `a2m-core` (plan, steps, values, findings), `a2m-expr` (the normalizer of a field type), `a2m-files` (file kind, lowering, lint, and drafting registration), `a2m-sources` (the parsed form with positions, `typed`, the JSON document tree), `a2m-auth` (the lease type in the driver's signature; the pack holds no session), `a2m-provider` (`Driver`, `Discoverer`, `Describer`, `Roots`, `CheckedPath`, registration), `a2m-telemetry` (the clock for a finding's time). Outside: `tokio`, `serde`, `serde_json`, all in the workspace already. Tests only: `a2m-ledger`, `a2m-vault`, and the stand `example-files` |
| Built in | Packages H2 and H3 of the [roadmap](../../plan/roadmap.md) |
| Carries out | Spec section [308](../../spec/3-packs/308-the-file-pack.md); [201](../../spec/2-kernel/201-archetypes-and-modes.md) (artifact); [502](../../spec/5-security/502-defending-against-a-manipulated-agent.md) (no input chooses a file); [401](../../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md), rule 4 (a file beneath an application) |
| Decisions | [0102](../../decisions/0102-the-file-mode-and-what-the-file-pack-registers.md), [0103](../../decisions/0103-the-file-pack-as-building-it-settled-it.md) |

## Owns

- The mode `file` and the surface kind `file`: a folder inside a named root.
- The `documents/` file kind: its schema, and its compilation into the pack's section of the plan.
- The lowering of `read`, `list`, `update`, `create`, and `delete` into the steps `open`, `locate`, `set`, `read`, and `save`.
- Reading a file once, parsing it through `a2m-sources`, checking its layout, and finding named places.
- The patch editors for delimited text and JSON, the untouched check, and the save.
- The `file_layout` and `file_key_candidate` finding kinds, the discoverer that makes them, and the rule that drafts a document from a layout.

It must never: build or resolve a path; let an input reach a file name or a place; edit a file in place; write a format it has no patch editor for; calculate a formula or open a macro; replace a file marked read-only; retry; remove a file it did not name itself.

## What it registers

| Registration | Content |
| --- | --- |
| Manifest | `anything2mcp-provider.toml` in the crate, embedded. Provider `file`, kind `pack` |
| Mode | `file`, artifact; verbs `read`, `list`, `update`, `create`, `delete`; steps `open`, `locate`, `set`, `read`, `save`; `exclusive = "file"` |
| Surface kind | `file`. Address key `folder`, a relative folder with forward slashes, or `.` for the root itself. Optional `root`, a root's name, `files` when absent |
| File kind | `document`, folder `documents/`, for `file` surfaces. A document is named by its file's name without the extension |
| Finding kinds | `file_layout`, `file_key_candidate` |
| Discoverer | `file_layout`: artifact, `file` surfaces, source `live_system`, at most `discovered` |
| Drafting rule | `file_layout` to `document` |
| Effects | Reads the roots `project` and `files`; writes the root `files`. No process, no network, no session |
| Settings | `packs.file.max_bytes` (16 MiB), `max_rows` (200,000), `max_cells` (2,000,000), `max_cell_bytes` (65,536) |

### The document

As in section 308, with these fields as built: `surface`, `file`, `format`, `dialect` (`delimiter`, `header`, `encoding`, `line_ending`), `fingerprint` (`sheets`, `tables`, `names`, `headers`, `keys`; a list that is absent is not compared), `may_be_open`, `lock_signal: {sibling}`, and `records`. A record kind states one place: `rows: all` for delimited text; `path: [keys]` for JSON; `table: {sheet, name}`, `sheet: name`, or `cell: {name}` for a workbook. A field has `header` or `path`, a `type`, and optionally `writable` or, in a workbook, `formula`. A key field is never writable.

### The binding

`document`, `record`, `key` (a value for each key field), `fields` (a value for each field set), `limit` (for `list`), and `read_back: {record}`.

### Lint rules

| Code | Requires |
| --- | --- |
| `file.stands_undeclared` | A binding that writes is on a surface that declares `stands` |
| `file.beneath_application` | A write beneath an application is `irreversible`, and `target.yaml` names the authorization record |
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
| Prepare | | The surface's folder is asked of the pack's roots, for writing where the root allows and for reading otherwise. The pack's section of the plan is decoded, and the field types of this surface's documents are loaded |
| Act | `open` | The file's path is the folder joined with the document's one name, through the same check. Not a regular file, or more than one link on Unix: refused. With `for_write`, a present lock signal is `busy`. The bytes are read once, on a blocking thread, within `max_bytes`; digested; parsed; and the layout is compared. Returns `file_digest` for reconcile |
| | `locate` | The record kind's rows are found in the parsed form. Every key in the table is typed, so a key that is there twice is `key_not_unique`. A lookup that finds nothing is `Absent`; a `create` that finds the key is `stale` |
| | `set` | Builds the patches, applies them, parses the new bytes, runs the untouched check, and reads the named places back from the new bytes. Returns `before_digest` and `intended_digest` for reconcile |
| | `save` | The commit point. See below |
| Confirm | `read` | The file at the path has the intended digest |
| Read back | `open`, `locate`, `read` | The file is opened again from disk; the values are typed by the same `typed` a source uses |
| Release | | Removes the temporary files this driver named, if any is still there |

**The save.** The target is checked again: a regular file, one link, not read-only, no lock signal. The new bytes go to `<file>.a2m-<token>.tmp` in the same folder, made through the checked folder, where the token is sixteen characters of the digest of the record's idempotency token. A leftover of that name is this record's own and is replaced. The temporary file is flushed, and takes the old file's permissions. The target is digested again; a digest other than the one read is `stale`, and the temporary file is removed. Then one `rename`, and on Unix a flush of the folder. If the rename fails, the target is digested once more: the intended digest is success; the old digest is a named failure with the request answered; anything else has an unknown fate, which the handle makes `possible_commit`.

**The editors.** A delimited field keeps its quotes and gains them only when its text would end the field or the row. A row is added at the end with the line ending the file shows, and the last line stays ended or unended as it was. A JSON token is replaced by a token of the same kind; a new element is written on one line, set apart as the last element is. A value that already agrees is not rewritten.

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

Used by the product crate only, which hands the pack to the registry. Uses the provider contract and the layers below it. It reaches a file only through `CheckedPath`: `join`, `size`, and `exists` for the checks, and `as_path` for the operating-system calls that open, write, rename, and flush.

## State

None of its own outside a prepared driver. A prepared driver holds the checked folder, the loaded documents, the file last opened with its parsed form, the place last located, the new bytes between `set` and `save`, and the temporary files it named. No file handle is kept between steps.

## Failures

| Class | Code | When | Sent |
| --- | --- | --- | --- |
| `input` | `path_not_allowed` | A path outside the root, a link out, not a regular file, more than one link, a folder that cannot be written | Not sent |
| `input` | `file_unavailable` | The file cannot be opened, read, or replaced for a reason with no name of its own | Not sent, or answered |
| `input` | `payload_unreadable` | The plan's section or a step's payload does not decode, or steps arrive out of order | Not sent |
| `input` | `value_unparsable`, `value_out_of_range` | A key or a field value its field type refuses | Not sent |
| `policy` | `file_protected` | Encrypted or password-protected; marked read-only | Not sent |
| `policy` | `file_too_large` | Beyond the limits of `packs.file` | Not sent |
| `policy` | `effect_not_declared` | A save into a root the pack may only read | Not sent |
| `drift` | `file_missing`, `file_unreadable`, `place_not_found`, `key_not_unique`, `fingerprint_mismatch` | As section 308 | Not sent |
| `drift` | `round_trip_failed` | The untouched check fails | Not sent |
| `unknown_variant` | `formula_at_place` | A formula where a value is declared | Not sent |
| `unknown_variant` | `value_unreadable` | A place holds what its field type cannot read | Not sent |
| `unknown_variant` | `shape_unsupported` | A shape with no rule: a date to write, a nested member to add, a row to add where no line ending is shown or declared | Not sent |
| `unknown_variant` | `metadata_not_kept` | Permissions cannot be carried over | Not sent |
| `busy` | `file_locked` | The lock signal is present; the system refuses to share the file | Not sent at `open`; answered at the replace |
| `busy` | `storage_full` | The disk or folder is full | Not sent |
| `stale` | `changed_since_preview` | The file changed after it was read; a key to create is already there; the confirm finds another file | Not sent; answered at the confirm |
| `transient` | `timeout` | The step's deadline had passed | Not sent |
| `possible_commit` | `no_confirmation` | The replace was called and the file is neither the old one nor the new one | Unknown |

No message holds a value from a file.

## Invariants

Each is proven in `tests/` against the stand, which reports the bytes on disk through readers of its own.

| Invariant | Test |
| --- | --- |
| A registry with only this pack builds, and its digest is stable | `pack.rs` |
| Every code the driver returns is declared with its one class | `pack.rs` |
| The fixtures compile twice to the same bytes | `pack.rs` |
| What the pack cannot carry out does not compile, with a file, a line, and a fix | `pack.rs`, nine cases |
| A value is read from a named place in delimited text, JSON, and a workbook; five reads give one outcome | `stand.rs` |
| A changed layout, a formula at a value, a protected file, a broken file, a missing file, a link out, and a repeated key each stop with their own name | `stand.rs` |
| An update changes one span and nothing else; the stand counts one write, a replace | `stand.rs` |
| A value that already agrees leaves the file byte for byte the same | `stand.rs` |
| A byte-order mark, CRLF, an unended last line, and quotes survive | `stand.rs` |
| A row and an element are added and removed, and every other byte stays | `stand.rs` |
| A file changed after it was read is `stale` and is not replaced | `stand.rs` |
| A save stopped half way leaves the old file whole, and resumes to the new one with nothing left behind | `stand.rs` |
| A lock signal, a read-only mark, a second link, and a read-only root each stop the write before anything is written | `stand.rs` |
| Discovery writes nothing, stores no value, and a draft holds no key and no type the file did not state | `stand.rs` |

Not proven here: a file held open without sharing, which only Windows shows. `a_file_held_without_sharing_is_busy` runs it there and ends early elsewhere. CI on Windows must show that test passing, and that the save tests pass with `rename` over an existing file.

## Build order

1. Done: the manifest and registration.
2. Done: the `document` file kind, the lowering of five verbs, four lint rules, and the fixtures in `tests/fixtures/example/`.
3. Done: the driver's reads, from delimited text, JSON, and workbooks.
4. Done: writes to delimited text and JSON: `update`, `create`, `delete`, the untouched check, the save, the confirm, the read-back, and digests for reconcile.
5. Done: the discoverer, the describer, and the drafting rule for documents.
6. Not built: `upsert`; a list of rows in one save; the drafted `list` read for each record kind; workbook writes with the formula rule and the declared second place; YAML, TOML, XML, and text documents; writing dates, times, and measurements; removing a leftover temporary file when a record returns to `planned` and is never tried again.
7. No auth: the pack holds no session.

## Open points

- **What the kernel lacked**, for hypothesis H-26: nothing in the plan names the files of a surface, so `PrepareContext.paths` is not used; a manifest lists roots by fixed name; `Observed` has no place for the trace's `means` and `measured`; no public function spells a value as its field type shows it; `CheckedPath` hides the kind of an input-output error and cannot make a flushed, named temporary file; `TargetView` does not show the authorization record; the JSON parser refuses a duplicate key and so cannot report one.
- **`key_not_unique` has two classes in the product:** `input` in `a2m-sources`, for a source, and `drift` here, for a target, as section 308 states.
- **The window between the last digest check and the replace** stays, as section 308 says.
- **Whether `file` fitted the artifact archetype**, for H-29: the five steps were enough. The one bend is that `open` belongs to act and not to prepare.
