# 0150. A take records its message before it acknowledges, and an answer with no rule leaves a write in doubt

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the findings of a safety audit of the stream pack's code against [spec section 309](../spec/3-packs/309-the-stream-pack.md) and the ten rules: three serious, five minor, and two the work itself turned up. **Changes** sentences of section 309. **Supersedes** in [decision 0105](0105-the-stream-mode-and-the-stream-pack-as-building-it-settled-it.md): "what a message is as values", for the headers and for the name `message_meta`. The crate is `a2m-pack-stream`; its [design](../architecture/crates/a2m-pack-stream.md) names the test of each line below.

## Context

A reviewer read the crate and ran nothing. Each finding was first shown by a test against [the test broker](../../targets/example-broker/README.md) that failed, and then fixed. All eight were confirmed; none was refuted. Rule two decided each doubt: safe, then correct, then fast, then easy.

## Decision

**S1. A take gives its whole message out before it acknowledges.** The driver gave the engine only the message's identifier before the acknowledging step. An acknowledgement that landed and whose answer was lost left the message in no ledger row. Now the step that waits returns the redacted message, with its identifier, as values for reconcile, which the engine stores durably before it sends the committing step. The stored form is bounded by the channel's `limits.message_bytes`; a larger message is given back to the queue and the run stops with `policy`/`take_too_large`, unacknowledged. A take in doubt is not taken again: the engine stops a record in doubt that has no identity, and the record holds the message.

**S2. A scan says "absent" only of a log it covered.** A look that said more is held, with no position to go on from, was answered as "absent". It is now `unknown_variant`/`unexpected_reply`, as is a look that says that was all while the log's own end lies further on. The wire client no longer reads a look that does not say whether more is held as "no more".

**S3. A write is refused only in words the protocol defines for it.** A refusal of a publish or an acknowledgement under a code the protocol does not have was reported as answered. It is now `possible_commit`/`no_confirmation`, in the wire client (the fate of such a request is unknown) and again in the driver, which accepts as a refusal of a write only the kinds that belong to one.

**Minor findings.**

- **S4.** `message_meta` is always the broker's. A body with a field of that name is `drift`/`response_schema_mismatch` when read; the driver never sends one, and the compiler refuses a binding that gives one.
- **S5.** A fetch that hands over more than the one message asked for is `unexpected_reply`. The connection is given up, which gives the messages back.
- **S6.** A wait that saw nothing says `coverage: complete` only where its subscription stands at a position the broker reported. Elsewhere it says `partial`, with `limitation: starts_at_subscription`.
- **S7.** Of a message's headers a result holds only the one the channel declares for correlation.
- **S8.** A connection is not kept after any failure of its run. A session whose provider places anything but one token stops the run with `input`/`credential_not_carried`, where it was dropped silently.

**Found while fixing.**

- A broker's words were cut to a length before the redactor saw them, so a secret that lay across the cut left its first part in a failure's sentence. The wire client now passes the words on whole, and the driver redacts, then cleans and cuts.
- Nothing made text from a message reach a caller as untrusted text; the kernel's rule only warns. The pack adds the rule `stream.untrusted_unmarked`, an error: an output that is or holds text is of a field type marked `untrusted`, unless the channel's schema holds the field of that name to an `enum`.

**Checked and found sound.** The session's token is registered with the vault's redactor by `SessionLease::apply` before the driver's sink is given it.

## Alternatives weighed

*A digest of the message for reconcile, and the body as a capture.* Smaller in the ledger. The engine stores a step's values for reconcile before the commit point and gives no such promise for a capture, so the content would again exist only after the step that may lose it.

*No bound on the recorded message.* Simpler. A message of any size would then be written into the ledger's evidence by whoever can publish to the queue. The channel's own limit is the bound a target already states.

*Acknowledging a message too large to record, and returning it.* It keeps the queue moving. It is the defect itself: an acknowledgement without a stored message.

*A step that settles a take in doubt by a look at the queue.* The record holds the identifier, and a look that covers the queue would say whether the broker still holds the message. It was not built, because a step of a resumed run is given nothing of what was recorded (gap 2 of decision 0105). The pack does not keep the identifier in memory instead: memory does not outlast the process that a resume replaces.

*`scan_limit` for a look that does not say how far it reached.* The reviewer named both codes. `scan_limit` says the log is longer than may be read, which a person answers by shortening retention; here the broker's answer has no rule, which is `unexpected_reply`.

*Overwriting a body's `message_meta` with the broker's facts.* The list would still be returned. It would also drop a field of the message without saying so. One message of that shape now stops a look at its channel, and the failure names the field.

*A channel file key that lists more headers for a caller.* Not needed by any test, and each header shown is text from another producer. It can be added when a target needs one.

*Keeping the connection after an answered failure.* It saves a connection. The design already said it is dropped, and a connection that has just refused the session's credentials is of no use to the session's next run.

*The untrusted rule without the `enum` exemption,* as the MCP pack has it. A status the schema holds to three texts is not free text, and the driver refuses any other; the rule would push targets to mark closed lists as untrusted and so blur the mark.

## Consequences

- Two failure codes are added: `take_too_large` (policy) and `credential_not_carried` (input). One lint rule is added: `stream.untrusted_unmarked`.
- A target whose operation returns text from a message under the plain `text` type no longer compiles, unless the schema lists the texts. The example target of the pack's tests gained a `types.yaml`.
- A caller no longer sees headers other producers set. A result of a wait that saw nothing has one more field, `limitation`.
- A run that failed once opens a new connection for the session's next run.
- The test broker gained five faults and one case; its design is its README.

## What other crates would have to change

- **The provider contract or the lowering context:** a look-up or read-back step of a resumed run should be given the values recorded before the commit point. A take in doubt could then be settled by a look at the queue for the recorded identifier, and a scan could start at a recorded position.
- **The engine:** a record in doubt with no identity stops as `cannot_reconcile`; what was recorded before its commit point should be shown with that stop. The engine also refuses any binding with no read-back, so a take, and a publish to a queue or a topic, cannot run through it yet.

## What would change it

- Either change above. The pack would gain the step that settles a take, and the bound on a recorded message could become the engine's.
- A second wire protocol that carries a user and password or a client identity: `credential_not_carried` would then depend on the protocol.
