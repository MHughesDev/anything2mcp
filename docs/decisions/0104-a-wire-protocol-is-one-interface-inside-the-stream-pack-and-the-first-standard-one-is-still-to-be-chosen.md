# 0104. A wire protocol is one interface inside the stream pack, and the first standard one is still to be chosen

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles, in part:** the open point "Which protocol first" of [spec section 309](../spec/3-packs/309-the-stream-pack.md). It settles where a protocol lives and what is asked of one. It does not choose one.

## Context

Section 309 names no protocol. Its test stand, `targets/example-broker`, speaks a line protocol of its own, because no one small open protocol offers a channel of every shape the section lists with a look that takes nothing. The pack had to be built and proven before a protocol was chosen, and must not be rebuilt when one is.

## Decision

**One interface, inside the pack.** `a2m-pack-stream` holds two private traits, `Protocol` (a name, and `connect`) and `Link` (one open connection). The driver and the discoverer speak only these. A surface of kind `broker` states `protocol`, and the driver picks the implementation by that name. A protocol is never a mode, a surface kind, or a crate of its own: it adds no verb and no file kind.

**The first implementation is the stand's own protocol,** `example-broker/1`, as a small client the pack wrote on Tokio. The pack does not depend on the stand's crate outside its tests, so what is proven is what would run.

**No broker client crate is added.** The pack's outside crates are `tokio` and `serde`.

**What the interface requires of any protocol that fills it:**

| Requirement | Why |
| --- | --- |
| Every call ends by a deadline, and a fetch returns at most a stated number of messages after at most a stated wait | Every read ends within its limit |
| A look that takes nothing: the channels the broker lists, what it reports of one channel, the retained value for a key, and a bounded browse | `read`, `list`, read-back, reconcile, and discovery never consume |
| A subscription the broker discards with the connection, and attaching to a subscription a person declared | A wait reads through the runtime's own subscription and creates none |
| A position the broker reports for each message of a log, and a start position for a cursor and a browse | A wait from a position; a scan for a record's token |
| Acknowledge and release of one delivery of a queue | `take`, and giving back what was not taken |
| A publish that carries headers unchanged and is confirmed with the message's identifier and, on a log, its position | The record's token reaches the broker; the confirmation is evidence |
| A refusal of a repeated token that names the message first stored | A resumed publish delivers one message and still learns which one |
| For every failure, whether the request was not sent, was answered, or has an unknown fate | A lost confirmation is `possible_commit`, never a retry |
| A cheap test, from memory, of whether a connection is still open | An idle connection is not reused after the broker dropped it |

A protocol that cannot offer one of these does not fill the interface in part. The channel shapes it cannot serve are refused at compile time for surfaces that name it; how a protocol states which shapes it serves is to be decided when the second protocol arrives.

## Alternatives weighed

- **Pull in a client for one standard protocol now.** Refused for tonight: no single small protocol covers retained values, a log with positions, a queue with acknowledgement, and a browse, so the pack would be proven in part, and the crate and its licence tree would have to be verified first.
- **A protocol as a mode or a pack of its own.** Refused: the verbs, the file kind, the read-back rules, and the failure table are the same for every broker. Only the bytes differ.
- **Depend on the stand's client.** Refused: a test stand is never on the path of a run.

## Consequences

- The first standard protocol is a research brief and a slice of its own: which one, its client crate verified against `deny.toml` on three operating systems, and the same test cases run against the stand through a second way in.
- Until then the pack runs only against a broker that speaks `example-broker/1`, which is the test stand.
- The surface kind's `protocol` field is a closed list of words, so a target that names a protocol this build does not speak does not compile.
