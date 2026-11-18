# 0148. The provider contract and the scheduler after their safety audit

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the findings of a safety audit of `a2m-provider` (three serious, six minor) and `a2m-sched` (three minor), and seven requests that the audits of the packs sent to the contract. It adds to decisions [0088](0088-the-provider-contracts-outside-crates.md), [0089](0089-the-provider-contract-as-building-it-settled-it.md), [0090](0090-the-supervisor-ends-a-group-only-while-its-leader-is-unreaped.md), and [0093](0093-the-schedulers-interface-as-building-it-settled-it.md), and carries [decision 0139](0139-a-write-that-was-sent-is-never-settled-by-an-answer-that-is-not-the-applications-verdict.md) into the driver handle. No public signature changed; everything here is an addition or a stricter check.

## Context

A reviewer read the two crates against their designs and the ten rules, and ran nothing. Every finding was checked against the code with a test first. All twelve were confirmed. Reviews of the file, command, and HTTP packs then asked the contract for what a pack could not do safely by itself.

## The provider contract

| Finding | What was decided | The alternative weighed |
| --- | --- | --- |
| P1, serious. `CheckedPath::write` wrote through a link planted at `<name>.a2m-new`, to a file outside the root | The new file has a name of 128 random bits, is made only where nothing stands, and is never opened through a link. On Linux and macOS the folder is reached by opening each folder from the root down without following a link, and the file is made, renamed, and removed relative to that handle. `write` and `remove_file` refuse where a link stands at the final name. `open` and `read` go the same way down | Keeping the predictable name and adding create-new: it closes the planted link and leaves a planted file able to stop every write of that name. Checking the folder by path and then creating by path: the two moments can be parted. Windows keeps that form, because the standard library cannot open a name relative to a folder's handle; an outside crate for it was not added tonight |
| P2, serious. A requirement's known digests were compared when the registry was built and never at a start | The scoped supervisor carries, for each program it may start, every digest declared for it: a process's `digest`, a requirement's `known_digests`, a plan program's digest. At every start the program is opened once and digested through that handle, and a digest that is none of the declared ones is refused, `policy`, `effect_not_declared`. This is one place, for every pack | Checking in each pack: the browser engine does so after the start, by reading the digest the supervisor reports, which is too late to stop the start |
| P4, minor. The program was digested by path and started by path | On Linux the program is started from the handle it was digested through, by the name the process file system gives an open file, with the path it was asked for by as the name it sees. A file that starts with `#!` is handed to an interpreter by name, and macOS cannot start an open file: there the path is looked at once more under the start lock and must lead to the very file that was digested, by device, file number, size, and time of last change. On Windows the file is held open, shared for reading only, until the process exists | Starting a script from a handle that the child inherits: the interpreter then reads the digested bytes, and the script sees a handle's name where it expects its own path, which breaks scripts that look for files beside themselves |
| P3, serious, Windows. The child ran before it was in its job object | The child is created suspended, placed in the job, and then resumed. The standard library does not hand out the first thread's handle, so the thread is found in a snapshot of the system's threads by the child's process identifier, opened for resuming alone, and resumed | The job in the process attribute list, which the stable standard library does not offer; the undocumented call that resumes a whole process |
| P5, minor. The handle trusts the `commits` mark, and a discoverer builds its own steps | A mode may list `commit_steps`, the kinds that always save. The handle refuses one sent without the mark, `policy`, `intent_missing`. The limit is stated in the design: no first-party manifest lists one yet | Refusing by the step's effect, which the kernel does not know |
| P6, minor. A pack's manifest could name a program by a relative path | Refused for a pack, `input`, `path_not_allowed`. The supervisor never matches a declared path that is not absolute on this operating system, and the availability check passes such a path over: another system's form would be looked for in the engine's working folder | None |
| P7, minor. A provider could declare, and return, a code that is the kernel's alone | `codes::KERNEL_ONLY` lists seven codes that state the kernel's bookkeeping. No manifest may declare one, and the handle lets no driver return one, also through a step's `fails_as`. The engine's and the pack's shared names, such as `changed_since_preview`, are not in it | Reserving every code of the crates above the contract, which it cannot see and which packs rightly share |
| P8, minor. Blocking calls on async threads | Resolving, digesting, and starting are one piece of blocking work | None |
| P9, minor. No planted-secret test | Written, with the backstop below | None |

