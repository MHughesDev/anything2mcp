# Build readiness

An audit of the whole documentation set, made on 2026-11-15, to answer one question: can building start?

## Verdict

**Phase 0 can start now.** Nothing in the design blocks it.

**Phases 1 to 3 are designed in enough detail to build**, once the research listed for each has been run. Together they produce the first usable product: a person on one machine reading from and writing to an application through tools an agent built, with sign-in kept alive and every write approved, verified, and resumable.

**Phases 4 to 9 have their cores designed and some parts still open.** Each open part is listed below. None needs deciding before Phase 3 is finished.

"Ready" here means the design is complete, consistent, and specific enough that a builder does not have to invent architecture. It does not mean the design is proven. Most of the research behind it comes from search-result summaries, and every speed figure is a target. The spikes in Phase 0 and the measurements in Phase 1 are where the design first meets reality, and it should be expected to change there.

## What the audit checked

A script, [`tools/docs_audit.py`](../../tools/docs_audit.py), now ships with the repository and checks these on every run:

| Check | What it catches |
| --- | --- |
| Links | A link to a file that does not exist |
| Structure | A spec section missing from the index or numbered wrongly |
| Tool names | A built-in tool that is not an action in the catalog |
| Crates | A crate named in one place and missing from the layout or the layering table |
| Failure classes | A class in the engine section but not the reference, or the reverse |
| States | A run, record, or session state missing from the place that should list it |
| Commands | A command that is not a shortcut, a process command, or a catalog action |
| Replaced statements | Wording that a later decision withdrew |
| References | A section, phase, or research brief number that does not exist |
| Counts | An index that states the wrong number of files |
| Fixed sets | A count of modes other than seven, or identity modes other than four |

The script reports no findings on the set as delivered. Run it after any documentation change.

Beyond the script, each of the 52 sections was read against the later sections that refine it.

## What the audit found and fixed

| Problem | Fix |
| --- | --- |
| Commands were named two ways: flat in the reference, noun and verb in the authoring section | One [action catalog](../reference/action-catalog.md) now lists every action; commands, MCP tools, routes, and SDK methods all take their names from it |
| Three built-in tools did not follow the naming rule | Now `w2m_tool_call`, `w2m_tool_search`, and `w2m_draft_try` |
| The failure classes `input` and `busy` were in the reference but not in the engine's table | Added to the engine's table |
| The engine was said to attach to the user's running browser | Withdrawn in five places; the engine always uses its own browser profile |
| Each run was said to get a copied browser context | Replaced: runs share one live session in separate tabs |
| Solo was described as one process per MCP client | Replaced by one engine per user |
| The pip name was said to be taken | Corrected in three places |
| The status list an agent sees was shorter than the ledger's run states | Aligned |
| Session states in the schema did not match the session keeper | Aligned |
| The record state `unchanged`, and the case of a record changed by someone else, were missing from the ledger outline | Added, with a new failure class `stale` |
| No glossary; the same thing had two names in places | A [glossary](../reference/glossary.md), and a rule to use it |
| Three core parts were named but not designed: the compiler, the trace format, and the vault's keys | Sections 46, 47, and 48 |
| Index counts were out of date | Corrected, and now checked by the script |

A second pass asked a different question: what would a builder have to invent? It found five things.

| Missing | Added |
| --- | --- |
| No field-by-field definition of the target files; only examples | The [file format reference](../reference/file-formats.md): every field, the locator kinds, the conditions, the built-in field types, and the expression functions |
| The steps a plan is made of, the snapshot an agent sees, and the diff a person approves were named but not defined, including how the diff hash is computed | [Steps, snapshots, and diffs](../reference/steps-snapshots-and-diffs.md) |
| What a person actually sees when asked to approve or sign in, which Phase 3 needs | Section 49: a diff page served for that moment, then an operating-system prompt; no tray application in the first release |
| Where the engine keeps things on disk, and what its logs are | The on-disk layout in the technical reference; logs in section 47 |
| No statement of what the tool sends out by itself | Nothing, by default; section 41 and decision 0040 |

A third pass looked for places where the design was weaker than it should be, not merely incomplete.

