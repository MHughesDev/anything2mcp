# 0311. The review shows, from the ledger, what this engine proved

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Supersedes point 2 of [decision 0307](0307-what-the-evidence-level-is-for-and-what-a-review-shows-about-a-tools-evidence.md), and corrects points 1 and 3.

**Date:** 2026-11-23

**Changes:** `fixture.record` and the publish review in `a2m-actions`, roadmap package V2.

## Context

Decision 0307 had a fixture say `recorded_from: trial`, signed "with the digest of the trial". A review against the code found four faults:

1. A trial's digest is a public hash of fixed text, the input and the binding. It keys nothing, so it signs nothing.
2. A real key of this engine would make every honest fixture fail on every other machine and in CI.
3. A builder can record a genuine trial against a stand-in at the target's address.
4. The levels it gave contradicted their own meanings: `schema_verified` means that real answers matched, and `executed` means that the draft ran in the sandbox.

The same review found two things more. A binding can be written `status: verified` by hand, which skips `binding.promote`. And the level does decide one thing already: a promote accepts a read the scout tried at `executed`.

## Decision

1. **No claim about evidence is written into a file.** `fixture.record` writes an entry in this engine's ledger with the digest of the fixture's folder, the run it came from, and the address it was recorded against.
2. **The review looks each fixture's current digest up in this engine's ledger**, and shows one of two things:
   - "recorded here, from run R against address A, unchanged";
   - "not recorded by this engine".

   A clone or a CI machine shows the second, which is true. Editing a file cannot change the answer.
3. **A tool's level at review follows the meaning of the levels:**
   - with only written fixtures, at most `reconstructed`;
   - with a fixture this engine recorded and that is unchanged, `schema_verified`;
   - with a trial this engine's ledger holds, `executed`, as the code gives it today.
4. **The review shows each binding marked `verified` and whether a promote in this ledger proved it.** One that no promote proved is named first.
5. **Point 1 of decision 0307 is corrected.** The level decides two things in the code: a finding may not claim more than its source gives, and a promote accepts a read the scout tried at `executed`. Nothing else decides by it.
6. **Point 3 of decision 0307 is corrected.** The gate runner holds no table today, and package V2 builds one. The review shows each gate as passed, failed, or not built, from that table.

## What this risks

The review says what this engine saw, and nothing about the system's real behaviour beyond that. A stand-in at the target's address still records a genuine fixture. The review shows the address, so a person can see it.

## Proof

- A version whose only fixtures were written by hand is shown at `reconstructed`, with every fixture "not recorded by this engine".
- A fixture recorded, then edited by one byte, is shown as not recorded.
- A binding written `verified` by hand is named first.
