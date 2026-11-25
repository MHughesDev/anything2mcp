# 0998. A run's trace is summarized for its person, and exported as one file in the working folder

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Changes:** `run.trace` and `trace.export` in `a2m-actions`, built under [decision 0465](0465-while-the-first-release-waits-on-the-owner-agents-build-the-one-machine-packages-moved-after-it.md); the [action catalog](../reference/action-catalog.md).

## Context

Spec [section 217](../spec/2-kernel/217-traces-evidence-and-repair.md) says that `run.trace` gives a summary of a run's trace with links to its captures, and that a whole trace is never returned inline. It says that `trace export` writes one archive with a manifest, for attaching to a bug report. It names no format, no place, and no rule for whose run may be read. The engine keeps a run's captures in two places. Some are references in the step trace's lines. Others are blobs that the run's events name.

## Decision

1. **`run.trace` is for the run's own person,** as `run.get` is, under the viewer's role. Another person's run is `not_found` to them. For each step it gives the sequence number, the pass, the phase, the kind, the outcome, and the time, with the operation and the mode where a pass begins. It also gives the count of values, the step's capture references, and every capture the events name, each by `blob:` reference with its name and media type. It gives no value. It lists at most 200 steps and counts the rest. It gives the trace's check and, unless the trace is broken, its signature, counts, and limitations.
2. **`trace.export` is the auditor's,** as `trace.show` is, and it writes one file: `.anything2mcp/exports/<run>.json` in the call's project folder. `init`'s `.gitignore` already keeps that working folder out of the project's history. The file is one JSON document. `manifest` holds the format `anything2mcp.trace_export`, version 1, the run's facts, the trace's check and signature, and one entry for each piece. `events` holds the events as `trace.show` gives them, `trace` holds every line of the step trace, and `captures` holds each text capture's text.
3. **A piece is left out, and named with why, rather than written:** an image, because it is not text; a capture that can no longer be opened; and any piece that still holds a registered secret at a last check on the way out (rule 4).
4. **It is written and then proven.** The file is written by way of a new file that then takes the name, never through a link, inside the project. It is then read back. The answer gives the file's digest only when the bytes read back are the bytes written. Otherwise the answer is `stale`, `export_changed`. A second export of the same run replaces the first.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A zip or tar archive with the captures as files | It needs a new dependency or an archive writer of our own, and the first-release captures are text. It can follow once images are exported |
| `run.trace` bound to the workspace, as `trace.show` is | A viewer would read other people's runs. Reading another person's evidence is the auditor's role |
| Writing the file outside the project, in the engine's data folder | A person attaching it to a report would have to find a folder the engine keeps for itself |

## Revisit if

Images must be exported for a report, or a field test finds the single file too large to attach.
