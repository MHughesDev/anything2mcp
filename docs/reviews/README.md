# Design review results

This folder holds the results of design review rounds: what a round looked at, what it found, and where each finding went. A review describes and proposes; it decides nothing by itself. Decisions are in [docs/decisions/](../decisions/README.md), and a review points to the records it raised.

A round is run one lens at a time, by the procedure in the [design review lenses](../plan/design-review-lenses.md) and the `design-review` skill in `.claude/skills/`. Its report is filed here, named for the lens or the question and the date. A review that draws a map or a table of its own files it here too, beside the report.

| Document | What it holds |
| --- | --- |
| [coverage-review-2026-11-18.md](coverage-review-2026-11-18.md) | The round by lens 15, coverage: for every way software on a personal computer can be read or changed from outside itself, whether the design has a place for it and whether that place is right; the findings, most important first, each with where it went |
| [coverage-map.md](coverage-map.md) | The map the coverage review drew: one row per mechanism by which a program on a person's computer can reach other software, and where each falls in the design today |
| [breadth-review-2026-11-18.md](breadth-review-2026-11-18.md) | The round by lens 16, breadth: the whole dimensions of "anything" in which the design is still narrow (being told, waiting, acting at a time, handing over files, asking the person, several systems, several agents), each line marked Rule, Choice, or Not yet |
| [configurability-review-2026-11-18.md](configurability-review-2026-11-18.md) | The round by lens 17, the person fitting it: the true state of every setting, the dimensions along which a person cannot yet shape the product, and the fixed frame that no setting reaches, with the kind of each line |
| [faults-and-manipulation-review-2026-11-24.md](faults-and-manipulation-review-2026-11-24.md) | The round of package 3.10, by lens 5, self-failure, with the questions of lens 4: what each fault test of the kernel's own parts and each manipulation test showed the documents had wrong, and where each finding went |
| [measured-system-review-2026-11-24.md](measured-system-review-2026-11-24.md) | The round by lens 10, the measured system: which assumptions of section 225 and the packaging sections the first measured numbers contradict (the key derivation in the start, the ledger in the call, the browser's share of a `ui` step, the gate on medians) and where each finding went |
| [field-tests.md](field-tests.md) | Not a review round: what each field test showed when the product was run against existing software the project runs itself, by the kind of system, with each finding and where it went ([decision 0293](../decisions/0293-software-we-run-ourselves-is-tested-against-during-the-build-and-may-be-written-to.md)) |
