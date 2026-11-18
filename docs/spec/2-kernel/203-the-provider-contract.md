# 203. The provider contract

A provider is anything that supplies capabilities to the runtime. This section is the contract every provider meets, whoever wrote it and however it runs. Anything2MCP owns this contract; no outside project defines any part of it. Decision: [record 0049](../../decisions/0049-a-neutral-kernel-and-packs-behind-one-provider-contract.md). Built today: for packs compiled into the binary; plug-ins in another process are not built (package F3).

## Three ways to be a provider

| Way | What it is | Trust |
| --- | --- | --- |
| A first-party pack | Rust crates in this repository ([section 202](202-kernel-and-packs.md)) | Reviewed with the product |
| A plug-in | A separate process in any language, speaking the plug-in protocol | Signed, or off by default. An administrator approves its manifest |
| An upstream MCP server | An existing MCP server reached through the MCP pack's `mcp` mode | Only tools named in its mapping file are reachable, each pinned by the digest of its definition; the mapping file is its manifest, and its effects are `declared`, because the server runs code this runtime cannot see. Policy, approval, and audit wrap every call |

## Extension points

A provider fills one or more of a fixed list. It cannot add to the list.

| Extension point | Supplies | Defined in |
| --- | --- | --- |
| Driver | How the steps of one or more modes are carried out | `a2m-provider`; the technical reference |
| Auth provider | How a session of one kind is made, checked, renewed, and ended | `a2m-auth`; the technical reference; section 215 |
| Source reader | How one kind of input becomes validated records | `a2m-sources`; section 210 |
| Widget driver | How one kind of control is set and read | The technical reference |
| Discoverer | How a target is observed, returning findings | `a2m-provider`; [section 218](218-discovery-and-findings.md) |
| Compiler registrations | File kinds, the lowering of each verb of its modes, lint rules, and drafting rules | `a2m-files`; [section 206](206-the-compiler-and-the-plan.md) |

A provider may also add actions to the registry, under its own name space. They are defined once, like every action, and their four faces are generated. Only the dispatcher in `a2m-actions` calls one, after the caller's credential is checked and policy has decided, and it is given checked paths and the registry, never the store. A provider never adds a command or a tool by hand.

**Registered names.** A mode with its archetype, a surface kind with its archetype and whether a session is held on it, a step kind within its mode, a file kind, a lint code, a finding kind, and a control name are names a provider registers, each checked for shape in `a2m-core` ([decision 0062](../../decisions/0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md)). The registry is the only list of them. A target that names one nobody registered does not compile. Two providers that register one mode or surface kind must agree on its archetype.

**The registry is an immutable snapshot.** It is built once at start, one call per enabled pack and then the plug-ins found on the machine, and replaced whole when a provider is enabled or disabled. Its digest is recorded in every trace, and registration order never changes it or any choice made from it.

## What a provider can never do

- Decide whether something may happen. Only the policy engine decides.
- Sign in or renew. Only the session keeper does, calling the provider's auth provider.
- Approve anything, or mark anything verified.
- Write to the ledger. The engine writes; a driver returns outcomes. A driver may ask, within a step, whether another record of its operation holds a value it recorded, and is told which record, or no, or that the engine cannot tell; it is never given another record's values ([decision 0193](../../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)).
- Read the vault. A secret reaches a provider only through injection, for one step.
- Retry inside a step.

## The manifest

