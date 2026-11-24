# 0301. Every part has a proper name, and a description is not a name

**Status:** Accepted. Owner decision, given in conversation on 2026-11-23 and written here in the agent's words ([decision 0231](0231-the-owners-direction-is-written-in-exact-words-of-the-agents-own.md)).

**Date:** 2026-11-23

**Changes:** [AGENTS.md](../../AGENTS.md) (the line "One vocabulary"), the [glossary](../reference/glossary.md), the architecture page and its data under [docs/architecture/page/](../architecture/page/README.md).

## Context

The architecture page showed parts under phrases that described them: "Choose a binding", "Store the result", "Who calls", "What the loop uses all the time". A phrase like that cannot be cited, searched for, or used as a heading, and two documents will describe the same thing in two ways. The owner asked that every part be given a proper name, on the page first and then in the specification as a rule.

## Decision

1. **Every part of the system that a document or a drawing shows has one proper name.** A part is anything a reader could point at: a component, an interface, a store, a file or folder kind, a step of a sequence, a group of parts, a walk through the system.
2. **A proper name is a noun or a short noun phrase,** written in Title Case where it stands as a name: Engine Loop, Write Path, Binding Selector, Holding Folder.
3. **A description is not a name.** These are not names: a question, a clause or a sentence, a phrase that begins with a verb, with "what", "who", "how", "when", or "where", or with an article, a phrase with a colon that explains itself, and a placeholder such as "other".
4. **A name comes from one of three places, in this order:** the glossary's term for the thing; the name the code gives it (a type, a trait, a module, an action, a file kind); a new noun phrase of at most three words. One thing has one name everywhere.
5. **The glossary is where a name becomes the project's.** A name the specification or a crate design uses is in the glossary. A name coined for the architecture page stays in the page's list of proposed names until a document takes it up, and enters the glossary then.
6. **What says what happens stays a description:** the text of a step, the label of an arrow, a sentence in a section. The rule is about what a thing is called, not about how it is explained.

## What I inferred

The owner gave the rule for the architecture page and asked that it reach the specification. I inferred point 4's order, because the repository already has one vocabulary and code that names things, and a third name beside those two would break the first. I inferred point 5, because the page's first renaming coined 287 names in one pass and no one has yet checked each against the documents; putting them all into the glossary at once would make it say things nobody reviewed.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Put every coined name into the glossary now | 287 unreviewed entries in the document that every agent treats as the vocabulary |
| Let the page keep descriptive labels and name things only in the specification | The page is where the owner reads the design; two sets of words for one system is what the rule ends |
| Code names only | Many parts a reader needs to point at, such as a phase or a group, have no single code name |

## Consequences

- The architecture page's parts, groups, and walks were renamed on 2026-11-23: 63 names from the glossary, 48 from the code, 287 coined.
- Where the glossary and a first choice differed, the glossary's term was kept: Control, and Registered Names. One part takes the code's name because the glossary's word means something else: the Stdio Proxy, since Bridge is an archetype.
- New documents and new diagrams follow the rule. Existing section titles are not renamed in bulk; a section is brought in line when it is next changed.
