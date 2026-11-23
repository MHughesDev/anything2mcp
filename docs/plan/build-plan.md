# The build plan: what is left, at three levels

This is the one file to work from. It orders everything that is not yet built into eight stages, and describes each stage at three levels:

- **High level:** what a person can do when the stage is reached, and the one proof that it is.
- **Medium level:** the work packages of the stage, in order, each with what it builds and what proves it.
- **Low level:** the tasks inside each package, in the order to do them.

Written on 2026-11-23, after the first field test. The order is [decision 0296](../decisions/0296-the-build-is-ordered-by-what-the-next-field-test-needs.md).

**How this file relates to the roadmap.** The [roadmap](roadmap.md) is the record of every package: its crates, what it needs, its proof, and its state. This file is the order to take them in and the tasks inside them. A package's number here is its number there. When a package is finished, mark it done in the roadmap and strike it here in the same pull request. When the two disagree about a package's proof, the roadmap is right; when they disagree about order, this file is.

**How to use it.** Take the first package not struck through, in the first stage not reached. Read its crate designs. Follow the `build-slice` skill. A stage is reached when its proof passes and the journal says so.

## Where the build stands

- **Built and passing on three operating systems, against the test stands:** the kernel, ten packs, the local engine with its command line and bridge, the scout, and the flow from an address to a published read.
- **Met existing software:** only the HTTP pack's reads, once ([field test results](../reviews/field-tests.md)). One read of three worked. Seven findings.
- **Never done:** a write to existing software; a sign-in to existing software; a publish by a person on Windows or macOS; a release to a registry; anything against a system the project does not run.
- **The largest gaps for a user:** an application with no published description cannot be learned, because the capture of a person's use is not built; a read-only tool takes about ten commands; most existing MCP servers cannot be started, because they are run by an interpreter (decision 0282, undecided).

## The rules of the order

1. **A field test decides what is next.** Whatever stops the next field test from passing is built before anything else.
2. **A fault found on existing software is first made a case in a test stand,** then fixed, so that CI holds it on three operating systems.
3. **No new kind of system** until Stage 5. The ten packs are enough to be wrong in.
4. **One package at a time,** given to the smallest size of model that can do it well; the table under "Who does which line of work" says which ([decision 0294](../decisions/0294-the-owner-hands-the-open-questions-to-the-agents.md)).
5. **The ten rules, and what an agent does not do alone, are unchanged.** Publishing a release and touching a system the project does not run are the owner's.

## High level: the eight stages

| Stage | A person can then | The proof | Version |
| --- | --- | --- | --- |
| 1. Reads that work on existing software | Hand the scout a service's own description and get reads that run, with no hand editing | Field test 1 passes with no step marked `gap` | |
| 2. The short path | Go from an address to a published read tool in three commands, and publish it on any of the three systems | A person does it by hand on Linux, Windows, and macOS; the first tool in under ten minutes, measured | |
| 3. The web path | Use a web application while the product watches, and get drafted tools for what they did | A field test of a web application with a sign-in and no description ends with reads that run | 0.2 |
| 4. Verified writes on existing software | Approve a change and have it written once and proven, even when a connection is lost | A field test writes through a proxy that loses the answer; the software's own database holds the record once | |
| 5. Every kind | Do the same over each kind of system the product claims | One field test for each kind, with its findings closed or written down | |
| 6. The first release | Install with pip or npm and do all of the above | A tagged pre-release installs by each route on three systems and passes the first-hour test | 0.1 to 0.3 |
| 7. A server | Share one installation among several people, each acting as themselves | Two users share one server; a runner killed mid-write leaves no duplicate | 0.7 to 0.8 |
| 8. Breadth and hardening | Rely on it | Every named failure and attack has a test; systems the project does not run are tried with the owner; an outside review is closed | 1.0 |

