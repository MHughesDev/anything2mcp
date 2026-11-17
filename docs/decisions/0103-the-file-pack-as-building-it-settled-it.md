# 0103. The file pack, as building it settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** where [spec section 308](../spec/3-packs/308-the-file-pack.md) was silent or the built kernel said otherwise. Several small choices share this record.

## Decision

**An element of a JSON list is chosen by the record's key, not by a selector in the path.** Section 308 wrote `path: [warehouses, {key: code, equals: input.code}]`. A document cannot type-check an expression that reads an operation's inputs, because it belongs to no operation. A path is now a list of keys only. With `key`, the path leads to a list and each element is one record, found by comparing the key fields, typed, with the values the binding gives under `key`. Rows of a table and elements of a list are located the same way.

**A document is named by its file.** `documents/stock.yaml` is the document `stock`. One file holds one document.

**The file is read in the `open` step, not in `prepare`.** `prepare` is given a surface, not a binding, so it cannot know which document a run will touch. It resolves the surface's folder and loads the field types. Each `open` reads the file from disk as a new read; the read-back is therefore `open`, `locate`, `read`.

**The folder comes from the pack's own roots.** The driver keeps the roots its manifest declares, handed to it when it registers, and asks them for the folder and for each file in it. It does not use `PrepareContext.paths`: nothing in the plan tells the kernel which files a surface holds.

**Digests for reconcile.** Every `open` returns `file_digest`. `set` returns `before_digest` and `intended_digest`. All three travel in `Observed.for_reconcile`, which the contract now has.

**A value that already agrees is not rewritten.** `set` leaves a place alone when its typed value equals the intended one, so its spelling survives: `0040` stays `0040` when 40 is written.

**The untouched check has three parts.** The new bytes are parsed again; every place outside the named ones is compared, token for token, with the old; and the named places are read back from the bytes about to be saved and compared with the intended values. Any difference is `round_trip_failed` and nothing is written.

**A file marked read-only is never replaced,** on any operating system: `policy`, `file_protected`, before anything is written. On Unix a replace would succeed over such a file; on Windows it may not. Refusing on both makes the rule the same everywhere and respects the owner's mark.

**`path_not_allowed` is of the class `input`.** The provider contract defines the code with that class, and a code has one class. Section 308 said `policy`.

**A layout finding has the source `live_system`.** For this pack the file read from disk is the running system. The core gives `artifact` findings at most `inferred`, and section 308 asks for `discovered`.

**A JSON file with a duplicate key is not read.** The parser in `a2m-sources` refuses it, so it is `file_unreadable`, not read-only.

**Not built, and why.** `upsert`, which is decided at the preview by the engine. A list of rows in one save. Writing a date, a time, or a measurement: the kernel has no public function that spells a value the way its field type shows it, so the pack spells text, whole numbers, decimals, money, booleans, and choices itself, and refuses the rest with `shape_unsupported`. The check of a single link on Windows, where safe code cannot count a file's names.

**Windows, unproven here.** A replace that fails while another program holds the file is reported `busy`, `file_locked`, for the sharing and lock violations and for a refusal of access. When the operating system's answer leaves the outcome in doubt, the file is digested once more: the intended digest is success, the old digest is a named failure, and anything else is `possible_commit`.

## Alternatives considered

- **Keeping the selector and reading `input.<name>` as text.** An expression that is not checked is a string that is split.
- **Using `CheckedPath::write` for the save.** It does not flush, cannot name its temporary file with the record's token, and cannot check a digest between writing and replacing.
- **Reading a JSON file with a duplicate key through a second parser.** One parser per format is the rule.

## Revisit if

The plan gains a way to name the files of a surface; the kernel gains a public formatter for field types; or CI on Windows shows another error for a file held open.

## Spec sections

- [308. The file pack](../spec/3-packs/308-the-file-pack.md)
- [203. The provider contract](../spec/2-kernel/203-the-provider-contract.md)
- [`a2m-pack-file`](../architecture/crates/a2m-pack-file.md)
