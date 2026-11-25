# 0102. The `file` mode, and what the file pack registers

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** item 8 of the entry bar in [spec section 308](../spec/3-packs/308-the-file-pack.md), the decision record for the mode; which formats the pack reads and writes first; its outside crates.

## Context

[Decision 0060](0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md) makes a mode real by a record when its pack is built. Packages H2 and H3 of the roadmap built the file pack, `a2m-pack-file`, against the stand in `targets/example-files`.

## Decision

**The mode.** `file`, of the artifact archetype. Its verbs are `read`, `list`, `update`, `create`, and `delete`. Its steps are the archetype's five: `open`, `locate`, `set`, `read`, `save`. One write holds one file alone (`exclusive = "file"`). The provider is named `file`.

**What it registers.** The surface kind `file`, whose address is `folder`, a folder inside a named root, with the optional field `root` (`files` when absent). The file kind `document`, in `documents/`. The finding kinds `file_layout` and `file_key_candidate`, the discoverer `file_layout`, and one drafting rule from `file_layout` findings to `document` files. Four lint rules: `file.stands_undeclared`, `file.beneath_application`, `file.open_without_signal`, `file.format_read_only`. The settings `packs.file.max_bytes`, `max_rows`, `max_cells`, `max_cell_bytes`.

**Roots.** The manifest declares that the pack reads the roots `project` and `files` and writes the root `files`. A file surface is in one of the two. A machine whose root `files` is read only does not load the pack, because the registry refuses a declared write root that cannot be written; a target that only reads puts its surfaces in `project`.

**Formats.** Read: delimited text, JSON, and workbooks, each through the parsed form of `a2m-sources`. Written: delimited text and JSON, by this crate's own patch editors. A workbook is read-only until its editor passes the two round-trip tests. YAML, TOML, XML, and text documents are named by the schema and refused with `file.format_not_built`: `a2m-sources` reads none of them, and a second reader with exact positions for each is more than their use yet justifies.

**Outside crates.** None beyond what the workspace already declares. The replace is `std::fs::rename`; the flush is `File::sync_all`, on the temporary file everywhere and on the folder on Unix.

## Alternatives considered

- **A mode per format** (`csv`, `json`, `workbook`). One driver, one save, and one identity rule serve all three; the format is a property of the document.
- **A small YAML or TOML reader of the pack's own, for reads only.** It would be a second parser beside the compiler's, and the format's hard part, writing without loss, would still be missing.
- **A zip writer for workbooks now.** The package must be copied entry for entry without recompressing; that is its own slice, with the formula rule.

## Revisit if

A target needs to read YAML, TOML, or XML as a system; a workbook editor passes the round-trip tests; or a second root for written files is needed beside `files`.

## Spec sections

- [308. The file pack](../spec/3-packs/308-the-file-pack.md)
- [201. Archetypes and modes](../spec/2-kernel/201-access-kinds-and-modes.md)
- [`a2m-pack-file`](../architecture/crates/a2m-pack-file.md)
