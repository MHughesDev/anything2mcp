# `anything2mcp`

The product: the one program, and the only code that names a pack. It reads the settings, fills the provider registry with one call per first-party pack, opens the kernel, and runs a command line that is generated from the action registry apart from a few process commands. `a2m` is the same program under a short name.

**As built today** ([decision 0119](../../decisions/0119-the-product-crate-as-building-it-settled-it.md), [decision 0131](../../decisions/0131-the-front-doors-joined-serve-the-thin-client-and-the-bridge.md), and after its safety audit [decision 0158](../../decisions/0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md)): `serve` runs the local engine in the foreground, a command is its thin client when one runs, and `stdio` is the stdio proxy an MCP client starts. A command or a stdio proxy that finds no engine starts one on demand, detached, and talks to it; with `engine.in_process` on, a command runs the engine inside its own process and the stdio proxy tells its caller to run `serve` ([decision 0165](../../decisions/0165-a-client-starts-the-engine-on-demand-detached-and-gives-it-the-passphrase-on-a-pipe.md)). When an engine opens, it reads `network.trust_roots` and sets what every HTTP client trusts ([decision 0166](../../decisions/0166-https-in-the-product-trust-roots-at-engine-start-and-one-maker-of-clients.md)); the feature `tls` is on by default, and an engine whose data directory is removed refuses the next request by name and stops ([decision 0181](../../decisions/0181-the-product-and-the-local-engine-take-up-what-the-kernel-now-offers.md)). The server and runner roles are not joined, so `runner` is named and answers that it is not built. Nine providers are registered: `web` and `web_probe`, `http`, `mcp`, `command`, `data`, `file`, `stream`, and `device`. The end-to-end proofs of the whole product live in this crate's `tests/`, one test program, `product`, with each file a module of it ([decision 0446](../../decisions/0446-the-products-end-to-end-tests-are-one-test-program.md)).

## Facts

| | |
| --- | --- |
| Side | Product |
| Layer | 11, Product |
| May depend on | Everything |
| Depends on | `a2m-actions`, `a2m-provider`, `a2m-core`, `a2m-vault`, `a2m-logs`, `a2m-policy`, `a2m-auth`, `a2m-local`, `a2m-mcp`, `a2m-api`, and every pack crate that is built. To come: `a2m-runner` and the desktop pack. Outside: `serde_json`, Tokio, axum for the router the roles are joined in, `futures-core` for the one stream the stdio proxy wraps, and `tracing` for the engine's own start and stop lines; all five are in the tree of the role crates already. No command-line parser crate: the command tree is data, and a small parser of our own reads it ([decision 0119](../../decisions/0119-the-product-crate-as-building-it-settled-it.md)) |
| Roadmap packages | Roadmap package 1.1 (a command that prints one read, in process); 1.4 (`serve` on one machine, commands as thin clients); 1.5 (the command tree from the registry); 5.2 (shortcuts and `init`); 6.2 and 7.1 (server and runner roles) |
| Carries out | Spec sections [226](../../spec/2-kernel/226-configuration.md) for loading, [222](../../spec/2-kernel/222-front-doors-and-the-local-engine.md) for the clients, [220](../../spec/2-kernel/220-actions-and-the-authoring-interface.md) for the command face, [605](../../spec/6-running-it/605-packaging-and-release.md) and [605](../../spec/6-running-it/605-packaging-and-release.md) for what is shipped |

## Owns

- `main` for both program names.
- Reading settings once at start, through the loader in `a2m-actions`, and handing each pack its own group.
- Filling the provider registry: one call per enabled first-party pack, then the plug-ins found on the machine; where a pack has several crates, joining them, as with the web pack's shared browser handle.
- Choosing and starting a role, and joining the role crates to each other: the routers of `a2m-api` and `a2m-mcp` onto the local channel of `a2m-local` or the server listener; the stdio proxy onto a `Link`; the runner link's service onto the server listener.
- The command line: the command tree built from the action registry, the shortcuts, and the process commands `serve`, `runner`, `stdio`, and `channel`, which carries a client's own bytes to the engine after the check of its owner every command makes, for the Node client on Windows ([decision 0999](../../decisions/0999-the-node-client-on-windows-reaches-the-engine-through-the-channel-command.md)).
- The output contract and the exit codes, applied to every `Response`.
- The library facade for a host that embeds the engine.

It must never: do anything with a pack except join its crates and hand it to the registry; hold a branch on an action's result beyond choosing the exit code from its exit class; define an action or a face by hand; read a setting after start.

## Interface

As built.

```rust
// main.rs and bin/a2m.rs: both call the same function, so the two names behave identically.
pub mod cli {
    /// The program's main. Unsafe to call: it removes the vault's passphrase from
    /// the process environment, which is sound only before any other thread starts.
    pub unsafe fn run(args: impl IntoIterator<Item = String>) -> ExitCode;
    /// The same with the process's surroundings as data, for a test.
    pub async fn run_with(args: &[String], host: &Host) -> Finished;
    /// One command line on an engine that is open already.
    pub async fn run_on(engine: &Engine, args: &[String], json: bool, is_agent: bool) -> Finished;
    pub fn usage_lines() -> Result<Vec<String>, Failure>;   // every action command, for the parity test
    /// `program`: the program a client starts as `serve` when it finds no engine;
    /// `None`, for an embedding host or a test, starts nothing.
    pub struct Host { cwd, environment, stdout_is_terminal, person_at_terminal, person, key_store, program }
    pub struct Finished { code: u8, stdout: String, stderr: String }
}

/// The library facade, for a host that wants the engine in its own process.
pub struct EngineOptions { paths, project, environment, flags, place, key_store, clock, random, os_user, person }
impl EngineOptions { pub fn of_machine(paths: Paths, environment, person: Arc<dyn PersonSurface>) -> Self; }
pub struct Engine { /* the kernel, and the one caller a process on this machine is */ }
impl Engine {
    /// With the store on disk, takes the engine lock and holds it until `close`.
    pub async fn open(options: EngineOptions) -> Result<Engine, Failure>;   // settings, the lock, the caller
    pub fn kernel(&self) -> &Kernel;
    pub fn caller(&self, door: FrontDoor, is_agent: bool) -> Caller;
    pub fn request(&self, action: &str, input: Json, caller: Caller) -> Request;   // in the project folder
    pub async fn dispatch(&self, request: Request) -> Response;
    pub fn runner_problem(&self) -> Option<&Failure>;
    pub async fn close(self);
}
/// The action registry of this build without opening a store: for help and for the command tree.
pub fn action_registry() -> Result<ActionRegistry, Failure>;

/// The local engine: what `serve` runs, and what a host that embeds it starts.
pub struct LocalEngine { /* the engine, its LocalSurface, the bound channel, the one router */ }
impl LocalEngine {
    /// Binds the channel, which takes the engine lock, then opens the kernel behind it.
    /// The person surface of the options is replaced by a LocalSurface over `prompt`.
    pub async fn start(options: EngineOptions, prompt: Arc<dyn PresencePrompt>) -> Result<LocalEngine, Failure>;
    pub fn engine(&self) -> &Engine;
    pub fn surface(&self) -> &LocalSurface;
    pub fn control(&self) -> ChannelControl;       // stops it once it serves
    pub async fn serve(self) -> StopReason;        // until asked or idle; then closes the kernel
}
pub mod cli { pub const READY_LINE: &str; }        // what `serve` writes to standard error when it answers

/// The person surface of a one-shot command, and the terminal it asks at.
pub struct TerminalSurface; pub trait Terminal; pub struct ProcessTerminal;
pub const PASSPHRASE_VARIABLE: &str = "A2M_VAULT_PASSPHRASE";

// packs.rs, private: the only place a pack is named. One entry per provider: its manifest,
// whose settings are the schema of its group, and the one call that hands it to the builder.
fn first_party() -> Vec<FirstParty>;
fn fill(builder: &mut RegistryBuilder);
```

