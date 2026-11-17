# `anything2mcp`

The product: the one program, and the only code that names a pack. It reads the settings, fills the provider registry with one call per first-party pack, opens the kernel, and runs a command line that is generated from the action registry apart from a few process commands. `a2m` is the same program under a short name.

**As built today** ([decision 0119](../../decisions/0119-the-product-crate-as-building-it-settled-it.md)): every command runs the engine inside its own process. The local engine, its channel, the stdio proxy, and the server and runner roles are not joined yet, so `serve`, `runner`, and `bridge` are named and answer that they are not built. Nine providers are registered: `web` and `web_probe`, `http`, `mcp`, `command`, `data`, `file`, `stream`, and `device`. The end-to-end proofs of the whole product live in this crate's `tests/`.

## Facts

| | |
| --- | --- |
| Side | Product |
| Layer | 11, Product |
| May depend on | Everything |
| Depends on | `a2m-actions`, `a2m-provider`, `a2m-core`, `a2m-vault`, `a2m-telemetry`, `a2m-policy`, `a2m-auth`, and every pack crate that is built. To come: `a2m-local`, `a2m-mcp`, `a2m-api`, `a2m-runner`, and the desktop pack. Outside: `serde_json` and Tokio. No command-line parser crate: the command tree is data, and a small parser of our own reads it ([decision 0119](../../decisions/0119-the-product-crate-as-building-it-settled-it.md)) |
| Built in | Roadmap package 1.1 (a command that prints one read, in process); 1.4 (`serve` on one machine, commands as thin clients); 1.5 (the command tree from the registry); 5.2 (shortcuts and `init`); 6.2 and 7.1 (server and runner roles) |
| Carries out | Spec sections [226](../../spec/2-kernel/226-configuration.md) for loading, [222](../../spec/2-kernel/222-front-doors-and-the-local-engine.md) for the clients, [220](../../spec/2-kernel/220-actions-and-the-authoring-interface.md) for the command face, [605](../../spec/6-running-it/605-packaging-and-release.md) and [605](../../spec/6-running-it/605-packaging-and-release.md) for what is shipped |

## Owns

- `main` for both program names.
- Reading settings once at start, through the loader in `a2m-actions`, and handing each pack its own group.
- Filling the provider registry: one call per enabled first-party pack, then the plug-ins found on the machine; where a pack has several crates, joining them, as with the web pack's shared browser handle.
- Choosing and starting a role, and joining the role crates to each other: the routers of `a2m-api` and `a2m-mcp` onto the local channel of `a2m-local` or the server listener; the stdio proxy onto a `Link`; the runner link's service onto the server listener.
- The command line: the command tree built from the action registry, the shortcuts, and the process commands `serve`, `runner`, and `bridge`.
- The output contract and the exit codes, applied to every `Response`.
- The library facade for a host that embeds the engine.

It must never: do anything with a pack except join its crates and hand it to the registry; hold a branch on an action's result beyond choosing the exit code from its exit class; define an action or a face by hand; read a setting after start.

## Interface

As built.

```rust
// main.rs and bin/a2m.rs: both call the same function, so the two names behave identically.
pub mod cli {
    pub fn run(args: impl IntoIterator<Item = String>) -> ExitCode;
    /// The same with the process's surroundings as data, for a test.
    pub async fn run_with(args: &[String], host: &Host) -> Finished;
    /// One command line on an engine that is open already.
    pub async fn run_on(engine: &Engine, args: &[String], json: bool, is_agent: bool) -> Finished;
    pub fn usage_lines() -> Result<Vec<String>, Failure>;   // every action command, for the parity test
    pub struct Host { cwd, environment, stdout_is_terminal, person_at_terminal, person, key_store }
    pub struct Finished { code: u8, stdout: String, stderr: String }
}

/// The library facade, for a host that wants the engine in its own process.
pub struct EngineOptions { paths, project, environment, flags, place, key_store, clock, random, os_user, person }
impl EngineOptions { pub fn of_machine(paths: Paths, environment, person: Arc<dyn PersonSurface>) -> Self; }
pub struct Engine { /* the kernel, and the one caller a process on this machine is */ }
impl Engine {
    pub async fn open(options: EngineOptions) -> Result<Engine, Failure>;   // only the settings can fail
    pub fn kernel(&self) -> &Kernel;
    pub fn caller(&self, door: FrontDoor, is_agent: bool) -> Caller;
    pub fn request(&self, action: &str, input: Json, caller: Caller) -> Request;   // in the project folder
    pub async fn dispatch(&self, request: Request) -> Response;
    pub fn runner_problem(&self) -> Option<&Failure>;
    pub async fn close(self);
}
/// The action registry of this build without opening a store: for help and for the command tree.
pub fn action_registry() -> Result<ActionRegistry, Failure>;

/// The person surface of a one-shot command, and the terminal it asks at.
pub struct TerminalSurface; pub trait Terminal; pub struct ProcessTerminal;
pub const PASSPHRASE_VARIABLE: &str = "A2M_VAULT_PASSPHRASE";

// packs.rs, private: the only place a pack is named. One entry per provider: its manifest,
// whose settings are the schema of its group, and the one call that hands it to the builder.
fn first_party() -> Vec<FirstParty>;
fn fill(builder: &mut RegistryBuilder);
```

