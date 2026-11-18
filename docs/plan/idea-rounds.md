# Idea rounds

Every ten minutes by the clock, while a session runs, the lead agent puts forward one new idea that is not part of the work already in hand, tests it at once, and either takes it up or rules it out. The owner asked for this on 2026-11-18. This page is the log, newest round last.

A round is small on purpose: one idea, one test that can be finished in minutes, one verdict. The test is real (a measurement, a search of the code, a run of the program), and the log says how deep it went, so that nobody mistakes a ten-minute check for a proof. An idea that is taken up becomes work in the ordinary way: a package in the [roadmap](roadmap.md), a decision record, or an entry in the [hypothesis register](../hypotheses/README.md) when it needs a longer test. An idea that is ruled out stays here with the reason, so it is not raised again without new evidence.

| Verdict | Meaning |
| --- | --- |
| **Taken up** | The test supports it. The row says what happens next |
| **Ruled out** | The test speaks against it, or shows it is not worth its cost. The row says why |
| **Changed** | The test spoke against the idea as stated and for a different one, which is taken up |

## Round 1 (2026-11-18)

**Idea.** Every failure code the product can return is already documented in one place, the technical reference, so a command that explains a code (`explain <code>`: its class, what it means, what to do) could be generated from that table with nothing written by hand.

**Test.** Listed every failure code declared in the crates' own code tables and provider manifests, and searched the documents for each. A search of sixteen `codes.rs` files and the manifests; codes declared elsewhere in the code are not counted, so the true total is higher.

**Result.** 136 distinct codes found. The technical reference names 40 of them. Another 77 appear only in scattered specification sections. 19 appear in no document at all (most in the web, data, desktop, and file packs). The guides the program itself serves name 105. The code, though, already holds the list in a form a program can read: eleven crates keep a table of every code with its failure class, and each provider's manifest declares its codes.

**Verdict: changed.** The reference is not the source and cannot be: it holds under a third of the codes. The source is the code. Taken up in the reverse direction: the failure-code table in the technical reference is to be generated from the crates' own tables, the documentation audit is to fail when a code in the code is missing from it, and `explain <code>` is to be served from the same tables. This also mends a standing breach of "knowledge lives in one place": AGENTS.md says every error maps to a code "from the technical reference", and two thirds of the codes are not there.

**Next.** A small package: one enumerable table of codes per crate (five crates have none), a generator for the reference's section, an audit check, and the `explain` action.

**What came of it (the same day).** The generator (`tools/failure_codes.py`), the generated table in the technical reference, and the audit check are built. Reading every crate, and not only the sixteen files of the first count, the generator finds 356 codes, not 136: the round's count was low, as it warned. No code is declared with two classes. Fourteen crates still have no table a program can read (their classes are taken from comments and from the calls that build a failure), and no code states its way out as data, so `explain` waits on that: one more package.

## Rounds 2 to 13, run together (2026-11-18)

The owner asked for the ideas the first rounds would most likely reach to be tested at once, so that work on the good ones could start. Each was tested against a scratch engine started from a copy of the built program, with the built-in test application as the system. Nothing here was run on Windows or macOS, and each test is as small as a round allows.

