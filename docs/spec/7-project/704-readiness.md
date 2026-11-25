# 704. Readiness

How firm the ground is under each part of this specification: for each section, how complete its design is and how much of it is built; then what rests on judgment alone, where each bet is to be proven, what is left undesigned on purpose, the risks, and what only the owner can do. The order of work is the [roadmap](../../plan/roadmap.md).

"Ready" here means the design is complete, consistent, and specific enough that a builder does not have to invent architecture. It does not mean the design is proven. Much of the research behind it comes from search-result summaries, and every speed figure is a target until it is measured.

## Where the state of the code is kept

This section does not keep its own list of what is built; such a list falls behind the code within a day. Four places are kept current, each by the change that alters it:

- The **roadmap**'s State column, and the "Built so far" note in a row that is held: which packages are done, and what each held package has already built.
- Each **crate design**'s build order ([architecture](../../architecture/README.md)): every step marked Done, Done in part, or Not built.
- The **README**'s Status section: what a person can use today, in one paragraph.
- **[SCOPE.md](../../../SCOPE.md)**: what the product does and does not do today, each "does not" marked as a rule, a choice, or not yet.

In short, as of 2026-11-18: the kernel, nine packs, the local engine with its command line and stdio bridge, and the flow that builds a tool are built and tested in CI on Windows, macOS, and Linux, against the test stands under `targets/`. Pull requests 14 and 15 were merged with every check passing on all three systems. In pull request 14 the desktop pack's reads ran for the first time on Windows and macOS, against their stand, and the shared check of who may change a program ran for the first time on Windows, with the MCP pack starting a local server program there ([decision 0278](../../decisions/0278-what-the-first-runs-on-windows-and-macos-showed-about-the-desktop-pack-and-the-check-of-programs.md)); the network readers ran on both systems too. In pull request 15 the second audit's fixes, the network tools, files handed to the scout, and the command line's log, help, and completion passed there with the rest of the suite. What pull request 16 adds (watches for agents, the publish page's typed review, and the command pack's reading of handed files) has run on Linux only, with its CI in progress. Nothing has been run against a real system, and nothing is published to a package registry. The server roles, remote runners, and plug-ins are not built. No hypothesis has a result file yet; notes in the [register](../../hypotheses/README.md) record what packages have shown so far.

## The state of each section

**Design** says how complete the section's design is. **Built** is coarse on purpose (built, partly built, not built, or not applicable) and names where the detail is kept; it is the one column here that a package changes.