Joining a pack's own crates is the only other thing this crate does with a pack: the web pack's two crates are given one `SharedBrowsers` handle, made here. Each pack crate exposes one registration call. Whether a pack is enabled is the setting `providers.disabled`, which the registry builder reads: a pack that is switched off is listed as `not_enabled`, registers nothing, cannot be reached, and costs nothing at run time. A pack with a problem is listed with the problem named, as `web_probe` is when `http`, which owns a kind it reads, is switched off.

**What the host gives the packs.** The supervisor, the redactor and the logs and traces of the prepared kernel, and four named roots: `project` and `files`, which stand for the project folder of each request (`<project>`, the default of `files.allowed_roots`) and are bound to no folder (`Roots::add_project`, [decision 0193](../../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)); `command_runs`, a folder under the data directory; and `profiles`, the data directory's own. A pack resolves `project` and `files` in the checked project folder of the run's request, so every engine, one a command runs inside itself, `serve`, and one started on demand, resolves a request of one project in that project, whichever project started it, and never in the folder it was started in; a run whose request came with no project folder stops with `input`/`project_folder_missing`, unsent ([decision 0194](../../decisions/0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md); [decision 0192](../../decisions/0192-a-served-engine-hands-no-root-of-the-project-that-started-it-and-clients-start-and-compare.md) refused a root bound to the starting project). An engine's own folder whose folder does not exist is left out, and a pack that needs it learns so when it checks a path. A provider action is handed the same roots and no checked project folder yet, so one that resolves `files` is refused `project_folder_missing`; none of the first-party packs has one.

