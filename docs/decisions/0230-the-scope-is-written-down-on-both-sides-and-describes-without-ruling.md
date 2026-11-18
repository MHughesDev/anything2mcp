# 0230. The scope is written down on both sides, and it describes without ruling

**Status:** Accepted. Owner decision of 2026-11-18. The direction is the owner's; the wording is the lead agent's, as the owner asked ([decision 0231](0231-the-owners-direction-is-written-in-exact-words-of-the-agents-own.md)). Only the owner changes it. The lead agent first drafted this record as a list of things the product "does not answer for", each to be changed only by a superseding record. The owner read that and turned it down in that form; this is the record as the owner decided it. The earlier draft was never in force and was never merged.

**Date:** 2026-11-18

## Context

The specification says in several places what the product refuses to do ([section 101](../spec/1-product/101-summary.md), [section 402](../spec/4-building-tools/402-the-scout-and-who-does-what.md)), and the lead agent proposed gathering "what the product is not responsible for" into one decision. The danger in that is real: a list of things the product is not, once written as a decision, starts to act as a rule. An agent that finds "the product is not X" in a record will refuse an idea that would make it X, and the product narrows by accretion without the owner ever having chosen it.

## Decision

The scope is to be visible, whole, and honest in both directions: what the product does and takes responsibility for, and what it does not. A reader, the owner first of all, should be able to see the edge of the product on one page.

And that page must never harden into a fence. Saying what the product is not, today, is a description of today. It is not a promise to stay that way, and it must not be read as one. The owner does not read every document as it is written; a product that narrows a little with each sentence of the form "this is not that" would end up smaller than the owner ever chose, and the owner would find out only on wanting the thing that had been written away.

Three things follow.

1. **One page shows the scope, both sides.** [SCOPE.md](../../SCOPE.md), at the root, says for each area what the product does and answers for, and what it does not. It is kept true to the code and the decisions, like every document.
2. **That page describes; it does not rule.** A line that says "does not" records where the product stands today and why. It is never by itself a reason to refuse an idea, and changing it needs no record that supersedes it: when the product grows, the line is edited in the same change. The only things that bind are the ten rules in [AGENTS.md](../../AGENTS.md) and the owner's decisions.
3. **Every "does not" says what kind it is**, so the owner can see at a glance what is firm and what is only how things are today:
   - **Rule**: it follows from one of the ten rules or an owner decision. Only the owner changes those, and the page names which one.
   - **Choice**: a design choice in force today. Any agent may change it with evidence, in the ordinary way.
   - **Not yet**: nothing stands against it; it is not built.

## What this asks of agents

- **Do not refuse an idea because a document says the product "is not" that.** Find out which kind of line it is. If it is a rule, write a Proposed record for the owner and carry on with other work. If it is a choice or a "not yet", weigh the idea on its merits.
- **Do not write new "is not" statements as if they were rules.** In a spec section, a decision record, or a README, a limit is stated with its reason and its kind. A design decision that narrows what the product could later become says so plainly, so that the owner can see the narrowing when they read it.
- **Say both sides.** A document that says what the product does also says, near it, what it does not, and the reverse.
- When SCOPE.md and another document disagree, one of them is out of date: fix it in the same change, as [the working method](../plan/working-method.md) says for any two documents.

## Consequences

- SCOPE.md is written with this record and linked from the README and the documentation index.
- The six questions the earlier draft raised as choices (whether to report a weakness the product meets on its way, whether to mark or filter what a system said, how much the ledger keeps, and the others) are recorded in SCOPE.md as choices, with the reason for each. None is an owner decision, and none is closed.
- The existing "does not" tables in sections 101 and 402 stay as they are; the cleanup round now under way is asked to mark each of their lines with its kind where it is not obvious.

## Alternatives weighed

- **A decision listing what the product is not responsible for, each line superseded only by a new record.** This was the lead agent's draft. The owner turned it down for the reason in the context above.
- **No statement of scope at all.** Turned down by the owner: both sides are to be visible.