Stages 1 to 4 are one after another. Stage 5's packages can be taken in any order once Stage 4 is reached. Stage 6 needs Stages 2 and 4. Stage 7 needs Stage 6. Stage 8 is last.

## Stage 1: reads that work on existing software

**High level.** Field test 1 stopped on two reads of three and needed two hand repairs. This stage removes every one of its `gap` steps.

**Medium level.**

| # | Package | What it builds | Proof |
| --- | --- | --- | --- |
| FT3 | An absent object | What a read does when the system answers `null` for a described object | The two reads of field test 1 return the stored values; a value of a kind the description does not allow still stops |
| FT2 | Findings 1, 2, 4, 5, 6 | Swagger 2.0 read; the scout's summary gives the code and fix; a description handed in as a file is tied to the surface; no doubled tool name, and one bad draft does not block the rest; the credential lint matches whole words | Each `gap` step fails and is rewritten to expect the right behaviour |
| 4.4 | Published descriptions, the rest | References to files outside the description, refused by name; the `api` mode | A described service becomes drafted reads that compile |
| E4 | The drafter, the rest | Drafting rules for the packs that lack them | A draft made by rule compiles and carries no unit or key the system did not state |
| 1.3 | The plan, the same on three systems | The compiled plan's bytes compared across systems in CI | The plan is byte-identical on three operating systems |

**Low level.**

FT3:
1. Write the decision record. Weigh: (a) an object the description gives and does not list as required may be `null` or missing; (b) the same, only when a setting on the target says so; (c) stop, and offer a one-command fix that marks the field. Rule 7 forbids guessing at an unknown kind; an absent optional value is not an unknown kind.
2. Add a case to the test application: an object field answered as `null`, as missing, and as a wrong kind.
3. Make the HTTP pack's shape check follow the record. A wrong kind still stops with `response_schema_mismatch`.
4. Say in the tool's output schema that the field may be absent.
5. Run field test 1; rewrite its two `gap` steps to expect the stored values, checked against the database.

FT2, one finding at a time, each with a stand case first:
1. Finding 1: read Swagger 2.0 by converting it in the reader to the shape the reader already holds: `definitions`, `basePath`, a response's `schema`, parameters typed in place. Fuzz the reader with the existing corpus plus Swagger files.
2. Finding 2: the scout's text summary prints, for a discoverer that failed, its code and its fix.
3. Finding 4: a description handed in under a surface's name, or to a target with one surface of the kind, drafts endpoints on that surface. With two candidate surfaces it stays an open question, and the summary counts it.
4. Finding 5a: the drafter's names are made unique by rule, and the rule is written in the HTTP pack's design.
5. Finding 5b: decide, with a record, whether a trial compiles only the drafts the tool needs. If yes, build it in `a2m-files`.
6. Finding 6: the credential lint matches a word, and a key of a response's field map is not a value.
7. Also fix: `draft.lint` answers `ok: false` when it lists a problem at the level of an error.

4.4: 1. Refuse a reference to a file outside the description, by name. 2. The `api` mode and its decision record. 3. A stand case for each.

E4: 1. List which packs draft by rule and which do not. 2. Add the missing rules, one pack per pull request.

1.3: 1. Have each CI system write the plan hash of the `example` target as an artifact. 2. A final job compares the three.

## Stage 2: the short path

**High level.** A person's work for a read-only tool drops to: add, sign in if needed, connect. Publishing a read no longer needs what a write needs.

**Medium level.**

