# 401. Overlaying tools on an existing system

This is the method, end to end: how an existing system that knows nothing of us comes to have verified tools laid over it. It is the same for every kind of system. Each pack fills in what its kind needs: what a surface is, what can be observed, what a draft looks like, and what "cannot write" means for it. Several techniques here are taken from how the REA project observes applications ([research note 16](../../research/16-rea-methods.md)). One thing the method adds that REA does not have is the point of the whole design: every write is checked against the system itself.

Anything2MCP supplies the evidence and the guardrails. Whoever builds the tools, a developer or an agent working for them, supplies the reasoning. No model provider is built in, and nothing a builder writes runs until it has passed the compiler and the gates.

## The method

| # | Step | What happens | Ends with |
| --- | --- | --- | --- |
| 1 | Register | The target is named, its surfaces are declared, and on a server the record of who agreed to it being wrapped is attached | A target with no tools |
| 2 | Attach | The probe looks at each surface and says what kind it is, which pack serves it, and how sign-in works | Surfaces, each with a pack and a sign-in method |
| 3 | Observe | Discoverers explore the system at rest and read what it publishes about itself, and a person's own use of it may be captured. Discovery writes nothing | Findings, `discovered` or `inferred` ([section 218](../2-kernel/218-discovery-and-findings.md)) |
| 4 | Model | The target's files are written from the findings: screens, endpoints, commands, schemas. The drafter writes what follows by rule; an agent or a person writes the rest | Drafts at `reconstructed` |
| 5 | Bind | Each business action is defined once as an operation, with an identity strategy and a read-back rule, and bound to the surfaces that can carry it out | Operations with bindings |
| 6 | Verify | Fixtures, the sandbox, test records, and the publish gates ([section 403](403-testing-and-publish-gates.md)) | A version at `behavior_verified` |
| 7 | Publish | A person reads the review summary, ordered by risk, and publishes | Tools at `approved` |
| 8 | Watch | Canaries, the target's identity, and drift detection. A change sends the affected bindings back to step 6 | Tools that stay true, or stop |

Steps 3 to 5 may use any help, an AI agent included. From step 6 on, nothing improvises. The levels are those of the evidence ladder ([section 219](../2-kernel/219-the-evidence-ladder.md)).

The scout runs steps 2 and 3, the mechanical part of step 4, and the read half of step 6 as one fixed pipeline, with no model in it. [Section 402](402-the-scout-and-who-does-what.md) says what it does, what it leaves to the developer, and what only a person may do.

## The actions a builder uses

Every step is done through authoring actions. Each action is defined once in the action registry and is a command, an MCP tool, a REST route, and an SDK method; commands and files are the primary interface and MCP is the second door ([section 220](../2-kernel/220-actions-and-the-authoring-interface.md)). The [action catalog](../../reference/action-catalog.md) lists every action and its faces.

| Step | Action | What it gives the builder |
| --- | --- | --- |
| Register | `target.add` | A target in the project folder, with its declared surfaces |
| Attach | `target.probe` | For each surface: its kind, the pack that serves it, its platform where the pack knows platforms, and surfaces the target did not declare and plainly offers |
| Attach | `auth.detect` | The sign-in method, with evidence, and a recommended identity mode |
| Observe | `discover.run`, `finding.list` | Every suitable discoverer run within its bounds, and the findings it returned |
| Observe | `screen.snapshot` | A compact snapshot of the current screen of an interface, with element references |
| Observe | `capture.start`, `capture.stop` | A recording of a person using the system: actions, exchanges, and responses, as shapes, with secrets already removed |
| Observe | `endpoints.infer` | Endpoint templates, schemas, the tokens and identifiers a request depends on, and a map from typed values to request fields |
| Observe | `spec.import` | Findings from a description the system publishes, such as an OpenAPI, GraphQL, or OData description, or an upstream MCP server's tool list |
| Model | `draft.write`, `draft.read`, `draft.lint` | Target files written after schema validation and linting; the drafts as they stand |
| Model | `unknown.list`, `unknown.settle` | The open questions, and closing one with evidence |
| Verify | `draft.try` | A draft binding run in the sandbox, below |
| Verify | `test.run`, `fixture.record` | The publish gates, with their evidence; fixtures for offline replay |
| Verify | `shadow.report` | How often a shadow binding matched the verified one, and where it differed |
| Publish | `publish.request` | A version submitted for a person's review |
| All | `scout.run`, `scout.report` | Steps 2 to 6 for reads, as one pipeline, and its report |

