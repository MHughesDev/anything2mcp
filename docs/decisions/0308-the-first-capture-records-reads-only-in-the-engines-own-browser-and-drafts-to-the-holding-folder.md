# 0308. The first capture records reads only, in the engine's own browser, and drafts to the holding folder

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** the scope of roadmap package E2 and what it needs, the handlers of `capture.start`, `capture.stop`, and `endpoints.infer`, roadmap package FT6.

## Context

Field test 1, finding 3: software that publishes no description of itself gives the scout nothing to draft from. The design's answer is a capture: a person uses the software while the engine records. The three actions are in the catalog with no handler. Package E2 waits on every sign-in provider (2.6), which holds the web path behind work it does not need.

## Decision

1. **A capture runs only in the engine's own browser,** in the observing tab that exists, on a surface the target declares. Rule 5: nothing is read from a person's everyday browser, and nothing is placed between another program and the network.
2. **What is recorded of a request is its method, its path as a template, the names of its parameters, and the shape of its answer.** Values are scrubbed as they are recorded, by the scrubber fixtures use, before anything is stored. A header that carries a credential is recorded as present and never by value.
3. **The first capture drafts reads only.** `endpoints.infer` drafts an endpoint and a read for a request that is a `GET`, was answered with a success and a structured body, and was seen at least twice with one shape. Every other request is listed in the report as seen and not drafted, with the reason.
4. **Every draft from a capture goes to the holding folder** ([decision 0298](0298-a-draft-with-an-open-question-waits-in-a-holding-folder.md)), with the question a person must answer: whether the read is one the software means to be called this way. A capture is evidence at `inferred`, and a person moves a draft out of the holding folder by answering.
5. **A placeholder in a path is drafted only from two requests that differ in that one part.** One request gives a fixed path.
6. **E2 needs the sign-in window (2.3) and not every provider.** Writes from a capture are a later package and wait for the first verified web write.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A proxy that records any program's traffic | Rule 5, and it would see credentials of software the target does not name |
| Draft writes from what a person did | A write needs a read-back the capture cannot know. It would be drafted `irreversible`, which is the wrong default to hand a person |
| Write the drafts in place | An inferred read compiled beside proven ones looks as good as they do |

## Proof

In the test application, a page that makes its own requests: a person's use, replayed by the test, gives held drafts for the reads seen twice and none for a `POST`; a planted credential is in nothing stored. FT6: reads drafted from a capture of software with a sign-in and no description return what its own database holds.
