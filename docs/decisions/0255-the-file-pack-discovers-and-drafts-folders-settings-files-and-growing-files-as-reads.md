# 0255. The file pack discovers and drafts folders, settings files, and growing files as reads

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** [section 308](../spec/3-packs/308-the-file-pack.md), Discovery and drafting, and the [design of `a2m-pack-file`](../architecture/crates/a2m-pack-file.md). It builds the row "Discovering and drafting folders, settings files, and logs" that [decision 0246](0246-the-file-pack-reads-folders-settings-files-and-growing-files.md) left not built.

## Context

Decision 0246 gave the file pack three kinds a person could reach only by writing the target's files by hand: a folder as a collection of entries (`folders/`), the settings formats `ini` and `property_list` of `documents/`, and a growing file (`logs/`). The discoverer read only the files the kernel handed it, and the engine hands a discovery job none: it gives the run's project folder alone. So in the product the file pack discovered nothing.

## Decision

1. **What discovery reads.** When the kernel hands files, the discoverer reads those, as before. When it hands none, it reads the surface's own folder: the place the target names, resolved from the roots the pack was handed exactly as the driver resolves it, for reading only. No input chooses it: the job's focus is not read ([section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md)). The roots' check refuses a folder that leads out of its root. Inside the folder no link is followed and nothing that is not a regular file under its own name is opened.
2. **A folder** is a finding `file_folder`, `discovered`: each entry's name, kind (`file`, `folder`, `link`, `other`), and a file's size, to the bound's depth (the manifest says 1; the kernel may tighten it; never more than 4, the most a `folders/` file declares), at most 1,000 entries. No content and no modification time are recorded. A folder at the depth looked to is named under `cut`, the finding is `partial` with the limitation `truncated`, and the report names the bound `depth`.
3. **Each regular file directly in the folder** whose name the pack knows is then read: delimited text, JSON, and workbooks as before, `.ini` and `.plist` as settings files, `.log` as a growing file.
4. **A settings file** is a `file_layout` finding, `discovered`: an INI file's sections, a property list's root keys, and each value by its path of keys and its shape (`text`, `number`, `boolean`, `empty`, `list`, `record`, `date`, `data`, `unread`), never the value. An INI value's shape is how its text is spelled; a property list's is its element. A form 0246 does not read as one value, and a key the file holds twice in any ASCII case, is `unread`. Where any key on the path names a secret, or the value has the form of a credential (key material, an authorization header's value, a signed token, a long run of letters and digits), the shape is `withheld`, the finding says why per value, and it carries the limitation `redacted`. An INI file that does not read with `=` and reads with `:` is recorded with that separator.
5. **A growing file** is a finding `file_log`, `inferred`, with the limitations `not_exercised` and `sampled`: that other programs append to it is taken from its name, never watched. Its first complete lines, at most 64 in the first 64 KiB, are looked at for one form: one JSON record per line; fields apart by one delimiter (tab, comma, bar, or semicolon), the same number in every line; or words of the form `key=value`. Where none fits, more than one fits, or a line is not UTF-8, no form is named (rule 7). No line is kept.
6. **Drafting is reads only, and each draft compiles as written.**
   - A folder: `folders/<surface>.yaml` with `surface` and the depth at which entries were found; beside it a `list` of the folder and a `read` of one entry by its name. The folder's file states no `writes` and no `may_be_open`, so no write compiles over it (the lowering says the folder "does not declare that it is written by" the verb). The read gives no content until a person adds `content: true`.
   - A settings file: `documents/<file name>.yaml`, named by the whole file name (`example_ini`), because a settings file and a data file of one stem often stand side by side. One record kind for the keys before the first section, for each section, and for each dictionary that holds a value; a field for each value of a shape the pack reads. An INI value is `untrusted_text`, whatever it spells, as a delimited cell is. A property list's `string` is `untrusted_text`, a number `decimal`, true or false `boolean`. A withheld, unread, list, record, date, or data value is never a field, so no drafted read gives it to a caller (rule 4).
   - **One read per record kind**, not one per key and not one of the whole file: `<target>_<document>_<record>_get`, no input, one output per field. A settings record is one place, so the read holds no question.
   - A growing file: `logs/<stem>.yaml` with `surface` and `file`, and a `read` from the position an optional input gives, with `lines` as `untrusted_text`.
   - Nothing for a write: not a folder's create, update, rename, or delete; not a settings update. The drafter's own refusal of a file that writes stands behind the rules, and the pack's tests assert its reason.

## Alternatives weighed

- **The kernel hands the surface's folder in `DiscoveryScope::paths`.** Each pack would be handed its place by the engine, and the pack would hold no roots in its discoverer. Not taken now: the engine has no plan field that names a surface's files (the crate design's first open point), and the driver already resolves the surface's folder from the same roots by the same check. Owed to the engine if it gains one, and the pack's own resolution can then go.
- **One read per key.** A tool for each value is many tools for a small file, and one value is seldom wanted alone. **One read of the whole file**: a binding reads one record kind, so it would need a binding per section or a new kind of place. Not taken.
- **Typing an INI value by its spelling** (`103` as an integer). The file states no type; a key spelled as a number in one file is text in the next. The shape is recorded as evidence for the person, and the draft stays `untrusted_text`, as for delimited text.
- **Recording a withheld value's shape anyway.** A shape says little, but the rule is that nothing of a secret reaches a finding; the per-value reason is enough for the person.
- **Taking every file whose lines look alike as a growing file.** One look cannot see a file grow. The name is the pack's only sign, and the finding says it is a sign and not an observation.
- **Keeping a sample of a growing file's lines.** No existing discoverer keeps one; a line may hold anything a program logged. None kept.

## Consequences

- The manifest declares the finding kinds `file_folder` and `file_log` and four drafting rules more: `file_folder` to `folder` and to `operation`, `file_log` to `log` and to `operation`. `LayoutFound` gains `sections`, `settings`, and `separator`.
- The pack's list of key words that name a secret repeats the kernel's credential lint, which `a2m-files` keeps private. Knowledge in one place asks the kernel to export it; owed.
- Not built: discovering a folder or a file handed by the kernel as a folder; a growing file named otherwise than `.log`; a binary property list (a named stop, as for reads); a JSON per line, delimited, or `key=value` form carried into the draft as fields of a line.

## Three systems

| Matter | Proven here | Only CI proves |
| --- | --- | --- |
| A link in the folder is an entry of the kind `link`, never opened | Linux, a link to a file and to a folder outside the root | Windows, where the stand makes a link only for a user who may; and whether a junction is reported as a link |
| Names in the order of their bytes; endings matched in any case | Linux | macOS and Windows, where the file system folds case |
| A growing file's line ends in LF or CRLF | Linux | The same test on Windows and macOS |
