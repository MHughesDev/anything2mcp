# 0246. The file pack reads folders, settings files, and growing files, and writes the first two

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** [section 308](../spec/3-packs/308-the-file-pack.md) and the [design of `a2m-pack-file`](../architecture/crates/a2m-pack-file.md). It follows [decision 0208](0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md), whose scope is anything the person's machine can reach, and finding 8 of the [coverage review of 2026-11-18](../reviews/coverage-review-2026-11-18.md): most files a person's applications keep are not tables.

## Context

The file pack read named places in tables (delimited text, JSON, workbooks) and wrote delimited text and JSON by patching the bytes of a place. Three kinds of file that applications keep had no place: a folder whose entries are the records, a settings file of keys and values, and a log that other programs append to. Each is added here under the one mode `file`, on the one surface kind `file`, with the save, the roots, and the project folder of [decision 0194](0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md) unchanged.

## Decision

### 1. A folder as a collection: the file kind `folders/`

A file under `folders/` declares the folder of a `file` surface a collection of entries: `surface`, `depth` (0 to 4; 0, the default, is the folder's own entries), `max_entries` (default 1,000), `digest_max_bytes` (default 1 MiB), `content_max_bytes` (default 64 KiB, and never more than `packs.file.max_bytes`), `writes` (any of `create`, `update`, `rename`, `delete`), and `may_be_open`. A binding names it with `folder:`; its one record kind is `entry`, keyed by `name`.

- **A name is a key, not a path.** The folder is fixed by the target; an input gives the entry's name, as it gives a row's key. A name is one name of at most 255 bytes: no separator, no `..`, no drive, no device name (`con`, `aux.txt`), no part ending in a dot or a space, nothing the roots' check refuses. A read may name an entry the declared depth down with `/` between folders, each folder reached by no link; a write names an entry directly in the folder. Spec section 502 says no input chooses which file is read; here the input chooses an entry inside a folder the reviewed target names, and the pack holds it there by the checks above at every step. This is the same reach a row's key has over a table, and it is the line this record draws.
- **A name is the name the folder holds.** A look-up finds an entry only under the very name the folder lists. Where the folder holds the name only in another case, the step stops with `name_case_conflict` on every system: on a system that folds case the two are one entry, elsewhere two. A create, or a rename to a name the folder holds in another case, stops the same way.
- **Reads.** `list` gives every entry, and those of folders below to the depth, in the order of their names' bytes: `name`, `kind` (`file`, `folder`, `link`, `other`), `size` (a file's), `modified` (UTC, `YYYY-MM-DDTHH:MM:SS`), and, when the binding says `digest: true`, a file's digest where it has at most `digest_max_bytes`; as one list per value, beside `rows`, `truncated`, and `skipped` (names that are not UTF-8 or not a form the pack names by). A folder of more names than `packs.file.max_rows` is `file_too_large`. `read` gives one entry, with `content: true` its content as text: UTF-8, every byte kept, a byte-order mark included; other bytes are `value_unreadable`. A link, a device, a pipe, or a socket is listed by its kind and never followed or opened; reading its content is `path_not_allowed`.
- **Writes**, each read back from disk:
  - `create`: the content goes to a temporary file of a random name (`.a2m-new-` and 128 random bits), readable by the engine's user alone, written, flushed, and read back through its handle; then the system gives it the entry's name as a second name (a hard link), a call that fails where anything has the name, so a file that appeared meanwhile is never replaced (`changed_since_preview`, unsent); then the temporary name is removed and the folder flushed.
  - `update` replaces a file's content with the pack's verified save, held to the file that was read: its identity, which the save now compares before anything is made (`SaveJob::expected`), and its digest.
  - `rename` gives the file found a second name in the same folder by the same call, then removes the old name only while it still holds the file that was read. The read-back finds the new name holding that identity and the old name gone. The new name is an input of its own, `fields: {name: input.<name>}`, and the read-back gives the new name under that input's name, so the engine compares it.
  - `delete` removes a file that is still the one that was read, or a folder only when it is empty (`folder_not_empty` otherwise; the system's own call refuses a folder that gained an entry since the look). It is `irreversible`, as every delete of the pack is: the pack keeps no copy, and no binding declares how an entry is restored.
  - A folder that is written states `may_be_open: false`: the pack has no sign per entry that an application holds one open. Each written verb is named under `writes`, or its binding does not compile. A write binding says `read_back: {record: entry}`.

### 2. Settings files: the formats `ini` and `property_list` of `documents/`

A document of either format names one-value places by a path of keys: a record's `path`, then a field's. For an INI file a record is the keys before the first section (`path: []`) or one section (`path: [<section>]`), and a field is one key. For a property list both are keys of nested dictionaries. A record has no key of its own; a list of records in a settings file is not built. A value's type is the field type the document names. Reads and `update` are built; `create` and `delete` do not compile.

The INI reader's rules. A line is blank, a comment (`;` or `#` first, after any blanks), a section header (`[name]`), or a key, the separator (`=`, or `:` where `dialect: {separator: ":"}`), and a value trimmed of spaces and tabs. UTF-8, with or without a byte-order mark; lines end in LF or CRLF. Each of these stops the whole file with `shape_unsupported`, because programs read it in more than one way: a line continued by a backslash or by indenting, a header that does not close, a line with no separator, a carriage return that ends no line. Each of these stops the read of that one value, and the file's other values stay readable: a `;` or `#` after a blank in the value (an inline comment to some programs, text to others); a value in quotes where the document does not say what quotes are (`dialect: {quotes: literal}` keeps them as text, `{quotes: strip}` takes them as around the value); a key that holds the other separator. A key the file holds twice in one section, two spellings that differ only in ASCII case included, is `key_not_unique`; a key it holds only in another case is `place_not_found`. A write refuses a value that would read back otherwise: a line break, a blank at either end, `;` or `#` where a comment could begin, a leading quote where quotes are not literal, the quote it is written inside.

The property-list reader takes the rules of the HTTP pack's XML answers ([decision 0217](0217-the-http-pack-reads-answers-that-are-not-json.md)). No document type is processed. The one a property list carries is recognized by its form, a document type named `plist` with no declarations of its own, whatever its identifiers say, and is never fetched; any other, and any with declarations of its own, is `file_unreadable`. Only the five references the format defines and character references are read. Depth is bounded at 32, values by `max_cells`, text by `max_cell_bytes`. A literal section and a processing instruction inside the values are `shape_unsupported`; a dictionary with one key twice is `file_unreadable`; `data` is not read; a binary property list is `shape_unsupported`, not built. A write keeps the element: text is escaped, an integer or a real must be spelled as one, `true` and `false` replace the element, a date is not written yet.

A write replaces the bytes of the value and nothing else, then parses the new bytes and requires every other value, section, and root key to be as it was (the untouched check). Comments, order, blank lines, line endings, the byte-order mark, and the last line's ending are therefore kept byte for byte, which the stand's test proves by comparing the whole file with the seed edited at the value's bytes alone.

### 3. A growing file: the file kind `logs/`

A file under `logs/` declares one file of a `file` surface that other programs append lines to: `surface`, `file`, `encoding` (`utf-8`), `max_lines` (default 1,000), `max_bytes` (default 1 MiB, and never more than `packs.file.max_bytes`). A binding names it with `log:`, verb `read`, and `from:`, the position the last read gave, or none to read from the start. The read gives `lines` (the complete lines after the position, up to the bounds), `position`, `outcome`, and `more`.

- A position is the offset, a digest of the file's identity (the device and file number on Unix, the moment it was made elsewhere), and a digest of its first 4,096 bytes. Where the identity or those bytes differ, the outcome is `replaced`; where the file is shorter than the offset, `truncated`. Either way no line is read and the position given comes back unchanged: the read never starts over on its own.
- A line ends at a line feed, with a carriage return before it taken off. A partial last line is never a line. A line longer than the read's byte bound is `file_too_large`. A line that is not UTF-8 is `value_unreadable`, and the caller's position stays good. A position no read gave is `value_unparsable`.
- The file is opened for the one read, through a handle proven to be the file at the name, and closed. It is never held between reads, never locked, never written.
- **Where the position is kept.** The kernel keeps a step's recorded values within one run and on its resume ([decision 0162](0162-the-provider-contracts-second-pass-what-a-step-is-handed-and-hands-back.md)), and nothing between runs. So the position is an output the caller hands back as an input. For the engine to keep it, the kernel would need a cursor a read step returns and a later run of the same operation, surface, and file is handed, kept in the ledger beside the run, and shown to the person as the read's own state.

### Three systems

| Matter | What the pack does | Proven here | Only CI proves |
| --- | --- | --- | --- |
| Line endings | INI: LF and CRLF read, a lone carriage return stops; a log line loses one CR before its LF; a write never changes a line's ending | Stand tests on Linux | The same tests passing on Windows and macOS |
| Case-insensitive names | Two names that differ only in case are one name to the pack on every system: a look-up, a create, and a rename stop with `name_case_conflict` | On Linux, where the system does not fold | On macOS and Windows, that the hard link of a name another case holds is not reached, because the look stops first |
| Reserved names | A device name, and a part that ends in a dot or a space, are refused on every system by the roots' check | On Linux (`con.txt`) | Nothing more |
| Path length | An entry name is at most 255 bytes, the relative path at most 1,024 (the roots' check) | Unit test | A long path on Windows, under the stand's long-form root |
| Hard links | `create` and `rename` need a file system that gives a file a second name; one that does not is `shape_unsupported`, unsent | On Linux | NTFS and APFS in CI; a file system without them is not exercised anywhere |
| Identity | Unix: device and file number; elsewhere the moment the file was made and its length (a log's: the moment alone) | On Linux | That a file renamed on Windows keeps the moment it was made, so the rename's read-back holds |
| A log held open | A read opens with the system's default sharing, which on Windows lets the writer go on | Not shown here | A read while another program appends, on Windows |

## Alternatives weighed

- **Rewriting a settings file from a parsed form.** An INI or property-list writer that serializes the whole file would lose comments, order, spacing, references, and line endings, and would need its own proof that it wrote what any program reads back. Not taken: the pack's rule is a patch editor or read-only (section 308), and a patched span with the untouched check proves more with less.
- **An outside crate for INI or XML.** No INI crate was looked at: the pack needs every value's position and a refusal of each form whose meaning differs between programs, which a general reader is not built to give. quick-xml is in the workspace and gives positions, and a property list is a small closed grammar. The pack's own readers are a few hundred lines with their tests, and add nothing to the tree. Not taken.
- **Renaming with the system's rename call.** It replaces whatever has the new name on Linux and macOS; a check before it leaves a window in which a file that appears is lost. The second-name call fails where anything has the name. Taken, at the cost of needing hard links.
- **Creating the file at its name directly, with a create that fails where anything stands.** It never replaces, and it leaves a partial file under the entry's name when the engine stops mid-write. Not taken: the temporary file and the second name leave either nothing or the whole file at the name.
- **Separate modes `folder` and `log`.** Their steps are the artifact archetype's five, their roots and surface are the file surface's, and one driver already holds the folder. Not taken: three file kinds under one mode keep one surface kind and one set of roots.
- **Keeping a log's position in the ledger now.** Nothing in the contract hands a step a value from an earlier run (decision 0162). Not taken; named above as what the kernel would need.

## Not in this package

| What | Kind |
| --- | --- |
| Archives (zip, tar), read as folders | Not yet |
| Documents' named fields (word processing, PDF forms) | Not yet; section 308's open point |
| Media metadata | Not yet |
| Binary property lists | Not yet: a named stop |
| Watching a folder for changes | Not yet: the kernel's watch is designed in [decision 0234](0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md) |
| Recursion beyond a declared depth, and beyond 4 | Choice: this record; a list stays bounded |
| Writes below a folder's own entries | Choice: this record |
| A path outside the request's project roots | Rule: section 502 and decision 0194 |
| A declared restore for a folder's delete | Not yet |
| Discovering and drafting folders, settings files, and logs | Not yet: drafting follows by rule (a folder's list, a settings file's keys as reads), and the discoverer reads neither yet |
| A settings value added or removed; a list of records in a settings file | Not yet |
| Writing a date into a settings file | Not yet, as for every format of the pack |

## Consequences

- The pack registers three file kinds (`documents/`, `folders/`, `logs/`), the verb `rename`, and the codes `name_case_conflict` (input) and `folder_not_empty` (policy); `value_unparsable` is now one of the pack's own codes too.
- **A gap in the kernel, found here.** An operation that writes one fixed place (a settings value, a JSON record at a path with no key, a workbook's named cell) has no identity input to give: the compiler asks one input for `natural` and refuses `none` for a write. The fixtures name such a record by an input the binding does not use. The compiler should accept an identity of no input where the lowering says the binding writes one fixed place. This is for hypothesis H-26.
- The stand gains an INI file, a property list, a folder of notes, and a growing log, and the change `Appended`.