| # | Package | What it builds | Proof |
| --- | --- | --- | --- |
| S1 (new) | The lighter publish for a read | A decision record, then the build: whether a read-only version may be published without the presence prompt, and what guards it instead | A read is published by one command in a container; a version holding a write is still refused there |
| S2 (new) | One command to a tool | `add` runs the scout, keeps the reads that passed their trial, records their offline tests, and requests the publish | From an empty folder: `init`, `add`, `connect`, and an agent calls a read |
| 3.6 | The presence prompt on Windows and macOS | The prompts, run by a person | A person approves on each system; a script cannot |
| 2.2 | The operating system's key store | The vault's key held by the system's store on each system | A saved value survives a restart and cannot be read without the key store |
| 5.2 | The shortcuts and `doctor` | What remains of the project folder, build state, and shortcuts | A new project goes from nothing to a linted draft with the shortcuts alone |
| 5.3 | The built-in documents | The guidance an agent reads, brought up to the short path | An agent with no prior knowledge finds every step from `docs.get` |
| 1.4, 1.5 | The local engine and the bridge, the rest | A shared browser between two commands; a different user refused; two outside MCP client libraries call a read | The roadmap's proofs for both |

**Low level.**

S1: 1. Read sections 403 and 503 and decisions 0252 and 0264. 2. Write the record: a version whose every tool is a read with no declared destination outside its surface may be published by the local user without the presence prompt; anything else keeps it. Name what a manipulated agent could gain and why that is acceptable or not. 3. If the record cannot make that case, keep the prompt and build only S2. 4. Build, with a manipulation test that tries to pass a write as a read.

S2: 1. Write the rule for which drafted reads are kept: passed a trial, compiled, no open question. 2. `add` does target add, scout, keep, fixture record, test, promote, publish request, and prints what it kept and what it left with the reason. 3. Each step stays a separate action; the shortcut only chains them. 4. A test from an empty folder in `crates/anything2mcp/tests/`. 5. Measure the time and write it in the journal.

3.6: 1. Write the two-minute steps for a person. 2. The owner, or anyone at such a machine, runs them; the result goes in the journal. 3. Fix what they show. This is the one package of the stage that needs a person at a machine.

2.2: 1. One system at a time: Linux secret service, macOS keychain, Windows credential store. 2. A CI test where the runner offers the store; a named stop where it does not.

5.2, 5.3, 1.4, 1.5: take the "not shown" items from each roadmap row, one test each.

## Stage 3: the web path

**High level.** The product's main case: a web application with a sign-in and no description. A person uses it while the product watches, and tools are drafted from what was seen.

**Medium level.**

| # | Package | What it builds | Proof |
| --- | --- | --- | --- |
| 1.6 | Reading a page | The rest of the read verbs, locators, wait conditions, and widget drivers for reading | The Phase 1 cases of the test application pass on three systems |
| 2.3 | The sign-in window | A visible window of the engine's own browser where the person signs in | A person signs in to the test application once; a read then works |
| 2.4, 2.5, 2.6 | Sessions | Expiry in `page_http`; the keeper told when a sign-in is the person's own; single sign-on | The roadmap's proofs for each |
| E2 | Capture and inference | `screen.snapshot`, `capture.start`, `capture.stop`, `endpoints.infer` | The test application's cases for capture and inference |
| E3 | Described against observed | The comparison, and the unknowns it raises | An endpoint described and never seen is reported as `inferred` |
| E5 | The scout, the rest | Capture's findings flow into the scout's report | Scouting twice gives the same report, by hash |
| 4.2 | Requests from inside the page | The `page_http` driver, with the page's own tokens | The anti-forgery case passes |
| P3b | Destinations | Refused origins carried in findings and proposed as destinations | The roadmap's proof |
| FT6 (new) | The web field test | A recipe for a web application with a sign-in | Reads drafted from a capture run and match the database |
| 2.7 | A design review round | One lens over the web path | Findings fixed or recorded |

**Low level.**

