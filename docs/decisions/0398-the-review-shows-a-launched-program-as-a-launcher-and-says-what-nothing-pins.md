# 0398. The review shows a launched program as a launcher, and says what nothing pins

**Status:** Accepted. Design decision, made by the design architect. The owner had not read this record when it was merged. Nothing in it is built. It changes the review summary, which a front door serves, by addition.

**Date:** 2026-11-24

**Changes:** `ProgramRun` and the notices of `crates/a2m-files/src/review.rs`; what the MCP pack's lowering states (`crates/a2m-pack-mcp/src/lower.rs`); the publish page of `a2m-local`. Carries out what [decision 0371](0371-the-mcp-pack-starts-a-server-through-a-launcher-with-its-installed-entry-file-pinned-where-it-lies.md), point 6, owed, as [decision 0390](0390-the-open-questions-of-2026-11-24-are-settled-by-the-lead-and-the-first-release-is-what-progress-counts-toward.md), point 13, asks before the first release. New roadmap package L4.

## Context

A server started through a launcher runs an entry file that an installer put on the machine. The product pins the launcher and the entry file by digest. It pins nothing the entry file loads, and nothing that code starts. Decision 0390 said where that risk belongs: on the page, at the moment a person publishes.

Read from the code on 2026-11-24: the MCP pack states such a run as `ProgramRun::Script`, and the page shows it with the notice written for a script the target keeps: "runs a script this target keeps: the digest pins its bytes; it does not make it safe". For an installed entry file that sentence is untrue in its first half and silent on the point that matters.

## Decision

1. **`ProgramRun` gains `Launcher`:** the launcher's path and digests, the arguments it is always given, the entry file's path as it lies on the machine, and the entry file's digest.
2. **It carries a fixed notice of its own,** `LAUNCHER_NOTICE`: "starts a program through a launcher: the digests pin the launcher and its entry file; whatever the entry file loads, and whatever that starts, is not pinned and is not shown here".
3. **The MCP pack states a launched server as `Launcher`,** and a script the target keeps stays `Script`.
4. **The ordering of the summary counts a launcher as it counts a script:** among what a tool starts, before a pinned program.
5. **No hazard is added and no operation's class changes.** Decision 0371, point 6, stands: the notice informs, and gates nothing.

## Alternatives

| Alternative | Why not |
| --- | --- |
| List the packages the entry file loads | It means reading and resolving another runtime's dependency files at review, a guess about what will load, shown as a fact |
| Reword the script's notice to cover both | One sentence would then be untrue for one of the two |

## Proof

Package L4's row: in `crates/a2m-files/src/tests/review.rs`, a target whose MCP server is started through a launcher gives a run of the kind `launcher` with the notice, and a script the target keeps still gives `script` with its own; through the product, the publish page of such a target shows the launcher, the entry file's path and digest, and the sentence.

## Revisit if

A sandbox for started programs is built, or a launcher's own lock file is found to pin what the entry file loads.
