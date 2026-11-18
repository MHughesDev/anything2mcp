# 0238. Whether a person's own code may take part in proving a write

**Status:** Proposed. Owner decision: it bears on rule three, how a write is proven, and on rule six and [decision 0002](0002-rust-first-other-languages-where-they-fit.md), what language is on the path of a run. Written by the [configurability review](../plan/configurability-review-2026-11-18.md) of 2026-11-18; the owner has not read it. Nothing here is in force.

**Date:** 2026-11-18

**Asks about:** rules three and six of [AGENTS.md](../../AGENTS.md#rules-that-never-bend); [section 605](../spec/6-running-it/605-packaging-and-release.md), languages and plug-ins; [section 206](../spec/2-kernel/206-the-compiler-and-the-plan.md), field types in the plan; [section 202](../spec/2-kernel/202-kernel-and-packs.md), packs and plug-ins.

## Context

A write is proven by reading the value back and comparing it with the intended value after normalization (rule three; [section 208](../spec/2-kernel/208-runs-the-ledger-and-resume.md), ledger rule 2). The normalization and the comparison belong to the field type. Today a field type is data: a target extends a built-in type in `types.yaml` (a unit, a step, a number of places, a date pattern, case and white-space rules, codes), and the kernel's own Rust code normalizes and compares by that data.

Some systems hold values no data of that kind can describe: a measurement with a vendor's own rounding, a code whose equivalences are a table with rules, an address written three ways. A developer meeting one today can only leave the write unverified. They would want to bring a normalizer of their own.

The provider contract lets a pack bring drivers, discoverers, sign-in providers, source readers, lint rules, drafting rules, and more, but not a field type's normalizer or comparison. Plug-ins, which section 605 lets be written in any language as separate supervised processes, are designed to bring drivers and the like. Section 605 also says everything on the path of a run is Rust, naming the drivers, the ledger, and policy, and the comparison that proves a write is the kernel's.

So the question is the owner's: whether "verified" may rest on a comparison the product does not hold.

## The ways through that stay inside the rules today

- **More data in a field type.** Most of the cases above are a rounding mode, a table of equivalent codes, or a pattern; each can be a field of `types.yaml` that the kernel's Rust code applies. This needs no decision, only design, and is where most of the need goes.
- **A normalizer as a compiled-in pack's Rust code.** A pack in this repository may register field-type behaviour if the contract gains that registration: it is reviewed with the product, and the path of a run stays Rust. This is a design decision of the contract (a public contract, so it needs its own record), not the owner's.

## The question for the owner

May a person's own code, from outside the product (a plug-in in another process and language, or a pack the person compiled in), normalize or compare a field type's values, so that a write whose read-back passes that comparison is called verified?

## Recommendation

No, for the first release; and yes later only under three conditions, should the owner want it: the code is pinned by digest and approved by a person as a provider is; the result of a write it proved is marked in the ledger and in the tool's result as "verified by a comparison the target brings", never as the product's own verified; and the kernel still checks what it can check by itself (the value read back is the one the system returned, the types agree). Until then, the two ways above are built first.

## Alternatives weighed

- **Treat a normalizer like any plug-in.** Section 605 already lets a plug-in driver run in any language. But a driver's answer is checked by the kernel's comparison; a normalizer is the comparison. Treating them alike moves the proof itself outside the product.
- **Let a person's code propose a normalized value, and the kernel compare it with its own.** Safer, and it needs the kernel to be able to normalize the value anyway, which is the case the person's code was for.
