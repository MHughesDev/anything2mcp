# 0298. A draft with an open question is set aside, and the rest of the target compiles

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** the compiler in `a2m-files`, the typed diagnostic for a name that is not found (used by the kernel and by every pack that looks a name up), the actions `draft.lint`, `draft.try`, `test.run`, and `publish.request` in `a2m-actions`, the publish page's review, [section 206](../spec/2-kernel/206-the-compiler-and-the-plan.md), [section 403](../spec/4-building-tools/403-testing-and-publish-gates.md), the designs of both crates.

## Context

A target compiles whole or not at all. When the drafter cannot decide a value it writes an open question in its place, and a file that holds one does not compile. So one drafted file with an open question stops every tool of the target from being tried, tested, or published.

Field test 1 met this twice ([results](../reviews/field-tests.md)). With a small description, one draft had to be deleted by hand before any read could be tried (finding 5). With the system's whole description, which the reader now takes, the scout wrote several hundred drafts and 79 problems stood, most of them open questions about answers the pack cannot give a shape to. No read could be tried until a person had deleted or answered every one. A person who wants three tools from a system that describes three hundred is made to deal with all three hundred first.

## Decision

1. **A file that holds an open question is set aside.** It is not an error of the target. The compiler leaves it out, with every file that names it, directly or through another file: an operation that uses a set-aside endpoint is set aside with it.
2. **What is left compiles as the target.** Its plan holds only tools none of whose files were set aside. Lint, a trial, the publish gates, and a publish all work on that plan.
3. **Nothing is hidden.** `draft.lint` lists every file set aside, each with its open question and the file that caused it to be set aside where that is another file. The publish review lists the tools that are in the version and, apart from them, what was set aside. Asking to try or publish a tool that was set aside is refused by name, with its open question as the next step.
4. **Only an open question sets a file aside.** Any other problem at the level of an error, such as a name defined twice or a reference to nothing, still fails the whole target as today. Those are mistakes in what a person or a rule wrote, and a plan made by skipping past a mistake would not be the target the person meant.
5. **A file set aside changes the plan's hash only by its absence.** The same files give the same plan, in any order.
6. **A file that describes the target as a whole is never set aside.** `target.yaml` and the sign-in file fail the whole target when they hold an open question, and the sign-in file also fails it when its probe names something that was set aside. A target compiled without its sign-in would run its tools signed in as nobody.
7. **A name defined twice is found before anything is set aside,** over every file of the target. Setting one of the two files aside never hides it.
8. **A file is set aside for naming another only through a reference the compiler knows as one.** Every place where the compiler or a pack looks a name up says which kind of thing it is looking for and reports a name it cannot find through one typed diagnostic that carries the kind and the name. A file is set aside for another exactly when that diagnostic names a kind and a name that only set-aside files define. No other error is ever read as such a reference: not by its place, and not because its text is the same as a name. A lookup that does not use the typed diagnostic fails the whole target, as before.
9. **A mistake beside a reason to set a file aside is still told.** Lint reports every other problem of a set-aside file as a warning that says the file was set aside. It does not fail the target, because the file is in no plan.
10. **An operation is set aside with its tests.** Where publishing requires an operation to have a test, and every test of an operation was set aside, the operation is set aside too, with the test's file as the cause.
11. **The drafter reports a draft set aside for naming another as held back,** with the file that caused it.
12. **A map that holds `open_question` beside any other key is an error,** not an open question and not a value.
13. **The source digest in a plan's header covers the files that were not set aside,** so that point 5 holds. Whatever checks that a target's files are unchanged since they were tested compares that same digest, made the same way, and also refuses when the list of files set aside is not the one that was tested.

Points 6 to 13 were added the same day, before anything was merged, after an unfriendly review of the first build found that it took errors for references by their place and text, let the sign-in file be set aside, and let a doubled name pass.

## Why this keeps rule 7

Rule 7 says an unknown is a stop and never a guess. An open question is the drafter saying it does not know. This decision does not guess the answer: the tool with the question does not exist until a person answers it. What it ends is one tool's unknown stopping other tools that have none.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep the target whole | The first field test: nothing can be tried until hundreds of drafts are dealt with by hand |
| Have the scout write only drafts with no open question | The questions are how a person learns what the system left unsaid; they belong in the files, where they can be answered |
| Set aside any file with any error | A typing mistake in one operation would silently drop a tool from a version. An open question is marked on purpose; a mistake is not |
| Compile only the files one tool needs, on each trial | A trial would then pass for a tool the publish gates refuse, and the plan tried would not be the plan published |

## What this risks

A person publishes a version believing it holds a tool that was set aside. Point 3 is the guard: the review names what is in and what is not, and a call to a tool that is not there is refused by name.

## Proof

A target with one sound read and one draft that holds an open question: lint answers ok and lists the one set aside; the sound read is tried, passes the gates, and is published; the publish review names both; a trial of the other is refused with its question. A target with a name defined twice still fails whole. In field test 1, the scout is handed the system's whole description and reads are tried with no file deleted by hand.
