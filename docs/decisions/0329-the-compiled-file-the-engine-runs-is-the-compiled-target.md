# 0329. The compiled file the engine runs is the compiled target

**Status:** Accepted. Owner decision. Supersedes point 1 of [decision 0326](0326-plan-names-only-the-compiled-plan-and-the-build-document-is-the-build-order.md).

**Date:** 2026-11-23

**Changes:** the name of the compiled form of a target version, everywhere: the glossary, the specification, the file formats, the crate designs, the code's types and modules, the failure codes that carry the word, and the messages a person reads. Roadmap package N0 carries the change.

## Context

Decision 0326 kept "plan" as the name of the compiled, hashed form of a target version. In everyday words a plan is a set of intentions, which this file is not: it is compiled from a target's files, fixed by its hash, and run by the engine step by step with no model. The owner found the word confusing for that reason too, and chose another.

## Decision

1. **The compiled target** is the compiled, hashed form of a target version that the engine runs: what the compiler makes from a target's files. Short form, where the context makes it plain: the compiled target, never "the plan".
2. The rename covers every place the word names this part: types (for example `Plan` becomes `CompiledTarget`), modules and files, the plan's format number and hash, failure codes whose name carries the word, the file formats, and every document. Words that name something else (a step's plan inside an approval, the word in prose about planning work) are left alone, each checked by hand.
3. Until package N0 lands, the code keeps its old names and the documents may say "plan"; the glossary's entry for Plan points here.
4. **The Build Order** and **build briefs** keep the names decision 0326 gave them.

## Why not

| Alternative | Why not |
| --- | --- |
| Program | Also names the product's own binary and the programs the command pack drives |
| A coined name | A new word to learn, where two plain words say what it is |
| Keep "plan" with a note | The owner found the word itself the trouble |
