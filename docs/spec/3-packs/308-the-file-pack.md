# 308. The file pack

The file pack treats a file in a known format as the system being operated on: a spreadsheet, a delimited text file, a structured configuration or data file, a document. It is the artifact archetype ([section 201](../2-kernel/201-archetypes-and-modes.md)). It registers one mode, `file` ([decision 0102](../../decisions/0102-the-file-mode-and-what-the-file-pack-registers.md), under the rule of [record 0060](../../decisions/0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md)). Packages H1 to H3 of the [roadmap](../../plan/roadmap.md) build it. Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

The pack also reads a folder as a collection of entries and writes its entries, reads and updates the values of settings files (INI files and property lists), and reads a growing file from where the last read stopped ([decision 0246](../../decisions/0246-the-file-pack-reads-folders-settings-files-and-growing-files.md)).

A file is the simplest system there is to verify, because the whole of it can be read again after every write. It is also the easiest to damage without noticing, because a careless save rewrites everything the tool did not mean to touch. This section is mostly about the second point.

## A file as a target, and a file as a source

`a2m-sources` already reads spreadsheets, delimited text, and JSON as inputs to pipelines ([section 210](../2-kernel/210-sources-and-value-normalization.md)). The two are different things.

| | A source, in `a2m-sources` | A target file, in this pack |
| --- | --- | --- |
| What it is | Input to a pipeline: records to be written somewhere else | The system itself: the place the records live |
| Who names the file | The caller, for each run: a blob on a server, a path in an allowed root on one machine ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)) | The target, once, in its files. No input names it |
| Direction | Read only | Read and write |
| What is checked | The template fingerprint; every row validated before anything runs | Every write: intent, replace in one step, read-back by opening the file again |
| In the ledger | The file's content hash is part of the input hash | Each named place is a record with its own row |

**What they share, and which way the dependency points.** The pack depends on `a2m-sources`, which is in layer 4 and so within a pack's reach ([decision 0061](../../decisions/0061-the-layers-are-redrawn-so-that-every-needed-dependency-is-allowed.md)). It uses three things from it: the parsed form of each format the kernel reads, the template fingerprint, and the rules that turn a cell into a typed value, so that a value read through a source and the same value read through a file tool can never disagree. Field types and normalization come from the kernel as for every pack. Everything about writing, locking, and replacing a file is the pack's alone. `a2m-sources` never depends on the pack, and gains no writing code. Settings files are read by the pack's own readers, which give each value's span: no source reads them yet ([decision 0246](../../decisions/0246-the-file-pack-reads-folders-settings-files-and-growing-files.md)).

To patch a file without disturbing it, the pack needs to know where in the bytes each value sits. The parsed form must therefore carry the position of every value it reads. That is a reading feature, useful to `a2m-sources` for its own error messages, and it is the one addition this pack asks of it.

## What it is for, and what it refuses

| For | Refused |
| --- | --- |
| Reading and writing values at named places: a row by key in a table, a named cell, a key path | Any input that chooses which file is opened, but the name of an entry inside a folder the target declares, which is a key held to one name inside it ([decision 0246](../../decisions/0246-the-file-pack-reads-folders-settings-files-and-growing-files.md)) |
| Adding and removing a row by key in delimited text and structured files | Fixed cell coordinates as a place ([section 210](../2-kernel/210-sources-and-value-normalization.md)) |
| Saving by writing a new file and replacing the old one in one step | Editing a file in place |
| | A format the pack cannot write back without loss: such a format is read-only |
| | Running, opening, or evaluating macros, embedded code, external links, or formulas. Their bytes are copied untouched |
| | A file that is encrypted, password-protected, or signed: reading past a protection is refused, and changing a signed file would break the signature |
| | Writing while an application has the file open, where that can be detected; and a write binding where it cannot and the target has not said |
| | Writing a file the engine's user may not write itself, though a replace needs only the right to write its folder |
| | Writing text into delimited text that a spreadsheet program would run as a formula when a person opens the file, unless the document declares the column free text that may begin so. A value is never changed to make it safe |

