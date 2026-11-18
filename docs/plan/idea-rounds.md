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

## Round 20 (2026-11-18, 04:27 UTC)

**Idea.** A person who holds a file that describes a system, such as an OpenAPI description or a device's published register map, can hand it to the scout in the real product, and the scout reads it.

**Test.** The file pack's agent had just found that the engine passes a discovery job none of a surface's files. So the same question was put to every pack: read each discoverer that takes its material from the files of the job, read the one place where the engine builds a discovery job, and searched the product's tests for any that hands discovery a file. Reading only: no scout was run.

**Result.** The person cannot. The engine gives a discovery job exactly one checked path, the project folder of the request that started it, and nothing puts any other file there. Three discoverers depend on more. The HTTP pack looks for a file under the name `description` and never finds one, so in the product an OpenAPI description is imported only when the system itself serves it at a path; when that fails, the failure's own fix says to give the description as a file, which the product offers no way to do. The device pack's reader of a published register map walks the job's files for names that end in `.csv`, meets only the project folder, and finds nothing. The file pack had the same fault until today's package gave its discoverer the surface's own folder. Each pack's own tests pass because they build the job by hand with the files in place. No product test hands discovery a file.

**Verdict: taken up.** The scout is to take, by name, files of the project that the person points it at, each checked as every project path is and offered to the discoverers under the names they declare, and a product test is to import a description from a file and a register map from a file through the real command. Until that is built, the HTTP pack's fix text is to stop naming a step that cannot be taken.

**Next.** A package of its own in the action registry and the engine's discovery job, after the validation in hand. The scope page gains a row for it, marked not yet built.

## Round 21 (2026-11-18, 04:37 UTC)

**Idea.** The help of every command that changes something says what kind of change it makes, who may run it, and whether a person is needed.

**Test.** Asked the built program for every action in its registry and what the registry holds about each (its effect, who may run it, whether it is built), then read the help of each action's command and looked for the same facts there. One debug build from earlier in the day; all 92 actions; a search for words, so a help text that says the fact in other words than the ones searched for would be missed.

**Result.** The registry holds the facts and the help does not print them. Of the 41 actions that change something, the help of none says who may run it, seven say anything about the effect, and twelve mention a person. The larger finding was not the one looked for: 38 of the 92 actions are not built, among them importing a description from a file, starting a capture, moving a toolset to a version, and setting a configuration value. The top-level help marks them. The help of each one does not: it describes the command in full, as if it worked, and a person learns otherwise only from the refusal after running it.

**Verdict: taken up.** A command's help is generated from the registry, so the fix is one change to the generator and no hand-written text: every help gains the action's effect, who may run it, and whether a person at the machine is asked; and the help of an action that is not built says so in its first line, with what the product does today instead where the registry knows it.

**Next.** Joins the package for what a person and an agent meet at the command line. The count of actions not built, 38 of 92, belongs in the readiness section beside the roadmap's counts.

## Round 22 (2026-11-18, 04:47 UTC)

**Idea.** An agent that calls a tool can tell which parts of the answer are the system's own words, and those words reach it bounded and made neutral, as section 502 says: free text from the system sits only in fields marked untrusted, limited in length and stripped of markup.

**Test.** Followed the mark from where it is set to where it would have to be used: the field types and the compiler's lint, the plan, the engine that builds a result, the function that builds a tool's output schema, and the doors. Searched every crate for a reader of the mark. Reading only: no hostile text was sent through a running tool.

**Result.** The first third of the sentence holds and the rest does not. The compiler refuses a free-text output whose field type is not marked untrusted, so the mark is always set. After that nothing reads it. The engine has no reader of the mark, so a marked field is returned at whatever length the pack allowed and with whatever characters it held; only the HTTP pack bounds and refuses control characters, since this morning, by its own rule. The output schema of a tool is built from the value's type alone, so the schema an agent is shown says `string` for a customer's note exactly as for a field the product made itself. The one place where a system's words are fenced and made neutral is a failure's `said`, which has its own ceiling and its own function for it.

