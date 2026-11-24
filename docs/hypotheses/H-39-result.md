# H-39: settled in part

**Status:** partly. The bet stays open.

**Tested:** nothing was run for this file. It was written on 2026-11-24, in the sweep of the register, from the tests and records it names ([decision 0393](../decisions/0393-a-bet-is-settled-only-by-a-named-test-or-a-measured-number-and-a-part-settled-is-marked-partly.md)).

## What was assumed

A link or launcher named `a2m` starts within the budget for a command on every install route.

## What was found

From pip on Linux `a2m` starts in 16 ms against 30 (decision 0363). Not met on npm when first measured, 43 ms (decision 0135), and not measured again.

## What is still unknown

Windows and macOS.

## What changes

Nothing in the design. The part still unknown is not to be built on as if it held.
