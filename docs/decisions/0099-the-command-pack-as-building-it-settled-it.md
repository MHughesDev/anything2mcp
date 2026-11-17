# 0099. The command pack as building it settled it, and what the kernel could not give it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the open point of [spec section 306](../spec/3-packs/306-the-command-pack.md) on the step payloads, and the choices its text left open or stated in a form the kernel does not take. **Measures:** hypothesis H-26, for package D4 of the [roadmap](../plan/roadmap.md).

## The outside crate

`regex` 1.13.1, MIT or Apache-2.0, with the features `std` and `unicode-perl` only, which brings `regex-automata` and `regex-syntax` and nothing else. It matches a program's version line, its prompts, its output, and its text arguments. It was chosen because its matching takes time linear in the text: output from a program is untrusted, and a pattern must not be able to run away on it. A pattern that must cover a whole text is wrapped by the pack, so an alternative inside it cannot slip past its anchors.

*Instead of:* a matcher of our own, which is a large piece of code to get wrong where it guards arguments; or a backtracking matcher, which a crafted line can stall.

`rustix`, already in the workspace, gives the engine's own user identifier on Linux and macOS. JSON output is read by a small reader of the pack's own, which keeps every number as its text, so that no value passes through a float.

## Choices

| Point | Choice | Instead of |
| --- | --- | --- |
| Where a file names its surface | `surface:` at the top of a `commands/` file, where the kernel reads it | Inside `program:`, which the kernel does not look at |
| An environment's program | The address as text: `surfaces: {tool: '/opt/example-test/bin/example-tool'}` | A map with `path`, which `target.yaml` does not take |
| The JSON output form | Fields declared in the file, each `text`, `integer`, `decimal`, or `boolean`, required unless it says otherwise. A field that is not declared is drift. One document that is a list, and one document a line, are rows | A schema file beside the commands: a file kind owns YAML documents in one folder and nothing else |
| A text param's pattern | `pattern:` on the param | On the field type: `types.yaml` has no pattern |
| The binding | `command` is a name, since a command's name is unique in a target. `read_back` and `lookup` take `command`, `args`, and `key`, the params that take the parts of the record's key | A file and a name; a read-back that can only read operation inputs, which cannot read a record whose key the program assigned |
| Steps of a write | `act` is the one `spawn`, marked `commits`; `confirm` is `expect` and `parse` | All three in `act`: the commit point is the start of the program, and nothing else |
| A write that is not confirmed | `expect` and `parse` raise `possible_commit`/`no_confirmation` themselves, because the handle turns a doubtful failure into one only on the committing step | Returning the read's class from a write's `expect` |
| A question during a write | `possible_commit`, even when the exit status says nothing changed: the program did not behave as its file says | Believing the exit status |
| `absent` from a write | `stale`/`record_absent`, a new code: the record the preview saw is gone | `rejected`, which blames the values |
| A command's help argument | `{command}` in a help form stands for the words of a listed command, one element each, and each word is letters, digits, and `_.:-` | One element with a space in it |
| The finding | One `command` finding for each listed command, at `discovered`, holding the usage line as the program printed it. What the line's parts mean is read by the drafting rule | A second, `inferred` finding for the arguments, which a drafting rule cannot join to the first |
| The draft | One file for each command, each with the program block. Equal program blocks compile to one program | One file of several documents |
| Limits | `timeout`, `stdout`, `stderr`. `grace` and `concurrency` are not taken; one start at a time holds a surface | Taking keys that nothing carries out |

## What the kernel could not give the pack (H-26)

**One kernel change was forced.** The plan now declares a surface's programs, the compiler fills them from the fields a surface kind marks, and the engine hands them to the supervisor ([decision 0124](0124-the-plan-declares-a-surfaces-programs-and-a-schema-checks-run-time-values.md)). Section 306 needs it where it says the pack starts "only programs declared in a published plan", and it is item 2 of that section's list of likely changes.

**Of that list's seven items:** 1 (surface kinds) and 3 (steps) needed nothing, because the kernel already takes both from a pack as data; 2 (the supervisor) needed the change above; 4 (leases) needed nothing: `prepare` takes no lease for a surface with no sign-in; 5 (read-back) needed nothing: read-back is steps, and a lookup step may report absent; 6 (the sandbox) needed nothing: the driver is told the posture and refuses a command itself; 7 (target identity) was not exercised, since the identity check is the engine's.

**What the kernel's public types could not express, with no kernel edit made.** Each is left out or done by the pack, as stated.

| # | Section 306 says | The kernel has | The pack |
| --- | --- | --- | --- |
| 1 | `digests`: known builds, by platform | One optional digest for a declared program | Keeps the lists in its own section and checks the file itself |
| 2 | "byte ceilings for each stream" | One bound for both streams of a one-shot run | Passes the larger, and compares each stream's count afterwards |
| 3 | "A stream that passes its ceiling ends the process" | Output beyond the bound is read and dropped until the program ends | Names the failure when the program has ended |
| 4 | "waits for exit up to the grace limit, then ends it by force" | The group is ended at once at the time limit | Does not take `grace` |
| 5 | `limits.concurrency`: "how many may run at once on this surface" | One holder of a place, or a count for a whole kind | One start at a time for a surface |
| 6 | A secret on standard input, on an inherited handle, or in a variable | A credential sink with a header, a cookie, a certificate, and a challenge; a one-shot run with no inherited handle; an environment that "never" holds a secret | Secrets are not built |
| 7 | `{session.state_dir}`, a folder the engine owns for the session | A profile's name on the lease, and no checked path for it | Not built; the placeholder does not compile |
| 8 | Refuse a program that "can be written by another user", or that is a script | The supervisor checks that the file is there and its digest; a checked path cannot name a file outside a root | Reads the file's owner, mode, and first bytes with the standard library, on the path the plan declares |
| 9 | "a field type with an anchored pattern" | Field types with no pattern | `pattern` on the param |
| 10 | "The stop, its cause, and the cleanup are three facts in the trace" | A step's result holds values, a key, and captures; a driver has no place to report how a step was done | Not reported |
| 11 | `output: {json: schemas/item.json}` | A file kind owns YAML documents in one folder | Fields in the file |
| 12 | A fresh folder "made for each run" | A checked path can remove a file and not a folder | Removes the files, then the empty folder with the standard library |
| 13 | The version "once per lease" | Nothing a driver can keep for a lease | Asks in every `prepare` |

Rows 2 to 5 are the supervisor's one-shot link; rows 6 and 7 are the session keeper's; rows 8 and 12 are file access; row 10 is the driver contract. None was worked around with something unsound: where the pack reads a file itself (rows 8 and 12), the path comes from the published plan or from the pack's own checked root, never from an input.

## Consequences

- Section 306 is corrected where its examples differed from the above.
- By section 306's own rule ("if more than items 1 and 3 need a kernel edit, the boundary is redrawn before a third pack is designed"), item 2 counts. The change was small and general: a surface kind marks which of its fields name a program. Whether that calls for redrawing is the lead's to decide in D4.
- The supervisor's one-shot link is the place a second command-like pack would meet the same gaps first.
