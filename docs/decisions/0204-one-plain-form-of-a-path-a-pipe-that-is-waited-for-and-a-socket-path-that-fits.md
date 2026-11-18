# 0204. One plain form of a path, a pipe that is waited for, and a socket path that fits

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the provider contract's path interface ([design of `a2m-provider`](../architecture/crates/a2m-provider.md)), the local channel ([design of `a2m-local`](../architecture/crates/a2m-local.md)), and the product's reading of trust roots ([decision 0166](0166-https-in-the-product-trust-roots-at-engine-start-and-one-maker-of-clients.md)).

## Context

The first run of the test suite on Windows and macOS after a day of new work found six faults. Linux had passed every test. Each fault is a place where one operating system differs and the code or a test had only met Linux.

1. On Windows, a file's resolved path comes back in the system's long form, `\\?\C:\...`. A root holds its folder in the plain form, `C:\...`. The product read a trust root's path in the long form and looked for it under the plain form, so every trust root was refused as outside the allowed folders. Three tests built their expected value the same way. The rule for a path's plain form was also written twice, in the provider and in the action registry, and the two copies differed.
2. A Windows named pipe has no queue. While an engine was starting, or had stalled, its one pipe instance took one client and every other client found the pipe busy. The client tried forty times, about one second, and then failed with words the system calls "uncategorized error". On a Unix socket the second client waits in the system's own queue.
3. On Windows, the test of the runtime folder's access list compared the list as text, and Windows writes the built-in administrator's identity as an alias, not as its full form.
4. On macOS, a socket's path may be 103 bytes long. The first-hour test's folder made a path a few bytes longer, and the product reported "invalid input parameter".
5. On macOS, the system's verifier refuses an unknown authority without saying which check failed. One test asserted the code Linux gives.
6. The check of the npm package set gave npm a relative path from another folder, and npm read it as the short name of a repository.

## Decision

- **One function gives a path's resolved, plain form: `resolved_path`, in the `a2m-provider` crate.** It is public, and it is the provider's only resolver. Roots, the product's trust roots, the action registry's project folder, and every test that compares a path with what a root holds go through it. The stricter of the two old rules is the one kept: a path stays in its long form when the plain form would name something else, such as a device. The action registry's copy is gone.
- **A client waits for a busy pipe until its caller's patience ends**, using the system's own wait for a pipe instance, never a count of tries. A pipe that exists and stays busy to the deadline is an engine that does not answer: `engine_unreachable`, with the engine's output file named, as for a stalled hello. A pipe that is not there is no engine. The server always has an instance pending: when a connection attempt on an instance fails, a new instance replaces it before anything else.
- **A refusal by the operating system is told by its number and the system's fixed words for that number**, built from the number alone so that no path can come with it. The word "uncategorized" is never shown.
- **A socket path longer than the system takes is a named stop before anything is made**: `input`/`socket_path_too_long`, with the path, its length, the limit, and the way out. The limit is taken from the system's own check, not from a number written in the code. It is checked where an engine would bind and where a client would connect or start one.
- **Identities and access lists are compared as what they are, never as text.** The test now reads the access list's entries and compares the trustee with the user by the system's own equality. The product's own checks were read: none compares an access list or an alias as text.
- The trust test accepts either refusal code on Apple systems, as its two sibling tests already did, with the same comment.
- The npm check resolves its folder to an absolute path first.

## Consequences

- `resolved_path` is an addition to a public contract. A pack that compares a path with a root's folder must use it.
- The device-name rule is now one function and refuses a few more names than the provider's old copy did.
- A person whose data directory is deep enough to push the socket path over the limit is told so. The only way out today is a shorter `A2M_HOME`. A setting that moves the runtime folder alone would be kinder and is not built.
- The product tests' temporary folders have a shorter name, so that the engine's socket fits on macOS with room to spare.
- None of the Windows code in this decision has run on Windows yet. It type-checks and lints for Windows from Linux. The next run of CI is its first test.

## Alternatives weighed

- **Keep two copies of the plain-form rule and fix the product's comparison alone.** It would have passed the tests. It leaves the same fault waiting wherever the next comparison is written. Not taken.
- **More tries for a busy pipe.** Any fixed count is wrong for some machine. The caller already has a patience; the wait follows it.
- **Give the Windows pipe several instances from the start**, so that several clients can connect while the engine opens. It hides the wait for a few clients and not for the next one, and each instance is a connection the engine must be ready to refuse before it has opened its store. Not taken; the single pending instance and the client's bounded wait are simpler to reason about.
- **Skip the first-hour test on macOS.** Never: the test showed a real limit a person can meet.
