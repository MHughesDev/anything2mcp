# 402. The scout, and who does what

A developer points the runtime at a system and wants tools. This section draws the line between the work the runtime does by itself, the work that stays with the developer or their agent, and the work only a person may do. It also says what the runtime assumes about the world and what it leaves out on purpose. Decisions: [record 0058](../../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md) and [record 0059](../../decisions/0059-the-scout-drafts-and-proves-reads-and-three-things-stay-with-the-developer.md). Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

## The line

Every job falls into one of three kinds. The test is what the job needs.

| Kind | The job needs | Who does it | Examples |
| --- | --- | --- | --- |
| Mechanical | A rule that gives the same answer every time | The runtime, by itself | Telling what kind of system this is; reading what it publishes; turning a published description into an endpoint; trying a read five times and comparing |
| Judgment | Knowing what the system is for | The developer, or an agent working for them, at build time | Naming a tool; choosing which of two hundred endpoints matter; saying how a record is found again; settling a conflict between two findings |
| Authority | The right to decide | A person | Saying the system may be inspected; signing in; approving a write; publishing tools to other people |

Two rules hold the line in place.

- **The runtime never does judgment.** It runs no model ([principle 3](../1-product/103-design-principles.md)). Where a rule gives no answer, the result is an open question in the unknowns ledger ([section 218](../2-kernel/218-discovery-and-findings.md)), never a guess.
- **Nobody but a person holds authority.** An agent cannot grant itself consent, approve its own write, or publish. These are the product's own protections and they are not settings ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)).

## The scout

The scout is the mechanical half of the overlay method ([section 401](401-overlaying-tools-on-an-existing-system.md)) run as one pipeline. One action, `scout.run`, takes a target and does steps 2 and 3 of that method, the mechanical part of step 4, and the read half of step 6. `scout.report` returns the last report.

| # | Stage | What happens | Built from |
| --- | --- | --- | --- |
| 1 | Identify | Each declared surface is probed: what kind it is, which pack serves it, how sign-in works. Surfaces the target did not declare and plainly offers, such as a published API description, are reported as candidates and are not entered | `target.probe`, `auth.detect` |
| 2 | Discover | Every enabled discoverer that suits a surface runs, within its bounds, as a discovery job on a runner inside the engine's sandbox ([section 218](../2-kernel/218-discovery-and-findings.md)) | `discover.run` |
| 3 | Compare | What the system says about itself is set against what was seen. Each difference becomes a finding or an open question | [Section 218](../2-kernel/218-discovery-and-findings.md) |
| 4 | Draft | The drafter writes draft files wherever a finding maps to a file by rule | `draft.write` |
| 5 | Check | The drafts are linted and compiled. A draft that fails is kept, with its findings, and marked | `draft.lint` |
| 6 | Try | Each drafted read that the scout is allowed to try runs in the sandbox, five times, and the results are compared | `draft.try` |
| 7 | Report | One report: what was found, what was drafted, what was proven, what is open, and what the developer should look at first | `scout.report` |

- **It is a fixed pipeline.** There is no model in it and no choice in it. The same system in the same state gives the same report, and the report carries a hash of its content, which leaves out the identifiers of its runs, to show it ([decision 0171](../../decisions/0171-the-scouts-report-is-hashed-data-with-the-systems-words-kept-apart.md)).
- **It is resumable.** Each stage stores its result. A scout that is stopped, or that hits a bound, continues from the last finished stage. Identify and discover share one discovery job per surface, because the engine runs every discoverer that suits a surface in one job ([decision 0170](../../decisions/0170-the-scout-identifies-each-surface-with-one-discovery-job-and-stores-each-stage.md)).
- **It is bounded and polite.** Every stage has a limit on time, size, depth, and number of requests, and the report says which limits were hit. The scout identifies itself as automation, keeps to the target's rate limit, and stops at a block ([principle 9](../1-product/103-design-principles.md)).
- **It needs consent like any discovery.** On one machine the person who added the target has given it. On a server the target's consent record must exist first ([section 213](../2-kernel/213-effects-hazards-and-consent.md)).
- **It needs a session, and cannot make one.** If a surface needs a sign-in, the scout reports `needs_login` for that surface, carries on with the others, and resumes when the person has signed in.

## What the drafter writes

The drafter is kernel code. Each pack registers the drafting rules for its own file kinds ([section 202](../2-kernel/202-kernel-and-packs.md)).

