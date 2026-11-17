# Roadmap

Every work package from where the repository stands to version 1.0, in one list: what each package is, which crates it touches, what it needs first, and what proves it. An agent picks the first package marked ready whose crates nobody else holds, builds it, proves it, merges it, and marks it done here.

This document is the order of work, and the only plan. The [architecture](../architecture/README.md) says how the crates fit together, and each crate's design gives its own build order; where they differ on order, this document wins and the other is corrected. The plans it replaced are in the [archive](../archive/README.md).

The order after milestone 3 is a proposal. It is reviewed at the end of every milestone and changed by a line in the journal.

## Core first

The engine loop, one driver, the compiler and the plan, the local engine with its command and MCP faces, the session keeper and the vault, the ledger and the policy gate, and the step trace are the core. Nothing else is deepened until the core works for reads and writes on one machine, which is milestone 3. The first goal is not a finished part: it is one read travelling through every layer, each as thin as it can be, so that the shape of the whole is proven before any part is deepened. A second kind of system is added early and on purpose, at milestone 4, to test that the kernel is neutral, and no more than one.

Since 2026-11-17 the kernel is built layer by layer, with one agent per crate, and the first driver through the loop is the HTTP pack's request driver ([decision 0067](../decisions/0067-the-kernel-is-built-layer-by-layer-in-parallel-and-the-first-driver-is-the-http-packs.md)). The rows below keep their proofs. A row is marked `done` when its proof is shown, whichever crate was built first.

## Reading the tables

