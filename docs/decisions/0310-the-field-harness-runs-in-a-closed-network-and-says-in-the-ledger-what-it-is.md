# 0310. The field harness runs in a closed network, and says in the ledger what it is

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Supersedes [decision 0304](0304-a-field-build-stands-in-for-the-person-in-a-field-test.md).

**Date:** 2026-11-23

**Changes:** a new program under `tools/field-harness/`, one name in `a2m-local`'s list of prompts and in the policy engine's list of presence checks, the `prompt` of the command line's `Host`, `tools/field_test.py`, the packaging check, roadmap packages FT4 and FT9.

## Context

Decision 0304 let a field test pass a presence prompt by writing the change's hash to a file. A review against the code found it wrong in six ways:

1. The prompt is shown only the first twelve characters of the change's hash (`PromptText`, in `a2m-local`).
2. The ledger can record only the name of an operating system's prompt. A field prompt would make the ledger say that a person passed polkit.
3. A sign-in, a publish, and a watch's confirmation are a person's acts. Each needs a proven presence, which a prompt that passes "only an approval" does not give.
4. Any program running as the user can read an approval link and write the file. That is an approve route, which the product has none of on a local machine.
5. An irreversible operation would be approved by no person, against rule 3.
6. "Every address is on this machine" is true of a local tunnel to a real system.

## Decision

1. **The field harness is a program of the workspace under `tools/field-harness/`, built from the product crate's library.** The command line's `Host` gains a `prompt`, which the shipped program never sets. The harness sets it to the field prompt and is otherwise the product's own `serve`. A cargo feature is not used, because a workspace build would turn it on in the shipped program too. The audit's layout and layering table name the harness in the change that creates it.
2. **The field prompt answers on a pipe that only the runner holds.** The runner starts the harness and keeps its standard input. When the field prompt is asked, it writes the prompt's text to its standard output and waits for one line. It passes only when the line is the short hash the prompt was shown, and the runner sends only the hash of the change it means to approve. No file and no address can answer it.
3. **The ledger says what passed.** `field_harness` is added to the names of prompts and to the presence checks, and is recorded as that. The shipped program has no prompt that answers with it, and a test of `system_prompt_for` holds that on every system. A version published under a field check is shown as such wherever its approval is shown.
4. **The field prompt passes the person's acts a field test needs:** an approval of a write, a sign-in on the engine's own page, and a publish. It refuses an approval of an `irreversible` operation, always (rule 3).
5. **The harness and the systems it is tried on share a container network with no route out.** The runner makes a network with `--internal`, starts the system's container and the harness's container on it, and runs the harness's commands inside its container. At start the harness tries one public address and refuses to serve if it is reached. The marker file in its home folder is kept, and it guards only against starting the harness by accident.
6. **The harness is never packaged or left behind.** The runner builds it into a build folder of its own and deletes the folder after the run. The packaging check builds only the product's binary and fails when a wheel or npm package holds any binary but the expected ones.
7. **What is still true:** anyone who can do what the runner does on this machine can approve through the harness. What makes that safe is point 5: all the harness can reach are copies the runner started, holding invented data, and thrown away.

## Alternatives

| Alternative | Why not |
| --- | --- |
| The hash in a file (decision 0304) | Point 4 of the context |
| A Rust test in the repository that reads a recipe from a path it is given | It runs the engine in the test's own process, so the product's program and its `serve` are not what is tested |
| Answer approvals as `Confirmed`, recorded as `not_available` | Truthful for a write, but a person's act refuses it, so a field test could not sign in or publish |

## Proof

Roadmap FT4. Through the fault proxy, a write whose answer is lost is reconciled, and the system's own database holds it once. The ledger records its approval as `field_harness`.

Four tests hold the protections:

- The harness refuses to serve when a public address is reached.
- A line with another hash leaves the write resting.
- An irreversible operation is refused.
- A wheel holds the expected binaries only.