**Verdict: taken up.** A stated guarantee of section 502 is carried out at compile time only. Three changes close it. The engine applies one rule to every output field whose type is marked untrusted: a ceiling on its length and the same making-neutral that a failure's `said` already gets. The output schema of such a field says that it is text from the system and data, never an instruction, and states the ceiling, so that every client's model is told. A test sends text shaped like an instruction, with control and direction characters, through a real tool against the stand and reads what the agent receives.

**Next.** A package in the engine and the compiler, after the validation in hand. Until it is built the scope page says so.

## Corrections to rounds 19 and 21 (2026-11-18)

A designer who read the code for the command-line package found two statements in the rounds above that the source does not bear out. Both rounds' verdicts stand; the statements are corrected here and the plans follow the corrected form.

- **Round 19** said the registry already marks which actions need no health at all, and that a command for one of those can be answered from the program itself. The mark it meant covers eleven actions, and most of them need the kernel, the settings, or the provider registry. Only three read nothing but the registry: `docs.get`, `action.search`, and `action.describe`. So the fix needs a mark of its own for an action the program can answer alone.
- **Round 21** said the top-level help marks the actions that are not built. It does not: it marks one process command and the steps of shortcuts, which is what the word search matched. No help, at any level, says that an action is not built.

## Round 23 (2026-11-18, 04:57 UTC)

**Idea.** A person can ask the product what it holds for them on this machine, and can take any of it back.

**Test.** Looked at a scratch engine's home after an afternoon of use, asked the built program the same question through its own commands (`secret`, `session`, `run`, `trace`, `audit`, `doctor`), and read the credential store's interface and the action catalog for anything that lists or removes. One debug build; the scratch home had no passphrase set, so the session commands answered with that refusal and their content was read from the source instead.

**Result.** For sessions the answer is yes: they can be listed, and one can be ended. For runs and traces a person can list and read, and nothing deletes them, which the scope page already says. For secrets the answer is no on both counts. The command has one verb, `set`. The store beneath it can put a credential, say whether one is held, and open one for a sign-in; it cannot name what it holds, and it cannot remove anything. So a person who stored a password for a target last month cannot see that the product still has it, and cannot make the product forget it short of storing another value over it or deleting the whole home. No command gives the whole picture either: what is kept, by kind, where, and how much.

**Verdict: taken up.** Three additions. The credential store names what it holds for a person (the reference, the target it belongs to, when it was stored, never a value) and removes one. Two actions serve that: a list any caller acting for the person may read, and a removal that is the person's own act and is written to the audit log. And one read answers "what do you hold for me" on one screen: secrets by name, sessions, runs, traces, captures, and findings by count and size, and the home's place on disk.

**Next.** A package in the session keeper's store and the action registry, placed in the registry lane after the wiring that is already planned. The scope page says so until it is built.

## Round 24 (2026-11-18, 05:07 UTC)

**Idea.** When the scout looks at a compiled program, reading the printable text in the program's own bytes would tell it of flags and commands that the program's help leaves out.

**Test.** Took the long flags that a program's help states and the things shaped like a long flag in the printable text of its bytes, and compared the two sets: first for the command-line test stand, with the help of each of its nine commands, then for eight ordinary programs installed on this machine, counted together and not named. A count of strings only: no flag found in bytes was tried, and nothing was run but each program's help.

**Result.** The bytes are not a better source than the help, and not a safe one. For the test stand the two sets are the same thirteen flags. For five of the eight installed programs the bytes add nothing or one string. For one, the bytes miss 114 of the 170 flags the help states, because the program keeps its option names in another form. For one, the bytes hold 244 flag-shaped strings beyond the twelve its help prints, and the reason is plain: that program has a short help and a fuller one, and only the short one was asked for. In every case a string from the bytes comes with no description, no argument, and no sign of which command it belongs to, among some twenty-six thousand other strings in a program of this size.

**Verdict: ruled out.** A flag found in a program's bytes would be a guess about a thing the program itself can be asked. The one case where the bytes held more is answered with certainty by declaring the program's fuller help form, which the target already can. Nothing in the rules forbids reading a program's bytes at build time, so this is a finding about usefulness and not a limit: if a kind of program appears whose help says little and whose bytes say much, the question is open again.

**Next.** One sentence added to the command pack's section, where a target declares its help forms: declare the fullest form the program has.
