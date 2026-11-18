# 2026-11-18: the wider reach

This entry covers the work after pull request 13 merged into `main` (merge commit 631d3ea). The entry before it is [the overnight build](2026-11-17-the-overnight-build.md). Its authors ran no tests: every count below is taken from a record, a test file, CI's result, a pull request's description, or the lead agent's notes, and the last two are marked. It was first written while pull request 15 was open, and brought up to date after it merged; the section "After pull request 15 merged" is the later part.

## State at the end

- **On `main`:** pull request 14, merged at its head 07eda99 (merge commit b487c6e) after five CI rounds, with the work of decisions 0200 and 0205 to 0255; and pull request 15, merged at its head 851853e after two CI rounds, with decisions 0258 to 0277 and their code. For each, every check passed on Windows, macOS, and Linux: 40 checks, with the two publish jobs skipped.
- **Open:** pull request 16, a draft, at 0a09a5b, with CI running when this was brought up to date. It holds decisions 0264 and 0278 to 0284 and their code. The whole tree passed on Linux at that commit: 2,153 tests, none failing (its description's count). Nothing new in it has run on Windows or macOS.
- **Written after that commit:** record 0285, the documents it and the state above change, and this entry's later part. They change no code.
- **What works that did not before, proven in CI on all three systems:** the desktop pack reads Windows and macOS applications against its stand; the MCP pack starts a local server program on Windows; the shared check of who may change a program runs on Windows; the network readers run on Windows and macOS (pull request 14); the second audit's fixes, the network tools, files handed to the scout, and the command line's log, help, and completion (pull request 15).

## Done

### Merged in pull request 14

- **Sign-in.** An agent asks and its person signs in on the engine's page (0205); a web application through its own sign-in form, with a credential stored for the person (0245); a person signing in in a window of the engine's own browser, single sign-on included, with every declared sign of expiry a stop (0254); a sign-in goes only where a published version says, and a credential belongs to its target (0248, the answer to the critical finding below).
- **Writes and shadow bindings.** Writes from inside a page by `PUT` with the page's own token (0206); a shadow binding beside the verified one, promoted by a person (0207, 0219), its evidence counted by distinct inputs in each environment (0251).
- **Three systems.** The desktop pack on Windows (0209) and macOS (0244), with every act proving its record again; desktop writes on Linux (0218); the command pack on three systems (0216); one check of who may change a program, in the provider contract, and the MCP pack's local server on Windows (0250).
- **More kinds of answer and file.** Answers that are not JSON (0217); the HTTP pack reading in bounded work and sending nothing that may write outside a live run (0249); INI files, property lists, folders, and growing files (0246), discovered and drafted as reads (0255).
- **The machine and its network.** One typed reading of where an address is, over a corpus of 103 spellings (0240); the machine's own network state, read and sending nothing (0243).
- **Settings and the first run.** A first run on Linux needs no administrator's file (0220); protected settings loosen only from an administrator's file, and every key says whether it is built (0241); a fixed place, a review summary that shows what a tool runs, and a sign-in declared before its probe (0252); the key derivation compiled for speed in every build (0253).
- **Watches, the ledger's and the scheduler's halves** (0247). The fourth audit's fixes (0200). The repository cleanup (0221).

### In pull request 15

- **The registry's wiring.** The seven network facts served as reads of the local user, `net.standing` listed (0260); a person hands the scout files of the project's `sources` folder, by names the discoverers declare, and `spec.import` is that scout (0263).
- **The command line.** The engine's log says each start and stop and what asked for it, and `engine logs` reads it where the engine runs; help says each action's effect, who may run it, and whether it is built; a completion script for four shells (0266). One table form for failure codes, the `failure_codes!` macro in `a2m-core` (0258), with four crates moved to it (`a2m-core`, `a2m-expr`, `a2m-sources`, `a2m-telemetry`).
- **The sixth audit's answers:** 0261, 0262, 0269, 0270, 0271, 0272, 0273 (below).
- **What the scout may learn from a program itself** (0276) and research note 30 behind it, with the roadmap packages X1 to X7.

### After pull request 15 merged

- **Its two CI rounds.** The validated commit 42eea49, then 851853e: the test that a network answer from another process is dropped unread sent its forged datagram to the reader's own port, which a system allows only to a sender with the network-admin right, which a CI runner lacks. The reader's rule was unchanged; the reader now takes its datagrams through one function, so the test proves the drop on every machine.
- **The unexplained lost answer.** During pull request 15's CI, one request on macOS lost its answer, and its cause was not found; the head that merged passed every check. Established since, in pull request 16: a command's failure for a lost answer now says what the client saw, in one of six fixed phrases, and `a_request_being_answered_when_the_channel_stops_is_still_answered` in `crates/a2m-local/src/tests/channel.rs` holds that a request the engine is answering when its channel stops is still answered. Not established: which side closed, whether the engine or the client was at fault, and whether it happens again. If it does, the phrase names the side.
- **Watches reach an agent** ([0283](../decisions/0283-watches-reach-an-agent-through-the-registry-and-their-person-confirms-each-on-the-page.md)): eight `watch` actions through `a2m_action_run`, the person's confirmation on the engine's page, the tending of due checks, five protected settings, and an engine held from idle while a watch is due. Against the test application, ten quiet checks recorded nothing and one change made through the stand's interface was reported once, on that watch alone. Every read is compared whole. The line 0283 left owed to the scheduler, that a check refused for a spent budget waits for the refill, is done in `Scheduler::submit_check` (`a_check_refused_for_its_budget_waits_until_the_share_refills_and_misses_nothing`).
- **Two watches of one read share a run** ([0285](../decisions/0285-two-watches-of-the-same-read-for-one-person-share-a-run-and-ending-one-never-cancels-a-run-another-rests-on.md), written in this later part): rule 6 gives the second watch's check the first one's unfinished run. That is kept. But a check's run is cancelled by its identifier alone in four places, so ending one watch, or a run resting for a sign-in, can cancel a run another watch's check or the person's own call rests on. The fix is package W2b; the person's own call is Not yet reached by it.
- **The publish page** ([0264](../decisions/0264-the-publish-page-renders-the-typed-review-sign-ins-first-from-one-keeper-call.md)) renders the typed review, sign-ins first, from one call to the session keeper; the command, HTTP, and MCP packs state what their tools start, set, and send, and the file pack its part.
- **The browser's request gap,** found while writing the web pack's statement for that page: `Page::judge_request` refuses a request only where a tab observes and the method may write, so a tab's images, scripts, styles, and its `GET`, `HEAD`, and `OPTIONS` requests go to any origin, and every request of a tab that may act passes. The web pack therefore states nothing about where a web tool sends. It is package P3, and `SCOPE.md` says so.
- **The readers** ([0284](../decisions/0284-the-command-packs-scout-reads-a-fish-completion-file-and-a-manual-page-it-is-handed.md)): the command pack's scout reads a fish completion file and a manual page a person hands it from the project's `sources` folder, as text and never run, and keeps what each source says of each option; where they disagree, the draft asks. PowerShell's manifests and help files are package X4b.
- **The failure-code tables** (0258): twenty crates are in the one form, `a2m-core` among them; seven are not (`a2m-actions`, `a2m-local`, `a2m-pack-desktop`, `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-web-browser`, `a2m-pack-web-ui`), package FC2.
- **The roadmap** was brought to the code: W2 split into W2a (done), W2b, and W2c; X4 into X4a (done) and X4b; new rows P1 to P3 for the publish page and FC1 and FC2 for the tables; eighteen packages are ready.

### The five CI rounds of pull request 14

1. The validated commit 0dce455 (2,022 tests on Linux, none failing; the lead's count). What failed on it was mended by the four commits below.
2. ed2c47a: five desktop files.
3. b55ff9f: nine files. Among them the two test stands now place their programs under the system's temporary folder, and a real Windows fault of the file pack, a file's identity taken before its write (finding C2 of the sixth audit had predicted it).
4. 8d61828: four desktop files: a report-view list read by its shape on Windows, the password edit's refusal asserted, and the macOS title element. The macOS desktop job passed.
5. 07eda99: a test that waited on a race on macOS now waits on the engine's lock coming free, and three files of the desktop stand. Every check passed.

What the rounds showed is recorded in [decision 0278](../decisions/0278-what-the-first-runs-on-windows-and-macos-showed-about-the-desktop-pack-and-the-check-of-programs.md).

## Measured

- The cold start of a command was one key derivation (Argon2id, 64 MiB, three passes): 1.9 s in an unoptimized build, 0.19 s optimized for size, 0.13 s fully optimized. Decision 0253 compiles it optimized in every build.
- The `use` toolset's list was 8,431 bytes for eleven tools, about 2,100 tokens; the `build` toolset's 10,424 bytes for fifteen (round 2 of the [idea rounds](../plan/idea-rounds.md)).
- The generator of the failure-code table found 356 codes where round 1 had counted 136; it finds 368 on the tree as it stands, 23 crates of them still outside the one table form (`tools/failure_codes.py --list`). At 0a09a5b, seven crates are outside it.
- 107 of 111 entries of the engine's home were readable by group and others, none of them in the clear (round 14); decision 0241 made the home the owner's alone.
- The engine's log was empty after about 300 commands at the default level (round 16); decision 0266.
- 38 of the catalog's 99 actions are not built (counted from the catalog and the handlers for [section 704](../spec/7-project/704-readiness.md)). After the eight `watch` actions, 38 of 107 (counted the same way, in the later part).
- Nothing was measured on Windows or macOS beyond CI's pass or fail, and no budget of section 225 was measured.

## The two audit rounds, and what answered each

**The fifth round** (three readers: programs and desktops; the person and the first run; answers, page writes, and shadow): one critical finding, 11 serious, 25 minor.

- Critical, F1: `session.login` took its sign-in from drafts, so a steered building agent could send a stored secret to a drafted address with no person asked. Answered with F2 and F3 by 0248.
- S1 and S2 (an XML answer that stalls a thread; no guard against a write outside a live run) and M1 to M5: 0249. S3, S4, M6, M7: 0245. S5 and M8 to M12 (shadow evidence made cheaply): 0251. PR-1, PR-2, PR-4 (an interpreter that could be a shell; variables that redirect loading; run folders made open): 0250. PR-3 (an act landing on another record): 0244.

**The sixth round** (two readers: sign-in, browser state, and settings; network, watches, files, and programs): one critical finding, latent, 18 serious, 32 minor, all in code written that day.

- Critical, A1 of the network reader: the proxy cleaner cut a value before it looked for its credentials, so part of a password could survive. Fixed before the network facts were served: 0261.
- Watches B1 to B9: 0262. Files C1 to C8: 0272. Programs D1 to D6: 0271; D7 and D8 are not done. Sign-in A2, A3, A6: 0269; A1, A4, A5 in part, A7, A8: 0254. Settings B1 to B4 and the registry's half of C1: 0273. The HTTP pack's C1 to C4, and the refusal of the product's own headers in the HTTP and MCP packs: 0270.

## Learned

- **A push cancels the CI runs in progress on a branch.** The first Windows Rust run was lost that way. A fix is held until the running jobs end, and a CI fix commit is built on the validated head from the named files alone, so that work in flight never reaches the pull request.
- **Validate the whole tree before more stacks up.** A regression from decision 0200 in three packs' drafting tests was found only by another package's run.
- **The runners differ from the assumptions in four places** (decision 0278): a report-view list view's shape on Windows; Windows refusing another process's request for a password edit's text; macOS's title element; and the checkout drive of a hosted Windows runner, which every signed-in user may change. The hosted macOS runner did allow the Accessibility grant by an administrator's write.
- **Disk was the binding limit.** The build folder reached about 19 GB and the disk filled once; at most three building agents ran at a time after that.
- **The container restarted three times.** Nothing on disk was lost; the agents in flight were started again from what was on disk.
- **An engine was stopped from outside in the middle of another agent's test,** most likely by a stop by name. The working method now says that no agent stops a process by name on a shared machine.
- **A safety check stopped a network design round, and blocked one auditor's first report** for its phrasing; the report was written again in terms of mechanism and fix. Neither was worked around.

## Changed in the design

- **The owner's decisions:** 0208 (the machine is the measure of reach and the boundary of service), 0222 (the product knows its network), 0230 (the scope on both sides, describing without ruling), 0231 (the owner's direction in exact words of the agent's own), 0242 (the recommended answer to each of eighteen questions), and 0277 (the North Star). The owner handed the choice of 0276 to the agents, and stopped the idea rounds after round 25.
- **Records written by agents,** each with its own status line: 0200, 0205 to 0207, 0209, 0211 to 0221, 0223 to 0226, 0232 to 0234, 0236 to 0241, 0243 to 0255, 0258, 0260 to 0264, 0266, 0269 to 0273, 0276, 0283 to 0285, and in this pass:
  - [0278](../decisions/0278-what-the-first-runs-on-windows-and-macos-showed-about-the-desktop-pack-and-the-check-of-programs.md), what the first runs on Windows and macOS showed (it supersedes points 5 and 8 of 0209 in part);
  - [0279](../decisions/0279-what-the-three-surveys-left-open-against-decision-0276-is-settled.md), what the surveys left open against 0276 (type libraries read from their bytes, the D-Bus reader on `quick-xml`, fish completions only, E6 withdrawn, one interception question left open, four interfaces and not five);
  - [0280](../decisions/0280-the-use-toolset-lists-at-most-twelve-tools.md), the `use` toolset lists at most twelve tools (it supersedes the last sentence of 0094 point 3);
  - [0281](../decisions/0281-an-mcp-result-is-to-carry-its-decision-so-that-a-call-at-the-mcp-door-can-be-explained.md), an MCP result is to carry its decision (not built);
  - [0282](../decisions/0282-whether-the-command-pack-may-start-programs-that-run-what-their-arguments-name.md), Proposed: whether the command pack may start programs that run what their arguments name.
- **Numbers held for planned work and not yet written:** 0256, 0257, 0259, 0267, and 0268 (the command-line package), 0265 (a keyed digest of a body), 0274 (bounding a system's words in a tool's result), and 0275 (what the product holds for a person).
- **Research:** notes 27 to 30. Note 30 compares three ways for each thing the scout might learn from a compiled program: use existing software, take a part and adapt it, or build it.
- **Reviews:** coverage, breadth, and configurability, in `docs/reviews/`, and the two audit rounds above.

## Noticed, not acted on

- The plans of the wiring packages and of the command-line package exist only in the lead's scratch space, not in the repository. What remains of them is listed under Next.
- The transitive dependency `dispatch2` entered the lock with the macOS desktop work, and no decision names it.
- `is_share_path` of `a2m-core` counts `\\?\C:\` as another machine's share, the safe side; a more exact rule and a test are owed.
- The CI run of the network readers' namespace test with `A2M_REQUIRE_NAMESPACE=1` may need the runner's limit on unprivileged namespaces lifted first; untried.
- The wheel and source-package steps of the Python package were not run in this container.
- Section 221 says the listed tools are configurable per caller; `tools.listed_limit` is a setting of the user's scope, not of a caller.

## Decided without the owner having read it

Every record above that is not the owner's. The widest-reaching: a sign-in goes only where a published version says (0248); the check of who may change a program and the programs it refuses by kind (0250, 0271), which narrow what the command pack reaches and are put to the owner in 0282; what the scout may learn from a program, with the studied project out of the design (0276, 0279); and the cap of twelve tools (0280).

## For the owner

- Decision 0282 asks whether to widen what the command pack may start; it is on [the page of questions](../plan/open-questions-for-the-owner.md).
- Decision 0277 says what the lead agent inferred from the North Star; the owner corrects it by saying so.
- The publishing setup is still the owner's own act.

## Next

1. **Read this entry, then pull request 16's CI result.** Mend what fails on Windows and macOS with commits built on 0a09a5b from the named files alone, and merge when every check passes. Expect the first runs there of the watch tests (and their timing on slower runners), the publish page, and the command pack's readers; watch whether the lost answer on macOS comes again, and read its phrase if it does.
2. **Then the registry lane, one package at a time in `a2m-actions`:** W2b, the cancels of decision 0285; then W2c; the command-line package, its answers carrying their next step and a retry hint, and an MCP failure carrying the same data as every face, with the decision of 0281.
3. **Alongside, in crates nobody holds:** FC2, the seven crates outside the failure-code form, one at a time; P2 and P3, the remaining packs' statements and the browser's request gap; the scout's readers X1 and X4b, and the spike X7; package N2b; D7 and D8 of decision 0271.

## Pull request 16 in CI

Four runs. What each showed:

- A product fault on macOS: the walk went inside the title bar's own buttons, whose contents change between walks ([decision 0286](../decisions/0286-the-macos-walk-lists-a-title-bars-buttons-and-walks-nothing-under-them.md)).
- A product fault on Linux: a connection the engine closed untaken was reported as `channel_unavailable`; it is `connection_lost`. Reproduced in 61 of 300 runs before, 0 of 300 after.
- A wrong test: it asked for nothing in hand between the engine's write that ends a run and the runner's record of the job's end (decision 0132). It now waits for the runner's notice.
- The hosted macOS runners: one full Rust job ran its tests twice as slowly as usual and met its limit of 30 minutes, with three browser tests past their deadlines on it; and in three runs a macOS job was cancelled 15 minutes after it was queued, with no log. Not established: why. Six macOS jobs start at once, so the macOS desktop job now waits for the wheel-and-npm jobs. No limit was raised. The browser tests' deadlines on a slow machine are work still owed.
- This session may not rerun a job; a cancelled job runs again only with a new commit.
- A fifth run, with the macOS desktop job waiting: every test passed again, and one macOS wheel was cancelled after 16 minutes in the queue while another waited 10 minutes for a runner. The job without TLS now waits for the wheel-and-npm jobs too, so four macOS jobs ask at once.
- A seventh run, on the watch fix and two more packs' code tables: the job without TLS on macOS failed in `local_engine`, test `an_engine_whose_data_directory_is_removed_refuses_the_next_request_by_name_and_stops`. The command got `possible_commit` / `answer_lost`, "the connection closed before any answer", where the engine should have refused with `data_directory_gone`. This is the second lost answer on macOS without TLS; the first was in pull request 15. Not reproduced on Linux in about 240 runs, and the cause is not established: either the engine's process ended, or it closed a connection that was idle between the hello and the request. The test now reports, when it fails, whether the engine's process has ended and what it wrote on standard error, so the next failure says which.
- A ninth run, on every crate's code table: the full Rust job on macOS failed in the web interface pack's `sign_in`, with `unexpected_page`. A product fault: the pack asked whether a page shows a sign by waiting 250 milliseconds and read no answer as "no". It now asks the page once and waits for its answer within the step's time ([decision 0288](../decisions/0288-whether-a-page-shows-a-sign-now-is-the-pages-own-answer-and-a-slow-answer-is-never-a-no.md)). This is likely what the three browser tests that missed their deadlines on the slow runner were showing too; not established.
- A tenth run, on the fix of 0288: every job of the CI workflow passed on all three systems, the full Rust job on macOS among them. The release check's two macOS wheels were cancelled after 15 minutes in the queue. The wheel-and-npm jobs now wait for the sign-in window job, so the wheels are given runners first.