| Section | Design | Built | A builder can | A builder must not |
| --- | --- | --- | --- | --- |
| [101](../1-product/101-summary.md). Summary | A statement of scope | Not applicable | Use it to explain the product | Read it as a list of what is built |
| [102](../1-product/102-requirements-and-guarantees.md). Requirements and guarantees | Designed | Not applicable; [section 702](702-requirements-audit.md) traces each requirement | Hold every change to the guarantees | Weaken a guarantee without the owner |
| [103](../1-product/103-design-principles.md). Design principles | Designed | Not applicable | Settle a conflict by the stated order: safe, correct, fast, easy | Treat a principle as a setting |
| [104](../1-product/104-system-overview.md). System overview | An overview | Not applicable | Find where a part lives | Build from it alone; the architecture and the kernel sections are exact |
| [105](../1-product/105-core-concepts.md). Core concepts | Designed | Not applicable | Use the words as the glossary defines them | Introduce a synonym |
| [106](../1-product/106-what-it-latches-onto.md). What it latches onto | A statement of scope and a catalog | Not applicable; the README's table of packs says which kinds are built | Decide whether something belongs; pick the next pack | Treat a kind of system as supported because it is named, or start a pack from a row in a table |
| [107](../1-product/107-the-bar.md). The bar | Designed | Its scorecard says, row by row | Hold a change to the bar's targets | Move a row forward without its proof |
| [201](../2-kernel/201-access-kinds-and-modes.md). Access kinds and modes | Designed in outline | Partly built: the access kinds are types in `a2m-core` and each pack registers its modes; shadow and promotion are package 4.3 | Classify a new mode; see what its driver must supply | Build a mode whose steps are not yet fixed without fixing them first |
| [202](../2-kernel/202-kernel-and-packs.md). Kernel and packs | Designed | Built: the boundary is enforced by the audit's layering rules | Draw the crate boundary and enforce it; apply the entry bar | Let a pack reach the kernel outside its registrations |
| [203](../2-kernel/203-the-provider-contract.md). The provider contract | Designed | Built for packs compiled in (package B3); plug-ins in another process are package F3 | Extend it by decision record | Add an extension point without a decision record |
| [204](../2-kernel/204-supervising-subsystems.md). Supervising subsystems | Designed as rules | Partly built; the design of `a2m-provider` says what | Build the supervisor against the rules | Promise confinement of plug-ins before the research is done |
| [205](../2-kernel/205-target-files.md). Target files | Designed, with the [file formats](../../reference/file-formats.md) | Built for the kernel's file kinds (package B4) and for each built pack's own kinds | Add a file kind through a pack's registration | Add a field the formats do not define |
| [206](../2-kernel/206-the-compiler-and-the-compiled-target.md). The compiler and the compiled target | Designed | Built; package 1.3's proof, the same compiled target bytes on three operating systems, is not yet shown | Extend it through the registration points | Let the engine depend on the compiler |
| [207](../2-kernel/207-the-engine-loop.md). The engine loop | Designed | Built, for reads and verified writes | Add what a package needs, through `Control` and the driver interface | Let the engine name a mode, a driver, or a pack |
| [208](../2-kernel/208-runs-the-ledger-and-resume.md). Runs, the ledger, and resume | Designed, with open points | Built on SQLite, with the nine rules as tests (package 3.1); Postgres is package 6.1; the `run_steps` and `outbox` tables are not built | Add a table through a migration | Merge a change that breaks a rule |
| [209](../2-kernel/209-record-identity-and-dependent-records.md). Record identity and dependent records | Designed | Built (package 3.8) | Add an identity strategy by decision record | Resume a create with no identity strategy |
| [210](../2-kernel/210-sources-and-value-normalization.md). Sources and value normalization | Designed | Built (packages 3.4 and 3.9), except what package 3.9's row names | Add a source format or a field type | Compare a business value as a float |
| [211](../2-kernel/211-concurrency.md). Concurrency | Designed; a weighted share for a caller is not | Partly built; the design of `a2m-sched` says what. Round robin between callers, priority, locks, and limits are built | Build queues, locks, and limits | Invent a weighted share for a caller; it is designed with Phase 6 |
| [212](../2-kernel/212-policy-and-approvals.md). Policy and approvals | Designed | Partly built: the gate with the built-in local policy set (package 3.2), and roles and grants in part (package 6.3) | Extend the gate's rules | Give any part but the policy engine a decision |
| [213](../2-kernel/213-effects-hazards-and-consent.md). Effects, hazards, and consent | Designed | Partly built; consent records are not built (the design of `a2m-policy`) | Add hazards to operation files and policy | Claim `confined` anywhere yet |
| [214](../2-kernel/214-access-control-toolsets-and-tenancy.md). Access control, toolsets, and tenancy | Designed | Partly built: roles, grants, role grants, toolsets that only narrow, label and option rules, versioned policy sets with a second administrator on publish, and explanations that name the deciding grant are in `a2m-policy` (`crates/a2m-policy/tests/policy.rs`, decision 0328), and the engine serves the built-in toolsets `use`, `build`, and `admin` (`crates/anything2mcp/tests/local_engine.rs`); the actions `toolset.*` and `grant.*` are built in `a2m-actions` ([decision 0339](../../decisions/0339-the-access-actions-the-built-in-toolsets-and-where-a-reach-is-kept.md)), and tenancy waits for the server (package 6.3) | Build it with the server (package 6.3 and Phase 6) | Let any role run as another person |
| [215](../2-kernel/215-sessions-and-the-session-keeper.md). Sessions and the session keeper | Designed | Partly built: the keeper with one renewer (`ten_runs_meet_an_expired_session_and_exactly_one_renewal_happens` in `a2m-auth`), the neutral providers, and the sign-in actions `session.login`, `session.revoke`, `session.list`, and `session.end` (`session.login` and `session.revoke` in `crates/anything2mcp/tests/first_hour_signed_in.rs`, `session.list` in `crates/a2m-actions/src/tests/second.rs`; no test names `session.end`); packages 2.3 to 2.6 say what remains | Build what packages 2.3 to 2.6 name | Renew anywhere but the keeper |
| [216](../2-kernel/216-the-vault-and-its-keys.md). The vault and its keys | Designed | Partly built; package 2.2 says what | Build what package 2.2 names | Keep the root key anywhere but the operating system's key store, a key service, or the passphrase route |
| [217](../2-kernel/217-traces-evidence-and-repair.md). Traces, evidence, and repair | Designed; repair is outlined | Partly built: the trace and its signature (package 1.2); the repair loop is package 9.1 | Build repair in package 9.1 | Store evidence under a plain content hash |
| [218](../2-kernel/218-discovery-and-findings.md). Discovery and findings | Designed; the closed lists of limitation codes and reasons for an unknown subject are fixed in `a2m-core` | Partly built: the findings store and the extension point (package E1); discoverers per pack (package E2); a capture of a person's use is designed and not built (packages E2a to E2); the comparison of what is described with what is observed waits until after the first release (package E3, [decision 0397](../../decisions/0397-what-the-first-release-needs-of-the-packages-left-and-what-waits-until-after-it.md)) | Add a discoverer through a pack | Let anything raise a finding above `discovered` |
| [219](../2-kernel/219-the-evidence-ladder.md). The evidence ladder | Designed | Partly built: no finding can be stored above `discovered` (`a_finding_is_stored_once_and_never_above_discovered` in `a2m-ledger`); a draft is at `reconstructed` (`DRAFT_LEVEL` in `a2m-files`), a scout's report marks a drafted read `executed` once its trial in the sandbox proved it, and `finding.list` shows each finding's level. No code outside `a2m-core` names the levels `schema_verified` and `behavior_verified` | Show the level at review ([decision 0307](../../decisions/0307-what-the-evidence-level-is-for-and-what-a-review-shows-about-a-tools-evidence.md)); no level gates a listing or a call | Add a field that sets a level by hand |
| [220](../2-kernel/220-actions-and-the-authoring-interface.md). Actions and the authoring interface | Designed | Partly built: every action is in the registry; an action with no handler answers `action_not_built`. On 2026-11-18, 38 of the catalog's 107 actions are not built: 37 have no handler, and `self.update` is given to the client end, which does not carry it out; the help of each of the 38 says so on its first line. The eight `watch` actions are built ([decision 0283](../../decisions/0283-watches-reach-an-agent-through-the-registry-and-their-person-confirms-each-on-the-page.md)) | Add each action's handler as its package needs it | Hand-write a face, or add an action that approves |
| [221](../2-kernel/221-the-tool-surface-agents-see.md). The tool surface agents see | Designed | Partly built: MCP over stdio through `bridge` (package 1.5) | Extend it through the registry | Make a tool's result depend on which client called |
| [222](../2-kernel/222-front-doors-and-the-local-engine.md). Front doors and the local engine | Designed | Partly built: the local engine and its clients (package 1.4); the server's front doors are Phase 6 | Extend the local engine | Put engine logic in a client |
| [223](../2-kernel/223-the-runner-protocol.md). The runner protocol | Designed | Not built (Phase 7; `a2m-runner` is an empty crate) | Build it in Phase 7 | Let a runner that lost its lease write without reconciling |
| [224](../2-kernel/224-when-the-engine-itself-fails.md). When the engine itself fails | Designed | Partly built: the fault tests of package 3.10, in the engine, the ledger, the policy engine, the vault, the session keeper, and through the product against the test application (decision 0352); not built: a kernel that recovers by itself from a start with the key store unreachable, and a source of edge blocks; each failure one machine can meet is mapped to its test, or to a roadmap row, in the section's "Proving it" (package 9.3a); a lost runner and a second node are package 9.3 | Build each failure as a fault test | Write while the engine is degraded |
| [225](../2-kernel/225-performance.md). Performance | Budgets set; none measured | Partly built: the harness runs in CI (package 0.7); no budget is measured, and the 10% gate is not wired | Measure against them (package 1.7) | Treat a budget as a fact before it is measured |
| [226](../2-kernel/226-configuration.md). Configuration | Designed | Partly built: the settings loader (package 1.4a). Some declared keys are read by no code yet; [decision 0236](../../decisions/0236-a-declared-setting-whose-feature-is-not-built-is-refused-by-name.md) has such a key refused by name, and is not built | Add a key to the one schema with the code that reads it | Add a setting that weakens a guardrail |
| [301](../3-packs/301-the-web-pack.md). The web pack | Designed; the interface flavors and hard cases are a catalog | Partly built: `ui` reads and verified writes, `page_http` reads and an update; `site_tools`, the widgets, and the hard cases are built (packages 8.2a, 8.4, and 8.5), and platform profiles are applied to locators, to the tier's limit on writes, and to the input commit (package 8.4a), with detection left to the probe (package E2). The pack states on the publish page where a web tool sends: its surface and its declared destinations, to which the browser holds every request of the tool's tab ([decision 0292](../../decisions/0292-a-web-tab-sends-only-to-its-surface-and-the-destinations-its-tool-declares.md)); discovery carries the origins its tab refused and the drafted reads wait in the holding folder with a question for each origin, a surface may refuse origins without a stop, and the sign-in tab holds what a page loads ([decision 0335](../../decisions/0335-a-scout-carries-the-origins-a-page-tried-and-a-surface-may-refuse-origins-quietly.md)); a name the browser looks up without a request is neither held nor stated | Build a widget or a platform rule with its test application case | Build a widget or platform rule without a test application case |
| [302](../3-packs/302-the-web-pack-browser-engine.md). The web pack: the browser engine | Designed; its base is a client of our own ([decision 0070](../../decisions/0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md)) | Built for what its crate design marks done | Extend the client as its crate design orders | Drive the user's own browser profile |
| [303](../3-packs/303-the-http-pack.md). The HTTP pack | Designed in its crate design | Partly built: `http` reads and verified writes, the import of descriptions, and `api` reads pinned to a description's digest ([decision 0389](../../decisions/0389-the-api-mode-binds-a-read-to-a-pinned-description-and-names-its-change-as-drift.md)); not built: `api` writes, GraphQL, OData | Build what packages 4.1 to 4.5 name | Write over a mined binding before it has passed shadow |
| [304](../3-packs/304-the-mcp-pack.md). The MCP pack | Designed in its crate design | Partly built (package 8.2 is done); its crate design lists what is not | Build what its crate design lists as not built | Reach an upstream tool its mapping file does not name |
| [305](../3-packs/305-the-desktop-pack.md). The desktop pack | Designed; its base chosen by the spike of package 8.1 ([decision 0120](../../decisions/0120-the-desktop-pack-stands-on-each-systems-own-interface-through-thin-bindings.md)) | Partly built: reads and writes on Linux; reads on Windows and macOS, run against the stand in CI ([decision 0278](../../decisions/0278-what-the-first-runs-on-windows-and-macos-showed-about-the-desktop-pack-and-the-check-of-programs.md)); writes on Windows and macOS not built (package 8.3); starting an application, keys and clicks, and `desktop_probe` wait until after the first release ([decision 0397](../../decisions/0397-what-the-first-release-needs-of-the-packages-left-and-what-waits-until-after-it.md)) | Build the writes on Windows and macOS in package 8.3 | Click by position on a picture of the screen |
| [306](../3-packs/306-the-command-pack.md). The command pack | Designed | Partly built: reads and writes against its stand, proven through the product, including a kill and a resume (packages D3 and D5); its scout reads a fish completion file and a manual page a person hands it, as text (package X4a, [decision 0284](../../decisions/0284-the-command-packs-scout-reads-a-fish-completion-file-and-a-manual-page-it-is-handed.md)) | Build what packages D3 to D5 name | Start a shell |
| [307](../3-packs/307-the-data-pack.md). The data pack | Designed | Partly built: reads and writes on an embedded database file, and on PostgreSQL behind a feature (package G2 done; G3 in part) | Build what package G3 names | Run a statement an agent wrote |
| [308](../3-packs/308-the-file-pack.md). The file pack | Designed | Partly built: reads of delimited text, JSON, and workbooks; writes of delimited text and JSON (packages H2 and H3) | Build what packages H2 and H3 name | Write a format that has not passed both of its round-trip tests |
| [309](../3-packs/309-the-stream-pack.md). The stream pack | Designed | Partly built: bounded reads and keyed publishes, on the stand's own protocol and on NATS with JetStream, against the stand and, in CI, a real NATS server (packages J2 and J3) | Build what packages J2 and J3 name | Consume a message during discovery |
| [310](../3-packs/310-the-device-pack.md). The device pack | Designed, with rules and a catalog | Built against the simulated device: reads, writes, and firmware inspection and update (packages K2, K3, and K4) | Writes with the hazard `physical`, and the real device, with the owner | Run anything against real hardware, or publish a firmware update without a declared recovery route |
| [401](../4-building-tools/401-overlaying-tools-on-an-existing-system.md). Overlaying tools on an existing system | Designed as a method | Partly built, through the scout and the packs' discoverers and drafting rules | Follow it for any pack | Write beneath a system's own rules without its owner's word |
| [402](../4-building-tools/402-the-scout-and-who-does-what.md). The scout, and who does what | Designed, without each pack's drafting rules | Partly built: the scout's pipeline and report, from a discovery or from a person's capture (package E5), and drafted reads for the built packs (package E4); the comparison of described against observed (E3) and a budget for a whole scout are not | Build each remaining stage as the actions it is made from come to exist | Let the scout try a write, or draft a unit, key, or read-back rule the system did not state |
| [403](../4-building-tools/403-testing-and-publish-gates.md). Testing and publish gates | Designed | Partly built: two of the twelve gates, schema and lint and offline replay; the publish page shows the compiler's typed review, sign-ins first (package P1) | Build each gate as its package needs it | Prove a write through the product's own read path alone |
| [404](../4-building-tools/404-setup-and-ease-of-use.md). Setup and ease of use | Designed | Partly built: the project folder, the shortcut commands, and the first-hour flow without a hand step (packages 5.2 and 5.6); `connect` is built (`client.connect`, proven by `crates/a2m-actions/tests/build.rs`: the block is printed, and a client profile kept as data is written only by a person), and the ten-minute measure has not run | Build what packages 5.2 to 5.6 name | Add a step a person must take that the gates do not need |
| [501](../5-security/501-security-model.md). Security model | Designed | Partly built; the outside review is package 9.6 | Hold every change to it | Let a secret reach a caller |
| [502](../5-security/502-defending-against-a-manipulated-agent.md). Defending against a manipulated agent | Designed | Partly built: the manipulation tests of package 3.10, in the engine, the file and source readers, the MCP and REST doors, and through the product (input that is not declared, or reads like a path, an address, a header, or an expression); each defence is mapped to its test, or to a roadmap row, in the section's "Proving it" (package 9.3b) | Build each defence with a test that fails without it | Let an input change where a request goes or which file is read |
| [503](../5-security/503-approval-and-sign-in-surfaces.md). Approval and sign-in surfaces | Designed; the hosted sign-in page is not | Partly built: the approval page and the sign-in page on one machine, and the Linux presence prompt; the macOS and Windows prompts are written and not yet run by a person | Build what package 3.6 names | Give the caller any way to approve |
| [601](../6-running-it/601-deployment-configurations.md). Deployment configurations | Designed | Partly built: the `solo` profile (an engine started on demand and stopped when idle); the `workstation` profile's local HTTP is not built, because nothing reads `server.local_http`; the server profiles come after the first release ([decision 0169](../../decisions/0169-the-first-release-is-for-one-machine.md)) | Plan the profiles | Build a server feature before Phase 3 has passed |
| [602](../6-running-it/602-identity-on-a-shared-server.md). Identity on a shared server | Designed; research brief 17 comes first | Not built: it waits for the server | Build it in Phase 7 | Share one identity's session with another |
| [603](../6-running-it/603-networking.md). Networking | Designed | Not built for a server; on one machine the engine listens only on its own channel and the loopback page | Build it with the server | Open an inbound port on a runner |
| [604](../6-running-it/604-management-and-operations.md). Management and operations | Outlined; the admin console's screens are not designed | Partly built: some of its functions exist as actions; the console is package 6.5 | Expose each function as an action | Give the console a route of its own into the kernel |
| [605](../6-running-it/605-packaging-and-release.md). Packaging and release | Designed | Partly built: the wheel and the npm packages are built and checked in CI; nothing is published | Build what packages R1 and 5.5 name | Count a name as held before a release is published under it, or ship two copies of the binary |
| [701](701-prior-art.md). Prior art | A reference | Not applicable | Learn from what others did | Take code from it without checking its licence |
| [702](702-requirements-audit.md). Requirements audit | A record of the owner's requirements against the design | Not applicable | Check a change against it | Drop a requirement without the owner |
| [703](703-open-questions.md). Decisions and open questions | A list | Not applicable | Find what is still to be settled | Settle an owner's question alone |
| 704. Readiness | This section | | | |

