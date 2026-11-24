# H-11: Cedar fits the size and speed budgets and expresses all seven policy questions cleanly

**Status:** changed

**Tested:** 2026-11-17, research brief 31, written up as [decision 0082](../decisions/0082-the-decision-core-is-the-policy-crates-own-evaluator.md). This file was written on 2026-11-24 from that record's numbers; nothing was measured again for it.

## What was assumed

Cedar fits the size and speed budgets and expresses all seven policy questions cleanly. The alternative was that a small in-house matcher is enough and lighter.

## What was done

A spike linked Cedar into an empty program and compared it with an evaluator written in the policy crate, on licence, crates added, code that is not Rust, binary size, and the time of one decision in a release build. The table is in decision 0082.

## What was found

| Measure | Cedar | The policy crate's own evaluator |
| --- | --- | --- |
| One decision, release build | 1.9 microseconds, two policies | 0.07 microseconds, over 1,000 generated requests |
| Binary size, stripped, against an empty program of 0.3 MB | 5.8 MB | Not measurable apart from the crate |
| Crates added | 107 compiled, 69 linked | None |
| Licence of the tree against the closed list in `deny.toml` | Fails: one build dependency is under an expression not on the list | Nothing added |

Both are far inside the budget of 100 microseconds for a decision, so the half of the bet about speed held. The half about size did not, and the tree does not pass the closed licence list. Whether Cedar expresses the seven questions cleanly was not what decided it and is not answered here.

## What changes

Decision 0082 supersedes [decision 0033](../decisions/0033-policy-decisions-use-cedar-with-grants-compiled-to-it.md): the decision core is the policy crate's own evaluator. Nothing further changes with this file.

## What is still unknown

What is lost without Cedar's analysis tools when policies grow past fixed forbids and rows of permits; decision 0082 names it as the cost. No hypothesis is registered for it.