**No digest is remembered between starts.** A digest kept by a file's identity was asked for, to save the time of reading a large program at every start, on the condition that any write changes the identity. That does not hold. Device, file number, size, and time of modification are all restored by whoever may write the file, with one ordinary call for the time. The time a file's record changed cannot be set on Linux and macOS, but it does not move for a write through a shared mapping until the page is written back, some file systems do not keep it, and on Windows every time of a file can be set by whoever may write it. So the cache would trade a full check for a check that a writer of the file can pass. The digest at every start is what the code did before this record, and it stays. Reading a browser of some 290 MB is reported to take over a second; this work did not measure it.

**What the digest does not defend against.** Someone who can write the program's file in place, and not only replace it at its path, can change it between the digest and the start on Linux and macOS, where nothing denies a write to a file that is not yet running. The supervisor refuses a file that changed while it was read. On Windows the open handle denies the write.

**What the next Windows run must show.** The Windows half was compiled and linted for Windows from a copy of the two files, because the crate's own tree builds C. It has not been run. Three unit tests there prove it: a child created suspended does not exit until it is placed in the job and resumed; a child that starts a second process at once has both in its job, and ending the job leaves neither; a held program cannot be written, renamed, or deleted. The existing tests against the command-line stand must pass unchanged.

Requests from the audits of the packs:

| Request | What was decided |
| --- | --- |
| A caller cannot tell a file from a link to one, because `join` resolves | `CheckedPath::entry_kind` reports what stands at the final name and follows nothing. The design states what `check` and `join` promise about links, in a folder on the way and at the final name |
| A write that was sent must never be settled by a class that means unsent | In the handle: a failure of a committing step with `Sent::Answered` and class `policy` or `input` becomes `possible_commit`, `no_confirmation`, and leaves as `Sent::Unknown`. `stale` stays: it is the system's answer that nothing was written. `Sent::NotSent` keeps its class |
| A folder made new, or not at all | `CheckedPath::create_dir` |
| The handle redacts failures and not what a step observed | It now redacts values, values for reconcile, a capture's name, and a capture whose bytes are text. A capture that is not text is not searched |
| A step has no redactor | None was added: the pack's redactor, the run's, and the handle's are one object, the vault's, and the design says so. A field on `StepInput` would have broken every site that builds one |
| A save in steps, a file's identity, and whether a file may be written | Not built. They are listed as open points of the design |
| A mode's `exclusive` is enforced by nobody | See below |

## The scheduler

| Finding | What was decided |
| --- | --- |
| S1. `finish` took the runner's word for whether a record is in doubt | A `Stopped` end also asks the ledger, as `sweep` does |
| S2. A clock that went back made a budget refill twice | The moment a bucket is counted to never goes back, and a wait is counted from it |
| S3. A waiter whose lease had run out could be granted a resource | `grant` checks the lease's time, as `acquire` does |

**What a mode holds alone.** A manifest's `exclusive` reaches the registry, and the engine's `exclusive_needs` reports it for a tool as pairs of a kind and a surface. The scheduler takes a job's keys when it leases the job, frees them when the job ends, and passes them to a reconcile job. The missing piece is between the two: the action registry submits every job with no keys. `ResourceKey::held_alone(kind, workspace, target, environment, surface)` now fixes the key's form, so every submitter names one thing by one key. The submitter's change is owed by `a2m-actions`. A screen belongs to a runner's machine, and no key can name the runner before the lease: that part is not designed.

## Consequences

- A link that stays inside a root, at the name of a file a pack writes or removes, is now refused where it was followed. A pack that wants the file a link leads to names that file.
- A program a pack requires, with known digests, that is updated in place stops starting until its new digest is listed.
- A driver's `policy` or `input` answer on a committing step that was sent now reconciles the record where it released the lock.
- `rustix` is used with its file-system calls and `windows-sys` with its thread snapshot; no crate was added.
