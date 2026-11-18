# 0136. The MCP pack refuses free text, reads its list again before a write, and passes on only listed members

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the open point on `free_text` in the crate's [design](../architecture/crates/a2m-pack-mcp.md), and three places where the code of `a2m-pack-mcp` was weaker on safety than [spec section 304](../spec/3-packs/304-the-mcp-pack.md). **Replaces:** the row of [decision 0109](0109-the-mcp-pack-as-building-it-settled-it.md) that leaves a tool mapped `free_text: true` callable, and the sentence of [decision 0108](0108-an-upstream-mcp-server-is-reached-only-through-a-pinned-mapping-by-a-client-of-the-packs-own.md) that reads the list again only on the server's notice.

## Context

A reading of the pack's code against its documents found six defects. Three were plain: a connection was reused without asking whether it still reached what the plan names, a structured result with no pinned schema was taken unchecked, and no test ran discovery. They are fixed and need no choice. Three needed one.

- Section 304 refuses a tool that takes one free-text string and runs it. The code let a mapping say `free_text: true` and then called the tool, with a warning.
- Decision 0108 reads the tool list again on the server's notice. Over HTTP the pack opens no stream, so no notice is ever read, and with no handshake a server promises none. A tool that changed after `prepare` was called against its old pin for the rest of the run.
- Section 304 passes text from the server to a caller only as untrusted data. The compiler checked only the outputs of a read that the mapping lists under `untrusted_fields`, and the driver passed on every member of a structured result.

## Decision

**A tool that runs free text is never called.** `free_text` is what a reviewer found the server does with the text a tool is given.

| The entry says | Meaning | Result |
| --- | --- | --- |
| `free_text: true` | The server runs the text, as a command, a query, or a prompt | The driver refuses every call, unsent, as `policy`/`not_granted`, whatever the tool's input looks like and whatever compiled the plan. A binding that uses the tool, as its act, its read-back, or its lookup, does not compile. The entry alone is a warning: it may stand as the record of a review |
| nothing | No one has said | A tool whose whole input is one string is refused by the driver, unsent |
| `free_text: false` | The server does not run the tool's one string: it is a key, or plain data such as a text to look for | The tool may be called |

A text field that is one argument among several, such as the body of a note, is an ordinary typed input. It needs no flag.

`free_text: false` is a person's statement, as `effect` is a person's judgment. The pack cannot see what a server does with a string, so it cannot catch a statement that is false. What it guarantees is that no tool is called which the mapping says runs free text, and no one-string tool is called which no one has reviewed. The drafter no longer writes `free_text: true` for a one-string tool; it leaves an open question, so neither answer is guessed.

**A write is compared with a list read for it.** Before every call that commits, on every connection, the tool list is read again and the tool is compared with its pin. A read is compared with the list held, which is read again on the server's notice where the connection gives notice, and when it is older than the setting `list_max_age_ms` where it gives none. A connection gives notice when it is a local program that said in its handshake that it does. Every HTTP connection, and every connection with no handshake, gives none.

The cost: one more listing, as many requests as the list has pages, for every write. For reads, at most one more listing each `list_max_age_ms`, 10 seconds unless set, and a read may run against a pin up to that old. Setting it to 0 reads the list before every read. A listing that fails before a call leaves the call unsent, so a failed listing never makes a write `possible_commit`.

**Only listed members of a result are passed on, and every text output is marked.**

- At run time, a structured result gives the members the mapping lists: the ones the entry names under `key`, `records`, or `untrusted_fields`, and the ones the pinned output schema declares as something that cannot hold free text, which is a number, a truth value, nothing, or a value from a closed list. Any other member is left out of what the step returns. A tool whose result is text gives its one value, `text`, and nothing else.
- What the entry names must be there. The member under `records` is a list of records, and each member under `key` is text or a whole number, or the step stops as `drift`/`response_schema_mismatch`. A tool with no output schema therefore gives only what its entry names, checked.
- At compile time, every output of an operation bound in the mode that is or holds text must have a field type marked untrusted. The rule looks at the operation's outputs and not at one tool's `untrusted_fields`, so it holds for what a read returns, what a write returns, and what a read-back or a lookup fills. A choice is not text: the kernel admits only its codes.

**Stop or leave out.** Section 304 states a stop for a result that does not fit what is pinned, and that stays: a member the pinned schema forbids is `drift`. A member that fits the pin and that nothing names is not a misfit. The section's rule for it is "only what the mapping names is reachable", and for a member of a result that means it is not passed on. The capture of the whole result still holds it, as evidence.

## Alternatives weighed

*Making `free_text: true` an error wherever it is written.* Simpler to state. It lost because a mapping then cannot record that a tool was reviewed and refused, so discovery keeps reporting the tool as unmapped and the next person maps it afresh. A tool that is recorded and unbound is as unreachable as one that is absent.

*Refusing every tool whose whole input is one string.* No statement by a person could then be false. It lost because a read by key is exactly such a tool, and the pack would reach almost nothing.

*Reading the list again before every call, reads too.* The strictest rule, and one with no clock in it. It lost on cost: a run that reads many records would double its requests. A read against a stale pin is still checked against the pinned schemas and writes nothing the mapping judged a read; the setting bounds how stale.

*Trusting a notice wherever the link could carry one.* It lost because a server that never said it gives notice owes none.

*Stopping on any member the mapping does not list.* It reads like rule 7. It lost because a result with one member more is not a response shape the pack acts on: nothing is derived from it. A stop after a write would turn a good write into `possible_commit` over a member no one reads, and a server whose own schema allows extra members could never be pinned out of it.

*Keeping the compile check on `untrusted_fields` and widening it to writes and read-backs.* It lost because a record taken from a list, and a key, reach a caller under names the list does not hold. Checking the operation's outputs has no such gap.

## Consequences

- A target that binds a tool mapped `free_text: true` no longer compiles, and a plan that holds one no longer runs it.
- An output of type `identifier`, `email`, or any other text needs a field type with `untrusted: true` when its operation is bound in the mode `mcp`. The kernel's own rule asks this only of the base `text`.
- A member that can hold text reaches a step's values only when a person listed it under `untrusted_fields`, `key`, or `records`.
- A typed member of a tool with no output schema cannot be taken unless it is a key. A declared shape for such a result is not built.
- The stand gained one fault, a structured result of the test's choosing, to play a tool with no output schema that returns structured content.

## What would change it

- A link that carries the server's notices over HTTP, read by this pack: reads over HTTP would then wait for a notice, as a local program's do.
- Measured cost of the extra listing on a real server with a long tool list. The rule for writes would stay; how the list is fetched could change.
- A way for the compiler to see a mapped tool's schema. The one-string check would then move to compile time.