1. 1.6: list the test application's Phase 1 cases that fail; one widget driver per pull request.
2. 2.3: the window opens on the engine's own profile (rule 5); the credential never passes through an action's input.
3. E2, in four slices: `screen.snapshot` as an action; `capture.start` and `capture.stop`, started only by the person whose use it records, with secrets removed as it records; `endpoints.infer` from stored captures; the mapping from what was typed to request fields.
4. E3 and E5: the comparison in `a2m-provider`, then the scout's stage for it.
5. 2.4 to 2.6: one sign-in variant of the test application per pull request.
6. 4.2: measure first. The first measurement did not show it faster than a screen read; keep it only if it is, or if a case needs it.
7. P3b: the first slice named in the journal of 2026-11-18.
8. FT6: the recipe signs in through the sign-in window driven by the recipe as a person would be, captures three screens, and checks the drafted reads against the database.
9. The answer lost on macOS without TLS, noted on 2026-11-18, is investigated here, because the web path meets it first.

## Stage 4: verified writes on existing software

**High level.** The product's central claim, shown on software the project did not write: a change is recorded, sent once, read back, and reconciled after a fault.

**Medium level.**

| # | Package | What it builds | Proof |
| --- | --- | --- | --- |
| FT4 | Faults on purpose | A proxy between the product and the software that drops, delays, or cuts a connection at a named point | A write whose answer is lost is reconciled; the database holds it once |
| 3.3, 3.4, 3.5 | The write, the rest | Field types and normalization on real values; the diff's hash the same on three systems | The roadmap's proofs |
| 3.7 | Stop and resume | A kill at a random step | Killed and resumed: no duplicate, nothing unverified |
| 4.1 | The `http` driver, the rest | The cases still open | The test application's cases pass over direct requests |
| 2.1 | The redactor | A secret planted in the system's own answer | It appears in no result, log, or trace |
| 3.10 | Faults and manipulation | The other kernel crates; a design review round | Each fault leaves no duplicate and no unverified record |
| 9.1 | Canaries and drift | A changed system is caught before a write fails | A deliberately changed screen is caught by a canary |
| 4.5 | The exit test of Phase 4 | A request binding gained through shadow | Identical results, at least ten times faster |

**Low level.**

FT4:
1. `tools/field_proxy.py`: a TCP proxy with rules a recipe gives: pass, delay, close before the request, close after the request and before the answer, close mid-answer.
2. A recipe step kind that sets a rule for the next request matching a method and path.
3. The first write: create one record in field test 1's system through an operation written by hand, with its identity and read-back rule.
4. Approval in a field test: the runner stands in for the person as the first-hour test does, and the recipe says so.
5. Steps: preview, approve, write, read back, database check. Then the same with the answer lost; then with the connection cut before the request; then a kill of the engine between intent and act.
6. Each result is checked against the database: exactly one record, or none, never two.

3.7: a test that kills the engine at a step chosen by a seeded random number, a hundred seeds in CI.

The others: take each roadmap row's "not shown" items, one test each.

## Stage 5: every kind

**High level.** Each kind of system the product names is met once on existing software, and what that shows is fixed or written down. New kinds may be added again after this stage.

**Medium level.**

| # | Package | What it builds | Proof |
| --- | --- | --- | --- |
| 0282 | The interpreter decision | Whether, and how, a path may start an interpreter | A decided record; then an MCP server run by an interpreter starts, or is refused with a reason a person can act on |
| FT5 | A field test for each kind | Recipes: an existing MCP server; a command-line program; a database; files; a broker; a desktop application; a device's protocol | One recipe each, findings written down |
| D3, D4, D5 | The command pack | The rest of reads; the result file of H-26; writes with a kill and resume | The roadmap's proofs |
| G2, G3 | The data pack | Reads and writes, the rest | The roadmap's proofs |
| H2, H3 | The file pack | Reads and writes, the rest | The roadmap's proofs |
| J2, J3 | The stream pack | Reads and publishing, the rest | The roadmap's proofs |
| 8.3 | The desktop pack | Writes on Windows and macOS | Case 28 passes on three systems |
| P2 | What a tool starts, sets, and sends | The statement for the desktop, device, stream, and data packs | Each is shown on the publish page |
| W2d | Watches, the rest | Records compared by key; a removal reported | The roadmap's proof |
| N2b, N3 | The network | The place of an address; the reach a person names | The roadmap's proofs |
| X1, X5 | What a program says of itself | The D-Bus reader; the plain facts of a binary | The roadmap's proofs |

