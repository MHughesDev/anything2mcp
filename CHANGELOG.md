# Changelog

Everything a user of Anything2MCP would notice, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html) as [RELEASING.md](RELEASING.md) applies it.

The design's own history is in the [decision records](docs/decisions/README.md), and the work session by session is in the [journal](docs/journal/README.md).

## [Unreleased]

Nothing usable yet. The project is in its foundation phase.

### Added

- A Cargo workspace of 29 crates: a neutral kernel (`a2m-*`), packs that each know one kind of system (`a2m-pack-*`), and the product crate.
- The `anything2mcp` command, and `a2m` as a short name for it. Both report the version and nothing else.
- The kernel's shared vocabulary in `a2m-core`: archetypes, effect classes, hazards, failure classes, evidence levels, and the states of runs, records, sessions, and bindings, with a secret type that cannot be printed or serialized.
- Findings in `a2m-core`: what discovery returns, with a subject that can be told apart from any other, a level that cannot exceed what its source supports, and an identifier that is a digest of its content.
- Expressions in target files: a typed subset of CEL with no floating-point numbers. A decimal is written `decimal("1.5")` and takes `+`, `-`, `*`, and comparisons; division is `div(a, b, places, rounding)`.
- The vault: saved state is sealed with an authenticated cipher under a key held in the operating system's key store, with a passphrase route for machines that have none. On Linux this build has no operating-system key store and uses the passphrase route ([decision 0123](docs/decisions/0123-the-linux-secret-service-back-end-is-left-out-until-its-store-crate-is-maintained.md)).
- The redactor masks registered secrets in their encoded forms as well as their plain form.
- Target files load, lint, and compile to a hashed plan. The `example` target shows every file kind the kernel owns.
- Source readers for `csv`, `json`, and `xlsx`, with exact decimals and template fingerprints: an input whose structure matches no known template version is refused.
- The policy engine: nothing is allowed unless a rule allows it, a write needs a person's approval unless a pre-approval rule covers it, and a kill switch stops everything, a workspace, a target, or an operation.
- Sessions: one renewer for each session, and built-in sign-in providers for keys, tokens, basic and digest credentials, client certificates, and OAuth.
- A test application with a web interface, a JSON interface, a control interface, and injectable faults.
- The engine loop: a tool runs as a read, or as a verified write that records its intent first and reads the value back. A resume reads before it writes, so a write whose answer was lost is never sent twice.
- The action registry: the first 26 of its actions are served, through one dispatcher that records a policy decision for every call. Settings load in layers, with presets; an unknown key is refused, and no layer above the installation can loosen a protected setting.
- The REST interface: every action at one route, five resource aliases, and a generated OpenAPI 3.1 document.
- The HTTP pack: tools over a JSON interface, with verified writes. An OpenAPI description can be imported to draft endpoint entries. HTTPS is in a build with the feature `tls` only.
- The command pack: a program can be a target. Arguments are typed, and no shell is used.
- The data pack: the `sql` mode over an embedded database file.
- The file pack: named places in delimited text, JSON, and workbooks. A write replaces the whole file in one step.
- The stream pack: bounded reads, a look that takes nothing, and keyed publishes. No standard broker protocol is in this build yet.
- The device pack: points over a Modbus TCP link, with ranges enforced before a frame is sent.
- The MCP pack: an upstream MCP server is reached through a mapping file, and a changed upstream tool is detected as drift.
- The browser engine: it starts an installed Chrome, Edge, or Chromium over the debugging pipe, locates elements by role and name, and has an observing tab that cannot write.
- Continuous checks on Windows, macOS, and Linux: formatting, lints, build, tests, a closed list of dependency licences, and a documentation audit.
- A release workflow that builds the pip package repeatably and publishes it with short-lived credentials, from a version tag or a manual start. It now builds eight platform wheels and a source distribution, checks all nine together, and publishes them through one gated job. It has never run.
- `anything2mcp serve` starts the local engine. A second engine on the same data directory is refused by name before it opens the store. A command is a thin client of a running engine, and runs the engine inside itself when none runs; `--in-process` asks for the second way and is refused while an engine runs ([decision 0131](docs/decisions/0131-the-front-doors-joined-serve-the-thin-client-and-the-bridge.md)).
- `anything2mcp bridge` carries MCP over standard input and output to a running engine. While none runs it answers each request with the failure and the next step, and carries the next request as soon as one does. The Python and TypeScript reference client libraries connected through it in both protocol revisions.
- `engine.stop`, the terminal's interrupt, and an idle engine with nothing waiting all stop `serve`.
- The pip package is a wheel for each of eight platforms, holding the program, the short name `a2m`, and a small Python client, and the npm package is a launcher with eight platform packages ([decision 0135](docs/decisions/0135-the-wheel-carries-the-binary-and-a-thin-client-and-npm-uses-platform-packages.md)). Both are built and tested on Linux; neither is published.
- On Linux the approval of a write asks the operating system's own prompt, through polkit, after the person opens the approval page. `serve` says in one line what approvals need on this machine, and the approval page shows the stored change it will approve. Nothing can be approved on macOS or Windows until the prompts below are run there ([decision 0142](docs/decisions/0142-the-presence-prompt-interface-and-the-linux-prompt-as-built.md)).
- An engine with a run waiting for a person, or for a runner, is held open when idle.
- Approvals can use Touch ID or the account password on macOS, and Windows Hello on Windows. They are written and not yet run on those systems ([decision 0146](docs/decisions/0146-the-macos-and-windows-presence-prompts-as-built.md)).
- The Python client has a typed method for every action, the npm package has a Node client with the same methods, the Python client runs on Windows over the engine's pipe, and `a2m.cmd` is the short command on Windows ([decision 0156](docs/decisions/0156-the-clients-are-generated-from-the-served-document-and-reach-windows-by-the-pipe.md)). The Windows parts are not yet run.
- The HTTP and MCP packs reach HTTPS surfaces in a build with the feature `tls`, which is off by default, and name a failed certificate or handshake (`certificate_untrusted`, `certificate_name_mismatch`, `certificate_expired`, `certificate_not_verified`, `tls_handshake_failed`) ([decision 0157](docs/decisions/0157-tls-is-switched-on-with-the-provider-stated-in-each-client.md)).
- `secret set --stdin` reads the secret from standard input, so it is in no argument and no output.
- A pipeline's source file is given to `tool.call` as a path under the project's `sources/` folder, or as rows, and is stored with the run so that a resume reads the bytes it started with.
- A run's events can be followed over REST, as a page or as a stream, at `GET /api/v1/runs/{run_id}/events`, and an MCP caller that asks for progress gets one message for each step the run takes. The result of a call carries the following call as `next` in its structured content.
- `tool.search` can list tools in full, twenty to a page.
- A device's serial can be declared for each environment, and a write to a device that has none declared is refused.
- The design: a specification of 59 sections in seven parts, 144 decision records, 21 research notes, and a register of 40 hypotheses.
- The design of configuration: eight layers, twelve scopes, presets, a group of settings for every pack, and the rules that no setting can loosen.
- The bar: what being the best of its kind means, as twelve things that can be measured, with a scorecard.
- The architecture: which crate defines, implements, and joins every interface, with a design for each of the 29 crates, the command, data, file, stream, and device packs among them.
- A roadmap of every work package to version 1.0.

