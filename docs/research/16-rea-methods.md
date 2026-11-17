# Research: how the REA project works, and what applies here

**Status:** completed, from source. REA was read at commit `8ffea11` of <https://github.com/morluto/rea> on 2026-11-16 and 2026-11-17, by five read-only studies of different parts. Nothing in it was run. The claims the design rests on were then checked against the source by hand. What could not be determined is listed at the end. Feeds [spec sections 56 to 61, 63, and 64](../spec/README.md).

REA ("Reverse Engineer Anything") is a command-line tool and MCP server for understanding software without its source. It is MIT licensed, published on npm as `rea-agents`, written in TypeScript for Node.js 22 or newer, and has roughly 200 tools; its own documents record 207 and 234 at different dates.

File paths below are in REA's repository.

## What it is not

REA discovers and explains. It does not carry out business operations, and it never verifies a write against the system it wrote to. Its own documents say that its effect declarations "are hints, not authorization controls" and that the host "must enforce the user's tool permissions and approvals" (`docs/reference/mcp-contracts.md`, `SECURITY.md`). Everything below is taken for its method, not as a base to build on.

## 1. Architecture

- **Layers in one table.** `scripts/lib/module-layers.mjs` orders its units: kernel, domain, contracts, foundation, ports, shared provider core, providers, application, composition, adapters. A dependency may point to the same or a lower layer, and "an undeclared unit fails the check". `scripts/verify-module-boundaries.mjs` enforces it as part of lint, resolving every import.
- **A baseline that may only shrink.** Known violations are counted in a file. Growth fails the build; so does shrinkage, until the count is lowered on purpose.
- **Entry files, not barrels.** Each provider lists the only files that outside code may import (`docs/adr/architecture/provider-entry-files.md`). A single re-exporting index was rejected because it defeats lazy loading: one module's import closure was 55 files with entry files and 1,408 through an index.
- **Ports and providers.** A port is an interface (`src/ports/AnalysisProvider.ts`): identity, capabilities, and a client with `execute` and `close`. A provider is one outside engine behind that interface. One file is "the sole production wiring point", shared by the command line and the MCP server.

## 2. Controlling outside engines

This is the part most useful to a runtime that must supervise many subsystems.

- **A bridge inside the host.** REA places a small script inside each analysis tool: Python in one, Java in another, a plug-in in a third (`bridge/`). The script answers typed requests and does nothing else.
- **Three ways in.** Start the host without a window with the script as an argument; start the host and load the script through a bootstrap file; or attach to a host the user started, through a plug-in the user installed.
- **A private handshake.** Each launch gets a random run identifier and token, handed over in a file only the user can read, deleted after reading. The bridge must return the run identifier, and for one engine the digest of the file it has open and its own version, before any work. A bridge attached to a different file is refused.
- **Mutual proof when attaching.** For a host the user started, both sides answer a challenge with nonces before any request (`bridge/cutter/rea_bridge.py`), and every request carries the document generation it expects.
- **One framing.** Newline-delimited JSON over a socket only the user can open, with a byte ceiling both sides must agree on. One shared reader handles framing and matching replies. An unknown reply, an oversized line, or bad text closes the stream.
- **Every failure says whether the command ran:** not started, or unknown. Some bridges keep completion receipts so a lost reply can be settled.
- **Cancelling is never free.** For one engine, cancelling releases the caller while the engine stays busy and is shown as busy. For another, cancelling closes the session.
- **No priority, no fallback.** A provider is selected by the request, by a setting, or because it is the only usable one. Otherwise the answer is "ambiguous", with a rejection code and a remedy for each candidate. "Registration order never affects selection" (`src/application/binary/AnalysisProviderRegistry.ts`).
- **Declared against observed.** What a provider can do is a static declaration that loads no code. Whether it is available here is observed "without starting an analysis process".
- **Owned processes.** A child is started in its own process group, or under a job object on Windows, with a run token in its environment. Before signaling, every process in the group is checked for the token; "unavailable identities must never be skipped", so cleanup can end as not clean, with a reason. "REA never kills a process it cannot prove it owns." The process list is read from fixed system paths so that a caller's search path cannot replace it.
- **Sessions that are counted.** Debugger sessions have identifiers, a limit, and a registry. A stopped session keeps its place until it is closed.
- **Engines by digest.** An outside engine is identified by its SHA-256. The source revision is recorded only when the bytes match a pinned release, and is null otherwise. "REA never downloads engines during an analysis operation."
- **A doctor that changes nothing.** It classifies each provider as healthy, missing a dependency, failing at run time, on an unsupported host, drifted in configuration, or unknown, and may answer "cannot be established".