**Low level.**

0282: 1. Read the Proposed record and section 213. 2. Decide under decision 0294. The case to weigh: most existing MCP servers are started through an interpreter and a package runner, so refusing them leaves the MCP pack with little to wrap; allowing them means the product starts code it cannot identify by one digest. A middle course is a declared interpreter with a pinned package and digest, as the command pack already does for a script. 3. Build what the record says, with the fault tests of the command pack.

FT5, in this order, each a recipe and a findings table:
1. A database server with invented rows: reads, then a write, checked by a direct query.
2. A broker: a bounded read that takes nothing; a keyed publish, checked with the broker's own client.
3. A command-line program: reads through declared commands; a write with a read-back command.
4. Files that another program wrote: a settings file, a growing log, a workbook.
5. An existing MCP server pointed at field test 1's system: its tools wrapped, a write verified against the database.
6. A device's protocol, against existing controller software.
7. A desktop application, on Linux in a virtual display; Windows and macOS need a person's machine and are written as such.

The packs' own packages: after each field test, close what it found, a stand case first.

## Stage 6: the first release

**High level.** Someone other than the project installs it and does the above.

**Medium level.**

| # | Package | What it builds | Proof |
| --- | --- | --- | --- |
| 1.7 | Budgets measured | Every budget of section 225 measured; `a2m-bench compare` run in CI | Numbers recorded; budgets confirmed or revised by record |
| F3, F4 | Provider packages; sizes | Packs found through a Python entry point; the binary and wheel measured | The roadmap's proofs |
| 5.4 | The clients | The Python and TypeScript clients on Windows | Each client calls a read |
| 5.5 | The release pipeline | pip, crates.io, npm, standalone binaries, signed, with a bill of materials | A tagged pre-release installs by each route on three systems |
| 5.6 | The exit test of Phase 5 | An agent takes a new system to published reads | The first tool in under ten minutes, measured |
| R1 | Release 0.1 | The changelog, the notices file, a wheel that carries the binary | An install from the registry runs a read and a write against the test application |
| 0.2p | The package names | Held on the registries | The owner's |

**Low level.** 1. 1.7 first: a release with unmeasured budgets promises what nobody checked. 2. 5.5 is prepared and rehearsed with the workflow's dry run only. 3. **Starting the release workflow, a version tag, and anything with a registry token are the owner's.** The lead prepares the pull request, writes the steps, and says in the journal that the release waits for the owner. 4. Everything else in the stage is the lead's.

## Stage 7: a server

**High level.** Several people share one installation. Each acts as themselves; nobody acts as another.

**Medium level.**

| # | Package | What it builds | Proof |
| --- | --- | --- | --- |
| 6.1 | Postgres beside SQLite | One schema, both back ends | Every ledger test passes on both |
| 6.3 | Toolsets, grants, roles | The lower layers that only narrow; a second administrator on publish | Two users see different tools |
| 6.2 | MCP over HTTP | An OAuth resource server | A token for another audience is refused |
| 6.4 | The REST interface | Its description, events, webhooks | A program follows a pipeline to its end |
| 6.5 | Approval and sign-in pages on a server; the console | | An approver approves; the caller cannot |
| 7.1, 7.2, 7.3 | Runners | The link; enrollment and lost leases; identity modes | A runner killed mid-write leaves no duplicate |
| 9.2, 9.4 | Audit chain, backup, upgrades; the cluster profile | | A broken chain is detected; a lost node loses no run |
| 6.6 | The exit test of Phase 6 | | Two users share one server |

**Low level.** 1. Before the stage, a design review round on sections 214, 222, and 223: they were written before anything ran, and Stages 1 to 5 will have changed what they rest on. 2. Run the research brief that 7.3 names. 3. 6.1, then 6.3, then the rest in the table's order. 4. Tasks inside each package are written when the review round ends, in this file.

