# 0213. Whether a target someone else wrote may be loaded, and found

**Status:** Accepted in the form its recommendation gave, by owner decision [0242](0242-the-owner-takes-the-recommended-answer-to-every-open-question.md) of 2026-11-18: a target someone else wrote may be loaded, under the rules of section 502; the project still keeps no list of places where targets are published. Only the owner changes it.

**Date:** 2026-11-18

**Asks about:** rule nine of [AGENTS.md](../../AGENTS.md#rules-that-never-bend); decision 0058; [section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md).

## Context

Rule nine keeps every real application's name out of this repository, and says target files for real applications live outside it and that the project publishes no catalog of them. Decision 0058 adds that a developer loads a target "from a path or from a package they built themselves", and that the project hosts no place to share them.

Two things meet it.

1. **The documents disagree.** Section 502 designs "targets written by someone else": a shared target or a bundle from another machine is checked by its signature, shown in the same review summary as a publish, and starts with its writes disabled until the local gates pass. Decision 0058's "a package they built themselves" leaves no room for that.
2. **What it costs.** The popular tools in the landscape are installed in one line, and the most-starred lists and registries are catalogs that other people add to ([research note 27](../research/27-the-public-landscape-of-agent-tools.md), finding 3). With no target from anyone else, every person builds every target from nothing, which costs the first hour most for the applications everyone uses.

## The safest way through that stays inside the rule

Section 502's rule, applied on one machine: the product loads a target from a path or a package the person chooses, whoever wrote it, checks its signature or has the person override for that file, shows the review summary, and enables no write until the local gates pass. The project itself publishes, hosts, and links to nothing.

## The questions for the owner

1. May the product load a target someone else wrote, from a path or a package the person chooses, under section 502's rules? (This review reads section 502 as the intent and decision 0058 as written before it.)
2. May the product, or this project's documents, point to places where other people publish targets they host, without this project hosting or listing any?

## Recommendation

Answer 1 yes, and supersede the one phrase of decision 0058 with it. Answer 2 no for now: a list kept by the project is a catalog in all but name, and the duty to keep it true is the one decision 0058 refused.

## Alternatives weighed

- Treating a target package as a plug-in. Refused: a plug-in is code in a process; a target is files the compiler checks, and section 502 already says how to take them.