### Changed

- Target files: `auth.yaml` gains `settings` and `probe_inputs`, a web surface's viewport is a field of the surface, and a target's time zone is `UTC` or a fixed offset.
- The specification is rebuilt around the product as it is: a neutral kernel and packs. Sections written for the first design were consolidated, and the originals are kept in `docs/archive/`.

- The project is named Anything2MCP. It was first designed as Web2MCP, a tool for wrapping applications; it is now a toolkit for any kind of system, built on the same core. Crates, the command, the packages, and the built-in tool prefix were renamed to match.
- The licence is MIT. Earlier work in this repository was marked Apache-2.0.
- A decline of an approval ends the run as `expired`, with `policy` and `approval_refused`, and nobody is asked again ([decision 0132](docs/decisions/0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md)).
- A tool's result shows an exact number as its exact decimal text, a calendar day as text, and a moment or a length of time as whole milliseconds, where it showed an internal tagged form.
- A kill switch is refused as `policy`, `kill_switch_on`, wherever it is met, and no longer as `busy`.
- A folder is a project only if it holds `anything2mcp.toml`, and an action on a project's files accepts only a folder inside the allowed roots that is none of the engine's own.
- A shortcut that reaches an action that is not built ends with `input`, `action_not_built` and exit code 1, where it ended with exit code 0.
- A browser that starts slowly is `transient`, `timeout`, and the start may take 120 seconds by default ([decision 0134](docs/decisions/0134-one-browser-starts-at-a-time-and-a-silent-start-is-slow-not-foreign.md)). One browser starts at a time.
- The device pack sends at most four frames a second by default, where it sent twenty, and at most 100 frames in a run.
- The presets `cautious` and `ci`, and a setting file that tightens a policy limit, stop the engine with the setting's name until the policy engine is opened with the installation's settings ([decision 0152](docs/decisions/0152-the-action-registry-and-telemetry-after-their-safety-audit.md)).

