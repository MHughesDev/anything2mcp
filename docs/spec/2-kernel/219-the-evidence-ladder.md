# 219. The evidence ladder

Finding out that something might work is not proving that it works. Every finding, every draft, and every tool carries one evidence level, and nothing is shown or used at a higher level than its evidence supports. Decision: [record 0051](../../decisions/0051-findings-and-the-evidence-ladder.md). Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

## The levels

| Level | Meaning | What proves it | Who can raise it |
| --- | --- | --- | --- |
| `inferred` | Derived and never observed: from a description, from reading code, from a pattern, from reasoning | Nothing yet. The reasoning and its source are recorded | A discoverer |
| `discovered` | Observed on the live system: seen in a capture, present on a screen, returned by a listing the system computes about itself | The stored observation, naming exactly what was observed | A discoverer |
| `reconstructed` | A draft operation exists, with inputs, outputs, and at least one binding | The draft compiles and passes lint | The compiler |
| `schema_verified` | Real answers from the system matched the declared schemas | Fixtures and a schema check | The test action |
| `executed` | The draft ran in the sandbox against a test record | A step trace | The sandbox |
| `behavior_verified` | The publish gates passed, with ground truth where a test stand can give it | Gate evidence stored with the version | The publish gates |
| `approved` | A person published it, and it is the current version | The publish record | A person, never an agent |

The first two belong to findings ([section 218](218-discovery-and-findings.md)). The rest belong to drafts and versions.

## Rules

1. **A level is earned, never declared.** No file has a field that sets it. The part of the system named in the last column computes it from stored evidence.
2. **No skipping.** A draft reaches `executed` only after `schema_verified`, and so on up.
3. **Only `approved` is listed.** A tool at any lower level is refused to ordinary callers, as drafts already are.
4. **A level can fall.** When the evidence under it stops being true, the level drops to what the remaining evidence supports.
5. **Evidence names its subject.** Each piece of evidence says what it was about: a file by its digest, an address, a process, a session, or a device, with the version of the system where that is known.
6. **Limitations travel with evidence.** What was not looked at, what was cut short, and what could not be read are recorded beside what was found.

## How levels fall

| What happened | Effect |
| --- | --- |
| The system reports a new version, or its identity no longer matches the evidence | Bindings on the changed surface drop to `reconstructed` until the gates pass again; a version already current keeps running until a canary fails |
| A canary or a run reports `drift` | The binding is marked unhealthy, as section 207 says, and its level drops to `reconstructed` |
| A fixture is found to be stale | `schema_verified` is withdrawn for what depended on it |
| A provider's component no longer matches its recorded digest | Findings that came through it are marked as from an unverified component; nothing built on them is raised further until re-observed |

A device is stricter. A device whose firmware version is outside its map stops every run on it with `unknown_variant`, reads included, at prepare and before any frame but identification, because a register's meaning may change with firmware. It does not keep running until a canary fails ([section 310](../3-packs/310-the-device-pack.md), [section 310](../3-packs/310-the-device-pack.md)).

## How this meets what is already designed

| Already designed | Relation |
| --- | --- |
| Binding status: `draft`, `shadow`, `verified`, `retired` | Unchanged. The compiler refuses `verified` on a binding whose version has not reached `behavior_verified` |
| Version stages: published, candidate, current ([section 224](224-when-the-engine-itself-fails.md)) | Unchanged. `approved` is a version that is current |
| The publish gates ([section 403](../4-building-tools/403-testing-and-publish-gates.md)) | They are what moves a version to `behavior_verified` |
| Shadow comparison | A binding in shadow is at `executed` at most until it has matched the verified binding that acts on the set number of distinct inputs, in each environment the evidence must come from, since its last mismatch, counted for it as it is now, by its digest, and a person at the machine promotes it ([decision 0251](../../decisions/0251-the-evidence-for-promoting-a-shadow-binding-says-what-it-covers-and-a-shadow-costs-what-it-says.md)) |
| The first binding of an operation | Made `verified` in the drafts by `binding.promote` on a read's trial at `executed` and the operation's passing offline tests, each of which names the binding it ran and the digest of what it does, and counts only for that binding as it is now; the version it is published in reaches `behavior_verified` through the gates ([decision 0189](../../decisions/0189-a-developers-first-hour-runs-without-hand-steps.md), [decision 0200](../../decisions/0200-evidence-names-its-binding-and-a-recorded-fixture-says-nothing-of-the-original.md)) |
| "Unknown means stop" | A run never acts on a finding. Only a plan runs, and a plan holds only compiled bindings |

## What an agent sees

Every result of a discovery or authoring action states the level of each thing it returns, in the same field, with the same seven words. An agent can ask for everything about a target at or above a level, and for the reason a thing is not higher: the missing evidence, by name.

For example, after capturing a session and reading a published description of the same service:

```text
endpoint  line_item_put    discovered     seen 4 times in capture c_19; in the description
endpoint  line_item_patch  inferred       in the description only; never observed
screen    order.line_item_form  discovered     snapshot s_07
operation example_line_item_upsert  reconstructed  needs: fixtures for schema_verified
```

## What a person sees

The review summary shown before publishing ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)) gains one line per tool: its level, and what its `behavior_verified` rests on. A tool whose gates ran only against fixtures, with no test stand and no test record, says so.

## What this does not do

- It does not measure how likely a thing is to be right. There are no scores. A level says what kind of proof exists.
- It does not make an observation true forever. Rule 4 exists because systems change.
- It does not let a higher level stand in for a person. `approved` is a person's act.

## Open points

- Whether `behavior_verified` should be split by where the gates ran: against fixtures, a test stand, or a test record in the real system.
- How long a `discovered` finding stays `discovered` when nothing re-observes it. A time limit per target's drift risk is the likely answer.
