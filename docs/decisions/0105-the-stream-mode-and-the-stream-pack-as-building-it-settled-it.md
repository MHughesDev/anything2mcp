# 0105. The `stream` mode, and the stream pack as building it settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Registers:** the mode `stream`, of the stream archetype, as [decision 0060](0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md) requires when a pack is built. **Changes:** sentences of [spec section 309](../spec/3-packs/309-the-stream-pack.md), listed below. The crate's design is [`a2m-pack-stream`](../architecture/crates/a2m-pack-stream.md).

## Decision

**The mode.** `stream`, registered by the provider `stream`, with the surface kind `broker`.

**Where section 309 was changed, and why.**

| Section 309 said | Built | Why |
| --- | --- | --- |
| `schema: schemas/order_status.json`, a JSON Schema file | `schema: order_status`, the name of a document under `schemas/`, a second file kind in YAML | The loader reads YAML files of registered kinds only. The schema is the part of JSON Schema the pack checks: an object, `required`, and `properties` with a `type` and an `enum` of texts. A schema that needs more does not compile |
| Three shapes | Four: `topic` is added | The stand has a topic that keeps nothing, and it differs from a queue: it cannot be looked at and nothing is acknowledged |
| `read_through` is one word | One word, or a list of `browse` and `own_subscription`; and `subscription` names the runtime's own | A broker may offer both, and `list` and `await` use different ones. Rule 1 of "Consuming without stealing" says the subscription is named in the channel file, and the example had no field for it. On a log the name may be left out: the run reads through a cursor the broker discards |
| The surface kind takes an `address` list | One `address` | A surface's address is one text in the kernel, and an environment overrides it as one text |
| An operation declares `on_nothing: fail` | The binding does | The kernel owns the keys of an operation; a mode adds keys to a binding only |
| `take` is classed `write` | `take` is declared `irreversible`, with no identity | The compiler requires of a `write` identity inputs and a read-back. A take has neither: its record is the message the broker hands over, and an acknowledged message cannot be read again. The ledger row the section wants is still there: the acknowledging step commits |
| `take` returns nothing when nothing arrives | A `take` that finds nothing stops with `transient`, `nothing_arrived` | The acknowledging step would otherwise be sent with nothing to acknowledge |
| A broker that refuses a repeated token gives identity `stamped` | On a queue or a topic the operation declares no identity; the token still travels and the step is marked idempotent | The compiler requires a read-back of an `irreversible` operation with an identity, and a queue has none |

**What section 309 left open and the code settled.**

- **`await` does not compile on a queue.** A consumer on a queue takes from the others. `take` is the verb for a queue the runtime owns, and `list` for any other.
- **A wait starts where the subscription does.** A subscription never asks for the values a channel retains. On a log, `from` is an expression of the binding, so a pipeline can wait from the position an earlier publish returned. Through a named subscription the broker's own record is the start.
- **A wait fetches one message at a time,** and ends when its limit has passed. The limit is in the step's payload and the step's timeout is longer by the target's step time, so the plan holds both.
- **Two step kinds for discovery only,** `channels` and `describe`, and one for reconcile, `scan`. A step with no channel reference is carried out only in the discovery and sandbox postures, and returns names and types, never values.
- **Message bodies are read by the pack's own JSON reader.** A number is kept as its text: a whole number becomes an integer, any other an exact decimal, and one with an exponent is `drift`. A decimal and a date are sent as text. No float is ever made. The alternative, the general JSON crate, reads a fraction through a double.
- **What a message is as values:** the body's fields by name, and `message_meta` with the identifier, key, position, and headers.
- **Failure codes added to those of the section:** `protocol_unknown` (input), `scan_limit` (policy), `position_out_of_range` (drift), and `a2m-core`'s `payload_unreadable` for a plan compiled by another build of the pack.
- **Connections** are kept idle between the runs of one session, by address, protocol, and session, and tested before reuse. A run takes one for itself; nothing is multiplexed.

## What the kernel could not express (hypothesis H-26)

No kernel crate was changed. Each of these was met by building what could be built soundly inside the pack.

1. **An address list.** `SurfaceKindSpec` gives a surface one address as text. Section 309: "The surface kind `broker` takes an `address` list".
2. **Values recorded for reconcile do not come back.** A step may return `for_reconcile`, and `StepInput` has no field through which a later step, in the resumed run, receives them. Section 309: "Reconcile scans from the position recorded at intent". The scan starts at the start of the log instead and stops with `scan_limit` when the log is longer than it may read.
3. **`prepare` is not told which operation runs.** `PrepareContext` names the plan and the surface, not the binding. Section 309: prepare "checks that the channel exists and its declared shape matches". The check is made on the first step that names the channel.
4. **A write without identity inputs or a read-back.** The compiler refuses a `write` with neither. Section 309, rule 5: `take` "is classed `write`".
5. **An identity with no read-back.** The compiler requires a read-back of an `irreversible` operation whose identity is not `none`. Section 309: a broker that refuses a repeated token gives "Identity `stamped`", on a queue included.
6. **`Lowering::identity` is not shown the pack's files.** Whether a strategy is possible depends on the channel a binding names. The method refuses what no channel can do, and `lower` refuses the rest under the pack's own code.
7. **A file kind cannot see the pack's other file kinds while it compiles.** `KindContext` gives no entries. A channel's schema name is checked by a lint rule and joined at `prepare`.
8. **A step outcome has no place for trace details.** `Observed` carries values and captures; the `means` and `measured` of a trace record cannot be reported by a driver.
9. **A discoverer is given no clock.** A finding needs the time it was observed. The pack takes the clock from telemetry when it registers.

None of these made the pack name itself in the kernel or work around a check. Items 2, 4, and 5 changed what a target file must declare.

## Hypotheses

- **H-26:** nine gaps, above; no kernel change was needed to build the pack.
- **H-29:** the stream archetype held without a special case. Read-back fits retained and logged channels only, as the section predicted; every other publish is `irreversible`.
- A long wait holds one of a session's reads for up to its limit. Whether the scheduler should count waits apart was not measured.
