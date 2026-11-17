# 704. Readiness

How much of this specification a builder can work from today, what exists and is proven, what rests on judgment alone, where each bet will be proven, what is left undesigned on purpose, the risks, and what only the owner can do. The order of work is the [roadmap](../../plan/roadmap.md); this section says how firm the ground is under each package.

"Ready" here means the design is complete, consistent, and specific enough that a builder does not have to invent architecture. It does not mean the design is proven. Most of the research behind it comes from search-result summaries, and every speed figure is a target. The spikes and the first measurements are where the design first meets reality, and it should be expected to change there.

## What exists and is proven

| Exists | Proven by |
| --- | --- |
| A Cargo workspace of 24 crates, the kernel, the packs, and the product, in enforced layers | The audit's layering rules, each with a planted mistake in its self-test; CI on Windows, macOS, and Linux |
| `a2m-core`'s shared vocabulary: archetypes, effect classes, hazards, failure classes, evidence levels, and the states of runs, records, sessions, and bindings, with a secret type that cannot be printed or serialized (package B1) | A test that holds the names to the documents; a test that a secret cannot be printed |
| The `anything2mcp` command, and `a2m` for the same program, which report the version and nothing else | Its own tests |
| The test application, with a web interface, a JSON interface, a control interface that reports ground truth, and injectable faults, for cases 1 to 4 (package 0.3) | A test resets it, changes a record, and asks it what it stored |
| Continuous checks: formatting, lints, build, tests, a closed list of dependency licences, and the documentation audit with its self-test | Green on every pull request, on three operating systems |
| The release workflow and a pip placeholder at 0.0.1, with a manual start that can rehearse a release (packages 0.2 and F2) | The build job is green; the package installs from the built wheel. Nothing is published yet |
| The design: this specification; the [architecture](../../architecture/README.md), with a design for each of the 24 crates; the decision records; the research notes; a register of 39 hypotheses; the roadmap | The audit, which keeps links, names, counts, and the layering consistent, and requires a design for every crate |

Nothing of the engine is built. No hypothesis has been tested; all 39 in the [register](../../hypotheses/README.md) are open.

## The state of each section

