# 0163. A person's act an agent asks for is carried out on a page of the person surface

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the designs of [`a2m-actions`](../architecture/crates/a2m-actions.md) and [`a2m-local`](../architecture/crates/a2m-local.md); sentences of spec sections [220](../spec/2-kernel/220-actions-and-the-authoring-interface.md), [222](../spec/2-kernel/222-front-doors-and-the-local-engine.md), and [503](../spec/5-security/503-approval-and-sign-in-surfaces.md); the failure codes of the [technical reference](../reference/technical-reference.md#failure-codes). Takes up the way out [decision 0158](0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md) named.

## Context

Since decision 0158 every caller of the local channel is an agent, and a person's act (publishing a version, releasing a kill switch, loosening a protected setting) was refused `person_required` before the person surface was asked. With an engine running, nobody could publish. Sections 222 and 503 intend what an approval already has: a page the person surface serves for the moment, on which a press raises the operating system's presence prompt.

## Decision

| Question | Decided | Alternative weighed |
| --- | --- | --- |
| Where the route starts | In the dispatcher's person step. An act the definition marks as a person's, asked for by a caller marked as an agent, is not refused: it is handed to the person surface with `PersonSurface::ask_person_act`, whose default refuses by name (`policy`, `person_required`), so a surface with no page for it behaves as before. A caller that is no agent is still asked for presence directly, as in one process at a terminal. `publish.request` hands `version.publish` to the same route; it publishes nothing itself | A new action an agent calls to open the page. It would be a second way to the same act, and the dispatcher's step is where a person's act is recognised already |
| What the caller is told | `exit` person needed (code 3, REST 202, an MCP result that is not an error), a result `state: needs_person` with the act's identifier, the action, the page's `link`, what the person is told, and `expires_at`; and `next`, the same call again. Every output schema of a person's act admits that result (`ActionDef::output` is either the action's own or this one). The same call, while the act waits, gives back the same page; once the person carried it out, declined, or let the window end, it gives back what became of it, for the approval window | The link inside `next`. `next` names an action and its input; the link belongs in the result, as for a run that waits for an approval |
| Where the act waits | In memory, in the kernel, under sixteen random bytes: the action, the input, the caller that asked, the project folder as it was checked, at most 16 at once, each for `policy.approval_window_minutes`. What became of the last 64 is kept for the same window. Nothing is stored: an engine that stops forgets them and nothing was done | The store. An act that waits is a page that waits, and pages die with the process |
| What the page shows | Exactly what will be done: the action, its summary, every input field by name with its value as text (redacted, cut at 400 characters), who asked through which door, and, for a publish, what each tool may do, the most dangerous first. Every value is escaped and placed as text. The page has its own one-time value, and the same origin guard and headers as the approval page | Rendering the input as markup the action chooses. A value is a caller's text |
| What the person publishes | The plan they were shown. When a publish is asked for, the engine compiles the drafts at the version they will be published as and pins `version` and `plan_hash` into the act's input; at the press `version.publish` compiles again and refuses `stale`, `drafts_changed`, when the hash differs. A lint failure is answered at the ask, with no page | Showing the target's name only. Drafts an agent changes between the page and the press would be published under the person's name |
| What a press carries out | The page's press raises the presence prompt, one at a time with every other prompt of the surface. Only a proof from it, made with `PresenceProof::for_act`, carries the act out, through `ApprovalDesk::carry_out`, which checks that the proof was given for this act, is of the person the asking caller acts for, and is at most two minutes old; then decides the act again, recorded, for the person (the same principal, `is_agent` false), looks at the project folder again, runs the handler with the proof, and keeps what it came to. A proof for another act, for no act, of another person, or old does nothing, and the act goes on waiting; a second press, or a press after the window, finds nothing | Running the stored decision. The decision was made for the agent; the person's act is decided for the person |
| A press that is declined, or silence | `ApprovalDesk::decline_act`; the act is kept as declined (`policy`, `approval_refused`) and nothing is done. The page's window ends as an approval page's does | |

## Consequences

- A person publishes through a running engine, and so through any door: an agent asks, the person reads the page, presses, and answers the operating system's prompt.
- The person's act is carried out by the engine's own handler, in the engine's process; the page holds no engine logic.
- The limit of decision 0158 for prompts with fixed text stays: on Linux the dialog cannot say which act it is for, and the page says to answer only right after one's own press.
- Not built: a person's act on a server's console, which needs the organization's sign-in (section 503).
- Not run: anything on Windows and macOS.
