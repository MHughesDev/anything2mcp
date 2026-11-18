# 0203. Two packs may each draft a file kind of one name, each for its own surfaces

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the rule in `a2m-files` for which pack may draft a file kind, stated in [decision 0179](0179-what-each-pack-drafts-so-that-a-scout-ends-with-reads-that-work.md) and the [design of `a2m-files`](../architecture/crates/a2m-files.md). It follows [decision 0202](0202-the-desktop-packs-control-has-a-name-of-its-own.md).

## Context

A file kind is registered for a surface kind: the compiler looks a file up by the pair of its kind's name and the kind of the surface it names. So two packs can each register a kind named `screen`, one for `web` surfaces and one for `desktop` surfaces, and both do ([decision 0198](0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md)). A screen is one word of the vocabulary for both.

The drafter's rule was written when one pack held each name. It allowed a pack to draft a kind only when every registration of that name was the pack's own. With the web pack and the desktop pack in one registry, as in the product, neither held every registration of `screen`. The registry refused the web pack's drafting rule, the web pack was not registered, and four product tests failed.

## Decision

A pack may draft files of a kind it registered for at least one surface kind. A pack that registered the name for no surface kind, while another pack did, may not. A kind nobody has registered yet is not refused when a rule is registered, and is checked again when the rule drafts, as before. The kernel's own kinds keep their rule: a read operation and a source may be drafted by any pack, and no other kernel kind by any.

## Consequences

- The web pack and the desktop pack both draft `screens/` files. A file's surface says which pack's rules compile it.
- What this rule does not yet check: that a file a rule drafts names a surface of a kind for which the rule's own pack registered the file kind. A rule that drafted a `screen` for another pack's surface would have its file compiled by the other pack's rules, and refused or accepted by them. No first-party rule does this. The check belongs in the drafter, at the point where a drafted document is placed, and is owed.
- The existing test still holds: a pack that registered nothing of a kind is refused, at registration and again when it drafts.

## Alternatives weighed

- **A file kind and folder of its own for the desktop pack** (`desktop_screens/`). It keeps the old rule, and it makes one word of the vocabulary into two for no reason a developer would see. Not taken.
- **Keep the rule and key drafting rules by surface kind as well.** That is the complete form of this decision, with the owed check built in. It changes the drafting rule interface, which is a public contract, and is left for the package that builds the check.
