# 64. Rules for tools and capability discovery

Two things an agent needs from a universal runtime: to find out what is installed and usable, and to be given tools it can actually use well. This section sets the rules for both. It extends [section 21](21-agent-facing-tool-surface.md) and [section 40](40-authoring-interface.md). Several rules are taken from the REA project's written doctrine for tool design, which is the most careful treatment of the subject we found ([research note 16](../research/16-rea-methods.md)).

## Finding out what is there

| Action | Answers |
| --- | --- |
| `provider.list` | Every pack and provider: installed, enabled, ready, or what it lacks and how to get it |
| `provider.describe` | One provider's manifest: what it supplies, its effects and how strongly each is held, its requirements |
| `provider.enable`, `provider.disable` | Switch one on or off. On one machine, a protected setting where the provider starts processes or reaches a network |
| `component.list` | The outside programs providers need: found or not, at which path, with which digest, known or unverified |
| `discover.run` | Run the discoverers that suit a target's surfaces; returns findings |
| `finding.list`, `finding.describe` | Findings for a target, filtered by level, kind, or source |
| `unknown.list`, `unknown.settle` | The unknowns ledger; settling one requires evidence |
| `doctor.run` | Everything above, checked, with a fix for each problem |

These are ordinary actions: defined once, with a command, an MCP tool, a REST route, and an SDK method generated from the definition.

## When to add an action or a tool

1. **Add one for a distinct outcome or a materially different authority.** Matching an outside interface function for function is not a reason.
2. **One shape per action:** inspect, list or search, trace, compare, observe or capture, change. An action never mixes discovery with change.
3. **Primitives first.** An action reports one reusable fact about one identified thing. A workflow is composed only after repeated use has shown the same sequence. Pipelines are the place for sequences, and they are declared in files.

## Descriptions

- The first 240 characters must be enough to choose the tool: what it does, on what, and what comes back.
- Effects, and the line between this tool and its nearest neighbor, come early.
- A description is written for choosing and for making a valid first call. It is not documentation.
- Text from a target never enters a description without a person reviewing it ([section 51](51-defending-against-a-manipulated-agent.md)).

## Schemas

- Inputs are strict: no unknown fields, the target named explicitly.
- What is advertised in a tool list may be a compact form of the schema, provided every difference is exact or wider. The full schema is what the server validates against, always.
- Output schemas are published and are not sent in a tool list by default. REA measured that complete output schemas were about 92% of its tool list by size. They are returned by `tool.describe`, and in the list for a client that asks.
- One to three example inputs, taken from recorded tests.

## Results

- Short by default: a summary, and a reference to the full result, which is kept.
- One shared way to page through a kept result, by offset, limit, and fields. A tool does not invent its own.
- When a result is cut, it says by which limit, and returns a continuation that is ready to call.
- Every result says how complete it is. Missing coverage is unknown, never absence ([section 58](58-discovery-and-findings.md)).
- Anything that came from a target is marked untrusted.

## Limits

- Every limit is a named setting with a unit.
- A failure caused by a limit names the setting, its value, its bounds, and where it is changed.
- A fixed ceiling says that it is fixed, so that nobody goes looking for a setting.
- An agent cannot raise a protected limit. That is a difference from REA, where limits are editable through a tool.

## Failures

The existing contract stands: a class, a stable code, a plain sentence, and a next step. Three additions.

| Addition | Detail |
| --- | --- |
| Whether a retry is safe | Stated, per failure. Never implied by the class name |
| A fix is an instruction | "The file was not found" describes; "Pass a path inside an allowed root" repairs. Every failure carries the second kind |
| Malformed is not unresolvable | An input that breaks the schema, and a valid input that names something that does not exist, are different failures with different next steps. Only the first suggests reading the schema |

The list of every failure code is generated from the code that classifies them, and the audit script fails when the documents and the code disagree, as it already does for failure classes.

## Progress and cancellation

- Progress is sent only to a caller that asked for it. It is a count that only rises. Percentages are not invented.
- Cancellation is one signal, the same from a command, an MCP client, or a program.
- A cancelled run says what was completed before it stopped, and what cleanup achieved, as separate facts ([section 57](57-supervising-subsystems.md)).

## One definition, and proof of it

The four faces of an action are generated from one definition; that rule is unchanged. Two checks are added.

- **A recorded catalog.** The build writes the catalog of actions and tools with a digest. The engine checks the digest at start, and a test compares the recorded catalog with a live listing through a real client library.
- **Parity.** A test calls each action through each face with the same input and compares the results.

## Found tools become real tools

Section 21 lists few tools and finds the rest through search, calling them through one generic action. Where a client supports notice that the tool list has changed, a tool found by search is added to that client's list for the session, so that the client validates its arguments against its real schema. The generic call action stays for clients that do not.

## Proving that agents can use the tools

The usability gate of Phase 5 gains method from REA's own tests.

- A real agent client, in a throwaway home directory, set up through the public installation path and nothing else.
- Closed questions with one right answer, checked against a fixture and against what the tools actually returned.
- At least one task whose right answer is to call nothing.
- Scores kept separately for choosing the tool, filling the arguments, recovering from a failure, and the final claim. A valid schema and good keywords do not show that a tool will be chosen.

## Open points

- The size budget for a tool list, and for one tool's compact schema.
- Whether findings and unknowns are exposed as MCP resources as well as through actions.
- Which clients support notice of a changed tool list well enough to rely on.