**Writing beneath an application.** A file an application owns, such as its data file or its configuration, is a lower level of that application ([section 401](../4-building-tools/401-overlaying-tools-on-an-existing-system.md), rule 4). The `file` surface declares `stands: alone` or `stands: beneath: <surface>`, exactly as a database surface does ([section 307](307-the-data-pack.md)), and the same rule follows: undeclared, no write compiles; beneath, a write compiles only with the owner's word in the authorization record, is shown first in the review summary, and is `irreversible` unless read back through the application.

## The file kind: `documents/`

A file under `documents/` declares one file on a `file` surface: its format, its fingerprint, and its record kinds, each a named place typed by field types.

```yaml
# documents/prices.yaml, in the imaginary example target
surface: price_files               # a file surface: one folder inside an allowed root
file: price-list.xlsx              # a fixed name in that folder; never an input
format: workbook                   # the ECMA-376 spreadsheet format
fingerprint: {sheets: [Prices, Notes], tables: [price_table], names: [standard_discount]}
may_be_open: true                  # an application may hold it open
lock_signal: {sibling: price-list.xlsx.lock}   # what shows that it does
records:
  price:
    table: {sheet: Prices, name: price_table}  # a table the file itself defines
    key: [item_code]
    fields:
      item_code:  {header: Item code,  type: identifier}
      length:     {header: Length,     type: length_ft_in}
      unit_price: {header: Unit price, type: money, writable: true}
  discount:
    cell: {name: standard_discount}            # a defined name for one cell
    fields:
      value: {type: percent, writable: true}
---
# documents/settings.yaml
surface: config_files
file: example-settings.json
format: json
records:
  warehouse:
    path: [warehouses]             # a list; with a key, each element is one record
    key: [code]
    fields:
      code:   {path: [code], type: identifier}
      cutoff: {path: [cutoff_time], type: time, writable: true}
---
# documents/stock.yaml
surface: price_files
file: stock.csv
format: delimited
dialect: {delimiter: ",", header: true, encoding: utf-8}
records:
  stock:
    rows: all
    key: [item_code]
    fields:
      item_code: {header: item_code, type: identifier}
      on_hand:   {header: on_hand,   type: integer, writable: true}
```

An operation binds to it like this:

```yaml
bindings:
  - mode: file
    document: prices
    record: price
    verb: update
    key: {item_code: input.item_code}
    fields: {unit_price: input.unit_price}
    read_back: {record: price}
```

| Field | Meaning |
| --- | --- |
| `surface`, `file` (required) | A `file` surface, and a name inside its folder. The name has no `..`, no separator that leaves the folder, and no placeholder |
| `format` (required) | `delimited`, `json`, `workbook`, `ini`, `property_list`, `yaml`, `toml`, `xml`, or `text_document`. Which of them can be written is a property of the pack, below |
| `fingerprint` | What identifies this layout, as for a source template. A file that does not match is `drift` |
| `may_be_open`, `lock_signal` | Whether an application may hold the file open, and the sign that it does: a sibling file the target names, or the operating system's own refusal to share. Absent is not said, never `false`: a binding that writes the file compiles only when the document states `true` or `false` |
| `records.<kind>` | One kind of named place: a `table` the file defines, or a `sheet` with a header row found by its text; a `cell` by defined name; a `path` in a structured file; `rows` of delimited text |
| `key` | The fields that identify one row or element, each of a type someone stated, never `untrusted_text`. Tables, sheets, and rows with no key can be listed and nothing else: no read by key and no write compiles over them ([decision 0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md)). A path with a key leads to a list whose elements are the records; a path without one leads to a single record |
| `fields` | Each field's place within the record (`header`, `path`) and its field type. `writable: true` marks a field a binding may set; nothing else is ever set. In delimited text, `may_begin_formula: true` says the column is free text that may begin with `=`, `+`, `-`, or `@` and that no spreadsheet program opens the file |

A key path is a list of keys, never a string to be split, and no input reaches it. An element of a list is chosen by the record's key, as a row is: the binding gives each key field a value, and the driver compares it, typed, with what each element holds. An input cannot add a key to a path, leave the file, or name an element by position ([decision 0103](../../decisions/0103-the-file-pack-as-building-it-settled-it.md)).

## Settings files, folders, and growing files

[Decision 0246](../../decisions/0246-the-file-pack-reads-folders-settings-files-and-growing-files.md) states each rule; this is what each adds.