| From a finding about | The drafter writes | It leaves to the developer |
| --- | --- | --- |
| A published API description | Endpoint entries, field types taken from the schema, and one read operation for each GET the description states, with the description's own example ([decision 0179](../../decisions/0179-what-each-pack-drafts-so-that-a-scout-ends-with-reads-that-work.md)) | Which of them become tools, and their names in the language of the work |
| Captured exchanges, with no description | Endpoint entries with two-tier path templates ([section 401](401-overlaying-tools-on-an-existing-system.md)), marked `inferred` | Confirming every template; all operations |
| An upstream MCP server's tool list | A pass-through read operation for each tool the server marks read-only, unless its whole input is one string | Tools not marked read-only; whether the server's marking is believed for writes |
| A request a page made by itself, with GET, answered in JSON | A page request in the web pack's `page_requests/` and a `page_http` read of the answer's plain values, a string as `untrusted_text`; requests that differ in one segment, made by pages that differ in the same value, are matched into one read whose placeholder is its key ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)) | Numbers with a fraction, nested values, every write, and the names |
| A program's own help and version output, from only the help and version forms the target declares | Command definitions with typed arguments | Which commands read and which write. Help text does not say, so no operation is drafted |
| An observed interface | For each table a snapshot names and can tell apart: a screen model that locates it by role and name and reads it by its headings, and a list read of its columns as `untrusted_text`; for the values the page labels, a `get` read of them as `untrusted_text`. Pages of one screen whose addresses differ in one segment are matched into one screen, whose placeholder is the reads' key ([decisions 0183](../../decisions/0183-the-web-pack-drafts-a-read-of-each-table-a-snapshot-names.md) and [0187](../../decisions/0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)) | Every field that is set and every action on a screen; what a field means; a label two values share |
| A database's schema | Record kinds, field types, and for each table with a declared key a read by key and a list bounded by the pack's row limit | Joins, business names, and every write |
| The structure of a real file a target names | A document draft for the file pack: the fingerprint, and a record kind for each defined table or plainly headed sheet, a delimited or workbook cell as `untrusted_text`; and a `list` read of each, which compiles ([section 308](../3-packs/308-the-file-pack.md), [decision 0182](../../decisions/0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md)) | The key, unless the file states one, asked in a drafted read by key; units; names |
| The structure of a real file a pipeline reads | A source draft: a source definition and a template fingerprint | The mapping from columns to fields |
| A device's own description of itself | A map of its readable objects and a read for each | Units the device does not state; every write |

Every draft made this way:

- is an ordinary draft at `reconstructed` ([section 219](../2-kernel/219-the-evidence-ladder.md)), and names the findings it was made from;
- gets a mechanical name, made from the surface's own words, that is marked as provisional;
- carries no unit, key, or read-back rule the system did not itself state. Where one is needed and missing, the draft has an open question in its place and does not compile until it is settled;
- types an output of the system's text as `untrusted_text`, and carries an example only where the system published one about itself, such as a description's own example of a parameter. A value of the system's data is never kept as an example, and no test or fixture is drafted: a read with no example is drafted, and the report says why it was not tried ([decision 0179](../../decisions/0179-what-each-pack-drafts-so-that-a-scout-ends-with-reads-that-work.md)).

## What the scout may try

The scout tries a read only when one of two things is true.

| The read is safe because | Examples |
| --- | --- |
| The engine enforces it | A database read inside a read-only transaction; a file opened for reading; in the web pack, an interface snapshot that follows plain links and never submits a form or presses a button |
| The system's own contract says so | A request with a method the service's own description marks safe; a device object the device lists as readable |

Everything else is left untried: a command with unknown effect, a tool not marked read-only, a request with no description behind it. For a program, the scout runs nothing but the help and version forms the target declares. The scout never tries a write, on any system, under any setting. A read that passes five identical runs moves to `executed`. It goes no higher by the scout's hand: `behavior_verified` needs the publish gates, and `approved` needs a person.

## What stays with the developer

Three things, and the report lists them in this order.

1. **Meaning.** What things are called, which capabilities are worth being tools, what a field's unit is when the system does not say, what counts as the same record.
2. **Writes.** Everything about them: the operation, its identity strategy, its read-back rule, its effect class, and above all where a test write is allowed. The developer declares test records ([section 403](403-testing-and-publish-gates.md)). Without one, a write can be drafted and can never be verified.
3. **Open questions.** Whatever the scout could not decide is in the unknowns ledger, each with the observation that would settle it. `unknown.settle` closes one with evidence.

An agent is well suited to all three, and the product gives it the same tools a person has: the report, the findings, the drafts, the sandbox. The agent works at build time. Nothing it writes runs until it has passed the compiler and the gates.

