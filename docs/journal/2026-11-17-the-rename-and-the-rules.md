# 2026-11-17: the rename, the owner's answers, the rules for working without waiting, and the first kernel code

## State at the end

The repository is Anything2MCP in every living file: 24 crates in a kernel, packs, and a product crate, with the boundary enforced by the audit; the command `anything2mcp` and its short name `a2m`; the pip placeholder under the new name. The licence is MIT. The rules now tell agents to plan, build, and merge without waiting, which is what the owner asked for in so many words. The repository has the files a public project needs. The first kernel code exists: the shared vocabulary in `a2m-core`. Nothing is published on any registry.

## Done

- **Stage A of the long-term plan, packages A1 to A6.** Crates renamed and sorted; a new empty kernel crate, `a2m-provider`; the product crate builds two commands from one `cli` module; the layering table redrawn with the kernel in layers 1 to 6, packs in 7 and 8, the product in 9.
- **The audit** now refuses three things by name: a kernel crate that depends on a pack crate, a pack crate that reaches above the kernel's third layer, and a pack crate that depends on another pack. It also refuses an earlier name anywhere outside history. Its self-test plants each mistake: 20 cases.
- **The owner's answers,** as decision records 0055 to 0058: agents do not wait; one name everywhere; MIT; a toolkit for developers with no public catalog and no preferred agent, proven on test stands with real systems at the end.
- **The line between what is automated and what is left to the developer,** which the owner asked the agent to draw: decision 0059 and spec section 67. A scout runs discovery, drafts what follows by rule, and proves reads, with no model in it. Meaning, writes, and open questions stay with the developer; consent, sign-in, approval, and publishing stay with a person.
- **AGENTS.md, the working method, and the six skills** rewritten for decision 0055.
- **The public files:** README, CHANGELOG, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, RELEASING, issue forms, a pull request template, Dependabot settings, editor settings.
- **Package F2, brought forward:** the release workflow can be started by hand with the version to publish typed in. It merged, and then the session's attempt to start it was refused; see "Learned".
- **Stage B, package B1, and the first half of B2,** in a second pull request: `a2m-core` now holds the closed vocabularies as enums (archetypes, effect classes, hazards, failure classes, evidence levels, coverage, and the states of runs, records, sessions, and bindings), `Mode` and `FailureCode` as checked names, `Failure`, and `Secret`. 36 new tests. Thirteen of them read the design documents and fail if a name, or the order of names, differs between code and document. One fails to compile if `Secret` ever becomes serializable or copyable.

## Learned

- The session's access was tied to the repository's old address. The renamed repository had to be attached to the session before a push would go through. The existing clone was kept and its remote repointed.
- Putting packs beside the kernel's layers, and not inside them, changes one real thing: the engine can no longer name a driver. That was the intent of decision 0049, and the build now refuses anything else.
- A blind rename falsifies records of the old name. Two research notes, the publishing setup, and the names table of section 41 state facts about the earlier names and had to be handled by hand. The audit's list of history files is the list of those places.
- **The session cannot publish by either route.** Pushing a tag is refused, and starting the release workflow by hand is refused too: the session's access to GitHub does not include running workflows. The manual start is still worth having, because the owner can use it from a browser. The placeholder release is therefore the owner's click.
- **A mode cannot be an enum in a neutral kernel.** AGENTS.md said modes are enums. With packs and plug-ins bringing modes, that would change a kernel crate for every pack. Decision 0060: the archetype is the closed type; a mode is a checked name that a provider registers.
- A test that reads the documents is cheap and already earned its keep: it is now impossible to rename a state in the spec without the build saying so.
- From this session, on 2026-11-17, none of the three registries showed anything published as `anything2mcp`. The owner reports the registration done. Both can be true: an account or a publisher configured in advance does not show in a public index, and reserves nothing.

## Changed in the design

- Decisions 0055 to 0060. Decision 0018 (Apache-2.0) and 0022 (package names) are superseded.
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

Records 0056, 0059, and 0060, and the following choices inside them: modes as registered names; no alias packages under the earlier names; `a2m` as the short command and crate prefix; documents always use the full command; the copyright line "The Anything2MCP Authors"; contributions accepted without a signed agreement; vulnerability and conduct reports through GitHub's private reporting; the scout runs when a target is added in the single-user profiles.

## For the owner

Three things only the owner can do, none of which blocks the build:

1. **PyPI.** A pending trusted publisher for the project `anything2mcp` must name the repository `MHughesDev/anything2mcp`, the workflow `release.yml`, and the environment `pypi`. One set up before the rename names the old repository and will be refused. Then publish the placeholder: Actions, Release, Run workflow, on `main`, and type `0.0.1`. The session tried to do this and was refused.
2. **crates.io and npm.** Each needs one first publish from the owner's account before trusted publishing can be configured. The names are free today and are not held.
3. **Private vulnerability reporting.** Switch it on in the repository's settings, under Security. SECURITY.md and the code of conduct send reports there.

## Next

Finish package B2: the tagged identity of a finding's subject, as a core type, from the table in spec section 58. Then B3, the provider manifest and the registry in `a2m-provider`, which is where a mode gets registered. In parallel, on separate branches: package A7, and slice 0.4, the browser engine spike with research brief 1.
