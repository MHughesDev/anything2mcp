# 0300. Publishing always needs a person, and the short path to a working read is the sandbox

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** [the build plan](../plan/build-order.md) and the [roadmap](../plan/roadmap.md): package S1 is closed by this record, S2 is restated, and packages V1 to V5 are new. No code changes with this record.

## Context

The owner agreed that a read-only tool should take about three commands, and that a lighter publish for a version that holds only reads should be proposed. Publishing needs a person to pass the operating system's presence prompt, which is proven on Linux only and cannot be passed in a container or over a remote shell.

The lead drafted the proposal on this reasoning: an agent can already try any drafted read with no person present, so the prompt at publish does not guard what data an agent can reach. An unfriendly review of the draft, against the code, showed the reasoning to be false.

## What the review showed

A trial and a published read are not the same act.

- **A trial runs in the sandbox posture, and a published read runs live.** Every driver's guard against writing is keyed on the posture. In a trial the HTTP driver sends nothing that may write, the command driver runs only a program's declared help and version forms, the data driver cannot write, and the web driver refuses every set and invoke. Live, those guards are off.
- **A published read reaches more.** Any declared environment, not only the default; rows from a source file; shadow bindings; any caller with the viewer's role, where a trial needs the builder's.
- **A stored sign-in is used and renewed only for what is published.** A version carries its sign-in file and its surfaces' addresses. A version of reads published by an agent could move a token's address, and the keeper would send the stored secret there and renew it there.
- **A published tool is listed to every client, with its description as written.** A description an agent wrote reaches other agents' tool lists, where clients read it as documentation.
- **Nothing reads the evidence level.** Trust in the code is keyed on the word "published", so publishing at a lower level would have guarded nothing.

## Decision

1. **Every publish needs a person at the presence prompt,** whatever the version holds.
2. **The short path is the sandbox.** `add` carries a system to drafted reads that have passed their trial, and reports them. An agent with the build toolset calls any of them at once through the trial action. That is a working read in three commands with no person, and it trusts nothing new: the sandbox posture is what makes a trial safe, and it stays on.
3. **Publishing is the step that makes a tool live:** run without the sandbox's guards, in any declared environment, signed in for, and listed to every client. A person does it, having seen what the tools will do.
4. **What the review found beside the question becomes work,** as packages V1 to V5 in the roadmap:
   - V1. Something must read the evidence level, or the level must stop being described as deciding anything.
   - V2. Only two publish gates are built, and the offline one replays fixtures the builder wrote. Say so wherever the gates are described, and build the gate that a builder cannot satisfy alone.
   - V3. A web read binding that sets a field or invokes a control the pack does not lower as a commit: prove the compiler refuses it, or make it.
   - V4. The help text of `tool.call` names the first production environment; the code uses the target's default environment. Make the two agree.
   - V5. A tool's description reaches other clients' tool lists as written. Decide what a published description may hold and who wrote it.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Publish a version of reads with no person (the draft) | It lets a steered agent publish what is in effect a write, send a stored secret to an address it chose, and plant text in every client's tool list |
| A narrower version: reads over HTTP drafted by rule, never edited by hand, over surfaces and sign-ins a person already approved, run for ever in the sandbox posture | Sound, and it is point 2 by another road: a read that runs in the sandbox posture and that only its builder calls is a trial. Listing such reads to other clients is the part that needs a person |
| Wait for the prompt on Windows and macOS and change nothing | The prompt is not the cost. The cost was treating publishing as the only way to use a read |

## Revisit if

The owner wants reads listed to other clients without a person. Then the narrower version above is the place to start, with a posture that stays read-only for the life of the version and a record of what "published" means.