| Round | Idea | Test | Result | Verdict |
| --- | --- | --- | --- | --- |
| 2 | An agent is handed too much: serve only a search tool and a call tool by default | Spoke `initialize` and `tools/list` to the bridge for both toolsets and measured what came back | The `use` toolset lists 11 tools in 8,431 bytes (about 2,100 tokens); `build` lists 15 in 10,424 bytes. The largest single tool is 1,159 bytes | **Ruled out.** The surface is already small. Two tools would save little and cost a search before every call |
| 3 | The program is slow to start | Timed ten runs each of three commands against a running engine, and three commands that had to start the engine first | With an engine running: 18 to 25 ms a command. Starting the engine on demand: 2.1 to 2.3 seconds, every time | **Taken up**, for the cold start only: find where two seconds go (the key derived from the passphrase is the first suspect; the store's opening and the wait for the engine's channel are the others), and give a cold start a budget in section 225. An investigation is started |
| 4 | Two published tools from different targets can collide in name | Read the naming rule and tried to add a second target of the same name | A tool's name begins with its target's name, and a second target of a name is refused | **Ruled out.** One thing found on the way goes to round 5: the refusal's code is `schema_invalid`, which is not what happened |
| 5 | A refusal should tell an agent what to do next in a form it can follow without reading prose | Ran ten commands that are refused and read each refusal | All ten carry a class, a code, a sentence of `fix`, and whether a retry is safe. None carries a `next` action, though the bridge's instructions tell an agent to follow `next`. Two refusals about existing state (a target that already exists; a malformed identifier) are both coded `schema_invalid` | **Taken up.** A refusal carries a `next` action wherever one exists (a tool not found: search; gates not passed: test; a person needed: the page). A refusal about state gets a code that says so. With round 1's generated table of codes and `explain`, this is one package: refusals an agent can act on |
| 6 | The bridge should state the product's limits when a client connects | Read the `instructions` the bridge returns | 637 characters. They already say that every tool is deterministic, the shape of a failure, that a write waits for a person, and how to follow `next` and find an unlisted tool | **Ruled out.** The essentials are there, and every added sentence is paid for in every session |
| 7 | The same read can give different bytes on different runs, which would break an agent's caching | Ran one read five times and compared digests | The result is byte for byte the same in all five. Only the envelope differs: the run's identifier and its trace | **Ruled out** as a fault. It is evidence for the determinism the product claims, by one read against one stand |
| 8 | Help text runs off an ordinary terminal | Ran `--help` for the program and each of its 54 commands and measured every line | 440 lines; 53 of them (12%) are wider than 80 columns; the widest is 306 | **Taken up.** Help is wrapped to the terminal's width. Small; it joins the package of round 5 |
| 9 | The first result could be one command | Timed the steps from an empty folder to a first read | `init` 0.02 s, adding the target 2.3 s (it starts the engine), the scout 1.8 s, the trial read 0.06 s: a little over four seconds of the machine's time, with nothing a person must do before the read | **Taken up.** A `demo` command is feasible, as the [adoption plan](adoption.md) proposed. Open: whether the test application is carried in the released program or fetched, and what that does to its size |
| 10 | The documentation audit could run before every snapshot | Timed it | 3.4 seconds | **Ruled out** for snapshots: with agents in the middle of edits it would fail by design. It stays where it is, at validation and in CI |
| 11 | The generated clients' methods can be guessed from the action names | Compared the action catalog with the generated Python and Node clients | Every action has a method. But the catalog said the form is `client.<noun>.<verb>()`, and the clients generate `client.<noun>_<verb>()` and `client.<noun><Verb>()` | **Changed.** The clients are consistent; the catalog's sentence was wrong and is corrected |
| 12 | Most of the test suite's time is the browser | Read the per-file times in the last full test log | 363 seconds of test time in 132 files. The web pack's own files are 1% of it. One file, the tests of starting the engine on demand, takes 65 seconds for 7 tests (18%), and it is the file with the one failure nobody has explained | **Changed.** The browser is not the cost. Taken up: those tests are to wait on a named condition with a short idle setting instead of on real idle time, and the unexplained failure is looked at again with that in mind. An investigation is started |
| 13 | A run leaves more on disk than it should | Looked at the engine's home after a dozen runs | About 3 KB of trace for a run; a store of 0.7 MB. Nothing is ever deleted | **Ruled out** as new: retention that deletes is already known to be missing and is queued from the configurability review |

**Not tested in this batch, and still open for a later round:** whether target files would be clearer with defaults left out; whether `tool describe` could show a worked call from a tool's recorded fixture (it answers only for a published tool, and the scratch engine had none).

## Round 14 (2026-11-18, 03:27 UTC)

**Idea.** Something under the engine's home can be read by another user of the machine that should not be.

**Test.** Listed the permissions of every file and folder under a scratch engine's home after a dozen commands, with the usual default of new files readable by all, and looked inside what was readable. One home, on Linux; Windows and macOS not looked at.

**Result.** Of 111 entries, 107 can be read or entered by group and others: the store, every trace, the vault's salt, the folders for logs and profiles. Only the folder that holds the socket is closed to others (the socket itself is the owner's alone), and the two log files are the owner's alone. What is readable is not in the clear: a trace is sealed (its bytes are ciphertext under the vault's key), and a search of the store for a value a read had returned found nothing. So another user who can reach the home learns that the product is used, when, and how much, and gets ciphertext to keep; they do not get a value. Whether they can reach the home at all rests on the folders above it, which the product does not check.

