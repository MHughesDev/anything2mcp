# 0056. One name everywhere: `anything2mcp`, with `a2m` for the parts

**Status:** Accepted. The owner renamed the repository, registered the registry accounts, and left the remaining names to the building agent. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Supersedes:** [0022](0022-published-package-names.md), the published package names. **Carries out:** [0048](0048-the-product-becomes-anything2mcp.md). **Refines:** [0045](0045-the-workspace-starts-with-every-crate-and-the-audit-enforces-the-layering.md), the layering table.

## Context

The product became Anything2MCP on 2026-11-16. The repository is now `MHughesDev/anything2mcp`. Every name inside it still said Web2MCP: 23 crates, the binary, the pip placeholder, the built-in tool prefix, and several hundred lines of documents. The long-term plan proposed the new names and waited for the owner, who has now answered.

## Decision

| Thing | Name |
| --- | --- |
| Product | Anything2MCP |
| Repository | `MHughesDev/anything2mcp` |
| Pip package and Python import | `anything2mcp` |
| Crate on crates.io | `anything2mcp` |
| npm package | `anything2mcp` |
| Command | `anything2mcp`, with `a2m` as a short name for the same program |
| Kernel crates | `a2m-*` |
| Pack crates | `a2m-pack-<pack>` or `a2m-pack-<pack>-<part>` |
| Built-in tool prefix | `a2m_` |
| Project file | `anything2mcp.toml` |
| Working folder | `.anything2mcp/` |
| Environment variables | `A2M_` and a name, such as `A2M_HOME` |

- **One name on every registry.** The earlier design had a different name on npm because the first name was taken there. That reason is gone.
- **`a2m` is never a package name.** It is taken on PyPI and npm. It is the short command and the crate prefix.
- **No alias packages.** The earlier names shipped nothing but two placeholders. They are not carried forward as a second way to install the product. The placeholders that exist stay where they are; [publishing setup](../plan/publishing-setup.md) says what to do with them.
- **Documents use the full command.** Examples say `anything2mcp`, so that every document, test, and message agrees. `a2m` is for typing.
- **Crates were renamed and sorted.** The six crates that know one kind of system became pack crates. A new kernel crate, `a2m-provider`, holds the provider contract. The layering table in the [technical reference](../reference/technical-reference.md#crate-layering) now has the kernel in layers 1 to 6, the packs in layers 7 and 8, and the product in layer 9.
- **The audit enforces three rules by name:** a kernel crate never depends on a pack crate; a pack crate sees only the kernel's first three layers; a pack crate depends on another pack crate only when that crate is the base of its own pack.
- **History keeps its names.** Decision records, journal entries, and two research notes that record checks of the earlier names are left as written. The audit refuses an earlier name anywhere else.

| Earlier crate | Now |
| --- | --- |
| `w2m-browser` | `a2m-pack-web-browser` |
| `w2m-driver-ui` | `a2m-pack-web-ui` |
| `w2m-probe` | `a2m-pack-web-probe` |
| `w2m-driver-http` | `a2m-pack-http` |
| `w2m-driver-mcp` | `a2m-pack-mcp` |
| `w2m-driver-desktop` | `a2m-pack-desktop` |
| Every other `w2m-*` | The same name with `a2m-` |
| `web2mcp` | `anything2mcp` |

## Consequences

- The engine can no longer name a driver. It gets drivers from the registry in `a2m-provider`. This was the intent of [decision 0049](0049-a-neutral-kernel-and-packs-behind-one-provider-contract.md); the table now makes the build refuse anything else.
- The capability probe sits in the web pack. Its neutral part, if it has one, moves to the kernel when the probe is built, by a new record.
- On 2026-11-17 the registries showed nothing published as `anything2mcp` on PyPI, crates.io, or npm. A name is held only by a published release, so the names are not counted as held until one succeeds.
- A reader of an old decision record meets the earlier names. The table above is the key.

## Alternatives considered

- Keep the earlier names as alias packages, as the long-term plan first proposed: two names to publish, test, and explain, for a product nobody has installed.
- `a2m` as the documented command: shorter, and it is another project's package name on two registries.
- One crate per pack, with the browser engine as a module: simpler table, and the browser engine is large enough, and reused enough inside the pack, to earn its own crate.
- Folders `crates/kernel/` and `crates/packs/`: the name already says which side a crate is on, and a flat folder keeps every path short.

## Revisit if

A registry refuses `anything2mcp`, or the second pack shows that packs need something above the kernel's third layer.

## Spec sections

- [29-repository-crates-and-packaging.md](../spec/29-repository-crates-and-packaging.md)
- [41-packaging-and-release.md](../spec/41-packaging-and-release.md)
- [55-kernel-and-packs.md](../spec/55-kernel-and-packs.md)
