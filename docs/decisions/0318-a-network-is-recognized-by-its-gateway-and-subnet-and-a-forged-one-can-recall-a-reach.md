# 0318. A network is recognized by its gateway and subnet, and a forged one can recall a reach

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Refines the network bullet of [decision 0223](0223-a-network-is-a-place-a-recognized-network-and-a-reach.md), which stays Proposed as a whole; this record says what its fingerprint is whenever slice N3 builds it.

**Date:** 2026-11-23

**Changes:** the [network-awareness plan](../plan/network-awareness.md), part 1 ("Network") and part 2 ("Recognizing a network", "Spoofability, stated plainly"); hypothesis H-44.

## Context

Decision 0223 fingerprinted a network from the gateway's hardware address, the DHCP server identifier, the DNS suffix, the subnet, and on wireless the SSID and BSSID, and said a forged fingerprint "can only impersonate a network the person already trusted". [Research note 32](../research/32-how-stably-a-network-is-recognized.md) read how each signal is reached on each system and how each is forged. What it read directly: the Windows adapter record carries the gateway's address, the DHCP server and the DNS suffix with no elevation; the Windows Wi-Fi calls refuse the SSID and BSSID without the person's location permission; a macOS dump of the dynamic store carries the router and its hardware address. Its stability claims (what a reconnection or a renewed lease changes) are from documentation, forum reports, and memory, and nothing was run; they stay the open half of H-44. Its reasoning about the two kinds of forger rests on the design itself, not on an outside fact.

## Decision

1. **Core signals:** the default gateway's hardware address, the default gateway's address, and the subnet with its prefix, for each address family the interface has. A match needs every core signal equal in every family both sides have.
2. **Secondary signals:** the DNS suffix and the DHCP server identifier, where the system gives them. Compared only when both sides have a value. A difference makes the network not recognized; a value missing on one side is not a difference.
3. **Hints:** the SSID and BSSID, shown to the person in the naming dialogue and never part of the fingerprint. They are not read while the system's location permission is missing ([decision 0243](0243-the-machine-reads-its-own-network-state-and-sends-nothing.md)).
4. **Not signals:** the operating systems' own network identifiers, which are derived from the same facts or name a saved profile, and this machine's own hardware address. The Windows identifier may be shown as a label.
5. **Unreadable means unknown.** A core signal that cannot be read makes the network unknown. The gateway's hardware address comes from the system's neighbour table, which sends nothing; with no entry, the fingerprint is incomplete, and the product does not ask the network for one (rule 7).
6. **A reach says how it was recognized.** It records which signals were read and equal when the person named it, and the person is shown, with the reach, "recognized by gateway and subnet" or "by gateway, subnet, and DNS suffix".
7. **The forgery that matters is stated.** A copied fingerprint can recall a saved reach on a network the person never named, and a recalled place to send to can be answered by the forger's host. Recognition still sends nothing and raises no rung. The naming dialogue tells the person that a "may send" on an open link is no stronger than that link. Binding a "may send" to a host identity (a pinned certificate or key, or a device identifier) read at naming time, with a named stop on a mismatch, is an open point for a later slice; [decision 0225](0225-whether-anything-is-ever-sent-on-a-network-the-person-has-not-named.md), the owner's, is not touched.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep all six signals | The SSID and BSSID are behind a permission on two systems and the BSSID changes on every roam, so the fingerprint would split one network and be unreadable without a prompt |
| Use the system's own network identifier | Not independent of the core signals on Windows; names a configuration, not a network, on macOS and Linux |
| A secondary signal missing on one side counts as a difference | Linux cannot always read the DHCP server, and many networks have no suffix; it would split networks for no evidence |
| Send an address-resolution request when the neighbour entry is missing | It sends on a network the person may not have named (decision 0225) and guesses where the system said nothing |

## Proof

Slice N3, on the Linux namespace stand of note 32 part 6 (hypothesis H-44): the same router across a reconnect and a renewed lease is recognized; a changed gateway hardware address is new; a secondary present on one side only is recognized, and one that differs is not; an absent neighbour entry gives unknown and sends nothing; two networks with one range and one virtual gateway hardware address are recognized as one, which the test records as the known false match. Windows and macOS readers wait for the real-systems session.