### Safety fixes

A safety review read each crate against its design and the ten rules, and every finding was confirmed with a test. These are the ones a user would see.

- The vault: a passphrase of fewer than 16 bytes is refused (`vault_passphrase_too_short`). Two engines starting together on a fresh installation make one root key. A session that was revoked cannot be brought back by a restore that arrives late ([decision 0138](docs/decisions/0138-the-vault-and-the-session-keeper-after-their-safety-audit.md)).
- The redactor finds a secret across the pieces of a stream, in hexadecimal in either letter case, in base64 with line breaks, in character references, and as a number. A number that holds a secret is shown as `[redacted]`.
- The engine: a write that was sent stays in doubt until it is read back, whatever failed after it, and is reconciled and never sent again. A value that holds the redaction mark is never sent (`argument_masked`), and a write whose values hold a registered secret is refused. The lease is checked at the commit point ([decision 0147](docs/decisions/0147-the-engine-after-its-safety-audit.md)).
- Runs: a run is its person's alone, and anyone else is told there is no such run. A run whose stored call cannot be read is not run (`run_call_unreadable`). A proof of presence older than two minutes is refused.
- The provider contract: a file is written under a random name, only where nothing stands, and never through a link. A program's digest is checked at every start, and on Linux the file that was checked is the file started. A pack's manifest may not name a program by a relative path.
- The HTTP pack: a write answered by a redirect to another origin, by an answer too large to read, or by one that cannot be read is in doubt and reconciled, and was settled as unsent before. A write that sends a version is refused when the version was not read (`stale`, `version_not_read`). An API description is read within bounds, and one past them is refused (`description_too_large`). A body that is not JSON is `drift` ([decision 0139](docs/decisions/0139-a-write-that-was-sent-is-never-settled-by-an-answer-that-is-not-the-applications-verdict.md)).
- The command pack: a program a group can write, or reached through a folder or link another user can write, is refused. A value that begins with `@`, and every form of a flag that runs its value, are refused. The new lint rules `command.secret_variable`, `command.secret_literal`, and `command.secret_argument` refuse a credential written into a command file ([decision 0140](docs/decisions/0140-the-command-pack-after-its-safety-audit.md)).
- The data pack: a statement is one plain read or one keyed write, and `ATTACH`, `PRAGMA`, `VACUUM`, and the like are refused. A read-back must be written `effect: read` with `form: table`; a target whose read-back was a hand-written read does not compile until it is. A write that cannot know what it reaches stops ([decision 0141](docs/decisions/0141-the-data-pack-holds-a-statement-to-one-read-or-one-keyed-write-and-a-connection-to-one-file.md)).
- The file pack: a document that a binding writes must state `may_be_open`. A text value for delimited text that begins with `=`, `+`, `-`, or `@` is refused (`value_begins_formula`) unless its field says `may_begin_formula: true`. A link at a document's name is refused, and a file left by a killed save is never used or removed ([decision 0149](docs/decisions/0149-the-file-pack-after-its-safety-audit.md)).
- The stream pack: a take records the whole message before it acknowledges. An empty look is "absent" only where the log was fully covered. A refusal in words the protocol does not define leaves a write in doubt. A field of text from a message must be marked untrusted, or the target does not compile (`stream.untrusted_unmarked`) ([decision 0150](docs/decisions/0150-a-take-records-its-message-before-it-acknowledges-and-an-answer-with-no-rule-leaves-a-write-in-doubt.md)).
- The device pack: a device that reports another serial than the one declared is refused (`device_identity_mismatch`), a write frame is recorded before it is sent, and a run past its frame ceiling is refused (`frame_budget_exceeded`) ([decision 0137](docs/decisions/0137-a-device-is-held-to-its-serial-a-write-frame-is-recorded-before-it-is-sent-and-a-run-has-a-frame-ceiling.md)).
- The MCP pack: a tool that the mapping says runs free text is never called, and a tool whose whole input is one string and that nobody reviewed is refused. The tool list is read again before every write. A member of a result that the mapping does not list is not passed on, and an output that is text must be marked untrusted ([decision 0136](docs/decisions/0136-the-mcp-pack-refuses-free-text-reads-its-list-again-before-a-write-and-passes-on-only-listed-members.md)).
- The browser engine: a tab stays on the origins its plan declares, a move a page starts in an observing tab is ended, every download is denied, a window a page opens is closed, the profile folder is made private to the user, and a password field is masked in the accessibility tree ([decision 0143](docs/decisions/0143-the-browser-engine-after-its-safety-audit.md)).
- The compiler and the plan: an operation that writes must declare a commit point, a draft never writes, a code list in which one text stands for two choices does not compile, and a plan that breaks a rule of a write is refused when it is read ([decision 0153](docs/decisions/0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md)).
- The browser engine refuses every permission a page asks for, and reports a download or a pop-up it refused ([decision 0143](docs/decisions/0143-the-browser-engine-after-its-safety-audit.md)).
- The `ui` driver does not click a write over a state the page showed before the click, and a `submit` through `ui` is `irreversible`. Discovery never visits an address that looks like an action ([decision 0144](docs/decisions/0144-the-web-drivers-and-discoverers-after-their-safety-audit.md)).
- An approval is checked whole when a write begins, the `cautious` and `ci` presets are in force, and an approval's window is at most one day ([decision 0145](docs/decisions/0145-only-the-policy-engine-clears-a-write-and-the-ledger-reads-the-whole-clearance.md)).
- An operation that writes can no longer compile as a read, in any pack ([decision 0154](docs/decisions/0154-the-http-command-and-mcp-packs-state-what-saves-what-writes-and-which-key-is-sent.md), [0155](docs/decisions/0155-four-packs-state-what-saves-what-writes-and-when-their-guard-lifts.md)).
- Telemetry: a trace that was altered is reported as broken and gives no signature, and a log file is private to the user on Linux and macOS.
- The browser engine: a closed tab is taken off the browser's count every time, so a browser with no tab open reads idle. A closed tab could stay counted, its page left open, when two threads heard its end in one order.
- A provider's process group whose leader is still exiting is ended without a false report that it could not be; macOS showed it.

## [0.0.1]

The first version of the pip package. It was a placeholder to hold the name; it now holds the pre-alpha software described above under Unreleased. Not yet published.

[Unreleased]: https://github.com/MHughesDev/anything2mcp/commits/main
[0.0.1]: https://github.com/MHughesDev/anything2mcp/tree/main/bindings/python