**The vault's root key on this machine.** `vault.backend`, which only the installation's settings file can set, and whose default is the build's (`DEFAULT_BACKEND` of `a2m-vault`: `passphrase` where the build has no key store, as on Linux; [decision 0220](../../decisions/0220-a-first-run-on-linux-needs-no-administrators-file.md)), chooses: the operating system's key store, under an entry named for the installation (`local-` and sixteen hexadecimal characters of the digest of the data directory's path, so that two installations never share a root), or, written `passphrase`, a key derived from the passphrase in the environment variable `A2M_VAULT_PASSPHRASE`, with its salt kept in the data directory. The program copies its environment at start and then removes the passphrase from the process environment, before any thread starts, so that no process it starts can inherit it; an engine it starts on demand is given the passphrase on its standard input instead ([decision 0165](../../decisions/0165-a-client-starts-the-engine-on-demand-detached-and-gives-it-the-passphrase-on-a-pipe.md)); the environment the process was started with stays readable to a program of the same user (`/proc/<pid>/environ` on Linux), as its memory does. A key store that does not answer is not a failure to start: the kernel opens `degraded`, reads go on, nothing is written, and `doctor.run` gives the way out. With the passphrase route and no passphrase the vault never attaches, and every action that needs the store is refused by the vault's own failure, `input`/`vault_passphrase_missing`. A command that has no passphrase to give starts no engine on demand; an engine started on demand (`serve --started-on-demand`) that cannot open its vault writes `stopped_line` of `a2m-local` and ends, so no engine lingers without its vault. A Linux machine has no key store this build can reach, so the passphrase route is the one there, by default.

**The person at the local engine.** `LocalSurface` of `a2m-local`, made from the presence prompt this build has for the machine, the approval window `policy.approval_window_minutes`, and the engine's clock and randomness. It is the only holder of the approval desk. On Linux the prompt is polkit, once an administrator has installed its policy file; on macOS and Windows none is built, so through the program no approval is recorded there and no person's act is carried out by a served engine ([decision 0142](../../decisions/0142-the-presence-prompt-interface-and-the-linux-prompt-as-built.md)). A host that embeds `LocalEngine`, and a test, give their own prompt.

**The person at a one-shot command.** `TerminalSurface`. It drops the approval desk the kernel hands over, so nothing in a one-shot command can record an approval. For an act that is a person's it asks, at the terminal the command was started at, for the word `yes`, and the proof says `not_available`: this is the dialog-only fallback of spec section 222, which stops a mistake and is not a security boundary. The policy engine does not take that proof for loosening a protected setting. With no terminal nobody is asked and the act is refused. The operating system's presence prompt and the approval page belong to `a2m-local`.

## Starting a process

1. **Paths.** `Paths::resolve` from `a2m-actions`. `A2M_HOME` overrides every directory. When the engine opens, its data, configuration, and cache folders are made its user's alone with `keep_folder_private` of `a2m-local`, as the runtime folder is; `doctor.run` names one others can enter ([decision 0241](../../decisions/0241-protected-settings-loosen-only-from-an-administrators-file-and-every-key-says-whether-it-is-built.md)).
2. **Settings.** The loader in `a2m-actions` layers built-in defaults for the deployment shape, the installation's and the user's `settings.toml`, the project's `anything2mcp.toml`, environment variables prefixed `A2M_`, and flags; records which layer set each value; refuses a value that would weaken a guardrail; and returns `Settings`. The installation's file may loosen a protected setting only when no account but an administrator can change it, which `Sources::read` checks on disk; under `A2M_HOME` it never can. A test of the product that stands for an administrator's installation in a temporary folder says so with `EngineOptions::with_installation_kept_by_an_administrator_for_tests`, which no product source calls, a test holds ([decision 0241](../../decisions/0241-protected-settings-loosen-only-from-an-administrators-file-and-every-key-says-whether-it-is-built.md)). Every first-party pack's manifest is given to the loader, because a pack's settings are the schema of its group. Workspace, toolset, and target layers are not read here: they live in the store and in target files.
3. **What clients trust.** Each file named in `network.trust_roots`, a full path inside a folder `files.allowed_roots` allows, read through `Roots::check` of `a2m-provider`, is handed to `tls::install` of `a2m-auth` before any pack registers and before anything makes a client; a file that cannot be used stops the opening with `input`/`trust_roots_unreadable` ([decision 0166](../../decisions/0166-https-in-the-product-trust-roots-at-engine-start-and-one-maker-of-clients.md)). The store and the vault: `Kernel::prepare`, with this machine's key store.
4. **Providers.** `fill`, above, on a builder made from the prepared kernel's redactor and the logs and traces. Plug-ins are not looked for yet: a plug-in comes from three places, built in, a Python entry point in the group `anything2mcp.providers` (`a2m-provider` has the search, `find_python_providers`; this crate does not call it yet), or a mapping file, and each is checked against its manifest by `a2m-provider`.
5. **The kernel.** `Kernel::open` with the registry, its problems, and the person surface; then the in-process runner loop. A kernel that is stopped or degraded runs no job, and says why.
6. **Role.** As the table below. Built: the first two rows, with start on demand; `serve` in the `solo` and `workstation` deployment shapes over the local channel; and `stdio` for MCP. Not built: the server's roles; `runner`; `stdio` for the SDK.

| Command | Role | Opens the kernel | Front doors |
| --- | --- | --- | --- |
| Any action command | Thin client | No | None: a `Connector` to the local engine, which it starts if nothing answers |
| An action command with `--in-process` | One-shot engine | Yes, inside the command, refused if an engine runs | None |
| `serve` in the `solo` or `workstation` deployment shape | Local engine | Yes, with `LocalSurface` | The local channel carrying the REST router and the MCP endpoints; the in-process runner loop. Local HTTP only if switched on in `workstation` |
| `serve` in the `team`, `company`, or `cluster` deployment shape | Control plane, and runners if `profile.roles` includes them | Yes, with `ServerSurface` | The server listener carrying the REST router, `/mcp/<toolset>`, the runner link's service, and the console; the local channel for administration on the host |
| `runner` | Runner | No | None: the outward link of `a2m-runner` |
| `stdio` | Thin client | No | Standard input and output: MCP messages to the local engine's MCP endpoint, or the SDK's JSON messages to its REST API |

`serve` is also what a thin client starts, detached, when it finds no engine. `--in-process` and `engine.in_process` exist for CI and containers that run one command and exit.

### The local engine, as built

`LocalEngine::start` does the steps above in this order: the settings; `Channel::bind`, which takes the engine lock and binds the socket or the pipe; the kernel, opened for the user the operating system reports, with `LocalSurface`; then the one router. A second engine on the same data directory is refused at the bind, before it opens the store. A client that connects while the kernel opens waits for the answer to its hello.

The router is the REST router of `a2m-api` and the MCP endpoint of `a2m-mcp` for each built-in toolset, nested at `/mcp/use` and `/mcp/build`, behind one layer of this crate. The layer does four things and decides nothing:

| What | How |
| --- | --- |
| The caller | The `Peer` the channel put on the request is given to `Kernel::verify_caller`; the caller that comes back goes on the request. A request with no peer is refused. An engine whose store never opened (`engine_degraded`) names nobody and answers its own status; any other refusal is the answer |
| Agent or person | Every caller is an agent. Nothing a caller says of itself, its label or a header, makes it a person for any decision: a person's act asked through the channel waits on a page of `LocalSurface`, answered `needs_person` with the page's link, and raises no prompt until a person presses there ([decision 0163](../../decisions/0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md)), a pre-approval rule that asks for a caller who is no agent is never met, and the decision records an agent. The label names the door, for the audit log. A person is shown only by the presence prompt, raised from a press on an approval page |
| The project folder | The command sends the folder it was run in as `A2M-Project`, percent-encoded by `anything2mcp::wire`, which is public so that a caller written in Rust, and the crate's tests, name a folder as the command does; it goes on the request as the `ProjectFolder` of `a2m-api`. The handlers check it before any use. An MCP request carries none yet |
| `engine.stop` | Sent to the dispatcher, so that the policy engine decides and the decision is recorded. When the dispatcher answers that the action is the client end's, this process answers `{"stopping": true}` with `Connection: close`, and asks its channel to stop only once that answer is handed over whole ([decision 0382](../../decisions/0382-the-engine-stops-once-its-answer-to-engine-stop-is-handed-over-and-a-lost-one-is-no-write-in-doubt.md)) |
| The data directory | Before anything else, `LockPlace::holds` of `a2m-local`: when the lock file of the data directory is gone or another file, the request is answered `busy`/`data_directory_gone`, nothing reaches the kernel, and the channel stops with `StopReason::DataGone`. The idle hold asks the same at each idle look, so an engine whose socket went with its data directory notices at its idle time. `serve` then ends with code 1 and that failure ([decision 0181](../../decisions/0181-the-product-and-the-local-engine-take-up-what-the-kernel-now-offers.md)) |

`serve` writes one line to standard error when it answers, `anything2mcp: the local engine is ready.`, and before it the reason when the engine runs no job, and one line that begins `anything2mcp: approvals on this machine:` with the kind of presence check, the reason, and what to do where nothing can be raised (`PRESENCE_LINE`, `LocalEngine::presence_line`). It stops when `engine.stop` is dispatched, on the interrupt, the termination signal, and the hang-up on Unix and on the console's interrupt, break, close, and shutdown events on Windows, and after `engine.idle_stop_minutes` with no connection, no page waiting for a person, and no run in hand as `Kernel::runs_in_hand` counts them (0 never stops). It listens for the signals from its first step, before the kernel opens, so a signal sent while the kernel opens stops it cleanly as soon as it serves. What asked is recorded once, the first only, by `StopAsk` (`StopBy`: the signal's own name, or `engine.stop` with the process the channel saw), and said at once on standard error, `anything2mcp: the engine was asked to stop by the signal SIGTERM.` It then closes the kernel, lets go of the engine lock, and prints one object, `{"ok": true, "action": "serve", "result": {"stopped": "asked", "by": "SIGTERM"}}`, with exit code 0; for `engine.stop`, `by` is `engine.stop` and `process` is the sender's process where the system names it; an engine that stopped by itself names nothing. Its log carries `engine.started` once it is open (`pid`, `version`, `on_demand`, `by`) and `engine.stopped` before the kernel closes (`why`, and `by` and `process` when it was asked) ([decision 0266](../../decisions/0266-the-engines-log-says-each-start-and-stop-and-what-asked-and-engine-logs-reads-it-where-the-engine-runs.md)).

