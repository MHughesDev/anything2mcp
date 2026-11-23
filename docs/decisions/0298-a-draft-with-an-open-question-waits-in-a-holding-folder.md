# 0298. A draft with an open question waits in a holding folder, and the target compiles whole

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. This record was rewritten once before it was merged; its first form is the first alternative below.

**Date:** 2026-11-23

**Changes:** where the drafter writes a draft (`a2m-files`), what the loader reads, the actions `draft.lint`, `draft.try`, `scout.run`, and `scout.report` (`a2m-actions`), [section 205](../spec/2-kernel/205-target-files.md), [section 402](../spec/4-building-tools/402-the-scout-and-who-does-what.md), the [file formats](../reference/file-formats.md), the designs of both crates. The compiler does not change.

## Context

A target compiles whole or not at all. When the drafter cannot decide a value it writes an open question in its place, and a file that holds one does not compile. So one drafted file with an open question stops every tool of the target from being tried, tested, or published.

Field test 1 met this twice ([results](../reviews/field-tests.md)). With a small description, one draft had to be deleted by hand before any read could be tried (finding 5). With the system's whole description the scout wrote several hundred drafts and 79 problems stood, most of them open questions. A person who wants three tools from a system that describes three hundred was made to deal with all three hundred first.

## Decision

1. **The drafter writes a draft that holds an open question into a holding folder,** `held/` inside the target's folder, at the same path it would have had: `held/operations/order_get.yaml`. A draft with no open question is written where it always was.
2. **A held draft takes with it what names it, and nothing else.** A file drafted from the same finding goes to the holding folder with it when it names something the held file defines, directly or through another held file: an endpoint with a question takes the read that uses it. A file of the same finding that names nothing held stays in place. The first build held every file of a finding, and the desktop pack showed why that is wrong: one window is one finding, and one undecided screen held four sound drafts that had compiled and run before.
3. **The loader does not read the holding folder.** The compiler never sees it. A target compiles whole or not at all, exactly as before, and its plan and hash depend on the files outside the holding folder only.
4. **Nothing is hidden.** `draft.lint` lists every held draft with its question. The scout's report counts them and lists them. Asking to try or publish a tool whose draft is held is refused by name, with its question as the next step.
5. **A person releases a held draft by answering its question and moving the file out.** The action `draft.release` does it: it refuses a file that still holds a question, moves the file to its place, and lints the target. Until that action is built a person moves the file by hand.
6. **An open question outside the holding folder is an error to the compiler, as it always was.** So is a file that names something held: the target fails, and the answer says that held drafts exist and that lint lists them. It does not try to say which held draft was meant, because working that out from an error's words can name the wrong file. The scout's own trial step still leaves out a file outside the holding folder that holds a question, as it did before ([decision 0180](0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md)), so a target drafted before this decision behaves as it did.
7. **A finding with a file a person changed is left alone.** Nothing drafted from it is moved or rewritten; the conflict is reported.
8. **`held` is a reserved folder name.** No pack registers a kind of file there.

## Why this is better than compiling around the questions

The first form of this decision had the compiler set such files aside and compile the rest. It was built, and an unfriendly review of the build found six faults: the sign-in file could be set aside, so a target compiled with no sign-in; a name defined twice passed when one of its files was set aside; real mistakes were taken for references to set-aside files, because dependants were found by reading errors; the check that files are unchanged since they were tested broke; a dependent draft was reported as sound; and a file's other mistakes were hidden. Each could be fixed, and the fixes needed a new typed diagnostic in the kernel and in every pack.

All six come from one cause: the compiler was asked to decide what a target is. Here it is not. What is in the target is what is in its folders, a person can see it by listing them, and the rule that makes a plan stays "all of it, or nothing".

## Why this keeps rule 7

An open question is the drafter saying it does not know. Nothing guesses the answer: a held draft is no part of any plan until a person answers it. What ends is one draft's unknown stopping tools that have none.

## Alternatives

| Alternative | Why not |
| --- | --- |
| The compiler sets files aside and compiles the rest (this record's first form) | Built and reviewed; six faults, one cause. It makes the compiler decide what the target is |
| Keep the target whole and keep the questions in place | The first field test: nothing can be tried until hundreds of drafts are dealt with by hand |
| Have the scout not write a draft with a question at all | The questions are how a person learns what the system left unsaid, and the rest of the draft is work already done |
| Compile only the files one tool needs, on each trial | A trial would pass for a tool the publish gates refuse, and the plan tried would not be the plan published |

## What this risks

- A person believes a tool exists whose draft is held. Point 4 is the guard.
- A draft outside the holding folder names something inside it. Point 6: the target fails and says so. The scout's own trial step already compiles without what it cannot use, and reports what it left out.

## Proof

A scout of a target whose description gives one sound read and one read whose answer has no shape the pack can follow: the first is in `operations/`, the second and its endpoint are in `held/`; lint answers ok and lists the held draft with its question; the sound read is tried, passes the gates, and is published; a trial of the held one is refused with its question. A file placed by hand outside the holding folder with an open question still fails the target. In field test 1 the scout is handed the system's whole description and reads are tried with no file deleted by hand.