Joining a pack's own crates is the only other thing this crate does with a pack: the web pack's two crates are given one `SharedBrowsers` handle, made here. Each pack crate exposes one registration call. Whether a pack is enabled is the setting `providers.disabled`, which the registry builder reads: a pack that is switched off is listed as `not_enabled`, registers nothing, cannot be reached, and costs nothing at run time. A pack with a problem is listed with the problem named, as `web_probe` is when `http`, which owns a kind it reads, is switched off.

**What the host gives the packs.** The supervisor, the redactor and telemetry of the prepared kernel, and four named roots: `project` and `files`, both the project folder (the default of `files.allowed_roots`); `command_runs`, a folder under the data directory; and `profiles`, the data directory's own. A root whose folder does not exist is left out, and a pack that needs it learns so when it checks a path.

**The vault's root key on this machine.** `vault.backend`, which only the installation's settings file can set, chooses: the operating system's key store, or, written `passphrase`, a key derived from the passphrase in the environment variable `A2M_VAULT_PASSPHRASE`, with its salt kept in the data directory. A key store that does not answer is not a failure to start: the kernel opens `degraded`, reads go on, nothing is written, and `doctor.run` gives the way out. A Linux machine has no key store this build can reach, so the passphrase route is the one there.

**The person at a one-shot command.** `TerminalSurface`. It drops the approval desk the kernel hands over, so nothing in a one-shot command can record an approval. For an act that is a person's it asks, at the terminal the command was started at, for the word `yes`, and the proof says `not_available`: this is the dialog-only fallback of spec section 222, which stops a mistake and is not a security boundary. The policy engine does not take that proof for loosening a protected setting. With no terminal nobody is asked and the act is refused. The operating system's presence prompt and the approval page belong to `a2m-local`.

## Starting a process

1. **Paths.** `Paths::resolve` from `a2m-actions`. `A2M_HOME` overrides every directory.
2. **Settings.** The loader in `a2m-actions` layers built-in defaults for the profile, the installation's and the user's `settings.toml`, the project's `anything2mcp.toml`, environment variables prefixed `A2M_`, and flags; records which layer set each value; refuses a value that would weaken a guardrail; and returns `Settings`. Every first-party pack's manifest is given to the loader, because a pack's settings are the schema of its group. Workspace, toolset, and target layers are not read here: they live in the store and in target files.
3. **The store and the vault.** `Kernel::prepare`, with this machine's key store.
4. **Providers.** `fill`, above, on a builder made from the prepared kernel's redactor and telemetry. Plug-ins are not looked for yet: a plug-in comes from three places, built in, a Python entry point in the group the pip package reads, or a mapping file, and each is checked against its manifest by `a2m-provider`.
5. **The kernel.** `Kernel::open` with the registry, its problems, and the person surface; then the in-process runner loop. A kernel that is stopped or degraded runs no job, and says why.
6. **Role.** As the table below. Today only the second row is built, and it is what every action command does, with or without `--in-process`.

| Command | Role | Opens the kernel | Front doors |
| --- | --- | --- | --- |
| Any action command | Thin client | No | None: a `Connector` to the local engine, which it starts if nothing answers |
| An action command with `--in-process` | One-shot engine | Yes, inside the command, refused if an engine runs | None |
| `serve` in the `solo` or `workstation` profile | Local engine | Yes, with `LocalSurface` | The local channel carrying the REST router and the MCP endpoints; the in-process runner loop. Local HTTP only if switched on in `workstation` |
| `serve` in the `team`, `company`, or `cluster` profile | Control plane, and runners if `profile.roles` includes them | Yes, with `ServerSurface` | The server listener carrying the REST router, `/mcp/<toolset>`, the runner link's service, and the console; the local channel for administration on the host |
| `runner` | Runner | No | None: the outward link of `a2m-runner` |
| `bridge` | Thin client | No | Standard input and output: MCP messages to the local engine's MCP endpoint, or the SDK's JSON messages to its REST API |

