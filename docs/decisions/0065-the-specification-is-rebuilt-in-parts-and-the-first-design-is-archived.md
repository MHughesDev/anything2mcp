# 0065. The specification is rebuilt in parts, and the first design is archived

**Status:** Accepted. The owner asked for a full restructure of the repository around the product it has become; how to do it was left to the building agent. The owner had not read this record when it was merged.

**Date:** 2026-11-17

## Context

The product was designed as a tool for wrapping web applications and became a toolkit for any kind of system. The rename changed the names. It did not change the shape of the documents, which still carried the history of how they were written:

- The specification had 72 sections in three generations. Sections 1 to 38 described a web-only product. Sections 39 to 52 refined thirteen of them. Sections 53 to 72 widened everything. A reader had to apply a rule that later sections win, and an agent that opened only an early section built against text that was no longer true.
- Six planning documents overlapped: a build plan, a phase map, four phase lists, a slice order, a long-term plan, and a roadmap.
- Kernel sections said "application" where they meant any system, and web-only detail sat in sections that every pack must obey.

## Decision

- **The specification is rebuilt in seven parts,** each a folder: product, kernel, packs, building tools, security, running it, and project. A section's number has three digits and begins with its part's digit.
- **Each section states the design as it stands.** There is no rule about which section wins, because no two sections cover the same ground.
- **A section that stood on its own was moved and renumbered,** with its wording kept and brought up to date with the crate and pack designs.
- **Sections that had been refined by later ones were consolidated into one,** written from all of them, with the later generation winning and the architecture and decisions 0055 to 0064 winning over all.
- **The originals of every consolidated section are kept in `docs/archive/`,** unchanged except for a line saying where their content now lives. The archive's index is the key from the old numbers to the new.
- **History is not rewritten.** Decision records and journal entries keep the section numbers of their time. Their links were re-aimed: at the moved section where there is one, at the archived original where the section was consolidated.
- **One plan.** The roadmap is the order of work. The build plan, the phase map, the phase lists, the slice order, the long-term plan, and the first readiness report are archived. What they said that is still true is in the roadmap, the working method, and section 704.
- **The audit enforces the shape:** a section sits in the part its number names; nothing else sits in the specification's folder; a living document that cites an old section number fails.

## Consequences

- 58 sections replace 72. Thirty-eight were moved, fourteen are consolidations of two or three, and six were written fresh: the summary, the overview, the concepts, and sections for the HTTP, MCP, and desktop packs, which had been described only in passing.
- Every living document cites sections by their new numbers. Anything written before today that cites "section 42" means the old numbering; the archive says what that is now.
- Web-only material that sat in the technical reference, the widget drivers and the platform profile schema, moved to the web pack's section.
- A consolidation can lose something. What limits that: each consolidating agent reported what it dropped; the originals are one click away; and the tests that hold the code's vocabulary to the documents still pass against the new sections.

## Alternatives considered

- Leave the sections where they were and add a status line to each superseded statement: no risk of loss, and the reader still has to reconcile three generations.
- Move the sections into folders without consolidating: tidy to look at, and the contradictions stay.
- Rewrite the whole specification from nothing: one voice throughout, and every rule would have to be re-derived, with nothing to check it against.

## Revisit if

A builder finds a rule in an archived section that the new sections lost. Restore it to its new section and note it in the journal.

## Spec sections

- [The specification's index](../spec/README.md)
- [The archive](../archive/README.md)