## What only a person does

| Act | Why it cannot be handed to an agent |
| --- | --- |
| Saying that a system may be inspected and wrapped | It is a statement of right, and the agent may have been steered |
| Signing in, and supplying a credential | A secret never reaches a caller ([principle 10](../1-product/103-design-principles.md)). The person signs in to the system; the vault holds the rest |
| Declaring a test record on a real system | It says where damage is acceptable |
| Approving a write, and raising what a tool is allowed to do | [Section 212](../2-kernel/212-policy-and-approvals.md) |
| Switching on a provider that starts a process or reaches a network | A protected setting ([section 203](../2-kernel/203-the-provider-contract.md)) |
| Publishing tools for other people to use | [Section 214](../2-kernel/214-access-control-toolsets-and-tenancy.md) |

On one machine, with one user, these are light: a prompt from the operating system, a sign-in window. They are never absent.

## What the runtime assumes

| It assumes | It does not assume |
| --- | --- |
| The system has at least one surface that the user is entitled to use and that a pack can reach | Source code, a vendor's help, a published interface, or a stable one |
| Reading is repeatable, or the system says when it is not | That a read is free: reads are rate-limited and counted |
| A write is unsafe to repeat until an identity strategy proves otherwise | That a system forgives a duplicate |
| The machine it runs on belongs to the user, with a key store and a browser already there | A cloud service, an account with us, or a network beyond the target |
| Someone, a person or an agent, exercises judgment at build time | That an agent is present. Every file can be written by hand |
| A person can be reached when authority is needed | That the person is watching. Work that needs them stops and waits |
| Whatever calls the tools may have been steered ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)) | Any particular client, model, or vendor. A client is anything that speaks the protocol |

## What is left out on purpose

Each line carries its kind, as [SCOPE.md](../../../SCOPE.md) does: a rule (one of the ten rules in AGENTS.md, or an owner decision), a choice (a design choice in force today), or not yet. A line here describes where the product stands; it is not by itself a reason to refuse an idea ([decision 0230](../../decisions/0230-the-scope-is-written-down-on-both-sides-and-describes-without-ruling.md)).

| Left out | Because | Kind |
| --- | --- | --- |
| A model inside the product, or an agent framework | Principles 3 and 4 of [section 103](../1-product/103-design-principles.md). The developer brings their own agent, of any make | Rule 1, for a model in a run. Choice, for an agent framework |
| A catalog of definitions for real products | [Record 0058](../../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md). A target is a folder the developer keeps | Rule 9 and owner decision 0058 |
| Getting past a sign-in, a licence check, a bot defence, or a device's protection | Principle 9. A block is a named stop | Rule 5 |
| A free-form shell, and clicking by position on a picture of the screen | Neither can be verified ([section 213](../2-kernel/213-effects-hazards-and-consent.md)) | Choice, for the shell. Rules 1 and 7, for clicking by position; a read-only form is a question open for the owner ([decision 0212](../../decisions/0212-whether-an-application-that-offers-only-pixels-is-ever-reached.md)) |
| A hosted place to share target files | The product runs where the developer runs it | Owner decision 0058 |
| Guessing | Principle 11. Unknown means stop | Rule 7 |

## Settings

Scouting is configured in the `authoring` group ([section 226](../2-kernel/226-configuration.md)). The keys are declared in the settings schema of `a2m-actions`, where an unknown key is refused.

| Key | Values | Default |
| --- | --- | --- |
| `authoring.scout.run` | `on_request`: only when `scout.run` is called. `on_add`: also when a target is added | `on_add` in the `solo` and `workstation` profiles; `on_request` in the others |
| `authoring.scout.try_reads` | Whether stage 6 runs | On |
| `authoring.scout.budget` | Limits on time, requests, and depth for one scout | Small enough that a scout of an ordinary system ends within minutes |
| `authoring.scout.providers` | Which optional discoverers the scout may use, by name | None. The first-party discoverers are always available |

No setting lets the scout write, try a write, or skip consent.

## Open points

- The exact drafting rules for each pack. They are written when the pack is, and each is a small table from finding kind to file kind.
- How mechanical names are made so that they are stable across two scouts of the same system.
- Whether following plain links in an interface is always a read. Some web applications change state on a plain request; the web pack's discoverer must refuse a link whose address looks like an action, and what "looks like" means is to be settled in Phase 4.
- Whether stage 6 should compare across sessions as well as across runs.
- The size of the default budget. It is a target until the first scout of the test application is measured.
