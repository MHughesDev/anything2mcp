# H-40: settled in part

**Status:** partly. The bet stays open.

**Tested:** nothing was run for this file. It was written on 2026-11-24, in the sweep of the register, from the tests and records it names ([decision 0393](../decisions/0393-a-bet-is-settled-only-by-a-named-test-or-a-measured-number-and-a-part-settled-is-marked-partly.md)).

## What was assumed

One stateless MCP service can serve both protocol eras, the revision with a handshake and the revision without one, to any client that follows the standard.

## What was found

Two client libraries, one of them in two release lines, connected and called a read in both revisions, seven runs by hand (decision 0131).

## What is still unknown

The runs in CI, and any other client.

## What changes

Nothing in the design. The part still unknown is not to be built on as if it held.
