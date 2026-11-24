# 0399. A read that waits for a sign-in, as built, and a surface added with its own fields

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the scout's `try` stage and summary, `draft.try`, and `target.add` in `a2m-actions`; the field runner `tools/field_test.py`. Builds [decision 0303](0303-a-declared-sign-in-is-kept-and-a-read-that-waits-for-one-is-reported-as-waiting.md) (roadmap FT9) and closes field findings 11, 17, 18, and 23 ([results](../reviews/field-tests.md)).

## Decision

1. **Waiting, as built.** A drafted read whose last trial rests at `needs_login` is `waiting`, whatever stopped it there: the keeper's refusal before any request, which is the case decision 0303 names, or a system's own answer that the session ended. Its entry in the report names the surface its binding acts on and `next`, `session.login` with the target and that surface. The trial action's answer for such a trial names the same `next` in place of `run.resume`.
2. **The counts add up with five terms, not four.** The try stage's `counts` are `drafted`, `proven`, `waiting`, `failed`, `not_tried`, and `held`, and drafted is the sum of the other five. Decision 0303 named four; a read the scout has no input for, or no environment for, is neither failed nor held, and leaving it out would make the sum false. `failed` holds every read whose trials ended without a proof: failed, differed, or not ended. The summary says the counts in one line, and the surface the waiting reads need in another.
3. **The same trial made again after a sign-in is the first one, taken up.** A trial is a run, and ledger rule 6 gives the same call back its unfinished run. So `draft.try`, when the run it is given back was made before the call and rests at `needs_login`, queues that run's job once more and waits for it: the same trial made after the person signed in reads through the session. A trial made now that rests is not queued again.
4. **The field runner signs in as a person would.** A `person` step may say `signs_in`, a map from a credential's role to its value, typed into that role's field of the page of `session.login`. An `http` step may say `secret`: a value the system issued, such as a token, kept by name as a secret of the run. Every step's output, the report, and the runner's own messages are cleared of each secret, and the step that made it shows nothing of the answer. An `unseen` step says that the secret is in no file of the engine's home, the project, or the harness's log.
5. **`target.add` takes a surface's own fields** (finding 17). `fields` is a map of the surface's other fields to texts, numbers, or true or false, given only with `address` and `kind`. The address key, `kind`, and the kernel's own surface fields are refused there; which names a kind takes is its pack's, and the draft compile that already checks the file before it is written refuses a name the kind does not declare, with the compiler's message. Each value is written as data.
6. **The summary names a surface's stop** (finding 18). A surface whose discovery stopped says the code in its summary line, and its line under "Not looked at" says the code as a discoverer's failure does.
7. **A second target's sign-in with no act is right** (finding 23). The person's presence did not carry over. The second target was a published version, published by the person's own act, and its password was stored under that target's own reference. A sign-in that a published version declares, through a provider that works from a stored credential alone, is made by the call that asks ([decision 0085](0085-the-session-keepers-interface-as-building-it-settled-it.md)), within the attempt limit and at most four times an hour without the person ([decision 0269](0269-a-stored-credentials-attempts-outlive-a-restart-and-sign-ins-without-the-person-are-limited.md)). The first target's sign-in needed an act because only its drafts declared it ([decision 0248](0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md)). Neither rule 4 nor a person's own session was crossed: the session is the person's, and the credential never reached a caller.

## Why not

- **A trial that waits is cancelled, so the next one is new.** The run's evidence, its trace and its events, would be split over two runs for one trial, and a trial an agent made would end without anyone asking.
- **`target.add` checks field names against the kind's schema itself.** The schema is the compiler's; `a2m-actions` would hold a second reading of it. The compile it already runs gives the same refusal by the same rule.
- **A secret in the recipe.** A recipe is kept outside the repository, but a token written in it would be in every copy of it. The system issues one at the run, and the runner keeps it as a secret.

## What the work showed

Field test 1 with the sign-in, three runs alike: before the sign-in each of the four reads waits and names `session.login` for the surface `http`; the person types the system's own token on the engine's page; the four reads then return what the system's own store holds. Two steps were added that a person takes by hand: an `auth.yaml` for each target, because the drafter drafts no sign-in from a description's security definitions, and `unencrypted_credentials: accepted` on the surface, because the run's closed network reaches the system over plain HTTP at an address that is not this machine's.

## Proof

- `a_read_that_waits_for_a_sign_in_is_waiting_and_the_counts_add_up` (`handlers/scout.rs`) and the trial's `next` in `an_agent_writes_reads_tries_and_tests_drafts_inside_the_project_alone` (`crates/a2m-actions/tests/build.rs`).
- `a_surface_is_added_with_the_fields_its_kind_declares_and_checked_before_it_is_written` (`crates/anything2mcp/tests/target_add.rs`).
- `a_refused_surface_is_summed_up_with_the_code_of_its_stop` (`handlers/scout_report.rs`).
- The runner's `Secrets` and `PersonStep` tests in `tools/test_field_test.py`.

## Spec sections

- [215. Sessions and the session keeper](../spec/2-kernel/215-sessions-and-the-session-keeper.md)
- [402. The scout, and who does what](../spec/4-building-tools/402-the-scout-and-who-does-what.md)
- [`a2m-actions`](../architecture/crates/a2m-actions.md)
