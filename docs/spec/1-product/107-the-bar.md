# 107. The bar: what the best means

The owner's aim is that Anything2MCP be the best system of its kind. A claim like that is worth nothing unless it can be checked. This section turns it into a test: the dimensions on which a system that exposes existing systems to agents is judged, what the best looks like on each, the number or test that shows it, where it is measured, and where the design stands today. It also says what the product will not do to look better, how it compares with the kinds of approach that already exist, and how the bar is kept. Its scorecard says, row by row, how far each proof has got.

## The dimensions

Each dimension names a proof that can fail. A proof is a test in the suite, a benchmark in `bench/`, the exit test of a package in the [roadmap](../../plan/roadmap.md), or a hypothesis in the [register](../../hypotheses/README.md).

### Correctness of writes

**The best:** a write is done exactly once and is proven by the system itself. No interruption, at any step, leaves a duplicate or a record nobody checked.
**The test:** killed at a random step and resumed, a run leaves no duplicate and nothing unverified, by the test stand's own count, never by Anything2MCP's read path ([section 208](../2-kernel/208-runs-the-ledger-and-resume.md)). Each of the ledger's nine rules is a test that fails when the rule is broken. Every write pipeline runs twice in testing, and the second run changes nothing.
**Measured in:** packages 3.1, 3.7, and 3.10; D5 for the command pack; 7.2 for a lost runner.

### Determinism

**The best:** the same input against the same system state gives the same steps, every time, on every operating system.
**The test:** five runs give one trace signature, on Windows, macOS, and Linux ([section 217](../2-kernel/217-traces-evidence-and-repair.md)). Two scouts of the same system give the same report, by hash.
**Measured in:** package 1.2 and hypothesis H-16; package E5 for the scout.

### Honesty about evidence

**The best:** nothing is ever presented as more certain than it was shown to be. A guess never reaches an agent as a fact.
**The test:** a finding cannot be built above `discovered`; a draft made by rule carries no unit, key, or read-back rule the system did not state; only tools at the level their listing needs are listed ([section 219](../2-kernel/219-the-evidence-ladder.md)). A write that could not be read back is never reported as checked.
**Measured in:** packages B2, E3, E4, and E5.

### Safety against a steered agent

**The best:** a manipulated agent can do nothing through Anything2MCP that a person could not have been asked to approve, and cannot make it reach, read, or send anything its tools do not declare ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)).
**The test:** a suite of manipulation attempts, run against every release: inputs that try to redirect a request, name a file, approve a write, loosen a protected setting, or plant an instruction in a result. Each attempt has a test that fails when its defence is removed. A script cannot approve a write that a person can.
**Measured in:** packages 3.6, 3.10, and 9.3; hypotheses H-12 and H-17.

### Secrets

**The best:** no cookie, token, or password ever appears in a tool input, result, log, trace, capture, fixture, or error message.
**The test:** a secret planted in the system's answer appears nowhere outside the vault; a secret value cannot be printed or serialized by type.
**Measured in:** packages B1 and 2.1.

### Breadth

**The best:** the kinds of system a person actually needs to reach are reached, each with the same guarantees, and no kind is claimed before it is proven.
**The number:** the count of kinds of system with a pack that has cleared the bar a new pack must clear ([section 202](../2-kernel/202-kernel-and-packs.md)), and that a second kind needed no change to the kernel beyond its contracts.
**Measured in:** milestones 4, 8, and 10 of the roadmap; hypotheses H-26 and H-29.

### How little the developer must do

**The best:** pointed at an address, the toolkit does every mechanical step by itself, and leaves the developer only meaning, writes, and open questions ([section 402](../4-building-tools/402-the-scout-and-who-does-what.md)).
**The numbers:** the time from an address to a first verified read tool, under ten minutes for an agent with no prior knowledge; the share of drafted reads that pass their first try and are kept without edits.
**Measured in:** package 5.6 and hypothesis H-18; package E5 and hypothesis H-31.

### Speed

**The best:** Anything2MCP adds almost nothing to the time the system itself takes, and uses the fastest binding that has been proven correct.
**The numbers:** the overhead budgets of [section 225](../2-kernel/225-performance.md): under 100 ms from start to ready; under 3 ms for the policy check, scheduling, and ledger write of a call; under 5 ms beyond the system for a direct request; under 50 ms for a step in an interface. An operation that gains a request binding through shadow runs at least ten times faster than through the interface, with identical results. A regression over 10% on a budgeted path is to fail the build; the comparison exists (`a2m-bench compare`) and CI does not run it yet, because no budgeted path is measured.
**Measured in:** packages 0.7, 1.7, and 4.5; hypothesis H-20.