| Section | State | A builder can | A builder must not |
| --- | --- | --- | --- |
| [101](../1-product/101-summary.md). Summary | A statement of scope | Use it to explain the product | Read it as a list of what is built |
| [102](../1-product/102-requirements-and-guarantees.md). Requirements and guarantees | Designed | Hold every change to the guarantees | Weaken a guarantee without the owner |
| [103](../1-product/103-design-principles.md). Design principles | Designed | Settle a conflict by the stated order: safe, correct, fast, easy | Treat a principle as a setting |
| [104](../1-product/104-system-overview.md). System overview | An overview | Find where a part lives | Build from it alone; the architecture and the kernel sections are exact |
| [105](../1-product/105-core-concepts.md). Core concepts | Designed | Use the words as the glossary defines them | Introduce a synonym |
| [106](../1-product/106-what-it-latches-onto.md). What it latches onto | A statement of scope and a catalog | Decide whether something belongs; pick the next pack | Treat a kind of system as supported because it is named, or start a pack from a row in a table |
| [201](../2-kernel/201-archetypes-and-modes.md). Archetypes and modes | Designed in outline | Classify a new mode; see what its driver must supply | Build a mode whose steps are not yet fixed without fixing them first |
| [202](../2-kernel/202-kernel-and-packs.md). Kernel and packs | Designed | Draw the crate boundary and enforce it; apply the entry bar | Let a pack reach the kernel outside its registrations |
| [203](../2-kernel/203-the-provider-contract.md). The provider contract | Designed; the manifest is a TOML file whose schema is written in package B3 | Write the manifest's schema from the field list | Add an extension point without a decision record |
| [204](../2-kernel/204-supervising-subsystems.md). Supervising subsystems | Designed as rules | Build the supervisor against the rules | Promise confinement of plug-ins before the research is done |
| [205](../2-kernel/205-target-files.md). Target files | Designed, with the [file formats](../../reference/file-formats.md) | Write the schemas and the loader (package B4) | Add a field the formats do not define |
| [206](../2-kernel/206-the-compiler-and-the-plan.md). The compiler and the plan | Designed | Build it in package 1.3 | Let the engine depend on the compiler |
| [207](../2-kernel/207-the-engine-loop.md). The engine loop | Designed | Build it from package 1.1 | Let the engine name a mode, a driver, or a pack |
| [208](../2-kernel/208-runs-the-ledger-and-resume.md). Runs, the ledger, and resume | Designed, with open points | Build the nine rules as tests first (package 3.1) | Merge a change that breaks a rule |
| [209](../2-kernel/209-record-identity-and-dependent-records.md). Record identity and dependent records | Designed | Build it in package 3.8 | Resume a create with no identity strategy |
| [210](../2-kernel/210-sources-and-value-normalization.md). Sources and value normalization | Designed | Build it in packages 3.4 and 3.9 | Compare a business value as a float |
| [211](../2-kernel/211-concurrency.md). Concurrency | Designed; fairness between callers under load is not | Build queues, locks, and limits | Invent a fairness scheme; it is designed with Phase 6 |
| [212](../2-kernel/212-policy-and-approvals.md). Policy and approvals | Designed; the policy language waits on research brief 31 | Build the gate with the built-in local policy set (package 3.2) | Give any part but the policy engine a decision |
| [213](../2-kernel/213-effects-hazards-and-consent.md). Effects, hazards, and consent | Designed | Add hazards to operation files and policy | Claim `confined` anywhere yet |
| [214](../2-kernel/214-access-control-toolsets-and-tenancy.md). Access control, toolsets, and tenancy | Designed | Build it with the server (package 6.3) | Let any role run as another person |
| [215](../2-kernel/215-sessions-and-the-session-keeper.md). Sessions and the session keeper | Designed; research briefs 4 and 30 come first | Build it in Phase 2 | Renew anywhere but the keeper |
| [216](../2-kernel/216-the-vault-and-its-keys.md). The vault and its keys | Designed; research brief 5 comes first | Build it in package 2.2 | Keep the root key anywhere but the operating system's key store or a key service |
| [217](../2-kernel/217-traces-evidence-and-repair.md). Traces, evidence, and repair | Designed; repair is outlined | Build the trace and its signature (package 1.2) | Store evidence under a plain content hash |
| [218](../2-kernel/218-discovery-and-findings.md). Discovery and findings | Designed, without its closed lists | Build the findings store and the comparison | Let anything raise a finding above `discovered` |
| [219](../2-kernel/219-the-evidence-ladder.md). The evidence ladder | Designed | Tie the level to the compiler and the gates | Add a field that sets a level by hand |
| [220](../2-kernel/220-actions-and-the-authoring-interface.md). Actions and the authoring interface | Designed | Add each action to the registry as its package needs it | Hand-write a face, or add an action that approves |
| [221](../2-kernel/221-the-tool-surface-agents-see.md). The tool surface agents see | Designed | Build it with MCP over stdio (package 1.5) | Make a tool's result depend on which client called |
| [222](../2-kernel/222-front-doors-and-the-local-engine.md). Front doors and the local engine | Designed; research brief 26 comes first | Build the local engine in package 1.4 | Put engine logic in a client |
| [223](../2-kernel/223-the-runner-protocol.md). The runner protocol | Designed | Build it in Phase 7 | Let a runner that lost its lease write without reconciling |
| [224](../2-kernel/224-when-the-engine-itself-fails.md). When the engine itself fails | Designed | Build each failure as a fault test | Write while the engine is degraded |
| [225](../2-kernel/225-performance.md). Performance | Budgets set; none measured | Measure against them (package 1.7) | Treat a budget as a fact before it is measured |
| [226](../2-kernel/226-settings.md). Settings | Designed; key names are drafts until the loader is written | Build the loader | Add a setting that weakens a guardrail |
| [301](../3-packs/301-the-web-pack.md). The web pack | Designed; the interface flavors and hard cases are a catalog, each built in Phase 8 | Build its interface mode from package 1.1 | Build a widget or platform rule without a test application case |
| [302](../3-packs/302-the-web-pack-browser-engine.md). The web pack: the browser engine | Designed; its base waits on package 0.4 | Run the spike | Drive the user's own browser profile |
| [303](../3-packs/303-the-http-pack.md). The HTTP pack | Designed in its crate design | Build it in Phase 4 | Write over a mined binding before it has passed shadow |
| [304](../3-packs/304-the-mcp-pack.md). The MCP pack | Designed in its crate design | Build it in package 8.2 | Reach an upstream tool its mapping file does not name |
| [305](../3-packs/305-the-desktop-pack.md). The desktop pack | Designed in outline; its base waits on package 8.1 | Run the spike | Click by position on a picture of the screen |
| [306](../3-packs/306-the-command-pack.md). The command pack | Designed, not built | Build its test stand (package D2), then its reads | Start a shell |
| [307](../3-packs/307-the-data-pack.md). The data pack | Designed, not built | Build its test stand (package G1) | Build the pack before package D4 has reported |
| [308](../3-packs/308-the-file-pack.md). The file pack | Designed, not built | Build its test stand (package H1) | Write a format that has not passed both of its round-trip tests |
| [309](../3-packs/309-the-stream-pack.md). The stream pack | Designed, not built | Build its test stand (package J1) | Consume a message during discovery |
| [310](../3-packs/310-the-device-pack.md). The device pack | Designed, with rules and a catalog; not built | Build the simulated device (package K1) | Run anything against real hardware, or publish a firmware update without a declared recovery route |
| [401](../4-building-tools/401-overlaying-tools-on-an-existing-system.md). Overlaying tools on an existing system | Designed as a method | Follow it for the web pack now | Write beneath a system's own rules without its owner's word |
| [402](../4-building-tools/402-the-scout-and-who-does-what.md). The scout, and who does what | Designed, without each pack's drafting rules | Build the pipeline's stages as the actions they are made from come to exist | Let the scout try a write, or draft a unit, key, or read-back rule the system did not state |
| [403](../4-building-tools/403-testing-and-publish-gates.md). Testing and publish gates | Designed | Build each gate as its package needs it | Prove a write through the product's own read path alone |
| [404](../4-building-tools/404-setup-and-ease-of-use.md). Setup and ease of use | Designed; research briefs 10 and 29 come first | Build it in Phase 5 | Add a step a person must take that the gates do not need |
| [501](../5-security/501-security-model.md). Security model | Designed | Hold every change to it | Let a secret reach a caller |
| [502](../5-security/502-defending-against-a-manipulated-agent.md). Defending against a manipulated agent | Designed | Build each defence with a test that fails without it | Let an input change where a request goes or which file is read |
| [503](../5-security/503-approval-and-sign-in-surfaces.md). Approval and sign-in surfaces | Designed; the presence prompt per platform and the hosted sign-in page are not | Build the diff page (package 3.6) | Give the caller any way to approve |
| [601](../6-running-it/601-deployment-configurations.md). Deployment configurations | Designed | Plan the profiles | Build a server feature before Phase 3 has passed |
| [602](../6-running-it/602-identity-on-a-shared-server.md). Identity on a shared server | Designed; research brief 17 comes first | Build it in Phase 7 | Share one identity's session with another |
| [603](../6-running-it/603-networking.md). Networking | Designed | Build it with the server | Open an inbound port on a runner |
| [604](../6-running-it/604-management-and-operations.md). Management and operations | Outlined; the admin console's screens are not designed | Expose each function as an action | Give the console a route of its own into the kernel |
| [605](../6-running-it/605-packaging-and-release.md). Packaging and release | Designed in outline; research brief 28 comes first | Plan the wheel (package F1) | Count a name as held before a release is published under it, or ship two copies of the binary |
| [701](701-prior-art.md). Prior art | A reference | Learn from what others did | Take code from it without checking its licence |
| [702](702-requirements-audit.md). Requirements audit | A record of the owner's requirements against the design | Check a change against it | Drop a requirement without the owner |
| [703](703-open-questions.md). Decisions and open questions | A list | Find what is still to be settled | Settle an owner's question alone |
| 704. Readiness | This section | | |

