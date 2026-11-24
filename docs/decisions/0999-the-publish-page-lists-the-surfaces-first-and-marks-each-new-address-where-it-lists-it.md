# 0999. The publish page lists the surfaces first, and marks each new address where it lists it

**Status:** Accepted. Design decision, made by the worker who built the page half of package 9.3u, under the lead's brief. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** [`a2m-actions`](../architecture/crates/a2m-actions.md), whose `ActSurface` gains `new_in` and `new_here` and whose root exports the fixed words `NEW_PLACE`; [`a2m-local`](../architecture/crates/a2m-local.md), whose act page puts "Where it leads" before the review and marks each such address. It supersedes the order of [decision 0264](0264-the-publish-page-renders-the-typed-review-sign-ins-first-from-one-keeper-call.md) only where that order put the surfaces after the tools; the sign-ins still come before the tools. It leaves the review summary of `a2m-files`, the comparison of [decision 0413](0413-the-addresses-a-person-registered-for-a-target-are-those-given-to-target-add-and-those-of-every-version-a-person-published.md), and the kernel's line that names each new place as they are.

## Context

Decision 0413 compares the places a draft reaches with those a person registered for the target, and carries out a publish that reaches a new one only from the publish page. The page named the new place among the kernel's lines, after the sign-ins, the tools, and the list of surfaces. Section 502, rule 2, asks that the person see a widened reach before they confirm it; a line at the foot of a long page, apart from the address it is about, is easy to pass over.

## Decision

1. **The surfaces first.** On the publish page, "Where it leads" comes first: every surface, its kind, and its address in each environment. Then the review (the sign-ins, then the tools), then the kernel's lines, then the checks' warnings. An act with no review is laid out as before.
2. **Each new address marked where it is listed.** The registry states, for each surface, the environments in which its address is at a place no person registered for the target (`ActSurface::new_in`), and whether its default address is (`new_here`), from the same comparison and the same places as `version.publish` makes. The page prints, right after each such address, `NEW_PLACE` in bold: "a new address: no person registered it for this target, and publishing registers it". No other address carries a mark, and a draft that reaches no new place shows none.
3. **The kernel's line stays.** "New address: the drafts reach …" is still the first of the kernel's lines, so a page or a client that reads only `details` is told too.

## What was rejected

- **Marking the place, not the address.** The registry could have handed the new places alone, and the page matched each address against them. The page would then hold the rule of what a place is, which is the registry's (decision 0413, point 1); a page holds no rule.
- **A section of its own for the new places.** It would say again, apart, what the list of surfaces already says, and leave the address unmarked where the person reads where each surface leads.
- **The review summary in `a2m-files` carrying the mark.** The compiler cannot know what a person registered; the registry fills what the compiler cannot, as it does for a sign-in's addresses, and the surfaces are the registry's type, not the summary's.

## Consequences

- `a2m-local`'s `the_publish_page_lists_the_surfaces_first_and_marks_a_new_address_there` holds it through the page the surface serves: a person publishes the `example` target; an agent's publish of a draft that reaches no new place is shown with the surfaces first and no mark; one that adds a surface at another place is shown with that surface's address marked, before the tools, and the registered address unmarked.
- `the_publish_page_shows_where_each_surface_leads_its_notices_and_every_warning` holds the mark's words and that one address is marked of three.

## What would reopen it

A page where the surfaces are many enough that listing them first hides the tools; or a person act other than a publish that widens a target's reach.
