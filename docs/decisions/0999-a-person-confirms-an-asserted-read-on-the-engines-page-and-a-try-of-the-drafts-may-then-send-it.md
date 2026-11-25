# 0999. A person confirms an asserted read on the engine's page, and a try of the drafts may then send it

**Status:** Accepted. Design decision, made by the worker on FT6d under the lead's brief. The owner had not read this record when it was written. It changes a public contract, the provider contract: `PrepareContext` gains `confirmed`, and `ControlPort` gains `confirmed_reads`, with a default.

**Date:** 2026-11-25

**Changes:** takes up the alternative of [decision 0392](0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md) that it kept as H-52, a person's confirmation on the engine's page that lets a draft's asserted read be tried; amends the table of point 5 of decision 0392 by one row; a new action, `assertion.confirm`; the provider contract (`a2m-provider`, `a2m-engine`); `a2m-actions`; `a2m-pack-web-ui`; spec sections [203](../spec/2-kernel/203-the-provider-contract.md), [301](../spec/3-packs/301-the-web-pack.md), and [502](../spec/5-security/502-defending-against-a-manipulated-agent.md); the [action catalog](../reference/action-catalog.md); the glossary. Carries out roadmap row FT6d, from field test 11, finding 64.

## Context

Field test 11 met a page whose list arrives by `POST` to an address that takes no write ([finding 64](../reviews/field-tests.md)). The person asserted the request, as decision 0392 says, and then had no honest step to a read that returns:

- `draft.try` stopped as `policy`, `page_write_refused`: under the drafts the observing tab lets no asserted read through, in any posture.
- `fixture.record` stopped as `fixture_read_failed`, for the same reason: a recording is a trial.
- `draft.ready` stopped as `publish_gates_not_passed`: the read had no offline test, since it had no fixture.
- A publish needs a passing offline test, and a published read acts only through a binding marked `verified`, which `binding.promote` sets on a trial's evidence.

So the only way through was to write a fixture and `status: verified` by hand for a binding that had never run, as the product's own proof of an asserted read does (`crates/anything2mcp/tests/projects/rows`). That is worse than the complaint H-52 names: the person vouches for a read nobody has seen work, and a verified mark is written that no trial earned.

Decision 0392 weighed this and kept it for later: "The person confirms each assertion on the engine's page as the capture ends, and a draft may then be tried. ... It needs a new stored statement and a second rule beside the published version's." The count H-52 asked for came back as the strong form of the complaint, so this record takes it up. Its point is not to loosen the gate. The person's word is still the only evidence that the request changes nothing; this record lets that word be given before a publish, on the engine's page, behind the presence prompt, and lets it cover one try's request and nothing more.

## Decision

### 1. A person confirms the drafts' asserted reads on the engine's page: `assertion.confirm`

`assertion.confirm` takes a target. It is a person's act: an agent's call is shown to the person on a page of the person surface and the agent is told `needs_person` with its link, as for a publish, a watch, and a capture ([decision 0163](0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md), [decision 0283](0283-watches-reach-an-agent-through-the-registry-and-their-person-confirms-each-on-the-page.md)). The press raises the presence prompt, and only a proof from that press, given for that act, carries it out, once, as the person. A caller who is not a person at the machine is refused `person_required`, and the handler refuses a call that reached it with no proof of presence.

The page names, for each read the drafts assert ([decision 0394](0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md)), in the words the publish page uses: the file kind and its entry, the surface and the address it leads to in each environment, the method, the path, each field the body may carry with its kind and the values its `enum` allows, and the sentence that the person's word is the only evidence that the request changes nothing. Then what the press does: a try of the drafts, in an environment of kind `test`, may let a page send that request, with a body that fits its closed schema, and nothing else changes; a publish still asks the person on its own page.

An agent may ask for the page, and nothing is stored until the person presses. A target whose drafts assert nothing is refused, `nothing_asserted`.

### 2. What is confirmed is exactly what the page showed, by its digest

A confirmation is a digest over the target's name, the file kind, the entry, the surface, the method, the path, every field with its kind and its `enum`, and the address the surface leads to in each environment of the drafts. The asking pins the digests of the reads shown; the press compiles the drafts again, and when the reads it finds are not the ones pinned, nothing is stored and the act is refused, `assertion_changed`: the person confirms what they read. A confirmed read whose method, path, body, or address changes afterwards has another digest, so the confirmation no longer covers it and the try stops again; the person is asked again.

The confirmations are kept in the engine's store, by target, as the addresses a person registered are ([decision 0413](0413-the-addresses-a-person-registered-for-a-target-are-those-given-to-target-add-and-those-of-every-version-a-person-published.md)); at most 64 per target, the oldest dropped. No target file holds them and no action but this one writes them, so an agent that writes the drafts, `read_asserted: true` included, confirms nothing. The act's policy decision is recorded as every action's is, with the person whose presence was proved.

