# 0382. The engine stops once its answer to `engine.stop` is handed over, and a lost one is no write in doubt

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** how `serve` carries out `engine.stop` in `anything2mcp` (`local.rs`), and what a command ends on when the answer to its `engine.stop` is lost (`cli/client.rs`). It refines [decision 0131](0131-the-front-doors-joined-serve-the-thin-client-and-the-bridge.md), which joined the doors and gave `engine.stop` to the engine's process; it leaves the dispatcher's decision, the record of what asked ([decision 0266](0266-the-engines-log-says-each-start-and-stop-and-what-asked-and-engine-logs-reads-it-where-the-engine-runs.md)), and every other action's lost answer as they were.

## Context

The test `two_commands_share_one_engine_a_second_engine_is_refused_and_a_stop_leaves_nothing` failed once in CI on macOS: `engine stop` ended with exit code 4 and `possible_commit`/`answer_lost`, the client having seen "the connection closed before any answer". It passed on Linux and Windows in the same run, and here 20 times in 20 in a row, 16 at once, and 12 at once on one processor before any change: the failure was not reproduced, and its exact cause on macOS is not known.

What the code did is known. The engine asked its channel to stop inside the handler of `engine.stop`, before the answer existed; the channel then began its graceful stop, which closes at once every connection with no request in hand, while the answer was still to be made and written. The order of the answer and the stop rested on the HTTP library's treatment of a request in hand at that moment, and on nothing the engine itself held to. A second fault was in the client: a stop whose answer is lost was reported as a write in doubt, `possible_commit`, with `retry_safe: false` and the advice to look up a run. `engine.stop` writes nothing of the person's and starts no run; asking for it twice is safe. Telling a person not to repeat it, and to look for a run that cannot exist, is untrue.

## Decision

1. **The engine asks its channel to stop only once its answer to `engine.stop` is handed over.** The answer's body asks for the stop when the HTTP library lets go of it, which it does once the whole answer is in the connection's write buffer, a buffer it flushes before it closes the connection, or once the connection ended without it. The answer carries `Connection: close`: it is the last on its connection. Until the stop is asked, the channel serves as before; after it, it takes no connection, as before.
2. **A lost answer to `engine.stop` is never `possible_commit`.** The command looks at the channel once more, without starting an engine. When nothing answers there, or the engine closes the connection before its hello, the engine has stopped serving, which is what its answer says: the command succeeds with `{"stopping": true}` and a warning that the answer was lost and how the stop was learned. When an engine still answers, it did not take the stop: the command fails with `busy`/`engine_running`, `retry_safe: true`, and its fix says to run `engine stop` again. Any other failure of that look is the command's failure, as for any connection.
3. **Every other action's lost answer stays `possible_commit`/`answer_lost`.** Only `engine.stop` is known to write nothing.

*Instead of:* asking for the stop after a fixed wait, or retrying the stop in the client until it succeeds: a wait is timing, not order, and a retry hides an engine that refuses. *Instead of:* asking for the stop when the connection that carried the request has closed: a client that keeps its connection would keep the engine up. *Instead of:* making `answer_lost` of `a2m-local` a `transient` code for every request: the channel cannot tell a stop from a tool call, and for a write a lost answer is in doubt.

## Proof

`crates/anything2mcp/tests/local_engine.rs`: `two_commands_share_one_engine_a_second_engine_is_refused_and_a_stop_leaves_nothing` now also holds that the stop's answer is the engine's own, with no warning of a lost one, and says what the engine wrote when it fails; it passed 30 times in 30 in a row after the change. `crates/anything2mcp/src/cli/client.rs`: `a_lost_answer_to_a_stop_is_a_stop_when_the_channel_no_longer_answers` holds both outcomes of the second look, with no engine and with a channel that still serves.

## What would reopen it

A failure of the test on macOS after this change, which would show that the answer is lost after the HTTP library has taken it, a fault of the connection's close on that system rather than of the order here; or an `engine.stop` that comes to write something of the person's.
