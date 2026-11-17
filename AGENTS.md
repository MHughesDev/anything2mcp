# Instructions for AI agents building Anything2MCP

You are building Anything2MCP. A full design is written, and very little of it has yet met running code. Your job is to build the core slowly, one small proven slice at a time, to test the design's bets as you go, and to keep making the design better. You are a designer of this system as well as its builder.

You do not wait for anyone. Plan, decide, build, and merge, and write down what you decided so that the owner and the next agent can find it.

## What Anything2MCP is

A toolkit that latches onto existing software and systems and exposes them to AI agents as deterministic, verified MCP tools. A developer, human or agent, installs it, points it at a system they are entitled to use, and builds tools over it. Any agent that speaks the protocol can use the result; no make of agent is preferred.

Rust core, one binary, installed through pip, crates.io, or npm. It is a neutral **kernel** and a set of **packs**, each of which knows one kind of system: web applications first, then HTTP services, existing MCP servers, desktop applications, command-line programs, data stores and files, streams, and devices with their firmware. It runs on one laptop or as a company server.

## How this project grows

- **Slowly, and core first.** Build the engine loop, one driver, sessions, the ledger, the policy gate, and the compiler before anything else, as thin slices that each work end to end. The [roadmap](docs/plan/roadmap.md) lists every package in order.
- **Every slice tests the design.** The design is a set of bets, listed in [docs/hypotheses/README.md](docs/hypotheses/README.md). Before building, name an alternative to the documented design. After building, write down what the work showed.
- **Keep improving the design.** Question the documents. When something is ambiguous, fix it. When evidence shows a section is wrong, correct it and record the decision. When a different design looks better, test it or propose it. [docs/plan/working-method.md](docs/plan/working-method.md) says how each kind of change is made.
- **Review the whole design regularly,** one lens at a time, with [docs/plan/design-review-lenses.md](docs/plan/design-review-lenses.md).
- **The repository is your memory.** Start every session with the newest entry in [docs/journal/](docs/journal/README.md) and end every session by writing one.

Six skills in `.claude/skills/` carry these procedures: `start-session`, `end-session`, `build-slice`, `test-hypothesis`, `design-review`, and `research-brief`. Use them.

## How agents work here

The owner does not review plans or pull requests, and has said so ([decision 0055](docs/decisions/0055-agents-plan-build-and-merge-without-waiting-for-the-owner.md)).

- **Do not wait.** Plan, build, open a pull request, and merge it when every check is green. Write the plan and the alternative you weighed in the pull request or the journal. Nobody has to answer them.
- **Tell, do not ask.** Every choice between real alternatives is a decision record. A record the owner has not read says so in its status line and is in force. Every session ends with a journal entry.
- **Machines hold the gates.** A change merges only when the build, the lints, the tests, and the documentation audit pass on Windows, macOS, and Linux. Never skip, disable, or weaken a check to get there, and never merge over a red one.
- **Work in parallel.** One work package, one branch, one pull request. Two agents do not change the same crate at the same time. An agent that leads others gives each a package with a stated proof, and merges what passes.
- **Know whose a decision is.** A record marked as a design decision can be superseded by any agent, with a new record that gives the evidence. A record marked as an owner decision, and the ten rules below, are changed only by the owner. If you think one is wrong, write a record with status Proposed and carry on with other work.
- **Careful is not the same as slow to start.** The care is in the size of the step and the proof of it, not in waiting.

## Read these first, in this order