**Verdict: changed.** Nothing sensitive is readable in the clear, so the idea as stated is wrong. But the product's protection of its own files should not rest on a folder it did not make: taken up as a small change. The engine makes its home and everything in it the owner's alone, as it already does for the socket's folder; where it finds a home that others can enter, `doctor` says so with the fix.

**Next.** Joins the queue for the crates that hold the home's paths, with a test that every entry under a fresh home is closed to group and others.

## Round 15 (2026-11-18, 03:37 UTC)

**Idea.** Some command, asked for JSON, prints something that is not JSON: a stray line of text, a warning on standard output, a usage message. A program that reads the output would then break exactly when something went wrong.

**Test.** Ran every command and subcommand the program lists (54 commands) with `--json`, three ways each: with no argument, with the name of a target that exists, and with a name that does not. 276 runs against a scratch engine, with standard output parsed strictly each time. Commands that start or stop the engine or serve were left out; inputs that fail deeper inside an action (a refused write, a lost connection) were not reached by this.

**Result.** All 276 outputs parse as one JSON value, the refusals and the usage errors included. One successful command also wrote text to standard error, where it belongs. The exit codes seen: 0 for success, 2 for a command line that is itself wrong, 1 for an action that refused, 3 where a person is needed.

**Verdict: ruled out.** There is no such command among those reached. The result stands as evidence for the claim that the command line is safe to drive from a program.

**Next.** Nothing to build. Still open for a later round: whether exit codes tell "try again later" from the other failures, which this run could not reach.

## Round 16 (2026-11-18, 03:47 UTC)

**Idea.** A person can read the engine's own log at a glance and see what it has been doing.

**Test.** Read the log files under a scratch engine's home after some three hundred commands and several starts and stops of the engine, then asked the product the same question through its own commands (`engine logs`, `audit query`, `run list`). One build, one home, the default settings.

**Result.** There is nothing to read. The engine's log file is empty: zero bytes. The file of the engine's own output holds fifteen lines, all about starting and being ready. The audit holds three entries, each the engine closing. The command that should show the log, `engine logs`, refuses when run as a command on the engine's own machine, with a fix that says to run it as a command on the machine the engine runs on. What the engine did can be found only run by run, through `run list` and each run's trace.

**Verdict: changed.** The question was whether the log reads well; the finding is that at the default level the engine writes nothing a person could use to answer "what happened, and why did it stop". Taken up in two parts. First, `engine logs` is to work as a command where the engine runs: the refusal is either a fault in this build or a fault in the design, and either way a person cannot read the log with the command made for it. Second, at the default level the log carries one line for each start and each stop of the engine with what caused it, and one for each refused call with its class and code and never its input. The second part would have answered, on the day it happened, the one test failure nobody has explained: an engine that stopped for a reason recorded only as "asked".

**Next.** Both parts join the package for what a person and an agent meet at the command line. The first is checked against a fresh build before anything is changed.

## What came of rounds 3 and 12 (2026-11-18)

An investigation traced both, and wrote down the evidence for each phase.

**The slow cold start (round 3).** About nine tenths of the two seconds is one call: the derivation of the vault's root key from the passphrase, which is slow on purpose, compiled without optimization. With the same parameters it takes about 1.9 seconds unoptimized, about 0.19 under the release profile, and about 0.13 optimized for speed. The two crates that carry it are now optimized in every build ([decision 0253](../decisions/0253-the-key-derivation-is-compiled-for-speed-in-every-build.md)); the cost that protects the passphrase is unchanged. A release build of the product itself has not been timed. Found on the way: `engine status` and `engine stop`, run when no engine is running, open the whole kernel inside the command only to say so; and the budget of 100 ms from start to ready cannot hold on the passphrase route in any build.

**The slow test file and its one unexplained failure (round 12).** The file takes 65 seconds because one test waits a real minute, the smallest idle time the setting allows, and each of its seventeen engine starts paid the two seconds above. The failure was reproduced exactly by sending the test's engine one outside signal to terminate; every way the product or the tests could send one was excluded, by the lines of code that exclude it; twenty of twenty runs passed. The likely cause, not established, is a signal from outside the test: this machine is shared by many agents that start and stop engines, and nothing records what asked an engine to stop. Taken up: the idle time settable in seconds; a record of what asked an engine to stop, in its last line and its log; and a rule for agents on a shared machine never to stop a process by name.

