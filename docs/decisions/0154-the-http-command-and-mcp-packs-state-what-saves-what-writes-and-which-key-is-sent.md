# 0154. The HTTP, command, and MCP packs state what saves, what writes, and which key is sent

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** how the three request and command packs answer the additions of decisions [0148](0148-the-provider-contract-and-the-scheduler-after-their-safety-audit.md) and [0153](0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md). It adds to the latest records of the packs: [0139](0139-a-write-that-was-sent-is-never-settled-by-an-answer-that-is-not-the-applications-verdict.md), [0136](0136-the-mcp-pack-refuses-free-text-reads-its-list-again-before-a-write-and-passes-on-only-listed-members.md), and [0140](0140-the-command-pack-after-its-safety-audit.md).

## Decision

| Question | Answer in all three packs |
| --- | --- |
| Which step kinds save? | The kind that sends or starts something (`request`, `spawn`, `call_tool`) is `AsDeclared`: the same kind does both, and which it is is a person's `effect` on the endpoint, command, or mapped tool. The lowering marks it `commits` exactly for a write, and each driver refuses the mismatch in both directions where it can. Every other kind is `Never`. No kind always saves, so no manifest lists `commit_steps` |
| The least effect of a binding | `create`, `update`, and `upsert` are `write`. `delete` and `submit` are `irreversible` (the kernel's words: one that deletes or sends). The HTTP pack also makes a write with no `read_back` `irreversible`, because what it wrote could never be found again. A verb that reads, and `call`, claim nothing more than a read; the file's own effect holds a `call` |
| Which file kinds can describe a write? | HTTP `endpoint`: yes unless marked `read` with a safe method; an open question is not known to read, so it counts. Command `command` and MCP `mapping`: yes when any entry's effect is anything but `read`; an effect left as an open question does not count, because the drafter leaves one for every entry it cannot judge and the file does not compile until a person answers |
| The sandbox guard | None of the three drivers has a guard that a posture lifts: each refuses a write from a step not marked `commits` in every posture, and the handle alone lets a committing step through, in the sandbox only for a declared test record. The command driver's own read-only guard is lifted only for the step marked `commits` |

## The MCP pack and the key

A tool's `key_args` are filled from `StepInput::key`. For an operation with no key input that is the record's place in the run ([0092](0092-a-record-that-waits-for-an-earlier-record-is-planned-under-its-place-in-the-run.md)), which no server knows. The lowering refuses a step whose tool has a `key_args` argument that neither the step's own `args` nor a key input fills. A read-back after a write is exempt when the written tool reports the assigned key under `key`, because the engine then gives the read-back that key.

## Alternatives

- **`Always` for `request`, `spawn`, `call_tool`** and a separate kind for reads: it would split every driver and plan for the sake of a flag the file already carries.
- **An open-question effect counts as a write everywhere:** the drafter could then draft no mapping or command file for a system with one tool it cannot judge. The draft states no write; it does not compile.

## Not settled

An open-question effect counts as a write for HTTP and not for the other two. The HTTP drafting rule drafts `read` only for a safe method and `write` for the rest (its test shows it), so the difference costs nothing today; a person may want one rule.
