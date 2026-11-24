# 0999. The addresses a person registered for a target are those given to `target add` and those of every version a person published

**Status:** Accepted. Design decision, made by the worker who built package 9.3u, under the lead's brief. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** [`a2m-actions`](../architecture/crates/a2m-actions.md), which gains `src/origins.rs`, the failure code `address_unregistered`, and a check in `version.publish`; [spec section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md), rule 2, which says what "the origins a person registered" are. It leaves the page of a publish, in `a2m-local`, and the review summary, in `a2m-files`, as they are.

## Context

Section 502, rule 2, says that a draft cannot widen a target's reach by itself: a surface or a destination outside the origins a person registered for the target needs that person's confirmation. Nothing registered an origin, so nothing compared one. The line of the Build Order said what is registered: the address the person gave `target add`, and asked for a record if that is not what a person gives.

It is not all a person gives, and it is not always a person's. `target add` asks for no proof of presence: an agent may call it, and a target written by hand, as every test project is, never passes through it. What a person gives with proof is a publish: a command that passes the presence prompt, or a press on the publish page, which shows every surface of the version and where it leads.

## Decision

1. **What is registered.** For each target: the place of the address given to `target add`, when one is given; and every place of every version a person published, added when the publish is stored. A place is an address's scheme and authority, lower-cased, with no path, so that another path at the same place is the same place; an address with no scheme, such as a folder or a program, is its own place. They are kept in the kernel's own table of installation facts, under the target's name, and only added to.
2. **What is compared.** At `version.publish`, every place the compiled target's surfaces lead to, in every environment it declares, against those registered. A target with nothing registered yet compares nothing: its first publish registers what it publishes, and the person who publishes it is shown every surface and where it leads.
3. **What a new place needs.** A publish that reaches a place not registered is carried out only from the publish page, which names each such place first, in the kernel's own sentence ("New address: the drafts reach …, which no person registered for this target. Publishing registers it."). A publish from the command line stops before the gates run, `policy`/`address_unregistered`, naming each place, and says to ask for it on its page with `publish request`. A publish counts as from the page when its proof of presence was given for the page's act. A draft that reaches no new place publishes from either, with no further question.

## What was rejected

- **Only the address given to `target add`.** A target written by hand would have nothing registered and could never be compared, or would need the page for every publish for ever; and a target added by an agent would register the agent's address as if a person had given it.
- **Every first publish through the page.** It would make a person's first publish of a hand-written target, from the command line, a question that names every address as new, which says nothing the publish page does not already say.
- **A new input that lists the confirmed addresses.** It would change the schema of `version.publish`, and every face generated from it, to carry what the act's proof of presence already says: that the person pressed on the page that named them.

## Consequences

- `crates/anything2mcp/tests/local_engine.rs`, `a_draft_that_reaches_a_new_address_is_published_only_from_the_page_that_names_it`, holds the rule: a target added at the test application's address; a draft that adds a surface at another; the command's publish stops, naming it, and publishes nothing; the page names it and a press publishes; a later draft that adds none publishes from the command line.
- The page shows the new place among the kernel's lines, after the review and the surfaces. That the page shows surfaces first, and marks a new place where it lists the surfaces, is the half of package 9.3u that belongs to `a2m-files` and `a2m-local`, and is not built.
- A sign-in's destinations are held by decision 0248, and are not compared here.

## What would reopen it

A target whose places move between versions as a matter of course, such as one whose test environment gets a new port each time; or a front door that carries out a person's act other than through the page.
