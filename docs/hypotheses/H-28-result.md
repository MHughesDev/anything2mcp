# H-28: settled in part

**Status:** partly. The bet stays open.

**Tested:** nothing was run for this file. It was written on 2026-11-24, in the sweep of the register, from the tests and records it names ([decision 0393](../decisions/0393-a-bet-is-settled-only-by-a-named-test-or-a-measured-number-and-a-part-settled-is-marked-partly.md)).

## What was assumed

One binary with every first-party pack compiled in stays under the 25 MB wheel target.

## What was found

The stripped program is 24,042,992 bytes and the wheel 11,410,659 on Linux x86-64, both under 25 MB (decisions 0363 and 0369).

## What is still unknown

The other seven targets; the margin is under 1 MB.

## What changes

Nothing in the design. The part still unknown is not to be built on as if it held.