## What rests on judgment alone

Every part not yet built, and every built part beyond its test stands: nothing has met a system the project does not run, and only the HTTP pack's reads have met existing software at all, in one copy test on 2026-11-23 that made seven findings ([copy test results](../../reviews/field-tests.md)). Passing CI on Windows and macOS shows a part works on a hosted runner against a stand, not on a person's machine with its own applications, accounts, and settings. The parts taken from REA have met code in REA, in a different language, for a different purpose, which is evidence that they can work and not that they work here. The research notes behind sections 205 to 226 are mostly search-result summaries. The components gathered in [section 604](../6-running-it/604-management-and-operations.md) rest on design judgment with no research behind them. [Research notes](../../research/README.md) list which sections have research behind them.

## What must be proven, and where

Every bet is in the [hypothesis register](../../hypotheses/README.md). These are the ones that could change the shape of the design, with the package that settles each. Several of those packages are done or partly built, and the register's notes say what each showed; a bet counts as proven or refuted only when its result file is written, and none is yet.

| Bet | Hypothesis | Proven or refuted by |
| --- | --- | --- |
| A Rust engine on the DevTools protocol is good enough | H-01 | Package 0.4 |
| The driver interface and `Control` are drawn right | H-35, H-36 | Package 1.1 |
| Compiling target files into a compiled target is worth it | H-14 | Package 1.3 |
| The trace signature is stable across runs and systems | H-16 | Package 1.2 |
| The speed budgets can be met | H-20 | Package 1.7 |
| One engine per user, found through a socket or pipe, is reliable | H-06 | Package 1.4 |
| The crate layering holds as code arrives | H-25 | Every package through milestone 3 |
| Tables with one owner each stay simple to use in one transaction | H-38 | Packages 3.1 to 3.7 |
| Sign-in in an engine-owned profile works on managed machines | H-05 | Research brief 30, then package 2.3 |
| A person can approve quickly and an agent with a shell cannot | H-12 | Research brief 27, then package 3.6 |
| The kernel is neutral | H-26 | Packages D3 and D4 |
| Seven access kinds are enough | H-29 | Each pack's design and build |
| Two interface packs share their formats through documents alone | H-37 | Package 8.3 |
| Drafts written by rule save work | H-31 | Package E5 |
| An outside discovery provider is worth its cost | H-27 | None: package E6 is withdrawn ([decision 0279](../../decisions/0279-what-the-three-surveys-left-open-against-decision-0276-is-settled.md)) |
| Reading back through a second surface catches more | H-30 | The test application, from Phase 3; the data pack's writes (package G3) |
| Shadow comparison catches logic a front end performs | H-10 | Package 4.3 |
| One binary with every first-party pack stays small | H-28 | Package F4 |
| A link or launcher named `a2m` starts within budget | H-39 | Package F4 |
| An agent with no prior knowledge reaches a first tool in ten minutes | H-18 | Package 5.6 |

