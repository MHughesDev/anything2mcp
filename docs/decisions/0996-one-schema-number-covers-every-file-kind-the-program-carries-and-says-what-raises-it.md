# 0996. One schema number covers every file kind the program carries, and says what raises it

**Status:** Accepted. Design decision, made in the design review round by lens 14, a year later ([report](../reviews/a-year-later-review-2026-11-25.md), finding 6). The owner had not read this record when it was written. It states what `schema_version` in `target.yaml` promises, which no document said. It changes no code today: the compiler reads number 1, the only one there is.

**Date:** 2026-11-25

**Changes:** the [file formats reference](../reference/file-formats.md) (its opening and `schema_version`); [section 206](../spec/2-kernel/206-the-compiler-and-the-compiled-target.md) ("Compatibility"); [section 604](../spec/6-running-it/604-management-and-operations.md) ("Upgrades").

## Context

A target's files are what a person keeps longest: written by hand or by an agent, reviewed, committed to their own repository, and edited for years. `target.yaml` states `schema_version`, and the compiler refuses any number but its own, 1, as `kernel.schema_version_unsupported`. Every map is closed, so a key the compiler does not know is `kernel.unknown_key` and a word it does not know is `kernel.unknown_value`.

The documents left four things open:

1. **Whose number it is.** The kernel owns eight file kinds and the packs bring the rest (`screens/`, `endpoints/`, `commands/`, and so on). A pack registers its kinds' schemas and has no number of its own, so it was not said whether a change to a pack's kind raises the target's number.
2. **What raises it.** The file formats reference said "after the first release, a change is a new schema version with a migration": every change, an added optional field included.
3. **How long an old number loads.** Section 206 said "a newer compiler reads older versions and can rewrite files forward"; section 604 said "older formats keep loading, and `version.migrate` rewrites them"; the action catalog says `version.migrate` recompiles stored sources for a newer engine. None said how many numbers back, and the catalog and section 604 gave the action two jobs.
4. **A new refusal.** Section 605 counts "a target file that used to compile and no longer does" as breaking. Most decisions that tighten the compiler add such a refusal, for a good reason each time, and say nothing about the files it newly refuses.

A published version is not at risk from any of this: its compiled target and its source files are stored together, and the engine runs the compiled target ([section 206](../spec/2-kernel/206-the-compiler-and-the-compiled-target.md)). What breaks is the person's next edit and publish.

## Decision

1. **One number for every file kind the program carries,** the kernel's and every pack's, because every pack ships inside the one program at its version. A plug-in's own file kinds, once the plug-in host is built, are numbered by the plug-in's manifest.
2. **What raises the number:** a field or a word removed or renamed; a field's meaning, type, or default changed; an optional field made required. **What does not:** a new optional field, a new file kind, or a new word in a list. A file written before an addition still loads; a file that uses one is refused by an older compiler as `unknown_key` or `unknown_value`, by name, which is a stop and never a guess.
3. **A compiler reads its own number and the one before it.** `version.migrate` rewrites a project's files from the number before to its own, and recompiles the stored sources of a published version whose compiled target format the engine no longer reads; it is built with the first number 2, by the change that brings it, and the catalog's one line names both jobs then.
4. **A new refusal arrives as a warning first.** A rule that would refuse a file the release before compiled is a warning, with its code, in the release that adds it, and an error from the next minor version (before 1.0) or the next major. Where the file it refuses would be unsafe, or could not run, it is an error at once, and the changelog names it as a break.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A number for each pack, as a map in `target.yaml` | A person would keep several numbers that only ever move with the program's own version; the one number already changes when any of them would |
| Raise the number on every change, as the reference said | An optional field would then refuse every file written for the number before until it is migrated, for a change that breaks nothing |
| Read every number ever made | Each old number is a second schema to keep correct forever; one number back gives a person one release in which to run `migrate` |
| Make every new refusal an error at once | Turns each tightening into a break for files that ran safely, and teaches people to pin an old version |

## Proof

None to build now. When the first number 2 is made, its change carries a test that a target folder kept at number 1 compiles, is rewritten by `version.migrate` to 2, and compiles to the same tools. A new rule that refuses what compiled before carries a test that it is a warning in the release that adds it.

## What would reopen it

A plug-in host whose plug-ins bring file kinds that people use as much as a pack's, which may call for numbering them in `target.yaml` after all.
