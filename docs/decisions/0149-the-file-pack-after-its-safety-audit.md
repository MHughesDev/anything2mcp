# 0149. The file pack after its safety audit

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** eleven findings of a reading of `a2m-pack-file` against [spec section 308](../spec/3-packs/308-the-file-pack.md), its [design](../architecture/crates/a2m-pack-file.md), and the rules of `AGENTS.md`: six serious, five minor. Each was checked with a test before it was fixed. **Supersedes** nothing that decisions [0102](0102-the-file-mode-and-what-the-file-pack-registers.md) and [0103](0103-the-file-pack-as-building-it-settled-it.md) decide. It corrects one sentence of 0102, "One write holds one file alone (`exclusive = "file"`)": that is what the mode declares, and nothing enforces it yet. It replaces the temporary file's name that section 308 gave, which was made of the record's token alone.

## Context

A file is the easiest system to damage without noticing. The pack as first built reached every file through the path the roots' check resolved. That check follows a link while the link stays inside the root, which is right for a path a caller reads through and wrong for the three things this pack does by name: it replaces a document's file, it makes and removes a temporary file, and it takes a sibling's presence as a lock signal. Rule two decides every doubt below: safe, then correct, then fast, then easy.

## Decision

### The name itself, never what a link at it points to (findings 1, 2, 8)

**Confirmed, all three.** A link planted at the temporary file's name made the save delete another file in the root and take it over. A document's file that was a link was read and written through to its target, and the "not a regular file" check looked at the target. A lock signal that was a link to nothing counted as absent at the save, and stopped the read under the wrong name.

- The pack gives the operating system the surface's checked folder with one checked name added, and asks about that name itself. A link at a document's name is `path_not_allowed`, for a read and a write, wherever it points. The handle that is opened is compared with the name by identity, so a link put there between the look and the open is caught before a byte is used.
- **A link in the way to the surface's folder** is followed by the roots' check while it stays inside the root. A read may go through it. A write may not: it would land in a folder the surface does not name, past that folder's own `stands` declaration. `path_not_allowed`, at `open` and again at the save.
- **A temporary file** is named `<file>.a2m-<token>-<random>.tmp`: the record's token, so a person can tell whose it was, and sixteen characters from the operating system's randomness. It is made only if nothing has the name, by a call that follows no link. What has the name is never removed, opened, or written; the save tries four names and then stops with `file_unavailable`.
- **Removal is by identity.** The driver records the identity of the file it made, and removes a file only when the thing at the name is a regular file with that identity. On Unix that is the device and the file's number. Elsewhere safe code is given neither, and the moment the file was made, with its length, stands in.
- **A leftover of a killed save is never used and never removed** by a later run, which cannot prove it made it. This is weaker for tidiness than the first design, which removed "the temporary file named with its token", and it is the only reading of "removes nothing it cannot prove it wrote" that a planted name cannot turn against another file.
- **A lock signal counts by being there:** a file, a folder, or a link, whether or not the link points at anything. A signal that cannot be looked at stops the write.

### A write needs the target's word on whether the file may be open (finding 3)

**Confirmed.** Absent was read as `false`. The lint `file.open_undeclared` refuses a binding that writes a document which does not state `may_be_open`. The driver refuses a plan that holds such a write, and one with `may_be_open: true` and no lock signal, before the file is read: the compiler's refusal is repeated where the harm would be done.

### Two runs on one file (finding 4)

**Confirmed, and not the pack's to close.** Nothing in the engine or the scheduler reads a mode's `exclusive` setting. In the pack the digest check was there; it is now the last act before the replace, after the temporary file is written, flushed, and given its owner and bits. Of two runs that read one file, the second to save is `stale` and the first run's change stays: a test holds this. What remains is the time of one `rename` call after the comparison. A second save that falls inside it undoes the first, and the save cannot see it.

### Text that a spreadsheet program would run (finding 5)

**Confirmed.** No document addressed it. A text value for delimited text that begins with `=`, `+`, `-`, or `@`, in either width and after any spaces, is refused before anything is built: `input`, `value_begins_formula`, a new code. So is a new row's key. The value is never changed, because rule three reads back what was written, and a quote put in front would be stored.