1. The newest entry in [docs/journal/](docs/journal/README.md), then [docs/plan/working-method.md](docs/plan/working-method.md).
2. [docs/plan/roadmap.md](docs/plan/roadmap.md): every work package, what it needs, what proves it, and which are ready now.
3. [docs/architecture/README.md](docs/architecture/README.md): how the crates fit together. Then the design, in `docs/architecture/crates/`, of each crate you are about to touch.
4. Part 1 of the [specification](docs/spec/README.md), once: the [summary](docs/spec/1-product/101-summary.md), [requirements](docs/spec/1-product/102-requirements-and-guarantees.md), [principles](docs/spec/1-product/103-design-principles.md), [overview](docs/spec/1-product/104-system-overview.md), [concepts](docs/spec/1-product/105-core-concepts.md), [what it latches onto](docs/spec/1-product/106-what-it-latches-onto.md), and [the bar](docs/spec/1-product/107-the-bar.md). Then [kernel and packs](docs/spec/2-kernel/202-kernel-and-packs.md) and [the scout, and who does what](docs/spec/4-building-tools/402-the-scout-and-who-does-what.md).
5. The spec sections the crate designs name for your package. The specification has seven parts: product, kernel, packs, building tools, security, running it, and project. Each section states the design as it stands; there is no rule about which section wins.
6. [docs/spec/7-project/704-readiness.md](docs/spec/7-project/704-readiness.md): what is ready, what rests on judgment alone, and what is deliberately not designed yet.
7. [docs/decisions/README.md](docs/decisions/README.md): the decisions already made.
8. The references, as needed: [technical reference](docs/reference/technical-reference.md), [action catalog](docs/reference/action-catalog.md), [file formats](docs/reference/file-formats.md), [steps, snapshots, and diffs](docs/reference/steps-snapshots-and-diffs.md), [ledger schema](docs/reference/ledger-schema.md), [glossary](docs/reference/glossary.md), and the [walkthrough](docs/reference/walkthrough.md) of what the finished product should feel like.

Do not read the whole spec into context at once. Open the sections a task touches.

Decision records and journal entries written before 2026-11-17 cite spec sections by older numbers, and some by the product's first name. [docs/archive/](docs/archive/README.md) has the key, and keeps the documents that were consolidated. Nothing in the archive is in force.

## Rules that never bend

These are the product's rules. They are the owner's, and no agent changes them.

1. **No AI at run time.** Nothing in a run may call a model or improvise. If a step cannot be done as written, stop with a named failure class.
2. **Safe, then correct, then fast, then easy.** In that order when they conflict.
3. **Every write is verified.** Record intent in the ledger before acting; read the value back; compare after normalization. On resume, reconcile before writing.
4. **Secrets never reach a caller.** Cookies, tokens, and passwords never appear in tool inputs, tool results, logs, traces, captures, fixtures, or error messages. Register every secret with the redactor.
5. **Operate openly.** The browser always runs with a data directory the engine owns; never read, copy, or drive the user's everyday browser profile or another program's cookie store. No stealth, no fingerprint spoofing, no CAPTCHA or MFA bypass, no imitating another application's client id, and no getting past a licence check or a device's protection. A block is a named stop.
6. **Rust first, other languages where they fit.** Everything on the path of a run is Rust. Use another language where it is clearly the better tool: the web pages, in-page scripts, SDKs, installers, migrations, and plug-ins, as listed in [spec section 605](docs/spec/6-running-it/605-packaging-and-release.md). A new use of another language gets a decision record.
7. **Unknown means stop.** A control, platform, response shape, or page variant with no rule is a failure, never a guess.
8. **Knowledge lives in one place.** Locators only in screen models, endpoints only in the endpoint catalog, units only in field types, business steps only in operations and pipelines. Knowledge about one kind of system lives in its pack.
9. **Name no real application.** Examples, tests, fixtures, file names, and commit messages use the imaginary `example` target or the built-in test application. Target files for real applications live outside this repository, and the project publishes no catalog of them.
10. **Never test writes against a real business application.** Use the test stands under `targets/`. Running anything against a real application needs the owner's explicit go-ahead in that session.

## How to work

