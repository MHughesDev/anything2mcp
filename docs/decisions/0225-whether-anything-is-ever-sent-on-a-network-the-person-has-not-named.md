# 0225. Whether anything is ever sent on a network the person has not named

**Status:** Accepted in the form its recommendation gave, by owner decision [0242](0242-the-owner-takes-the-recommended-answer-to-every-open-question.md) of 2026-11-18: nothing is sent on a network the person has not named beyond the addresses the person named in a target; there the product reads only what the machine already knows. Only the owner changes it.

**Date:** 2026-11-18

**Rests on:** the reach idea of [decision 0223](0223-a-network-is-a-place-a-recognized-network-and-a-reach.md) and [docs/plan/network-awareness.md](../plan/network-awareness.md).

## Context

Decision 0222 says the product works through access the person already has, on networks the person is entitled to look at, and that "how the product knows which networks those are, and what a person must say before anything is sent to an address they did not name, is the first question for the design." A laptop moves between a home network, a café, and a client's office. The product recognizes a network it has been told about; on one it has not, it must decide what, if anything, it may send.

## The question

On a network the person has not named, is anything ever sent beyond rung 0 — any multicast query, any probe, any packet to an address the person did not name in a target?

## The lead's recommendation

**No.** On a network the person has not named, reach is rung 0 only: the product reads what the machine already knows and sends nothing to any address the person did not name in a target. Raising the ladder on a network is a person's presence-checked act of naming that network to their reach (looking, and separately sending). This is the safest default, it keeps the product off a café's or a client's network until the person says it is theirs to look at, and it holds rule 5 and decision 0222's entitlement point without the product ever having to judge whether a network is the person's.

A target the person already added keeps working on any network: its declared surface addresses are a reach the person already named by adding the target. What the default refuses is the product reaching *beyond* those named addresses on a network the person has not named.

## Alternatives weighed

- **Send the protocols' own multicast queries on any network, because they are the network's own questions and politely bounded.** It would let the scout survey a café network without the person first naming it. Rejected for the default: it sends on a network the person has not said is theirs to look at, which is the entitlement line decision 0222 draws; the person naming the network is a light act (one presence prompt) and is where the right belongs.
- **Send on the home network only, recognized by fingerprint, without a prior naming.** Rejected: recognition is spoofable, and letting recognition alone authorize sending is exactly what decision 0223 forbids; the person names the network once, then recognition recalls it.

## Revisit if

The owner decides; or the first-hour usability work shows that requiring a person to name their home network before the scout surveys it is a wall people do not get over, and a narrower default is warranted.
