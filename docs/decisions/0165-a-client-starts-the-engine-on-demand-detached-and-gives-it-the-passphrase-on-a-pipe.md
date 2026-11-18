# 0165. A client starts the engine on demand, detached, and gives it the passphrase on a pipe

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** start on demand, step 2 of the lifecycle in [section 222](../spec/2-kernel/222-front-doors-and-the-local-engine.md), which the designs of [`a2m-local`](../architecture/crates/a2m-local.md) and [`anything2mcp`](../architecture/crates/anything2mcp.md) listed as not built; the open point of research brief 26 on starting a detached process on each system. It revisits [decision 0131](0131-the-front-doors-joined-serve-the-thin-client-and-the-bridge.md) points 4 and 6 and keeps the lock rule of [decision 0158](0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md).

## Decision

**Who starts one.** A command that finds no engine starts one and is then its thin client; so does `bridge`, for the first request that finds none and again after an engine has stopped; and `anything2mcp engine start` starts one by name and says whether it did (`{"started": true, "engine_version": ...}`). `engine status` and `engine stop` never start one: they mean the engine that runs. `--in-process` starts none. *Instead of* starting from the bridge at once when it starts: a client that lists its servers at start-up and never calls would leave an engine running for nothing until its idle time.

**How, in `a2m-local`.** `Connector::connect_or_start(paths, &Launch, patience)`:

1. Looks at the channel. While another client holds the start lock (`start.lock`, an operating-system file lock in the runtime directory, taken without waiting), it looks again every 25 ms.
2. With the start lock held, it looks again. An engine lock that is held while nothing answers is an engine starting or closing, or a command that runs the engine inside itself, and is waited for.
3. It starts the program detached and waits for the channel to answer its hello, which the engine gives only once its kernel has opened. A started engine that ends before answering, and whose end leaves the engine lock free, is `busy`/`engine_unreachable` with the log's path; one that lost the engine lock to another engine is waited for like any other.
4. Everything ends at `patience`: 30 seconds from the product. Then `busy`/`engine_unreachable`, exit code 4.

The wait is on a named condition, the channel answering, with a deadline; the operating system offers no handle to wait on for a socket or a pipe that does not exist yet. Two clients that start at once end with one engine: the start lock lets one start it, and if two engines are started anyway (two runtime directories leading to one data directory), the engine lock lets one bind and the other ends.

**Detached, on each system.**

- macOS and Linux: the child calls `setsid` between fork and exec, so it is in a session of its own, with no controlling terminal, and no signal of the caller's terminal reaches it. This is the crate's one new `unsafe` block, in `os/unix.rs`; `setsid` is safe to call there. The standard library's own `CommandExt::setsid` is still unstable in Rust 1.100. *Instead of* the child calling `setsid` itself once it runs, which leaves it in the terminal's foreground group until then, and *instead of* a new process group alone, which keeps the controlling terminal.
- Windows: `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB`, and again without the last when the caller's job does not allow a process to leave it (`ERROR_ACCESS_DENIED`); the engine then lives inside that job. Before the start, the client makes its own three standard handles not inheritable: Windows gives a new process every inheritable handle of its parent, so an engine started by a command would otherwise hold the command's output open and whoever reads it would wait for the engine to end. The standard library still gives a child that inherits a stream a copy made for it.
- A refusal to start a process is `Started::Refused`. A command then runs the engine inside itself, holding the engine lock, and says so on standard error; the bridge answers with the failure.

**What the started engine is, and what it inherits.** The product builds the `Launch` (`cli/start.rs`):

| Part | What | Why |
| --- | --- | --- |
| Program | This program's own file (`Host::program`, from `current_exe`) with `serve`; `--project <folder>` when the caller has a project | The same engine `serve` would be in that project. A host that embeds the command line, and a test, set `program: None` and nothing is started |
| Working folder | The data directory | It outlives every project folder and holds none open; on Windows a held folder cannot be removed |
| Environment | Cleared, then only: `PATH`, `HOME`, the user's name, the temporary folder, `A2M_HOME`, the language and time zone, `LC_*` and `XDG_*`, the desktop's display and bus addresses, the Windows folders and system variables a program needs to start, and the settings variables `A2M_<GROUP>__<KEY>` | A variable that holds a credential is none of these, so none reaches the engine. A list of what is kept fails closed; a list of what is removed would let a new credential name through. A proxy variable can carry a password and is not kept: `network.proxy` is the setting |
| Standard input | Empty; or, when the vault takes its key from a passphrase, the passphrase, written once to a pipe and closed, read by `serve --passphrase-from-stdin` | See below |
| Standard output and error | Appended to `engine.log` in the log directory, mode 0600 on macOS and Linux; the client appends one line naming the started process | A person can read why an engine did not start |
| Other handles | None: every file, socket, pipe, and lock this program opens is closed on exec (macOS and Linux) or not inheritable (Windows); the start lock is not inherited | A handle the caller's own parent leaked to it without that flag passes on, as it would to any program it starts |

**The vault's key on a machine with no key store.** On Linux this build reaches no key store, so the passphrase route is the one there. The passphrase goes to the started engine on its standard input, which only it reads, and never in its environment or its arguments, which every program of the user can read for as long as the engine runs (`/proc/<pid>/environ` and `cmdline`; on macOS the process's argument area). When the vault takes its key from a passphrase and the caller was not given one, nothing is started: a command runs the engine inside itself, whose vault then says what is missing, and the bridge answers that the engine must be started with `anything2mcp serve`. *Instead of* the environment, which would put the passphrase where other programs read it for the engine's whole life; *instead of* refusing to start on demand under the passphrase route, which would mean never on Linux; *instead of* a file, which another program could read before the engine removes it.

**The setting.** `engine.in_process` ([section 226](../spec/2-kernel/226-configuration.md)) is the setting that says never start: with it on, a command runs the engine inside itself, as the `ci` preset wants, and the bridge, which cannot (decision 0131, point 6), tells its caller to run `anything2mcp serve`. An engine that already answers is used either way. *Instead of* a new key such as `engine.start = "on_demand" | "never"`: the schema is `a2m-actions`'s, and one key that says what happens when no engine answers is enough today.

**Idle stop** is unchanged: `engine.idle_stop_minutes`, with no connection, no page waiting, and no run in hand. An engine started on demand stops by itself, and the next client starts another.

## What was proven, and what the other systems must show

Proven on Linux, through the real program (`tests/start_on_demand.rs`): a command with no engine starts one and is answered, and ends while the engine runs on (so the engine holds none of its streams); the engine is in a session of its own, in its data directory, with the passphrase in neither its environment nor its arguments; two commands at once start one engine (one start line and one ready line in the log); a bridge starts one, and after one minute with nothing in hand it stops by itself (`{"stopped": "idle"}`) and its lock is free; with `engine.in_process` on, the bridge is told to run `serve`, a command runs the engine inside itself and starts nothing, and `engine start` starts one by name.

The next Windows CI run must show the same four tests passing: the creation flags accepted, or the start retried without leaving the job; the command's output read to its end while the engine runs, which shows its standard handles were not inherited; and the idle stop letting go of the lock. The next macOS run must show the same tests, the Linux-only checks of session and environment aside. Not built: the Python client starting an engine (it can run `anything2mcp engine start` when it finds none); rotation of `engine.log`; the drain and restart of an older engine; the measured cold start of section 222.