## What rests on judgment alone

Every section beyond the parts already built. None has met engine code. The parts taken from REA have met code in REA, in a different language, for a different purpose, which is evidence that they can work and not that they work here. The research notes behind sections 205 to 226 are mostly search-result summaries. The components gathered in [section 604](../6-running-it/604-management-and-operations.md) rest on design judgment with no research behind them. [Research notes](../../research/README.md) list which sections have research behind them.

## What must be proven, and where

Every bet is in the [hypothesis register](../../hypotheses/README.md). These are the ones that could change the shape of the design, with the package that settles each.

| Bet | Hypothesis | Proven or refuted by |
| --- | --- | --- |
| A Rust engine on the DevTools protocol is good enough | H-01 | Package 0.4 |
| The driver interface and `Control` are drawn right | H-35, H-36 | Package 1.1 |
| Compiling target files into a plan is worth it | H-14 | Package 1.3 |
| The trace signature is stable across runs and systems | H-16 | Package 1.2 |
| The speed budgets can be met | H-20 | Package 1.7 |
| One engine per user, found through a socket or pipe, is reliable | H-06 | Package 1.4 |
| The crate layering holds as code arrives | H-25 | Every package through milestone 3 |
| Tables with one owner each stay simple to use in one transaction | H-38 | Packages 3.1 to 3.7 |
| Sign-in in an engine-owned profile works on managed machines | H-05 | Research brief 30, then package 2.3 |
| A person can approve quickly and an agent with a shell cannot | H-12 | Research brief 27, then package 3.6 |
| The kernel is neutral | H-26 | Packages D3 and D4 |
| Seven archetypes are enough | H-29 | Each pack's design and build |
| Two interface packs share their formats through documents alone | H-37 | Package 8.3 |
| Drafts written by rule save work | H-31 | Package E5 |
| An outside discovery provider is worth its cost | H-27 | Package E6 |
| Reading back through a second surface catches more | H-30 | The test application, from Phase 3; the data pack's writes (package G3) |
| Shadow comparison catches logic a front end performs | H-10 | Package 4.3 |
| One binary with every first-party pack stays small | H-28 | Package F4 |
| A link or launcher named `a2m` starts within budget | H-39 | Package F4 |
| An agent with no prior knowledge reaches a first tool in ten minutes | H-18 | Package 5.6 |

