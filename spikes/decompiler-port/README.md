# Spike: the Rust port of the decompiler against the engine

Roadmap package X7, with [decision 0276](../../docs/decisions/0276-what-the-scout-may-learn-from-a-program-itself-and-by-which-means.md) point 5. Throwaway code. Nothing here is promoted into the product; if a provider over the port is ever built, it is written fresh.

**The question.** Is the pure Rust port of Ghidra's decompiler (`ghidra-decompiler` 2.1.0) equal to the engine's own output (Ghidra headless) on the project's own test programs?

**The answer** is in [research note 31](../../docs/research/31-the-rust-port-of-the-decompiler-against-the-engine.md) and [H-48](../../docs/hypotheses/H-48-result.md): no. Of 125 functions compared, none is text-identical; 10 are equal once blanks, addresses, and type spellings are put in one form; 115 differ, mostly in a layer of analysis the engine's Java side adds before decompiling.

## What is here

| Path | What it is |
| --- | --- |
| `Cargo.toml`, `src/main.rs` | The port driver: lists every function, and decompiles those a filter selects (`^prefix`, `substring`, or `@file` of addresses). The empty `[workspace]` table keeps the crate, and so the port's dependencies, out of the product's `Cargo.lock` |
| `GhidraDump.java` | The same dump from the engine, as a post-script of `analyzeHeadless` |
| `control.c` | The control program: 45 lines of plain C with imaginary `example` names |
| `compare.py` | Matches functions by address and sorts each pair into exact, blanks, addresses, spelling, or different |
| `results/<program>-<mode>.txt` | The comparison output for each program and each symbol mode, with one `ROW` line per function |

## How it was run

Ghidra 12.1.4 (Apache-2.0) was downloaded to a scratch folder and run on JDK 21; it is not in the repository. The port was built in its own target folder (`cargo build --release` here) and both were removed afterwards.

```
sha256  4a5f14c6...  control      (gcc -O1; the -O0 build is the same source)
sha256  51306a7c...  example-cli  (debug, x86-64 ELF, copied from the workspace build folder)
sha256  80adf67b...  example-mcp  (the same)

dump <program> full <filter> out/<p>.port-full.txt        # the port, its own extra symbols
dump <program> loader @<addresses> out/<p>.port-loader.txt # the port, the C++ loader's symbols only
analyzeHeadless <proj> p -import <program> -scriptPath . -postScript GhidraDump.java out/<p>.ghidra.txt <filter> -deleteProject
compare.py out/<p>.ghidra.txt out/<p>.port-full.txt --show 6 --rows
```

For the two Rust programs the filter was `^example_cli::` and `^example_mcp::` on the port, and then the list of addresses the port had decompiled, for the engine and for the port's `Loader` mode, so each side decompiled the same functions. For the control, every function.
