# 0090. The supervisor ends a group only while its leader is unreaped, and its links are started by three calls

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles, in part:** the open point of the `a2m-provider` design on proving ownership at stopping. **Carries out:** what [decision 0070](0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md) asks of the supervisor's foreign link.

## Decision

**Linux and macOS.** Every child leads a new process group. The supervisor signals the group only while the leader has not been reaped: until then the leader's identifier, which is the group's, cannot be given to another process, so the signal reaches nothing the supervisor did not start. So the leader's exit is observed without reaping it (`waitid` with `WNOWAIT`, on a blocking thread), the group is ended, and only then is the leader reaped. This needs no reading of another process's environment and is the same code on both systems. *Instead of:* reaping first and signalling the group afterwards, which is the common way and can, in principle, reach a recycled identifier; *and instead of* checking each process's environment for the owner mark, which macOS does not allow without more privilege. The owner mark is still placed in the child's environment, for the ownership records of a later step.

**Windows.** Every child is placed in a job object of its own, set to end its processes when its last handle closes. Membership is the proof; the tree also ends if the engine dies. Every start takes one process-wide lock, because a foreign link marks two handles inheritable for the moment of its start.

**When the group counts as gone.** The signal was delivered, or the system says there is no such group. macOS gives a third answer: signalling a group in which nothing is left alive but its exited leader is "not permitted" there, where Linux reports success, because macOS does not count an exited process and reports that it found nothing it may signal. With the leader exited, that answer means the group is gone; with the leader alive it means the group cannot be ended, and the cleanup is incomplete. The first build read the answer as a failure, so on macOS every program that exited by itself and left nothing running was reported as an incomplete cleanup, which then refused further starts of its kind.

**One lock around every start, on every operating system.** Windows needs it for the inherit mark. macOS needs it because it cannot make a pipe already marked close-on-exec: the pipe is made, then marked, and a child started by another thread in between inherits it and holds it open. The lock is held from before a child's pipes are made until the child exists. The supervisor is the only place the product starts a process, so this closes the gap for the product. A test stand that starts programs of its own on other threads is outside the lock.

**A run does not wait for the end of its output, and its verdict does not depend on who holds the pipe.** Once the group is gone, everything it wrote is in the pipe. On Linux and macOS the run takes what is there and stops; on Windows it reads to the end of the pipe, for at most five seconds. A process outside the group that still holds the pipe, whether it left the group or inherited the pipe by the gap above, cannot be told apart from here, so it neither delays the run nor makes its cleanup incomplete. *Instead of:* waiting a bounded time for the end of the pipe and calling a late end an incomplete cleanup, which blocked a kind of subsystem on the strength of a guess.

**What it does not catch.** A process that leaves its group (a new session) on Linux or macOS: it is not found and not ended. A member of the group that the engine's user may not signal, on macOS, once the leader has exited. A child started in the instant before it is placed in its job on Windows.

**Three calls, not one.** `run(spec, OneShot)` gives the exit status and bounded output; `start_foreign(spec, ForeignPipe)` gives the two pipe ends; a framed start will give a connection. `ForeignPipe` carries how the child receives its pipe: two descriptor numbers, and for Windows one argument in which `{read}` and `{write}` are replaced by the handle values. *Instead of:* one `start` returning one `Subsystem`, which would hand every caller a connection type that two of the three links do not have.

**A run's environment** is cleared; only the names listed are passed through and only the pairs listed are set. Without a working folder a program starts in the system's temporary folder, never in the engine's.

## Consequences

- Not built: the framed link and its handshake, the queue and the one-start-per-place rule (a taken place is refused, not joined), ownership records in the store, and ending the children of an engine that crashed on Linux and macOS.
- The time limit of a one-shot run is `transient`, `timeout` with `Ran::Unknown`; a pack that ran it for a committing step reports `Sent::Unknown`, and the handle makes it `possible_commit`.