## Stage 8: breadth and hardening

**High level.** The hard cases, the tests that the defences hold, and the outside world.

**Medium level.**

| # | Package | What it builds | Proof |
| --- | --- | --- | --- |
| 8.4, 8.5 | Widget drivers and edge cases of web interfaces | | Every control and every hard case of the test application passes |
| 8.2a | Tools a site publishes | A mode of the web pack | A page registers a tool and it is called |
| K4 | Firmware | Inspection as discovery; the update operation | An update without a recovery path cannot be published |
| X2, X3, X6, X7 | More that a program says of itself | The macOS and Windows readers; the optional engine; the spike | The roadmap's proofs |
| 9.3 | Every failure and attack as a test | | Each test fails when its defence is removed |
| 9.5 | Systems the project does not run | A read, then a write to a declared test record | The owner's, with the owner present |
| 9.6 | An outside security review | | The owner's; a test for every finding |
| R2 | Release 1.0 | | The owner's |

**Low level.** Written when Stage 7 is reached. Until then the roadmap rows are the detail.

## Who does which line of work

Four sizes of model work on this build. The lead is the largest and the dearest, and the owner's usage is limited, so work goes to the smallest size that can do it well. The sizes are named here by their place, not by a product's name.

| Size | Its lines of work | It does not |
| --- | --- | --- |
| **The lead** (the largest) | Every decision and its record. This plan and the roadmap. The brief for every other agent, with a stated proof. Reading every diff before it merges, and merging. Reading what a field test shows and deciding what it means. The three open decisions by name: an absent object (FT3), the lighter publish (S1), the interpreter (0282). Anything that touches the ten rules, the provider contract, the ledger's nine rules, or the policy engine's verdicts | Write code another size can write from a brief |
| **The large builder** | A package that changes a kernel crate, or a write path, or crosses several crates: the engine, the ledger, policy, sign-in and sessions, stop and resume (3.7), capture and inference (E2), the first write of a field test (FT4), the lighter publish once decided (S1). A design review round. A second reading of a diff that touches security, before the lead's | Decide between designs; it returns the alternatives it saw and the lead decides |
| **The middle builder** | A package inside one pack or one tool, with a stand case and a proof: the Swagger reader, drafted names, the credential lint, and the scout's summary (FT2); a widget driver; one pack's drafting rules; a sign-in variant; a test stand's new case; the fault proxy (`tools/`); a field test's recipe; finding the cause of a red check | Change a public interface or a failure code's class without a record from the lead |
| **The small worker** | Work with one right answer that a check confirms: counts and index rows after a new document; a rename across files; running the audit, the lints, or a field test and reporting what they print; searching the repository; reading a CI log for the first failure; seeding invented data; striking a finished package here and in the roadmap | Write a decision, a design, or product code |

**Rules for handing work down.**

1. A brief names the package, the files, the proof, and what not to touch. An agent that cannot meet the proof stops and says why; it does not widen its task.
2. Two agents never change the same crate at the same time.
3. Nothing merges on an agent's word. The lead reads the diff and CI holds the gate.
4. When a smaller size fails a package twice, the package goes up one size, and the journal says so.
5. The large builder is used sparingly: one at a time, and only for the lines in its row.

## What is the owner's, and nothing else is

- Package 0.2p, the package names.
- Starting a release: a version tag or the release workflow.
- A person at a Windows and a macOS machine for package 3.6, about two minutes each.
- Packages 9.5, 9.6, and R2.
- A change to one of the ten rules.

Everything else in this file the lead decides, builds, and merges, and writes down.

## Keeping this file true

- Strike a package through when it is done, with the pull request's number.
- A stage's low level is rewritten when the stage before it is reached, from what was learned.
- A new package gets a row here and in the roadmap in the same change.
- A field test's finding that needs work becomes a package in the stage it blocks.
