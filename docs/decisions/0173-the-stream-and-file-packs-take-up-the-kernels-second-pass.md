# 0173. The stream and file packs take up the kernel's second pass

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the designs of [`a2m-pack-stream`](../architecture/crates/a2m-pack-stream.md) and [`a2m-pack-file`](../architecture/crates/a2m-pack-file.md); sentences of spec sections [309](../spec/3-packs/309-the-stream-pack.md) and [308](../spec/3-packs/308-the-file-pack.md). Takes up decisions [0161](0161-the-plan-and-the-engine-after-the-packs-were-built.md) and [0162](0162-the-provider-contracts-second-pass-what-a-step-is-handed-and-hands-back.md). Builds on [0149](0149-the-file-pack-after-its-safety-audit.md), [0150](0150-a-take-records-its-message-before-it-acknowledges-and-an-answer-with-no-rule-leaves-a-write-in-doubt.md), and [0167](0167-the-stream-pack-speaks-nats-with-jetstream-through-a-client-of-its-own.md).

## Context

The kernel's second pass added, with defaults, what the packs' audits asked for: what earlier steps recorded, how a step was done, the system's own words, a write's evidence, the run's project folder, and a save in steps. Each becomes real for a pack only when the pack takes it up. These are the choices the two packs made in doing so.

## Decision

| Pack | Choice | Instead of, and why |
| --- | --- | --- |
| Stream | **Evidence by what can be read.** `acknowledged` for a `take` and for a `publish` that has no read-back (a queue, a topic, a log that declares no idempotency header); `read_back` for every other publish. A keyed publish to a log states `read_back`: the scan for its token finds that very message. The key alone reads nothing back, because on a log it is only the subject's last token and another producer may use it | `acknowledged` for every publish to an outside reader: it would drop a read-back the pack can make |
| Stream | **A position recorded before a publish.** A new step kind, `position`, runs before a publish that is read back by a scan. It reads where the log ends and gives it for reconcile as `scan_from`, a name no operation's output has. A scan starts at this run's confirmed position, else at `scan_from`, else at the log's start; the trace says which | The publish's own answer: a committing step may give nothing for reconcile. `describe` reused in a live run: it is the discoverer's, and refused there. The name `position`: the engine keeps recorded values beside the outputs, and an operation's `position` output would have been the log's end, not the message's |
| Stream | **A take is read back by a look at its queue.** A new step kind, `taken`, a lookup, in the take's `read_back`: the list the engine reads a record with values recorded before its commit point by. It reads the recorded `message_id`, and the message's recorded position where the protocol gives one. Gone from a queue the runtime owns means the acknowledgement landed (found); still held means it did not (absent). Over `nats` the one sequence is read; elsewhere the whole queue, and a look that shows fewer messages than the queue holds, or says more is held, settles nothing. Handed nothing recorded, it stops unsent with a new code, `input`/`nothing_recorded` | The `lookup` list: the engine searches by lookup only for `stamped` and `find`. No step, as before: a person settled every take in doubt by hand |
| Stream | **The system's words** in a refusal and in an answer of no rule are the failure's `said`, redacted, then cleaned and cut; the sentence is the product's | — |
| File | **The save keeps its own temporary file.** It takes `CheckedPath::may_write` for the right to write the file, `CheckedPath::flush_folder` after the replace, and a read-back of the temporary file before the last digest comparison; it keeps its own temporary file, write, and `rename`. A flush that fails after the rename leaves the save in doubt | `create_temporary` and `Temporary`: the contract's temporary file hands out no handle, so the new file could not be given the old file's owner, group, and bits before a byte is written, and the save would have had to stop with `metadata_not_kept` or give them up; it is made readable by more than the engine's user; and it is removed by its name, where decision 0149 removes only a file whose identity it recorded. Ignoring a failed flush, as before: a name that may not outlast a power cut is not a verified save |
| File | **The contract's codes are mapped.** `path_missing` is `file_missing`, `path_denied` `file_protected`, `path_locked` `file_locked`, `disk_full` `storage_full` | Passing them on: two codes would then mean one thing |
| File | **A project document is the run's.** A surface in the root `project` is below `CheckedPaths::project_folder()`, and is read and never written, as the manifest declares (`read_roots` holds `project`, `write_roots` does not). With no project folder the run stops with `project_folder_missing` | The pack's root `project`, which is the folder the engine was started in. Writing the caller's project: the manifest never declared it, and widening a pack's effects is not this package's to decide |
| File | **A delete is `irreversible`.** No binding can declare how a removed row is restored: the old file is gone once it is replaced, and the pack keeps no copy | `write`, as before: it called undoable what is not |

## Consequences

- The stream pack's manifest gains the step kinds `position` and `taken` and the code `nothing_recorded`. The plan of a publish read back by a scan has one step more. The example operations that offer `acknowledged` were already `irreversible`.
- The file pack's fixtures declare both deletes `irreversible`. A target that declares a delete `write` no longer compiles.
- What only the engine can show: a take and an acknowledged publish ending `acted` (the ledger's `Control::acknowledged` is being built); the `taken` look run by reconcile (the engine stops a record with no identity before it reads); the recorded values handed back on a real resume. The packs' tests play each through `DriverHandle::act_recorded`.

## What the provider contract would need for the file pack to use its save whole

1. `CheckedPath::create_temporary` makes the file readable and writable by the engine's user alone (mode `0o600` on Unix).
2. `Temporary::keep_owner_and_mode(&mut self) -> Result<bool, Failure>`: gives the temporary file, through its open handle, the owner, group, and permission bits of the file it will replace, looked at without following a link, and looks that it has them; `Ok(false)` where any cannot be kept. Callable before the first write and again after the last.
3. `Temporary`'s removal, on drop and after a failed `replace`, only while what stands at its name is the file it made, by the identity of its own handle.
4. `FileIdentity` of an open handle, to compare with `CheckedPath::identity`.

## Revisit if

The contract gains the four above: the file pack then moves its save onto `Temporary` whole. Or the engine reconciles a record of no identity from what it recorded: the `taken` look then runs without a person.
