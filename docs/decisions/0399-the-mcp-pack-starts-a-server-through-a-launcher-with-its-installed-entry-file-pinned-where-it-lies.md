# 0399. The MCP pack starts a server through a launcher with its installed entry file pinned where it lies

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). It builds package L3 of what [decision 0299](0299-programs-that-run-what-their-arguments-name-are-reached-in-two-steps.md) decided, decides the question [decision 0349](0349-the-two-steps-of-decision-0299-as-built-a-closed-list-of-commands-and-the-javascript-runtime-as-a-launcher.md) left to L3, and supersedes 0299, point 6, in part. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** [spec section 304](../spec/3-packs/304-the-mcp-pack.md); the design of [`a2m-pack-mcp`](../architecture/crates/a2m-pack-mcp.md); the [MCP test stand](../../targets/example-mcp/README.md); the [technical reference](../reference/technical-reference.md) (three failure codes); the [roadmap](../plan/roadmap.md) and [the build plan](../plan/build-order.md) mark L3 done.

## Context

Most local MCP servers are started through a launcher given an installed entry file, or through a package runner given a package's name. Decision 0299 let the first in, pinned, and kept the second out. Decision 0349 built the launcher as a row of the provider's closed table of interpreter forms for the command pack, which copies a target's script into the run's private folder and runs the copy alone, and left to L3 how a server's installed entry file is run, since such a file loads the files beside it.

## Decision

1. **The surface names the launcher and the entry file.** An `mcp` surface with the address `stdio` names the launcher as `program`, pinned by `digest`; the server's installed entry file as `entry`, pinned by `entry_digest`; the fixed arguments of the launcher's form as `launcher_args` (none for the JavaScript runtime, `-I` for Python); and the server's own arguments as `args`, which follow the entry file. Both files are programs the plan declares for the surface, so the review shows both with their digests, and the driver starts neither unless the plan declares it so pinned.
2. **The checks are the provider's, shared, not the command pack's.** The pack calls the provider's `interpreter_form` (the closed table) and `check_program` in the role `Interpreter` for the launcher, by its declared name and by the name of the file its path leads to, and `check_program` in the role `Script` for the entry file: it and every folder on the way to it closed to other users. The copying of a script into the run's folder lives in the command pack and is not shared: the MCP pack does not copy, and depends on no other pack.
3. **The entry file runs where it lies.** An installed entry file loads the files beside it and its packages, so a copy alone would not run. Before every start, after the check of who may change it, its bytes are read and their digest compared with the pin; a file that differs stops with `drift`/`entry_digest_changed`, and nothing starts. Between that reading and the launcher's opening of the file, only the engine's user or the administrator could change it, as for any program the supervisor starts.
4. **A package runner is refused by name, with what to do instead.** A program whose declared name or real name is a package runner (`policy`/`package_runner_refused`), or which is given an entry file and is not a launcher of the table in its form (`policy`/`launcher_not_in_table`), is refused before anything starts; the lint refuses the same at compile (`mcp.package_runner`, `mcp.launcher_form`), and `mcp.program_unpinned` covers an entry file with no digest. The pack's list of package runners only makes the refusal say what to do; a test holds every name on it to the provider's name list, which refuses each as a program anyway.
5. **The review shows the pair as a fixed pair.** The binding's statement is the review's `Script` run: the launcher, its digests, its fixed arguments, and the entry file with its digest. The review's notice beside it says the digest pins its bytes and does not make it safe.
6. **Decision 0299, point 6, in part: no operation is made to declare `sends_outside`.** The compiler derives `irreversible` for any operation with a hazard, and an irreversible operation's result is never called verified. Requiring the hazard would therefore make every read and write through a launched server irreversible, and no write through one could be verified, which is this package's proof. The hazard stays the developer's to declare. Naming, for each such tool, the code nothing pins (the packages the entry file loads, and what those start) needs a kind of run in the review summary that `a2m-files` does not have yet; it is owed there.
7. **The stand runs as an installed package.** The MCP stand writes `server.js` and the file it loads beside it into its own folder (`StdioStand::launcher_entry`). The entry file starts the stand's own program, named by its first argument, on the same standard streams. These are the stand's first files in JavaScript, which is the language of the servers they imitate (rule 6).

## Alternatives

| Alternative | Why not |
| --- | --- |
| Copy the entry file into the run's folder, as the command pack does a script | It loads the files beside it and its packages; a copy alone does not run, and copying the package tree is the pin of every file that decision 0299 turned down |
| Make the MCP pack depend on the command pack for the checks | A pack depends on no other pack; the checks are already the provider's |
| Require `hazard: sends_outside` on every operation on a launched server, as 0299 said | It makes every such operation irreversible, so nothing through a launched server can be verified (point 6) |
| Keep the list of package runners in the provider | The pack only needs it to word a refusal; the provider refuses each by name already |

## Revisit if

The review summary gains a run for a launcher that names what nothing pins (then point 6 is built there); a field test meets a server whose entry file the table's form cannot start; or a sandbox for started programs is built.