## What is deliberately not designed yet

These are named in the specification and left for the package that builds them. A builder reaching one designs it in that package, records the choice, and does not improvise it inside other work.

| Part | When |
| --- | --- |
| The manifest's schema, and how a plug-in registers schemas across a process boundary | Package B3 |
| The closed lists of findings: kinds, limitation codes, reasons for an unknown subject | Package E1, with the first discoverer |
| The scenario file format | With the first discoverer, package E2 |
| Each pack's drafting rules | With each pack, package E4 for the first |
| Confinement of plug-in processes on each operating system | Before any plug-in may claim more than `declared` |
| Operating-system user-presence prompts, per platform | Package 3.6, after research brief 27; a dialog-only fallback exists |
| The scheduler beyond its settings: fairness between callers, and priorities under load | Phase 6; one machine needs no more |
| The admin console's screens | Package 6.5; every admin function already exists as an action |
| The hosted sign-in page that shows a server's browser to a remote person | Package 7.3, after research brief 17; only one identity mode depends on it |
| Each widget driver and platform profile in detail | Package 8.4; a catalog exists, and each is built against the test application |
| The desktop pack's internals | After the spike, package 8.1 |
| The plug-in software kit | With package F3; the protocol is outlined and first-party packs come first |
| Triggers, and runs started by an event | Phase 6 or later; until then any outside scheduler can start a run through the REST interface |
| Usage metering | Phase 9; the ledger already holds the data |
| High-availability details of the cluster profile | Package 9.4; the stateless front and outward-connecting runners already allow it |
| Packs beyond the nine designed | One at a time, each designed as its own section first and cleared against the entry bar |
| A browser extension that works inside the person's own profile | Later, if ever; a possible answer to systems that allow one session, and not committed |
| A vision mode for interfaces that offer only pixels | Not planned; clicking by position cannot be verified |