`serve` is also what a thin client starts, detached, when it finds no engine. `--in-process` exists for CI and containers that run one command and exit.

## The command line

The command tree is built from the action registry each time the program starts. Nothing in it is written by hand except the three process commands and the global flags.

| Part | Form | Comes from |
| --- | --- | --- |
| Action commands | `anything2mcp <noun> <verb>`, such as `anything2mcp target probe` | Every `ActionDef`: the noun and verb, flags from the input schema, one declared positional field |
| Shortcuts | `init`, `add`, `learn`, `lint`, `try`, `test`, `publish`, `status`, `call`, `run`, `diff`, `bundle`, `rollback`, `migrate`, `connect`, `doctor`, `docs` | `Shortcut` data in `a2m-actions`: a fixed list of actions, each dispatched in turn; the first that is not a success ends the sequence |
| Process commands | `serve`, `runner`, `bridge` | Here; they start a role and are not actions |
| Global flags | `--json`, `--project`, `--in-process`, `--idempotency-key`, `--help`, `--version` | Here |

A command finds its project by looking for `anything2mcp.toml` in the current directory and then each parent, or takes `--project`. It sends the project's path in the `Request`; the engine checks it against the allowed roots before using it.

**The parser** is our own, over the command faces: no outside crate. The rules, all of them:

- A word with one of its verbs is the action. The bare word, or the word with anything else, is the shortcut or the process command of that name when there is one (`run get` is the action `run.get`; `run <tool>` is the shortcut).
- An argument is filled by its flag, `--name value` or `--name=value`, or, for the one positional field, by a word with no flag. A `flag` takes no value; a `list` may be repeated; a `map` takes a JSON object; a `value` takes JSON, or text when it is not JSON. After `--` every word is taken as written.
- A global flag may stand anywhere before `--`. `--version` counts only as the first word, because an action may have a field of that name. A test fails if an action's flag is ever one of the others.
- A missing required argument, an unknown flag, a second positional word, and a value of the wrong type are refused by the tree, before any engine is opened, with the usage line as the fix.
- A shortcut dispatches its actions in turn and stops at the first that is not a success; the response printed is the last one dispatched. A shortcut of one action also takes that action's flags; one of several takes its one argument and nothing else. `init` has no action beneath it and says what to do instead.
- Help is text built from the same data: `--help` alone lists every noun with its verbs, the shortcuts, the process commands, and the global flags; `<noun> --help` lists a noun's commands; `<noun> <verb> --help` gives the usage line, the summary, and every argument.

