# 0307. What the evidence level is for, and what a review shows about a tool's evidence

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Decides packages V1 to V5, which [decision 0300](0300-publishing-always-needs-a-person-and-the-short-path-is-the-sandbox.md) left open. Point 2 is superseded, and points 1 and 3 are corrected, by [decision 0311](0311-the-review-shows-from-the-ledger-what-this-engine-proved.md).

**Date:** 2026-11-23

**Changes:** the fixture's form and the publish review in `a2m-actions`, the plan's check of a read in `a2m-files` and `a2m-provider`, the help of `tool.call`, what `a2m-mcp` sends with a tool, the documents that say the evidence level decides.

## Context

The review of the short path found five places where the documents promise more than the code does. The evidence level is compared in the code only where a finding is checked against its source; nothing decides whether a tool may be listed or published by it. Ten of the twelve publish gates are not built, and the two that are can be passed by fixtures the builder wrote. The web pack's lowering refuses a read that sets or invokes, and nothing in the kernel does. The help of `tool.call` does not say which environment a call runs in. And nothing bounds a published description or tells a client whose words it is.

## Decision

1. **The evidence level informs a person and decides nothing in a run** (V1). It says how far a finding or a tool has been proven. The two checks there are stay: a finding may not claim more than its source can give. The documents that say a level gates listing or a call are corrected. One new use is added, in point 2.
2. **A fixture says where it came from, and the engine writes that line** (V2). `recorded_from` is `trial` when the engine recorded the fixture from a trial against the system, and `written` otherwise; a builder's file cannot set `trial`, because the engine signs a recorded fixture with the digest of the trial it came from and checks it at the gate. A tool whose passing evidence is only `written` fixtures reaches `schema_verified` and no higher; one with a recorded fixture reaches `executed`. The publish review shows each tool's level and names every tool below `executed` first. Publishing it is still the person's choice.
3. **The review states which gates ran.** Each gate is shown as passed, failed, or not built, from the one table the gate runner holds. No document lists the gates a second time.
4. **The kernel refuses a read that acts** (V3). A pack states, where it registers a step kind, whether a step of that kind can change the system. The compiler refuses a plan whose effect is a read and which holds such a step, whichever pack lowered it. The web pack's own refusal stays as the nearer message.
5. **`tool.call` with no environment runs in the target's default environment, and its help says so** in one sentence (V4). The code already does this.
6. **A tool's description is bounded text, and a client is told whose it is** (V5). It is plain text of at most 1,024 characters, with no control characters; the compiler refuses more. The engine adds nothing to it. With each tool the MCP door sends, in the tool's `_meta`, the target's name, the version, and whether the description was drafted by rule or written by a builder.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Refuse to publish a tool below `executed` | The example target and every offline-only target are built from written fixtures; a person may rightly publish one. The review tells them, and they decide |
| Let a level gate a call at run time | A run would then depend on a judgment made while building, which is the policy engine's place and nobody else's |
| Leave the read check to each pack | Rule 2's first word rests on it, and a pack added later would have to remember |

## Proof

A version whose only evidence is fixtures its builder wrote is shown below `executed` at review, and a fixture edited to say `trial` fails the gate. A hand-built read plan with an acting step fails to compile for a test pack. A description of 1,025 characters is refused. The default environment has a test.