## Round 17 (2026-11-18, 03:57 UTC)

**Idea.** After a day in which the product changed in many places, the two pages a person meets first on a package registry, the pip package's README and the npm package's, say something that is no longer true.

**Test.** Read both pages claim by claim against the packages' own sources: the client methods the examples call, the starting of an engine by `connect`, the refusal named when starting is switched off, how each client reaches the engine on each system and what it checks first, the npm package's claim that no install script runs and its message when the platform package is missing, and the tool the examples call. Then compared the saved copy of the engine's OpenAPI document, from which both clients are generated, with the action registry's source. Reading only: neither client was run.

**Result.** Every claim holds. The methods exist under the names the examples use. The Python client checks that the socket and its folder belong to the user, and on Windows that the process serving the pipe runs as the user; the Node client refuses on Windows unless the caller supplies the check, as its page says. The npm package declares no install script and names the package to install when the program is missing. The tool the examples call is an operation of the imaginary `example` target. Every action named in the registry's source is in the saved OpenAPI copy. Both pages still open by saying the product is not ready for use, which is true.

**Verdict: ruled out.** Nothing to change. What the test did not cover: the saved OpenAPI copy was compared by action name only, so an action whose inputs changed today without its name changing would not show; the whole-tree validation regenerates the copy and compares it exactly.

**Next.** Nothing from this round.

## Round 18 (2026-11-18, 04:07 UTC)

**Idea.** A script or an agent that gets a failure can tell, from the exit code or from the error itself, whether calling again could help and how long to wait first.

**Test.** Read the one place where a failure becomes an answer (the dispatcher's exit class and error body in the action registry), the place where a failed run is reported, and section 220's rules for failures, and searched every crate's source for any field that says when to try again. Reading only: no failure was provoked in a running engine.

**Result.** It cannot be told. Every failure of an action ends with exit code 1, whether the system was busy for a moment or the policy will refuse the call for ever. The error carries `retry_safe`, which answers a different question: whether the same call could do harm if sent again. It is true for every class except a write that may have gone through, and a refused or mismatched write. So a policy refusal, a bad input, a drifted page, and a busy surface all say `retry_safe: true`, and an agent that reads that as "try again" loops on a refusal that will never change. Nothing anywhere says how long to wait: no failure carries a time, although the scheduler knows when a busy surface's lease ends and a system that limits its callers often states a wait. Only the class name tells `transient` and `busy` from the rest, and section 220 says the answer must be stated and never implied by the class name.

**Verdict: taken up.** An error is to say two more things beside `retry_safe`: whether the same call with the same input could end differently with nothing else changed (true for `transient`, `busy`, and a session the keeper can renew; false for `policy`, `input`, `drift`, `unknown_variant`, and the rest), and, where the engine knows it, the earliest moment worth trying. Whether the command line also gets an exit code of its own for "try again later" is a change to a public face and is decided in the same package, with a record.

**Next.** Joins the package for what a person and an agent meet at the command line, beside the refusals that carry their next step as data.

## Round 19 (2026-11-18, 04:17 UTC)

**Idea.** A command that only asks a question leaves nothing behind on a machine where the product has never been set up.

**Test.** Ran ten commands with the built program, each against a home folder that did not exist, and counted the files and bytes each left and the time it took. One debug build from earlier in the day, Linux, no passphrase set, so no key was derived in any of them.

**Result.** Help and the version leave nothing and take 25 to 100 milliseconds. Every other command that ran made the home: a store of 450 kilobytes with every migration applied, an empty log, and two lock files. That includes the two that read only what is compiled into the program, the built-in documentation page (`docs get settings`) and the search of the action list (`action search`), and it includes `engine stop` and `run list`, which then failed because there was nothing to stop or list. Each took about 300 milliseconds, twelve times the version command. Nothing unsafe was found: the folders are made private and nothing leaves the machine.

**Verdict: taken up.** An action that needs nothing from the engine is to open nothing: the registry already marks which actions need no health at all, and a command for one of those can be answered from the program itself, without a store, a lock, or a home. The same holds for a command that finds no engine and no home: it says so and makes neither. This is the general form of what the cold-start investigation found for `engine status` and `engine stop`, which open a whole kernel to say that no engine is running.

**Next.** Joins the package for what a person and an agent meet at the command line, with a test that runs each such command against a home that does not exist and asserts that it still does not exist afterwards.