### The thin client, as built

A command looks for the engine with `Connector`. When one answers, each action is sent to `POST /api/v1/actions/<name>` with its input as the body, the project folder as the header above, and `--idempotency-key` as `Idempotency-Key`. An answer lost after the request may have reached the engine is `possible_commit`/`answer_lost`, `retry_safe: false`, with a fix that says not to run the command again and to find its run first; only a failure before anything was sent (`connection_lost`) is safe to run again. The one exception is `engine.stop`, which writes nothing: when its answer is lost the command looks at the channel once more, and succeeds with `{"stopping": true}` and a warning when nothing answers there, or fails with `busy`/`engine_running`, safe to run again, when an engine still answers ([decision 0382](../../decisions/0382-the-engine-stops-once-its-answer-to-engine-stop-is-handed-over-and-a-lost-one-is-no-write-in-doubt.md)). A body with the channel's own four parts is shown as that failure. The answer's body is the response, and its header `A2M-Exit-Class` gives the exit code; both go to the same renderer as a response made in this process. A shortcut's steps go one after another on the one connection, and the first that is not a success ends them. The client holds no engine logic.

When nothing answers, the command starts the engine on demand and then sends to it, as [decision 0165](../../decisions/0165-a-client-starts-the-engine-on-demand-detached-and-gives-it-the-passphrase-on-a-pipe.md) gives it: `Connector::connect_or_start` of `a2m-local` under the start lock, with a `Launch` this crate makes (`cli/start.rs`): this program's `serve`, with `--project` when the command has one, in the data directory, with the environment cut to a fixed list that holds no credential, its output appended to `engine.log`, and, when the vault takes its key from a passphrase, the passphrase on its standard input for `serve --passphrase-from-stdin`. `engine status` and `engine stop` start nothing. No engine is started, and the command takes the engine lock in the same step and runs the engine in its own process, holding the lock until its kernel has closed, when: `--in-process` or `engine.in_process` asks for that; the vault takes its key from a passphrase the command was not given; `Host::program` is `None`; or the machine refused the start, which the command says on standard error. No `serve` and no second command opens the store meanwhile. When the lock is held and nothing answers, a `serve` is starting or closing, or another command runs the engine inside itself: the command looks again at the channel and the lock for up to twenty seconds, then ends with `busy`, `engine_unreachable`, and exit code 4. `--in-process` skips the engine and is refused, `busy`, `engine_running`, while one runs. In one process the caller is the process itself, and whether a person is at its terminal is what it sees of itself; that is no proof of a person and is never carried over the channel.

### The stdio proxy, as built

`anything2mcp stdio [--toolset use|build]` hands `stdio_proxy` of `a2m-mcp` its standard input and output and a way to the engine: for each message, one connection through `Connector`, kept until the answer's body has been read. Standard output is written by the proxy alone. The stdio proxy never runs the engine inside itself. A request that finds no engine starts one on demand, as a command does, before the request is written, so nothing is sent twice; where none may be started (`engine.in_process` on, or no passphrase for a vault that needs one), the request is answered with error -32000 carrying the failure and the next step, `anything2mcp serve`, and the stdio proxy says so once on standard error when it starts. It ends with code 0 when its input ends.

## The command line

The command tree is built from the action registry each time the program starts. Nothing in it is written by hand except the three process commands, the completion command, and the global flags.

| Part | Form | Comes from |
| --- | --- | --- |
| Action commands | `anything2mcp <noun> <verb>`, such as `anything2mcp target probe` | Every `ActionDef`: the noun and verb, flags from the input schema, one declared positional field |
| Shortcuts | `init`, `add`, `learn`, `lint`, `try`, `test`, `ready`, `publish`, `status`, `call`, `run`, `diff`, `connect`, `doctor`, `docs` | `Shortcut` data in `a2m-actions`: a fixed list of actions, each dispatched in turn; the first that is not a success ends the sequence. The tree holds only the shortcuts whose every step is built (`shortcuts_offered`); `bundle`, `rollback`, and `migrate` are not offered until their actions are |
| Process commands | `serve [--passphrase-from-stdin]`, `stdio [--toolset use\|build]`, and `runner`, which is not built | Here; they start a role and are not actions |
| The completion command | `completion <bash\|zsh\|fish\|powershell>` | Here: the program answers it from the command tree, with no engine and no folder made, and it is not an action ([decision 0266](../../decisions/0266-the-engines-log-says-each-start-and-stop-and-what-asked-and-engine-logs-reads-it-where-the-engine-runs.md)). The script holds every command and flag and each flag's help where the shell shows one, offers no value that lives in the engine, and runs nothing when it completes |
| Global flags | `--json`, `--project`, `--in-process`, `--idempotency-key`, `--help`, `--version` | Here |

