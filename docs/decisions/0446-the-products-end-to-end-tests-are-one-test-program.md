# 0446. The product's end-to-end tests are one test program

**Status:** Accepted. Design decision, made by the agent that merged the product crate's tests. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Changes:** how `crates/anything2mcp/tests/` is built; the names of its tests; the filters of CI's PostgreSQL, NATS, and window steps that run some of them. No test is added to, taken from, or made less strict, but one: a guard that every file is built.

## Context

The product crate had 53 test files, each a test program of its own, and each program linked the whole product: every pack, every front door, and the test stands. Linking was most of the time of `cargo test -p anything2mcp --no-run`, and the 53 executables most of the build folder.

## Decision

1. **One test program, `product`.** `tests/main.rs` declares the shared helpers, `tests/common/`, once, and each other file in `tests/` as a module of the same name. The crate's `Cargo.toml` sets `autotests = false` and names the one `[[test]]`, `product`, at `tests/main.rs`. The files stay where they were, so every document that names one still names the right file.
2. **A test is named by its module.** `faults::a_test` where it was `a_test` in the program `faults`. One file's tests run with `--test product -- faults::`. The four tests that run another test of their own program in a child process (`capture_use`, `expired_session`, `signed_in_web_read`, and `trust_roots`) name it by its new full name with `--exact`.
3. **A filter that matches nothing is not a pass.** The PostgreSQL step and the window step, which ran `--test data_write` and `--test signed_in_web_read --test capture_use`, now run `--test product` with the module filters and check the count of passed tests in the result line, as the NATS step already did; the NATS step's filter is `stream_write::`.
4. **A file left out fails a test.** With cargo finding no test program of its own, a new file not declared in `main.rs` would build nothing and say nothing. `every_test_file_is_a_module_of_this_program` reads the folder and fails on such a file.

No test needed a process of its own: none sets an environment variable or a working folder of the process, and the two statics of a file (`stream_write`'s lock and connection count) are private to their module. The tests that need a display or a separate engine already start a child process of their own, and still do.

## Measured

On the same 4-core Linux machine, from an empty build folder, the workspace's dev profile:

| | Before (53 programs) | After (1 program) |
| --- | --- | --- |
| `cargo test -p anything2mcp --no-run` | 3 m 43 s (user 4 m 48 s) | 2 m 37 s (user 3 m 35 s) |
| Build folder after it | 7.8 GB | 1.5 GB |
| `cargo test -p anything2mcp --no-fail-fast`, built | 7 m 58 s | 3 m 36 s |
| Tests listed by `-- --list` | 191, 3 of them ignored | 192, 3 of them ignored |
| Result | 188 passed, 3 ignored | 189 passed, 3 ignored |

The one test more is the guard of point 4. The run is shorter mostly because the tests of all files now share one pool of threads, where cargo ran the 53 programs one after another. Another build ran on the same machine at the time, so the times are good to about a quarter; the folder sizes are exact.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Move the files to `tests/product/` with a `main.rs` there | Cargo would find the program without `autotests = false`, but about 140 references in the documents name `crates/anything2mcp/tests/<file>.rs`, and decision records are not rewritten |
| Keep the window tests as programs of their own | They already run their display work in a child process; a program of their own only saved a filter |
| A few programs, by kind of test | Each still links the whole product; the saving is in linking once |

## Consequences

- `tools/gate.py` weighs a test program by the `#[test]`s of its source file, so it weighs `product` by `main.rs` alone, as one test. Every program still runs in exactly one shard; the shards are less even. Weighing a `main.rs` program by its folder, as the tool already weighs a library, would mend it.
- Tests of different files run at the same time now. A test that needs the machine to itself takes a lock of its own, as `stream_write` does.

## Revisit if

- Two tests of different files are seen to interfere when run at the same time.
- A test is found to need process-wide state; it then gets a program of its own and a line here.