| Weakness | Change |
| --- | --- |
| Any record the application assigns an identifier to had to be treated as irreversible, which ruled out resuming the commonest pipeline: a header and its lines | Section 50: five ways to find a record again, assigned keys and results kept in the ledger, and dependent records |
| An agent steered by text it read could pass a crafted input to redirect a request or name any local file for upload | Section 51: typed request templates, allowed file roots, downloads as data |
| A steered building agent could write target files that do more than they claim, and the reviewer would see only a file difference | Section 51: a review summary ordered by risk; drafts cannot widen a target's reach |
| On one machine an agent with a shell could loosen safety settings | Section 51: protected settings need the user-presence prompt |
| Nothing said what happens when the engine's own database, disk, key store, or browser fails | Section 52: three rules, three health states, a response for each failure, each a test |
| A newly published version took over at once | Section 52: published, then candidate with shadowed reads, then current |
| The fallback browser engine could not be compared fairly with the first choice | A driver conformance suite both must pass, in the [test application plan](test-application.md) |
| No description of what the finished product should feel like | A [walkthrough](../reference/walkthrough.md) that doubles as the Phase 5 acceptance test |
| Performance figures disagreed between two sections, and one still spoke of browser contexts | Aligned |
| Only Phase 0 had a task list | Draft task lists for Phases [1](phase-1.md), [2](phase-2.md), and [3](phase-3.md), each to be revised as the phase before it ends |
| Phase 1 was to be proven on a real application before sign-in existed | That test moved to Phase 2 |

## Requirement coverage

Every requirement the owner stated, and where the design meets it.

| Requirement | Where | Status |
| --- | --- | --- |
| Latch onto any existing application and expose MCP tools | Sections 1, 6, 21 | Designed |
| Use the application through the user's own account | Sections 15, 16, 44 | Designed; depends on the organization's sign-in rules |
| Only that user or their agents may use their account | Sections 44, 45, 48; decision 0035 | Designed |
| Custom sign-in or existing sign-in, discovered automatically | Sections 15, 44 | Designed; silent sign-in in the engine's profile is unverified |
| Sessions kept between calls; no approval of MFA per call | Section 44 | Designed, within the organization's maximum session age |
| Reads and writes; single tasks and end-to-end pipelines | Sections 5, 7, 8, 42 | Designed |
| Several modes, chosen by the runtime | Sections 6, 8 | Designed |
| Infer backend calls and use them directly where proven | Section 26 | Designed; viability depends on each application |
| Deterministic, repeatable, reproducible, safe | Sections 2, 3, 42, 46, 47 | Designed; proven only by the gates once built |
| Many agents at once | Sections 11, 39, 43 | Designed |
| Usable by software, not only MCP | Sections 22, 40, 41 | Designed |
| Any coding agent can build tools; minimal work for the agent | Sections 26, 40 | Designed; the usability gate measures it |
| Any operating system | Sections 23, 29, 39, 48 | Designed; desktop mode is the least certain |
| pip package, TypeScript package, or cloned into a repo | Section 41 | Designed; names not yet reserved |
| Large applications with many modules | Section 14 | Designed |
| Many kinds of interface and edge cases | Sections 13, 14 | Designed as a catalog; each entry is built and tested in Phase 8 |
| A dedicated machine serving a company, tools exposed to chosen users | Sections 16, 20, 23, 24, 43, 45 | Designed |
| Hosted agents such as Copilot Studio can connect | Sections 22, 24 | Designed; untested against the product |
| Runs entirely on one user's machine | Sections 23, 39 | Designed |
| Rust core, other languages where applicable | Section 29; decision 0002 | Decided |
| No real application named | Decision 0021 | Done |
| Speed as a core metric | Section 9 and each section's targets | Budgets set; none measured |

## Readiness by phase

| Phase | Design | Run first | Biggest risk |
| --- | --- | --- | --- |
| 0. Foundations and spikes | Complete | Brief 1 | The browser engine spike finds no good base |
| 1. Solo read path | Complete | Briefs 2, 3, 11, 26, 28 | Waiting and acting reliably through the DevTools protocol |
| 2. Sessions | Complete | Briefs 4, 5, 6, 30 | The organization's sign-in rules refuse an automated browser |
| 3. Writes | Complete, except the per-platform presence prompt | Briefs 7, 8, 12, 21, 27, 31 | Approval on a machine where the agent has a shell |
| 4. HTTP modes and mining | Core complete | Brief 9 | Applications that compute or sign in the browser |
| 5. Authoring and ease | Complete | Briefs 10, 29 | An agent with no prior knowledge not finishing in the target time |
| 6. Server | Core complete; admin console screens not designed | Briefs 13, 14, 15, 16 | Hosted agents' protocol support differs from the specification |
| 7. Runners and identity | Core complete; hosted sign-in page not designed | Briefs 17, 18 | Per-user modes blocked by device rules |
| 8. Breadth | A catalog, not yet designs | Briefs 19, 24 | Desktop accessibility libraries are immature |
| 9. Operations and hardening | Outlined | Briefs 20, 22 | Repair quality decides how much downtime drift causes |