A command finds its project by looking for `anything2mcp.toml` in the current directory and then each parent, or takes `--project`. The walk looks only in folders that only the user or the administrator can change, and stops at the first that others can, so a shared folder such as the system's temporary folder is never looked in, nor anything above it; a project file, found or named, that others can change is refused `input`/`path_not_allowed`. Who may change a path is `kept_by` of `a2m-local`: on macOS and Linux the owner is the user or the administrator, nobody else may write, and a group may only when it is the user's primary group. On Windows this build reads no owner, so the walk looks in the working folder, and above it only inside the user's profile folder. It sends the project's path in the `Request`; the engine checks it against the allowed roots before using it.

**The parser** is our own, over the command faces: no outside crate. The rules, all of them:

- A word with one of its verbs is the action. The bare word, or the word with anything else, is the shortcut or the process command of that name when there is one (`run get` is the action `run.get`; `run <tool>` is the shortcut).
- An argument is filled by its flag, `--name value` or `--name=value`, or, for the one positional field, by a word with no flag. A `flag` takes no value; a `list` may be repeated, and a value that reads as a non-empty JSON array of text is its items, a comma being part of an item; the help names both forms and a `next` line writes the repeated one ([decision 0445](../../decisions/0445-the-first-hours-faults-a-list-on-the-command-line-a-key-off-a-list-and-a-project-held-to-one-store.md)); a `map` takes a JSON object; a `value` takes JSON, or text when it is not JSON. After `--` every word is taken as written.
- A global flag may stand anywhere before `--`. `--version` counts only as the first word, because an action may have a field of that name. A test fails if an action's flag is ever one of the others.
- A missing required argument, an unknown flag, a second positional word, and a value of the wrong type are refused by the tree, before any engine is opened, with the usage line as the fix.
- A shortcut dispatches its actions in turn and stops at the first that is not a success; the response printed is the last one dispatched. A shortcut of one action also takes that action's flags; one of several takes its one argument and nothing else. `init` has no action beneath it: the client end writes the project's first files in the folder it was run in, or the one `--project` names, with `init::init_project` of `a2m-actions` (the project file, `AGENTS.md`, `.gitignore`, `targets/`, and the working folder `.anything2mcp/`, each made new and never through a link, a file already there kept), and refuses a folder others can change ([decision 0164](../../decisions/0164-the-authoring-actions-served-through-the-engine.md)). Its help says what it writes, and that the command does it itself with no action beneath it (package 5.6c).
- Help is text built from the same data: `--help` alone lists every noun with its verbs, the shortcuts, the process commands, and the global flags; `<noun> --help` lists a noun's commands; `<noun> <verb> --help` gives the usage line, the summary, a line with the action's effect, who may run it, and whether a person at this machine is asked, and every argument. The help of an action that is not built says so on its first line, with the phase that builds it, and both listings mark each such verb `(not built)`; a client's action that the command line does not carry out (`self.update`) counts as not built. Every line is wrapped to `COLUMNS` when it is a whole number of 40 or more, else to 80 ([decision 0266](../../decisions/0266-the-engines-log-says-each-start-and-stop-and-what-asked-and-engine-logs-reads-it-where-the-engine-runs.md)).