### 3. A try of the drafts hands the engine the reads its person confirmed

When the kernel keeps the drafts' compiled target for a trial, in `draft.try` and in the trial of `fixture.record` (and so of `draft.ready`, which calls both), it compares each read the drafts assert with the target's confirmations and keeps, beside the compiled target, the entries a confirmation covers, by file kind, surface, and entry. The engine asks for them through `ControlPort::confirmed_reads` and hands them to the driver in `PrepareContext::confirmed`. A published compiled target is handed none: it has its own rule, the published version's. A `ControlPort` that does not answer hands none, so nothing that existed before changes meaning.

### 4. The web pack lets a confirmed read through only in a try in an environment of kind `test`

The table of point 5 of decision 0392 gains one row and keeps the others:

| Where the read runs | An asserted read is let through |
| --- | --- |
| A live run of a published version | Yes |
| A try or a shadow of a published version, in an environment of kind `test` | Yes |
| A try under the drafts, in an environment of kind `test`, of an entry its person confirmed on the engine's page | Yes, that entry alone |
| A try under the drafts otherwise | No. The stop is `policy`, `page_write_refused`, and its next step names the entry and says to ask the person with `assertion.confirm`, or to publish |
| Discovery, a scout's exploring | No |

What a try may send is the page's own request that matches the confirmed entry by every rule of decision 0392's point 5: method and origin, the path segment for segment, no method override, and a body of the declared media type that fits the closed schema. The trial runs in the sandbox posture, where the handle refuses every step that commits. Every other request the page makes is failed before it leaves the browser, as before, and stops the read: a trial that meets a write stops.

### 5. Nothing else moves

- A binding is marked `verified` only by `binding.promote` on a trial's evidence, as before; this record adds no way to mark one, and the trial it now allows is that evidence.
- The publish page still names each asserted read with the sentence, and a publish is still the person's press.
- The drafter never writes `read_asserted`, an `enum`, or a confirmation.
- The HTTP pack's asserted reads are not let through under the drafts: it does not read `confirmed`. Whether it should is left to the first application that needs it.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep decision 0392: no try before a publish | The person must vouch for a read nobody has seen work, and the only way to a publish is a `verified` mark written by hand for a binding that never ran (finding 64) |
| Let `read_asserted: true` in a draft be enough for a try in an environment of kind `test` | A file any agent can write would then send a request by a method that may write. The person's word must be the person's act, not a line in a file |
| Confirm on the capture's page as the capture ends, as decision 0392 first put it | At the end of a capture nothing is asserted yet: the drafts wait in the holding folder with the question, and the enum the person writes comes after. A confirmation of the drafts as they are when the person releases them is the same act at the moment it can be made |
| Mark the trial by compiling the drafts under a version other than the draft version, which the packs already read as published | It would claim the drafts were published, and every rule that reads a version would be loosened with it, a sign-in's among them |
| A bare flag "the drafts' asserted reads were confirmed" in the contract | It holds only while the kernel checks every asserted read of the compiled target; a list by entry lets a pack check its own, and a pack that does not read it is unchanged |
| A confirmation by the entry's name alone | A changed body or address would stay confirmed. The digest is over what the page showed |
| Store the confirmation in a target file the press writes | Any agent can write that file too |

## Proof

- The test application's case 39 through the product, on a virtual screen: a capture of two visits to `/cases/rows-by-post` drafts its held page request and its table; the person releases them, asserting the request with its `enum`; the try stops and names `assertion.confirm`; the agent asks for the confirmation, and before the press the try still stops; the person presses; the try reads the open rows the application holds; `draft.ready` records the fixture, promotes the binding on that trial, passes the gates, and asks for the publish; the person publishes; the call reads the rows. The application heard searches and no archive, and stored nothing. No file was written by hand but the person's release of the held drafts.
- Faults, through the product: the prompt declined stores nothing and the try still stops; a press after the drafts changed is refused `assertion_changed`; a confirmation made for one body, and one made for one address, cover neither when the body or the surface's address changes.
- Faults, in the web pack against the stand: a confirmed entry is let through in a try in an environment of kind `test` and in no other place (not live, not in an environment of another kind, not in discovery); a request of the page whose body does not fit the confirmed entry stops; a request of another page that writes, met in a try with every read confirmed, stops and reaches nothing.

## Revisit if

A person confirms a read that wrote; the provider contract gains a posture that tells a try from a shadow ([decision 0249](0249-the-http-pack-reads-in-bounded-work-and-sends-nothing-that-may-write-outside-a-live-run.md)), which would let this row name a try alone; or the HTTP pack meets an application whose asserted read must be tried before its publish.