## What is deliberately not designed yet

These are named in the specification and left for the package that builds them. A builder reaching one designs it in that package, records the choice, and does not improvise it inside other work.

| Part | When |
| --- | --- |
| How a plug-in registers schemas across a process boundary (the manifest itself is built) | Package F3 |
| The scenario file format | With the first discoverer, package E2 |
| Confinement of plug-in processes on each operating system | Before any plug-in may claim more than `declared` |
| The scheduler beyond its settings: a weighted share for a caller (round robin between callers is built) | Phase 6; one machine needs no more |
| The admin console's screens | Package 6.5; every admin function already exists as an action |
| The hosted sign-in page that shows a server's browser to a remote person | Package 7.3, after research brief 17; only one identity mode depends on it |
| Each widget driver and platform profile in detail | Package 8.4; a catalog exists, and each is built against the test application |
| The plug-in software kit | With package F3; the protocol is outlined and first-party packs come first |
| Runs started by an event. The reading half, a watch that reports a change and never acts on it, is designed ([decision 0234](../../decisions/0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md)) and built in part (packages W1 and W2a); starting a run because something changed is not designed | Phase 6 or later; until then any outside scheduler can start a run through the REST interface |
| Usage metering | Phase 9; the ledger already holds the data |
| High-availability details of the cluster profile | Package 9.4; the stateless front and outward-connecting runners already allow it |
| Packs beyond the nine designed | One at a time, each designed as its own section first and cleared against the entry bar |
| A browser extension that works inside the person's own profile | Not while rule five stands: it would drive the person's everyday profile. The owner confirmed this ([decision 0242](../../decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md)): the engine does not act in the person's everyday browser, by an extension or otherwise |
| A vision mode for interfaces that offer only pixels | Not planned; clicking by position cannot be verified. The owner decided that nothing in a run reads a picture ([decision 0242](../../decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md)) |

