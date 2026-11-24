# Measured-system review, 2026-11-24

One design review round, by [lens 10](../plan/design-review-lenses.md), the measured system: now that numbers exist, which sections assumed something the numbers contradict. It is the review round of package 1.7. The numbers are those of [section 225](../spec/2-kernel/225-performance.md), taken on one shared Linux machine in a release build; the round read sections 225, 222, 208, 605, and the hypotheses H-28 and H-39 with them in mind.

## Findings

| # | Finding | Where | Serious | Kind | Sorted to |
| --- | --- | --- | --- | --- | --- |
| 1 | The start-to-ready budget assumed no key derivation. On the passphrase route the start is 340 to 360 ms, and 250 ms of it is the derivation, which is meant to be slow. A machine with no key store takes this route, and so does every container | 225 | Medium | Assumption contradicted | Fixed: the row says both routes; [decision 0363](../decisions/0363-the-budgets-as-first-measured-two-are-revised-and-four-cannot-yet-be-told-from-the-systems-own-time.md) |
| 2 | The `http` budget left out the durable write of the ledger. A call is 10 ms over the request made directly, and about 4 ms of that is the store on disk; 225 says the ledger is group-committed, which helps concurrent runs and not a single caller | 225, 208 | Medium | Assumption contradicted | Fixed: the row says what the 15 ms is measured through; the aim of 5 ms for the loop stays; decision 0363 |
| 3 | "Beyond the system's response time" has no definition for a `ui` step. The page's own request is not the system's time in a browser: drawing and scripts are the browser's, and are 200 ms of the 206 ms measured. The product's share cannot be read from a wall-clock difference | 225 | High | A budget that cannot be held | Not fixed: the share needs the trace's division of time between the engine and the wait; `run trace` is not built. The row is left as the aim and the number is recorded as an upper bound |
| 4 | Three rows have no scenario at all and say so only now: the policy-check, scheduling and ledger split of a call, the warm tab alone, and `page_http`; `api` is the same row as `http` | 225 | Medium | Unmeasurable as designed | Recorded in 225 as not measured, with what each needs: the trace's split, and a benchmark project with a `page_http` binding |
| 5 | The 10% gate compares medians. On a shared machine the median of the same build moved from 10 ms to 31 ms between runs, while the minimum barely moved. A gate on medians of a shared runner fails builds for noise, and the noise floor that would hide the noise hides a real regression | 225, `bench/` | High | Design of the gate | Not fixed, and nothing weakened: the comparison stays out of CI until a baseline from a quiet runner and a statistic are chosen; listed as open in decision 0363 |
| 6 | The program has grown from 18.5 MB to 23.7 MB since the release profile was set; the 25 MB target for H-28 has 1.3 MB left, and the wheel (11.4 MB) is far under | 605, H-28 | Low | Margin | Recorded as a line under H-28; no status change |
| 7 | `a2m` through the script is 16 ms against 6 ms for the program, as the first measurement said; no assumption contradicted | 605, H-39 | None | Confirmed | A line under H-39 |
| 8 | Idle memory (37 to 40 MB against 50) and listing tools (1.5 to 3.5 ms against 5) hold. The listing was timed through the command front door; the MCP `tools/list` itself was not | 225 | Low | Confirmed, with a gap | The gap is open in decision 0363 |

## What this round did not look at

The other two operating systems (CI's, not measured here), a machine with a key store, the npm launcher, the benchmark of a write, and a quiet runner. The round wrote documents and the harness only.