## 3. Evidence

- **One record for every result** (`src/domain/evidence.ts`): a subject, the provider and its version, an analysis profile, the operation and its parameters, raw and normalized results, limitations, links to other records.
- **Identifiers are content digests.** Display names and local paths are left out. A record whose identifier does not match its content is rejected.
- **Two separate axes.** Confidence is observed, derived, or inferred. Authority is one of five sources, from a shipped artifact to an analyst's inference. There are no numeric scores except in matching heuristics.
- **A subject for everything** (`docs/adr/evidence/target-identity.md`). The decision record found that 50 of 120 places could name no subject, so that "a rebuilt binary under the same path, or a different page at the same tool input, is indistinguishable". It adopts one tagged identity: a file by digest, an address, a process by identifier and start time and program digest, a session, or unknown with a reason from a closed list.
- **Never stronger than a parent.** A view is never more certain than what it was made from; one templated path segment makes the whole page inferred.
- **Coverage apart from content.** Complete, partial, or unknown. "Missing coverage is unknown, not absence."
- **Described against observed.** Comparing endpoints found in code with endpoints seen in traffic gives matched, not observed in this capture, observed without a static match, or unknown. Not observed "never means hidden, unused or inaccessible".
- **Unknowns are records.** Each has a question, a scope, a status, what would settle it, and a resolution that needs supporting evidence.
- **Verdicts per claim.** A verification passes claim by claim, and any limitation or open unknown turns a pass into unknown.
- **No migration.** Evidence in an old format is refused, not rewritten, because rewriting "would claim an observation REA did not make".

Weaknesses we avoid: omitted fields default to the strongest values; limitations are free text; four vocabularies overlap where one ladder would do; pass flags and owner digests in its ledgers are supplied by the caller; engine verification rests partly on a version string; kept evidence lives in memory for one connection.

## 4. Tools for agents

From `docs/development/tool-design.md` and `docs/reference/mcp-contracts.md`.

- **One contract per tool** holds name, description, input and output schemas, effects, annotations, and validated examples. The command line derives from the same contract, and a test checks parity.
- **Ten effects per tool**, declared and reviewed: changes the target, changes the session, writes files, starts a process, uses the network, changes the screen, may discard data, runs code it does not own, changes a device, is idempotent. Protocol hints are derived only from these. A reviewed list of destructive tools grows automatically and shrinks only by a deliberate edit.
- **When to add a tool:** for "a distinct outcome or materially different authority". Parity with an upstream interface is not a reason.
- **Primitives before workflows.** No orchestration language without observed need.
- **Descriptions:** the first 240 characters must stand alone; effects and the line to neighboring tools come early.
- **Recovery:** malformed input and a valid selection that cannot resolve are different failures. "An error description is not a repair instruction." Every error carries a required remedy and may carry one of a few recovery kinds.
- **Context:** compact input schemas where "every rewrite is exact or widening", with the full schema still validated; output schemas left out of the tool list because they were about 92% of it; summaries with a reference to a kept result and one shared paging tool.
- **Limits:** each is a named constant with a unit; a hit names the setting, its value, its bounds, and how to change it; fixed ceilings say so.
- **Progress:** only when asked, a rising count, never an invented percentage.
- **A recorded catalog:** written at build time with a digest, checked at start, and tested against a live listing.
- **Target text is untrusted by default**, with a short reviewed list of exceptions.
- **Testing with a real agent:** a real client in a throwaway home, closed questions checked against a fixture, one task whose right answer is to call nothing, and separate scores for selection, arguments, recovery, and final claims.

## 5. Observing applications

