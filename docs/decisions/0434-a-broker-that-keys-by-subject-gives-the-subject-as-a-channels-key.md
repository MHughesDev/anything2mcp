# 0434. A broker that keys by subject gives the subject as a channel's key

**Status:** Accepted. Design decision, made by the worker on field test 7's findings under the lead's brief. The owner had not read this record when it was written. It adds one form to the `key` of a channel file; no public interface changes.

**Date:** 2026-11-25

**Changes:** `a2m-pack-stream` (`src/channel.rs`, `src/wire.rs`, `src/nats_wire.rs`, `src/example_wire.rs`, `src/driver.rs`, `src/discover.rs`, `src/draft.rs`); the design of `a2m-pack-stream`; [spec section 309](../spec/3-packs/309-the-stream-pack.md).

## Context

In field test 7 (finding 44) a stream that keeps one message a subject was held on the question "Which field of a message is its key?". The broker keeps the key in the last token of the subject, which the stream's subjects state (`stock.levels.*`), and no field of the message is the key to the broker. Its list, which needs no key, was held with it, because it names the held channel ([decision 0298](0298-a-draft-with-an-open-question-waits-in-a-holding-folder.md)). A stream of the latest value per key, the common case of state on this kind of broker, got no draft that could be tried until a person answered a question the broker does answer.

A channel file could say only `key: {field: ...}`. Over `nats` the pack already takes the key from the subject: a read asks for `subject.<key>`, a publish sends to it, and a message's key is the token below the channel's subject. The field named only what a binding calls the key.

## Decision

1. **A channel file may take its key from the subject:** `key: {from: subject}`, with `type` as before. The key is then the last token of the subject a message is published on, below the channel's own; a binding names its key as it will. `key` holds exactly one of `field` and `from`, and a retained channel still needs one (`stream.channel_invalid`).
2. **A broker says whether it keys by subject.** The channel facts gain `key_from_subject`. Over `nats` it is true for a stream whose subjects lie within the channel's and include one below its subject (`subject.*` or `subject.>`). The test broker keys by a field of the body and says false. The channel finding carries it.
3. **The drafter takes the subject as the key where the broker keys by subject,** for every shape, and asks the key only of a retained channel whose broker names neither a field nor the subject: over `nats`, a retained stream whose only subject is the channel's own. The channel file and the list over it are then drafted in place.
4. **A file that takes its key from the subject, over a broker that names a field, is drift.** The run's shape check stops as `shape_changed`, "the broker takes the key from a field of the message, not from the subject".

## Alternatives

| Alternative | Why not |
| --- | --- |
| Draft `key: {field: key}` for a broker that keys by subject | It says a field of the message is the key, which is false, and the next person to read the file would look for one |
| Answer the key only for a retained channel | A log and a queue over `nats` are keyed by subject in the same way, and saying so costs nothing; a channel that states a key is read no differently |
| Treat a stream whose subjects are `subject.>` as unknown | Its key may hold more than one token, which a read of one key cannot ask for and refuses unsent; the broker still keys by subject, and a list reads every message |

## Proof

`a_retained_stream_keyed_by_subject_is_drafted_with_the_subject_as_its_key`, in `crates/a2m-pack-stream/tests/nats.rs`, against the stand's test server, failed before the change: the retained stream is drafted in place with `key: {from: subject}` and its list, with no question; a retained stream whose only subject is its own is still held, with its one question; the drafted list returns as many messages as the server holds; and the example target's channel file rewritten to `key: {from: subject}` compiles and reads a key as the server holds it. `the_drafted_reads_over_nats_return_what_the_server_holds` now drafts the three lists in place and holds nothing.

## What would reopen it

A broker whose key is a part of the subject other than its last token, or a second standard protocol that keys in another way.