**Output.** Text a system returned is never obeyed by the terminal: text for a terminal and all of standard error write every control character but line end and tab, and every mark that turns the direction of text, as a visible escape such as `\u{1b}`; JSON escapes the same characters as `\u` escapes, the encoder doing those below a space. With `--json`, or whenever standard output is not a terminal, exactly one JSON object on standard output, as [the output contract](../../reference/technical-reference.md#command-output-contract) says: the response, whether it succeeded or failed. At a terminal the result is printed as indented lines, laid out from the same JSON without looking at what it holds, with the next command to run. Warnings, and a failure with its class, code, message, and next step, go to standard error in both forms. What the system itself said, `result.failure.said.text`, follows on a line of its own, `  The system said: "…"`, through `said_line` of `a2m-actions`, never inside the product's sentences; at a terminal the result's lines leave it out, so it is shown once ([decision 0181](../../decisions/0181-the-product-and-the-local-engine-take-up-what-the-kernel-now-offers.md)). No prompt is ever shown when output is not a terminal. A caller whose input and error output are not a terminal is taken to be an agent. The exit code comes from the response's exit class alone:

| Exit class | Code |
| --- | --- |
| Succeeded | 0 |
| Failed | 1 |
| Malformed: the command tree refused the arguments, or the dispatcher refused the input's shape | 2 |
| A person is needed: sign-in, approval, input | 3 |
| The engine could not be reached or started | 4 |

A settings file that does not load, a second `serve`, and a process command that is not built end as failed, 1. Code 4 is the client end's alone: no response carries it. A command ends with it when an engine is there and could not be used: it holds the lock and never answered, it belongs to someone else, it speaks another local API, or it went away during the request.

Of the `engine.*` actions, which act on the process, `engine.stop` is carried out by the engine's process after the dispatcher has decided, and `engine.status` is the kernel's. `engine.start` is carried out in the client end: it starts the engine detached when none answers, whatever `engine.in_process` says, and answers `{"started": true|false, "engine_version": ...}`. `engine.logs` is carried out in the client end, before it looks for an engine: it reads `engine.log` and its rotated files in the data directory with `recent_lines` of `a2m-logs`, starts nothing and makes nothing, and answers `{"file", "exists", "lines"}`, `--lines` 50 when not given and 1 to 1,000 ([decision 0266](../../decisions/0266-the-engines-log-says-each-start-and-stop-and-what-asked-and-engine-logs-reads-it-where-the-engine-runs.md)); through the engine, over MCP or REST, it still answers `input` / `client_action`. With no engine running, `engine.stop` and `engine.status` start none and are answered by the engine the command runs inside itself, which is not what a person means by them.

## What the packages wrap

| Route | What it holds | How `anything2mcp` and `a2m` arrive |
| --- | --- | --- |
| pip | One wheel per platform: this binary and the pure-Python `anything2mcp` package, the SDK. No engine logic in Python ([decision 0025](../../decisions/0025-pip-and-npm-packages-ship-the-binary-with-a-thin-sdk-no-in-process-bindings-yet.md)) | The binary as a script on the path; `a2m` to be settled by research brief 28 |
| npm | A launcher package and one package per platform holding only the binary; the TypeScript SDK; no install script | The launcher's commands start the platform binary |
| Standalone | Installers, Homebrew and winget entries, a container image, an offline bundle | The binary, and a link or copy for `a2m` |

Every first-party pack is compiled in and switched off until enabled ([spec section 605](../../spec/6-running-it/605-packaging-and-release.md)). The SDKs start `stdio` for a local engine, or speak HTTPS to a server; both are generated from the action catalog and the OpenAPI document.

**`a2m` is the same program.** One binary is shipped. On each install route `a2m` is a link to it or the smallest launcher that route supports, and the cost of each in start time is measured against the 30 ms budget of spec section 222. The workspace today builds two small binary targets, `anything2mcp` and `a2m`, from one module for convenience; each is a one-line `main` calling `run`, and the output names the product whatever name started it. The package build is where the link replaces the second target, because two shipped copies would count twice against the 25 MB target of hypothesis H-28.

## Used by, and uses

Nothing uses it; it is the top. It uses `a2m-actions` for settings, `Paths`, the registry, the kernel, and the face descriptions; `a2m-provider` for the registry it fills; each pack crate's registration call; `a2m-vault` for the key stores; and, when they are joined, `a2m-local`, `a2m-mcp`, `a2m-api`, and `a2m-runner` for the roles.

## State

One file of its own: the salt of the passphrase route, `vault-salt` in the data directory, which is not a secret. Settings are read once at start and handed down.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `input` | `schema_invalid` | Malformed arguments; exit code 2 |
| `input` | `action_not_built` | `runner`, or a shortcut with no action beneath it |
| `input` | `schema_invalid` | A word `serve` or `stdio` does not take; a project folder the engine cannot read from the request |
| `busy` | `engine_running` | Passed through from `a2m-local` for a second `serve`; raised here for `--in-process` while an engine runs |
| `policy` | `caller_missing` | A request that reached the local engine's router with no peer of the channel |
| `input` | `settings_invalid` | Passed through from the loader: the settings file or an environment variable does not load, or would weaken a guardrail, with the setting and its layer named |
| `busy` | `key_store_unreachable` | The reason a kernel is degraded when this machine's key store does not answer or no passphrase was given; never raised to a caller by this crate |
| `busy` | `engine_unreachable` | Raised by `a2m-local` when nothing answers, and here for a lock held for the whole wait, by an engine that never answered or by another command that runs the engine inside itself; exit code 4. The stdio proxy passes it to its caller as error -32000 |
| `possible_commit` | `answer_lost` | Raised by `a2m-local`, and here for an answer that cannot be read: the request may have reached the engine; exit code 4, `retry_safe: false` |
| `input` | `path_not_allowed` | A project file, or its folder, that others can change |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No crate but this one names a pack | The audit's layering rules |
| `anything2mcp` and `a2m` behave identically | `both_names_are_the_same_program`, which runs both binaries with the same arguments and compares output and exit code |
| Every command but the process commands is an action or a shortcut, and every action with a command face is a command | `the_command_tree_is_the_action_registry_and_nothing_else`, which reads the binary's own help and compares it with the registry, both ways |
| Non-terminal output is one JSON object | `output_that_is_not_a_terminal_is_one_json_object_for_every_action`, over every action of the registry through the binary |
| The exit code follows the failure | `the_exit_code_follows_the_failure` |
| A pack that is switched off is listed and not registered; a pack with a problem is listed with it | `a_pack_that_is_switched_off_is_listed_and_registers_nothing`, with each pack disabled in turn |
| Nothing a command offers can approve a write | `a_write_rests_is_approved_by_a_person_and_is_written_and_verified_exactly_once`: no command holds the word, and resuming leaves the run resting until a person approves at the approval surface |
| A secret is in no output and in nothing on disk | The two tests of `signed_in_read.rs`; the passphrase, in `verified_write.rs` |
| An unconfigured engine opens no outside connection | Not tested here: the CI test of spec section 605 |
| A command prints and ends the same through the engine and in one process | `two_commands_share_one_engine_a_second_engine_is_refused_and_a_stop_leaves_nothing`: five commands, each run both ways, give the same exit code and the same object |
| One engine per data directory | The same test: a second `serve` and a command with `--in-process` are refused with `engine_running` while one serves; and `a_command_that_runs_the_engine_itself_holds_its_lock_and_no_second_engine_opens_the_store`: while a command runs the engine inside itself, a second command waits and is refused by name, `serve` is refused, and a third runs once the first has gone |
| An answer lost after the request was sent is never said to be safe to repeat | `an_answer_lost_through_the_served_engine_is_a_write_in_doubt_and_never_safe_to_retry`: the engine dies while a write's answer is held back; the command ends `answer_lost`, `retry_safe: false`, and the stand holds one write |
| Nothing a caller says makes it a person | `a_program_that_says_it_is_a_person_at_a_terminal_is_an_agent_and_raises_no_prompt` |
| `init` makes an empty folder a project and writes over nothing | `init_makes_an_empty_folder_a_project_and_keeps_what_is_there`, through the binary |
| An agent's person's act through a served engine waits on its page and is carried out once | `an_agents_publish_waits_on_its_page_and_a_persons_press_publishes_it_once`: the binary asks to publish, gets `needs_person` and the link; a press with the page's value from its origin passes the test's prompt once; the read then answers; the same call and a second press publish nothing more |
| A project file others can change is not taken | `a_project_file_that_others_can_change_is_never_taken_as_a_project` (macOS and Linux) |
| Text a system returned is not obeyed by the terminal | `text_a_system_returned_reaches_a_terminal_as_text_and_is_never_obeyed` |
| What the system said is one marked line apart from the product's words | `what_the_system_said_is_one_marked_line_apart_from_the_products_words`: a newline, an escape sequence, a direction mark, and words shaped like an instruction stay on one line marked as the system's |
| An engine whose data directory is gone fails closed | `an_engine_whose_data_directory_is_removed_refuses_the_next_request_by_name_and_stops` (macOS and Linux): the next command ends `busy`/`data_directory_gone`, `serve` ends with code 1 and the same failure, and the lock is free |
| A stopped engine leaves no process and no socket | The same test: after `engine stop` the process ends with code 0, the runtime directory holds the lock file alone, and the lock is free |
| Standard output of the stdio proxy is protocol messages alone | `a_stdio_proxy_answers_with_the_failure_until_an_engine_runs_and_then_reads_in_both_revisions`: every line is a JSON-RPC message, with no engine and with one; and `the_exit_code_follows_the_failure` for a malformed `stdio` |
| Every action command can be sent to the engine | `every_action_command_has_the_rest_route_a_thin_client_sends_it_to` |
| A client that finds no engine starts one, and two at once start one | `a_command_with_no_engine_starts_one_detached_and_is_answered`, `two_commands_at_once_start_one_engine`, and `a_stdio_proxy_starts_an_engine_and_the_engine_stops_by_itself_when_idle`, through the real program |
| An engine started on demand inherits no credential and none of its caller's handles | `a_command_with_no_engine_starts_one_detached_and_is_answered`: the command's output ends while the engine runs; on Linux the engine's environment and arguments hold no passphrase, and its session and folder are its own |
| With `engine.in_process` on, nothing is started unless asked by name | `with_engine_in_process_on_nothing_is_started_unless_asked_by_name` |
| An engine started on demand stops by itself when idle and lets go of its lock | `a_stdio_proxy_starts_an_engine_and_the_engine_stops_by_itself_when_idle`, with `engine.idle_stop_minutes` at 1; its last object names nothing that asked |
| What asked the engine to stop is named, and a signal never leaves it unclosed | `serve_stop.rs`: `serve_names_the_signal_that_stopped_it` for the interrupt, the termination signal, and the hang-up; `a_signal_while_the_kernel_opens_stops_the_engine_cleanly`; `engine_stop_is_named_with_its_process` (the first two on macOS and Linux); and every `stop` of `start_on_demand.rs` asserts `by: engine.stop` |
| The log says each start and stop and never a call's input; `engine logs` reads it and makes nothing | `engine_log.rs`: `the_log_says_each_start_and_stop_and_never_an_input`, with a marker in three refused calls found in no file under `logs/`; `engine_logs_with_no_home_says_so_and_makes_none` |
| Help says each action's effect, who may run it, whether a person is asked, and whether it is built, and fits the width | `every_actions_help_says_its_effect_who_and_whether_a_person_is_asked`, `the_help_of_an_action_not_built_says_so_on_its_first_line`, `no_help_line_is_wider_than_the_width` (80, 60, 40), over every action of the build |
| A completion script is made with no engine and no home, and runs nothing when it completes | `completion.rs`: a script for each shell against a home that does not exist; the bash script completes a noun and a flag in a real bash; no line of any script runs the program |
| Every HTTP client of the workspace is made by one of two helpers that state their TLS provider | `every_http_client_of_the_workspace_is_made_by_one_of_the_two_helpers` in `a2m-auth` |
| Nothing reached through the channel approves | `a_write_through_the_served_engine_rests_is_approved_on_its_page_and_is_written_once`: resuming through the engine leaves the run resting until the page is answered, in a real browser, and the presence prompt is passed; a press from `Origin: null` or another origin raises nothing |

### The end-to-end proofs

Each goes from target files in a project folder, through lint, publish, and a tool call, and asks the test stand what it holds. They run the command line on an engine opened as the product opens it, with a person surface the test answers for; where a person is not needed, or a process must die, they run the binary itself.

| Milestone | Test | Shows |
| --- | --- | --- |
| 1, the first read | `first_read.rs` | Lint, publish (refused to an agent), search, and the same value read five times through the `http` mode with one trace signature; each with its run row, its events, its recorded decision, and its stored trace |
| 2, the signed-in read | `signed_in_read.rs` | A read behind a bearer token: a named failure before the secret is stored, the read after `secret set`, `needs_login` once the application revokes the token, and the token in no output and no file. A second system echoes the token in its answer, and the result shows it masked |
| 3, the verified write | `verified_write.rs` | A write rests at `needs_approval` with the diff's hash; an agent cannot approve; a person does, and `run resume` writes once, reads back, and records `verified`; resuming again writes nothing. An answer lost after the write was stored, and the real program killed while it waited for one, both end with one write after the run is reconciled |
| 4, anything, proven once | `other_packs.rs` | The same read through the `cli` mode against the test program, with no kernel change |
| The first hour | `first_hour.rs` | From an empty folder, through the command face only, as a thin client of one served engine: `init`, `target add`, the surface declared, the scout, a drafted read linted and tried, its offline test written by hand and passing, the publish carried out by a person on the engine's page, and the published read called and compared with what the test application holds; nothing written. It asserts today's named stops where the flow cannot go on by itself: a target that declares only its surface gives the scout nothing to observe through (`no_binding`), so one read is written by hand first; the scout's try stage does not end; a drafted binding is `draft`, so the developer marks it verified and publishes the next version. 7.5 seconds here, against the target of ten minutes |
| The web pack, page to read | `web_draft.rs` | The probe's `web_snapshot` of `/orders/A-100` at depth 0, drafted through the registry of an engine opened as the product opens it, compiled without the files that hold open questions, and the drafted `example_orders_a_100_line_items_list` run through the `ui` driver in `Posture::Sandbox`; each column equals what the application's control interface holds, and nothing is written ([decision 0183](../../decisions/0183-the-web-pack-drafts-a-read-of-each-table-a-snapshot-names.md)) |
| Every other pack | `other_packs.rs` | One read each through `device`, `stream`, `mcp`, `file`, `sql`, and `ui`, against each pack's stand |

| The local engine | `local_engine.rs`, through the real program | `serve` as a child process on a temporary data directory: two commands at once read through it, holding no passphrase themselves; a second engine is refused; `engine stop` ends it. A read through `stdio`, by raw protocol messages over its standard streams, in 2026-07-28 and in 2025-11-25, and on the `build` toolset's endpoint; a stdio proxy started with no engine answers with the failure and carries the next request once an engine runs |
| Start on demand | `start_on_demand.rs`, through the real program | A command, two commands at once, a stdio proxy, and `engine start`, each with no engine running; the idle stop; `engine.in_process` on |
| The verified write, served | `local_engine.rs`, in the test's process | The same `LocalEngine` that `serve` starts, with the real channel, `LocalSurface`, both routers, and the kernel: a write called through the thin client rests with a link; a real browser (the web pack's browser engine, `A2M_TEST_BROWSER`) opens the page and presses Approve; the prompt is raised once; `run resume` completes; the test application holds one write. It is not run through the binary, because the program ships no presence prompt and nothing was added to it for a test |