## Risks

Ordered by how much they could change the design.

| # | Risk | What limits it |
| --- | --- | --- |
| 1 | The Rust browser engine proves much harder than a dozen verbs suggests | The spike of package 0.4 measured the candidates, and the engine is built on a client of our own ([decision 0070](../../decisions/0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md)); it was not compared with a sidecar |
| 2 | The kernel boundary is drawn wrong, and every pack leaks into it | Packages D3 and D4 find out with one pack, as unlike the web as possible, before another is built |
| 3 | "Anything" dilutes "verified": packs arrive faster than they can be proven | The entry bar of [section 202](../2-kernel/202-kernel-and-packs.md), and the evidence ladder, which stops an unproven thing from being listed |
| 4 | An organization's sign-in rules refuse an automated browser, or bind sessions to hardware | Four identity modes; user-device runners; service accounts; research brief 30 before Phase 2 |
| 5 | A system's owner, or the organization's IT, does not agree to the tool being used | An authorization record is required before publishing on a server; the design operates openly. This cannot be engineered away |
| 6 | Scope grows past what one owner and a team of agents can finish | Milestone 3 is a complete product for one person, and nothing later is needed to ship it |
| 7 | On one machine, an agent with a shell is inside the user's trust boundary | No approve action; operating-system presence checks; protected settings; the limit is documented in [section 222](../2-kernel/222-front-doors-and-the-local-engine.md) |
| 8 | A runner that lost its lease writes late | Early stop, quarantine, read-back, later verification; the remaining case is documented in [section 223](../2-kernel/223-the-runner-protocol.md) |
| 9 | Mined request bindings miss logic the system's front end performs | Shadow comparison and promotion by a person; the interface binding stays |
| 10 | Speed budgets are unrealistic | They are targets; package 1.7 measures them and revises them by decision record |
| 11 | MCP clients differ from the specification and from each other | The spike of package 0.5 ran two unrelated clients; a client's quirk is to be data in a client profile, which is not built |
| 12 | The policy language does not fit the size or speed budget | Settled for now: the decision core is the policy crate's own evaluator ([decision 0082](../../decisions/0082-the-decision-core-is-the-policy-crates-own-evaluator.md)) |
| 13 | More sessions become bound to their device, so fewer can move between runners | Designed for: such sessions are marked not movable |
| 14 | The package names are taken before a release holds them | The owner's first publishes, below |
| 15 | Reverse engineering creates legal or reputational exposure | Off by default, at build time only, bound by rule five, and behind the authorization record |
| 16 | An outside discovery provider changes or disappears | It is optional, pinned to a version, reached only through a mapping file, and nothing is built on its code |
| 17 | Agents working in parallel collide | One package per branch, one crate per agent, and the lead merges |
| 18 | A decision rests on a research summary that turns out wrong | Every note states its status; a dependency is verified before it is depended on |

## What only the owner can do

None of these stops the build. [Publishing setup](../../plan/publishing-setup.md) has the detail for the first three.

- PyPI: a pending trusted publisher for `anything2mcp` that names the renamed repository, the workflow `release.yml`, and the environment `pypi`. The first release from the workflow then holds the name.
- npm: one first publish of `anything2mcp`, after which trusted publishing can be configured.
- Switch on private vulnerability reporting in the repository's settings.
- Confirm a Windows machine is available, or that CI is the only Windows environment.
- At the end of the build, for each real system to be tried: agreement from its owner before any use, and the owner's go-ahead in that session.

## Keeping it ready

- Build slowly and core first, by the [working method](../../plan/working-method.md) and the order in the roadmap.
- Test each bet as its package comes up, and record the result in the hypothesis register.
- Run a design review round at the end of each milestone, one [lens](../../plan/design-review-lenses.md) at a time.
- When code and a section disagree, change one of them in the same commit; when research is run, write the note, update the section, and remove the brief.
- Update this section when a package changes the state of a section.
