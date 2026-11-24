# 2026-11-24: a gate before every push

The entry before this is [many lanes at once](2026-11-24-many-lanes-at-once.md). This one covers the morning of 2026-11-24, when the owner asked that CI become faster, cheaper, and green when code is ready, and that CI not rest on GitHub's runners alone.

## State at the end of this entry

- **Merged:** pull request 21, six batches of the night's work, every check green on Windows, macOS, and Linux.
- **Open:** pull request 22 (packages 9.1, 8.4a, the faster read path). Its first run failed one macOS test, fixed since; the next batch adds E4, 3.6's automated half, and 4.4.
- **Progress:** 119 of 150 packages, 79% (`python3 tools/progress.py`). Package 3.6 waits on the owner (the macOS and Windows prompts run once by a person), as 0.2p, 9.5, and 9.6 do.

## What the owner decided

- **Make CI faster, cheaper, and green when code is ready, and do not rest on GitHub's runners alone.** Written as [decision 0372](../decisions/0372-a-push-waits-for-the-checks-of-cis-linux-job-run-here-and-ci-keeps-the-compiled-dependencies.md): a push waits for `tools/gate.py`, which runs CI's Linux checks here or split over sibling cloud sessions, and lints the whole workspace for Windows and macOS from Linux; CI keeps the compiled dependencies. No check is removed, narrowed, or retried. Inferred: the bottleneck was red rounds, not the length of one round, so the gate is the larger part of the answer.
- **Use other cloud sessions for the gate; not the owner's own computers for now.** The owner approved the sessions and declined, for now, sharing a computer for macOS and Windows tests.

## Done in this stretch

9.1 (a changed screen is caught by a canary, its writes stop, and a repaired version writes once; decisions 0380, 0381, 0384), 8.4a (decision 0385), E4 (decision 0387), 4.4 (the `api` mode; decision 0389), 3.6's automated half (decision 0388), and fixes: the grid row read again after a redraw (0379), `engine.stop` answers before the engine stops (0382), the exit test measured in pairs (0383), a read's run without a disk sync of its own (0386), the stand's TLS client, and the gate itself (sharded runs build the whole workspace, because cargo unifies features of only the packages a command names).

## Learned

- **Every red round of the night failed on Linux for a reason a whole-workspace run here would have shown.** The gate caught three such faults before a push in its first hours: a migration test that removed one migration and not the one after it, a feature combination that built TLS with no crypto provider, and four product tests whose counts an earlier package had changed.
- **Cargo's caching cut the Windows build from fourteen minutes to seven** and Linux's from five to two; the test steps (about eighteen minutes) are now most of a round.
- **The Phase 4 exit test's margin was thin** (9.0 on a CI runner against a bar of ten). The bar stayed; the read got faster: most of a read's time was eight synced commits that spec 208 had never asked for.
- **Disk is the hard limit here.** The gate's build folder and one worker's do not fit together; when a worker builds, the gate runs in cloud sessions.
- **A worker's report is a claim.** Twice this morning a report said a rule held where it did not (the durability rule and an administrator's action; a test's floor statistic that made the bar easier). Reading the change in full found both.

## Open, and where it is

- The owner's steps: 3.6's prompts on macOS and Windows; 0.2p, 9.5, 9.6, R1, R2, 5.5, 5.6.
- E2 waits on a choice: where traffic capture joins the provider contract.
- Rows held although their dependencies are done: 6.1, 6.2, 6.5, 8.3; the phase gate for 6.x is to be read against the Build Order before they start.
- N0, the rename sweep, now also carries renaming the profile that names a real framework.
- The Claude GitHub App is not installed on the repository, so pull request events do not reach the lead's session until the owner installs it.
