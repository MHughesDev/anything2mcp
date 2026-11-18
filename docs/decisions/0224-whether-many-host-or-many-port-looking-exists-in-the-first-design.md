# 0224. Whether many-host or many-port looking exists in the first design

**Status:** Accepted in the form its recommendation gave, by owner decision [0242](0242-the-owner-takes-the-recommended-answer-to-every-open-question.md) of 2026-11-18: asking many hosts or many ports does not exist in the first design; the rule stands as it was. Only the owner changes it.

**Date:** 2026-11-18

**Rests on:** [decision 0222](0222-the-product-is-aware-of-the-network-its-machine-is-on.md) and the ladder of looking in [docs/plan/network-awareness.md](../plan/network-awareness.md).

## Context

The design proposes a ladder of looking: rung 0 (what the machine already knows, nothing sent), rung 1 (listen to announcements), rung 2 (ask the announcers' own multicast questions), rung 3 (ask one named host one thing). Rung 4 is asking many hosts or many ports — a sweep of an address range or a port range. This is the rung that separates an inventory tool from a reconnaissance scanner, and the thing a café's or a client's security team would refuse. Rungs 0-2 already find what announces itself and what the machine already knows.

## The question

Does rung 4 exist at all in the first design, and if so in what narrow form, and on whose word?

## The lead's recommendation

**Rung 4 does not exist in the first design.** Rungs 0-3 are an honest inventory: they read what the machine holds, hear what announces itself, ask the protocols' own questions, and probe one named host the person put in reach. None of that sweeps. If the owner wants any form of rung 4 later, the only form that stays on the right side of rule 5 is:

- Probing **only** hosts already found by rungs 0-2 (the neighbour table, the announcements) — never an address a host had no prior sign of.
- A small **fixed set of declared ports**, never a range.
- Hard rate and count caps, a time limit, and the product's honest client string.
- The person's **explicit word per network, each time** — never a standing setting.
- Never discovering a host by trying an address it had no prior sign of.

Any form that sweeps a range to find hosts is refused outright, whatever the setting.

## Alternatives weighed

- **A bounded sweep of the local subnet, off by default, behind the presence prompt.** It would find hosts that announce nothing (many devices do). Rejected for the first design: it is a scanner by any definition, hypothesis H-45 has not shown it is needed, and it is exactly what a shared network's owner refuses. Held for the owner.
- **Rung 4 never, in any release.** Simplest and safest; loses the silent devices permanently. The lead leans here but leaves the door the narrow form above describes, for the owner.
- **Rung 4 as a plug-in, off by default, an administrator approves.** Possible later; still the owner's to allow, because it is still a scan.

## Revisit if

The owner decides; or hypothesis H-45 shows rungs 0-2 miss most of a real network and a narrow rung 4 is the only way to a usable inventory.
