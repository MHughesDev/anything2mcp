# 2026-11-17: the universal design, sections 53 to 66, and a deep study of REA

## State at the end

The design now reaches from applications to any existing system. Fourteen new spec sections, 53 to 66, write out the architecture the long-term plan proposed: the stack from devices to hosted services, seven ways of reaching any system, a neutral kernel and packs, one provider contract, supervision of outside subsystems, discovery and findings, an evidence ladder, the method for laying tools over a system, effects and hazards, a catalog of kinds of system, rules for devices and firmware, rules for tools, and packaging. No code changed, and nothing was renamed. The owner has read none of it yet.

## Done

- **A deep study of REA,** at one commit, in five read-only parts: its architecture and control of outside engines; its tool contracts and doctrine; its evidence model; how it observes applications; and what it does with firmware and devices. Nothing was run. The claims the design rests on were checked against the source by hand. [Research note 16](../research/16-rea-methods.md).
- **Spec sections 53 to 66.** The [index](../spec/README.md) lists them.
- **Decision records 0049 to 0054:** kernel and packs behind one provider contract; seven archetypes, with modes arriving by pack; findings and the evidence ladder; effects enforced and hazards raising the approval; REA used for method and only as an optional provider; devices through their own interfaces and firmware only through the device's own update.
- **Hypotheses H-29 and H-30;** ten new actions in the catalog; new glossary terms.

## Learned

- REA's most useful part for us is not what it inspects but how it controls what it did not write: a private handshake for every launch, one framing, a broken stream closed and never repaired, every failure saying whether the request ran, and a process stopped only when ownership is proven at that moment.
- Its evidence model confirms the ladder and sharpens it: level and source are two axes, a derived fact is never stronger than its parents, and coverage is separate from content. It also shows what to avoid: missing fields that default to the strongest value, and four overlapping vocabularies where one would do.
- REA never verifies a write, and says its effect declarations are hints for a host to enforce. That is the gap this project exists to fill, so nothing of its execution side transfers.
- At the lowest levels REA is entirely passive: it reads firmware and device artifacts from files, speaks to no hardware, and runs nothing under emulation. Operating a device is ours to design. It is realistic where a device offers an interface that can be read as well as written, and refused where the only way in defeats a protection.
- Most kinds of system need no pack of their own. Request and command packs reach hosted services, cloud platforms, containers, storage, and search. Five packs beyond the first four would cover nearly everything else.

## Changed in the design

- Sections 53 to 66 refine sections 5, 6, 7, 26, 29, and 41, and win where they differ.
- Decision 0005's seven modes are unchanged; decision 0050 says further modes arrive with packs, each in one of seven archetypes.
- New in the design: the discoverer extension point; findings; the evidence level on every finding, draft, and tool; hazards beside effect classes; the supervisor; scenarios; parts of a target; cross-surface read-back.
- The glossary, the action catalog, the technical reference's list of nouns, the spec index, and the long-term plan were updated to match.

## Noticed, not acted on

- AGENTS.md still says that sections 39 to 52 were written last and win. Sections 53 to 66 now do. See "Undecided".
- The older sections still say Web2MCP and still describe applications only. Stage A of the long-term plan renames; package A7 sorts each section into kernel or pack.
- Section 21 sends found tools through one generic call action. Section 64 proposes adding them to the client's list where the client supports it; no client has been checked.
- Confinement of plug-in processes is promised nowhere, because it has not been researched on any operating system.
- REA writes an ownership record for cleaning up after a crash, and no reader for it was found. Our supervisor needs the reader.

## Undecided

For the owner. The long-term plan, section 9, still has the full list; the first three block the rename.

1. The repository's new address. From the build session it still answers under the old name.
2. The package names, and a pending trusted publisher on PyPI for each. Nothing is published, and a session cannot push a tag.
3. `a2m` as the crate prefix and short command.
4. Whether AGENTS.md and the skills should say that the agent merges its own work, and that sections 53 to 66 win. Both need the owner's words or the owner's edit.
5. New with this session: whether the second pack is the command pack, as sections 55 and 62 propose; whether a second person should be required for the `physical` hazard; whether native-code capabilities of the optional REA provider are offered at all.

## Next

1. The owner reads [section 53](../spec/53-the-universal-runtime.md) and [section 66](../spec/66-readiness-of-the-universal-design.md) first; they are the shortest way into the rest.
2. With answers to questions 1 to 3, start Stage A of the [long-term plan](../plan/anything2mcp-master-plan.md): the rename and the kernel and pack separation, each package proven by the audit script and CI.
3. Without those answers, the work that does not depend on names can still go on: slice 0.4, the browser engine spike, and package B2, the evidence level and the tagged subject as core types.