| What | Declared in | Reads | Writes |
| --- | --- | --- | --- |
| A settings file: an INI file, a property list in its XML form | `documents/`, `format: ini` or `property_list`, a record's `path` and each field's `path` of keys; for INI, `dialect: {separator, quotes}` | `read` of a record's values | `update` of a value that is there, by replacing its bytes alone; nothing added or removed |
| A folder as a collection of entries | `folders/`: `surface`, `depth` (0 to 4), `max_entries`, `digest_max_bytes`, `content_max_bytes`, `writes`, `may_be_open` | `list` of entries in the order of their names' bytes, to the depth; `read` of one entry by its name, with its digest and its content as text when asked | `create` where no entry has the name, `update` of a file's content, `rename` in the same folder, `delete` of a file or an empty folder (`irreversible`) |
| A growing file other programs append to | `logs/`: `surface`, `file`, `max_lines`, `max_bytes` | `read` of the complete lines after a position the caller hands back, with the new position and whether the file was `replaced` or `truncated` | None |

A settings file's readers stop by name at every form whose meaning differs between the programs that read such files: an INI line continued, a comment after a value, a value in quotes when the document does not say what quotes are; a property list's document type with declarations of its own, an entity, a binary property list. A folder's entry is named by one name, never followed through a link, and found only under the very name the folder holds: one it holds only in another case is `name_case_conflict` on every system. A growing file is opened for one read and closed, never held, locked, or written.

## Verbs and steps

The artifact archetype's steps are `open`, `locate`, `set`, `read`, and `save` ([section 201](../2-kernel/201-archetypes-and-modes.md)).

| Verb | Lowers to | Commit point |
| --- | --- | --- |
| `read` | `open`, `locate` by key, `read` | None |
| `list` | `open`, `locate` the table, `read` up to the operation's row limit | None |
| `update` | `open`, `locate` by key, `set` each field, `save` | `save` |
| `create` | `open`, `locate` the table and check the key is absent, `set` a new row, `save` | `save` |
| `upsert` | Decided at the preview by reading ([section 209](../2-kernel/209-record-identity-and-dependent-records.md)), then as `create` or `update` | `save` |
| `delete` | `open`, `locate` by key, `set` the row removed, `save`. Read-back expects absent. The operation is `irreversible`: the removed row is gone with the old file once it is replaced, and no binding declares how it is restored | `save` |

`save` is the one step that saves, and it always does; the others never do. The manifest lists it as the mode's commit step. The driver carries it out only live, or in a sandbox with test records, and only when the plan marks it as the commit point. An operation whose input is a list of rows saves them in one `save`, and read-back compares each row by its key, as for one save that carries many records in section 209. A pipeline that repeats a single-row operation saves once per row: slower, and each row is its own resume point.

## The driver, part by part

| Part | What the driver does |
| --- | --- |
| Prepare | Receives from the kernel a path already resolved and checked to lie inside an allowed root, with links resolved before the check; the pack never resolves a path from text. Refuses anything that is not a regular file with one link. A link at the file's own name is refused wherever it points, for a read and for a write: the document names a file, not what a link leads to. A link in the way to the surface's folder is followed for a read while it stays inside the root, and refused for a write, which would land in a folder the surface does not name. Checks the lock signal, which counts by being there, whatever it is. Reads the bytes once, without changing the file, records their digest and the file's identity, and parses them through `a2m-sources`. Checks the fingerprint |
| Act | `locate` finds each named place in the parsed form, with its position in the bytes. `set` builds the new bytes by replacing only the spans of the places named. Then the untouched check: the new bytes are parsed again and every place outside the named ones must equal what it was. Then `save` |
| Save | Looks again that the file is a regular file under its own name that the engine's user may write. Makes the provider contract's temporary file in the same folder, under a random name, only where nothing stands and never through a link, readable by the engine's user alone; what has a name is never removed, opened, or followed. Gives the new file the old file's owner, group, and permission bits through its own handle before writing the new bytes, and stops with `metadata_not_kept` where it cannot, a set-user, set-group, or sticky bit included; on Windows the file takes its folder's access list, and a target whose own owner or access list differs from it stops with `metadata_not_kept` too, unsent; the read-only mark is kept ([decision 0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md), [decision 0194](../../decisions/0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md)). Flushes it to disk, and reads it back through its own handle: it must hold exactly the bytes built. Then, as the last act before the replace, digests the file at the declared path again: if it differs from the digest taken at prepare, stops as `stale` and removes the temporary file. Otherwise replaces the old file with the new one in one call of the operating system, and flushes the folder where the system allows; a flush that fails leaves the save in doubt, for reconcile to read |
| Confirm | The replace returned success, and the file now at the path has the intended digest |
| Read back | Opens the file again from disk, as a new read, parses it, reads the same places, and compares after normalization |
| Release | Nothing to remove: a temporary file not put in place is removed by the save itself, and only while its name still holds the file the save made, known by its identity and not by its name. It removes nothing it cannot prove it wrote |

