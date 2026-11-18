# 0239. Whether a person may replace how the product names itself to a system

**Status:** Proposed. Owner decision: it bears on rule five, operating openly, which forbids imitating another application's client and hiding that the engine is automation. Written by the [configurability review](../reviews/configurability-review-2026-11-18.md) of 2026-11-18; the owner has not read it. Nothing here is in force.

**Date:** 2026-11-18

**Asks about:** rule five of [AGENTS.md](../../AGENTS.md#rules-that-never-bend); [section 103](../spec/1-product/103-design-principles.md), principle 9, that the system identifies itself as automation; [section 501](../spec/5-security/501-security-model.md), silent misuse; [section 226](../spec/2-kernel/226-configuration.md), row 11 of what no setting can do.

## Context

The product names itself in what it sends. The HTTP pack sends the header `User-Agent: anything2mcp/<version>`; nothing a person sets can change it or add to it. The design states why: a system it reaches should be able to tell that automation is calling, and limit or refuse it.

People who run the product inside an organization meet two needs the fixed name does not meet:

- **Adding.** An organization's gateway, or a service's terms, may ask automated clients to say who to contact: an address or a page beside the product's own name. Adding it keeps the product's name in front, says more and hides nothing. The review counts this as consistent with rule five and as a design matter (Not yet, small): a setting whose value is appended to the product's own name, checked to be plain printable text of bounded length, with the product's name and version always first.
- **Replacing.** An organization's gateway may admit only clients registered under the organization's own name for this use, and refuse every other name, the product's included. To pass, the product would send the organization's registered name instead of its own.

Replacing is not imitating another application: the name is the person's organization's own registration for this very use. But it removes the product's name from what the system sees, and rule five's purpose, that a system can tell automation is calling, would then rest on the organization having registered it as such.

## The question for the owner

May a person replace the name the product sends with their organization's own registered name for this use of the product, never another application's?

## Recommendation

Not in the first release, which serves one person's machine ([decision 0208](0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md)). Adding is built first. If the owner allows replacing later, it is a protected setting of the installation alone, written by an administrator; the audit records every run that used it; and the replaced name still ends with the product's own (`<registered name> anything2mcp/<version>`), so that the system can tell automation is calling, unless the owner rules that even that may go.

## Alternatives weighed

- **Allow replacing as an ordinary setting.** It gives the most reach and makes the product's honesty depend on each person's choice of name, which is what rule five exists to prevent.
- **Never allow it, by rule.** Simple. It leaves a person whose organization admits only registered names unable to use the product there at all, which the owner may judge a fair price or not.
