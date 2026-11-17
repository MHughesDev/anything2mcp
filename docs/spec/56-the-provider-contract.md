# 56. The provider contract

A provider is anything that supplies capabilities to the runtime. This section is the contract every provider meets, whoever wrote it and however it runs. Anything2MCP owns this contract; no outside project defines any part of it. Decision: [record 0049](../decisions/0049-a-neutral-kernel-and-packs-behind-one-provider-contract.md).

## Three ways to be a provider

| Way | What it is | Trust |
| --- | --- | --- |
| A first-party pack | Rust crates in this repository ([section 55](55-kernel-and-packs.md)) | Reviewed with the product |
| A plug-in | A separate process in any language, speaking the plug-in protocol | Signed, or off by default. An administrator approves its manifest |
| An upstream MCP server | An existing MCP server reached through the `mcp` mode | Only tools named in a mapping file are reachable. Policy, approval, and audit wrap every call |

## Extension points

A provider fills one or more of a fixed list. It cannot add to the list.

| Extension point | Supplies | Defined in |
| --- | --- | --- |
| Driver | How the steps of one or more modes are carried out | The technical reference |
| Auth provider | How a session of one kind is made, checked, renewed, and ended | The technical reference; section 44 |
| Source reader | How one kind of input becomes validated records | Section 12 |
| Widget driver | How one kind of control is set and read | The technical reference |
| Discoverer | How a target is observed, returning findings | [Section 58](58-discovery-and-findings.md) |

A provider may also contribute actions to the registry, under its own name space. They are defined once, like every action, and their four faces are generated. A provider never adds a command or a tool by hand.

## What a provider can never do

- Decide whether something may happen. Only the policy engine decides.
- Sign in or renew. Only the session keeper does, calling the provider's auth provider.
- Approve anything, or mark anything verified.
- Write to the ledger. The engine writes; a driver returns outcomes.
- Read the vault. A secret reaches a provider only through injection, for one step.
- Retry inside a step.

## The manifest

One declaration per provider. It is data: the runtime can read it without loading or starting the provider.

| Field | Meaning |
| --- | --- |
| Identity | Name, version, publisher, and the digest of what will run |
| Contract version | Which version of this contract it was written for |
| Extension points | Which it fills |
| Modes and verbs | For a driver: the modes it serves and the verbs of each |
| File kinds | The target-file kinds and binding fields it defines, with their schemas |
| Actions | The actions it contributes |
| Effects | What it may do: start which programs, read and write which file roots, reach which network destinations, hold a session, change a device. [Section 61](61-effects-hazards-and-consent.md) |
| Requirements | Outside programs and runtimes it needs: how each is detected, which digests are known, and what to tell a person who lacks one |
| Platforms | The operating systems and architectures it supports |
| Evidence | The highest level its findings may claim, and the sources it can vouch for |

## Declared, and observed

Two different questions, kept apart. This distinction is one of the clearest lessons from REA ([research note 16](../research/16-rea-methods.md)).

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

This is separate from choosing a binding. An operation's bindings are tried in a declared order, by decision 0006; that order is written in the target's files and reviewed. Which program carries out a mode is a property of the installation, and must never change by accident.

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
| Started | On first use, under the supervisor ([section 57](57-supervising-subsystems.md)), with a handshake | `transient`, or a stop with the availability state |
| Healthy | The health check passes at intervals | The provider is recycled; a write in flight is `possible_commit` |
| Stopped | When idle, on shutdown, or when it exceeds its manifest | Cleanup is reported, complete or not |

A provider that does something its manifest did not declare is stopped, and the run that was using it stops with the class `policy`.

## Versions

- The contract has a version. A provider states the one it was written for. The runtime supports the current and the previous one.
- A provider has its own version, recorded in every trace and finding it touches.
- A change to the contract is a change to a public interface and takes a decision record.

## Open points

- The manifest's file format and schema. The long-term plan's package B3.
- How schemas and lint rules cross a process boundary for plug-ins.
- Whether an upstream MCP server needs a manifest of its own or the mapping file is its manifest. The second, most likely.
