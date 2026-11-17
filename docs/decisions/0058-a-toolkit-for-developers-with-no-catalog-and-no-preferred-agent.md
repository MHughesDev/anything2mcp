# 0058. A toolkit for developers, with no catalog and no preferred agent

**Status:** Accepted. Owner decision, from the owner's answers of 2026-11-17.

**Date:** 2026-11-17

## Context

The long-term plan asked the owner who the product is for, whether ready-made tool definitions for real products should be published, which AI clients matter, what the first real system is, and how far reverse engineering and devices are in scope.

## Decision

1. **The user is a developer.** A person or an agent picks the product up the way they pick up any library: one install, then they point it at a system they are entitled to use and build tools over it. It is used as an SDK, or set around existing software.
2. **No public catalog.** The project publishes no tool definitions for real products and hosts no place to share them. A target is a folder. A developer keeps it in their own repository and loads it from a path or from a package they built themselves. Rule 9 of AGENTS.md stands: this repository names no real application.
3. **No preferred agent.** Any client that speaks the protocol is served alike, whoever made it, a home-built runtime included. Nothing in the kernel knows a client's brand. Conformance is tested against the protocol, with at least two unrelated clients. A client's quirk, where one must be handled, is data in a profile.
4. **Proven on test systems; real systems at the end.** Every kind of system is proven against a test stand in this repository. Real systems are tried with the owner once the core works.
5. **Test stands are as real as is cheap.** A stand imitates the part of its kind that makes the kind hard, and nothing else. The suite is kept small on purpose: tests secure the system and do not advance it. The rules that make a test count do not loosen: a write is proven by asking the stand what it stored.
6. **Reverse engineering is in scope.** The product automates the work of finding out what a system can do: reading what it publishes, watching it in use, and, through optional providers, decoding and decompiling its artifacts. [Section 67](../spec/4-building-tools/402-the-scout-and-who-does-what.md) says what runs by itself. The limits already decided stand: discovery cannot write, a finding cannot run, and nothing gets past a sign-in, a licence check, or a device's protection ([0053](0053-rea-is-studied-for-method-and-used-only-as-an-optional-provider.md), [0054](0054-devices-through-their-own-interfaces-and-firmware-through-the-devices-own-update.md), rule 5).
7. **Breadth is the aim.** Software, services, data, and devices with their firmware are all in scope, each arriving as a pack that clears the bar in [section 55](../spec/2-kernel/202-kernel-and-packs.md).

## Consequences

- The first release is judged by a developer's first hour, not an administrator's. The company-server profiles remain designed and come later.
- Client tests in the MCP server spike use whichever unrelated clients are at hand; no client is named as the target.
- The test application family grows by one small stand per pack, and the [test application plan](../plan/test-application.md) keeps its "smallest thing that shows the hard part" rule.

## Alternatives considered

- A public catalog of definitions: the fastest way for a newcomer to get value, and a permanent duty to track products the project does not control, under their owners' terms.
- Certify against named clients: clearer claims, and a core that drifts toward whichever client is tested most.

## Revisit if

Developers ask, repeatedly, for somewhere to share targets.

## Spec sections

- [01-summary.md](../archive/spec-2026-11/01-summary.md)
- [53-the-universal-runtime.md](../archive/spec-2026-11/53-the-universal-runtime.md)
- [67-the-scout-and-who-does-what.md](../spec/4-building-tools/402-the-scout-and-who-does-what.md)
