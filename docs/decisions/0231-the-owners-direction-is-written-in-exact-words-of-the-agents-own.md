# 0231. The owner's direction is written in exact words of the agent's own

**Status:** Accepted. Owner decision of 2026-11-18. The direction is the owner's; the wording is the lead agent's, as this record itself requires. Only the owner changes it.

**Date:** 2026-11-18

## Context

The owner gives direction in conversation. It is spoken, quick, and often unfinished: it points somewhere, and stops before it arrives. Three records written on 2026-11-18 ([0208](0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md), [0222](0222-the-product-is-aware-of-the-network-its-machine-is-on.md), [0230](0230-the-scope-is-written-down-on-both-sides-and-describes-without-ruling.md)) quoted that speech word for word, on the thought that a quotation is the most faithful record. The owner read them and said it is not.

A decision record is not a transcript. Agents read these documents as instructions and steer by them. A sentence that was said in passing, kept word for word, gets read as if every word had been weighed. A hesitation becomes a condition, a figure of speech becomes a requirement, and a thought the speaker did not finish becomes a gap the next agent fills by guessing.

## Decision

1. **The owner's speech is direction, not text.** An agent that records an owner decision does not quote the owner. It works out where the direction is heading, finishes the thought, and writes the decision in careful words of its own: what is decided, what changes, and what is left alone.
2. **The words are chosen as carefully as code.** The decision is stated whole and exact, in the terms of the [glossary](../reference/glossary.md), with nothing in it that the agent would not want read literally. Every sentence in a document that is in force is true, and says one thing.
3. **What was inferred is said.** Finishing a thought means adding to it. Where the agent supplied a reading the owner did not state, the record says so in plain sight, so that the owner can correct it. Faithfulness is to the owner's intent, and the owner is the judge of that.
4. **The owner's records may be reworded when the owner asks.** A decision record is otherwise never rewritten, only superseded. The owner's own decisions are the exception, at the owner's request: on 2026-11-18 the owner asked for every decision of theirs to be read again and reworded where better words exist. A rewording changes the words, never the decision; a change to the decision is a new record. Each reworded record says in its status line that the wording is an agent's.
5. **This holds for every document that states the owner's direction**, not only decision records: [AGENTS.md](../../AGENTS.md), [SCOPE.md](../../SCOPE.md), the README, and the first part of the specification.

## What was done under this record on 2026-11-18

- Records 0208, 0222, and 0230 were rewritten without quotation. Record [0055](0055-agents-plan-build-and-merge-without-waiting-for-the-owner.md) lost its two quoted fragments.
- AGENTS.md was read again as the document every agent starts from. Its opening said very little of the design had met running code; that had stopped being true and was corrected. Its description of the product said it runs on a laptop or as a company server; it now states the first release as decisions [0169](0169-the-first-release-is-for-one-machine.md), 0208, and 0222 set it.
- **Rule 3 was reworded, and this is the one change that is more than polish.** It read "Every write is verified", and required a read-back. Since [decision 0161](0161-the-plan-and-the-engine-after-the-packs-were-built.md) the product has had writes that cannot be read back (a publish with no key, a call to a tool that keeps nothing): they are declared `irreversible`, approved by a person as that, never sent twice, and their results are never called verified. The rule as written said something the product does not do. It now reads "Every write is recorded first and proven after", and states the honest case in its own words. The owner is asked to read this one: if the old wording was meant as a ban on any write that cannot be read back, the rule goes back and decision 0161 is the thing to change.
- The older owner records (0002, 0019, 0021, 0035, 0048, 0057, 0058, 0169) were read. They were written in an agent's words from the start and state their decisions exactly. They carry the product's first name and old section numbers, which is history and is left as it is. Two things in them are not wording and are already with the owner: record 0058 says a target comes "from a package they built themselves", which the question in [record 0213](0213-whether-a-target-someone-else-wrote-may-be-loaded-and-found.md) reopens; and record 0019 says a 10% slowdown fails the build, which no check enforces yet.

## Consequences

- AGENTS.md carries the instruction, so that every agent has it before it writes a record.
- A status line that says "the wording is an agent's" tells the owner which records to read with a correcting eye.
- The cleanup round under way reads every document in force with the same standard: true, exact, and said once.

## Alternatives weighed

- **Quote the owner, then interpret below the quotation.** This was the practice for three records. It keeps the evidence and still leaves the raw speech in a place agents read as instruction.
- **Keep the owner's words in the journal and the decision in the record.** Reasonable, and not asked for: the journal is history and may describe what was said, but it does not need a transcript either.
