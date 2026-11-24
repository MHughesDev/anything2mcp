# H-16: settled in part

**Status:** partly. The bet stays open.

**Tested:** nothing was run for this file. It was written on 2026-11-24, in the sweep of the register, from the tests and records it names ([decision 0393](../decisions/0393-a-bet-is-settled-only-by-a-named-test-or-a-measured-number-and-a-part-settled-is-marked-partly.md)).

## What was assumed

The trace signature is identical across runs and operating systems.

## What was found

Five runs give one signature on each system, plain and signed in (`first_read.rs`, `sign_in_signature.rs`).

## What is still unknown

A test that compares one system's signature with another's.

## What changes

Nothing in the design. The part still unknown is not to be built on as if it held.