- **One slice at a time, inside one phase at a time.** A slice is not started while the one before it is unproven. Each phase in the build plan ends with an exit test; the next phase starts when it passes and the result is in the journal.
- **Start each slice with a plan.** Take a package marked ready in the roadmap, and read the design of each crate it touches in `docs/architecture/crates/`. State the slice, the hypotheses it tests, an alternative you considered, the crates and interfaces involved, which side of the kernel boundary each piece falls on, and the tests that will prove it. Write it down, then build. When the package merges, mark it done in the roadmap in the same pull request.
- **Run the research first.** Before a phase, check [docs/research/11-open-research.md](docs/research/11-open-research.md) for briefs marked as needed before it. Run them, write each up as a new file in `docs/research/`, and update the spec sections they feed.
- **Verify before you depend.** Many crate names in the docs are unverified. Before adding a dependency, confirm it exists, is maintained, carries a licence on the closed list in `deny.toml`, and supports Windows, macOS, and Linux. Record the choice.
- **Treat the whole design as open to evidence.** [docs/research/README.md](docs/research/README.md) lists the spec sections that rest on judgment alone, but any section can be wrong. If implementation shows a better design, say so and record it; never silently diverge and never silently conform to something you believe is wrong.
- **Decisions get records.** Any choice that changes the design, picks between real alternatives, or reverses an earlier decision gets a new numbered file in `docs/decisions/`. Never rewrite an old record; supersede it.
- **Keep the docs true.** When code and spec disagree, fix one of them in the same change. A spec edit that changes behavior needs a decision record. After any documentation change run `python3 tools/docs_audit.py`; it must report no findings.
- **Small changes.** One concern per commit. Every commit builds and passes tests. Add a line to [CHANGELOG.md](CHANGELOG.md) for anything a user of the product would notice.
- **Spikes are thrown away.** Experiments live in `spikes/`, answer one question, and are never promoted into the product; real code is written fresh.
- **Say what you did not do.** When you stop, report what is finished, what is stubbed, what failed, and what you could not verify.

## What an agent does not do alone

These act outside this repository and cannot be taken back.

- **Touch a real application, account, or credential.** Rule 10. Real systems are tried at the end, with the owner.
- **Handle a registry token.** Never ask for one to be pasted into a conversation, and never put one in a file. Releases go out only through `.github/workflows/release.yml` with short-lived credentials, started by a version tag or by hand; see [docs/plan/publishing-setup.md](docs/plan/publishing-setup.md) and [RELEASING.md](RELEASING.md).
- **Add a dependency under a licence that is not permissive.** The list in `deny.toml` is closed. A new permissive licence is added on purpose, with a line in the journal; anything else is not added.

Everything else is yours to decide and record.

## Code conventions

- **Workspace.** One Cargo workspace. Kernel crates are named `a2m-*`, pack crates `a2m-pack-*`, and the product crate `anything2mcp`, laid out as in [spec section 605](docs/spec/6-running-it/605-packaging-and-release.md).
- **Kernel and packs.** A kernel crate never depends on a pack crate. A pack crate sees the kernel up to the provider contract, layers 1 to 5, and the base of its own pack. Only the product crate names packs. The layering table in the technical reference is the rule, and the audit enforces it.
- **Toolchain.** Stable Rust, pinned in `rust-toolchain.toml`. `cargo fmt` and `cargo clippy -- -D warnings` must pass.
- **Errors.** Library crates return typed errors that map to exactly one failure class and code from the technical reference. No `unwrap` or `expect` in library code outside tests. No panics across crate boundaries.
- **Unsafe.** None without a comment explaining why it is sound and a test that exercises it.
- **Async.** Tokio. No blocking calls on async threads. No fixed sleeps as waits; wait on a named condition.
- **Types over strings.** Archetypes, effect classes, hazards, failure classes, evidence levels, and statuses are enums in `a2m-core`, and a test holds their names to the documents. A mode and a failure code are checked names, because packs and crates bring their own ([decision 0060](docs/decisions/0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md)). Secrets are a wrapper type that cannot be printed or serialized.
- **Numbers.** Decimal arithmetic for any business value. Never floats for comparison.
- **Public interfaces.** The driver, auth-provider, source-reader, discoverer, and plug-in interfaces, and the provider manifest, are public contracts; changing one needs a decision record.
- **Naming.** Tool names follow `{target}_{module}_{object}_{verb}`. Built-in tools are prefixed `a2m_`. Commands are subcommands of `anything2mcp`; `a2m` is a short name for the same program, and documents use the full one.
- **One vocabulary.** Use the terms in [the glossary](docs/reference/glossary.md) in code, files, and messages; do not introduce synonyms.
- **One registry.** Every authoring and admin action is defined once in `a2m-actions`; its command, MCP tool, REST route, and SDK method are generated. Never hand-write one of those faces.
- **A user's session is theirs alone.** A session signed in as a person may be used only by that person or an agent acting for them. No role, including administrator, runs as someone else.
- **One decider, one renewer.** Only the policy engine decides whether something may happen, and only the session keeper signs in or renews. Drivers and front doors ask; they never decide or renew.
- **Assume the agent may be steered.** No input may change where a request goes or which file is read; see [spec section 502](docs/spec/5-security/502-defending-against-a-manipulated-agent.md).
- **Fail closed.** No ledger, no write; nothing unverified runs; see [spec section 224](docs/spec/2-kernel/224-when-the-engine-itself-fails.md).
- **Nothing phones home.** No usage data, crash reports, or update checks unless a person switches them on.
- **Thin clients.** Commands, the stdio proxy, and the SDKs hold no engine logic; they talk to the local engine. There is no approve command, tool, or route on a local machine.
- **No client is special.** Nothing in the kernel knows which make of agent is calling. A client's quirk, where one must be handled, is data in a profile.

