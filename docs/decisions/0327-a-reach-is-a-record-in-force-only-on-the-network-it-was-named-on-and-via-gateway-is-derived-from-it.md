# 0327. A reach is a record in force only on the network it was named on, and `via_gateway` is derived from it

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Builds the part of slice N3 outside `a2m-actions`, on [decision 0223](0223-a-network-is-a-place-a-recognized-network-and-a-reach.md) and decision 0318.

**Date:** 2026-11-23

**Changes:** `a2m-netstate` (the fingerprint and its comparison, the DHCP server reader) and `a2m-provider` (the reach record and `NetworkPlace::of_in_reach`). Nothing in `a2m-actions`.

## Context

Decision 0318 says what a fingerprint is. Slice N3 needs it built, a stated comparison, the record of what a person named for a network, and the place `via_gateway`, which decision 0313 left underived because no gateway could be named.

## Decision

1. **The fingerprint** (`NetworkFingerprint` in `a2m-netstate`) is read from the preferred default route whose interface is up, and holds one digest per fact: the core (gateway hardware address, gateway address, subnet with prefix) and the secondary (DNS search domains, DHCP server). A digest is of the fact under a purpose name, so a stored fingerprint holds no address. It is not secret: a hardware address or an address in a small range can be found by trying every value, and the store's protection is what keeps it from a caller.
2. **The comparison** (`recognize`) is decision 0318's: every core fact stated by both sides and equal, and no secondary fact that both sides state differs, is `Same`; a stated difference is `Different(fact)`, even where another fact is missing; a core fact missing on a side is `Insufficient`, treated as a network not seen. Domains agree when they share one. What may change: this machine's address, lease times, name servers, a secondary fact one side lacks, a domain added or dropped while one remains.
3. **A default route through a tunnel gives an empty fingerprint.** A reach named at home is not recalled across a virtual private network.
4. **A reach** (`Reach` in `a2m-provider`) is a label, the fingerprint of the network when named, and two lists of entries (look, send), each a destination and, optionally, a gateway name the person chose. It is in force only where its fingerprint is recognized as `Same` as the current one.
5. **`via_gateway` is derived** by `NetworkPlace::of_in_reach`: a destination has the place `via_gateway` only when a reach in force has an entry for the same address or the same name (any case) with a gateway. Loopback stays `this_machine`. Two reaches in force naming different gateways for one destination give `unknown`. `NetworkPlace::of`, which knows no reach, never gives `via_gateway`, and its tests stand.
6. **Gaps, stated:** the fingerprint is of one address family, that of the preferred default route (IPv4 first), where decision 0318 says every family the interface has; the DHCP server is read on Linux from the lease files of the system's client (systemd-networkd, dhclient), and on Windows and macOS is `net_reader_not_built`, named as absent, so a reading there rests on the core alone; the wireless network's name is not read.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Make `ViaGateway` carry the gateway's name | `NetworkPlace` is a small `Copy` value that is ordered and listed; the name rides in `PlacedInReach`, which only the derivation that knows a reach returns |
| Store the facts in the clear | A stored reach would show addresses and a hardware address to anything that reads the store |
| Fingerprint every family at once | Two readings to keep equal for no case yet seen; one family is recorded and the gap named |

## Proof

`fingerprint.rs` and `reach.rs` tests: a reach named on one network is not in force on another; every core fact that differs is named; an unread core fact is not recognized; the known false match of one range and one virtual gateway address is held as a test; a reach and a fingerprint read back from storage the same and show no address.

## What `a2m-actions` must still do

Keep the reaches as a protected setting bound to the operating system's presence check, and offer the person-only actions to name, list, and withdraw a reach. No agent input may set a network, a destination, or a gateway.
