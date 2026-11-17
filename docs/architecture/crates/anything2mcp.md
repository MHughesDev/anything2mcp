# `anything2mcp`

The product: the one program, and the only code that names a pack. It reads the settings, fills the provider registry with one call per enabled pack and then the plug-ins, builds the action registry, and starts a role. It also holds the command line, which is generated from the action registry apart from a few process commands. `a2m` is the same program under a short name.

## Facts

| | |
| --- | --- |
| Side | Product |
| Layer | 11, Product |
| May depend on | Everything |
| Depends on | Today, nothing internal. To come: every kernel crate and every pack crate. Outside: a command-line parser, `clap` as research note 9 suggests, to be verified; Tokio |
| Built in | Roadmap package 1.1 (a command that prints one read, in process); 1.4 (`serve` on one machine, commands as thin clients); 1.5 (the command tree from the registry); 5.2 (shortcuts and `init`); 6.2 and 7.1 (server and runner roles). Today it reports its version |
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

```rust
// main.rs and bin/a2m.rs: both call the same function, so the two names behave identically.
pub fn run(args: impl IntoIterator<Item = String>) -> ExitCode;

/// The only place a pack is named. One line per first-party pack; the setting decides.
fn fill(settings: &Settings, providers: &mut ProviderRegistry) -> Result<(), Failure> {
    if settings.pack_enabled("web") {
        // The web pack has several crates. Its shared browser handle is built once here and
        // handed out: the full handle to the interface crate, the observing half to the probe.
        let browser = web_browser::Handle::new(settings.group("web"))?;
        web_ui::register(providers, browser.clone(), settings.group("web"))?;
        web_probe::register(providers, browser.observer(), settings.group("web"))?;
    }
    if settings.pack_enabled("http")    { http_pack::register(providers, settings.group("http"))?; }
    // ... one line each for the mcp and desktop packs, and each pack added later
    providers.add_plugins(PluginSources::found(&settings.paths))?;   // checked against manifests
    Ok(())
}

/// The library facade, for a host that wants the engine in its own process.
pub mod embed {
    pub async fn open(settings: Settings) -> Result<Engine, Failure>;   // Kernel::open, no channel
    impl Engine { pub async fn dispatch(&self, request: Request) -> Response; }
}
```

In the sketch `web_browser`, `web_ui`, `web_probe`, and `http_pack` stand for `a2m-pack-web-browser`, `a2m-pack-web-ui`, `a2m-pack-web-probe`, and `a2m-pack-http`. Joining a pack's own crates, as for the web pack's browser handle, is the only other thing this crate does with a pack; each pack crate exposes one register function that takes the registry and its own settings group and registers its manifest and its six kinds of thing ([spec section 202](../../spec/2-kernel/202-kernel-and-packs.md)). A pack that is switched off is never registered, so it cannot be reached and costs nothing at run time.

## Starting a process

1. **Settings.** The loader in `a2m-actions` layers built-in defaults for the profile, the system file `anything2mcp.toml`, environment variables prefixed `A2M_`, and flags; records which layer set each value; refuses a value that would weaken a guardrail; and returns `Settings`. Workspace, toolset, and target layers are not read here: they live in the store and in target files.
2. **Paths.** `Paths::resolve` from `a2m-actions`. `A2M_HOME` overrides every directory.
3. **Providers.** `fill`, above. A plug-in comes from three places: built in, a Python entry point in the group the pip package reads, or a mapping file. Each is checked against its manifest by `a2m-provider`; a protected one stays off until a person enables it.
4. **Actions.** `Registry::build` with the filled provider registry.
5. **Role.** As the table below.

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
| Global flags | `--json`, `--project`, `--in-process`, `--idempotency-key`, `--version` | Here |

A command finds its project by looking for `anything2mcp.toml` in the current directory and then each parent, or takes `--project`. It sends the project's path in the `Request`; the engine checks it against the allowed roots before using it.

**Output.** With `--json`, or whenever standard output is not a terminal, exactly one JSON object on standard output, as [the output contract](../../reference/technical-reference.md#command-output-contract) says; text for people goes to standard error. No prompt is ever shown when output is not a terminal. The exit code comes from the response's exit class alone:

| Exit class | Code |
| --- | --- |
| Succeeded | 0 |
| Failed | 1 |
| Malformed: the command tree refused the arguments, or the dispatcher refused the input's shape | 2 |
| A person is needed: sign-in, approval, input | 3 |
| The engine could not be reached or started | 4 |

The `engine.*` actions are carried out in the client end of `a2m-local`, because they act on the process.

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

Nothing uses it; it is the top. It uses every crate: `a2m-actions` for settings, `Paths`, the registry, and `Kernel::open`; `a2m-provider` for the registry it fills; each pack crate's register function; `a2m-local`, `a2m-mcp`, `a2m-api`, and `a2m-runner` for the roles.

## State

None of its own. Settings are read once at start and handed down.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `input` | `schema_invalid` | Malformed arguments; exit code 2 |
| `input` | `settings_invalid` (new) | The settings file or an environment variable does not load, or would weaken a guardrail, with the setting and its layer named |
| `busy` | `engine_unreachable` (new) | Raised by `a2m-local`; exit code 4 |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No crate but this one names a pack | The audit's layering rules |
| `anything2mcp` and `a2m` behave identically | A test that runs both with the same arguments and compares output |
| Every command but the process commands is an action or a shortcut | The parity test in `a2m-actions`, over the command tree |
| Non-terminal output is one JSON object | A test over every action with output redirected |
| A pack that is switched off is not in the registry | A test with each pack disabled in turn |
| An unconfigured engine opens no outside connection | The CI test of spec section 605 |

## Build order

1. Done: the version, under both names.
2. An in-process command that runs a hand-written plan and prints one read. Package 1.1.
3. `serve` on one machine, the `Connector` for commands, `--in-process`. Package 1.4.
4. The command tree from the registry, the output contract, the exit codes; `bridge` for MCP. Package 1.5.
5. `fill` with plug-ins; `bridge` for the SDK. Packages F3 and 5.4.
6. Shortcuts and `init`. Package 5.2.
7. The server and runner roles. Packages 6.2 and 7.1.

## Open points

- `init` is a shortcut with no action behind it in the catalog; it needs one, for a project and its guidance file.
- Which link or launcher provides `a2m` on each route: research brief 28.
- Whether the embedding facade publishes the `a2m-*` crates, which spec section 605 leaves to a decision record.
- The global flag names, and whether `--idempotency-key` is a global flag or a field of the write actions' schemas.