The [phase map](phase-map.md) lists what to read and which decisions bind each phase. Phases 0 to 3 also have task lists.

## Risks

Ordered by how much they could change the design.

| # | Risk | What limits it |
| --- | --- | --- |
| 1 | The Rust browser engine proves much harder than a dozen verbs suggests | A Phase 0 spike with measurements; a Playwright sidecar behind the same driver interface as the fallback |
| 2 | An organization's Conditional Access rules refuse sign-in from an automated browser, or bind sessions to hardware | Four identity modes; user-device runners; service accounts; brief 30 before Phase 2 |
| 3 | The application's owner or the organization's IT does not agree to the tool being used | An authorization record is required before publishing on servers; the design operates openly; this cannot be engineered away |
| 4 | Scope: 48 sections and 10 phases never reach a release | Phases 1 to 3 are a complete product for one person; nothing later is needed to ship it |
| 5 | A runner that lost its lease writes late | Early stop, quarantine, read-back, later verification; the remaining case is documented in section 43 |
| 6 | On one machine, an agent with a shell is inside the user's trust boundary | No approve action; operating-system user-presence checks; hardened options; the limit is documented in section 39 |
| 7 | Mined HTTP bindings miss logic the application's front end performs | Shadow comparison and promotion by a person; the UI binding stays |
| 8 | Speed budgets are unrealistic | They are targets; Phase 1 measures and revises them by decision record |
| 9 | MCP clients differ from the specification and from each other | A Phase 0 spike against real clients; a compatibility table kept as data |
| 10 | Cedar does not fit the size or speed budget | A spike before Phase 3; an in-house matcher with the same semantics as the fallback |
| 11 | More sessions become bound to their device, so fewer can move between runners | Already designed for: such sessions are marked not movable |
| 12 | The package names are taken before they are reserved | The first task of Phase 0 |
| 13 | A decision rests on a research summary that turns out wrong | Every note states its status; AGENTS.md requires verifying before depending |

## Not designed yet, on purpose

These are named in the specification and left for the phase that builds them. A builder reaching one should stop and design it with the owner, not improvise.

| Part | Phase | Why it can wait |
| --- | --- | --- |
| The hosted sign-in page that shows a server's browser to a remote person | 7 | Needs brief 17; only one identity mode depends on it |
| The scheduler beyond its settings: fairness between callers and priorities under load | 6 | The queue, locks, and limits are designed; one machine needs no more |
| Operating-system user-presence prompts, per platform | 3 | Needs brief 27; a dialog-only fallback exists |
| Admin console screens | 6 | Every admin function already exists as an action |
| Each widget driver and platform profile in detail | 8 | A catalog exists; each is built against the test application |
| The desktop driver's internals | 8 | Depends on the Phase 0 spike |
| Triggers and schedules | 6 or later | Runs can be started by any outside scheduler through the REST API |
| Usage metering | 9 | The ledger already holds the data |
| Shared target packs | 9 | A distribution question, not an engine one |
| The plug-in software kit | 8 | The protocol is outlined; in-tree drivers come first |
| A browser extension that works inside the person's own profile | Later | A possible answer to single-session applications; not committed |
| A vision mode for pixel-only applications | Later | A stated non-goal for now |
| High-availability details of the Cluster profile | 9 | The stateless front and outbound runners already allow it |

## Before the first build session

Things only the owner can do.

- [x] Hold `web2mcp` on crates.io.
- [x] Hold `w2mcp` on npm.
- [ ] Secure `web2mcp` on PyPI by publishing a first release from the release workflow; see [publishing setup](publishing-setup.md).
- [ ] Decide whether the repository is private. Real target files never go in it either way.
- [ ] Choose where real target files will live.
- [ ] Name the MCP clients that matter most, for the Phase 0 spike.
- [ ] Confirm a Windows machine is available, or that CI is the only Windows environment.
- [ ] For the first real application: get agreement from its owner and from IT before any use on real work.

## Keeping it ready

- Run `python3 tools/docs_audit.py` after every documentation change; it must report no findings.
- When code and a section disagree, change one of them in the same commit.
- When research is run, write the note, update the section, and remove the brief.
- Regenerate the phase map when the build plan, the research table, or the action catalog changes.