### Configurability

**The best:** every behavior that may differ between installations can be set at the right scope, the defaults are safe without any file, and no setting can reach safety ([section 226](../2-kernel/226-configuration.md)).
**The test:** an empty file is a complete configuration; an unknown key fails the load; every key read in code is declared in the one schema, and `config.show` names where every value came from; every guardrail has a test that a file, a variable, a flag, and a preset each fail to loosen it.
**Measured in:** the settings loader in `a2m-actions`, with `config.show` in Phase 1 and `config.set` in Phase 3.

### How it fails

**The best:** every failure is named, has one class that alone decides what happens next, and leaves the run resumable where resuming is safe ([section 224](../2-kernel/224-when-the-engine-itself-fails.md)).
**The test:** every failure the design names, in the engine and in each pack, has a fault test: an expired session, a connection lost after a write was sent, bad input, a stale approval, a blocked surface, a lost runner. No failure surfaces as an unclassified error.
**Measured in:** packages 3.10, 7.2, and 9.3.

### Openness

**The best:** the product acts as the user, openly, where the user runs it, and talks to nobody it was not told to.
**The test:** CI runs the engine with no network configuration and fails if it opens any outside connection ([section 605](../6-running-it/605-packaging-and-release.md)). The browser runs only with a profile the engine owns. A block is a named stop, never a retry under disguise. The build and tests pass on all three operating systems.
**Measured in:** package 0.1 for the three systems; the outside-connection test; package 2.3 for the engine's own profile.

### Neutrality

**The best:** any agent that speaks the protocol is served alike, and nothing in the kernel knows which make is calling.
**The test:** conformance against the protocol with at least two unrelated clients; a dispatch with two client labels records the same decisions ([section 222](../2-kernel/222-front-doors-and-the-local-engine.md)). The building tools work as commands and as MCP tools, so no client's features are needed.
**Measured in:** package 0.5, then 1.5; hypothesis H-13.

## What it will not do to win

Some things would make a demonstration more impressive and the product worse. Refusing them is part of the bar, not a cost against it.

| It will not | Though it would | Because |
| --- | --- | --- |
| Let a model improvise when a step fails at run time | Make more runs finish | A run that improvises is neither repeatable nor verified. A step that cannot be done as written stops with a named class |
| Get past a sign-in, a second factor, a bot defence, a licence check, or a device's protection | Reach more systems | It acts with the user's rights and no more. A block is a named stop |
| Hide that it is automation, or imitate another client | Be blocked less | Operating openly is a rule, not a setting |
| Guess a control, a unit, a key, or a response shape it has no rule for | Draft more, and fail less often | Unknown means stop. A guess that is wrong writes the wrong thing |
| Count a write as done because a request returned | Look faster | A write is done when the system shows it back |
| Click by position on a picture of the screen, or run a free-form shell | Cover more systems at once | Neither can be verified |
| Publish a catalog of tools for real products | Shorten the first hour | A target is the developer's to keep and to answer for |

## Against what exists

[Section 701](../7-project/701-prior-art.md) lists the projects. By kind of approach:

| Approach | What it does well | What this product must do better to be worth choosing |
| --- | --- | --- |
| Deterministic integrations built for websites | Repeatable runs; a review of every build; reading the site back before another write | Build with whatever agent the developer has, keep a session alive between calls instead of signing in again, and run on all three operating systems |
| Reviewed workflows turned into read-only tools | Declared permissions; evidence for each version | Writes, verified and resumable; sign-in and sessions |
| Recorded traffic turned into request code | Finding the endpoints behind an interface | Verify what it finds, carry the user's identity, serve the result over MCP, and keep the interface as a fallback |
| Replayed code or cached actions, with an agent taking over when replay fails | Turning one agent's decisions into steps that are fast to repeat | Keep the replay and refuse the takeover: no agent at run time, a named stop instead |
| An agent driving a browser live, one action per tool | Reaching any page at once; snapshots built for agents | Deterministic tools that do a business step, with writes read back, instead of live control |
| Tools that a site declares for agents in the browser | The owner's own description of what may be done | Work on the systems whose owners have added nothing, and use such tools as one mode where they exist |
| Hosted workflow, extraction, and scraping platforms | Vaults, repair loops with human review, runs that finish later | Run where the user runs it, write as well as read, and act as each user |

