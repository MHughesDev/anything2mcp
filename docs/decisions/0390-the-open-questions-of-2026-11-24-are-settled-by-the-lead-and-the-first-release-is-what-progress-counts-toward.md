# 0390. The open questions of 2026-11-24 are settled by the lead, and the first release is what progress counts toward

**Status:** Accepted. Design decision. The owner asked the lead, as the architect, to answer the lead's own eighteen open questions and to weigh again the assumptions it held with low confidence; the answers are the lead's, given on the owner's behalf, and none is an owner decision. Three of them (points 2, 3, and 5) are recommendations, because only the owner can act on them. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the [roadmap](../plan/roadmap.md) (the reason 6.1, 6.2, and 6.5 are held; three new rows, 9.1a to 9.1c); [the build plan](../plan/build-order.md) (the progress line); nothing in the product.

## Context

At the end of the stretch that [the journal entry of this morning](../journal/2026-11-24-a-gate-before-every-push.md) covers, the lead held eighteen questions it had not put to anyone and a list of assumptions it was not sure of. Several records of the night ([0364](0364-the-command-pack-reads-silence-as-absent-a-path-into-nested-output-and-a-programs-own-server-as-no-hazard.md), [0371](0371-the-mcp-pack-starts-a-server-through-a-launcher-with-its-installed-entry-file-pinned-where-it-lies.md), [0379](0379-a-read-of-an-element-the-page-drew-again-finds-it-by-the-query-that-found-it.md), [0386](0386-a-reads-run-is-committed-without-a-disk-sync-of-its-own.md)) each made a call that touches rule 2 (safe, then correct, then fast) and that nobody but their builder had weighed.

## Decision

### Scope and pace

1. **The shared-server packages wait for the first release.** 6.1, 6.2, and 6.5 stay held, and 6.4 and 6.6 with them: AGENTS.md says the first release is for one person's machine and a shared server comes after it. Their dependencies being done does not make them ready.
2. **Recommended to the owner: one sitting at the end.** The steps only a person can take (3.6's prompts on macOS and Windows, 0.2p, 9.5, 9.6) are gathered into one checklist, which the lead writes when the last agent-buildable row of the first release is done. Nothing built waits on them.
3. **Recommended to the owner: an own computer for macOS and Windows is asked for again once**, when package 8.3 needs a desktop session that a CI runner does not give. Until then CI's runners and the cross lints are enough.
4. **Pace: two or three workers at once, the cheapest model that fits, and a finished row before a started one.** The limit is this machine's disk and the owner's weekly allowance, not the number of ready rows.
5. **Recommended to the owner: install the GitHub App on the repository.** Until then the lead polls a pull request it opened.
6. **A phase's exit test is run and written up when its last package merges, not at the end of everything.** For the phases already complete, the write-up is owed now: one journal entry that names, for each phase, its exit test, the test that is it, and its last result on three systems. It is a lane of its own and stops no other work.
7. **Progress stays a count of packages, and is stated against the first release.** A weight per package would be a guess stated as a number. The progress line says two things: packages done of all packages, and how many of those left an agent can build before the first release (the rest are after the release or the owner's).
8. **What 9.1 left becomes three rows.** 9.1a, a tool whose knowledge is marked as drifted is shown as unavailable where tools are listed; 9.1b, canaries run on their own schedule; both are before the first release, because a person must be able to see that a tool stopped and why without calling it. 9.1c, the repair bundle and proposer (H-49), is after the first release; it works at authoring time, so rule 1 does not forbid it.
9. **Field tests against copies we run ourselves go on**, under [decision 0293](0293-software-we-run-ourselves-is-tested-against-during-the-build-and-may-be-written-to.md) unchanged: our own copy, invented data, the recipe outside this repository. They are the only evidence the project has from software it did not write.

### Design

10. **Traffic capture joins the provider contract as a capability a pack may offer, not as a part of the web pack.** A pack that offers it says so in its manifest; a pack that does not is asked nothing. What a capture yields passes the redactor before it leaves the pack (rule 4), and keeps structure only, as [decision 0195](0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md) says. The record that changes the contract is written with package E2.
11. **Decision 0386 stands.** A read changes nothing in the target, so what a power loss can take is the last records that a read happened; rule 3 is about writes. Three conditions hold it, and each is a test today: a request with a diff, an action an administrator's role is needed for, and every write are synced. SCOPE.md is to say that a read's own records may be lost when the machine loses power.
12. **Decision 0379 stands, with one condition made explicit.** A read may find again an element the page drew again, by the query that found it, only when that query matches exactly one element; none or more than one is a stop (rule 7). An act never finds an element again. If the code does not hold the first clause as a test, the next worker in the web pack adds it.
13. **Decision 0371, point 6, stands.** A hazard says what an operation does to the world; verification says whether a read-back agreed. Making every operation through a launched server declare `sends_outside` would answer a question about the server's code with a mark about the operation, and would cost every verified write. The risk is real and belongs where the person meets it: the review, at the moment the launcher and entry file are pinned, names the code nothing pins. That is the piece 0371 says is owed in `a2m-files`; it is built before the first release.
14. **Decision 0364, point 2, stands, read narrowly.** `own_server` is the person's statement of where the target's records live. It decides only that a command is not marked as sending outside; it never makes a write verified. Verification comes from the read-back alone, by the kernel's rule. The review shows each declared host, so the statement is one the person made knowingly.

### Housekeeping

15. **N0 runs now, alone, before any other row starts**, because it touches every crate. The profile whose name is a real framework's is renamed `server_postback_forms` in the same sweep (rule 9).
16. **A pull request is merged with a merge commit.** AGENTS.md asks for one concern per commit; a squash would undo that.
17. **The lead stays on this machine.** Two or three workers each with a build folder of their own fit; a new machine would cost the context of the work.
18. **The gate runs here when the disk is free, and in sibling cloud sessions when a worker is building.**

## The assumptions, weighed again

| Assumption | Now |
| --- | --- |
| The roadmap's last row is what "finished" means | Wrong as stated. The agents' finish line is every row before the first release that an agent can build. The release, the real systems, and the shared server are not on it |
| Renaming what rule 9 forbids needs no leave | Right; it is the rule's own demand |
| The cause of the lost answer of `engine.stop` on macOS ([decision 0382](0382-the-engine-stops-once-its-answer-to-engine-stop-is-handed-over-and-a-lost-one-is-no-write-in-doubt.md)) is the order of the answer and the stop | Unproven, and not left so: a test that stops an engine many times in a row on each system is owed, as a proof and not as a retry |
| A text control's commit by `blur` holds on every system | Shown on Linux only. 8.4a's row says so until CI's browser job shows the rest |
| H-12 cannot be tested here | Stands; it stays marked as resting on judgment |
| The night's records need no second reader | Wrong. Points 11 to 14 are that reading for the four that mattered most; [decision 0372](0372-a-push-waits-for-the-checks-of-cis-linux-job-run-here-and-ci-keeps-the-compiled-dependencies.md) was read with them and stands |

## Alternatives

| Alternative | Why not |
| --- | --- |
| Build the shared server now, since its rows are unblocked | It widens what the first release must prove, and AGENTS.md puts it after |
| Weight progress by size | No measure of size exists that is not a guess |
| Sync reads to disk again | It costs more than half a read's speed and protects nothing rule 3 names |
| Capture as the web pack's own interface | The desktop and command packs can observe traffic too, and a second interface later would be the same contract stated twice |

## Revisit if

The owner reads this record and disagrees with a point; a field test shows a read re-found the wrong element; or a launched server is found to have sent a person's data outside.
