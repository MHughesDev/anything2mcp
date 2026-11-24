# 2026-11-24: many lanes at once

The entry before this is [the design settled for the next lead](2026-11-23-the-design-settled-for-the-next-lead.md). This one covers the night of 2026-11-23 to 2026-11-24, when the lead ran up to twenty workers at once, one per group of crates, and brought each finished package in by hand.

## State at the end of this entry

- **On the branch of pull request 21:** a batch of 46 commits pushed at 23:12. Its Rust jobs failed on three systems: a document cell that is empty now stops a read (from decision 0315), and the command pack's binary test on macOS hits its byte bound. Both are being fixed.
- **On the lead's staging branch, not pushed:** about 40 more commits, held until those fixes land, so that a push does not cancel a running check.
- **Progress:** 81 of 149 packages, 54% (`python3 tools/progress.py`). The total grew as packages were found: 4.6, N0, FT7b.

## What the owner decided

- **Every name says what the part is, in plain words** ([decision 0330](../decisions/0330-every-name-says-what-the-part-is-in-plain-words.md)). "Plan" becomes "compiled target" ([decision 0329](../decisions/0329-the-compiled-file-the-engine-runs-is-the-compiled-target.md)), and about twenty-five other names change. The owner left the choice of each name to the lead. Package N0 carries the renames, in one sweep at a quiet point.
- **The document of what to build next is the Build Order** ([decision 0326](../decisions/0326-plan-names-only-the-compiled-plan-and-the-build-document-is-the-build-order.md)), `docs/plan/build-order.md`.
- **Use the remaining time to build as much as possible, near to complete.** Inferred: run every lane the crates allow, keep one writer per crate, and leave to the owner only the packages that need them (0.2p, 9.5, 9.6, R1, R2, 5.5, 5.6).

## Done in this stretch

S2, FT7, FT7b, FT8, FT10, GD1, V1 to V4, V3 (decision 0312), 2.1, 2.2, 4.5 (about twelve times faster), N2b, F3, K4, X1 to X3, X5, X7 (refuted), P2, P3b, FT4 (the first verified write of a field test, through the harness, against a copy of real software, with its answer cut once), 1.4, 1.5, 3.4, 3.5, 6.3, N3, and eight research notes (32 to 39) folded into decisions 0318 to 0325.

## Learned

- **One shared build folder races.** Workers' copies overwrite each other's compiled crates and the product binary; a full local test run is not trustworthy while others build. CI is the gate. Workers touch a crate's `lib.rs` to force a rebuild.
- **Every push cancels the running checks.** Finished packages wait on a staging branch and go up in batches about 45 minutes apart.
- **Parallel workers pick the same decision number.** Workers now write 0399 and the lead renumbers on merge (a script does it).
- **The Rust job outgrew its time limit.** It now has sixty minutes; the Phase 4 test runs in a job of its own.
- **Disk fills from build output** several times a night; stale build folders are cleared by age.

## Found, and routed

- The product's random kill test (3.7) found a run left `runner_lost` after a kill between "verified" and "completed". It never wrote twice. The test waits outside the repository until the defect is fixed.
- The window sign-in did not work through the product: the local surface answered "is the person there?" with "unreachable". 2.3 stays open until it does.
- Field test 2's findings 12 to 15: a one-character secret masked everywhere (fixed: a chosen secret has at least eight bytes, decision 0338); help text of `secret set`; a drafted write cannot be tried in the sandbox (needs a decision).

## Next

The lanes running when this was written: the empty-cell fix, the actions halves of V5, FT11 and 4.6, the window sign-in and the macOS test, the web lane (N3 wiring, 1.3, decision 0322), X6, and FT5 on a database and files. Then the kill defect, 2.4 to 2.7, E2, FT9, 3.3, 3.7, 3.10, 9.1, the runner and server lanes, and N0.
