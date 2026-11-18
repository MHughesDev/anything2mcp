# 0223. A network is a place, a recognized network, and a reach

**Status:** Proposed. Design decision: any agent may accept it by building the part it names with the tests it names, in a record that says so. Written by the first network-awareness design wave of 2026-11-18. Nothing here is in force. No rule is changed by it; it carries out [owner decision 0222](0222-the-product-is-aware-of-the-network-its-machine-is-on.md).

**Date:** 2026-11-18

**Would change:** [section 105](../spec/1-product/105-core-concepts.md), [section 106](../spec/1-product/106-what-it-latches-onto.md), [section 201](../spec/2-kernel/201-archetypes-and-modes.md), [section 203](../spec/2-kernel/203-the-provider-contract.md), [section 218](../spec/2-kernel/218-discovery-and-findings.md), [section 402](../spec/4-building-tools/402-the-scout-and-who-does-what.md), [section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md), and the technical reference. The full design is [docs/plan/network-awareness.md](../plan/network-awareness.md).

## Context

[Decision 0222](0222-the-product-is-aware-of-the-network-its-machine-is-on.md) asks that the system be generalized to being network aware: "a website" one instance of a general idea. Today the kernel knows a target, its surfaces, and each surface's address as a string; a run's reach is a `Destinations` set compared as written; and four separate checks (the web origin guard, the destination allow-list, loopback-as-a-literal-IP, and the named proxy) each answer, in their own words, a question about where a thing is and how the machine reaches it.

## Decision

One small kernel idea, three nouns.

- **Place** is a typed property of every surface and every destination: where it is and how this machine reaches it (`this_machine`, `local_network`, `public_internet`, `via_gateway(name)`, `unknown`). It is derived from the address and the machine's own state, never taken from an input (section 502). It is checked everywhere a destination is checked. `unknown` is a stop where a place decides something, never a guess.
- **A network** is a recognized thing with an identity: a fingerprint of facts the machine already knows (gateway hardware address, DHCP server id, DNS suffix, subnet, SSID/BSSID). Recognition is spoofable, and that is acceptable only because recognition never authorizes sending; it only recalls what the person already said about a network they named.
- **A reach** is two sets the person named per network: places that may be looked at, and places that may be sent to. Consent binds to a network, not the machine. On a network the person has not named, reach is rung 0 only (see [decision 0225](0225-whether-anything-is-ever-sent-on-a-network-the-person-has-not-named.md)).

"A website" is one instance: its address derives a place, and the origin guard, the allow-list, loopback-as-literal, and the proxy become place-and-reach checks that already exist under other names. The payoff is that four hand-written refusals become one derived property checked in one place.

The place/reach types live in `a2m-core`; the derivation and the reach test in `a2m-provider` beside the path check; the reach ledger and the rung-0 inspection actions in `a2m-actions`; the network discoverers and surface kind in a new pack a2m-pack-net. Place must be checked wherever a destination is, which is kernel; knowledge of the network stack is a pack, by rule 8.

## Consequences, if accepted

- `Destinations` (a public contract, section 203) grows to carry place; the finding subject gains a `network` tag and the `address` tag a place; the provider manifest's effects name the rung and places a provider may reach. Each is a change to a public interface and takes its own record when built.
- Every pack that reaches over a network declares a place for each surface. The proxy owed by decision 0208 and the `localhost`/device-name rules of decision 0214 fall out of this idea instead of being special cases (hypothesis H-46).
- The ladder of looking, whether anything is sent on an unnamed network, and whether network findings are kept are the owner's, in decisions [0224](0224-whether-many-host-or-many-port-looking-exists-in-the-first-design.md), [0225](0225-whether-anything-is-ever-sent-on-a-network-the-person-has-not-named.md), and [0226](0226-whether-and-how-long-findings-about-a-network-are-kept.md). This record is the vocabulary they rest on.

## Alternatives weighed

- **No new concept, only more discoverers in the existing packs.** Add a network discoverer to the device and HTTP packs and keep address-as-string. Rejected: it does not bind consent to a network, does not generalize place, leaves the laptop-moves problem unsolved, and keeps the section 502 reasoning scattered across four checks. It is the smallest change and the least of what the owner asked for.
- **A general network layer under every pack** that mediates all egress. Rejected: a large new subsystem, against "prefer a small general idea that makes three special cases disappear". Place/reach is the small idea; it needs no new layer, only a property on the destination the kernel already checks.
- **Place asserted in the target file** rather than derived. Rejected by section 502: an input or a file an agent can write must not change where a request goes. Place is derived; the person's only lever is reach, through the presence-checked channel.

## Revisit if

The derivation of place proves unreliable on one operating system (hypothesis H-43); recognition proves too unstable or too forgeable to be worth it (H-44); or re-expressing the four existing checks over the type proves larger, not smaller (H-46).
