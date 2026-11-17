name: approvals

# Approvals

A write does not run until a person approves the exact diff. Nothing an agent can call approves it.

## What happens to a write

1. You call the tool. Inputs and the whole source are checked first. Bad input returns every problem at once and changes nothing.
2. The read steps run. The engine builds a preview: for each record, current beside proposed, classed `create`, `update`, or `unchanged`.
3. The run rests at `needs_approval`. The answer says so, with a summary and a path to the diff file. A person is needed.
4. A person gets a notification and a link to the diff page. They press Approve. The operating system's own prompt confirms it: a fingerprint, a PIN, or a password. The approval is bound to the hash of that diff and is used once.
5. You call `run.resume` with the run's identifier. The engine checks the grant again, reads each record, writes, reads back, and compares.

An approval is not reused for a changed diff. If a record changed since the preview, the run stops as `stale`, `changed_since_preview`. Start a new call. It gets a new preview and a new approval.

## What you do

- Say what is waiting and why, in a sentence. Give the run's identifier.
- Do not look for an approve tool, command, or route. There is none on a local machine.
- Do not try to reach the approval page, or the system prompt, yourself.

## What raises the bar

| Effect or hazard | Approval |
| --- | --- |
| `read` | None |
| `write` | Required, per policy |
| `irreversible` | A person, every run. No automatic retry |
| `data_elsewhere`, `sends_outside` | A person, every run. The diff says what else is at risk |
| `device_function`, `physical` | A person, every run. No standing approval, no pre-approval by policy |

A hazard raises the approval an effect asks for. It never lowers it.

## Publishing

`publish.request` asks for review. A person reads a summary ordered by risk: new surfaces and destinations first, then uploads and downloads, effects that rose, changed identity and read-back, changed expressions, changed descriptions. Only a person publishes. You cannot, and you cannot change a version once published.

## Decision points

An operation may declare `decision_points`: questions a person may have to answer during a run. A run asks only what its operation declares. The answer is recorded with who gave it.
