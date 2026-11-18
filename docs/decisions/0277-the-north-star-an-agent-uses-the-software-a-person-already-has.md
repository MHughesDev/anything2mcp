# 0277. The North Star: an agent uses the software a person already has, without waiting for its maker

**Status:** Accepted. Owner decision of 2026-11-18. The direction is the owner's; the wording is the lead agent's ([decision 0231](0231-the-owners-direction-is-written-in-exact-words-of-the-agents-own.md)). Only the owner changes it.

**Date:** 2026-11-18

## Context

The design had a summary, requirements, and a bar, and no single sentence that says what all of it is for. The owner gave that sentence in conversation, late in a day in which the product's reach was widened several times.

## Decision

**The North Star.** A person's AI agent can already act on their machine and their network. It cannot yet use most of the software that is there, because that software offers agents no tools. Anything2MCP exists to close that gap from the person's side: it makes MCP tools over what a piece of software already makes available to the person who uses it, so that the person's agents can do real work in that software without the software's maker having to build an MCP server for it.

Three things follow from the sentence, and each is part of the decision.

1. **The measure of progress is the gap.** A change is worth making in proportion to how much software it brings within an agent's reach, how little a person must do to bring it there, and how far the resulting tools can be trusted to do what they say. Work that does none of the three is not on the path.
2. **The material is what the software already gives.** Its screens, its requests, its command line, its files, its data, its published descriptions of itself, the interfaces the operating system publishes about it. The product works over these as it finds them. It does not ask the maker for anything, and it does not take what the software has not made available to the person: nothing in this sentence loosens rule 5.
3. **It is for the person who already uses the software.** They are entitled to use it, they point the product at it, and the tools that result are theirs. The first release serves one person's machine and the networks it is on ([decision 0208](0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md), [0222](0222-the-product-is-aware-of-the-network-its-machine-is-on.md)); the sentence itself names machine and network together.

**What the lead agent inferred.** The owner spoke of shrinking the gap and of not requiring an application to create its own MCP. The three consequences above, and the phrase "from the person's side", are the lead agent's reading of where that leads. The owner corrects any of them by saying so.

## Consequences

- AGENTS.md states the North Star at the head of "What Anything2MCP is", and the spec's summary names it.
- [SCOPE.md](../../SCOPE.md) stays a description of both sides; a "does not" there is weighed against this sentence before anyone treats it as settled ([decision 0230](0230-the-scope-is-written-down-on-both-sides-and-describes-without-ruling.md)).
- The choice of what the scout learns from a program itself ([decision 0276](0276-what-the-scout-may-learn-from-a-program-itself-and-by-which-means.md)) is in line with it: published interfaces first, because they bring the most software within reach for the least a person must do.
- The ten rules are unchanged. The North Star says where the product is going; the rules say what it never does on the way.

## Alternatives weighed

- **Leave the purpose spread across the summary and the requirements.** Every agent that builds here chooses between pieces of work many times a day; one sentence to weigh them against is worth more than several pages that agree with one another.
