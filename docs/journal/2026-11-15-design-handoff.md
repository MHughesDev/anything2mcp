# 2026-11-15: design handoff

## State at the end

The repository holds a complete design and no code. The design was written and audited in one long session without building anything, so none of it has been tested against a running system. Phase 0 is next.

## Done

- The system design: 52 spec sections, 44 decision records, 8 reference documents, 15 research notes, 31 open research briefs.
- Plans: a readiness report, a phase map, task lists for Phases 0 to 3, the test application plan, the working method, the core-first slice order, and the design review lenses.
- A hypothesis register with 22 entries, all open.
- A documentation audit script, `tools/docs_audit.py`, reporting no findings.
- Package names: `web2mcp` is held on crates.io and `w2mcp` on npm, each by a placeholder at version 0.0.1 that was built outside this repository. PyPI has a pending trusted publisher for `release.yml` and the name is not yet held. See [publishing setup](../plan/publishing-setup.md).

## Learned

- Auditing on paper found real problems five rounds in a row, then began to run out. What is left to learn needs running code.
- Two documented claims turned out false on checking and were corrected: that the engine could drive the user's everyday browser profile, and that the pip name was taken. Expect more of the research to be wrong in places; most of it came from search summaries.

## Changed in the design

Everything; this is the first entry. The readiness report lists what each audit round found and fixed.

## Noticed, not acted on

- Sections 1 to 38 came first and sections 39 to 52 refine them. The earlier sections were patched to agree, and the audit checks for known contradictions, but a careful reader may still find an older phrase that a later section overrides. The later section wins; fix the earlier one when found.
- The order of Phases 4 to 9 was set before anything was built. It should be reconsidered after Phase 3.
- The scheduler, the hosted sign-in page, and the admin console are thin on purpose. The readiness report lists everything left undesigned.
- Three design review lenses were planned and not run: one pipeline line by line, less, and the security reviewer. They are lenses 6, 7, and 8.

## Undecided

For the owner:

- Whether the repository stays public while the core is built.
- Where target files for real applications will be kept.
- Which MCP clients matter most, for the Phase 0 spike.
- Whether a Windows machine is available for local testing.
- Whether to add trusted publishing on crates.io and npm now that both packages exist.

## Next

1. Read AGENTS.md, then the [working method](../plan/working-method.md) and [core first](../plan/core-first.md).
2. Before any code, run design review lens 6 or 7 as a first exercise. It is a cheap way to learn the design and it will find something.
3. Then start slice 0.1 from core first: the repository skeleton and CI. Plan it, show the owner, and wait for a go-ahead.