## Testing

Tests secure the system; they do not advance it. Keep the suite small and make every test count.

- Every crate has unit tests. Every driver and widget driver has tests against a test stand, described in [docs/plan/test-application.md](docs/plan/test-application.md).
- A test stand imitates the part of its kind of system that makes the kind hard, and nothing else. Make it as real as is cheap.
- Tests ask the test stand what it stored. A test never proves a write using Anything2MCP's own read path alone.
- CI runs on Windows, macOS, and Linux.
- The benchmark suite in `bench/` runs in CI. A regression over 10% on a budgeted path fails the build. Budgets in [spec section 225](docs/spec/2-kernel/225-performance.md) are targets until measured; when you measure, record the numbers and revise the budget with a decision record if needed.
- The nine ledger rules in spec section 208 are each a test; a change that breaks one cannot merge.
- Fault-injection tests are part of the definition of done for anything that writes: expired session, lost connection after a write was sent, bad input, stale approval, blocked surface.
- Fixtures must be synthetic or scrubbed. Never commit recorded business data.

## Definition of done for a phase

- The exit test in the build plan passes on all three operating systems, or the gap is written down in the journal.
- Formatting, lints, unit tests, and benchmarks pass.
- New public interfaces are documented in the technical reference.
- Decisions made along the way have records.
- The spec sections touched are still true.
- A short report in the journal states what was built, what was measured, and what remains.

## Where things are

| Path | Contents |
| --- | --- |
| `crates/` | The kernel (`a2m-*`), the packs (`a2m-pack-*`), and the product crate (`anything2mcp`) |
| `targets/` | The imaginary `example` target and the test stands |
| `bindings/` | The pip and npm packages |
| `docs/architecture/` | How the code is put together: who defines, implements, and joins each interface, and one design per crate |
| `docs/spec/` | The specification, in seven parts, one section per file |
| `docs/reference/` | Interfaces, data model, action catalog, file formats, steps and diffs, ledger schema, glossary, settings examples |
| `docs/decisions/` | Decision records |
| `docs/research/` | Research notes and open research briefs |
| `docs/references/` | Every source URL cited |
| `docs/plan/` | The roadmap, the working method, the design review lenses, the test application plan, publishing setup |
| `docs/archive/` | The first design's consolidated sections and its plans, kept as history, with the key from old section numbers to new |
| `docs/hypotheses/` | The register of design bets and the results of testing them |
| `docs/journal/` | One entry per session; read the newest first |
| `.claude/skills/` | Procedures for starting and ending a session, building a slice, testing a hypothesis, reviewing the design, and running research |
| `spikes/` | Throwaway experiments; never part of the product |
| `docs/assets/` | Diagrams |
| `tools/docs_audit.py` | Checks the documents for broken links and contradictions, and the crates against the layering table |
| `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, `CHANGELOG.md`, `RELEASING.md` | How to contribute, report a vulnerability, behave, see what changed, and cut a release |
