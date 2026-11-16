# Instructions for AI agents building Web2MCP

You are building Web2MCP from an empty repository. A full design is written, and none of it has yet met running code. Your job is to build the core slowly, one small proven slice at a time, to test the design's bets as you go, and to keep making the design better. You are a designer of this system as well as its builder.

## What Web2MCP is

A self-hosted system that latches onto existing applications and exposes them to AI agents as deterministic, verified MCP tools. Rust core, one binary. It reaches applications through seven modes (interface, in-page HTTP, direct HTTP, API descriptions, site tools, an app's own MCP server, desktop accessibility), and runs on one laptop or as a company server.

## How this project grows

- **Slowly, and core first.** Build the engine loop, one driver, sessions, the ledger, the policy gate, and the compiler before anything else, as thin slices that each work end to end. [docs/plan/core-first.md](docs/plan/core-first.md) gives the order. One slice per session is a good pace.
- **Every slice tests the design.** The design is a set of bets, listed in [docs/hypotheses/README.md](docs/hypotheses/README.md). Before building, name an alternative to the documented design. After building, write down what the work showed.
- **Keep improving the design.** Question the documents. When something is ambiguous, fix it. When evidence shows a section is wrong, correct it and record the decision. When a different design looks better, propose it. [docs/plan/working-method.md](docs/plan/working-method.md) says how each kind of change is made.
- **Review the whole design regularly,** one lens at a time, with [docs/plan/design-review-lenses.md](docs/plan/design-review-lenses.md).
- **The repository is your memory.** Start every session with the newest entry in [docs/journal/](docs/journal/README.md) and end every session by writing one.

Six skills in `.claude/skills/` carry these procedures: `start-session`, `end-session`, `build-slice`, `test-hypothesis`, `design-review`, and `research-brief`. Use them.

## Read these first, in this order

0. The newest entry in [docs/journal/](docs/journal/README.md), then [docs/plan/working-method.md](docs/plan/working-method.md)
1. [docs/plan/readiness.md](docs/plan/readiness.md): what is ready, the risks, and what is deliberately not designed yet
2. [docs/spec/01-summary.md](docs/spec/01-summary.md), [02](docs/spec/02-requirements-and-guarantees.md), [03](docs/spec/03-design-principles.md), [04](docs/spec/04-system-overview.md), [05](docs/spec/05-core-concepts.md)
3. [docs/decisions/README.md](docs/decisions/README.md): the decisions already made
4. [docs/plan/phase-map.md](docs/plan/phase-map.md): for the phase in hand, what to read, which decisions bind it, and what to research first. Phases 0 to 3 have task lists: [0](docs/plan/phase-0.md), [1](docs/plan/phase-1.md), [2](docs/plan/phase-2.md), [3](docs/plan/phase-3.md). The later three are drafts to revise as each earlier phase ends
5. The spec sections that phase lists; the index is [docs/spec/README.md](docs/spec/README.md). Sections 39 to 52 were written last, refine earlier sections, and win where they differ
6. The references, as needed: [technical reference](docs/reference/technical-reference.md), [action catalog](docs/reference/action-catalog.md), [file formats](docs/reference/file-formats.md), [steps, snapshots, and diffs](docs/reference/steps-snapshots-and-diffs.md), [ledger schema](docs/reference/ledger-schema.md), [glossary](docs/reference/glossary.md), [settings examples](docs/reference/settings-example.md), and the [walkthrough](docs/reference/walkthrough.md) of what the finished product should feel like

Do not read the whole spec into context at once. Open the sections a task touches.

## Rules that never bend

1. **No AI at run time.** Nothing in a run may call a model or improvise. If a step cannot be done as written, stop with a named failure class.
2. **Safe, then correct, then fast, then easy.** In that order when they conflict.
3. **Every write is verified.** Record intent in the ledger before acting; read the value back; compare after normalization. On resume, reconcile before writing.
4. **Secrets never reach a caller.** Cookies, tokens, and passwords never appear in tool inputs, tool results, logs, traces, captures, fixtures, or error messages. Register every secret with the redactor.
5. **Operate openly.** The browser always runs with a data directory the engine owns; never read, copy, or drive the user's everyday browser profile or another program's cookie store. No stealth, no fingerprint spoofing, no CAPTCHA or MFA bypass, no imitating another application's client id. A block is a named stop.
6. **Rust first, other languages where they fit.** Everything on the path of a run is Rust. Use another language where it is clearly the better tool: the web pages, in-page scripts, SDKs, installers, migrations, and plug-ins, as listed in [spec section 29](docs/spec/29-repository-crates-and-packaging.md). A new use of another language gets a decision record.
7. **Unknown means stop.** A control, platform, response shape, or page variant with no rule is a failure, never a guess.
8. **Knowledge lives in one place.** Locators only in screen models, endpoints only in the endpoint catalog, units only in field types, business steps only in operations and pipelines.
9. **Name no real application.** Examples, tests, fixtures, file names, and commit messages use the imaginary `example` target or the built-in test application. Target files for real applications live outside this repository.
10. **Never test writes against a real business application.** Use the built-in test application under `targets/`. Running anything against a real application needs the owner's explicit go-ahead in that session.

## How to work

- **One slice at a time, inside one phase at a time.** A slice is not started while the one before it is unproven. Each phase in the build plan ends with an exit test; do not start the next phase until it passes and the owner has seen the result.
- **Start each slice with a plan.** State the slice, the hypotheses it tests, an alternative you considered, the crates and interfaces involved, and the tests that will prove it. Get the owner's go-ahead before creating files.
- **Run the research first.** Before a phase, check [docs/research/11-open-research.md](docs/research/11-open-research.md) for briefs marked as needed before it. Run them, write each up as a new file in `docs/research/`, and update the spec sections they feed.
- **Verify before you depend.** Many crate names in the docs are unverified. Before adding a dependency, confirm it exists, is maintained, is licensed compatibly with Apache-2.0, and supports Windows, macOS, and Linux. Record the choice.
- **Treat the whole design as open to evidence.** [docs/research/README.md](docs/research/README.md) lists the spec sections that rest on judgment alone, but any section can be wrong. If implementation shows a better design, say so and propose it; never silently diverge and never silently conform to something you believe is wrong.
- **Decisions get records.** Any choice that changes the design, picks between real alternatives, or reverses an earlier decision gets a new numbered file in `docs/decisions/`. Never rewrite an old record; supersede it. A change you want but have not been given is a record with status Proposed.
- **Keep the docs true.** When code and spec disagree, fix one of them in the same change. A spec edit that changes behavior needs a decision record. After any documentation change run `python3 tools/docs_audit.py`; it must report no findings.
- **Small, reviewable changes.** One concern per commit. Every commit builds and passes tests.
- **Spikes are thrown away.** Experiments live in `spikes/`, answer one question, and are never promoted into the product; real code is written fresh.
- **Say what you did not do.** When you stop, report what is finished, what is stubbed, what failed, and what you could not verify.

## Ask the owner before

- Changing or reversing any accepted decision in `docs/decisions/`. Proposing a change is always welcome; making one needs the owner.
- Changing the published package names: `web2mcp` on pip and crates.io, `w2mcp` on npm.
- Adding a dependency with a license other than MIT, Apache-2.0, BSD, ISC, or similar permissive terms.
- Touching any real application, account, or credential. Never ask for a registry token to be pasted into a conversation; see [docs/plan/publishing-setup.md](docs/plan/publishing-setup.md).
- Cutting scope from a phase's exit criteria.
- Making the repository public.

## Code conventions

- **Workspace.** One Cargo workspace. Crates are named `w2m-*` and laid out as in [spec section 29](docs/spec/29-repository-crates-and-packaging.md). Crates depend only downward, per the layering table in the technical reference.
- **Toolchain.** Stable Rust, pinned in `rust-toolchain.toml`. `cargo fmt` and `cargo clippy -- -D warnings` must pass.
- **Errors.** Library crates return typed errors that map to exactly one failure class and code from the technical reference. No `unwrap` or `expect` in library code outside tests. No panics across crate boundaries.
- **Unsafe.** None without a comment explaining why it is sound and a test that exercises it.
- **Async.** Tokio. No blocking calls on async threads. No fixed sleeps as waits; wait on a named condition.
- **Types over strings.** Modes, effect classes, failure classes, and statuses are enums. Secrets are a wrapper type that cannot be printed or serialized.
- **Numbers.** Decimal arithmetic for any business value. Never floats for comparison.
- **Public interfaces.** The driver, auth-provider, source-reader, and plug-in interfaces are public contracts; changing one needs a decision record.
- **Naming.** Tool names follow `{target}_{module}_{object}_{verb}`. Built-in tools are prefixed `w2m_`. Commands are subcommands of `web2mcp`.
- **One vocabulary.** Use the terms in [the glossary](docs/reference/glossary.md) in code, files, and messages; do not introduce synonyms.
- **One registry.** Every authoring and admin action is defined once in `w2m-actions`; its command, MCP tool, REST route, and SDK method are generated. Never hand-write one of those faces.
- **A user's session is theirs alone.** A session signed in as a person may be used only by that person or an agent acting for them. No role, including administrator, runs as someone else.
- **One decider, one renewer.** Only the policy engine decides whether something may happen, and only the session keeper signs in or renews. Drivers and front doors ask; they never decide or renew.
- **Assume the agent may be steered.** No input may change where a request goes or which file is read; see [spec section 51](docs/spec/51-defending-against-a-manipulated-agent.md).
- **Fail closed.** No ledger, no write; nothing unverified runs; see [spec section 52](docs/spec/52-when-the-engine-itself-fails.md).
- **Nothing phones home.** No usage data, crash reports, or update checks unless a person switches them on.
- **Thin clients.** Commands, the stdio proxy, and the SDKs hold no engine logic; they talk to the local engine. There is no approve command, tool, or route on a local machine.

## Testing

- Every crate has unit tests. Every driver and widget driver has tests against the built-in test application, described in [docs/plan/test-application.md](docs/plan/test-application.md).
- Tests ask the test application what it stored. A test never proves a write using Web2MCP's own read path alone.
- CI runs on Windows, macOS, and Linux.
- The benchmark suite in `bench/` runs in CI. A regression over 10% on a budgeted path fails the build. Budgets in [spec section 9](docs/spec/09-performance.md) are targets until measured; when you measure, record the numbers and revise the budget with a decision record if needed.
- The nine ledger rules in spec section 42 are each a test; a change that breaks one cannot merge.
- Fault-injection tests are part of the definition of done for anything that writes: expired session, lost connection after a write was sent, bad input, stale approval, blocked surface.
- Fixtures must be synthetic or scrubbed. Never commit recorded business data.

## Definition of done for a phase

- The exit test in the build plan passes on all three operating systems, or the gap is written down and agreed.
- Formatting, lints, unit tests, and benchmarks pass.
- New public interfaces are documented in the technical reference.
- Decisions made along the way have records.
- The spec sections touched are still true.
- A short report states what was built, what was measured, and what remains.

## Where things are

| Path | Contents |
| --- | --- |
| `docs/spec/` | The system design, one section per file |
| `docs/reference/` | Interfaces, data model, action catalog, file formats, steps and diffs, ledger schema, glossary, settings examples |
| `docs/decisions/` | Decision records |
| `docs/research/` | Research notes and open research briefs |
| `docs/references/` | Every source URL cited |
| `docs/plan/` | Working method, core-first slice order, design review lenses, readiness report, phase map, task lists for Phases 0 to 3, the test application plan, publishing setup |
| `docs/hypotheses/` | The register of design bets and the results of testing them |
| `docs/journal/` | One entry per session; read the newest first |
| `.claude/skills/` | Procedures for starting and ending a session, building a slice, testing a hypothesis, reviewing the design, and running research |
| `spikes/` | Throwaway experiments; never part of the product |
| `docs/assets/` | Diagrams |
| `tools/docs_audit.py` | Checks the documentation for broken links and contradictions |
