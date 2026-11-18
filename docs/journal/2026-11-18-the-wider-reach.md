# 2026-11-18: the wider reach

This entry covers the work after pull request 13 merged into `main` (merge commit 631d3ea). It is written while that work is still running, and says at each point what is proven and what is not. The entry before it is [the overnight build](2026-11-17-the-overnight-build.md).

## State at the end

- **On `main`:** the build of the previous entry, as merged: every check green on Windows, macOS, and Linux.
- **On the branch, as snapshots:** everything below. Pull request 14 is open as a draft and holds it. **Pull request 14 has had no CI run on this work.** The only validation is on Linux, in this container: formatting, lints, and tests passed on a validated commit before the audit fixes of decision 0200 (1,742 tests, none failing, and the lints clean for the macOS and Windows targets). The work since has not been validated as a whole tree.
- **Written and never run:** the Windows desktop back end (0209), the Windows check of the command pack (0216), and a PowerShell argument path in the kept-script feature. Linux cannot run them. Their first run is the first CI run of the branch, and failures are expected there.
- **Still being built when this was written:** the settings fix (0241); one typed place for every destination (0240); the network readers (0243); the macOS desktop back end (0244); web sign-in (0245); the file pack's new modes (0246); watches (0247); and a fifth round of safety audits. These numbers are reserved by the lead; none of them has a decision record yet.
- **What the entry cannot vouch for:** its author ran no tests. Every count below is taken from a record, a test file, or the lead's notes, and is marked where it is the last.

## What the session set out to do

The owner widened the product's scope and asked for the work to keep going with every resource. The scope question was answered first, and the rest followed from it: a person's machine is the measure of what the product can be set around, so the product should reach whatever that machine reaches, on all three systems; and it should be built, reviewed, and cleaned to match.

## What the owner decided

Written in the lead's words, as [decision 0231](../decisions/0231-the-owners-direction-is-written-in-exact-words-of-the-agents-own.md) requires; none of the owner's speech is quoted.

- **0208, what "on the person's machine" means.** Outward, the machine is the measure of reach: an installed application, a website, or any endpoint the machine can reach may be set around. Inward, it is the boundary of service: the tools are offered on that machine and nowhere else; there is no network door. This put the desktop pack in the first release on all three systems.
- **0222, the product knows its network.** The scout may look across the networks the machine is on; agents get tools to see where they are; the network is one general idea held in one way, with a website as one instance of it. The lead read it as being about what the product reaches, not who reaches it, so 0208's second half stands. The owner asked for research, then design, in rounds.
- **0230, the scope is written on both sides and describes without ruling.** SCOPE.md says what the product answers for and what it does not. Every "does not" is marked rule, choice, or not yet, and no agent turns an idea down because such a line exists.
- **0231, the owner's direction is recorded in exact words of the agent's own.** The records that had quoted the owner were rewritten. The same reading changed rule 3, which now says every write is recorded first and proven after, and that a write with nothing to read back is declared `irreversible`. That rewording is more than polish, because it matches what decision 0161 already built.
- **0242, the owner took the recommended answer to each of eighteen open questions** that had been gathered on one page, as the page stood. The lead read one direction as covering them all, and said so in the record. The answers are mostly "no", and each "yes" is narrow: the macOS desktop back end may be built on a permission the person grants; a target someone else wrote may be loaded under section 502's rules; integrated sign-in and passkeys are allowed with the person present; standing approval is allowed only narrowly and is built only if few actions need it; the first version is 0.1.0; the product is not published to crates.io.

## What was built

Each item has its record; the proof is the test named there, run on Linux unless stated.

- **Audit fixes, 0200 and 0201.** The fourth audit's three serious faults: a test or trial could run the first binding in file order and let its evidence promote another (now each runs the one binding it is evidence for, and the evidence names it); a recorded fixture kept a system's own map keys, and a choice was replaced by "the first code that is not the original", which reversed it (now keys are made-up and a choice is the first declared code). Also a resume's wake lost in a race (a test with no sleep), and the browser taking the system's proxy (it is launched with no proxy and no extensions; a machine policy still outranks that, and a check for it is owed). 0201 was in pull request 13; 0200 was not.
- **0205, sign-in an agent asks for is carried out by its person.** `session.login`, `.revoke`, `.list`, `.end`; the credential is typed on the engine's own page, never in a tool input; the scout stops at `needs_sign_in`. The signed-in first hour is a test (4.8 s, from the lead's notes).
- **0206, writes from inside a page.** The page's own token, an acting tab, and `PUT` only. The write is never drafted. Finding: the browser may resend a `PUT` by itself after a lost connection, so a token refusal is a write in doubt and never "not written". Its product test was owed to the registry and is part of 0219.
- **0207 and 0219, shadow bindings.** A second binding reads beside the verified one; the caller always gets the verified answer; five matches in a row for a read (twenty for a write, which is not built) let a person promote it. 0207 is the engine and ledger half, 0219 the product half (`shadow.report`, `binding.promote`), with a product test.
- **0209, the Windows desktop back end.** UI Automation on a thread of its own, a fixed list of read calls, a password edit never read, a stand of plain Win32 controls. Written and linted for Windows; never run. CI has a new job for it.
- **0216, the command pack on three systems.** The Windows path check that had passed everything (found by the coverage review, and confirmed in the code) is replaced by a check of owners and access lists; macOS counts its administrators' group; Linux counts none unless a protected setting names one; a script the target keeps runs as a fixed interpreter-and-script pair by digest. Windows and macOS parts are CI-only.
- **0217, answers that are not JSON.** An endpoint declares `xml`, `text`, or `bytes`; a document type in XML is refused. The lead's premise that JSON had a content-type check did not hold, and the record says so.
- **0218, desktop writes on Linux.** A second closed list reached through one function, the process proven again before every act, and a read-back of a window reached afresh, because a field read back would show what was typed even if Save stored nothing. The stand has a Save that stores nothing and says nothing, to prove the difference.
- **0219, the first-hour fixes and `connect`.** Hints that read the publish gates, `test.run` exiting 1 when a gate fails, bridge instructions made from the action's own field names, `draft.delete` for several files, and `client.connect`, which prints a server entry and writes a client file only when a person says yes. No profile of a real client ships.
- **0220, a first run on Linux.** Where the build has no key store the passphrase route is the default; an engine that cannot open its vault stops and says why; the presence prompt writes its policy file where a person can install it. Proven against a real authorization service in five stages.

