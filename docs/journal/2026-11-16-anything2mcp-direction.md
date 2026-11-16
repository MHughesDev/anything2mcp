# 2026-11-16: the owner renames the product Anything2MCP; an audit and a long-term plan

## State at the end

The owner decided that the system becomes Anything2MCP, a universal and modular runtime, and asked for an audit and one long-term document before any refactor. That document exists: the [long-term plan](../plan/anything2mcp-master-plan.md). No code changed and nothing was renamed. Pull request 4 (slice 0.3, the test application) had been merged before this session's work began. The pip placeholder is still not published.

## Done

- Read the REA project at one commit, from source, without running it: its licence, package, layout, tool families, decision records, and how it treats evidence and outside engines.
- Audited this repository against the new direction: what is built, what is only designed, which parts are already neutral, and how much the old name is attached to.
- Wrote the long-term plan: direction, audit, REA assessment, target architecture, names, trade-offs, an order of work in stages A to F with work packages, rules for agents, questions for the owner, and risks.
- [Decision 0048](../decisions/0048-the-product-becomes-anything2mcp.md) records the owner's decision and supersedes 0001.
- Hypotheses H-26, H-27, and H-28 added.

## Learned

- The existing design is more universal than its name. Seven modes behind one driver interface, plug-ins as processes, and a mode that wraps any MCP server are already there. The refactor is a separation and a renaming.
- REA discovers and explains; it does not execute business operations or verify them. It fits as an optional discovery provider behind the `mcp` mode, at build time only. Its best contributions are ideas: evidence with a tagged target identity, declared effects per tool, engines identified by digest and never fetched during an operation.
- `anything2mcp` and `any2mcp` are free on PyPI, crates.io, and npm. `a2m` is taken on PyPI and npm. `web2mcp` is still free on PyPI and can be taken by anyone.

## Changed in the design

- Decision 0048 added; 0001 marked superseded. Decision 0022, the package names, stands until the owner confirms new ones.
- Nothing else. Every architectural statement in the long-term plan is marked Decided, Proposed, or Open, and nothing Proposed has been built on.

## Noticed, not acted on

- From this session the repository still answers as `MHughesDev/web2mcp`, and no repository named `anything2mcp` is visible. No address in the repository was changed.
- The owner asked for the pip placeholder to be published. The session cannot push a tag: the push was cut off twice and no tag reached the remote. Publishing needs the owner to push `v0.0.1`, or a manual start added to the release workflow, which is package F2 of the plan. With the rename, which name to publish first is also the owner's to say.
- AGENTS.md, the skills, and every document still say Web2MCP. That is Stage A of the plan, not done here on purpose.

## Undecided

For the owner. The long-term plan, section 9, has the full list. The three that block Stage A:

1. The repository's new address, and whether the build session can reach it.
2. The package names, and a pending trusted publisher on PyPI for each name to be held.
3. `a2m` as the crate prefix and short command.

Still open from before: how much agents do without asking, since AGENTS.md and three skills say to wait and the owner has said otherwise.

## Next

1. Read the [long-term plan](../plan/anything2mcp-master-plan.md), sections 7 and 9.
2. Get the owner's answers to questions 1 to 3.
3. Start Stage A with package A1: the decision records for the names and for the kernel and pack rule. Then the mechanical renames, A2 to A6, each proven by the audit script and CI.
