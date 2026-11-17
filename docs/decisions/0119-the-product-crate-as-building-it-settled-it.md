# 0119. The product crate, as building it settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** what the design of `anything2mcp` left open about the command line, the one-shot engine, the person at a terminal, and the key store of a machine; and the small changes below the product that the first end-to-end runs through real packs needed. It changes the interface sketch of the crate's design.

## Context

This package was the first time the real packs met the real engine and the real action registry. The product crate was built with every command running the engine inside its own process, because the local engine was not built. The end-to-end proofs go from target files to a value in a test stand through the command line.

## Decisions

1. **The argument parser is our own.** The command tree comes from data: every command, argument, type, and help line is a `CommandFace` the action registry gives. A parser of about four hundred lines reads it. *Instead of:* `clap`, which research note 9 named. Its tree would be built at run time from the same data, so it would add a crate and its tree for the part we do not need (derive, completion, styling) and still leave the rules that are ours: a word that is both a noun and a shortcut, and a global flag whose name an action's field also has.
2. **The rules of the command line** are the list in the crate's design: a word with one of its verbs is the action, otherwise the shortcut or process command of that name; `--version` counts only as the first word, because `version.publish` has a field named `version`; a shortcut of one action takes that action's flags, and a shortcut of several takes its one argument alone; with no argument the program prints help whose first line is the version.
3. **Every command runs the engine in its own process, and `--in-process` is accepted and changes nothing,** until the local engine is joined. `serve`, `runner`, and `bridge` answer `input`, `action_not_built`.
4. **Both forms of output carry the failure.** When output is not a terminal, the one JSON object is the whole response, failed or not, and the failure's class, code, message, and next step are also written to standard error for a person. *Instead of:* the failure on standard error alone, which would leave a program that reads standard output with nothing.
5. **A caller whose input and error output are not a terminal is an agent.** The dispatcher then refuses a person's act before any surface is asked.
6. **The person surface of a one-shot command is the terminal.** It drops the approval desk, so a one-shot command cannot record an approval by any path. For a person's act it asks for the word `yes` at the terminal and makes a proof marked `not_available`. This is the dialog-only fallback of spec section 222: it stops a mistake and is not a security boundary, and the policy engine does not accept that proof for loosening a protected setting. *Instead of:* refusing every person's act until `a2m-local` brings the operating system's prompt, which would leave a first user unable to publish a version.
7. **The machine's key store is chosen by `vault.backend`.** The default is the operating system's store. `passphrase` takes the passphrase from the environment variable `A2M_VAULT_PASSPHRASE` and keeps the salt in the data directory. The setting can be written only in the installation's settings file, which is how a person chooses the weaker route. A key store that does not answer opens the kernel `degraded` with the way out in its fix; it never stops the program. *Instead of:* asking for the passphrase at a terminal, which a command run by a program cannot answer; that form can be added beside this one.
8. **The roots.** `project` and `files` are both the project folder; `command_runs` and `profiles` are folders of the data directory.
9. **End-to-end tests never touch the key store of the machine they run on.** Every test through the binary writes an installation file that chooses the passphrase route. With no passphrase the engine is degraded on every operating system alike, which is enough for the tests of the command line; the tests that write give one.

## Changes below the product, each forced by an end-to-end test

| Crate | Change | The test that needed it |
| --- | --- | --- |
| `a2m-actions` | `trace.show` returns `signature` and `records`, read from the last line of the stored trace. The signature was computed and then reachable by no action | `first_read.rs`: five reads, one trace signature |
| `a2m-ledger` | `Store::store_verified`, an async method in the style of `store_intents`, which carries the proof of `acted` into a transaction of its own | `verified_write.rs`: the event after `acted` is `verified` |
| `a2m-actions` | `Control::verified` uses it, so a verified write is recorded as `verified` and not as `reconcile_read`. The kernel test's assertion was changed to the stronger event | The same |
| `a2m-pack-http` | A `validate` or `extract` step has a time limit of one second, not the request's. The engine sums a binding's step limits before a commit point, and five request limits (150 s) passed the in-process lease (100 s), so no write through `http` could start | `verified_write.rs`: the first resume after approval ended as `busy`, `lease_too_short` |
| `a2m-actions` | `run.resume` waits for a job that was already working on the run to end, and then queues its own. Before, a resume that arrived while the scheduler's reconcile was in hand waited its whole time for a lease that never came | `verified_write.rs`: the resume after the program was killed |
| `a2m-pack-data` | The driver checks the store's file with the pack's own roots when the run was handed no checked path. No public signature changed | `other_packs.rs`: the read through `sql` |

No change was needed in `a2m-engine` or `a2m-provider`. The commands, device, stream, MCP, file, and web packs ran through the engine as they were.

## What was measured

- **H-26.** Seven kinds of system were read through one kernel, and the one change inside the kernel's crates was a wait in `run.resume` that has nothing to do with a kind of system. The data pack's missing path is the one place where a pack wanted something of the engine that no plan can state.
- **H-28.** A release build with nine providers and no TLS, no local engine, no MCP or REST face: 28.9 MB with symbols, 21.8 MB stripped, on Linux x86-64. The workspace has no release profile yet.
- **Start-up,** on one Linux machine, a release build: `--version` in 7 ms; a command that opens the store on disk with nine providers, dispatches `engine.status`, and closes, in about 22 ms; the same with the passphrase route, about 180 ms, because the root key is derived at every start. A debug build takes about 55 ms and 2.5 s. The derivation at every command is one more reason for the local engine.

## What follows

- The crate's design lists the interface, the rules, and the proofs.
- `a2m-local`'s surface replaces `TerminalSurface` when `serve` is joined, and commands become its clients.
- Open: a secret on standard input for `secret set`; telling the scheduler that a runner's process is gone, so that a run it left is stopped at the next start and not when its lease ends; the form of a decimal in a tool's result.

## What would reopen it

A command whose arguments the flat fields of an action cannot express; or a route by which a one-shot command would need to record an approval.
