# 0999. A closed kernel holds no clone of its store

**Status:** Accepted. Design decision, made by the worker on the Windows failure of the copied data folder test under the lead's brief. The owner had not read this record when it was written. It changes what `Kernel::close` does and how the approval desk reaches the store; no public interface changes.

**Date:** 2026-11-24

**Changes:** `a2m-actions` (`src/kernel.rs`, `src/person.rs`, `src/runner.rs`); the design of `a2m-actions`.

## Context

On Windows, the test `a_data_folder_put_back_holds_the_copy_and_its_stopped_run_resumes_with_no_second_write` could not remove the data folder after `Engine::close`: the store's database file was open in the same process (`os error 32`). SQLite opens its files on Windows shared for reading and writing and not for removal, so a folder that holds an open database cannot be removed or replaced; Linux removes it. The standard library opens every other file shared for removal too, which is why the engine's log file, held by the process's log writer, is no such fault.

`Kernel::close` let go of the store's lock on its data folder, and its comment said a clone of the store may outlive the kernel for a while. Measured on Linux through `/proc/self/fd` after close, two clones did, holding the database and its write-ahead files open:

- **The approval desk.** `Kernel::open` hands the desk to the person surface, and the desk held a clone of the store and the policy engine's approval surface. A surface that lives longer than one kernel keeps them: the tests' surface serves one engine after another on one machine.
- **A job in hand.** A job runs on a thread of its own, which cannot be aborted, holding a clone of the kernel until its job ends. A caller is told a run ended once the scheduler's table says so, and the job's thread goes on for a moment after that; close stopped the loops that hand jobs out and did not wait for it.

## Decision

1. **The desk reaches the store through the kernel's hold.** The desk holds the store and the approval surface through a shared `DeskReach`, of which the kernel keeps one clone and which `Kernel::close` empties. A desk kept after the close answers every call `busy`/`engine_degraded`, as a person's act already does once the kernel is gone.
2. **Close waits for every job in hand to end.** The runner keeps the handle of each job's thread; `Kernel::close` awaits them all after it has stopped the loops and before it closes anything else. Each job ends of itself: its steps keep their time limits and its lease its length.
3. **What holds it.** `a_write_rests_until_a_person_approves_at_the_desk_and_is_written_exactly_once` closes the kernel with the desk still kept by its surface and checks that the desk answers `engine_degraded` and that, on Linux, no file of the store is open in the process. The copied data folder test checks the same before it removes the folder.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Clear the desk in the tests' surface at close | The fault is the kernel's: close must release the store whatever a surface keeps, and a surface of the product that outlives an engine would meet it again |
| A weak reference from the desk to the store | The store is not held behind an `Arc` the kernel owns; a weak one would need the ledger's store to change its sharing for one caller |
| Drop the future of a job in hand at close, as the runner loop's stop says | A job dropped between its waits leaves its run to a reconcile at the next start; waiting lets a write already sent be read back and proven, which is safe before fast |
| Wait with a bound, then go on | The bound would be a guess, and a close that goes on leaves the store open, the fault itself |

## Consequences

- An engine that closes while a job runs closes once that job has ended. Nothing in one process holds a job longer than its lease.
- What a surface keeps after the close holds no file of the data folder open. The process's log writer still holds the last engine's log file, which Windows lets be removed.
- Not measured on Windows here: the fix is proven on Linux by the files the process has open, which is the condition Windows refuses on.
