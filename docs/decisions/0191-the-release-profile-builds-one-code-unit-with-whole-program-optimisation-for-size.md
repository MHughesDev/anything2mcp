# 0191. The release profile builds one code unit, with whole-program optimisation, for size

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** package F4 and hypothesis H-28: the binary and the wheel measured with every first-party pack compiled in, against the 25 MB target. Builds on [decision 0135](0135-the-wheel-carries-the-binary-and-a-thin-client-and-npm-uses-platform-packages.md) (the wheel holds the stripped binary) and [decision 0166](0166-https-in-the-product-trust-roots-at-engine-start-and-one-maker-of-clients.md) (the earlier measures).

**Changes:** the `[profile.release]` table of the root `Cargo.toml`, which had none. No workflow changes: the wheel, the npm packages, and `cargo install` all build with the release profile.

## What the target is a target of

[Spec section 605](../spec/6-running-it/605-packaging-and-release.md) and the register say "a wheel under 25 MB". [The H-28 note of 2026-11-17](../hypotheses/README.md) read the same number against the stripped binary. The documents do not say which; the wheel is what a person downloads, so this record reads the target as the compressed wheel and gives the binary's size too. (A wheel is a zip; the sizes below are the stripped binary alone, deflate level 9. The thin Python client adds a few kilobytes.)

## Measured

Linux x86_64, stable 1.100.0, `cargo build --release -p anything2mcp --locked -j 2`, one build at a time, four cores shared with other agents. Every row is the stripped binary with default features (HTTPS on) unless it says otherwise. Bytes; build time in seconds.

| Build (changes from the default release profile) | Stripped binary | Compressed (wheel) | Build, s |
| --- | ---: | ---: | ---: |
| Baseline: the profile as it stood (opt-level 3, no LTO, 16 units, unwind) | 37,217,880 | 15,168,746 | 438 |
| Same, unstripped (`cargo build` leaves symbols; maturin strips) | 49,538,168 | not measured | |
| `lto = "fat"` | 30,970,872 | 13,904,015 | 464 |
| `lto = "thin"` | 37,740,408 | 15,650,108 | 558 |
| `codegen-units = 1` | 30,003,040 | 13,029,575 | 320 |
| `opt-level = "s"` | 27,629,024 | 10,947,685 | 279 |
| `opt-level = "z"` | 27,450,336 | 10,712,938 | 226 |
| `panic = "abort"` (measure only) | 29,329,504 | 12,161,281 | 338 |
| `--no-default-features` (no HTTPS; number only) | 35,843,800 | 14,508,969 | 389 |
| fat LTO + 1 unit | 28,546,168 | 13,057,519 | 457 |
| **fat LTO + 1 unit + `opt-level = "s"`** | **18,471,032** | **8,602,578** | **284** |
| fat LTO + 1 unit + `opt-level = "z"` | 17,444,472 | 8,098,197 | 251 |
| fat LTO + 1 unit + `panic = "abort"` (measure only) | 23,991,800 | 10,893,059 | 394 |
| fat LTO + 1 unit + `z` + `panic = "abort"` | not measured: another agent's unfinished edit to `a2m-pack-web-probe` broke the build | | |

The rows were built over about an hour while other agents changed the tree, so each is not of exactly the same source; the tree grew about 0.8 MB over that time. The two rows that decide this record were therefore built again, in one go, on the tree as it stood at the end, with the adopted settings written into `Cargo.toml`:

| Same tree | Stripped binary | Compressed | Build, s |
| --- | ---: | ---: | ---: |
| Old profile (opt-level 3, no LTO, 16 units) | 38,066,768 | 15,532,217 | 424 |
| Adopted profile | 18,546,544 | 8,640,611 | 323 |

`strip = true` in the profile gives the same bytes as the `strip` tool within 0.5 percent; it is not adopted (below).

**What the size is made of**, symbols of the baseline summed by the crate named in the symbol (no `cargo bloat` here; `nm --size-sort -C`; a generic is counted to the first workspace or dependency crate in its name, so the figures are indicative, and about 8 MB of the 37 MB is not in named symbols). Sections of the baseline: `.text` 28.7 MB, `.eh_frame` and its index 3.4 MB, `.gcc_except_table` 1.9 MB, `.rodata` 1.8 MB.

