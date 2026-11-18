# 0234. A watch is a read repeated on a schedule a person confirmed, and a change is reported, never acted on

**Status:** Proposed. Design decision: any agent may accept it by planning the package that builds it, in a record that says so, or supersede it with evidence. Written by the breadth review of 2026-11-18; the owner has not read it. Nothing here is in force until then, and no rule or owner decision is changed by it.

**Date:** 2026-11-18

**Answers:** the first finding of the [breadth review](../plan/breadth-review-2026-11-18.md): every tool is asked and none tells. Takes up part of what [section 704](../spec/7-project/704-readiness.md) leaves undesigned ("triggers, and runs started by an event"): the reading half. Starting a run because something changed stays undesigned.

**Would change, when built:** the [action catalog](../reference/action-catalog.md) (new actions); spec sections [221](../spec/2-kernel/221-the-tool-surface-agents-see.md) (a watch's face), [222](../spec/2-kernel/222-front-doors-and-the-local-engine.md) (an engine with a watch is not idle), [401](../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md) (canaries may be built as watches), and 704; the crate designs of `a2m-actions`, `a2m-ledger`, `a2m-sched`, `a2m-mcp`, and `a2m-api`; the [glossary](../reference/glossary.md) (watch, change); SCOPE.md.

## Context

An agent works in turns. Between turns it asks nothing, and a system it cares about changes on its own: an order ships, a row appears in a table, a file is replaced, a device's reading crosses a limit. Today the only way to learn of it is to call the same read again, and to compare the answers itself, holding the old one in its context. Nothing in the product tells.

The kernel already holds most of the parts under other names: the scheduler keeps deadlines in the store's clock and a task sweeps when one is due (`a2m-sched`); a run's events are served as a stream a client resumes from its last event (`a2m-api`); a write's diff sorts records into created, updated, and unchanged; section 401's comparison of two observations reports "removed" only when the newer one's coverage is complete; `a2m-core` gives canonical digests of normalized values; `a2m-expr` evaluates conditions that always end. The 2026-07-28 revision of the protocol has `subscriptions/listen`, on which a server tags notifications that a resource changed ([research note 18](../research/18-mcp-server-base.md)).

## Decision

### What a watch is

A **watch** is one published read, called with one fixed input, again and again at a fixed interval, until an end. Each call is a **check**: an ordinary run of that read, with its ledger rows, its trace, its rate budget, and the person's session lease. After each check the watch compares what the read returned with what the previous check returned, and records each difference as a **change**. That is all a watch does.

| Part | What it is |
| --- | --- |
| The read | One published operation whose compiled effect class is `read`, named with its version (`example_order_list@3`). A draft cannot be watched: a draft runs only as a try in the sandbox |
| The input | Fixed when the watch is made, checked against the operation's schema then, and never changed. Nothing from a result is ever carried into a later input, so a steered agent or a system's own text cannot move what a watch reads ([section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md)) |
| The interval | A whole number of seconds, at least `watch.shortest_interval_seconds` (60 to begin with) and never shorter than the surface's declared rate allows. Due times are the start plus a whole number of intervals, so they never drift and nothing in them is random |
| The end | Required: a time no later than `watch.longest_days` (7 to begin with) after the person confirmed it. A watch also ends when its condition holds, when the person or its caller ends it, or when its operation's version is withdrawn |
| The condition | Optional: an expression over the newer check's result, in the operation's output names (`result.status == 'shipped'`). When it holds, the watch records `condition_met` and ends. This is "watch a value until it holds" for any read of any pack |
| What is compared | For a read whose output is a list of records with a declared key ([section 209](../spec/2-kernel/209-record-identity-and-dependent-records.md)): each record by its key. For any other read: the whole result. Values are normalized by their field types ([section 210](../spec/2-kernel/210-sources-and-value-normalization.md)) and reduced to a canonical digest. `ignore` may name output fields left out of the digest; each name is checked against the operation's output schema when the watch is made |

These starting values are settings, to be measured and revised with a record: `watch.shortest_interval_seconds` 60, `watch.longest_days` 7, `watch.most` 20 open watches per person.

### What a change is

| Change | When |
| --- | --- |
| `added` | A key in the newer check that the older did not hold |
| `changed` | A key in both, with different digests; or, for a read compared whole, a different digest |
| `removed` | A key the older check held and the newer does not, **only when both checks were complete**: each run ended `completed`, and the read reported no more records than it returned (the data pack's `more`, a pager's last page) |
| `unseen` | The same, when either check was not complete. It says the key was not seen, never that it is gone |
| `condition_met` | The condition held on the newer check |
| `gap` | A check did not complete (its failure's class and code are kept), or due times passed while the engine was not running (the first and last missed due time are kept) |
| `ended` | The watch ended, and why |

A change holds its kind, the record's key where there is one, the time, and the identifiers of the two check runs. **It holds no value.** An agent that wants the values reads the newer check's run with `run.get`, whose result is kept like any run's. What a watch keeps of its own is each check's map from key to digest, so no business value is held beyond what the runs already hold, and nothing outlives the runs' retention class ([section 217](../spec/2-kernel/217-traces-evidence-and-repair.md)). Changes are append-only, like a run's events.

### Who makes one, and who confirms it

- An agent asks with `watch.create`. The watch starts in the state `proposed` and does nothing.
- The person confirms it on the person surface, as they carry out a sign-in an agent asked for ([decision 0205](0205-a-sign-in-an-agent-asks-for-is-carried-out-by-its-person-on-the-engines-page.md)), through `Kernel::ask_person`. The page shows the read, its input, the interval, the end, how many checks that is and what share of the surface's rate budget, and what is kept. Confirming is a person's act on a page, not an approval of a write: it needs no presence prompt, and no write can ever follow from it.
- The reason a person confirms a read at all: a watch goes on acting after the agent's turn has ended, renews the person's session while they are away, and keeps a record of what it saw. A read an agent calls once does none of these.
- The person, or the caller that asked for it, may end it at any time with `watch.end`. A kill switch that covers the target stops its checks; they resume when it is released, with a `gap` for the time between.

### How it runs

- Each due time is a deadline in the scheduler. When it falls, the kernel enqueues the check as a job of a read run, under the person's identity and the toolset the watch was made in, with the watch's identifier as the run's asker label. The scheduler's budgets apply as to any read; a check that waits for its budget is late, never skipped.
- **An engine with an open watch is not idle.** The solo engine, which stops when idle (section 222), stays running until its last watch ends. It does not start itself at login; that would be a separate choice.
- **A due time missed is never made up.** When the engine starts after missing due times (the machine slept, the engine was stopped), it records one `gap` for them and runs one check.
- **A session that needs a person pauses the watch.** A check that ends `needs_login` records a `gap` with that code, and the watch waits in the state `waiting_for_sign_in` until the session keeper reports the session valid again. The watch never asks the person to sign in by itself; the agent, told by the change, may call `session.login`.
- A check that fails `drift` records a `gap` and pauses the watch until a person ends it or a new version is watched. A watch never moves to another version on its own.

### How a caller is told

Every client is told by asking; a client that listens is told sooner. Each way carries the same changes, and no text a system gave.

| Face | What it does |
| --- | --- |
| `watch.changes` (listed in `use`) | The changes after a sequence number, waiting up to `wait_seconds` (at most 60) for the first. Works on every revision and every client, with no session |
| MCP resources | Each watch is a resource. `resources/read` gives its state and its latest changes. A client that declared `subscriptions/listen` is sent a notice that the resource changed, carrying the resource and nothing else, when a change is recorded. Built with the MCP door's resources, not before |
| REST | `GET /api/v1/watches/{watch_id}/changes`, a page or a stream that resumes from its last change, built as the route of a run's events is: not an action, and showing a caller only the watches of the person it acts for |
| The clients | `watch_changes(...)` in Python and Node, generated as every action's method is |

The other actions: `watch.create`, `watch.get`, `watch.list`, `watch.end`. Who may call each is Viewer: a watch only reads. `watch.create` is listed in `use`; `watch.get`, `watch.list`, and `watch.end` are reached through `a2m_action_run`.

### What a watch never does

It never starts a write, or any operation but its own read. It never changes its input, follows a value from a result, or calls a model. It never reports a removal it cannot prove, keeps a value, asks the person anything once confirmed, runs past its end, or runs while a kill switch covers its target. Starting a run because a change was seen is a trigger, and triggers stay undesigned (section 704); a watch is built so that one can later be put after it, under the policy rules of [section 212](../spec/2-kernel/212-policy-and-approvals.md) and [section 604](../spec/6-running-it/604-management-and-operations.md).

### Where it lives

All of it is kernel, and no pack changes: any read of any pack can be watched. The tables (`watches`, `watch_checks`, `watch_changes`) are in `a2m-ledger`, by a migration; the deadlines in `a2m-sched`; the comparison and the actions in `a2m-actions`; the faces in `a2m-mcp` and `a2m-api`. No door decides anything, and the policy engine decides each action as it decides every action. Hypothesis H-26 is tested again: the engine must not learn the name of any mode to watch it.

**Later, and not in the first slice: signals that wake a check early.** A pack's way of waiting (the stream pack's `await`, a file system's change notification, an event stream from a service) may bring a due time forward. A signal is never a change by itself: the change is what the next read shows when compared. So a system that pushes is used without trusting what it pushed.

## What proves it

Tests against the test application, `targets/example-app`, each asking the stand what it stored ([test application plan](../plan/test-application.md)):

1. `a_watch_an_agent_asks_for_does_nothing_until_its_person_confirms`: no check runs and the stand counts no read until the person surface's act is carried out.
2. `a_watch_reports_the_record_the_stand_changed_and_no_other`: a list read of the stand's orders is watched; one record is changed through the stand's control interface, outside the product; the next check records one `changed` with that key; the stand counts no write by the product.
3. `quiet_checks_record_no_change`: ten checks with nothing changed record nothing (the hypothesis below).
4. `a_list_cut_short_never_reports_a_removal`: with more records than the read returns, a record deleted through the control interface is `unseen`, never `removed`; with the full list, it is `removed`.
5. `a_missed_due_time_is_a_gap_and_is_not_made_up`: with the store's clock moved past three due times while no engine runs, the next start records one `gap` and one check.
6. `a_revoked_session_pauses_the_watch_with_needs_login`: the control interface revokes the sessions (test application case 14, planned and not yet built; the slice builds it or waits for it); the next check records `gap` with `needs_login`, and no check runs until the person signs in again.
7. `a_condition_that_holds_ends_the_watch`.
8. Fault injection: a connection lost during a check retries as its class allows, then records `gap`; a kill switch stops checks and its release resumes them with a `gap`.

**A hypothesis for the register**, under its next free number. A canonical digest of normalized values, compared across repeated reads, reports every change a person cares about and no change across quiet checks. Refuted if a common read's normalized values move on every read (a time of last view, a counter), so that `ignore` would be needed on most watches; then the comparison needs a better default than "every output".

## Alternatives weighed

- **Triggers first: start a pipeline when something happens.** That is the half that acts, and it needs standing approvals, hazards, and a named principal (section 604). The reading half is useful alone and is the base the acting half would stand on.
- **Events only: webhooks, stream messages, file notifications.** Decision 0208 lets nothing listen for another machine; most systems the packs reach emit nothing; and an event's content is a system's claim. Repeating a read works for every pack and is verified by the same comparison as everything else. Events are kept as signals that wake a check early.
- **Leave it to the agent.** The agent is not running between turns, holds no baseline, and spends its context on comparing. The product is better placed: it holds the normalized form and the run evidence already.
- **An agent may start a watch without a person, within limits a setting sets.** Lighter, and it may come: a watch only reads. It is not the first form because a watch renews the person's session while they are away and keeps a record, which a single read does not. If confirmations prove a burden in use, a setting with a ceiling of checks per day, like a pre-approval rule, is the next step, with its own record.
- **A resource per published read with a subscription, and nothing else.** Only for clients that listen, and only over MCP. An action in the registry is every door's at once, as the registry rule requires; the resource is one of its faces.
- **Keep the values and show them in the change.** Easier for the agent, and it would make the product hold a second copy of the system's data with a lifetime of its own (section 401: an overlay never holds a system's data beyond what evidence needs). The newer check's run already holds them.