No existing approach combines every mode in one engine with verified writes, real identity handling, governance, and a server shape. That combination, proven on each dimension above, is what has to be true for this product to be the one worth choosing.

## The scorecard

One row per dimension. **State** is `designed` (a section designs it), `built` (code exists and its tests pass), or `measured` (the proof has run and its number is recorded). A row that says `built` names the part its test covers; the rest of that row is still `designed`. No row is `measured` yet.

| Dimension | Target | Proof | State |
| --- | --- | --- | --- |
| Correctness of writes | No duplicate and nothing unverified after any interruption | Kill-and-resume on every test stand; the nine ledger rules as tests | built, for the nine ledger rules (`crates/a2m-ledger/tests/rules.rs`, package 3.1) and for a process killed after a write was sent, on the test application (`crates/anything2mcp/tests/verified_write.rs`); kill-and-resume on every test stand is designed |
| Determinism | One trace signature across five runs and three operating systems | Package 1.2; H-16 | built, for five runs on one machine with one signature (`crates/anything2mcp/tests/first_read.rs`); the same signature across three operating systems is designed |
| Honesty about evidence | No finding above `discovered`; no listed tool below its level | Packages B2, E3, E5 | built, for the finding's level (B2, and the store of package E1); the rest designed |
| Safety against a steered agent | Every named attack fails, and its test fails when the defence is removed | The manipulation suite, package 9.3 | built, for the engine's manipulation tests (`crates/a2m-engine/tests/manipulation.rs`, package 3.10); the full suite is designed |
| Secrets | A planted secret appears nowhere outside the vault | Package 2.1 | built, for a secret that cannot be printed (B1), and for a planted secret through the REST and MCP doors, a result, and a log line; package 2.1's proof, a secret planted in a system's own answer, is not yet shown |
| Breadth | Each kind supported only after its pack clears the entry bar; the second kind needs no kernel change | Milestones 4, 8, 10; H-26 | built in part: nine packs have crates tested against their stands; the result of H-26 (package D4) is not yet written |
| Developer effort | A first verified read tool in under ten minutes; most drafted reads kept untouched | Package 5.6, H-18; H-31 | built, for the flow from an empty folder to a published read with no hand step (`crates/anything2mcp/tests/first_hour.rs`); the ten-minute measure with an agent of no prior knowledge has not run |
| Speed | The overhead budgets of section 225; ten times faster through a promoted request binding | Packages 1.7, 4.5; H-20 | built, for the harness, which runs in CI (package 0.7); no budget is measured, and the 10% gate is not wired |
| Configurability | Empty file complete; unknown key an error; no guardrail loosened by any layer | Tests over the settings schema | built, for the settings loader (package 1.4a); the rest designed |
| How it fails | Every named failure has a fault test; nothing unclassified | Package 9.3 | built, for the engine's fault tests (`crates/a2m-engine/tests/faults.rs`, package 3.10); the rest designed |
| Openness | No outside connection unasked; green on three operating systems | The CI connection test; package 0.1 | built, for the three operating systems (0.1); the rest designed |
| Neutrality | Two unrelated clients pass; decisions independent of the client label | Packages 0.5, 1.5 | built in part: two unrelated clients called the spike of package 0.5, and two outside client libraries called the product's `bridge` by hand, not in CI (package 1.5); the rest designed |

## How the bar is kept

- **Every milestone ends by updating the scorecard.** The pull request that closes a milestone changes each row it touched, with the number measured and a link to the result.
- **A number that misses its target is a finding, not a footnote.** It is written in the journal, and either the design changes or the target does, by decision record, with the evidence. A target is never quietly moved.
- **A row only moves forward on proof.** `built` needs the test in the suite and passing on three operating systems; `measured` needs the number recorded where the proof says.
- **New dimensions are added the same way.** A dimension is added only with a target and a proof that can fail.

## Open points

- The exact share of drafted reads that counts as "most" kept untouched. It is set when the scout first runs against the test application (H-31).
- How breadth is weighed: a count of kinds says nothing of how many systems of each kind work. A measure across real systems needs the owner's go-ahead, at the end (package 9.5).
- Whether the scorecard should be generated from the suite's results rather than kept by hand.