The build loop, by hand or by agent: probe, detect sign-in, sign in, observe, infer, draft, try, test, request publish. The person's own steps are those [section 402](402-the-scout-and-who-does-what.md) lists as authority; at the least, signing in and publishing.

## Observing: exploring, and capturing a person's use

Observation comes in two forms that must not be confused, because only one of them can change the system.

| | Exploring | Capturing a person's use |
| --- | --- | --- |
| Who acts | A discoverer, by itself, or the scout | A person, using the system as they normally would |
| Started by | `discover.run`, `target.probe`, `screen.snapshot`, `scout.run` | `capture.start` and `capture.stop`, only by the person whose use it records |
| Can the system change | No. A write is impossible, by the rules below | Yes. The person may save. That save is the person's act; the discoverer only listens |
| Consent | The target's consent to be inspected ([section 213](../2-kernel/213-effects-hazards-and-consent.md)) | That person's, each time: they start and stop it, and are told what is kept |

The scout never captures. A capture is never started on a person's behalf, by an agent or by a setting. In the web pack, for example, exploring uses an observing tab that cannot act, and a capture uses a watching tab in a visible window that the person drives ([section 301](../3-packs/301-the-web-pack.md)).

### What "cannot write" means

The kernel's half is the intent token. Discovery and drafts run in the engine's sandbox. The sandbox asks for an intent only when a record's key is one of the target's declared test records, and a driver's handle refuses every step that commits without the token. A discovery job declares no test records, so through it a write is impossible. A discoverer is given only what the sandbox offers: no driver, no ledger token, and no way to act ([`a2m-engine`](../../architecture/crates/a2m-engine.md)).

The other half is what a step that commits nothing may still do to a system of its kind. That is stated per archetype by each pack, and proven by the pack's conformance test against its test stand.

| Archetype | What the pack guarantees while exploring and in the sandbox |
| --- | --- |
| Interface | Only plain navigation within the declared surfaces; no control whose role or name means committing, such as save, submit, delete, approve, send, or pay, is ever activated; requests other than reads are blocked before they leave, unless a person marked one safe, and each blocked request is logged |
| Request | Only methods the service's own description marks safe, or read endpoints a person marked safe |
| Request, for a data store | A statement runs in a read-only transaction, so the store itself refuses a write |
| Command | A program is started only in the help and version forms its target declares |
| Artifact | A file is opened for reading only, and never replaced |
| Stream | A queue is never consumed and nothing is published; a sample is read without acknowledging |
| Device | Only what the device lists as reads, and the requests that ask it to identify and list itself |

Exploration also stays inside the target's declared surfaces and within its budget of time, size, depth, and requests. Each pack's section gives its own detail.

### One scenario per operation

The unit of observation is a scenario: a short, declared sequence of steps that exercises one thing, such as "change the length of a line item and save". The idea of a declarative scenario with stable step names, and events attributed to each step, is REA's.

- **Stable step names.** Every event captured is attributed to the step during which it happened. "The save request" then means the request seen during the step named `save`, not a guess from timing.
- **A fixed environment.** The target's declared environment is applied, so that two captures of the same scenario can be compared. For an interface that is the viewport, locale, time zone, and motion settings; each pack says what its kind has.
- **A save is a person's.** Exploring cannot carry out a step that commits. A scenario that includes one is carried out by a person during a capture.
- **Shape by default, values on request.** A capture keeps the shape of each exchange: names and types, not values. Values are kept only when a person asks for them and policy allows it. Credentials are removed by structure, not by search, and declared secrets become references.
- **Bounds, counted.** Limits on events, bytes, and exchanges. What was dropped is counted and reported, never silent.