## What was researched and reviewed

- **Research notes 27 to 29.** 27 is a survey of about 45 public agent-tool projects: what makes them used and what they lack. 28 is how a network can be seen from one machine (local state is readable unprivileged on all three systems; no network identity signal is both stable and unspoofable; announcements are claims). 29 is how existing agent tools bound where they look (almost none do; the pattern to copy is a scope the operator sets and the model cannot change).
- **Three design reviews.** Coverage (lens 15, a map of about 110 mechanisms, 14 findings, records 0211 to 0215); breadth (lens 16, ten ranked additions, records 0232 to 0234, the first being watches); configurability (lens 17, records 0236 to 0239). They are filed in `docs/reviews/`.
- **The network design.** Round 1 gave a plan and records 0223 to 0226 (a network as a place, a recognized network, and a reach; three owner questions) and four hypotheses. **Round 2 was stopped by a safety check** partway through a rewrite of the section on listening for announcements. The agent restored round 1's text, nothing of its work remains, and no record was written. It was not worked around and not retried. What it would have reconciled stays open; 0242 allows the rest of the design to be taken up again in small pieces.

## The repository cleanup, 0221

It ran as a pipeline the owner asked for, and it did not run as asked. Five small-model audits read the repository (about 115 findings in all); one verifier checked them, confirmed 31 of the 41 documentation findings with 8 amended, and rejected every rename on cost. A planner then wrote the plan and was to start a worker for the edits. **It could not start one**, because a session started by another cannot start sessions of its own, so it made every edit itself. The lead then started the worker, which moved the three review reports and the coverage map to `docs/reviews/` and corrected seven of the planner's statements. The planner checked the worker and agreed with all seven. **Its check also found that its own edits to section 704 had been reported done and had never been saved**; they were made then. The record was written knowing this, and the lesson is below.

## What was measured

- 1,742 tests passed on Linux on the validated commit before 0200; the lead's count, not rerun here.
- The settings table has 95 kernel keys: 17 are proven to work, 23 are read and unproven, 20 are accepted and do nothing, and 35 are for features not built. Of 28 pack keys, all are read. (The configurability review's count; the 55 that are not proven or read make "no setting is silently without effect" untrue today.)
- Nothing was measured on Windows or macOS, and no performance figure was taken.

## What went wrong, and what it taught

- **The container restarted once.** Nothing on disk was lost and the agents were resumed by name; local services started by hand were gone and were restarted when needed.
- **Settings faults found by review and not yet fixed when this was written.** The engine's home-folder variable can put the installation settings file, which can loosen every protected setting, where the person's own programs can write it; the two-person rule for irreversible operations can be switched off from a lower layer; settings load once, so "at once" is untrue; and the project file applied is the one of the folder the engine started in. Records 0236 and 0237 are accepted and the fix (0241) is being built. Until it lands, the first two are open safety faults on the branch.
- **A planner checking its own work.** A report of "done" that had not been saved was found only because a second reader checked. Check the file, not the report, and have someone other than the author do it.
- **A safety check stopped a design round.** The right response was to stop and tell the owner, and that was done.
- **A join between two agents' work broke.** Two packs registered one file-kind name, which unregistered the web pack's drafting until decision 0203 let each pack draft a kind it registered for its own surface kind; a check that a drafted file names one of the pack's own surfaces is still owed.

## Decided without the owner having read it

Records 0200 to 0221 are design decisions the owner has not read. 0222, 0230, 0231, and 0242 are the owner's. The widest-reaching of the others: evidence names its binding (0200); a shadow binding is promoted only by a person (0207, 0219); the command pack's Windows check and its fixed-pair scripts (0216); the closed list for desktop writes (0218); the cleanup's moves (0221).

## For the owner

- The publishing setup is still the owner's own act, and the first version is 0.1.0 (0242).
- Reword of rule 3 stands by 0242, point 13.

## Next

In this order:

1. **Validate the whole tree** (format, lints, all tests, no-TLS, the database and desktop jobs, lint for the other two systems), push it to pull request 14, and take it through CI on the three systems. Expect the first Windows runs of the desktop back end, the command pack's access-list check, and the PowerShell argument test to fail.
2. **Fix what the fifth round of audits finds,** and the settings faults first (0241).
3. **Wire watches (0247) and the network readers (0243) into the action registry.**
4. **The unbuilt publish gates:** ten of twelve are not built.
5. **What the reviews ranked:** after parity (macOS and Windows desktop, the MCP pack's local program on Windows), the proxy setting, non-JSON answers' drafting, the described-operation shape, and the file pack's new modes (0246).
