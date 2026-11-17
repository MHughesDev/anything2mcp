# 0151. The doors take up the action registry's second pass

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the designs of [`a2m-mcp`](../architecture/crates/a2m-mcp.md) and [`a2m-api`](../architecture/crates/a2m-api.md); two sentences of [spec section 221](../spec/2-kernel/221-the-tool-surface-agents-see.md) and four of [section 222](../spec/2-kernel/222-front-doors-and-the-local-engine.md). It supersedes point 6 of [record 0115](0115-the-mcp-endpoint-as-built-checks-in-front-of-the-sdk-and-a-redactor-on-the-way-out.md), which asked `tool.describe` before a call, and that record's sentence that `next` travels only in `_meta`. It adds to [record 0116](0116-the-rest-router-takes-its-caller-from-the-mount-and-chooses-its-status-in-one-table.md) the one route beside the document's own that no action generates.

## Context

[Record 0132](0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md) gave the doors what they had asked for: a tool list in one decided step, a run's events to follow, `next` inside a result, one type for the project folder, and a face that says which flag standard input fills. This record is what the doors and the command line did with it.

## Decisions

**The MCP door.**

1. **A published tool called by its own name costs one dispatch.** The door dispatches `tool.call` with the name. The dispatcher's `input`, `not_found` with no result says that no such tool is published, and is answered with the protocol's unknown-tool error, -32602, carrying the dispatcher's own words as the error's data. A run that failed for want of a record carries a result, and stays a result. *Instead of:* `tool.describe` first, which cost a second dispatch and a second recorded decision for every call.
2. **A list reads the published tools a page at a time** through `tool.search` with `full`, twenty to a page. A page the result size cap cut is asked for again at half the size. What is listed is unchanged: the control tools, then pipelines up to `tools.listed_limit`, or every published tool with `tools.full_listing`. *Instead of:* one `tool.describe` for each tool listed.
3. **`next` has one place: the structured content.** It carries the action, the name of its tool, its input, and when. The result's `_meta` keeps the warnings only. The name of the tool is given whether or not the toolset lists it, so the server instructions say what to do when it does not: call `a2m_action_run` with the action. *Instead of:* a second copy in `_meta`, which a client that reads structured content never saw.
4. **Progress is what the run does.** For a caller that asked, a call that starts a run is started with no wait, and the run's events are followed through `Kernel::next_events` until the run rests, ends, or the caller's wait is over. A call that continues a run the caller names is dispatched as it was written and followed meanwhile, from the run's last event on. Any other call counts two steps, as before.
   - **Which events are told:** a change of state, a step that began, the records planned, the stored preview, and, once for each page of events, how many records were verified, failed, or were read again.
   - **What is said:** a sentence of the door's own with a count, a step's number, or the name of a state in it. Nothing an event carries as text is passed on, so nothing a system said can read as an instruction.
   - **The answer:** a run that has ended, or is still worked on when the wait is over, is read with `run.get`. A run that came to rest is answered by the caller's own call made once more with no wait. The same call is the same run for as long as the run is unfinished, and only a call that starts or continues a run tells the run's person that it waits; `run.get` tells nobody (record 0132, decision 3). *Instead of:* `run.get` in every case, which left the person untold and the result without its link.
   - **A run whose state rests while its job is still in a runner's hands** is looked at again after 20 ms, then twice as long each time up to 2 s. The record that a job has ended appends no event, so `next_events` does not return on it. This is a timer where a condition is wanted; what would remove it is under "What would reopen it".
5. **The project folder is read from the request's extensions,** as the type `ProjectFolder` of `a2m-actions`, beside the caller, and handed to `Request::in_project_folder` with every dispatch. `bridge` names the folder it was started in, or the one `--project` gives, on every request, as a command does.

**The REST door.**

6. **A run's events are a route of the door's own, and not an action.** `GET /api/v1/runs/{run_id}/events` is fixed in the router beside the route of the OpenAPI document; `served_routes` lists both with an empty action, and the document describes both. No action generates it because reading a run's events is not one: `Kernel::events` records no decision and shows a caller only a run that the same caller started, which the decision about the call that started the run covered. *Instead of:* an action `run.events`, which would record a decision for every page of a stream; and authorizing the route "as `run.get`", which the design said and which is wider than what the kernel allows.
7. **One path, two forms.** A request that accepts `text/event-stream` gets a stream; any other gets one page as JSON, in the shape of every answer, with `after` and `limit` in the query. The stream starts after `Last-Event-ID`, or `after`; each event's `id` is its sequence number; it ends with one event named `settled` when the run has ended or rests with every event sent. A client follows a run across a rest by opening the stream again with the last identifier. The first page is read before anything is answered, so a refusal is an ordinary answer with its status and never the start of a stream. *Instead of:* a stream that stays open while a run waits for a person, which may be days.
8. **The router's own `ProjectFolder` is gone.** `a2m-api` re-exports the one of `a2m-actions`.

**The command line.**

9. **A flag whose face names a field for standard input** makes the command read standard input to its end, remove one line ending from the end, and send the text as that field; the flag is not sent. Giving the field as well is refused, by the names of the two arguments. The text is printed nowhere, and the message for input that cannot be read quotes nothing.
10. **A shortcut ends as a failure when it stops early.** It carries out the steps `Shortcut::on` gives, and then, if they all succeeded, ends with `input`, `action_not_built`, under the name of the action it stopped before, with the shortcut's note as the message and exit code 1. The step it stops before is never dispatched. *Instead of:* exit code 0 with a note on standard error, which a script would take for a shortcut that did all it says.

## Consequences

- Section 221 says where `next` travels; section 222 says what progress carries, what the events route is, and that two routes are no action's.
- The designs of both doors list what was built and what was not.

## What would reopen it

- `Kernel::next_events` returning when a run that was worked on comes to rest, and not only when an event is appended or the run ends. Both doors then drop their short second look.
- The kernel telling a run's person when the run comes to rest, whoever waits. The MCP door then answers every followed call with `run.get`.
- A client that needs a stream to stay open across a rest.
