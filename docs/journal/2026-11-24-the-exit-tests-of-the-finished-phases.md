# 2026-11-24: the exit tests of the finished phases

The entry before this is [a gate before every push](2026-11-24-a-gate-before-every-push.md). This one is owed by [decision 0390](../decisions/0390-the-open-questions-of-2026-11-24-are-settled-by-the-lead-and-the-first-release-is-what-progress-counts-toward.md), point 6: for each phase whose packages are all done, its exit test, the test that is it, and its last result. No product code changed; this entry only reads the repository.

## How to read it

- **The phases and their exit tests.** The Build Order's phases are numbered as in the [roadmap](../plan/roadmap.md) (Phase 1 is its milestone 1, Phase 2 is milestone 2, Phase 4 is milestone 6, and so on). The exit tests are those of the first design's build plan, kept in [the archive](../archive/spec-2026-11/36-build-plan.md) and in [the phase plans](../archive/plans-2026-11/phase-map.md); the roadmap rows 4.5, 5.6 and 6.6 restate three of them.
- **The last result.** Every check was green on Windows, macOS and Linux in the CI run of pull request 22, which merged on 2026-11-24. A test that existed then has that result. Two kinds of test run on one system by design, and the entry says which: the Phase 4 test (its own Linux job, in a release build) and the tests that need a screen (the Linux jobs that set `A2M_REQUIRE_DESKTOP`). I ran nothing for this entry; the result is CI's, not mine.
- **Pass and gap.** A phase passes when each clause of its exit test has a test that meets it. A clause with no test, or only a near one, is a gap and is named as one.

## Phases whose packages are all done

### Phase 1, the first read: gap in two clauses

Exit test: a read operation on the test application gives five runs with the same trace signature; two clients share one engine and one session; a locally built wheel installs and runs with `pip`; every budget this phase touches is measured and confirmed or revised by a decision record.

| Clause | The test | Result |
| --- | --- | --- |
| Five runs, one trace signature | `a_target_is_added_linted_published_and_read_five_times_with_one_trace_signature` in `crates/anything2mcp/tests/first_read.rs`, through the command face and the `http` mode; the trace and signature themselves by `five_runs_have_one_signature` in `crates/a2m-telemetry/tests/trace.rs` | Green on three systems |
| Two clients share one engine | `two_commands_share_one_engine_a_second_engine_is_refused_and_a_stop_leaves_nothing` and `two_commands_at_once_read_through_one_browser` in `crates/anything2mcp/tests/local_engine.rs`; the MCP side by `a_bridge_answers_with_the_failure_until_an_engine_runs_and_then_reads_in_both_revisions` in the same file | Green on three systems |
| A locally built wheel installs and runs | No Rust test. The CI job "Wheel and npm packages" builds the wheel, installs it into a clean environment and runs both names of the program and the import, on all three systems (`.github/workflows/ci.yml`, step "Install the wheel into a clean environment and run both names and the import"); `bindings/python/tools/try_wheel.py` does the same by hand and measures | Green on three systems |
| Budgets measured, confirmed or revised | `bench/tests/measure.rs` (`each_measured_scenario_runs_and_holds_what_it_reads`, `the_engine_as_a_process_starts_and_its_memory_is_read`), the benchmark step of the Rust job, and [decision 0363](../decisions/0363-the-budgets-as-first-measured-two-are-revised-and-four-cannot-yet-be-told-from-the-systems-own-time.md) | Green; see the gap below |

Gaps:

- **"One session."** No Phase 1 test has two clients share one session. The two clients share the engine and its browser. A shared session was first testable with Phase 2, and what exists is `ten_runs_meet_an_expired_session_through_the_engine_and_the_application_counts_one_sign_in` (`crates/anything2mcp/tests/renewal.rs`: ten concurrent reads through one engine share one sign-in) and `two_engines_over_one_store_make_one_renewal` (`crates/a2m-auth/src/tests/keeper.rs`). Neither is two clients of one engine using one session and both reading through it. This clause is not met as written.
- **Four budgets are not measured.** Decision 0363 revises two budgets and says four cannot yet be told from the system's own time: the split of a call, the warm tab alone, `page_http`, and `api`. The exit test asks that every budget the phase touches be measured. The numbers are Linux only; the Windows and macOS numbers are only what the CI benchmark step stores, and no one has read them into a decision.
- **One signature on three systems** is not asserted by a test that compares systems; each system proves only that its own five runs are equal. The roadmap says the same for package 1.2 and gives it to the scorecard of [the bar](../spec/1-product/107-the-bar.md).

### Phase 2, the signed-in read: pass, with the real-application clause moved out

Exit test: an expired session stops every call with `needs_login`; ten runs that meet an expired session cause exactly one renewal; no secret appears in any trace, capture, log or result; a signed-in read gives five identical runs. The first design also asked for a read on a real application chosen by the owner; [decision 0058](../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md) moved real systems to the end of the build, so that clause is package 9.5's, not this phase's.