| Rank | Crate | Bytes in symbols (baseline) |
| ---: | --- | ---: |
| 1 | `a2m_actions` (the generated faces of every action) | 2,879,509 |
| 2 | `serde_json` (mostly instances for the workspace's own types) | 2,613,935 |
| 3 | `tokio` | 2,026,410 |
| 4 | `a2m_core` | 1,894,459 |
| 5 | `a2m_engine` | 1,265,632 |
| 6 | `a2m_ledger` | 1,101,289 |
| 7 | `a2m_pack_web_browser` | 1,083,104 |
| 8 | `a2m_files` | 888,140 |
| 9 | `zbus` (the secret-service client of the vault) | 865,609 |
| 10 | `alloc` | 779,317 |

After the adopted profile the order is the same, at about half each: `a2m_actions` 1.58 MB, `serde_json` 0.88 MB, `tokio` 0.65 MB, `a2m_pack_web_browser` 0.57 MB.

**Speed.** The product's own benchmark harness does not call the product: its one scenario, `direct_read`, reads the test application, so it cannot see a profile's effect on the engine and is not used for this (it reads 33 to 49 microseconds run to run, noise larger than any effect). The quickest end-to-end check that takes a built binary is the program itself run in one-shot mode (`A2M_ENGINE__IN_PROCESS=true`) in a copy of the `orders` test project: `--version`, `provider list` (engine open and provider registry), and `draft lint example` (settings, files, compile). Median of 100 to 300 runs each, interleaved, in milliseconds; the machine was shared and loaded, so only same-run comparisons count.

| Same tree, same run | `--version` | `provider list` | `draft lint example` |
| --- | ---: | ---: | ---: |
| Old profile | 5.1 | 27.0 | 29.2 |
| Adopted profile | 4.7 | 26.2 | 29.1 |

Earlier runs, one run, binaries of the matrix above, against their baseline (`provider list` / `draft lint`): fat LTO and fat LTO + 1 unit about 5 percent faster; `codegen-units = 1` 3 percent faster; fat + 1 unit + `s` 2.6 percent faster / 2.5 percent faster; `opt-level = "s"` alone 14 percent slower; `"z"` alone 23 percent / 26 percent slower; fat + 1 unit + `z` 8 percent / 10.5 percent slower; `panic = "abort"` within noise.

Between the first and the last tree, `provider list` went from about 17 ms to about 27 ms in quiet runs, with either profile: some other change added about 10 ms to engine start. That is not this record's, and it is noted here so that it is looked at.

## Decision

Adopt, in `[profile.release]`: `lto = "fat"`, `codegen-units = 1`, `opt-level = "s"`. By the rule given for this package:

- (a) It does not use `panic = "abort"`. The setting is measured only: it takes 4.5 MB off the fat-LTO build, and it is not adopted, because the engine's failure handling and the rule on panics at crate boundaries rest on unwinding in places. Changing it is a design decision for the lead.
- (b) No benchmark slowed by more than 10 percent: the adopted profile is within 3 percent of the old one on all three commands, in the same run.
- (c) The build takes 284 to 323 seconds on this machine with `-j 2`, under the old profile's 424 to 438. A single code unit and a size-first optimiser make less work for LLVM than 16 units of full optimisation, and fat LTO costs less than it saves.

`opt-level = "z"` is not adopted: it takes a further 1.0 MB off the binary and 0.5 MB off the wheel, and in the one run made it was 8 to 10.5 percent slower, at the edge of the 10 percent limit, in a noisy run that cannot show that it stays inside it. `opt-level = "s"` or `"z"` alone, without LTO and one unit, is slower and saves less; the three settings work together.

**The 25 MB target.** Met for the compressed wheel: 8.6 MB against 25 MB, and it was met before (15.2 MB), so the wheel was never the problem. Met for the stripped binary: 18.5 MB against 25 MB, where it was 37.2 to 38.1 MB. The H-28 bet, that one binary with every first-party pack stays under the target, holds on this one platform. The other seven targets of the release workflow are unmeasured; on Windows and macOS the sizes will differ (the linker, the system libraries, and the unwind tables are not the same), and the first release run measures them.

## Alternatives weighed

- **A separate `[profile.dist]` used by the two maturin builds only,** with `--profile dist` in `release.yml` and the `bindings` job. It keeps the bench job's release build as it is. Rejected: the sdist, `cargo install`, and anyone building the npm packages by hand would still get the 38 MB build, and the one bench scenario that exists does not depend on the product's profile. If a later bench scenario is a budget of speed over the product itself, and the size profile moves it, split then.
- **`strip = true` in the profile.** The wheel is stripped by maturin already (decision 0135); a stripped `cargo build --release` loses its backtraces. Not adopted. A person who builds with `cargo install` gets 23.1 MB unstripped.
- **`panic = "abort"`, `opt-level = "z"`, `--no-default-features`:** see above. TLS costs 1.4 MB (3.7 percent) of the baseline and is on by default (decision 0157), so it is not a lever.

## Consequences

- The next levers, if the target moves or a platform comes out larger: first, `a2m_actions`, whose generated faces take the most of the product (1.6 MB after the profile) and are generated, so a table-driven face could be smaller; second, `serde_json` instances for the workspace's types (0.9 MB); third, `a2m_pack_web_browser` (0.6 MB) and `zbus` (0.5 MB), whose packs could be split into separate packages if the wheel ever has to be smaller than it is. `panic = "abort"` would be 4.5 MB, and is the lead's. None is done.
- Only the release profile changed; `dev`, `test`, and `bench` profiles are as before, and `cargo test --workspace` is unaffected.
- The Windows and macOS wheel jobs now build with fat LTO and one unit. Their timeouts are 60 minutes; the Linux build here takes under 6.

## Revisit if

A platform's wheel comes out over 25 MB; the release run's timing on the slowest runner approaches its timeout; a benchmark scenario that runs the product itself shows the profile costing more than 10 percent; or the lead decides on `panic = "abort"`.
