# 0320. What is kept and shown of other people's devices on a network

**Status:** Proposed. Written under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md), but it would change owner decision [0226](0226-whether-and-how-long-findings-about-a-network-are-kept.md), which only the owner changes, so nothing here is in force until the owner accepts it; 0226 holds as written. The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Would change:** decision 0226; [section 217](../spec/2-kernel/217-traces-evidence-and-repair.md) (a retention class for findings about a named network); [section 218](../spec/2-kernel/218-discovery-and-findings.md) (what a stored network finding keeps); [section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md) (what the redactor masks); the naming dialogue of the [network-awareness plan](../plan/network-awareness.md).

## Context

Decision 0226 keeps findings about a network for the life of the run, and longer only for a network the person saved, "under the ordinary retention of traces", and asked to be revisited when data-protection research came in. [Research note 34](../research/34-keeping-an-inventory-of-a-network-one-does-not-own.md) is that research. It is **Partial in a way that matters**: no legal text was opened; every legal claim is from a search summary, and it is not legal advice. So this record builds no rule on those claims. It sets out what the note proposes, for the owner to accept or not.

What the summaries say, as leads: an address, a hardware address, and an announced device name are each likely personal data about the device's owner; hashing or truncating one was not enough in one regulator's decision; shared-network terms commonly forbid scanning or probing; no source says who the "controller" is when a program on a person's machine, which sends nothing to its maker, lists other people's devices.

## Proposal

1. **Nothing about an unnamed network outlives the run, in any form.** Findings about a network the person has not named are held in memory for the life of the run and never written to the store or to a trace: every address, hardware address, announced name, and service banner, hashed or not. A hashed or truncated device identifier is treated as the identifier it stands for.
2. **A named network gets its own, shorter retention class.** Findings about a named network are kept under a class of their own, shorter than the trace default of 30 days unless the person sets a longer one, and the person can delete them at any time. A stored finding keeps the address, the kind of service, and the time; announced device names and hostnames are not kept by default. "Keep nothing for any network" stays the fallback, as decision 0226 already says.
3. **Other people's device names stay out of logs.** Device names, hostnames, and addresses of other people's devices on a network that is not named are registered with the redactor and do not appear in logs, traces, or error messages.
4. **The person is told once, when naming a network for looking,** that shared networks commonly forbid scanning, and that their statement that the network is theirs to look at does not replace the operator's permission.
5. **Capturing other people's traffic is a choice the product does not make,** recorded in [SCOPE.md](../../SCOPE.md) as a choice once accepted. The ladder of looking has no rung for it today.

## What this proposal does not change

- [Decision 0225](0225-whether-anything-is-ever-sent-on-a-network-the-person-has-not-named.md), also the owner's: nothing is sent on a network the person has not named beyond the addresses the person named in a target. Note 34's proposal that the ladder stop at reading local state and listening on an unnamed network is already met, and more strictly, by 0225 and the ladder's defaults, so it is not repeated here.
- Where a probe is recorded. Note 34 asks whether a probe to a host on an unnamed network is kept in the ledger by class and count only. Under 0225 the only such probe goes to an address the person named in a target, which is the target's own and not a third party's; the question returns only if 0225 changes.

## Open points

- The position of a program that runs only on a person's machine, under the common data-protection regimes, as controller or processor, and whether the household exemption covers a home network with guests. This needs a lawyer's reading, not a search: research brief 38.
- Every legal claim of note 34, checked against its primary text.

## Alternatives

| Alternative | Why not proposed |
| --- | --- |
| Keep 0226 as it is | It may be right; the owner decides. The proposal is narrower only where the leads point the same way |
| Keep nothing for any network | Loses "what changed since I last looked" on the person's own network; 0226 already holds it as the fallback |
| Hash identifiers and keep them | The one regulator decision in the summaries treated pseudonymised hardware addresses as personal data |
