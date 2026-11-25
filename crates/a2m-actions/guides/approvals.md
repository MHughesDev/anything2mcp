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

`publish.request` asks for review. It answers `needs_person` with a link to a page: give the person the link and stop. The page shows exactly what will be published: the target, the version, the hash of the compiled target the drafts compile to, and what each tool may do, the most dangerous first. The person presses Carry out and answers the computer's own prompt; only then is the version published, once, as the person. If the drafts change before the press, nothing is published. Call `publish.request` again afterwards to be told what was done. Only a person publishes. You cannot, and you cannot change a version once published.

When a version is in force, the page shows, right after where it leads, each tool whose change breaks an agent that calls it as it was, and why: a tool gone, an input gone or now required, a type changed or its words narrowed, an output gone or now possibly absent, the effect risen, the hazard added or changed, the record's key changed. The answer names the same under `breaking`. Nothing is refused for it: tell the person, and let them decide. `version.diff` gives the same classification for any two published versions. Not shown yet: changed read-back, expressions, and steps.

## Run questions

An operation may declare run questions, under the key `decision_points`: questions a person may have to answer during a run. A run asks only what its operation declares. The answer is recorded with who gave it.