| Clause | The test | Result |
| --- | --- | --- |
| An expired session stops every call with `needs_login` | `an_expired_session_with_no_renewal_path_gives_needs_login_to_every_call` in `crates/a2m-auth/src/tests/keeper.rs`; through the product, `an_expired_session_returns_needs_login` in `crates/anything2mcp/tests/expired_session.rs` and `a_probe_that_finds_the_session_ended_moves_it_to_needs_person_and_the_next_call_returns_needs_login` in `crates/a2m-actions/src/tests/probe.rs` | Green. The product test needs a screen and prints NOT RUN where there is none; the Linux jobs that set `A2M_REQUIRE_DESKTOP` make not having one a failure. Whether the macOS and Windows runners ran it is not shown by the repository |
| Ten runs, exactly one renewal | `ten_runs_meet_an_expired_session_and_exactly_one_renewal_happens` (`crates/a2m-auth/src/tests/keeper.rs`), and through the product `ten_runs_meet_an_expired_session_through_the_engine_and_the_application_counts_one_sign_in` (`crates/anything2mcp/tests/renewal.rs`), where the application itself counts one sign-in | Green on three systems |
| No secret anywhere | `a_secret_planted_in_the_applications_answer_appears_in_no_result_log_or_trace` (`crates/a2m-engine/tests/planted.rs`); the doors by `a_planted_secret_appears_in_nothing_the_door_sends` (`crates/a2m-api/tests/rest.rs` and `crates/a2m-mcp/src/tests/door.rs`); `a_planted_secret_appears_in_no_result` (`crates/a2m-actions/src/tests/kernel.rs`) | Green on three systems |
| Five identical signed-in runs | `five_signed_in_reads_give_one_trace_signature` in `crates/anything2mcp/tests/sign_in_signature.rs` | Green on three systems |

No gap, except the one unproven fact above (the screen-bound test on macOS and Windows).

### Phase 4, the request modes: pass, measured

Exit test: an operation gains a request binding through shadow verification, returns identical results, and runs at least ten times faster than through its interface binding.

The test is `a_request_binding_gained_through_shadow_is_identical_and_ten_times_faster` in `bench/tests/promotion.rs`, over the scenario `PromotionSpeedup` in `bench/src/promotion.rs`. Its setup is the whole path (publish, shadow, report, promote) and stops the run on any answer that differs from the interface binding's. The factor is asserted in a release build only, which CI's job "The exit test of Phase 4, in a release build" runs, alone, on one Linux runner.

What was measured, by the method of [decision 0383](../decisions/0383-the-promotion-exit-test-measures-the-two-paths-in-pairs.md): 40 iterations after two warm-up ones; in each iteration one interface call and one request call, one straight after the other, the order alternating; two factors, each asserted at ten or more: the ratio of the two medians, and the median of the per-pair ratios. The floors are printed, not asserted.

| Run | Factor |
| --- | --- |
| A CI runner, before the paired method (decision 0383) | 9.0 by the medians (interface 161.6 ms, request 17.9 ms); the bar failed |
| One machine, before decision 0386 | 10.9 to 13.7 by the medians and 11.5 to 12.9 by pairs, four runs ([section 225](../spec/2-kernel/225-performance.md)) |
| The same machine after decision 0386 (no disk sync of a read's own rows) and the command tree made once per engine | 16.0, 15.3, 14.8, 14.3, 17.6 by the medians and 16.4, 15.7, 14.6, 13.9, 16.7 by pairs, five runs |

The test passed in the CI run of pull request 22, on its Linux runner, which is the only system it runs on. The factor that run printed is not recorded in the repository; I did not read the run's log, so I give no number for it. Nothing shows the factor on Windows or macOS, and the section 225 numbers are Linux's. Beside other heavy work the factor fell to 6.8, which is why the test runs alone.

No gap in the clauses. Two limits, written down: the factor is a Linux release build's, and the promotion covers reads (a shadow of a write is not built, roadmap 4.3).

### Complete, with no exit test stated

Milestone 4 (the command pack over the kernel) and milestone 8 (more kinds: data stores, files, streams, devices) have all packages done. The Build Order states no exit test for either beyond their roadmap rows, so there is none to report. Each pack has driver tests against its stand; that is not an exit test and I do not give it as one.

## Phases not complete, and what is left

- **Phase 0, foundations:** 0.2p, publishing the placeholder to PyPI and holding the names on crates.io and npm (the owner's).
- **Phase 3, writes:** 3.6, the approval prompts run once by a person on macOS and Windows (the owner's; the automated half is done), and R1, release 0.1.
- **Stage E, the scout:** E2 (traffic capture; waits on where capture joins the provider contract, decision 0390 point 10), E3 (described against observed), E5 (capture's findings into the scout's report).
- **Phase 5, authoring and the first hour:** 5.1 to 5.4 (the rest of the authoring actions, the project folder and shortcuts, the built-in documents, the clients) and 5.5, 5.6 (the release pipeline and the exit test; the first-hour test `crates/anything2mcp/tests/first_hour.rs` is built and runs in about six seconds, but the package waits on the release the owner makes).
- **Phases 6 and 7, a server:** 6.1, 6.2, 6.4, 6.5, 6.6, 7.1, 7.2, 7.3, held until after the first release (decision 0390 point 1).
- **Phase 8, breadth:** 8.3, the desktop pack beyond reads on Linux.
- **Phase 9, hardening:** 9.1a, 9.1b (ready), 9.1c, 9.2, 9.3, 9.4 (held), 9.5, 9.6 (the owner's), and R2.

## Open

- Phase 1's two gaps are not yet rows in the roadmap. They are written here and owed: a test that two clients of one engine share one session, and a measure for the four budgets decision 0363 left open.
- The factor of the pull request 22 run should be read from its log and put in section 225 beside the others.

## Next
Read the log of the Phase 4 job of the next CI run and record the factor. Then take the ready rows in the order of the Build Order.