## From observation to model

| Technique | Rule | Level of the result |
| --- | --- | --- |
| Group requests into endpoints | By method, origin, and a path template | The endpoint is `discovered`; its template is `inferred` |
| Template a path, first tier | A segment becomes a parameter only when it is plainly an identifier by format: a number, a universally unique identifier, a long hexadecimal string, a date | `inferred`, with every literal that was seen kept beside it |
| Template a path, second tier | A segment that varied between otherwise identical requests. Switched on by request | `inferred`, and marked as resting on variance alone |
| Bind a request to an interface action | The action's confirmation names an endpoint; the match is by method and path, and an ambiguous match fails | As strong as the scenario that showed it |
| Infer a record's key and its read view | From reads seen before the first write. **A model is kept only if replaying it against the recordings gives the recorded answers** | `inferred` until then; `schema_verified` after |
| Join described with observed | The comparison of [section 218](../2-kernel/218-discovery-and-findings.md) | Lists what is still unexercised, and what was seen and is nowhere described |

Three things are always listed as unknown until evidence settles them, because an observation cannot show them: who is allowed to do the action, what the response means beyond its shape, and what state changes follow. They go into the unknowns ledger.

## A faster binding earns its place

A system with no documented interface can still gain a faster binding than its interface. The web pack's endpoint mining is the first case; any pack with two modes on one target can use the same steps.

1. **Observe** the exchanges made while the slower surface is used normally.
2. **Infer** endpoints, by the techniques above, and match typed values to the fields of each request.
3. **Draft** the faster binding beside the slower one for the same operation.
4. **Shadow.** While the verified binding does the real work, the new binding predicts what it would send. Matches and differences are recorded, and `shadow.report` shows them ([decision 0007](../../decisions/0007-new-bindings-run-in-shadow-before-promotion.md)).
5. **Promote** with `binding.promote`, after a set number of exact matches and a person's approval. Reads can be promoted quickly; writes need more evidence.

Mining can stop short. If the system computes values before saving, or signs its requests, in a way the new binding cannot reproduce, the binding stays in shadow or is marked unsupported, and the slower binding remains the way in. Writes go through the interface until a faster binding is proven ([decision 0013](../../decisions/0013-writes-go-through-the-interface-until-a-faster-binding-is-proven.md)).

## A gate must be able to fail

A test that passes proves little unless it would have failed. Before a write binding can reach `behavior_verified`, its check is shown to behave on four counts. The pattern is taken from REA's "witness" and fits the control interface of a test stand exactly.

| The check must | Shows |
| --- | --- |
| Fail on the seed data, before the write | It is not passing by accident |
| Pass after the write | The write did what was claimed |
| Fail after a near miss: a write that is almost right, such as the neighboring record or a value one step off | It tells right from nearly right |
| Find the seed again after restore | The test left nothing behind |

## Targets with several layers

A real system is reachable at several levels at once: an interface, a backend, a database, an administration tool, a device behind a controller. They are one target with several surfaces, as the target files allow ([section 205](../2-kernel/205-target-files.md)).

**Parts.** A large target declares parts. Each part has its own surfaces, its own drift risk, and where needed its own authorization, and tool names carry the part as their module.

**Where a surface stands.** A surface that holds a system's data, such as a database or a file, declares whether it stands alone, as the system of record, or beneath the surface of an application that owns it. A surface that says neither is unknown: its reads compile and its writes do not. The data and file packs give the field ([section 307](../3-packs/307-the-data-pack.md), [section 308](../3-packs/308-the-file-pack.md)).

