# H-32: settled in part

**Status:** partly. The bet stays open.

**Tested:** nothing was run for this file. It was written on 2026-11-24, in the sweep of the register, from the tests and records it names ([decision 0393](../decisions/0393-a-bet-is-settled-only-by-a-named-test-or-a-measured-number-and-a-part-settled-is-marked-partly.md)).

## What was assumed

A program can be started with exactly the same arguments on Windows, macOS, and Linux, element for element, using each system's standard rule for passing them.

## What was found

On Linux each awkward value reaches a script as literal text (`the_shell_and_python_receive_each_value_as_literal_text`).

## What is still unknown

Windows and macOS, and a program that parses its own command line.

## What changes

Nothing in the design. The part still unknown is not to be built on as if it held.