Not proven: a pipeline over a sheet; a write through any mode but `http`; the web pack's sign-in; anything of the local engine on Windows or macOS, where the tests are written to run and have not run. CI must show `local_engine.rs` passing there; the named pipe has never carried a request.

**Two unrelated client libraries, 2026-11-17.** A manual proof, not a CI test. The reference client library for Python (`mcp` 2.3.0) and the one for TypeScript (`@modelcontextprotocol/sdk` 1.32.2 and `@modelcontextprotocol/client` 2.4.0) each started `anything2mcp stdio` as a stdio server against a served engine with one published read over the test application. In all seven runs the library connected, listed the eleven tools of the `use` toolset, called `example_order_get` by name, and read the structured result. Four runs asked for 2026-07-28 and three for 2025-11-25. [Research note 18](../../research/18-mcp-server-base.md) has the table.

## Build order

1. Done: the version, under both names.
2. Done: a command that opens the kernel in its own process and prints one read, from target files and through a real pack. Package 1.1.
3. Done: the one-shot engine, which `--in-process` names; `serve` on one machine; the `Connector` for commands; start on demand for commands and the stdio proxy, `engine.in_process`, and `engine.start` ([decision 0165](../../decisions/0165-a-client-starts-the-engine-on-demand-detached-and-gives-it-the-passphrase-on-a-pipe.md)). Not built: the Python client starting an engine. Package 1.4. Done since: `engine.logs` in the client end, the signals and what asked recorded, and the start and stop lines of the log ([decision 0266](../../decisions/0266-the-engines-log-says-each-start-and-stop-and-what-asked-and-engine-logs-reads-it-where-the-engine-runs.md)).
4. Done: the command tree from the registry, the output contract, the exit codes, and `stdio` for MCP. Package 1.5. At a terminal a moment (`at`, `<something>_at`) of the engine's own fields is printed in RFC 3339 form and a scout's report as its summary with the command that prints it whole; JSON is unchanged ([decision 0219](../../decisions/0219-shadow-bindings-reach-the-product-and-the-first-hour-is-mended.md)). Product tests of decision 0219: a shadow binding of an `http` read counted and promoted through the commands (`tests/shadow.rs`), the in-page write through the engine loop (`tests/verified_write.rs`), the answer forms of decision 0217 (`tests/other_forms.rs`), and a first run on Linux with no settings file (`tests/first_run.rs`).
5. Done: `fill` with every first-party pack that is built. Not built: plug-ins; `stdio` for the SDK. Packages F3 and 5.4.
6. Done: shortcuts, as far as the actions beneath them exist, and `init` in the client end. Package 5.2.
7. Not built: the server and runner roles. Packages 6.2 and 7.1.

