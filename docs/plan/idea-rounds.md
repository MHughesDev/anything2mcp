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
