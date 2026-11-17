---
name: design-review
description: Run one design review round on the Anything2MCP documents through a single lens, to find missing pieces, contradictions, and weaker-than-necessary designs, and fix them. Use at the end of a phase, after five slices without a review, or when asked to audit or improve the architecture.
argument-hint: "[lens number from docs/plan/design-review-lenses.md]"
---

# Design review round

A round improves the documents. It writes no product code.

1. **Choose one lens** from `docs/plan/design-review-lenses.md`. Prefer one not used recently. One lens per round; mixing them finds less.
2. **Read with the question in mind.** Read the sections the lens points at in full, not from memory of them.
3. **List findings before fixing anything.** For each: what is wrong, where, how serious, and whether it is a contradiction, a gap, or a weakness.
4. **Sort each finding:**
   - It can be fixed in the documents now: fix it.
   - It needs evidence: add it to `docs/hypotheses/README.md`.
   - It changes a design decision: write the record that supersedes it, with the evidence.
   - It changes an owner decision or one of the ten rules: write a decision record with status Proposed, and carry on.
   - It needs outside facts: add a brief to `docs/research/11-open-research.md`.
5. **Prefer the smallest fix.** A new section only when no existing one can hold the content. Never rewrite an old decision record; supersede it.
6. **Check your own fixes.** Run `python3 tools/docs_audit.py`. If the round added a kind of mistake the script could catch, add that check to the script.
7. **Record the round:** update the lens's last-used date, add the findings and fixes to `docs/plan/readiness.md` if they change what is ready, and note the round in the journal.
8. **Report:** the lens, what was found, what was fixed, what was deferred and why.

Be honest about returns. If a round finds little, say so, and say whether the next gain is more likely to come from another lens or from building.
