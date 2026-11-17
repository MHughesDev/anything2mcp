# 0064. Five further packs are designed before they are built

**Status:** Accepted. Design decision. The modes named here are proposed: each becomes real, by its own record, when its pack clears the bar of spec section 55. The owner had not read this record when it was merged.

**Date:** 2026-11-17

## Context

The owner asked for the whole system to be designed ahead of the code, and for breadth: software, services, data, and devices. The catalog of surface kinds listed what supporting each kind would take and said an entry was a plan to design, not a design. Five kinds cover nearly everything the first four packs do not.

## Decision

Five packs are designed as spec sections 68 to 72, each answering every item of the entry bar.

| Pack | Proposed mode | Archetype | The choice that defines it |
| --- | --- | --- | --- |
| Command | `cli` | Command | Never a shell. Every argument is its own element of the argument list, typed and bounded. In a write, any stop after the program started is `possible_commit` |
| Data | `sql` | Request | A read is proved a read by the database, through a read-only transaction, never by parsing. A write the pack composes always carries the full key |
| File | `file` | Artifact | A write is a new file that replaces the old one in one step. A format is writable only if a save with one change alters only that place |
| Stream | `stream` | Stream | A wait is bounded and an empty wait is an answer. The runtime never consumes what others expect, and discovery never consumes at all |
| Device | `device` | Device | Every writable point carries a hazard and a range. A person approves every run. Nothing moves a device to a safe state automatically |

Choices that cut across them:

- **A surface says where it stands.** A database or a file is alone, or it is beneath an application that owns it. Beneath an application, reads are free and a write needs the owner's word and is a hazard.
- **Another machine is reached through a runner on that machine.** A remote login hands the remote shell a single line, which is the thing the command pack exists to avoid.
- **A console on a serial line or a raw socket belongs to the device pack.** A console reached by starting a program belongs to the command pack.
- **What "discovery cannot write" means is stated per archetype.** A program is started only in the help and version forms its target declares. A statement runs in a read-only transaction. A queue is never consumed. A device is only asked to identify and list itself.
- **Each pack brings its own test stand,** with a control interface that reports ground truth: a command-line tool, a local database, files in a folder, a small broker, a simulated device. No real hardware runs in CI.
- **Firmware** stays as decision 0054 fixed it. The device pack adds that an update cannot be published without a declared recovery route.

## Consequences

- Five test stands and five sets of pack crates join the roadmap, reads before writes in each.
- Each design names what it expects the kernel to need: registered surface kinds, a place for values known before the commit point, checked paths, a precondition that identifies a device before acting. These are in the crate designs now. Whether they were enough is hypothesis H-26.
- The embedded database the data pack would use as its test stand is in the public domain, which is not on the closed licence list. That is decided on purpose when the dependency is added.

## Alternatives considered

- Design each pack only when it is about to be built: less to keep true, and the kernel's contracts would be drawn knowing one kind of system.
- Design many more packs: mobile, terminal, bridge, directory, mail. They follow where there is demand; five was the number the catalog showed would cover the rest.

## Revisit if

The command pack, built first, shows that the designs of the other four rest on something the kernel cannot do.

## Spec sections

- [68-the-command-pack.md](../spec/68-the-command-pack.md)
- [69-the-data-pack.md](../spec/69-the-data-pack.md)
- [70-the-file-pack.md](../spec/70-the-file-pack.md)
- [71-the-stream-pack.md](../spec/71-the-stream-pack.md)
- [72-the-device-pack.md](../spec/72-the-device-pack.md)
