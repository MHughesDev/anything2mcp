# 0430. A file whose bytes are not on this machine is refused by name, and never fetched

**Status:** Accepted. Design decision, made in the design review round by lens 13, the other operating systems ([report](../reviews/other-operating-systems-review-2026-11-24.md), finding 3). The owner had not read this record when it was written. It adds to the provider contract, an addition: one kind of entry that `CheckedPath::entry_kind` and the opening of a file report. Built by roadmap package OS3.

**Date:** 2026-11-24

**Changes:** `a2m-provider` (`src/path.rs` and its system modules); `a2m-pack-file`, which maps the new kind to a failure of its own; [section 308](../spec/3-packs/308-the-file-pack.md); the technical reference's provider contract, once OS3 is built.

## Context

The file pack, and every pack that reads a file through the provider contract, takes a regular file to be bytes on this machine's disk. On Linux it is. On Windows and macOS a file in a folder that a sync program keeps can stand on disk with its bytes elsewhere:

- On Windows, a cloud file placeholder: a reparse point whose tag is not a name surrogate, with the attribute that its data is recalled on access, or the offline attribute. A Windows 11 machine, as set up, puts the person's desktop and documents folders in such a sync program. Rust's standard library takes a link to be a reparse point whose tag is a name surrogate, so it reports a placeholder as a regular file.
- On macOS, a dataless file, marked by the `SF_DATALESS` flag, which a file provider makes for a cloud-backed folder, the desktop and documents folders among them when the person turns that on.

Opening such a file for reading makes the system fetch its bytes over the network, through the sync program, as the person's account. The fetch can take as long as the network does, can fail with a code of the sync program's own, and is a request the engine did not declare and the person did not confirm. Replacing such a file, as the file pack's save does by one rename, hands the sync program a new file to send, and a second copy beside it when the copy elsewhere changed meanwhile. None of this is in sections 308 or 203. The office documents a person most wants an agent to work on are, on these two systems, often in exactly those folders.

## Decision

1. **The provider contract reports a file whose bytes are not on this machine as a kind of entry of its own,** never as a regular file: on Windows by the attributes of recall on access, recall on open, and offline; on macOS by `SF_DATALESS`. Linux has no such mark and reports none.
2. **Nothing reads such a file.** A read, a write, or a folder's read of its content stops before any byte is read, with a failure that names the file and says to make it kept on this machine in the sync program's own settings, which is the person's act. A folder's list gives such an entry its name, kind, size, and time, which the system gives without a fetch, and no digest or content.
3. **A file that is on this machine in a synced folder is read and written as any other.** What the sync program does with the new file after a save is the system's, as any other program's save would be; section 308 says so. Its second copy beside a changed file is not the engine's to resolve.
4. **The failure is declared with its class and code when OS3 builds it,** in the provider contract's table and the file pack's, by the procedure of decision 0258.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Let the system fetch the file, under the step's time limit | A request nobody declared, through another program, that a time limit cuts off with the fetch's fate unknown; section 203's reach would need a sync program as a destination |
| Ask the sync program to keep the file on this machine | Changes another program's state on the person's behalf, which is the person's act |
| Refuse every file in a synced folder | Refuses the files that are on this machine, which read and write like any other |
| Leave it to the operating system's error | The error comes after the fetch has begun, and names nothing a person can act on |

## Proof

Roadmap package OS3:

- A total function from each system's attributes and flags to the entry's kind, with unit tests that run on every system.
- On Windows CI: a file of the file pack's stand given the offline attribute, which a program of the user may set, is refused by a read and by a write before anything is read, and the stand's file is unchanged; a folder's list names it with no digest.
- On macOS a dataless file cannot be made without a file provider; the mapping's unit test is the proof there, and a person's check on a machine with such a folder is owed and written down.

## What would reopen it

A sync program on either system that marks its files in another way.