- **Browsers.** A passive client of its own on the DevTools protocol that attaches and "is not a general browser automation or remote-control surface"; and an active one on Playwright that runs declared scenarios in a fresh temporary profile. Endpoints must be loopback.
- **Scenarios.** Declarative steps with stable names; a fixed environment; everything captured is opt-in; events attributed to steps. Authorization and cookie values are removed by structure; declared secrets become references; passive capture reduces bodies to typed paths without values. Limits are fixed and omissions are counted.
- **From capture to model.** Paths are templated by four strict format rules, with variance rules opt-in and marked inferred. A further view infers record types and identifier fields from reads, and "a view plan is kept only when it reproduces the recordings".
- **Static analysis of JavaScript** without executing it: modules, routes, endpoints reached through unmodified platform functions, and for Electron the boundaries between its processes. Every fact carries its authority and state.
- **Tools a site declares for agents** are read through an experimental browser interface, never called, and returned as "page-declared-untrusted" with a digest of each schema.
- **Desktop.** Accessibility is read on macOS only, through a Swift bridge that first verifies the process by identifier, program digest, and start time.
- **A witness for a check:** it must fail on the seed, pass after the change, fail on a near miss, and reset cleanly (`docs/desktop-build-witness.md`).
- **Comparing versions** of an application graph pairs entities by a ladder of matches and reports absence only with complete coverage.

Not taken: attaching to a person's own browser profile, which our decision 0031 forbids; locators by style selector or child position; driving a desktop by pixels.

## 6. Firmware, devices, and live processes

- **Files only.** Firmware containers, device trees, kernel modules, saved signal recordings, and saved packet captures are parsed from files. "It does not acquire samples or communicate with hardware." It "never captures live traffic". Nothing is written to a device, nothing is decrypted, no keys are recovered.
- **Bounded extraction.** Input size, time, memory, entry count, and depth are capped; the destination must not exist and is never overwritten.
- **No hardware and no emulation.** No bus, no debug probe, no flashing tool, no full-system emulator appears in the source.
- **One place it changes a device:** six tools for a phone or emulator attached for debugging. Each requires the device's serial, takes a fixed argument list, and declares that it changes a device. Pushing a file takes the bytes first, probes the path, refuses to overwrite unless asked, stops on an unclear probe, and reads the device's digest back. There is no confirmation step and no dry run.
- **Live processes.** Instrumentation attaches to any process the caller names. Debugger tools accept unrestricted commands; REA bounds the session's lifecycle instead, and never kills an attached target.
- **Stated limits.** Its disclaimer refuses help with defeating copy or licence checks, authentication, or bot protection, and notes that "access to a debugging endpoint does not by itself establish permission".

## What we take

| Idea | Where it lands |
| --- | --- |
| Declared capability apart from observed availability | Section 56 |
| No priority or silent fallback between providers | Section 56 |
| The private launch handshake; one framing; a broken stream is closed | Section 57 |
| Every failure says whether the request ran | Section 57; it matches `possible_commit` |
| Ownership proven at the moment of stopping; cleanup may be incomplete | Section 57 |
| Bridges inside a host, three ways in | Sections 54 and 57 |
| Components by digest, never fetched during a run | Sections 57 and 65 |
| A tagged subject for every finding | Section 58 |
| Level apart from source; never stronger than a parent | Section 58 |
| Coverage apart from content | Section 58 |
| Described against observed | Section 58 |
| An unknowns ledger | Section 58 |
| Stored evidence is not migrated | Section 58 |
| Scenarios with named steps; shape by default; two tiers of path templates | Section 60 |
| A model is kept only if it reproduces the recordings | Section 60 |
| A check must be able to fail | Section 60 |
| Hints derived only from declared effects | Section 61 |
| Take the bytes, probe, read back as its own fact | Sections 61 and 63 |
| The tool doctrine | Section 64 |

## What we do differently, on purpose

| REA | Here |
| --- | --- |
| Effects are hints; the host enforces | The runtime is the host. Effects are enforced, and the listing says how strongly |
| It records and never verifies a write | Every write is read back, or is `irreversible` |
| Roughly 200 tools listed by default | Under 20 listed; the rest found by search |
| Limits can be raised through a tool | An agent cannot raise a protected limit |
| Evidence is kept in memory for one connection | Evidence is stored, encrypted, and retained by class |
| Four overlapping vocabularies of certainty | One ladder of seven levels |
| Missing fields take the strongest value | Missing fields take the weakest |

## Could not determine

- Whether every producer in REA now names a real subject; the change was landing in slices.
- Whether any reader exists for the ownership record that would let leftovers of a crashed process be cleaned at the next start.
- Whether its agent tests run in CI or only by hand.
- The exact tool count at this commit.
- The transports of four of its engine providers, which were not traced.
- Whether any client acts on its effect hints.
