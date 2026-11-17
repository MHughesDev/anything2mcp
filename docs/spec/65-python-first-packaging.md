# 65. Python-first packaging

The way in is `pip install anything2mcp`. This section says what that installs, what stays optional, and how an agent learns what is missing. It refines [section 41](41-packaging-and-release.md); the pip package still ships the Rust binary with a thin Python client, as decision 0025 says.

## Four layers

| Layer | What it carries | How it arrives |
| --- | --- | --- |
| The base | One Rust binary with every first-party pack compiled in and switched off until enabled; a thin Python client | `pip install anything2mcp` |
| What a pack needs from outside | A browser for the web pack; a database client library where one is not built in | Used where already installed. Fetched only by an explicit install action, never during a run |
| Provider packages | Plug-ins published by this project or anyone else | A pip package of their own, found through a Python entry point; or an extra, `anything2mcp[name]`, that names one |
| Managed components | Outside programs a provider drives, such as an analysis engine or its runtime | Never bundled. Detected, identified by digest, and reported when missing or unknown |

## Why first-party packs are compiled in

- The browser is not bundled, so the binary stays small. The target is a wheel under 25 MB with every first-party pack; hypothesis H-28 measures it.
- One binary is one version, one audit, and one thing to sign.
- A pack that is switched off costs nothing at run time and cannot be reached.
- Nobody meets a missing-piece error for a common case.

What is heavy, rare, or under a licence that should not be forced on everyone stays outside as a provider package.

## Enabling

A pack or provider is in one of four states, and the action `provider.list` shows which.

| State | Meaning | To move on |
| --- | --- | --- |
| Not installed | The runtime does not have it | Install its package |
| Installed, off | Present and unreachable | Enable it in settings. On one machine, enabling a provider that launches processes or reaches the network is a protected setting |
| On, not ready | Enabled, but something it needs is missing | The listing names the missing thing and how to get it |
| Ready | Its health check passes | Nothing |

`doctor.run` checks every enabled provider and prints a fix for each problem, as it already does for the browser and the key store.

## Provider packages and Python

A provider package may be written in any language. Python is the first language with a small kit for writing one, because that is what most people integrating a system will reach for.

- The package declares an entry point in a group the runtime reads. The entry names the provider's manifest and the command that starts it.
- The runtime starts it as a separate process and speaks the plug-in protocol to it ([section 56](56-the-provider-contract.md)). No Python runs inside the engine, and no engine logic runs in Python.
- Rule six is unchanged: everything on the path of a run is Rust, except a plug-in, which is a declared, supervised process.

## Names

The package, the crate, the import, and the command are all `anything2mcp`, and `a2m` is a short name for the command. There are no alias packages under earlier names ([decision 0056](../decisions/0056-one-name-everywhere-anything2mcp.md), [section 41](41-packaging-and-release.md)).

## Nothing phones home

Unchanged from section 41. Listing providers, checking for components, and `doctor.run` make no outside connection. An explicit install action is the only thing that fetches, and it says what it will fetch and from where before it does.

## Open points

- The exact entry-point group and the shape of an extra that names a provider package.
- Whether a provider written in Rust, outside this repository, is loaded as a process like any other plug-in. Almost certainly yes: one way in.
- Research brief 28, a wheel that carries the binary and the Python client, is still to run.
