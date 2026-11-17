# Decision records

One file per decision: the context, what was decided, what follows from it, what was rejected, and what would reopen it. Add a new numbered file for every decision that changes the design; never rewrite an old one. To reverse a decision, add a new record that supersedes it and mark the old one superseded.

A record's status is one of: **Accepted**, in force; **Proposed**, a change someone wants and has not been given, not to be built on until accepted or supported by a tested hypothesis; **Superseded**, replaced by a later record that names it.

Who may change what ([record 0055](0055-agents-plan-build-and-merge-without-waiting-for-the-owner.md)): a record marked as a design decision can be superseded by any agent with a new record that gives the evidence. A record marked as an owner decision is changed only by the owner; an agent that thinks one is wrong writes a Proposed record and carries on. A record the owner has not read says so in its status line and is in force all the same.

Records 0001 to 0054 were written when the product was named Web2MCP, and keep the names of their time. [Record 0056](0056-one-name-everywhere-anything2mcp.md) has the key from the earlier names to the present ones.

| # | Decision | Status |
| --- | --- | --- |
| 0001 | [Name: Web2MCP](0001-name-web2mcp.md) | Superseded by 0048 |
| 0002 | [Rust first, other languages where they fit](0002-rust-first-other-languages-where-they-fit.md) | Accepted (owner decision) |
| 0003 | [No AI at run time](0003-no-ai-at-run-time.md) | Accepted (design decision) |
| 0004 | [Rust browser engine on the DevTools protocol](0004-rust-browser-engine-on-the-devtools-protocol.md) | Accepted (design decision) |
| 0005 | [Seven modes behind one driver interface](0005-seven-modes-behind-one-driver-interface.md) | Accepted (design decision) |
| 0006 | [The runtime picks the fastest verified binding](0006-the-runtime-picks-the-fastest-verified-binding.md) | Accepted (design decision) |
| 0007 | [New bindings run in shadow before promotion](0007-new-bindings-run-in-shadow-before-promotion.md) | Accepted (design decision) |
| 0008 | [Control plane plus runners](0008-control-plane-plus-runners.md) | Accepted (design decision) |
| 0009 | [Four identity modes on a shared server](0009-four-identity-modes-on-a-shared-server.md) | Accepted (design decision) |
| 0010 | [Toolsets, and few tools listed up front](0010-toolsets-and-few-tools-listed-up-front.md) | Accepted (design decision) |
| 0011 | [Stateless Streamable HTTP and an OAuth resource server](0011-stateless-streamable-http-and-an-oauth-resource-server.md) | Accepted (design decision) |
| 0012 | [Approvals happen out of reach of the agent and bind to the diff](0012-approvals-happen-out-of-reach-of-the-agent-and-bind-to-the-diff.md) | Accepted (design decision) |
| 0013 | [Writes go through the interface until a faster binding is proven](0013-writes-go-through-the-interface-until-a-faster-binding-is-proven.md) | Accepted (design decision) |
| 0014 | [Declarative files: YAML targets, TOML settings, CEL expressions](0014-declarative-files-yaml-targets-toml-settings-cel-expressions.md) | Accepted (design decision) |
| 0015 | [SQLite locally, Postgres on servers, and a built-in ledger](0015-sqlite-locally-postgres-on-servers-and-a-built-in-ledger.md) | Accepted (design decision) |
| 0016 | [Plug-ins are separate processes](0016-plug-ins-are-separate-processes.md) | Accepted (design decision) |
| 0017 | [Operate openly: no evasion of bot defenses or MFA](0017-operate-openly-no-evasion-of-bot-defenses-or-mfa.md) | Accepted (design decision) |
| 0018 | [License: Apache-2.0](0018-license-apache-2-0.md) | Superseded by 0057 |
| 0019 | [Priority order: safe, correct, fast, easy](0019-priority-order-safe-correct-fast-easy.md) | Accepted (owner decision) |
| 0020 | [Every write is verified, and resume reconciles first](0020-every-write-is-verified-and-resume-reconciles-first.md) | Accepted (design decision) |
| 0021 | [No real application is named in the repository](0021-no-real-application-is-named-in-the-repository.md) | Accepted (owner decision) |
| 0022 | [Published package names](0022-published-package-names.md) | Superseded by 0056 |
| 0023 | [Commands and files first for building, MCP tools second, one registry for both](0023-commands-and-files-first-for-building-mcp-tools-second-one-registry-for-both.md) | Accepted (design decision) |
| 0024 | [One local engine per user, reached over a socket or named pipe](0024-one-local-engine-per-user-reached-over-a-socket-or-named-pipe.md) | Accepted (design decision) |
| 0025 | [pip and npm packages ship the binary with a thin SDK; no in-process bindings yet](0025-pip-and-npm-packages-ship-the-binary-with-a-thin-sdk-no-in-process-bindings-yet.md) | Accepted (design decision) |
| 0026 | [Local approvals come from the engine, with an operating-system user-presence check](0026-local-approvals-come-from-the-engine-with-an-operating-system-user-presence-check.md) | Accepted (design decision) |
| 0027 | [Ledger: one row per commit point, with append-only events beside current state](0027-ledger-one-row-per-commit-point-with-append-only-events-beside-current-state.md) | Accepted (design decision) |
| 0028 | [Runner link: mutually authenticated HTTPS with long polls, and the same interface in memory](0028-runner-link-mutually-authenticated-https-with-long-polls-and-the-same-interface-in-memory.md) | Accepted (design decision) |
| 0029 | [Lost leases: stop early, quarantine, reconcile, verify again](0029-lost-leases-stop-early-quarantine-reconcile-verify-again.md) | Accepted (design decision) |
| 0030 | [Where session secrets live](0030-where-session-secrets-live.md) | Accepted (design decision) |
| 0031 | [The browser always runs with a profile the engine owns](0031-the-browser-always-runs-with-a-profile-the-engine-owns.md) | Accepted (design decision) |
| 0032 | [One live session per identity, shared by tabs, with a single renewer](0032-one-live-session-per-identity-shared-by-tabs-with-a-single-renewer.md) | Accepted (design decision) |
| 0033 | [Policy decisions use Cedar, with grants compiled to it](0033-policy-decisions-use-cedar-with-grants-compiled-to-it.md) | Superseded by 0082 |
| 0034 | [Policy is a versioned, tested artifact, and every decision is recorded](0034-policy-is-a-versioned-tested-artifact-and-every-decision-is-recorded.md) | Accepted (design decision) |
| 0035 | [A user's session is used only by that user and agents acting for them](0035-a-user-s-session-is-used-only-by-that-user-and-agents-acting-for-them.md) | Accepted (owner decision) |
| 0036 | [Target files are compiled into a hashed plan before anything runs](0036-target-files-are-compiled-into-a-hashed-plan-before-anything-runs.md) | Accepted (design decision) |
| 0037 | [Evidence: events, a step trace, and artifacts, redacted at capture](0037-evidence-events-a-step-trace-and-artifacts-redacted-at-capture.md) | Accepted (design decision) |
| 0038 | [Vault keys in four layers, with the root in the OS store or a key service](0038-vault-keys-in-four-layers-with-the-root-in-the-os-store-or-a-key-service.md) | Accepted (design decision) |
| 0039 | [Local approval: a diff page for the moment, then an operating-system prompt](0039-local-approval-a-diff-page-for-the-moment-then-an-operating-system-prompt.md) | Accepted (design decision) |
| 0040 | [Nothing leaves the machine unless switched on](0040-nothing-leaves-the-machine-unless-switched-on.md) | Accepted (design decision) |
| 0041 | [Identity strategies make creates and dependent records resumable](0041-identity-strategies-make-creates-and-dependent-records-resumable.md) | Accepted (design decision) |
| 0042 | [Design for an agent that has been steered](0042-design-for-an-agent-that-has-been-steered.md) | Accepted (design decision) |
| 0043 | [The engine fails closed, with three health states](0043-the-engine-fails-closed-with-three-health-states.md) | Accepted (design decision) |
| 0044 | [A published version becomes current in stages](0044-a-published-version-becomes-current-in-stages.md) | Accepted (design decision) |
| 0045 | [The workspace starts with every crate, and the audit enforces the layering](0045-the-workspace-starts-with-every-crate-and-the-audit-enforces-the-layering.md) | Accepted (design decision) |
| 0046 | [The pip name is held by a placeholder, published only when a version tag is pushed](0046-the-pip-name-is-held-by-a-placeholder-published-on-a-version-tag.md) | Accepted (design decision) |
| 0047 | [The test application is a small server in the workspace, and the licence list is closed](0047-the-test-application-is-a-small-server-in-the-workspace-with-a-closed-licence-list.md) | Accepted (design decision) |
| 0048 | [The product becomes Anything2MCP, a universal runtime built on the same core](0048-the-product-becomes-anything2mcp.md) | Accepted (owner decision) |
| 0049 | [A neutral kernel, and packs behind one provider contract](0049-a-neutral-kernel-and-packs-behind-one-provider-contract.md) | Accepted (design decision) |
| 0050 | [Seven interaction archetypes, and modes that arrive with packs](0050-seven-archetypes-and-modes-that-arrive-with-packs.md) | Accepted (design decision) |
| 0051 | [Findings, and an evidence ladder from inferred to approved](0051-findings-and-the-evidence-ladder.md) | Accepted (design decision) |
| 0052 | [Effects are declared and enforced, and a hazard raises the approval](0052-effects-are-enforced-and-hazards-raise-the-approval.md) | Accepted (design decision) |
| 0053 | [REA is studied for its method, and used only as an optional provider](0053-rea-is-studied-for-method-and-used-only-as-an-optional-provider.md) | Accepted (design decision) |
| 0054 | [Devices are reached through their own interfaces, and firmware only through the device's own update](0054-devices-through-their-own-interfaces-and-firmware-through-the-devices-own-update.md) | Accepted (design decision) |
| 0055 | [Agents plan, build, and merge without waiting for the owner](0055-agents-plan-build-and-merge-without-waiting-for-the-owner.md) | Accepted (owner decision) |
| 0056 | [One name everywhere: `anything2mcp`, with `a2m` for the parts](0056-one-name-everywhere-anything2mcp.md) | Accepted (design decision) |
| 0057 | [The licence is MIT](0057-the-licence-is-mit.md) | Accepted (owner decision) |
| 0058 | [A toolkit for developers, with no catalog and no preferred agent](0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md) | Accepted (owner decision) |
| 0059 | [The scout drafts and proves reads by itself, and three things stay with the developer](0059-the-scout-drafts-and-proves-reads-and-three-things-stay-with-the-developer.md) | Accepted (design decision) |
| 0060 | [A mode is a registered name, and the archetype is the closed type](0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md) | Accepted (design decision) |
| 0061 | [The layers are redrawn so that every needed dependency is allowed](0061-the-layers-are-redrawn-so-that-every-needed-dependency-is-allowed.md) | Accepted (design decision) |
| 0062 | [Words that arrive with packs are registered names, and packs share data and never code](0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md) | Accepted (design decision) |
| 0063 | [The kernel's interfaces, as the crate designs settle them](0063-the-kernels-interfaces-as-the-crate-designs-settle-them.md) | Accepted (design decision) |
| 0064 | [Five further packs are designed before they are built](0064-five-further-packs-are-designed-before-they-are-built.md) | Accepted (design decision) |
| 0065 | [The specification is rebuilt in parts, and the first design is archived](0065-the-specification-is-rebuilt-in-parts-and-the-first-design-is-archived.md) | Accepted (design decision) |
| 0066 | [Configuration in layers that may only tighten, and a bar that is measured](0066-configuration-in-layers-that-may-only-tighten-and-a-bar-that-is-measured.md) | Accepted (design decision) |
| 0067 | [The kernel is built layer by layer, in parallel, and the first driver through the loop is the HTTP pack's](0067-the-kernel-is-built-layer-by-layer-in-parallel-and-the-first-driver-is-the-http-packs.md) | Accepted (design decision) |
| 0068 | [Values, names, and identifiers, as the core crate settles them](0068-values-names-and-identifiers-as-the-core-crate-settles-them.md) | Accepted (design decision) |
| 0069 | [The plan is canonical JSON, hashed whole, and keyed digests are per workspace](0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md) | Accepted (design decision) |
| 0070 | [The browser engine stands on its own client, over the debugging pipe](0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md) | Accepted (design decision) |
| 0071 | [The MCP server stands on the official SDK behind a seam, and serves two revisions](0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md) | Accepted (design decision) |
| 0072 | [Expressions are a CEL subset, on the engine's own parser, checker, and evaluator](0072-expressions-are-a-cel-subset-on-the-engines-own-parser-and-evaluator.md) | Accepted (design decision) |
| 0073 | [Decimals use operators, division is a function, and how a field type normalizes](0073-decimals-use-operators-division-is-a-function-and-how-a-field-type-normalizes.md) | Accepted (design decision) |
| 0074 | [The vault's cipher, key stores, and other outside crates](0074-the-vaults-cipher-key-stores-and-other-outside-crates.md) | Accepted (design decision) |
| 0075 | [The vault's interface, as building it settled it](0075-the-vaults-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0076 | [One trace record per step, and a signature chained over record digests](0076-one-trace-record-per-step-and-a-signature-chained-over-record-digests.md) | Accepted (design decision) |
| 0077 | [Logs through the `tracing` crate with our own subscriber, and spans through a handle](0077-logs-through-the-tracing-crate-with-our-own-subscriber-and-spans-through-a-handle.md) | Accepted (design decision) |
| 0078 | [Target files are read by an event parser into our own located tree, and checked by our own schema subset](0078-target-files-are-read-by-an-event-parser-into-our-own-located-tree-and-checked-by-our-own-schema-subset.md) | Accepted (design decision) |
| 0079 | [The compiler's registration points, and the file formats, as the code settles them](0079-the-compilers-registration-points-and-the-file-formats-as-the-code-settles-them.md) | Accepted (design decision) |
| 0080 | [The store is one SQLite file, and a crate reaches its own tables through the transaction](0080-the-store-is-one-sqlite-file-and-a-crate-reaches-its-own-tables-through-the-transaction.md) | Accepted (design decision) |
| 0081 | [What the ledger holds as data: the token, doubt, locks, and rule 3](0081-what-the-ledger-holds-as-data-the-token-doubt-locks-and-rule-three.md) | Accepted (design decision) |
| 0082 | [The decision core is the policy crate's own evaluator](0082-the-decision-core-is-the-policy-crates-own-evaluator.md) | Accepted (design decision) |
| 0083 | [The policy engine's interface, as building it settled it](0083-the-policy-engines-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0084 | [The session keeper's outside crates, and TLS for identity services](0084-the-session-keepers-outside-crates-and-tls-for-identity-services.md) | Accepted (design decision) |
| 0085 | [The session keeper's interface, as building it settled it](0085-the-session-keepers-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0086 | [The source readers are our own, over one outside crate that inflates](0086-the-source-readers-are-our-own-over-one-outside-crate-that-inflates.md) | Accepted (design decision) |
| 0087 | [The source readers' interface, as building it settled it](0087-the-source-readers-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0088 | [The provider contract's outside crates: a TOML parser, a descriptor helper, safe process calls, and the Windows calls](0088-the-provider-contracts-outside-crates.md) | Accepted (design decision) |
| 0089 | [The provider contract as building it settled it](0089-the-provider-contract-as-building-it-settled-it.md) | Accepted (design decision) |
| 0090 | [The supervisor ends a group only while its leader is unreaped, and its links are started by three calls](0090-the-supervisor-ends-a-group-only-while-its-leader-is-unreaped.md) | Accepted (design decision) |
| 0091 | [The engine loop, as its first two passes settled it](0091-the-engine-loop-as-its-first-two-passes-settled-it.md) | Accepted (design decision) |
| 0092 | [A record that waits for an earlier record is planned under its place in the run](0092-a-record-that-waits-for-an-earlier-record-is-planned-under-its-place-in-the-run.md) | Accepted (design decision) |
| 0093 | [The scheduler's interface, as building it settled it](0093-the-schedulers-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0094 | [The action layer's interface, as building it settled it](0094-the-action-layers-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0095 | [The settings loader and the in-process runner, as built](0095-the-settings-loader-and-the-in-process-runner-as-built.md) | Accepted (design decision) |
| 0096 | [The HTTP pack's endpoint entry, steps, and payload, as building them settled them](0096-the-http-packs-endpoint-entry-steps-and-payload-as-building-them-settled-them.md) | Accepted (design decision) |
| 0097 | [What the provider contract could not give the HTTP driver, and what the pack does instead](0097-what-the-provider-contract-could-not-give-the-http-driver-and-what-the-pack-does-instead.md) | Accepted (design decision) |
| 0098 | [The `cli` mode is real, and a program is a surface](0098-the-cli-mode-is-real-and-a-program-is-a-surface.md) | Accepted (design decision) |
| 0099 | [The command pack as building it settled it, and what the kernel could not give it](0099-the-command-pack-as-building-it-settled-it.md) | Accepted (design decision) |
| 0100 | [The mode `sql`, and the data pack's contract as building it settled it](0100-the-mode-sql-and-the-data-packs-contract-as-building-it-settled-it.md) | Accepted (design decision) |
| 0101 | [What the data pack refuses, and how it names it](0101-what-the-data-pack-refuses-and-how-it-names-it.md) | Accepted (design decision) |
| 0102 | [The `file` mode, and what the file pack registers](0102-the-file-mode-and-what-the-file-pack-registers.md) | Accepted (design decision) |
| 0103 | [The file pack, as building it settled it](0103-the-file-pack-as-building-it-settled-it.md) | Accepted (design decision) |
| 0104 | [A wire protocol is one interface inside the stream pack, and the first standard one is still to be chosen](0104-a-wire-protocol-is-one-interface-inside-the-stream-pack-and-the-first-standard-one-is-still-to-be-chosen.md) | Accepted (design decision) |
| 0105 | [The `stream` mode, and the stream pack as building it settled it](0105-the-stream-mode-and-the-stream-pack-as-building-it-settled-it.md) | Accepted (design decision) |
| 0106 | [The device pack speaks its first protocol itself, and checks a range before a frame is built](0106-the-device-pack-speaks-its-first-protocol-itself-and-checks-a-range-before-a-frame-is-built.md) | Accepted (design decision) |
| 0107 | [The `device` mode, and the device pack as building it settled it](0107-the-device-mode-and-the-device-pack-as-building-it-settled-it.md) | Accepted (design decision) |
| 0108 | [An upstream MCP server is reached only through a pinned mapping, by a client of the pack's own](0108-an-upstream-mcp-server-is-reached-only-through-a-pinned-mapping-by-a-client-of-the-packs-own.md) | Accepted (design decision) |
| 0109 | [The MCP pack as building it settled it](0109-the-mcp-pack-as-building-it-settled-it.md) | Accepted (design decision) |
| 0110 | [The browser engine's interface as building it settled it](0110-the-browser-engines-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0111 | [An observing tab's lock as built, and what the browser was measured to do](0111-an-observing-tabs-lock-as-built-and-what-the-browser-was-measured-to-do.md) | Accepted (design decision) |
| 0112 | [The `ui` driver as built: five step kinds, and a commit that waits for its confirmation](0112-the-ui-driver-as-built-five-step-kinds-and-a-commit-that-waits-for-its-confirmation.md) | Accepted (design decision) |
| 0113 | [A screen's fingerprint is a section, absent is a variant, and a table is read by its headings](0113-a-screens-fingerprint-is-a-section-absent-is-a-variant-and-a-table-is-read-by-its-headings.md) | Accepted (design decision) |
| 0114 | [The web probe as built: three discoverers over an observing tab](0114-the-web-probe-as-built-three-discoverers-over-an-observing-tab.md) | Accepted (design decision) |
| 0115 | [The MCP endpoint as built: checks in front of the SDK, and a redactor on the way out](0115-the-mcp-endpoint-as-built-checks-in-front-of-the-sdk-and-a-redactor-on-the-way-out.md) | Accepted (design decision) |
| 0116 | [The REST router takes its caller from the mount and chooses its status in one table](0116-the-rest-router-takes-its-caller-from-the-mount-and-chooses-its-status-in-one-table.md) | Accepted (design decision) |
| 0117 | [The local channel and the person surface on one machine, as built](0117-the-local-channel-and-the-person-surface-on-one-machine-as-built.md) | Accepted (design decision) |
| 0119 | [The product crate, as building it settled it](0119-the-product-crate-as-building-it-settled-it.md) | Accepted (design decision) |
| 0120 | [The desktop pack stands on each system's own interface, through thin bindings](0120-the-desktop-pack-stands-on-each-systems-own-interface-through-thin-bindings.md) | Accepted (design decision) |
| 0121 | [The plan carries what the run time needs from target files](0121-the-plan-carries-what-the-run-time-needs-from-target-files.md) | Accepted (design decision) |
| 0122 | [Small things the core crate gives its builders, and a secret is wiped when dropped](0122-small-things-the-core-crate-gives-its-builders-and-a-secret-is-wiped-when-dropped.md) | Accepted (design decision) |
| 0123 | [The Linux Secret Service back end is left out until its store crate stands on maintained crates](0123-the-linux-secret-service-back-end-is-left-out-until-its-store-crate-is-maintained.md) | Accepted (design decision) |
| 0124 | [The plan declares a surface's programs, and a schema checks run-time values](0124-the-plan-declares-a-surfaces-programs-and-a-schema-checks-run-time-values.md) | Accepted (design decision) |
| 0130 | [The MCP protocol layer is our own, because the SDK brings an unmaintained crate](0130-the-mcp-protocol-layer-is-our-own-because-the-sdk-brings-an-unmaintained-crate.md) | Accepted (design decision) |
| 0131 | [The front doors joined: `serve`, the thin client, and the bridge](0131-the-front-doors-joined-serve-the-thin-client-and-the-bridge.md) | Accepted (design decision) |
| 0132 | [The action registry's second pass: a decline ends the run, and a wait reads the scheduler's own table](0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md) | Accepted (design decision) |
| 0133 | [The presence prompt on each operating system, and what it is said to prove](0133-the-presence-prompt-on-each-operating-system-and-what-it-is-said-to-prove.md) | Accepted (design decision) |
| 0134 | [One browser starts at a time, and a silent start is slow, not foreign](0134-one-browser-starts-at-a-time-and-a-silent-start-is-slow-not-foreign.md) | Accepted (design decision) |
| 0135 | [The wheel carries the binary and a thin client, and npm uses platform packages](0135-the-wheel-carries-the-binary-and-a-thin-client-and-npm-uses-platform-packages.md) | Accepted (design decision) |
| 0136 | [The MCP pack refuses free text, reads its list again before a write, and passes on only listed members](0136-the-mcp-pack-refuses-free-text-reads-its-list-again-before-a-write-and-passes-on-only-listed-members.md) | Accepted (design decision) |
| 0137 | [A device is held to its serial, a write frame is recorded before it is sent, and a run has a frame ceiling](0137-a-device-is-held-to-its-serial-a-write-frame-is-recorded-before-it-is-sent-and-a-run-has-a-frame-ceiling.md) | Accepted (design decision) |
| 0138 | [The vault and the session keeper after their safety audit](0138-the-vault-and-the-session-keeper-after-their-safety-audit.md) | Accepted (design decision) |
| 0139 | [A write that was sent is never settled by an answer that is not the application's verdict](0139-a-write-that-was-sent-is-never-settled-by-an-answer-that-is-not-the-applications-verdict.md) | Accepted (design decision) |
| 0140 | [The command pack after its safety audit](0140-the-command-pack-after-its-safety-audit.md) | Accepted (design decision) |
| 0141 | [The data pack holds a statement to one read or one keyed write, and a connection to one file](0141-the-data-pack-holds-a-statement-to-one-read-or-one-keyed-write-and-a-connection-to-one-file.md) | Accepted (design decision) |
| 0142 | [The presence prompt's interface and the Linux prompt, as built](0142-the-presence-prompt-interface-and-the-linux-prompt-as-built.md) | Accepted (design decision) |
| 0143 | [The browser engine after its safety audit](0143-the-browser-engine-after-its-safety-audit.md) | Accepted (design decision) |
| 0144 | [The web drivers and discoverers after their safety audit](0144-the-web-drivers-and-discoverers-after-their-safety-audit.md) | Accepted (design decision) |
| 0145 | [Only the policy engine clears a write, and the ledger reads the whole clearance](0145-only-the-policy-engine-clears-a-write-and-the-ledger-reads-the-whole-clearance.md) | Accepted (design decision) |
| 0146 | [The macOS and Windows presence prompts, as built](0146-the-macos-and-windows-presence-prompts-as-built.md) | Accepted (design decision) |
| 0147 | [The engine after its safety audit](0147-the-engine-after-its-safety-audit.md) | Accepted (design decision) |
| 0148 | [The provider contract and the scheduler after their safety audit](0148-the-provider-contract-and-the-scheduler-after-their-safety-audit.md) | Accepted (design decision) |
| 0149 | [The file pack after its safety audit](0149-the-file-pack-after-its-safety-audit.md) | Accepted (design decision) |
| 0150 | [A take records its message before it acknowledges, and an answer with no rule leaves a write in doubt](0150-a-take-records-its-message-before-it-acknowledges-and-an-answer-with-no-rule-leaves-a-write-in-doubt.md) | Accepted (design decision) |
| 0151 | [The doors take up the action registry's second pass](0151-the-doors-take-up-the-action-registrys-second-pass.md) | Accepted (design decision) |
| 0152 | [The action registry and telemetry after their safety audit](0152-the-action-registry-and-telemetry-after-their-safety-audit.md) | Accepted (design decision) |
| 0153 | [What the compiler, the plan, and the redactor hold by themselves, after the safety audit](0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md) | Accepted (design decision) |
| 0154 | [The HTTP, command, and MCP packs state what saves, what writes, and which key is sent](0154-the-http-command-and-mcp-packs-state-what-saves-what-writes-and-which-key-is-sent.md) | Accepted (design decision) |
| 0155 | [Four packs state what saves, what writes, and when their guard lifts](0155-four-packs-state-what-saves-what-writes-and-when-their-guard-lifts.md) | Accepted (design decision) |
| 0156 | [The clients are generated from the served document, and reach Windows by the pipe](0156-the-clients-are-generated-from-the-served-document-and-reach-windows-by-the-pipe.md) | Accepted (design decision) |
| 0157 | [TLS is switched on with the provider stated in each client](0157-tls-is-switched-on-with-the-provider-stated-in-each-client.md) | Accepted (design decision) |
| 0158 | [The product crate and the local engine after their safety audit](0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md) | Accepted (design decision) |
| 0159 | [A lost answer is a write in doubt at the doors and in the clients](0159-a-lost-answer-is-a-write-in-doubt-at-the-doors-and-in-the-clients.md) | Accepted (design decision) |
| 0160 | [An idempotency key gives back its run, and `run.list` shows a caller's own runs](0160-an-idempotency-key-gives-back-its-run-and-run-list-shows-a-callers-own-runs.md) | Accepted (design decision) |
| 0161 | [The plan and the engine after the packs were built](0161-the-plan-and-the-engine-after-the-packs-were-built.md) | Accepted (design decision) |
| 0162 | [The provider contract's second pass: what a step is handed, and what it hands back](0162-the-provider-contracts-second-pass-what-a-step-is-handed-and-hands-back.md) | Accepted (design decision) |
| 0163 | [A person's act an agent asks for is carried out on a page of the person surface](0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md) | Accepted (design decision) |
| 0164 | [The authoring actions served through the engine](0164-the-authoring-actions-served-through-the-engine.md) | Accepted (design decision) |
| 0165 | [A client starts the engine on demand, detached, and gives it the passphrase on a pipe](0165-a-client-starts-the-engine-on-demand-detached-and-gives-it-the-passphrase-on-a-pipe.md) | Accepted (design decision) |
| 0166 | [HTTPS in the product: trust roots at engine start, and one maker of clients](0166-https-in-the-product-trust-roots-at-engine-start-and-one-maker-of-clients.md) | Accepted (design decision) |
| 0167 | [The stream pack speaks NATS with JetStream, through a client of its own](0167-the-stream-pack-speaks-nats-with-jetstream-through-a-client-of-its-own.md) | Accepted (design decision) |
| 0168 | [The data pack reaches a server database with the guarantees of an embedded file](0168-the-data-pack-reaches-a-server-database-with-the-guarantees-of-an-embedded-file.md) | Accepted (design decision) |
| 0169 | [The first release is for one machine](0169-the-first-release-is-for-one-machine.md) | Accepted (owner decision) |
| 0170 | [The scout identifies each surface with one discovery job and stores each stage](0170-the-scout-identifies-each-surface-with-one-discovery-job-and-stores-each-stage.md) | Accepted (design decision) |
| 0171 | [The scout's report is hashed data, with the system's words kept apart](0171-the-scouts-report-is-hashed-data-with-the-systems-words-kept-apart.md) | Accepted (design decision) |
| 0172 | [The ledger ends a record on an acknowledgement, and keeps what the system said](0172-the-ledger-ends-a-record-on-an-acknowledgement-and-keeps-what-the-system-said.md) | Accepted (design decision) |
| 0173 | [The stream and file packs take up the kernel's second pass](0173-the-stream-and-file-packs-take-up-the-kernels-second-pass.md) | Accepted (design decision) |
| 0175 | [The data and device packs take up the second pass of the provider contract](0175-the-data-and-device-packs-take-up-the-second-pass-of-the-provider-contract.md) | Accepted (design decision) |
| 0178 | [An effect left as an open question is undecided, not a write](0178-an-effect-left-as-an-open-question-is-undecided-not-a-write.md) | Accepted (design decision) |
| 0176 | [The action registry takes up the kernel's second pass](0176-the-action-registry-takes-up-the-kernels-second-pass.md) | Accepted (design decision) |
| 0177 | [A target's offline tests run against a replay of their fixtures](0177-a-targets-offline-tests-run-against-a-replay-of-their-fixtures.md) | Accepted (design decision) |
| 0179 | [What each pack drafts, so that a scout ends with reads that work](0179-what-each-pack-drafts-so-that-a-scout-ends-with-reads-that-work.md) | Accepted (design decision) |
| 0183 | [The web pack drafts a read of each table a snapshot names](0183-the-web-pack-drafts-a-read-of-each-table-a-snapshot-names.md) | Accepted (design decision) |
| 0186 | [The release workflow builds, checks, and publishes the npm packages](0186-the-release-workflow-builds-checks-and-publishes-the-npm-packages.md) | Accepted (design decision) |
| 0181 | [The product and the local engine take up what the kernel now offers](0181-the-product-and-the-local-engine-take-up-what-the-kernel-now-offers.md) | Accepted (design decision) |
| 0182 | [The file, data, and stream packs after the first drafted reads](0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md) | Accepted (design decision) |
| 0187 | [The web pack matches a screen by its address, reads by key, and reads labelled values](0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md) | Accepted (design decision) |