- A field declared `may_begin_formula: true` in its document may begin with a sign. The developer says by it that the column is free text and that no spreadsheet program opens the file. What protects a person then is that declaration, in a target that is reviewed, and the value in the diff they approve. Nothing in the file protects them.
- A tab or a line break at the start is refused even there. No value needs one, and a program that drops it would then see what follows.
- A number is spelled by the pack itself, and its sign is not refused. JSON is not opened as cells.
- Quoting was checked in both directions and found right: a delimiter, a quote, and a line break inside a value are quoted, a quote is doubled, and the stand's own reader and the pack's read-back give the text back. A test now holds it.

### The right to write the file, and whose file it is afterwards (findings 6, 7)

**Confirmed, both.** A replace needs only the right to write the folder, and the new file was the engine's user's.

- The save opens the target for writing and writes nothing. Where the operating system refuses, the file is not replaced: `policy`, `file_protected`. This asks the system itself, so it follows access lists and read-only volumes as well as permission bits.
- The new file is made readable and writable by the engine's user alone, and is given the old file's owner, group, and permission bits through its open handle before the new bytes are written. The bits are set again after the write, and the file is looked at. Where any of the three cannot be kept, the save stops with `metadata_not_kept` and removes its temporary file.
- **Not closed:** extended attributes and access lists are not carried, and the pack cannot tell that a file has them. On Windows the new file takes the folder's access list.

### The smaller ones (findings 9, 10, 11)

- **9, confirmed in part.** The kernel's schema makes `authorization` a text, so an empty record was not accepted; a blank text was. The rule now asks for a reference that is not blank. What the record covers lives outside the target's files, and the rule cannot see it.
- **10, confirmed.** An open question held a header and a file name from the file. It now holds the pack's own words and names a record kind and a field by their place in the draft.
- **11, confirmed.** Tests were added for a link inside the root, a file of another owner, a confirm that finds another file, and the rule that reads the file's bytes after a refused replace. What still has no test is listed in the design, with the reason for each.

## Alternatives weighed

*Ask `CheckedPath` for all of it.* The right home for "look at a name without following it", "make a file only if nothing has the name", and "give me the file's identity" is the provider contract, beside the check. The contract's own `write` was being changed for the same defect while this was written, and this package could not change that crate. The pack therefore adds one checked name to the checked folder's path itself. Two of the things it needs arrived in the contract meanwhile: `entry_kind`, a look at a name that follows no link, and an `open` that follows none and reaches the folder from the root down. The pack should take both in place of its own, which also closes the swap of a folder between a look and an act on Linux and macOS. Three are still missing, and a save cannot be built on `write` without them: a temporary file made under a random name where nothing has it, handed to the caller open so that it can be given an owner and bits, flushed, and held until the digest has been compared; a file's identity; and a way to ask whether the engine's user may write a file.

*Resolve the name and compare the two paths.* A link shows as a difference. So does a file name whose letters differ in case from the document on a file system that ignores case, and a short name on Windows. A look at the name itself says "link" and nothing else.

*Keep the token as the whole name, and refuse a leftover that is a link.* The fix the reviewer sketched. It still removes a file it cannot prove it made: a regular file somebody put at a name anyone can work out.

*A quote in front of a formula.* It is what many exporters do. The stored value is then not the value that was approved, and a later read returns the quote.

*Refuse every text that begins with a sign.* Simpler, and it makes a column of notes that may begin with a dash unwritable for ever.

*An advisory lock on the file for the length of a save.* It would hold two runs of the engine apart on one machine. It cannot be tested at a chosen moment, it means different things on Windows and on a shared folder, and the scheduler is where one file is held to one run.

*Give the file to the engine's user and say so.* The first build did, silently. An application that can no longer read its own data file is harm done after a write that was reported as verified.

## Consequences

- A target whose document is written and does not state `may_be_open` no longer compiles. `may_be_open: false` is the line to add where no application opens the file.
- A surface whose folder is reached through a link can be read and not written, until the surface names the real folder.
- A temporary file a killed save left stays in the folder until a person removes it. Its name starts with the file's name and `.a2m-`.
- The pack's section of the plan is version 2. A plan compiled before does not decode, and the driver says to compile again.
- The pack depends on `getrandom`, which the workspace already holds.
- The stand `targets/example-files` gained four faults and reports a file's owner, group, and permission bits on Unix.

## Asked of other crates

- **`a2m-provider`:** the three things still missing under the first alternative.
- **`a2m-sched` and `a2m-engine`:** read a mode's `exclusive` setting and hold one file to one run.
