# 0055. Agents plan, build, and merge without waiting for the owner

**Status:** Accepted. Owner decision.

**Date:** 2026-11-17

**Replaces:** the rules in AGENTS.md, the working method, and three skills that told an agent to wait for the owner's go-ahead, to show a plan first, and not to push unasked. No numbered record held those rules.

## Context

The repository's rules were written for an owner who read every plan and every change. The owner does not work that way and has said so three times: that agents merge their own pull requests; that agents "shouldn't wait for any human approval when we're in this repo" and should "work along in parallel" planning, designing, and building; and that the rule files may be changed as needed to say so. An agent was stopped by its own safety check from making this change without those words. It now has them.

## Decision

1. **An agent does not wait.** It plans, decides, builds, opens its pull request, and merges it when every check is green. The plan and the alternative it weighed are still written down, in the pull request or the journal, because writing them is what makes the work careful. Nobody has to answer them.
2. **The owner is told, not asked.** Every choice between real alternatives is a decision record. Every session ends with a journal entry. A record the owner has not read says so in its status line, and stays in force.
3. **Machines hold the gates.** A change merges only when the build, the lints, the tests, and the documentation audit pass on all three operating systems. An agent never skips, disables, or weakens a check to get there.
4. **Agents work in parallel.** One work package, one branch, one pull request. Two agents do not change the same crate at the same time. An agent that leads others gives each a package with a stated proof, and merges what passes.
5. **Design decisions are open to any agent.** A record marked as a design decision can be superseded by a new record that gives the evidence.
6. **The owner's decisions and the ten rules are the owner's.** A record marked as an owner decision, and the rules that never bend in AGENTS.md, are changed only by the owner. An agent that thinks one is wrong writes a record with status Proposed and carries on with other work; it does not stop and it does not build against the rule.
7. **Three things an agent still does not do alone,** because they act outside this repository and cannot be taken back:
   - touch a real application, a real account, or a real credential (rule 10 of AGENTS.md is unchanged);
   - handle a registry token, or publish by any route other than the release workflow with short-lived credentials;
   - add a dependency whose licence is not on the closed list in `deny.toml` when that licence is not permissive.

## What this does not change

This record is about how the repository is built. The product's own approvals are a feature of the product: a person approving a write at run time, a protected setting, a consent record. They stay out of an agent's reach exactly as designed ([section 18](../archive/spec-2026-11/18-safety-policy-and-approvals.md), [section 39](../archive/spec-2026-11/39-local-engine-and-clients.md), [section 61](../spec/2-kernel/213-effects-hazards-and-consent.md)).

## Consequences

- AGENTS.md, the [working method](../plan/working-method.md), and the six skills are rewritten to match.
- A mistake can now merge without a person having read it. What limits the damage: small changes, tests that ask the test application what it stored, the audit with its planted mistakes, records and journal entries that make every choice findable, and the fact that nothing is published without the release workflow.
- The phrase "the owner has agreed" disappears from task lists. Where a task list said it, the proof is now the passing check.

## Alternatives considered

- Keep the go-ahead for plans and drop it for merges: the owner asked for neither.
- Let agents change anything, including the owner's decisions and the ten rules: fast, and it would let one mistaken session undo the product's safety rules. The owner asked for agents not to wait, not for the rules to be optional.

## Revisit if

Merged work has to be undone often enough that a second reader, human or agent, would have paid for itself.

## Spec sections

- [The working method](../plan/working-method.md)
- [The long-term plan](../archive/plans-2026-11/anything2mcp-master-plan.md)