| Column | Meaning |
| --- | --- |
| # | The package's name. A slice of the first three phases is numbered by phase; a package of a later stage has a letter |
| Package | What is built |
| Crates | The crates it changes. Two agents never hold the same crate at once |
| Needs | Packages that must be done first |
| Proof | What must be shown before it counts as done. A package without its proof is not done |
| State | `done`, `ready` (everything it needs is done), `held` (waits on a package), or `owner` (needs the owner's hands) |

A package is one branch and one pull request. When it merges, its row changes to `done` in the same pull request.

## Milestones

| # | Milestone | What a developer can do when it is reached | Version |
| --- | --- | --- | --- |
| 0 | Foundations | Nothing yet. The workspace, the checks, the test application, the core types, and the contracts exist | 0.0 |
| 1 | The first read | Read one value from a web application through a command or an MCP client, the same way every time, with a trace | |
| 2 | The signed-in read | The same, on a system that needs a sign-in, with the session kept alive and no secret anywhere it should not be | |
| 3 | Verified writes | Enter a spreadsheet into a web application: previewed, approved, read back, resumable without duplicates. The first usable product | 0.1 |
| 4 | Anything, proven once | Do the same reads over a command-line program, with no change to the kernel | |
| 5 | The scout | Point at a system and get a report: what was found, what was drafted, which reads already work | 0.2 |
| 6 | Fast paths | The same operations over direct requests, proven equal to the interface and many times faster | |
| 7 | The developer's first hour | One install by pip, crates.io, or npm; from an address to published read tools in minutes | 0.3 |
| 8 | More kinds | Databases, files, queues, and simulated devices, each through its own pack | 0.4 to 0.6 |
| 9 | A server | Many users, each acting as themselves, with grants, toolsets, and runners | 0.7 to 0.8 |
| 10 | Breadth | Desktop applications, upstream MCP servers, and the hard cases of web interfaces | 0.9 |
| 11 | Hardening | Real systems tried with the owner; an outside security review; every failure a tested one | 1.0 |

## Milestone 0: foundations

| # | Package | Crates | Needs | Proof | State |
| --- | --- | --- | --- | --- | --- |
| 0.1 | Workspace, empty crates, CI on three operating systems, the audit in CI | All | None | A green build on a pull request | done |
| 0.2 | The release workflow and the pip placeholder | `bindings/python` | None | The build job is green; the package installs from the built wheel | done |
| 0.2p | Publish the placeholder to PyPI, and hold the name on crates.io and npm | None | 0.2 | Each row of [publishing setup](publishing-setup.md) says held | owner |
| 0.3 | The test application: cases 1 to 4, the control interface, the ground-truth harness | `targets/example-app` | 0.1 | A test resets the application, changes a record, and asks the application what it stored | done |
| A1 to A6 | The rename, the kernel and pack split, the audit's rules | All | None | The audit and its planted mistakes | done |
| A7 | Every spec section marked kernel, pack, or product-wide in the index | Documents | A5 | A reader can tell from the index which side a section is on | done |
| AR | The architecture: who defines, implements, and joins every interface; one design per crate; the layers redrawn | Documents; the audit | A2 | The audit refuses a crate with no design and a design with no crate | done |
| B1 | Core vocabularies, `Failure`, `Secret` | `a2m-core` | A2 | Names held to the documents by a test; a secret cannot be printed | done |
| B2 | `Finding`, its `Subject`, `Digest`, and the identifier rule | `a2m-core` | B1 | Two findings with the same content have the same identifier; none can be built above `discovered` | done |
| B2a | The checked names that arrive with packs (surface kinds, step kinds, file kinds, lint codes, finding kinds, control names), and the plan's types as data with the check of a plan's hash | `a2m-core` | B2 | A plan whose bytes changed fails its hash check; a badly formed name is refused. Settled by decisions [0068](../decisions/0068-values-names-and-identifiers-as-the-core-crate-settles-them.md) and [0069](../decisions/0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md) | done |
| B3 | The provider manifest, the registry, and a pack registering nothing but its modes | `a2m-provider` | B1 | A manifest that asks for an undeclared effect is refused with a named failure; a mode registered twice is refused. Settled by decisions [0088](../decisions/0088-the-provider-contracts-outside-crates.md), [0089](../decisions/0089-the-provider-contract-as-building-it-settled-it.md), and [0090](../decisions/0090-the-supervisor-ends-a-group-only-while-its-leader-is-unreaped.md) | done |
| B4 | Target file schemas and the loader, for the file kinds the kernel owns. This is slice 0.6 | `a2m-files`, `a2m-core` | B1 | The `example` target's files validate; a broken one fails with a file, a line, and a fix. Settled by decisions [0078](../decisions/0078-target-files-are-read-by-an-event-parser-into-our-own-located-tree-and-checked-by-our-own-schema-subset.md) and [0079](../decisions/0079-the-compilers-registration-points-and-the-file-formats-as-the-code-settles-them.md) | done |
| B5 | The first two actions, `provider.list` and `provider.describe`, defined and not yet served | `a2m-actions` | B3 | The catalog and the registry agree, checked by a test | ready |
| 0.4 | Spike: the browser engine base, with research brief 1 | `spikes/` | 0.3 | A results table per candidate and a decision record: [decision 0070](../decisions/0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md) | done |
| 0.5 | Spike: an MCP server with one tool over stdio, with research brief 2 | `spikes/` | None | Two unrelated MCP clients call the tool; a decision record on the revisions served: [decision 0071](../decisions/0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md) | done |
| 0.7 | The benchmark harness: measures overhead beyond the test application's own time | `bench/` | 0.3 | A sample benchmark runs in CI and stores its result | done |

## Milestone 1: the first read

Phase 1 of the build plan. One read travels through every layer, each as thin as it can be, before any is deepened.

| # | Package | Crates | Needs | Proof | State |
| --- | --- | --- | --- | --- | --- |
| 1.1 | The skeleton: a hand-written plan, the engine loop with an in-process `Control`, the driver interface, the interface driver reading one field, a result printed by a command | `a2m-core`, `a2m-provider`, `a2m-engine`, `a2m-actions`, `a2m-pack-web-browser`, `a2m-pack-web-ui`, `anything2mcp` | 0.4, B2a, B3 | One command reads one value from the test application. Settles the driver interface by decision record | ready |
| 1.2 | The step trace and its signature. Built so far: the trace and its signature exist in `a2m-telemetry`, proven on simulated steps | `a2m-telemetry`, `a2m-engine` | 1.1 | Five runs, one signature | held |
| 1.3 | The compiler replaces the hand-written plan; packs register their file kinds and lowerings. Built so far: the compiler and the registration points exist, proven through a test pack | `a2m-files`, `a2m-expr`, `a2m-provider`, `a2m-pack-web-ui` | 1.1, B4 | The same read from target files; the plan is byte-identical on three operating systems | held |
| 1.4 | The store for reads, and the local engine: one process, two clients. Built so far: the store exists | `a2m-ledger`, `a2m-local`, `a2m-actions` | 1.2 | Two commands at once share one browser; a different user is refused; a read leaves a run row and its events | held |
| 1.4a | The settings loader: one schema, the layers in order, an unknown key refused, and where each value came from | `a2m-actions` | 1.4 | An empty file is a complete configuration; a file that loosens a protected setting is refused; `config.show` names the layer of every value | held |
| 1.5 | The action registry with generated faces, and MCP over stdio | `a2m-actions`, `a2m-mcp`, `anything2mcp` | 1.4, 0.5 | An MCP client calls the same read; a hand-written face fails CI | held |
| 1.6 | The rest of the read verbs, locators, wait conditions, and widget drivers for reading | `a2m-pack-web-browser`, `a2m-pack-web-ui` | 1.3 | The Phase 1 cases of the test application pass on three operating systems | held |
| 1.7 | Measure every budget; a design review round; a locally built wheel installs | `bench/`, `bindings/python` | 1.6, 0.7 | Numbers recorded, budgets confirmed or revised by decision record | held |

## Milestone 2: the signed-in read

Phase 2. From 2.3 on, the test application gains its sign-in cases.

| # | Package | Crates | Needs | Proof | State |
| --- | --- | --- | --- | --- | --- |
| 2.1 | The redactor and the planted-secret test. Built so far: the redactor exists; the planted-secret test against a front door is to come | `a2m-vault`, `a2m-core`, `a2m-telemetry` | 1.2 | A secret planted in the application's answer appears in no result, log, or trace | held |
| 2.2 | The vault with the operating system's key store. Built so far: the vault exists, with key records in the store; no real key store has been exercised | `a2m-vault`, `a2m-ledger` | 1.4 | A saved value survives a restart and cannot be read without the key store | held |
| 2.3 | The sign-in window and one auth provider | `a2m-auth`, `a2m-pack-web-ui`, `targets/example-app` | 2.2 | A person signs in to the test application once; a read then works | held |
| 2.4 | Session states, the probe, and expiry. Built so far: the keeper exists with one renewer, proven in one process | `a2m-auth`, `a2m-engine` | 2.3 | An expired session returns `needs_login` | held |
| 2.5 | Renewal, with one renewer. Built so far: the keeper exists with one renewer, proven in one process | `a2m-auth` | 2.4 | Ten runs meet an expired session; one renewal happens | held |
| 2.6 | The remaining providers, and auth discovery. Built so far: the neutral providers exist, except `obo_token` and `os_broker` | `a2m-auth`, `a2m-pack-web-ui`, `a2m-pack-http` | 2.5 | Each sign-in variant of the test application is classified and works | held |
| 2.7 | A design review round | Documents | 2.6 | Five identical runs of a signed-in read; findings fixed or recorded | held |

## Milestone 3: verified writes

Phase 3. At its end the first usable product exists: stop, use it, and review the order of everything below.

| # | Package | Crates | Needs | Proof | State |
| --- | --- | --- | --- | --- | --- |
| 3.1 | The ledger tables and its nine rules as tests, with no application involved | `a2m-ledger` | 1.4 | Each rule fails a test when broken | done |
| 3.2 | The policy gate with the built-in local policy set. Built so far: the policy gate exists; its joining in the action registry is to come | `a2m-policy`, `a2m-actions` | 3.1 | A write without approval is refused, and the refusal is recorded | ready |
| 3.3 | One write to one field, with the intent recorded first and the value read back, approved by a stub | `a2m-engine`, `a2m-ledger`, `a2m-pack-web-ui` | 3.2 | The test application confirms exactly one write | held |
| 3.4 | Field types and normalization. Built so far: field types and normalizers exist | `a2m-files`, `a2m-expr`, `a2m-engine` | 3.3 | A value the application rounds is compared correctly | held |
| 3.5 | Preview, the diff, and its hash | `a2m-engine`, `a2m-ledger` | 3.4 | The same hash on three operating systems | held |
| 3.6 | The approval surface replaces the stub | `a2m-policy`, `a2m-local`, `a2m-api` | 3.5 | A person approves; a script cannot | held |
| 3.7 | Stop and resume, with reconcile before any write | `a2m-engine`, `a2m-ledger`, `a2m-sched` | 3.6 | Killed at a random step and resumed: no duplicate, nothing unverified | held |
| 3.8 | Identity strategies and dependent records | `a2m-engine`, `a2m-files` | 3.7 | A header and its lines, created once | held |
| 3.9 | Sources and a full pipeline. Built so far: the readers and the compiler half exist | `a2m-sources`, `a2m-expr`, `a2m-files`, `a2m-engine` | 3.8 | A spreadsheet entered, then run again with nothing changed | held |
| 3.10 | Engine faults and the first manipulation tests; a design review round | All kernel crates | 3.9 | Each fault leaves no duplicate and no unverified record | held |
| R1 | Release 0.1: the changelog, the notices file for third-party licences, a wheel that carries the binary | `bindings/python`, the release workflow | 3.10, F1 | The release steps in RELEASING.md pass; an install from the registry runs a read and a write against the test application | held |

## Milestone 4: anything, proven once

Stage D. The command pack is designed in [section 306](../spec/3-packs/306-the-command-pack.md). It is built read-only first, to answer one question: did the kernel have to change?

| # | Package | Crates | Needs | Proof | State |
| --- | --- | --- | --- | --- | --- |
| D1 | The design of the command pack | Documents | None | Section 306 answers every item of the entry bar | done |
| D2 | The command-line test stand, with its control interface: `targets/example-cli` | `targets/` | D1 | Its own tests, on three operating systems | done |
| D3 | The command pack, read verbs only, as new pack crates; the decision record for its mode | New pack crates | D2, 1.3 | A read through the same engine loop, with one trace signature across five runs | held |
| D4 | The result of hypothesis H-26: every kernel change the pack forced, counted | Documents | D3 | A hypothesis result file. If the kernel changed more than slightly, the boundary is redrawn before another pack starts | held |
| D5 | Writes through the command pack: a declared read-back command, `possible_commit` on a killed process | The command pack's crates | D4, 3.7 | The test stand confirms one write after a kill and a resume | held |

## Milestone 5: the scout

Stage E, with the scout of [section 402](../spec/4-building-tools/402-the-scout-and-who-does-what.md).

| # | Package | Crates | Needs | Proof | State |
| --- | --- | --- | --- | --- | --- |
| E1 | The discoverer extension point, the findings and unknowns store, and the sandbox that makes a write impossible | `a2m-provider`, `a2m-ledger`, `a2m-engine` | B2, B3, 1.4 | A finding cannot be stored above `discovered`; a discoverer that tries to write is stopped | held |
| E2 | The web pack's discoverers: the probe, snapshots, traffic capture, endpoint inference | `a2m-pack-web-probe` | E1, 2.6 | Test application cases for capture and inference | held |
| E3 | Described against observed: the comparison, and the unknowns it raises | `a2m-provider` | E2 | An endpoint present in a description and never seen is reported as `inferred` | held |
| E4 | The drafter, and each pack's drafting rules. Built so far: the drafter's kernel half exists | `a2m-files`, the packs | E1, 1.3 | A draft made by rule compiles, names its findings, and carries no unit or key the system did not state | held |
| E5 | The scout pipeline and its report | `a2m-actions` | E3, E4 | Scouting the test application twice gives the same report, by hash; no write is ever tried. The result of hypothesis H-31 | held |
| E6 | The optional provider for an outside reverse-engineering project, behind the `mcp` mode | `a2m-pack-mcp`, a mapping file | E1, 8.2 | Against the test application only: its findings are stored as `inferred` or `discovered` and nothing can be written. The result of hypothesis H-27 | held |

## Milestone 6: fast paths

Phase 4. The request modes of the web pack and the HTTP pack.

| # | Package | Crates | Needs | Proof | State |
| --- | --- | --- | --- | --- | --- |
| 4.1 | The `http` driver: reads, then writes with read-back | `a2m-pack-http` | 3.7 | The test application's cases pass over direct requests | held |
| 4.2 | The `page_http` driver: requests made from inside the page, with the page's own tokens | `a2m-pack-web-ui` | 4.1 | The anti-forgery case of the test application passes | held |
| 4.3 | Shadow, comparison, and promotion of a binding | `a2m-engine`, `a2m-ledger` | 4.1 | A binding is promoted only after the set number of exact matches; a mismatch stops promotion | held |
| 4.4 | Import of published descriptions, as discoverers and drafting rules; the `api` mode | `a2m-pack-http` | E4 | A described service becomes drafted reads that compile | held |
| 4.5 | The exit test of Phase 4 | Benchmarks | 4.3 | An operation gains a request binding through shadow, returns identical results, and runs at least ten times faster | held |

## Milestone 7: the developer's first hour

Phase 5, with Stage F.

| # | Package | Crates | Needs | Proof | State |
| --- | --- | --- | --- | --- | --- |
| F1 | Research brief 28, then a wheel that carries the binary and the Python client | `bindings/python` | 1.5 | The command and the import both work from one wheel on three operating systems | held |
| F2 | A manual start for the release workflow | The release workflow | None | A run that builds and publishes nothing | done |
| F3 | Provider packages found through a Python entry point | `a2m-provider`, `bindings/python` | B3, F1 | A sample plug-in installed with pip is listed as present and not enabled | held |
| F4 | The binary and the wheel measured with every first-party pack compiled in | Benchmarks | F1 | Numbers against the 25 MB target. The result of hypothesis H-28 | held |
| 5.1 | Every authoring action in the registry, with all faces generated | `a2m-actions` | 3.10, E5 | The catalog and the registry agree; no face is hand-written | held |
| 5.2 | The project folder, the build state, the shortcut commands, and `doctor.run` | `a2m-actions`, `anything2mcp` | 5.1 | A new project goes from nothing to a linted draft with the shortcuts alone | held |
| 5.3 | The guidance file and the built-in documents an agent reads | Documents shipped in the binary | 5.2 | An agent with no prior knowledge finds every step from `docs.get` | held |
| 5.4 | The Python and TypeScript clients, generated; the npm launcher | `bindings/` | 5.1 | Each client calls a read; neither contains engine logic | held |
| 5.5 | The release pipeline for pip, crates.io, npm, and standalone binaries, signed, with a bill of materials | The release workflow | F1, 5.4 | A tagged pre-release installs by each route on three operating systems | held |
| 5.6 | The exit test of Phase 5 | None | 5.5 | An agent takes a new system from an address to published read tools; the first tool in under ten minutes, measured | held |

## Milestone 8: more kinds

One pack at a time, each through the entry bar of [section 202](../spec/2-kernel/202-kernel-and-packs.md). Each is designed: data in [section 307](../spec/3-packs/307-the-data-pack.md), files in [section 308](../spec/3-packs/308-the-file-pack.md), streams in [section 309](../spec/3-packs/309-the-stream-pack.md), devices in [section 310](../spec/3-packs/310-the-device-pack.md). Reads come before writes in every pack, and the order of the four is reviewed when milestone 5 ends.

| # | Package | Crates | Needs | Proof | State |
| --- | --- | --- | --- | --- | --- |
| G1 | The data pack's test stand: a local database with a control query, at `targets/example-db` | `targets/` | None | Its own tests | done |
| G2 | The data pack, reads only, with the store's own catalog as its discoverer | New pack crates | G1, D4 | A read by key; a statement that is not a read is refused by the database, not by parsing | held |
| G3 | Writes: a required key, a rows-affected check inside the transaction, read-back by key | The data pack's crates | G2, D5 | The stand confirms one row changed; any other count rolls back | held |
| H1 | The file pack's test stand: files in a temporary folder, with a control interface that reports the bytes on disk, at `targets/example-files` | `targets/` | None | Its own tests | done |
| H2 | The file pack, reads only | New pack crates | H1, D4 | A value read from a named place in each supported format | held |
| H3 | Writes: a new file that replaces the old one in one step; read-back by opening it again | The file pack's crates | H2, D5 | Killed mid-write and resumed: the file is the old one or the new one, never a mixture | held |
| J1 | The stream pack's test stand: a small local broker with a control interface, at `targets/example-broker` | `targets/` | None | Its own tests | done |
| J2 | The stream pack, bounded reads only; discovery that never consumes | New pack crates | J1, D4 | A read ends within its limit; the stand shows no message was taken from anyone | held |
| J3 | Publishing, with a key that makes it safe to resume | The stream pack's crates | J2, D5 | A resumed publish delivers one message, by the stand's count | held |
| K1 | The device pack's test stand: a simulated device, with a control interface that reports every frame, at `targets/example-device` | `targets/` | None | Its own tests | done |
| K2 | The device pack, reads only | New pack crates | K1, D4 | A register read through the engine loop; the stand shows only read frames | held |
| K3 | Writes against the simulator only: a range limit enforced before a frame is sent; approval by a person every run | The device pack's crates | K2, D5 | A value outside its range sends no frame; a lost link after a write is `possible_commit` | held |
| K4 | Firmware: inspection as discovery through an optional provider; the update operation through the simulated device's own mechanism | The device pack's crates | K3, E1 | Findings are `inferred`; an update without a declared recovery path cannot be published | held |

## Milestone 9: a server

Phases 6 and 7.

| # | Package | Crates | Needs | Proof | State |
| --- | --- | --- | --- | --- | --- |
| 6.1 | Postgres beside SQLite: one schema, both back ends | `a2m-ledger` | 3.10 | Every ledger test passes on both | held |
| 6.2 | MCP over Streamable HTTP, as an OAuth resource server | `a2m-mcp`, `a2m-api` | 1.5 | A client signs in and calls a tool; a token for another audience is refused | held |
| 6.3 | Toolsets, grants, and roles, compiled to policy; versioned policy sets with tests; decision explanations | `a2m-policy`, `a2m-actions` | 3.2 | Two users with different grants see different tools and get different answers | held |
| 6.4 | The REST interface, its generated description, events, and webhooks | `a2m-api` | 5.1 | A program runs a pipeline and follows it to the end | held |
| 6.5 | The approval and sign-in pages on a server; the admin console | `a2m-api`, `web/console` | 6.3 | An approver signs in and approves; the caller cannot | held |
| 6.6 | The exit test of Phase 6 | None | 6.5 | Two users share one server, each acting as themselves | held |
| 7.1 | The runner link: hello, claim, lease, events, needs, heartbeat, result | `a2m-runner`, `a2m-sched` | 6.1 | A job runs on a runner in another process | held |
| 7.2 | Enrollment, lost leases, and reconcile after a lost runner | `a2m-runner`, `a2m-sched`, `a2m-engine` | 7.1 | A runner killed mid-write leaves no duplicate and nothing unverified | held |
| 7.3 | Service accounts, token exchange on behalf of a caller, the hosted sign-in page, user-device runners | `a2m-auth`, `a2m-api`, `a2m-runner` | 7.2, research brief 17 | One pipeline runs correctly in each identity mode | held |

## Milestone 10: breadth

Phase 8.

| # | Package | Crates | Needs | Proof | State |
| --- | --- | --- | --- | --- | --- |
| 8.1 | Spike: desktop accessibility, with research brief 19 | `spikes/` | None | A decision record choosing the base | ready |
| 8.2 | The MCP pack: pass-through to an upstream server through a mapping file, with verification added; a small MCP server as its test stand. The test stand exists at `targets/example-mcp` | `a2m-pack-mcp`, `targets/` | 3.7 | Only mapped tools are reachable; a changed upstream tool is `drift` | held |
| 8.2a | Tools a site publishes, as a mode of the web pack | `a2m-pack-web-ui`, `targets/example-app` | 1.6 | A page of the test application registers a tool and it is called through the engine loop | held |
| 8.3 | The desktop pack | `a2m-pack-desktop` | 8.1, D4 | Test application case 28 passes on three operating systems | held |
| 8.4 | Platform profiles and the remaining widget drivers | `a2m-pack-web-ui`, `profiles/` | 1.6 | Every control in the test application is set and read back | held |
| 8.5 | The edge-case handlers of spec section 301 | `a2m-pack-web-ui`, `a2m-pack-web-browser` | 8.4 | Every hard case in the test application passes on three operating systems | held |

## Milestone 11: hardening

Phase 9, the real systems, and version 1.0.

| # | Package | Crates | Needs | Proof | State |
| --- | --- | --- | --- | --- | --- |
| 9.1 | Canaries, drift detection, and the repair loop | `a2m-engine`, `a2m-actions` | 4.3 | A deliberately changed screen is caught by a canary and repaired with no failed write | held |
| 9.2 | The audit chain, alerts, backup, and upgrades | `a2m-ledger`, `a2m-actions` | 6.1 | A broken chain is detected; a restored backup resumes a stopped run | held |
| 9.3 | Every failure of spec section 224 as a fault test; the manipulation suite of spec section 502 | All kernel crates | 7.2 | Each named failure and each named attack has a test that fails when the defence is removed | held |
| 9.4 | The cluster profile | `a2m-sched`, `a2m-api` | 7.2 | A control plane node is lost and no run is | held |
| 9.5 | Real systems, with the owner: a read, then a write to a declared test record, on a few systems of different kinds | None in this repository | 9.3 | Five identical runs each; nothing about any system is committed here | owner |
| 9.6 | An outside security review, and its findings closed | As found | 9.3 | The review's report, and a test for every finding | owner |
| R2 | Release 1.0 | The release workflow | 9.5, 9.6 | Every milestone's exit test passes on the commit released | held |

## What can run at once

Lanes that do not share a crate. Within a lane, packages run in order.

| Lane | Packages | Why it does not collide |
| --- | --- | --- |
| The kernel | B2, B3, B4, then 1.1 to 3.10 in order | The spine. One agent at a time on each crate |
| Spikes and research | 0.4, 0.5, 8.1, and the research briefs before each phase | Throwaway code in `spikes/`, and new files in `docs/research/` |
| Test stands | D2, G1, H1, J1, K1, and the test application's later cases | Each is its own folder under `targets/` |
| Measurement | 0.7, then the benchmarks of each milestone | `bench/` only |
| Distribution | F1, F3, F4, 5.4, 5.5 | `bindings/` and the release workflow |
| Packs | One agent per pack, once the package it needs is done | Each pack is its own crates |
| Design | Design review rounds; a pack's section before its crates | Documents only; indexes are edited last and in small edits |

Right now, the packages marked `ready` are B5, 1.1, 3.2, and 8.1. Several held rows carry a note of what is built so far: the crate exists, and the row's proof waits on crates above it. Package 1.5 no longer waits on 0.5; it is still held by package 1.4.

## Rules for changing this document

- At the end of every milestone, update the scorecard in [the bar](../spec/1-product/107-the-bar.md). A number that misses its target is a finding: the design changes, or the target does, by decision record.

- Mark a package `done` in the pull request that completes it, and change `held` to `ready` for whatever it unblocks.
- Split a package that grows past a session; give the parts letters, such as 3.7a and 3.7b.
- Add a package when work turns up that belongs to none. Remove one only with a line in the journal saying why.
- Reorder milestones only at a milestone's end, with the reasons in the journal.