No retries inside. The replace is the commit point: before it the old file is untouched, and after it the new file is whole. There is never a moment when the file at the path is a mixture of the two.

**The intended digest is an output before the commit point.** It is known after `set` and before `save`. `set` returns it, with the digest the file had when it was read, among the step's values for reconcile, which the engine stores before the commit point is acted. Every `open` returns the digest of the file it read, so reconcile compares the three.

## A lost answer, and reconcile

A stop after the replace was called and before it was recorded is `possible_commit`. On resume the engine reconciles, and for a file the comparison is exact:

| The file at the path has | Means | Then |
| --- | --- | --- |
| The intended digest | The save landed | Read back the places, mark `verified` |
| The digest seen before the write | It did not land | Return the record to `planned`. A temporary file a killed save left is not used and not removed by a later run, which cannot prove it made it; its name starts `.a2m-new-`, so a person can tell it is the engine's |
| Neither | Someone else saved the file, or the save landed and was changed since | Read the places: if they hold the intended values, `verified`; otherwise `stale`, `changed_since_preview` |

## Change by someone else

- **Changed since the preview.** The engine's freshness check reads the places again ([section 208](../2-kernel/208-runs-the-ledger-and-resume.md)). If their values are unchanged, the write goes ahead on the file as it is now, though other parts of it changed, because the approved diff is about records and not bytes. If the values changed, `stale`.
- **Changed during the write.** The digest check is the last act before the replace, and catches a change since prepare. So of two runs that read one file, the second to save is `stale`. A change in the small window between that check and the replace, which is one call wide, is not caught by the save. Read-back then shows the pack's own values, and the other program's change is lost. This window is stated, not hidden.
- **Held open by another program.** On Windows a replace fails while another program holds the file without sharing it, and the run waits as `busy`. On macOS and Linux the replace succeeds, and the other program may later save its own copy over it. That is why `may_be_open` exists: when it is true, a write binding needs a `lock_signal`, and when the signal is present the run is `busy`, `file_locked`. When it is true and no signal can be declared, writes do not compile. When the document does not say, writes do not compile either, and the driver refuses a plan that holds one.

## What must survive a round trip

**The rule:** a format can be written only if the pack has a patch editor for it that passes two tests on the test stand. Opened and saved with no change, the file is identical byte for byte. Saved with one place changed, only that place's span differs, and the untouched check passes. A format without such an editor is read-only, whatever the file kind says.

| Format | What must survive | Writable |
| --- | --- | --- |
| Delimited text, as in RFC 4180 | Delimiter, the quoting of each field, line endings, encoding and byte-order mark, the last line's ending, rows not named | Yes, first |
| JSON, as in RFC 8259 | Key order, spacing, the spelling of every number, escapes. A file with a duplicate key is not read at all: it is `file_unreadable` | Yes, first |
| Workbook | Every part of the package not holding the place, byte for byte; within the sheet, formulas, styles, comments, validation lists, merged ranges, hidden rows and columns. A text value is written into the cell itself, so the shared table of strings is not touched | Yes, after the first two |
| INI | Comments, order, blank lines, spacing, line endings, the byte-order mark, quotes, the last line's ending | One value at a time, in place ([decision 0246](../../decisions/0246-the-file-pack-reads-folders-settings-files-and-growing-files.md)) |
| Property list, XML form | The declaration, the document type, comments, references, indentation, element kinds | One value at a time, in place. The binary form is not read |
| YAML, TOML | Comments, key order, quoting style, anchors, several documents in one file | Read-only until an editor passes the tests. A place reached through an alias is always read-only |
| XML | Namespaces, comments, entity references, attribute order | Read-only at first |
| Text documents | Everything | Read-only. Declared fields in a document are an open point |