## Open points

- Settled by [decision 0165](../../decisions/0165-a-client-starts-the-engine-on-demand-detached-and-gives-it-the-passphrase-on-a-pipe.md): start on demand. Settled by [decision 0192](../../decisions/0192-a-served-engine-hands-no-root-of-the-project-that-started-it-and-clients-start-and-compare.md) and [decision 0194](../../decisions/0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md): no engine hands the packs a root of the project that started it; `files` and `project` are resolved for each request. Every thread's panic is reported by its place alone, never its message, so no value a panic carries reaches the started engine's output file ([decision 0194](../../decisions/0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md)). An engine started on demand still takes the starting client's project settings and `<project>` in `network.trust_roots`. Its hello names its settings variables by digest, and a command or stdio proxy whose own differ is told `settings_differ`, by name; the engine is not restarted. `engine.log` is never rotated.
- Settled by [decision 0181](../../decisions/0181-the-product-and-the-local-engine-take-up-what-the-kernel-now-offers.md): TLS is on by default; `--no-default-features` builds without it. Still open: the HTTP and MCP packs send a credential over a plain `http` address off the loopback address, which the data pack refuses.
- Settled by [decision 0163](../../decisions/0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md): a person's act asked through a served engine waits on its page; the press raises the presence prompt, and only its proof carries the act out. A waiting act is an idle hold: `LocalSurface::waiting` counts it with the approval pages.
- Settled by [decision 0142](../../decisions/0142-the-presence-prompt-interface-and-the-linux-prompt-as-built.md): `Channel::serve` hands the engine lock back, and `LocalEngine::serve` drops it after the kernel has closed.
- A call that arrives over MCP carries no project folder, so the build toolset's actions on a project's files answer `path_not_allowed` through the stdio proxy. The folder needs a place that both `a2m-api` and `a2m-mcp` can read, which is `a2m-actions`.
- Settled: a run in hand holds an idle engine open, through `Kernel::runs_in_hand`. A run that rests for a sign-in nobody makes holds it open for as long as it rests.
- The stdio proxy names itself `stdio` on the channel. The MCP client's own name is in the protocol, which the stdio proxy does not read.
- The key given as `--idempotency-key` must be 1 to 200 printable characters through the engine, which is the REST interface's rule; in one process any text is taken.

- Settled by [decision 0164](../../decisions/0164-the-authoring-actions-served-through-the-engine.md): `init` has no action; the client end writes the first files. It chooses no client profile, and its summary no longer says it does (package 5.6c).
- Which link or launcher provides `a2m` on each route: research brief 28.
- Whether the embedding facade publishes the `a2m-*` crates, which spec section 605 leaves to a decision record.
- Whether `--idempotency-key` stays a global flag or becomes a field of the write actions' schemas. Today it is a global flag that fills the request's key.
- A secret given to `secret set` is an argument of the command, which other programs on the machine can see while it runs. A form that reads it from standard input is needed, and the action's input has no way to say so.
- A new engine started within the lease of one that was killed sees that engine's run as `running` until the lease ends, about 100 seconds. The scheduler has no way to be told that a runner's process is gone.
- Settled in `a2m-actions`: a decimal in a tool's result is its decimal text, `"12.5"`, padded by the engine to the places its field type states, `"12.50"` for two ([decision 0444](../../decisions/0444-a-results-decimal-is-padded-to-the-places-its-field-type-states.md)), and the command line prints it as it prints any text. `a_device_is_read_through_the_device_pack` reads one.
