# H-06: settled in part

**Status:** partly. The bet stays open.

**Tested:** nothing was run for this file. It was written on 2026-11-24, in the sweep of the register, from the tests and records it names ([decision 0393](../decisions/0393-a-bet-is-settled-only-by-a-named-test-or-a-measured-number-and-a-part-settled-is-marked-partly.md)).

## What was assumed

One engine per user, started on demand and found through a socket or pipe, is reliable on three operating systems.

## What was found

Two commands share one engine, a second engine is refused, and a stop leaves nothing (`two_commands_share_one_engine_a_second_engine_is_refused_and_a_stop_leaves_nothing`, green on three systems).

## What is still unknown

The start race, the crash, and the stale-file tests on each system; an answer to `engine.stop` was once lost on macOS (decision 0382).

## What changes

Nothing in the design. The part still unknown is not to be built on as if it held.
