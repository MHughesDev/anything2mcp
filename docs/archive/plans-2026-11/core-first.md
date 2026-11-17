# Core first

> Archived on 2026-11-17. This plan was written for the first design and is kept as the record of it. The order of work is now the [roadmap](../../plan/roadmap.md).

The order in which the core is built: as thin slices, each one working end to end before the next begins. This sits inside the phases of the [build plan](../spec-2026-11/36-build-plan.md) and refines the task lists for Phases 0 to 3. Where a task list groups work by component, this document orders it by what can be shown working.

## What the core is

| Core | Not core, built later on top |
| --- | --- |
| The engine loop | Servers, toolsets, grants |
| One driver: the interface driver | The HTTP, API, MCP, and desktop drivers |
| The compiler and the plan | Endpoint mining and shadow promotion |
| The local engine and its two faces: commands and MCP | REST, SDKs beyond thin clients, the console |
| The session keeper and the vault | Hosted sign-in, runners, identity modes on a server |
| The ledger and the policy gate | Undo, triggers, metering, target packs |
| Evidence: the step trace | Outside monitoring, the repair loop |

Nothing in the right-hand column is started until the left-hand column works for reads and writes on one machine.

## The walking skeleton

The first goal is not a finished part. It is one read travelling through every layer, each layer as thin as it can be, so that the shape of the whole is proven before any part is deepened.

## Slices

Each slice follows the loop in the [working method](../../plan/working-method.md). "Shown working" is what the owner sees at the end.

### Phase 0

| # | Slice | Shown working | Hypotheses |
| --- | --- | --- | --- |
| 0.1 | Repository skeleton: workspace, empty crates, CI on three operating systems, the documentation audit in CI | A green build on a pull request | None |
| 0.2 | The release workflow and a placeholder release to PyPI | `pip install anything2mcp` installs the placeholder | None |
| 0.3 | The test application: cases 1 to 4, the control interface, the ground-truth harness | A test resets the application, changes a record, and asks the application what it stored | None |
| 0.4 | Spike: the browser engine base, against the conformance suite's first groups | A results table per candidate and a decision | H-01, H-02, H-03 |
| 0.5 | Spike: an MCP server with one tool over stdio | Two MCP clients call the tool | H-06 in part |
| 0.6 | Core types and file schemas | The `example` target's files validate; broken ones fail with a place and a fix | H-22 in part |

The desktop spike in the Phase 0 task list can wait until just before Phase 8. It does not block the core.

### Phase 1

| # | Slice | Shown working | Hypotheses |
| --- | --- | --- | --- |
| 1.1 | **The skeleton.** A hand-written plan, the engine loop, the interface driver reading one field, a result printed by a command. No compiler, no local engine, no MCP | One command reads one value from the test application | H-02 |
| 1.2 | The step trace and its signature | Five runs, one signature | H-16 |
| 1.3 | The compiler replaces the hand-written plan | The same read, now from target files | H-14, H-22 |
| 1.4 | The local engine: one process, two clients | Two commands at once share one browser | H-06, H-21 |
| 1.5 | The action registry and the MCP face | An MCP client calls the same read; a hand-written face fails CI | H-13 in part |
| 1.6 | The rest of the read verbs, locators, and conditions | The Phase 1 cases of the test application pass | H-02, H-03 |
| 1.7 | Measure every budget; hold a design review round | Numbers, and a revised design where they disagree with it | H-20, H-25 |

### Phase 2

| # | Slice | Shown working | Hypotheses |
| --- | --- | --- | --- |
| 2.1 | The secret type, the redactor, and the planted-secret test | A planted secret appears nowhere | None |
| 2.2 | The vault with the operating system's key store | A saved value survives a restart and cannot be read without the store | None |
| 2.3 | The sign-in window and one auth provider | A person signs in to the test application once; a read then works | H-05 in part |
| 2.4 | Session states, the probe, and expiry | An expired session returns `needs_login` | None |
| 2.5 | Renewal, with one renewer | Ten runs, one renewal | H-04 |
| 2.6 | The remaining providers and discovery | Each sign-in variant of the test application is classified and works | H-05 |
| 2.7 | A design review round. The read on a real application that stood here moves to the end of the build, with the owner ([decision 0058](../../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md)); until then the proof is five identical runs of a signed-in read on the test application | Five identical runs | H-05, H-22 |

### Phase 3

| # | Slice | Shown working | Hypotheses |
| --- | --- | --- | --- |
| 3.1 | The ledger tables and its nine rules as tests, with no application involved | Each rule fails a test when broken | H-15 |
| 3.2 | The policy gate with the built-in local policy set | A write without approval is refused | H-11 |
| 3.3 | One write to one field, with read-back, approved by a stub | The test application confirms one write | H-08 in part |
| 3.4 | Field types and normalization | A value the application rounds is compared correctly | None |
| 3.5 | Preview, the diff, and its hash | The same hash on three operating systems | None |
| 3.6 | The approval surface replaces the stub | A person approves; a script cannot | H-12 |
| 3.7 | Stop and resume | Killed at a random step, resumed, no duplicate | H-15 |
| 3.8 | Identity strategies and dependent records | A header and its lines, created once | H-09, H-23, H-24 |
| 3.9 | Sources and a full pipeline | A spreadsheet entered, then run again with nothing changed | None |
| 3.10 | Engine faults and the first manipulation tests; a design review round | Each fault leaves no duplicate and no unverified record | None |

## Rules of pace

- One slice at a time. A slice is not started while the one before it is unproven.
- A slice that grows past a session is split, not stretched.
- Spike code lives in `spikes/`, answers one question, and is deleted or left behind. Real code is written fresh.
- Every slice ends with the documents true and a journal entry written.
- After Phase 3 the first usable product exists. Stop there, use it, and review the design before choosing what comes next. The order of Phases 4 to 9 is a proposal to be reconsidered then, not a commitment.