**Formulas.** The pack never evaluates one. A place that holds a formula is read as its cached value and is never written; a formula found where a value was declared is `unknown_variant`. Writing a cell that a formula refers to would leave that formula's cached value stale, so such a write is refused unless the record declares it, and then the pack removes the dependent cached values and marks the workbook for recalculation, so that any later read of them stops instead of returning an old number, and the hazard is `data_elsewhere`. A workbook with references the pack cannot follow without evaluating, such as references built from text, is treated as if every cell had dependents.

**Macros and embedded code** are copied as bytes. They are never opened, run, or inspected beyond their presence, which discovery reports.

## Identity, effects, hazards, and paths

| Strategy | In a file |
| --- | --- |
| `natural` | A defined name, or a key path with no selector |
| `composite` | The file and the key of a row or element: the usual case |
| `stamped`, `find` | Not needed. A file never assigns a key; the caller gives it, and the preview finds it absent before a create |
| `none` | Rows with no key. Writes are `irreversible`, and two rows with the same key are `drift`, `key_not_unique` |

The pack declares two effects, both `enforced`: it reads files, and writes files for `file` surfaces whose writes are bound, only inside the allowed roots of section 502, which are a protected setting.

| Situation | Hazard |
| --- | --- |
| A patch save that passed the untouched check | `none`: the pack can show nothing else changed, which is what section 213 asks before lowering the hazard |
| A format written by regenerating the whole file | `data_elsewhere`. No first-party editor does this; a plug-in might |
| A write that clears formula values elsewhere | `data_elsewhere` |
| A file beneath an application | As for a database, under section 401 rule 4 |

**Paths.** No input may choose which file is opened ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)). The surface names a folder inside an allowed root; in the root `project` that is a folder of the project of the request that started the run, never of the folder the engine was started in, and it is read and never written; a run whose request came with no project folder stops with `project_folder_missing`. The root `files`, by default, stands for the project folder of each request too, so one engine that serves several projects resolves each request's files in its own project, and that root is written as the manifest allows ([decision 0194](../../decisions/0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md)). The document names a file in it; both are fixed when the version is published and shown in the review summary. A `file` input type is never accepted by this mode. The temporary file is made in the same folder, so that the replace stays on one volume.

## Discovery and drafting

| Finding | Level |
| --- | --- |
| Sheets, tables the file defines and their headers, defined names, validation lists, formulas' presence, macro parts' presence, a signature's presence | `discovered`, source `live_system`: for this pack the file read from disk is the system itself; the subject is the file by digest and format |
| The header row of a plain sheet, taken as its first filled row | `inferred` |
| A column whose values are unique in this file, as a key candidate | `inferred`, with `sampled` and `single_variant` |
| A key structure in a structured file, from the file itself | `inferred` |
| A published schema for the format, such as a JSON Schema | `inferred`, source `published_description` |

Shapes, not values, are stored ([section 218](../2-kernel/218-discovery-and-findings.md)).

| The drafter writes | It leaves to the developer |
| --- | --- |
| A `documents/` file on the surface its file was observed on, with the fingerprint, and a record kind for each defined table or plainly headed sheet, no key, and no unit. A cell of delimited text is drafted as the built-in `untrusted_text`: what the file holds, marked untrusted, and no guess. A workbook stores each cell's kind, which discovery records for each column of a table as a shape: a column of text is `untrusted_text`, of numbers `decimal`, of true or false `boolean`; a column that holds a formula is marked `formula: true` and read as the result the workbook kept, as the formula rule above says; a column of cells of more than one kind is left out, and its type asked in the read by key ([decision 0188](../../decisions/0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md)). A JSON value's type and a JSON record kind's key stay open questions | The key; the type of a number, a date, or a decimal; units; names; what the file is for |
| A `list` read for each record kind of delimited text or a workbook, with the list limit, one list output per field and `truncated`; and a read by key that holds the questions of the key and its fields' types, which does not compile until they are answered | Which files are targets and which are sources. A file a pipeline takes as input is a source, and gets a source draft ([section 402](../4-building-tools/402-the-scout-and-who-does-what.md)) |
| Nothing for writes | Every write, whether the file may be open, and whether it stands beneath an application |

