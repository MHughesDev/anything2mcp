---
name: test-hypothesis
description: Test one design hypothesis for Web2MCP with an experiment and record the result. Use when a slice touches an entry in docs/hypotheses/README.md, when the design assumes something unproven, or when two designs need comparing.
argument-hint: "[hypothesis number, such as H-14]"
---

# Test a hypothesis

The design is a set of bets. This is how one gets settled.

1. **Pick it.** Read the entry in `docs/hypotheses/README.md`. If the doubt is new, add it first: a sentence that could be false, a real alternative, and a test that gives a number or a clear yes or no.
2. **Design the smallest experiment** that could change your mind. Say beforehand what result would support the hypothesis and what would refute it. Decide this before running anything.
3. **Give the alternative a fair try.** Build both sides to the same shallow depth. An experiment that only tries the documented design proves nothing.
4. **Run it** in `spikes/`, against the test application where an application is needed. Record the machine, operating system, and versions.
5. **Write the result** as `docs/hypotheses/H-NN-result.md` from `docs/hypotheses/TEMPLATE.md`. Numbers first.
6. **Update the register:** status `supported`, `refuted`, or `changed`.
7. **Carry it through.** If the result contradicts a spec section, correct the section. If it contradicts a decision, write a new decision record that supersedes the old one, and ask the owner first when the old decision was the owner's. If a budget was wrong, revise it by decision record.
8. **Say what is still unknown** and add the next hypothesis if the test raised one.

A refuted hypothesis is a good result. Report it as plainly as a supported one, and never adjust the test afterwards to rescue the design.