**Output.** With `--json`, or whenever standard output is not a terminal, exactly one JSON object on standard output, as [the output contract](../../reference/technical-reference.md#command-output-contract) says: the response, whether it succeeded or failed. At a terminal the result is printed as indented lines, laid out from the same JSON without looking at what it holds, with the next command to run. Warnings, and a failure with its class, code, message, and next step, go to standard error in both forms. No prompt is ever shown when output is not a terminal. A caller whose input and error output are not a terminal is taken to be an agent. The exit code comes from the response's exit class alone:

| Exit class | Code |
| --- | --- |
| Succeeded | 0 |
| Failed | 1 |
| Malformed: the command tree refused the arguments, or the dispatcher refused the input's shape | 2 |
| A person is needed: sign-in, approval, input | 3 |
| The engine could not be reached or started | 4 |

A settings file that does not load, and a process command that is not built, end as failed, 1. Code 4 is not raised in this build: there is no engine to reach.

The `engine.*` actions are carried out in the client end of `a2m-local`, because they act on the process. Until that crate is joined they answer `input` / `client_action`, except `engine.status`, which the kernel answers.

## What the packages wrap

| Route | What it holds | How `anything2mcp` and `a2m` arrive |
| --- | --- | --- |
| pip | One wheel per platform: this binary and the pure-Python `anything2mcp` package, the SDK. No engine logic in Python ([decision 0025](../../decisions/0025-pip-and-npm-packages-ship-the-binary-with-a-thin-sdk-no-in-process-bindings-yet.md)) | The binary as a script on the path; `a2m` to be settled by research brief 28 |
| npm | A launcher package and one package per platform holding only the binary; the TypeScript SDK; no install script | The launcher's commands start the platform binary |
| crates.io | This crate | `cargo install anything2mcp` builds both small targets from the one module |
| Standalone | Installers, Homebrew and winget entries, a container image, an offline bundle | The binary, and a link or copy for `a2m` |

Every first-party pack is compiled in and switched off until enabled ([spec section 605](../../spec/6-running-it/605-packaging-and-release.md)). The SDKs start `bridge` for a local engine, or speak HTTPS to a server; both are generated from the recorded catalog and the OpenAPI document.

**`a2m` is the same program.** One binary is shipped. On each install route `a2m` is a link to it or the smallest launcher that route supports, and the cost of each in start time is measured against the 30 ms budget of spec section 222. The workspace today builds two small binary targets, `anything2mcp` and `a2m`, from one module for convenience; each is a one-line `main` calling `run`, and the output names the product whatever name started it. The package build is where the link replaces the second target, because two shipped copies would count twice against the 25 MB target of hypothesis H-28.

## Used by, and uses

Nothing uses it; it is the top. It uses `a2m-actions` for settings, `Paths`, the registry, the kernel, and the face descriptions; `a2m-provider` for the registry it fills; each pack crate's registration call; `a2m-vault` for the key stores; and, when they are joined, `a2m-local`, `a2m-mcp`, `a2m-api`, and `a2m-runner` for the roles.

## State

One file of its own: the salt of the passphrase route, `vault-salt` in the data directory, which is not a secret. Settings are read once at start and handed down.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `input` | `schema_invalid` | Malformed arguments; exit code 2 |
| `input` | `action_not_built` | A process command, or a shortcut with no action beneath it |
| `input` | `settings_invalid` | Passed through from the loader: the settings file or an environment variable does not load, or would weaken a guardrail, with the setting and its layer named |
| `busy` | `key_store_unreachable` | The reason a kernel is degraded when this machine's key store does not answer or no passphrase was given; never raised to a caller by this crate |
| `busy` | `engine_unreachable` (new) | Raised by `a2m-local`; exit code 4. Not raised yet |

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

### The end-to-end proofs

Each goes from target files in a project folder, through lint, publish, and a tool call, and asks the test stand what it holds. They run the command line on an engine opened as the product opens it, with a person surface the test answers for; where a person is not needed, or a process must die, they run the binary itself.

| Milestone | Test | Shows |
| --- | --- | --- |
| 1, the first read | `first_read.rs` | Lint, publish (refused to an agent), search, and the same value read five times through the `http` mode with one trace signature; each with its run row, its events, its recorded decision, and its stored trace |
| 2, the signed-in read | `signed_in_read.rs` | A read behind a bearer token: a named failure before the secret is stored, the read after `secret set`, `needs_login` once the application revokes the token, and the token in no output and no file. A second system echoes the token in its answer, and the result shows it masked |
| 3, the verified write | `verified_write.rs` | A write rests at `needs_approval` with the diff's hash; an agent cannot approve; a person does, and `run resume` writes once, reads back, and records `verified`; resuming again writes nothing. An answer lost after the write was stored, and the real program killed while it waited for one, both end with one write after the run is reconciled |
| 4, anything, proven once | `other_packs.rs` | The same read through the `cli` mode against the test program, with no kernel change |
| Every other pack | `other_packs.rs` | One read each through `device`, `stream`, `mcp`, `file`, `sql`, and `ui`, against each pack's stand |

Not proven: a pipeline over a sheet; a write through any mode but `http`; the web pack's sign-in.

## Build order

1. Done: the version, under both names.
2. Done: a command that opens the kernel in its own process and prints one read, from target files and through a real pack. Package 1.1.
3. Done: the one-shot engine, which `--in-process` names and every command uses. Not built: `serve` on one machine and the `Connector` for commands, which wait for `a2m-local`. Package 1.4.
4. Done: the command tree from the registry, the output contract, the exit codes. Not built: `bridge` for MCP. Package 1.5.
5. Done: `fill` with every first-party pack that is built. Not built: plug-ins; `bridge` for the SDK. Packages F3 and 5.4.
6. Done: shortcuts, as far as the actions beneath them exist. Not built: `init`, which has no action. Package 5.2.
7. Not built: the server and runner roles. Packages 6.2 and 7.1.

## Open points

- `init` is a shortcut with no action behind it in the catalog; it needs one, for a project and its guidance file.
- Which link or launcher provides `a2m` on each route: research brief 28.
- Whether the embedding facade publishes the `a2m-*` crates, which spec section 605 leaves to a decision record.
- Whether `--idempotency-key` stays a global flag or becomes a field of the write actions' schemas. Today it is a global flag that fills the request's key.
- A secret given to `secret set` is an argument of the command, which other programs on the machine can see while it runs. A form that reads it from standard input is needed, and the action's input has no way to say so.
- A new engine started within the lease of one that was killed sees that engine's run as `running` until the lease ends, about 100 seconds. The scheduler has no way to be told that a runner's process is gone.
- A decimal in a tool's result is printed as `{"decimal": "12.5"}`, the form values are stored in. Whether a caller should see the number as text instead is a question for the result's shape in `a2m-actions`.
