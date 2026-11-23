# H-48: the Rust port of the decompiler is equal to the engine on the project's own test programs

**Status:** refuted

**Tested:** 2026-11-23, package X7, the spike in `spikes/decompiler-port/` (thrown away after this record; its code stays in the repository as history, never promoted).

## What was assumed

The pure Rust port of the engine's decompiler gives text equal to the engine's on the project's own test programs, so it could stand in for the engine as the decompiling provider (decision 0276, point 5).

## What was done

Ghidra 12.1.4 headless (default auto-analysis, JDK 21) and `ghidra-decompiler` 2.1.0 decompiled the same functions of three x86-64 Linux programs: the command-line and MCP test stands (the program's own functions, 37 and 61) and a small C control built twice (11 and 16 functions). Functions were matched by address; the port was run with its own extra symbols and with only the C++ loader's. The details are in [research note 31](../research/31-the-rust-port-of-the-decompiler-against-the-engine.md).

## What was found

Of 125 functions compared with the port's own symbols: 0 identical, 2 equal after blanks, 4 after addresses, 4 after type and variable spellings, 115 different. The port lists 97.5% (`example-cli`) and 97.8% (`example-mcp`) of the engine's functions and none the engine lacks. The differences are chiefly what the engine's Java analysis adds before decompiling (calling convention, import prototypes, names of import slots, some type inference); in a few functions the control flow differs, including one jump table the port recovered and the engine did not. The port took 0.4 s for 37 functions; the engine 3 min 20 s end to end, half a second of it decompiling.

## What changes

Nothing in the design. Decision 0276's condition for revisiting point 5 (the port shown equal to the engine) was not met; nothing in the product depends on the port; package X6 proceeds with the engine as written.

## What is still unknown

Whether the port equals the C++ decompiler console it was ported from, on these programs (that needs the console built from the engine's source). Release or stripped programs, other processors, and Windows or macOS programs. Which of two differing texts is the more correct. A port as a second, lighter provider with its own text would be a new bet.
