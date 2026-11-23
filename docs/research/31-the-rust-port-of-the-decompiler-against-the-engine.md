# Research: the Rust port of the decompiler against the engine

**Status:** completed on 2026-11-23, by package X7. The spike is in [`spikes/decompiler-port/`](../../spikes/decompiler-port/README.md) and is thrown away; this note and [H-48](../hypotheses/H-48-result.md) keep the result. It answers the last point of [decision 0276](../decisions/0276-what-the-scout-may-learn-from-a-program-itself-and-by-which-means.md) (point 5) and the open question at the end of [note 30](30-what-can-tell-what-a-compiled-program-does.md).

## The question

Decision 0276 chose the engine whose licence is permissive and which runs without a window (Ghidra, headless) as the optional decompiling provider, and noted that a pure Rust port of that engine's decompiler exists as a library under a permissive licence, written by an agent in one day. The decision's own condition for revisiting: the spike "shows the Rust port equal to the engine on real programs". Is it?

## What was compared

| | Version | Licence | How it was run |
| --- | --- | --- | --- |
| The engine | Ghidra 12.1.4 (its 12.2 needs JDK 25; the machine has JDK 21) | Apache-2.0 | `analyzeHeadless` with its default auto-analysis, then a post-script that decompiles each function and writes the C text |
| The port | `ghidra-decompiler` 2.1.0, written against Ghidra 12.2's decompiler source | Apache-2.0 (its 15 dependencies are MIT, Apache-2.0, Zlib, Unlicense, or 0BSD, alone or as one choice of an "or"; none is outside `deny.toml`'s list) | A small program on its library: `Program::open_with`, then `decompile` for each function. Run twice: with the port's own extra symbols (`Full`) and with only the symbols of the C++ loader (`Loader`), the mode in which the port says its corpus output equals the C++ decompiler's |

The test programs, all x86-64 Linux ELF, are the project's own: the command-line test stand `example-cli` and the MCP test stand `example-mcp` (debug builds; the functions compared are those of the program's own crate, 37 and 61, not the standard library's), and a 45-line C control (`control.c`, built at `-O1` and `-O0`) of the kind the port's own corpus holds. The programs were copied out of the build folder before the run; their digests are in the spike's README. The spike lives outside the product workspace, with its own `Cargo.lock`, so nothing it pulled is in the product's `Cargo.lock`.

A function is matched by its address once the engine's image base (0x100000) is taken off. Four tiers, each looser than the last: exact text; equal once runs of blanks are collapsed; equal once the engine's addresses and `[hash]` marks are put in the port's form; equal once type and variable spellings are put in one form (`int4` and `int`, `xunknown8` and `undefined8`, `iStack_10` and `local_10`).

## What was found

Port in `Full` mode. "Listed" is functions known to each side; "compared" is functions both decompiled, so for the Rust programs, the program's own.

| Program | Listed, engine | Listed, port | Compared | Exact | Blanks | Addresses | Spelling | Different |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| C control, `-O1` | 18 | 11 | 11 | 0 | 1 | 2 | 1 | 7 |
| C control, `-O0` | 24 | 16 | 16 | 0 | 1 | 2 | 3 | 10 |
| `example-cli` | 5,017 | 4,892 | 37 | 0 | 0 | 0 | 0 | 37 |
| `example-mcp` | 6,014 | 5,883 | 61 | 0 | 0 | 0 | 0 | 61 |
| All | | | 125 | 0 | 2 | 4 | 4 | 115 |

In `Loader` mode the same: 0 exact, and 10 of 22 equal after the tiers on the control, none on either Rust program (full tables in the spike's `results/`). Per function, the "different" ones share, after the spelling tier, a median of 0.62 to 0.70 of their lines with the engine's text (lowest 0.13, highest 0.90).

Where the texts differ, from reading the differences:

- **The engine adds a layer the port does not have.** The engine's text starts with a comment naming the function and prints a calling convention (`__rustcall`); it gives imported functions their prototype (`int printf(char *__format,...)` where the port prints `void printf(void)`); it names the slots an imported function is called through (`PTR_printf_00003fd0`) where the port prints `printf@got`; it infers `int` where the port keeps `undefined4`. These come from Ghidra's Java analysis, which runs before the decompiler and which the port, being the decompiler core only, does not have. Textual equality with the engine was never available from the port, whatever its fidelity to the C++ core.
- **Control flow differs in some functions.** `__do_global_dtors_aux` returns early in the port and at the end in the engine. In `example_mcp::stdio::deliver`, which holds a jump table, the engine did not recover the table ("Could not emulate address calculation", 18 lines) and the port did (64 lines). Which is right was not checked.
- **Discovery.** The port lists 97.5% of the engine's functions on `example-cli` (4,892 of 5,017) and 97.8% on `example-mcp`, and nothing the engine lacks. What the engine lists beyond it is its `<EXTERNAL>` entries and a few `FUN_` bodies its analysis found.
- **Speed.** The port opened `example-cli` and decompiled 37 functions in 0.4 s. The engine took 3 min 20 s end to end, of which its decompiling was half a second: the rest is the virtual machine and the analysis.
- **Faults.** Neither the port nor the engine returned an error or panicked on any function compared. Over a wider set run first (every function whose name held the crate's name, 3,246 and 3,830 of them, generic instances included, not compared) the port reported 16 and 31 errors; they were not looked into.

## What it means

On this test, the port is not equal to the engine, and could not be: its output and the engine's differ in every Rust function and in 115 of 125 overall, on spellings and on a layer of analysis the port lacks, and in a few functions on control flow. The bet in decision 0276, point 5, that the port might stand in for the engine's output, is refuted as worded. This is not a finding that the port is wrong: whether it equals the C++ decompiler it was ported from (its own claim, on its own corpus) was not tested here, because that needs the C++ console built from Ghidra's source.

What changes: nothing in the design. The decision's condition for revisiting was not met, nothing in the product depends on the port, and package X6 proceeds with the engine as written. Two things stay possible and neither is planned: the port as a second, lighter provider with its own text and its own named facts (it needs no virtual machine and no gigabyte), and a later test of it against the C++ console.

## What was not done

- The C++ decompiler console, the port's real reference, was not built or run.
- Ghidra 12.2 was not used (JDK 25 needed; 12.1.4 on JDK 21). The port follows 12.2's source.
- Only x86-64 ELF, debug Rust builds and unstripped C; no release or stripped programs, no other processor, no Windows or macOS program (the port says PE import tables and Mach-O stubs are not named).
- Which of two differing texts is the more correct was not judged, apart from reading.
- Version 2.1.0 (2026-09-25) was the newest on the registry on the day; the crate then had three versions.
