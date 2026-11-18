# 0266. The engine's log says each start and each stop and what asked for it, and `engine logs` reads it where the engine runs

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

## Context

Round 16 of the [idea rounds](../plan/idea-rounds.md) found the engine's log empty after some three hundred commands, and `engine logs` refused as a command on the engine's own machine. The investigation of round 12 found an engine that had stopped for a reason recorded only as `asked`, and could not say what had asked: a signal and `engine.stop` left the same last object. It also found that a hang-up signal ended the engine with no last object and its socket left behind, and that a signal sent while the kernel was still opening did the same, because `serve` listened for signals only once the kernel was open.

## Decision

1. **What asked the engine to stop is recorded, the first only.** `serve` listens, from its first step and before the kernel opens, for the interrupt, the termination signal, and the hang-up on Unix, and for the console's interrupt, break, close, and shutdown events on Windows. A signal that comes while the kernel opens is kept and stops the engine cleanly as soon as it serves. The hang-up is taken as a termination. One handle, `StopAsk` in the product crate, is the only way the engine is asked to stop from outside: it records what asked (`StopBy`: a signal by the system's own name, such as `SIGTERM` or `CTRL_CLOSE_EVENT`, or `engine.stop` with the process the local channel saw on the other end), keeps the first, writes one line to standard error at once, and stops the channel. `StopReason` of `a2m-local` is unchanged.
2. **`serve`'s last object names it.** `{"stopped": "asked", "by": "SIGTERM"}`, and for `engine.stop` also `"process": N` where the system names the peer (always on Linux). An engine that stopped by itself (`idle`, `restart`) names nothing; `stopped` keeps its words, so nothing that read it breaks.
3. **The log says each start and each stop.** At the default level, through the subscriber the kernel's telemetry installs: `engine.started` with `pid`, `version`, `on_demand`, and `by` (`serve` or `on_demand`), once the engine is open and before it says it is ready; and `engine.stopped` with `why` (`idle`, `asked`, `restart`, `data_gone`, `failed`) and, when it was asked, `by` and `process`, before the kernel closes. Neither carries a call's input or any text a caller chose.
4. **`engine logs` is read by the command where the engine runs.** The command line answers `engine.logs` itself, before it looks for an engine: it reads `engine.log` and the files rotated out of it with `recent_lines` of `a2m-telemetry`, which opens no file for writing and makes nothing, and answers `{"file", "exists", "lines"}`, each line the object the engine wrote or its text. `--lines` is 50 when not given and 1 to 1,000. It starts no engine, whatever `engine.in_process` says, and a home that does not exist stays so. A file of the log that is there and cannot be read is `busy`/`log_unreadable`, a new code of `a2m-telemetry`, whose table moves to the `failure_codes!` form in the same change.

Also decided here, because they were built in the same package:

5. **Help says what an action does to the world, and whether it is built.** Every action's help gives a line with its effect class, who may run it in the action catalog's words, and whether a person at this machine is asked (`no`, `always`, `when it loosens something protected`). The help of an action that is not built says so on its first line, with the phase that builds it, and the top-level and noun listings mark each such verb `(not built)`. An action the registry gives to the client end and that the command line does not carry out (`self.update` today) is shown as not built. Help is wrapped to the terminal's width: `COLUMNS` when it is a whole number of 40 or more, else 80. No dependency was added: `COLUMNS` and the default meet round 8's finding.
6. **Shell completion is a command the program answers itself.** `completion <shell>`, for `bash`, `zsh`, `fish`, and `powershell`, prints a script made from the command tree the help is printed from: commands, flags, and each flag's help where the shell shows one. Values that live in the engine, such as target and tool names, are not offered, and the script's header says so. Nothing in a script runs the program when it completes. It is written as a hand-made command of the product's parser beside the process commands, not as an action: an action is answered by an engine today, and answering one without an engine waits on the change that lets the program answer an action alone.

## Consequences

- A stray signal on a shared machine is named in the engine's last object and in its log; `start_on_demand.rs` asserts `by: engine.stop` on every stop it makes, so such a signal fails the test that meets it with its name.
- The refused-call line of the log (`call.refused`, by class and code) and the output schema of `engine.logs` belong to the action registry and are not built here; nor is `engine.logs` over MCP or REST, which still answers `client_action`.
- The Windows console events are compiled and linted for Windows here, and not run.

## Alternatives weighed

- **A new `StopReason` per sender in `a2m-local`.** It would change a public enum that the channel's own logic matches on, for a fact only the product crate's process knows. A record beside it is smaller and leaves the channel as it is.
- **Raising the log level instead of writing lines.** No crate but the session keeper's wrote any event, so a higher level shows nothing more.
- **`engine logs` through the engine.** It would start an engine to read a file the client can read as the same user, and could not answer when the engine will not start, which is when the log matters most.
- **Completion as an action.** It would give it every face, but an action needs an engine to answer it today, and completing a word must start nothing.
- **A crate for the terminal's width.** `COLUMNS` with a default of 80 is enough for help, and adds no dependency.