One declaration per provider. It is data: the runtime can read it without loading or starting the provider. It is a TOML file, `anything2mcp-provider.toml`, checked against a JSON Schema; a first-party pack embeds its own, so packs and plug-ins pass one parser and one schema. The full format, with an example and the checks made when it is read, is in the [`a2m-provider` design](../../architecture/crates/a2m-provider.md#the-manifest). Its fields:

| Field | Meaning |
| --- | --- |
| Identity | Name, version, publisher, kind (pack or plug-in), the digest of what will run, and a plug-in's signature |
| Contract version | Which version of this contract it was written for, beside the manifest's own format number |
| Run | For a plug-in: its program, fixed arguments, the environment names passed through, the plug-in protocol versions it speaks, its frame size, and how many requests it serves at once and queues |
| Extension points | Which it fills |
| Modes and verbs | For a driver: the modes it serves, each with its archetype, verbs, and step kinds; its rank, the default place in binding order when an operation states none; the intents stored ahead of acting; the parallel reads and writes per session; and what one write holds alone |
| Surface kinds | The surface kinds it registers, each with its archetype and whether a session is held on it |
| Discoverers | Each with its archetype, the finding kinds it may return, the sources it uses, the highest level it may claim, and its bounds |
| File kinds | The target-file kinds and binding fields it defines, with their schemas; its lint rules, with codes and levels; its drafting rules, from finding kind to file kind |
| Reads | The file kinds and finding kinds of other packs it reads as data |
| Actions | The actions it adds, each with its effect |
| Effects | What it may do: start which programs, read and write which file roots, reach which network destinations, hold a session, change a device. [Section 213](213-effects-hazards-and-consent.md) |
| Requirements | Outside programs and runtimes it needs: how each is detected, which digests are known, and what to tell a person who lacks one. Known digests are checked when availability is reported and again at every start |
| Platforms | The operating systems and architectures it supports |
| Evidence | The highest level its findings may claim, and the sources it can vouch for |

## Declared, and observed

Two different questions, kept apart. This distinction is one of the clearest lessons from REA ([research note 16](../../research/16-rea-methods.md)).

| Question | Answered by | When |
| --- | --- | --- |
| What can this provider do? | The manifest | At any time, without starting anything |
| Can it do it here, now? | An availability check that inspects the machine without starting the provider's subsystem | When listed, when enabled, and by `doctor.run` |
| Is it working? | A health check on the running provider | While it runs |

An availability check ends in one state, with a reason and a fix.

| State | Meaning |
| --- | --- |
| `ready` | Everything it needs is present and matches |
| `not_enabled` | Installed and switched off |
| `component_missing` | An outside program or runtime it needs was not found |
| `component_unverified` | The program was found and its digest is not a known one |
| `unsupported_host` | This operating system or architecture is not supported |
| `unsupported_version` | A needed program is present in a version the provider does not support |
| `misconfigured` | A setting it needs is absent or wrong |
| `unknown` | The check could not be made. Never reported as ready |

## Choosing between providers

More than one provider may be able to fill the same place: two browser engines for the `ui` mode, for instance.

- A provider is chosen by a setting, or because it is the only one that is ready.
- **There is no priority order and no silent fallback.** If two are ready and no setting names one, the answer is a failure that lists each candidate and why it was or was not usable.
- The choice and its reason are recorded in the trace, as the choice of binding already is.

This is separate from choosing a binding. An operation's bindings are tried in a declared order, by decision 0006; that order is written in the target's files and reviewed, or, where an operation states none, set by the compiler from each mode's rank in its manifest. Which program carries out a mode is a property of the installation, and must never change by accident.

## Lifecycle

```text
found -> checked -> enabled -> started -> healthy
                      |            |          |
                      v            v          v
                  disabled      failed     stopped
```

| Step | What happens | Fails as |
| --- | --- | --- |
| Found | The manifest is located: built in, from an entry point, or from a mapping file | Not listed |
| Checked | The manifest is validated against the contract version; a plug-in's signature is checked | A named problem in the listing |
| Enabled | A person switched it on. On one machine this is a protected setting when the provider starts processes or reaches the network | Stays off |
| Started | On first use, under the supervisor ([section 204](204-supervising-subsystems.md)), with a handshake | `transient`, or a stop with the availability state |
| Healthy | The health check passes at intervals | The provider is recycled; a write in flight is `possible_commit` |
| Stopped | When idle, on shutdown, or when it exceeds its manifest | Cleanup is reported, complete or not |

A provider that does something its manifest did not declare is stopped, and the run that was using it stops with the class `policy`.

## Versions

- The contract has a version. A provider states the one it was written for. The runtime supports the current and the previous one.
- A provider has its own version, recorded in every trace and finding it touches.
- A change to the contract is a change to a public interface and takes a decision record.

## Open points

- How schemas and lint rules cross a process boundary for plug-ins. The `a2m-provider` design proposes that schemas cross as the files the manifest names and that lowerings and lint rules are calls made at compile time, with the plug-in's digest recorded in the plan; the compiler's design still leaves it open.
- The manifest example in the `a2m-provider` design has no field yet for the kinds a provider reads from other packs.
