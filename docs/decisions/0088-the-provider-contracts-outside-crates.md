# 0088. The provider contract's outside crates: a TOML parser, a descriptor helper, safe process calls, and the Windows calls

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the outside crates the design of `a2m-provider` left "to be verified": the TOML parser and the JSON Schema validator. It adds the crates the supervisor needs.

## Context

`a2m-provider` reads a manifest written in TOML, and its supervisor starts and ends processes on Linux, macOS, and Windows. The workspace forbids `unsafe` except for one precise, commented case, and a kernel crate carries as few outside crates as it can.

## Decision

| Crate | Version | Licence | Features | Used for | Instead of |
| --- | --- | --- | --- | --- | --- |
| `toml` | 1.2.1 | MIT or Apache-2.0 | `std`, `serde`, `parse`; no default features, so no writer | Reading the manifest into the crate's own types. A problem names the line | `toml-span` (no `serde`, so every field would be read by hand), `basic-toml` (no spans, older TOML), a parser of our own (TOML's grammar is large and a manifest is security-relevant) |
| `command-fds` | 0.3.3 | Apache-2.0 | `tokio` | On Linux and macOS, placing the two pipe ends of a foreign link on the child's descriptors between fork and exec. Verified by research note 17 and decision 0070 | Our own code between fork and exec, which is `unsafe` of ours |
| `rustix` | 1.1.6 | Apache-2.0 with the LLVM exception, or Apache-2.0, or MIT | `std`, `process`; no default features | On Linux and macOS: ending a process group, and waiting for a child to exit without reaping it. Safe functions; already in the lock file through the key store's tree | `nix` (its `waitid` is not offered on macOS), `libc` directly (`unsafe` of ours) |
| `windows-sys` | 0.61.2 | MIT or Apache-2.0 | `Win32_Foundation`, `Win32_Security`, `Win32_System_JobObjects`, `Win32_System_Threading` | On Windows: marking a handle inheritable for one start, and the job object that ends a child's whole tree | The `windows` crate (a larger tree for the same five calls) |

`getrandom` and `sha2`, already in the workspace, give the owner mark and a program's digest.

**No JSON Schema validator is added.** The manifest's shape is checked by the types it is read into, which refuse an unknown key and a badly formed name with the line. A pack's schemas are `a2m-files`'s own `JsonSchema` objects. A pack's settings schema is written in the manifest itself ([decision 0089](0089-the-provider-contract-as-building-it-settled-it.md)), so no schema file has to be validated by an outside crate.

**The `unsafe` this brings.** None on Linux and macOS. On Windows, five calls into the operating system in `supervisor/windows.rs`, each in its own block with a comment that says why it is sound: making a job object and taking ownership of its handle, setting its limit, placing a process in it, ending it, and setting a handle's inherit mark. Decision 0070 foresaw the last. The job object needs the others, because the standard library offers no way to end a process tree on Windows. They are compiled and linted for `x86_64-pc-windows-msvc` from Linux; they run first in the Windows job of continuous integration, where the supervisor's tests and two tests of their own exercise them.

## Consequences

- Four lines in the root `Cargo.toml`. The licence check passes with the list unchanged.
- The whole tree of `a2m-provider` cannot be linted for Windows or macOS from Linux, because `a2m-ledger` below it compiles SQLite from C. The supervisor and the path check were linted for both targets on their own.
