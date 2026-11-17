# 2026-11-17: the rename, the owner's answers, and the rules for working without waiting

## State at the end

The repository is Anything2MCP in every living file: 24 crates in a kernel, packs, and a product crate, with the boundary enforced by the audit; the command `anything2mcp` and its short name `a2m`; the pip placeholder under the new name. The licence is MIT. The rules now tell agents to plan, build, and merge without waiting, which is what the owner asked for in so many words. The repository has the files a public project needs. Still nothing of the engine exists. Nothing is published on any registry.

## Done

- **Stage A of the long-term plan, packages A1 to A6.** Crates renamed and sorted; a new empty kernel crate, `a2m-provider`; the product crate builds two commands from one `cli` module; the layering table redrawn with the kernel in layers 1 to 6, packs in 7 and 8, the product in 9.
- **The audit** now refuses three things by name: a kernel crate that depends on a pack crate, a pack crate that reaches above the kernel's third layer, and a pack crate that depends on another pack. It also refuses an earlier name anywhere outside history. Its self-test plants each mistake: 20 cases.
- **The owner's answers,** as decision records 0055 to 0058: agents do not wait; one name everywhere; MIT; a toolkit for developers with no public catalog and no preferred agent, proven on test stands with real systems at the end.
- **The line between what is automated and what is left to the developer,** which the owner asked the agent to draw: decision 0059 and spec section 67. A scout runs discovery, drafts what follows by rule, and proves reads, with no model in it. Meaning, writes, and open questions stay with the developer; consent, sign-in, approval, and publishing stay with a person.
- **AGENTS.md, the working method, and the six skills** rewritten for decision 0055.
- **The public files:** README, CHANGELOG, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, RELEASING, issue forms, a pull request template, Dependabot settings, editor settings.
- **Package F2, brought forward:** the release workflow can be started by hand with the version to publish typed in, because a build session cannot push a tag.

## Learned

- The session's access was tied to the repository's old address. The renamed repository had to be attached to the session before a push would go through. The existing clone was kept and its remote repointed.
- Putting packs beside the kernel's layers, and not inside them, changes one real thing: the engine can no longer name a driver. That was the intent of decision 0049, and the build now refuses anything else.
- A blind rename falsifies records of the old name. Two research notes, the publishing setup, and the names table of section 41 state facts about the earlier names and had to be handled by hand. The audit's list of history files is the list of those places.
- From this session, on 2026-11-17, none of the three registries showed anything published as `anything2mcp`. The owner reports the registration done. Both can be true: an account or a publisher configured in advance does not show in a public index, and reserves nothing.

## Changed in the design

- Decisions 0055 to 0059. Decision 0018 (Apache-2.0) and 0022 (package names) are superseded.
- Spec section 67 is new. Sections 29, 41, 55, 58, 60, 65, and 66, the technical reference's layering table, the glossary, the action catalog (91 actions), and the hypothesis register (H-31) were updated to match.
- The second rule of discovery is reworded: a finding still cannot run, and a draft may now be written by the drafter as well as by a person or an agent.
- The read on a real application leaves Phase 2's exit test and moves to the end of the build, on the owner's word.

## Noticed, not acted on

- Package A7 is not done: the spec index does not yet say which side of the kernel boundary each section falls on, and many older sections still describe applications only.
- Sections 1 to 52 say "Anything2MCP" where they were written about a web-only product. The names are right and some of the sentences are narrower than the product now is. Section 1 has a paragraph saying so.
- `a2m-auth` still holds browser-specific auth providers beside the neutral session keeper. Section 55 lists this as an open point.
- A notices file for third-party licences is needed before any release that people install. MIT does not need one for our own code; the dependencies' licences do.
- Dependabot will open pull requests that no session is watching. An agent should look at open pull requests at the start of each session.
- The pull request template asks for a changelog line. The audit does not check the changelog against anything.

## Decided without the owner having read it

Records 0056 and 0059, and the following choices inside them: no alias packages under the earlier names; `a2m` as the short command and crate prefix; documents always use the full command; the copyright line "The Anything2MCP Authors"; contributions accepted without a signed agreement; vulnerability and conduct reports through GitHub's private reporting; the scout runs when a target is added in the single-user profiles.

## For the owner

Three things only the owner can do, none of which blocks the build:

1. **PyPI.** A pending trusted publisher for the project `anything2mcp` must name the repository `MHughesDev/anything2mcp`, the workflow `release.yml`, and the environment `pypi`. One set up before the rename names the old repository and will be refused.
2. **crates.io and npm.** Each needs one first publish from the owner's account before trusted publishing can be configured. The names are free today and are not held.
3. **Private vulnerability reporting.** Switch it on in the repository's settings, under Security. SECURITY.md and the code of conduct send reports there.

## Next

Start Stage B, package B1: the core types in `a2m-core` (modes, effect classes, failure classes and codes, states, the evidence levels, and the secret wrapper that cannot be printed), with tests. In parallel, on separate branches: package A7, and slice 0.4, the browser engine spike with research brief 1.
