# 0999. A capture is asked for by `capture.start`, begun by its person on the page, and held in the engine's memory

**Status:** Accepted. Design decision, made by the worker who built package E2b, under the lead's brief. The owner had not read this record when it was merged. It changes a public interface by addition: the approval desk gains `end_capture`.

**Date:** 2026-11-24

**Changes:** [`a2m-actions`](../architecture/crates/a2m-actions.md), which gains the handlers of `capture.start` and `capture.stop`, the capture job, two failure codes, and `ApprovalDesk::end_capture`; [`a2m-local`](../architecture/crates/a2m-local.md), whose person surface gains the page of a capture that records, and whose act page now names its failure code when the presence prompt is not passed; the [action catalog](../reference/action-catalog.md). It builds point 2 of [decision 0392](0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md), as [decision 0395](0395-a-person-driven-discoverer-is-called-through-a-method-of-its-own-and-the-scope-is-left-as-it-is.md) shaped it, and settles what they left to the builder.

## Context

Decision 0392, point 2, says who may ask for a capture (anyone who may build, an agent included), who begins it (only its person, on the engine's page, past the presence prompt, told what is kept), how it ends (its person ends it there or by closing the window, `capture.stop` asks for the end, a capture past its bound is ended and says so), and which side of the kernel boundary holds what. It names two actions and leaves open which action the person's press carries out, where a capture is kept, how the page ends one, how a refusal at the page is named, and what `capture.stop` answers.

## Decision

1. **One action asks and the same action begins.** `capture.start` with no `capture` asks: it keeps a capture as asked and hands its person a page of a person's act, as `watch.create` does, and answers `needs_person` with the page's address. The act the page carries out is `capture.start` again, with the capture's identifier pinned in its input. The handler begins a capture only when the call names one and carries a proof of presence; a call that names one without it, an agent's, is `policy`/`person_required`. The catalog gains no action: a separate confirm action, as watches have, would be a second name for one step that nobody but the page calls.
2. **What the page says.** The act's page lists, from the capture as the kernel holds it, the surface, the target, and the environment, the bound, how it ends, that the engine sends nothing while it records, and what is kept: the structure of what the person uses, never a value. After the press passes the prompt, the answer is the capture's own page, with Stop recording. It lives for the capture's bound and a minute more, and holds the listener open, and the engine from going idle, while it does.
3. **Ending it on the page asks for no prompt.** Stop recording carries the page's one-time value and goes to `ApprovalDesk::end_capture`, which ends a capture of that person and nothing else. Stopping a recording starts nothing and writes nothing to the system, so a second proof of presence would protect nothing. A capture that has ended is left as it is.
4. **The job.** Each discoverer a person drives that suits the surface is called by `DiscovererHandle::watch` with one capture end, which `capture.stop`, the page, and the engine's close set; the handle holds each to its manifest's bound and names `seconds` when it passes. What each finds passes a sink with the rules of a discovery's: a finding that holds a registered secret is withheld and counted as dropped, and the bytes and the count are bounded. The findings go to the store's `findings`, and what each discoverer reported to `actions_discovery_results` under the capture's identifier, as a discovery's are (decision 0170).
5. **Who ended it.** A capture says it ended `asked` (by `capture.stop`), `person` (on its page), `bound`, `not_begun` (stopped before its person began it), `engine_closed`, or `returned` (its discoverers returned by themselves). The first to ask for the end is the one named; a capture no one ended whose discoverer reports the bound ended at the bound.
6. **Where it is kept.** A capture is held in the engine's memory, one open capture of a target at a time for a person; a second `capture.start` gives back the one that waits or records. An engine that closes ends every capture that records and stores what each found first; an engine that starts again has none under way. A recording cannot outlive its engine, so a stored row for one would only ever say that it was cut short.
7. **What `capture.stop` answers.** The capture named, or the person's newest of the target: its state, how and when it ended, how many findings were stored, and each discoverer's report. Called again, it gives the same answer. Package E5 adds the report's reference (decision 0400).
8. **A refusal at the act page is named.** A press whose prompt is dismissed, denied, or not available answers `presence_failed`, and one met by a prompt already up `prompt_busy`, in the header a refusal of an approval already carries. So a script that presses the button is told by name why nothing was done.
9. **Two codes.** `input`/`capture_not_offered`, when no enabled pack watches a person's use of the surface, and `stale`/`capture_ended`, when a capture was ended before its person began it.
10. **The settings stay not built.** `authoring.capture.enabled`, `.values`, and `.limits.*` are read by nothing still: a capture is not switched off by a setting, it keeps no value, and it ends at its discoverer's own bound. What each says instead is now true of the built capture.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A new action the page carries out, as `watch.confirm` is | A catalog row, a generated face in every client, and a rule in the dispatcher, for a step only the page takes |
| A row in the store for each capture | A recording cannot go on across a restart, so the row would hold only how it was cut short. The findings and each discoverer's report are stored |
| The presence prompt for Stop recording too | It protects nothing: ending a recording writes nothing and starts nothing |
| One capture runs all a surface's discoverers one after another | A discoverer a person drives returns only at the end, so the second would watch nothing |

## Proof

`a_capture_records_only_once_its_person_begins_it_on_the_page_and_ends_as_asked`, in `crates/anything2mcp/tests/capture.rs`: the kernel and the local person surface joined as the product joins them, with a pack of the test's own whose discoverer a person drives counts what it is asked. An agent's capture waits and asks nothing of the discoverer; the agent naming it is refused; a press whose prompt the test does not pass is refused as `presence_failed`, and still nothing is asked; the person's press begins it; `capture.stop` ends it, and `finding.list` lists the finding kept and not the one that quoted a planted credential; a second capture left alone ends at its bound and says `seconds`; a third ends on its page as `person`; and the credential is in no file under the engine's home or the project.

## Revisit if

Package E5 needs a capture to be found after the engine that ran it has closed, or a second person surface, a console, must end a capture.
