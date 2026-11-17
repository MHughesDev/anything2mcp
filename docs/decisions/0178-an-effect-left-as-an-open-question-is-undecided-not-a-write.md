# 0178. An effect left as an open question is undecided, not a write

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** [record 0161](0161-the-plan-and-the-engine-after-the-packs-were-built.md), point 10, second half. Its first half (a delete is `irreversible`) stands.

## Context

Record 0161 said that a file kind entry whose effect is an open question counts as a write for `FileKind::writes`. The reason given was one rule for every pack: the HTTP pack already counted such an endpoint as a write, and the command and MCP packs did not.

Carrying it into the command and MCP packs showed what the rule does. The kernel's drafter refuses any drafted entry for which `FileKind::writes` is true, because the drafter never drafts a write. A pack's drafter leaves the effect as an open question for every entry it cannot judge: every command, and every tool the server does not mark read-only. With the rule, every such draft was refused, and discovery of a command-line program or an MCP server drafted nothing. The packs' own tests showed it, and the agent that found it stopped and did not decide.

## Decision

An effect left as an open question is **undecided**. It is not a read and not a write.

- `FileKind::writes` is true for an entry that is declared to write, and false for one whose effect is an open question. A pack may be stricter where it has evidence of its own: the HTTP pack counts an endpoint with an unsafe method as a write whatever its effect says.
- The drafter may write an entry whose effect is an open question. That is what an open question is for: the drafter states what it saw and leaves the judgment to a person.
- Nothing can use an undecided entry. The schema checker refuses an open question wherever it stands (`kernel.open_question`), in a draft and at a publish alike, so a file that holds one does not compile, no binding can name the entry, and no step can be lowered from it. The test `an_open_question_is_refused_wherever_it_stands` holds this.

So "the drafter never drafts a write" keeps its meaning: the drafter never writes an entry that is declared to write, and an entry it leaves undecided cannot run until a person has answered, at which point the compiler's own rules for a write apply to it.

## Rejected

- **Keep the rule and stop drafting undecided entries.** Safe, and it makes discovery of two kinds of system useless: the first thing a person needs from a scout of a command-line program is the list of its commands.
- **A third answer from `FileKind::writes`.** A type with three values says the same thing more exactly, and every pack would have to change for no change in what is refused. The checker already refuses the open question itself.

## What follows

- The `a2m-files` design states the rule as it is here.
- The command and MCP packs stay as they are. The HTTP pack's stricter rule stays.

## What would reopen it

A way for a file that holds an open question to compile, in any mode.