The scout tries each drafted read five times. Opening a file for reading is a read the engine enforces.

## Failures

| What goes wrong | Class | Code |
| --- | --- | --- |
| The path is outside an allowed root, a link leads out, the file's own name is a link, a link stands in the way to the folder of a write, the file is not a regular file with one link, or the folder is not writable | `input` | `path_not_allowed`, the provider contract's own code and class |
| The file is missing; it does not parse; a named place is not found; two rows share a key | `drift` | `file_missing`, `file_unreadable`, `place_not_found`, `key_not_unique` (proposed; the last shared with section 307) |
| The fingerprint does not match | `drift` | `fingerprint_mismatch` |
| A formula where a value was declared | `unknown_variant` | `formula_at_place` (proposed) |
| The file is encrypted, protected, signed, marked read-only, or not writable by the engine's user | `policy` | `file_protected` |
| Text for delimited text begins as a formula does, or with a tab or a line break | `input` | `value_begins_formula`. Nothing was written |
| The lock signal is present, or the system refuses to share the file | `busy` | `file_locked` (proposed) |
| The disk or folder is full | `busy` | `storage_full` |
| The untouched check fails, so the editor would change something else | `drift` | `round_trip_failed` (proposed). Nothing was written |
| The owner, the group, or the permissions cannot be carried to the new file | `unknown_variant` | `metadata_not_kept` (proposed). Nothing was written |
| The file changed between prepare and the replace | `stale` | `changed_since_preview` |
| A stop after the replace was called, before it was recorded | `possible_commit` | `no_confirmation` |
| Read-back differs | `mismatch` | `read_back_differs` |
| A folder holds an entry's name only in another case | `input` | `name_case_conflict` |
| A folder to remove is not empty | `policy` | `folder_not_empty` |
| A settings file uses a form whose meaning differs between programs; a binary property list | `unknown_variant` | `shape_unsupported` |
| A growing file's position is not one a read gave | `input` | `value_unparsable` |

## The test stand

Files in a temporary folder, in `targets/example-files/`. It is the cheapest stand of any pack and runs the same on Windows, macOS, and Linux, which matters because replacing a file behaves differently on each. Seed files are small and synthetic, written by the stand at reset from a seed description.

**The control interface reports the bytes on disk**, and reads values through the stand's own reader, which shares no code with the pack or with `a2m-sources`. Reset writes the seed files. Inspect returns a file's digest and size, its file identity on Unix, and, for delimited text and JSON, the value at a place. Count writes reports how many times each file changed between two looks the test asks for, from a ledger of digests the stand keeps; it tells a replacement from an edit in place on Unix only. Set a fault does one of the cases below.

| # | The stand does this | Risk it exercises |
| --- | --- | --- |
| 1 | Holds a workbook, a JSON file, and a delimited file for the `example` target's prices and stock | A value read from a named place in each format |
| 2 | Opens and saves each file with no change | Byte-identical round trip |
| 3 | Kills the engine between the replace and its record | `possible_commit`; the file is old or new, never a mixture |
| 4 | Changes a file from another writer between preview and write, in the place and elsewhere | `stale` in one case, a write on the new file in the other |
| 5 | Holds a file open without sharing, and creates the lock signal | `busy`, `file_locked` |
| 6 | Plants a formula at a declared value, and a formula that depends on a written cell | `formula_at_place`; the declared formula rule |
| 7 | Plants a macro part, a signature, and an encrypted file | Copied untouched; `file_protected` |
| 8 | Plants a link pointing outside the root, a link that stays inside it at a file's name and at a temporary file's name, and a file with two links | `path_not_allowed`; no other file is removed or written |
| 9 | Replaces a file with another layout | `fingerprint_mismatch`; levels fall |
| 10 | Holds a value the application would store differently: a date serial, a padded number, text with a narrow space | Normalization; `mismatch` |
| 11 | Holds text with instructions aimed at an agent | Untrusted fields |
| 12 | Holds two rows with one key | `key_not_unique` |
| 13 | Holds an INI file and a property list, each with comments, a byte-order mark or a document type, and forms whose meaning differs | Values by path; one span changed and every other byte kept; named stops |
| 14 | Holds a folder of notes, one not text, one a folder down | A list in order; a read by name; create, replace, rename, delete, each read from disk; a name in another case |
| 15 | Holds a log whose last line is unfinished, and appends to it, cuts it short, and replaces it | Complete lines only; `truncated`; `replaced` |

