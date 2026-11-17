# 0155. Four packs state what saves, what writes, and when their guard lifts

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** how the data, device, stream, and file packs take up what the kernel's safety fixes offered a pack ([0147](0147-the-engine-after-its-safety-audit.md), [0148](0148-the-provider-contract-and-the-scheduler-after-their-safety-audit.md), [0153](0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md)). **Supersedes** nothing. The crates are `a2m-pack-data`, `a2m-pack-device`, `a2m-pack-stream`, and `a2m-pack-file`; each design names the tests.

## Context

The kernel now holds a pack to what the pack says about itself: which step kinds save, the least effect a binding has, which of its file kinds can describe a write, and, for the handle, which step kinds always save. Until a pack says so, the kernel has only the pack's word at run time. A pack also has a posture guard of its own. The engine gives `Sandbox { test_records: true }` only to the context that writes a declared test record, and cannot see what a step that is not marked `commits` does there. So the guard must be lifted for the one marked step and kept for every other.

## Decision

### What each step kind is declared as

| Pack | `Always` | `AsDeclared` | `Never` | Manifest `commit_steps` |
| --- | --- | --- | --- | --- |
| data | `commit` | none | `begin`, `execute`, `fetch`, `expect_rows`, `catalog` | `commit` |
| device | none | `write_frame` | `open`, `build_frame`, `read_frame`, `expect` | none |
| stream | `publish`, `acknowledge` | none | `subscribe`, `await`, `latest`, `browse`, `scan`, `channels`, `describe` | `publish`, `acknowledge` |
| file | `save` | none | `open`, `locate`, `set`, `read` | `save` |

**Why `write_frame` is `AsDeclared`.** The same step kind sends the frame that writes a register and the frame that asks for a register's value. The lowering marks only the first as committing. `Always` would make the compiler refuse every read; `Never` would make it refuse every write. The claim is not made. The driver holds the write itself: a write frame not marked `commits` is refused in every posture. For the same reason the device manifest lists no `commit_steps`.

**Why a stream `subscribe` and `await` are `Never`.** On a queue they attach a consumer and hold a delivery that is given back unless a later `acknowledge` removes it. Nothing is saved until the acknowledgement. The data pack's `begin` is `Never` for the same reason: a writing transaction changes nothing until `commit`.

### The least effect of a binding

By the verb. Data: `delete` is `irreversible`, `create` and `update` are `write`, the reads are `read`. Device: `submit` is `irreversible` (a command cannot be read back), `update` is `write`, `read` is `read`. Stream: `take` is `irreversible`, `publish` is `write`, the rest `read`. File: `update`, `create`, and `delete` are `write`, `read` and `list` are `read`. A hazard asks for more through each pack's own lint rule, as before. A write can no longer compile as a read whatever a target file says; each pack has a test.

**The file pack's `delete` is `write`, not `irreversible`.** The fixtures of the pack declare it `write`, and the file is replaced whole with the row removed and then read back absent. Section 308 makes a write beneath an application `irreversible` unless read back through that application, which the lint rules of the pack already hold. Whether a plain removal should be `irreversible` is a judgment about safety that the documents do not settle; it is left as it was and listed in the report.

### Which file kinds can describe a write

A kind says yes when its file states what only a write needs, and when a value is not plainly the safe one. Data: a statements file where any statement is not plainly `effect: read`. Device: a map where any register is not plainly `access: read`. Stream: a channel file that states an `ownership` other than `shared`; a schema file never. File: a document with a field marked `writable`, or one that states `may_be_open` or `lock_signal`. The stream channel's `idempotency` is not counted: it says how the broker treats a repeat and lets nothing run, and the drafter writes it from what the broker stated. Each pack has a test in which a drafting rule that drafts a write is refused and one that only reads is written.

### The posture guard

In `Sandbox { test_records: true }` each driver lifts its own guard for the step marked `commits` alone.

- **Data:** a `commit` not marked `commits` is refused unsent and its transaction rolled back, in every posture.
- **Device:** a write frame not marked `commits` is refused in every posture; the lift is for the marked frame.
- **Stream:** `publish` and `acknowledge` need a live run or the marked step; so does the `subscribe` of a `take`. A `take` therefore runs only live. This is stricter than before, and it follows the rule as stated: the `subscribe` of a take is not marked, and the engine cannot see what a step not marked does there. Whether a take should be testable against a declared test record is a judgment the documents do not settle; it is listed in the report.
- **File:** the driver had no guard of its own and relied on the roots' access. It now keeps the run's posture and refuses a `save` unless the posture is live or a sandbox with test records and the step is marked `commits`. `FileDriver` and `DocumentKind` are exported so that a test can reach the driver below the handle, where the guard can be seen working.

Each pack has a test with a hand-built step that would change something and is not marked. It is refused, and the test stand shows nothing changed. For the file pack the test was checked by taking the guard out: it then fails.

### The smaller changes

- Tests that compile a draft pass the compiler's draft version.
- The file pack takes the provider's `CheckedPath::entry_kind` for a document's own name and for a file handed to discovery. It keeps its own look at a lock signal: the roots' check cannot join a name that is a link to nothing, and that name is a signal. It keeps its own refusal of a link at the final name, and its own save. Every test of [decision 0149](0149-the-file-pack-after-its-safety-audit.md) passes.
- The stream tests dropped imports that no test used; the file tests the same.

## Alternatives weighed

- **Declare `write_frame` `Always`, and split the step in two kinds.** It would give the device pack a precise claim. It changes the pack's step kinds and its plans, and the driver already refuses the unmarked write. Not done; the claim is `AsDeclared` and the reason is written.
- **Keep the stream `take` testable in a sandbox by leaving the `subscribe` guard lifted.** It lets a take run against a test record. It also lets a step that is not marked attach a consumer to a queue in a context that cannot see it. Rule two decided: the guard is kept, and the cost is stated.
- **Make a file pack's `delete` `irreversible`.** Safer on paper, and not what the documents say. Not decided here.

## Consequences

- A compiled plan of these packs can no longer mark a step against what the pack says.
- A stream `take` cannot run in a test-record sandbox until someone decides it may.
- A stream `publish` or `take` to a queue or a topic still cannot run through the engine: the engine refuses a write binding with no read-back unless the binding says the answer is the evidence, and the stream binding has no way to say it. See the report of this package; nothing was changed for it.
- Not verified here: the Windows and macOS builds of the file pack (the tree compiles C below it, so no cross-target lint could be run from this machine).
