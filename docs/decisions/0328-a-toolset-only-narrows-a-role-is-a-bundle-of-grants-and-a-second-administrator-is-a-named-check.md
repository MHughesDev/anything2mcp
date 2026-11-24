# 0328. A toolset only narrows, a role is a bundle of grants, and a second administrator is a named check

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Refines [decision 0034](0034-policy-is-a-versioned-tested-artifact-and-every-decision-is-recorded.md) and builds the policy half of package 6.3.

**Date:** 2026-11-23

**Changes:** `a2m-policy`: the layers of a policy set gain toolsets, role grants, and a switch for a second administrator; `publish` gains a reviewed form; the failure code `second_admin_required` is added.

## Context

Section 212 says a lower layer is compiled as a further restriction, that roles are bundles of grants, that publishing can require a second administrator, and that every decision can be explained. Section 214 says a toolset lists who may connect, which identity mode its runs use, and its approval policy. The crate had roles that only gated approving and performing actions, grants that stood alone, and an explanation that named a grant by its position only. Package 6.3 needs the compiled form of all of these before `a2m-actions` can give them commands.

## Decision

1. **A toolset holds no permit.** It is data in the set's layers: a name, who may connect, the tools it selects, the highest effect class of its runs, the identity mode of its runs, and whether a rule may let a write through it go without a person. The decision core consults it as forbids, after the guardrails and before any permit, for `UseTool`, `ActAs`, and `WriteWithoutPerson`, when the request's context names a toolset. It can refuse a caller a grant would admit; it can never admit one a grant does not. So the rule that a lower layer holding a wider permit does not compile holds by construction, and what `compile` checks is the shape: a unique name, someone who may connect, a tool selected, and well-formed patterns.
2. **An unknown toolset stops the call, once a set defines toolsets.** A set that defines none says nothing about toolsets, so the built-in sets and the built-in toolsets of the MCP door are unchanged. This is rule 7 (unknown means stop) applied where it can be applied without breaking a set that has not begun to use toolsets.
3. **A role is a bundle of grants.** A role grant is a grant given to a role. Everyone who holds the role by a role binding holds it, as a permit named `role_grant.<n>`, with the same effect ceiling and identity mode rules as a grant. The roles that only gate approving and performing actions are unchanged.
4. **A second administrator is a named check of `publish`.** A set says `second_admin_publish`. While the set in force says it, `publish_reviewed` needs a reviewer who is a person, is not the publisher, did not draft the version, and holds the administrator or publisher role in the set in force; an agent can neither publish nor review. Switching the need off is a loosening and also needs a person present. The first set that turns it on needs no reviewer, because nothing had asked for one. A failure is `policy`/`second_admin_required`. `publish` is `publish_reviewed` with no reviewer. This is the guardrail "the author of a version cannot be its only publisher" and the one "an agent cannot approve a publish" in the form the crate can hold today: the facts about the author come in with the call, not from the policy engine.
5. **A change to one grant is a new version.** `publish_change` reads the set in force, adds or removes one grant or one toolset, and publishes the result with a test table the caller brings, because a set's tests are stored and hashed but never read back into a request. The same tests, presence, and review rules apply.
6. **An explanation names the entry that decided.** A rule of a set is described with who holds it and what it covers (`Grant 2 gives the principal ... every tool of the target ...`), and each explained rule carries its kind. It never prints a pre-approval rule's condition, a credential, or any field of the request beyond what the decision record states.
7. **Questions over a catalog.** `tools_allowed`, `who_can`, and `toolset_tools` ask `would` over a catalog the caller supplies and record nothing.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A toolset as a layer with its own grants, compiled and checked against its parent | The spec's refusal of a wider permit is then a check that can be wrong. With no permit in the layer there is nothing to check |
| A toolset not defined means no limit | A mistyped toolset name would then widen a call; unknown means stop |
| A role required for every use of a tool | It would change every set already published and the built-in sets; role grants add the bundle without that |
| Add the second administrator to `PublishProof` | A field there breaks every caller that builds the proof by its fields; a second function does not |
| Read the author from the store | The store does not know who drafted a version; a draft is not a stored thing in this crate |

## Proof

In `a2m-policy`: tests that a toolset refuses a tool it does not select, a caller it does not list, an effect above its ceiling, and another identity mode, and cannot admit a caller no grant covers; that an unknown toolset is refused; that a toolset can make a write wait for a person; that a role grant admits the role's holders to its ceiling only; that a set needing two administrators is not published by one, by the author, by an agent, or by a person with no role, and is published with one; that a change to one grant is tested, published, listed in the history, and removable; and that an explanation names the grant, the role grant, the toolset, and the pre-approval rule, and prints no condition.

## What this leaves open

The layers of a workspace and of target files, which narrow in the same way. The commands, tools, routes, and SDK methods for all of this, which `a2m-actions` generates from its registry.