**Which level a tool uses.** Four rules.

1. **A write goes through the highest level that applies the system's own rules.** Usually that is the interface or the documented backend. This is [decision 0013](../../decisions/0013-writes-go-through-the-interface-until-a-faster-binding-is-proven.md), stated for every kind of system.
2. **A read may use any level the identity is allowed to read.** Lower levels are often faster and plainer.
3. **Read-back should use a different surface from the write where one exists.** Writing through the interface and reading back from the backend, or from the database, catches what the interface alone would hide. Hypothesis H-30 measures how often.
4. **A write beneath the system's own rules needs the system owner's word.** Writing straight to a database an application owns, or to a device behind its controller, skips validation that exists for a reason. Such a binding compiles only when the target's authorization record names the route as supported by the system's owner. It is a hazard, it is shown first in the review summary, and it is `irreversible` unless its read-back uses the system's own surface. The authorization is a stored record of who agreed and when; it is never inferred from the fact that the identity can write.

**A level is not a privilege.** Reaching a lower level never widens what the identity may do. The session used at each level is one the person was given for that level.

**Another machine** is reached through a runner on that machine, never through a remote login that hands a remote shell a single line ([section 223](../2-kernel/223-the-runner-protocol.md)).

## The sandbox, and what a builder may do

The sandbox is where a draft runs before it is a version: `draft.try`, the drafted reads the scout tries, and every discovery job.

- **Reads freely, writes only to declared test records.** A draft may read whatever its identity may read. It may write only to a record the developer declared as a test record ([section 403](403-testing-and-publish-gates.md)). Declaring one on a real system is a person's act ([section 402](402-the-scout-and-who-does-what.md)). Without one, a write can be drafted and can never be verified.
- **A try is not a run.** The sandbox records its runs as tries, so no binding's health or shadow count changes. Drivers are prepared in a posture other than live, so each pack's own read-only guard is on. The one exception is the driver context in which a declared test record is written: it is prepared for that write alone, and in it a pack lets through the step that commits and nothing else that changes the system.
- **A draft cannot widen a target's reach by itself.** Adding a surface or a destination outside what a person registered for the target needs that person's confirmation ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)).
- **Bounded.** Every try and every discovery has limits on time, size, and depth, and reports which it hit.

**What a builder never sees.** Cookies, tokens, and passwords are removed from every snapshot, capture, finding, and trace before a builder receives them. The builder works with the shape of what the system says, not the secrets in it. Fields whose sensitivity label is restricted are left out of what is given to a building agent, and text that came from the system is marked as untrusted ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)).

## Watching for change

A published tool describes a system at one moment.

- **Identity at every start.** The target's identity is taken again when a session starts and at each canary: the version it reports, and the digests of what can be digested. A difference is not a failure; it lowers the level of the bindings on that surface and schedules their gates ([section 219](../2-kernel/219-the-evidence-ladder.md)).
- **Comparing versions.** Two observations of the same target, old and new, are compared part by part: unchanged, added, removed, changed, or unknown. "Removed" is reported only when the new observation's coverage is complete.
- **What a system declares for agents,** such as tools a web site publishes for agents, is read as a signal of change and as a finding, never as something to call during discovery, and always as text from an untrusted source.

## What an overlay never does

- Change the system to suit us. The one exception is a bridge a person installs knowingly ([section 204](../2-kernel/204-supervising-subsystems.md)).
- Hold the system's data beyond what evidence needs, for longer than its retention class.
- Offer a tool the evidence does not support, however confident the description sounds.

## Open points

- The file format of a scenario, and whether it is the same file as a live test in `tests/`. Probably yes. It is designed with the first discoverer.
- Whether a scenario may be replayed in the sandbox against a declared test record, so that a save in it can be repeated without a person.
- How parts are declared in `target.yaml` beyond today's list of module names.
- Whether cross-surface read-back should become the default wherever two surfaces exist, once H-30 has a number.