## Risks

Ordered by how much they could change the design.

| # | Risk | What limits it |
| --- | --- | --- |
| 1 | The Rust browser engine proves much harder than a dozen verbs suggests | A spike with measurements (package 0.4); a Playwright sidecar behind the same driver interface as the fallback |
| 2 | The kernel boundary is drawn wrong, and every pack leaks into it | Packages D3 and D4 find out with one pack, as unlike the web as possible, before another is built |
| 3 | "Anything" dilutes "verified": packs arrive faster than they can be proven | The entry bar of [section 202](../2-kernel/202-kernel-and-packs.md), and the evidence ladder, which stops an unproven thing from being listed |
| 4 | An organization's sign-in rules refuse an automated browser, or bind sessions to hardware | Four identity modes; user-device runners; service accounts; research brief 30 before Phase 2 |
| 5 | A system's owner, or the organization's IT, does not agree to the tool being used | An authorization record is required before publishing on a server; the design operates openly. This cannot be engineered away |
| 6 | Scope grows past what one owner and a team of agents can finish | Milestone 3 is a complete product for one person, and nothing later is needed to ship it |
| 7 | On one machine, an agent with a shell is inside the user's trust boundary | No approve action; operating-system presence checks; protected settings; the limit is documented in [section 222](../2-kernel/222-front-doors-and-the-local-engine.md) |
| 8 | A runner that lost its lease writes late | Early stop, quarantine, read-back, later verification; the remaining case is documented in [section 223](../2-kernel/223-the-runner-protocol.md) |
| 9 | Mined request bindings miss logic the system's front end performs | Shadow comparison and promotion by a person; the interface binding stays |
| 10 | Speed budgets are unrealistic | They are targets; package 1.7 measures them and revises them by decision record |
| 11 | MCP clients differ from the specification and from each other | A spike against two unrelated clients (package 0.5); a compatibility table kept as data |
| 12 | The policy language does not fit the size or speed budget | Research brief 31 before Phase 3; an in-house matcher with the same meaning as the fallback |
| 13 | More sessions become bound to their device, so fewer can move between runners | Designed for: such sessions are marked not movable |
| 14 | The package names are taken before a release holds them | The owner's first publishes, below |
| 15 | Reverse engineering creates legal or reputational exposure | Off by default, at build time only, bound by rule five, and behind the authorization record |
| 16 | An outside discovery provider changes or disappears | It is optional, pinned to a version, reached only through a mapping file, and nothing is built on its code |
| 17 | Agents working in parallel collide | One package per branch, one crate per agent, and the lead merges |
| 18 | A decision rests on a research summary that turns out wrong | Every note states its status; a dependency is verified before it is depended on |

## What only the owner can do

None of these stops the build. [Publishing setup](../../plan/publishing-setup.md) has the detail for the first three.

- PyPI: a pending trusted publisher for `anything2mcp` that names the renamed repository, the workflow `release.yml`, and the environment `pypi`. The first release from the workflow then holds the name.
- crates.io: one first publish of `anything2mcp`, after which trusted publishing can be configured.
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