## Outside readers and writers needed

Stated as needs. Each is checked before it is added: maintained, under a licence on the closed list in `deny.toml`, and working on Windows, macOS, and Linux.

| Need | Checks beyond the usual |
| --- | --- |
| A reader and writer for the zip container that can copy an entry without recompressing it | Raw copy of compressed entries; no change to entries it was not asked to change |
| An XML reader that reports the position of every element and attribute | Needed for workbook parts and XML files; no network access for external entities, ever |
| Readers for delimited text and JSON that report positions | JSON: duplicate keys reported, numbers kept as text |
| A format-preserving editor for YAML and for TOML | Passes the two round-trip tests; until one does, those formats are read-only |
| Replacing a file in one step, and flushing a file and its folder | From the standard library, confirmed on each operating system in the first slice |

The workbook reader itself is `a2m-sources`' choice, made once, and this pack uses it.

## The entry bar

| # | Item of [section 202](../2-kernel/202-kernel-and-packs.md) | Answered by |
| --- | --- | --- |
| 1 | A driver, no retries inside | The driver, part by part |
| 2 | A read-back rule for every write | Opening the saved file again; a format that cannot round-trip is read-only |
| 3 | Each identity strategy | Identity, effects, hazards, and paths |
| 4 | A test stand with ground truth | Files in a temporary folder, read by the stand's own reader |
| 5 | A discoverer that claims no more than it saw | The file's own structure as `discovered`; header and key guesses as `inferred` |
| 6 | Failures mapped to the classes | Failures |
| 7 | Effects and hazards | Identity, effects, hazards, and paths |
| 8 | A decision record for the mode | `file`: [decision 0102](../../decisions/0102-the-file-mode-and-what-the-file-pack-registers.md) |

## Build order

1. **H1.** The stand and its control interface, with its own tests on three operating systems.
2. **H2.** Reads from delimited text and JSON, then from workbooks through `a2m-sources`, with positions added to its parsed form; the proof is a value read from a named place in each. Then the discoverer, the drafting rules, and the scout's tries. Done, except the scout's tries.
3. **H3.** Writes to delimited text and JSON: the patch editor, the untouched check, the replace, read-back, and reconcile by digest; the proof is case 3. Done for `update`, `create`, and `delete` of one record; `upsert` and a list of rows in one save wait for the engine. Then workbooks with the formula rule, then YAML and TOML, each when an editor passes the round-trip tests: not built, and YAML, TOML, XML, and text documents are not read yet either.
4. **Decision 0246.** Folders, settings files, and growing files: built, with their stand cases 13 to 15. Discovering and drafting them is not built.
5. Record what the kernel had to change, for hypothesis H-26, and whether `file` fitted the artifact archetype without bending it, for H-29.

## Open points

- **Files whose name changes,** such as one per month. Choosing by an input is refused. A declared rule such as "the newest file in the folder that matches this layout" is deterministic, and is still a choice of file; whether it is allowed is not settled.
- **Folders on a network share, or kept in step by another program.** A replace there may not happen in one step, and a program that copies files about may hold or rewrite them. Whether the pack can detect either, or the surface must declare it, is not settled.
- **Adding a row to a workbook table** changes the table's range in another part of the package. It is a declared second place, and its editor comes after updates.
- **The window between the last digest check and the replace.** A lock the operating system enforces would close it on Windows only. Between two runs of the engine it closes when the scheduler holds one file to one run, as the mode's `exclusive` setting states and nothing enforces yet ([decision 0149](../../decisions/0149-the-file-pack-after-its-safety-audit.md)).
- **Extended attributes and access lists.** A replace carries the old file's owner, group, and permission bits, and nothing more. Whether the pack must carry the rest, or refuse a file that has it, is not settled.
- **Declared fields in text documents,** and whether any document format can pass the round-trip tests.
- **The positions in `a2m-sources`' parsed form.** Their exact shape is settled with the crate's design.
