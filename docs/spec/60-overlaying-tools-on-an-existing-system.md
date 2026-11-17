# 60. Overlaying tools on an existing system

This is the method, end to end: how an existing system that knows nothing of us comes to have verified tools laid over it. It generalizes the authoring loop of [section 26](26-agent-toolkit-how-tools-get-built.md) from applications to every kind of system, and it takes several techniques from how the REA project observes applications ([research note 16](../research/16-rea-methods.md)). One thing it adds that REA does not have is the point of the whole design: every write is checked against the system itself.

## The method

| # | Step | What happens | Ends with |
| --- | --- | --- | --- |
| 1 | Register | The target is named, its surfaces are declared, and on a server the record of who agreed to it being wrapped is attached | A target with no tools |
| 2 | Attach | The probe looks at each surface and says what kind it is, which pack serves it, and how sign-in works | Surfaces, each with a pack and a sign-in method |
| 3 | Observe | Discoverers watch the system, in use or at rest, and read what it publishes about itself. Nothing is written | Findings, `discovered` or `inferred` ([section 58](58-discovery-and-findings.md)) |
| 4 | Model | An agent or a person writes the target's files from the findings: screens, endpoints, commands, schemas | Drafts at `reconstructed` |
| 5 | Bind | Each business action is defined once as an operation, with an identity strategy and a read-back rule, and bound to the surfaces that can carry it out | Operations with bindings |
| 6 | Verify | Fixtures, the sandbox, test records, and the publish gates | A version at `behavior_verified` |
| 7 | Publish | A person reads the review summary, ordered by risk, and publishes | Tools at `approved` |
| 8 | Watch | Canaries, the target's identity, and drift detection. A change sends the affected bindings back to step 6 | Tools that stay true, or stop |

Steps 3 to 5 may use any help, an AI agent included. From step 6 on, nothing improvises.

## Observing: one scenario per operation

The unit of observation is a scenario: a short, declared sequence of steps that exercises one thing, such as "change the length of a line item and save". The idea of a declarative scenario with stable step names, and events attributed to each step, is REA's.

- **Stable step names.** Every event captured is attributed to the step during which it happened. "The save request" then means the request seen during the step named `save`, not a guess from timing.
- **A fixed environment.** Viewport, locale, time zone, and motion settings are the target's declared environment, so two captures of the same scenario can be compared.
- **Shape by default, values on request.** A capture keeps the shape of each request and response: names and types, not values. Bodies are kept only when asked for and allowed. Credential headers and cookies are removed by structure, not by search, and declared secrets become references.
- **Bounds, counted.** Limits on events, bytes, and exchanges. What was dropped is counted and reported, never silent.

## From observation to model

| Technique | Rule | Level of the result |
| --- | --- | --- |
| Group requests into endpoints | By method, origin, and a path template | The endpoint is `discovered`; its template is `inferred` |
| Template a path, first tier | A segment becomes a parameter only when it is plainly an identifier by format: a number, a universally unique identifier, a long hexadecimal string, a date | `inferred`, with every literal that was seen kept beside it |
| Template a path, second tier | A segment that varied between otherwise identical requests. Switched on by request | `inferred`, and marked as resting on variance alone |
| Bind a request to an interface action | The action's confirmation names an endpoint; the match is by method and path, and an ambiguous match fails | As strong as the scenario that showed it |
| Infer a record's key and its read view | From reads seen before the first write. **A model is kept only if replaying it against the recordings gives the recorded answers** | `inferred` until then; `schema_verified` after |
| Join described with observed | Section 58's comparison | Lists what is still unexercised, and what was seen and is nowhere described |

Three things are always listed as unknown until evidence settles them, because a capture cannot show them: who is allowed to do the action, what the response means beyond its shape, and what state changes follow. They go into the unknowns ledger.

## A gate must be able to fail

A test that passes proves little unless it would have failed. Before a write binding can reach `behavior_verified`, its check is shown to behave on four counts. The pattern is taken from REA's "witness" and fits the test stand's control interface exactly.

| The check must | Shows |
| --- | --- |
| Fail on the seed data, before the write | It is not passing by accident |
| Pass after the write | The write did what was claimed |
| Fail after a near miss: a write that is almost right, such as the neighboring record or a value one step off | It tells right from nearly right |
| Find the seed again after restore | The test left nothing behind |

## Targets with several layers

A real system is reachable at several levels at once: an interface, a backend, a database, an administration tool, a device behind it. They are one target with several surfaces, as section 7 already allows.

**Parts.** A large target declares parts. Each part has its own surfaces, its own drift risk, and where needed its own authorization, and tool names carry the part as their module.

**Which level a tool uses.** Four rules.

1. **A write goes through the highest level that applies the system's own rules.** Usually that is the interface or the documented backend. This is decision 0013, stated for every kind of system.
2. **A read may use any level the identity is allowed to read.** Lower levels are often faster and plainer.
3. **Read-back should use a different surface from the write where one exists.** Writing through the interface and reading back from the backend, or from the database, catches what the interface alone would hide. Hypothesis H-30 measures how often.
4. **A write beneath the system's own rules needs the system owner's word.** Writing straight to a database an application owns, or to a device behind its controller, skips validation that exists for a reason. Such a binding compiles only when the target's authorization says the route is supported. It is shown first in the review summary, and it is `irreversible` unless the system's own read confirms it.

**A level is not a privilege.** Reaching a lower level never widens what the identity may do. The session used at each level is one the person was given for that level.

## Watching for change

A published tool describes a system at one moment.

- **Identity at every start.** The target's identity is taken again when a session starts and at each canary: the version it reports, and the digests of what can be digested. A difference is not a failure; it lowers the level of the bindings on that surface and schedules their gates ([section 59](59-the-evidence-ladder.md)).
- **Comparing versions.** Two observations of the same target, old and new, are compared part by part: unchanged, added, removed, changed, or unknown. "Removed" is reported only when the new observation's coverage is complete.
- **What a site declares for agents** is read as a signal of change and as a finding, never as something to call during discovery, and always as text from an untrusted source.

## What an overlay never does

- Change the system to suit us. The one exception is a bridge a person installs knowingly ([section 57](57-supervising-subsystems.md)).
- Hold the system's data beyond what evidence needs, for longer than its retention class.
- Offer a tool the evidence does not support, however confident the description sounds.

## Open points

- The file format of a scenario, and whether it is the same file as a live test in `tests/`. Probably yes.
- How parts are declared in `target.yaml` beyond today's list of module names.
- Whether cross-surface read-back should become the default wherever two surfaces exist, once H-30 has a number.
