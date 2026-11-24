# H-04: settled in part

**Status:** partly. The bet stays open.

**Tested:** nothing was run for this file. It was written on 2026-11-24, in the sweep of the register, from the tests and records it names ([decision 0393](../decisions/0393-a-bet-is-settled-only-by-a-named-test-or-a-measured-number-and-a-part-settled-is-marked-partly.md)).

## What was assumed

Several runs can share one signed-in browser in separate tabs without interfering.

## What was found

Ten runs that meet an expired session cause one sign-in, counted by the application, and two commands at once read through one browser (`renewal.rs` and `local_engine.rs` under `crates/anything2mcp/tests/`, green on three systems).

## What is still unknown

A write beside the reads on one session, and a look for cross-talk.

## What changes

Nothing in the design. The part still unknown is not to be built on as if it held.
