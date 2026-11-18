# 0242. The owner takes the recommended answer to every open question

**Status:** Accepted. Owner decision of 2026-11-18. The direction is the owner's; the wording is the lead agent's ([decision 0231](0231-the-owners-direction-is-written-in-exact-words-of-the-agents-own.md)). Only the owner changes it.

**Date:** 2026-11-18

## Context

By the evening of 2026-11-18 eighteen questions were waiting for the owner, gathered on one page ([open questions](../plan/open-questions-for-the-owner.md)), each with the state of the product today, a recommended answer, and a record with the reasoning. The owner read the state of the work and gave one direction for all of it: put every resource into the build, and proceed on the lead agent's best recommendations.

**What the lead agent inferred.** The owner did not answer the questions one by one. The lead agent reads the direction as taking the recommended answer to each question on that page, as the page stood when the owner spoke. That reading is the natural one, since the recommendations had just been put to the owner as a list; it is still a reading. Any one answer below is reversed by the owner saying so, and nothing built on an answer is hard to undo before the first release.

## Decision

Each open question is answered as its record recommended. Where the recommendation was "no", the rule or decision that stood in the way stands as it was.

### What the product may reach

1. **A session the person already has** ([0211](0211-whether-the-engine-may-ever-act-in-a-session-the-person-already-has.md)). The engine does not act in the person's everyday browser, by an extension or otherwise, and does not start a browser-based desktop application with that application's own data. A permission the person grants the engine in the system's own settings (accessibility, automation, and the like) is within rule 5: the person gave it, to this program, through the system's own door. The macOS desktop back end may be built on it.
2. **Applications that offer only pixels** ([0212](0212-whether-an-application-that-offers-only-pixels-is-ever-reached.md)). Not reached at run time. The scout may keep a screenshot as evidence at build time; nothing in a run reads a picture.
3. **A target someone else wrote** ([0213](0213-whether-a-target-someone-else-wrote-may-be-loaded-and-found.md)). It may be loaded, under the rules of [section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md): it is compiled, linted, and gated as any target, its evidence is never trusted, and the person publishes it afresh. This replaces the one phrase of [decision 0058](0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md) that said a target comes only from a package its developer built. The project still keeps no list of places where targets are published.
4. **A credential the machine holds for the person** ([0232](0232-whether-a-credential-the-machine-holds-for-the-person-may-be-used-through-its-holder.md)). It may be used through its holder, never read, in two cases: the system's integrated sign-in with the person's current logon, and a smart card or passkey with the person present. An SSH or signing agent is not used in the first release. Another program's credential store is never used.
5. **The name the product gives itself** ([0239](0239-whether-a-person-may-replace-how-the-product-names-itself-to-a-system.md)). A person may add to it, once that is built. Replacing it is not allowed in the first release.

### The network

6. **Asking many hosts or many ports** ([0224](0224-whether-many-host-or-many-port-looking-exists-in-the-first-design.md)). Does not exist in the first design.
7. **A network the person has not named** ([0225](0225-whether-anything-is-ever-sent-on-a-network-the-person-has-not-named.md)). Nothing is sent on it beyond the addresses the person named in a target. There the product reads only what the machine already knows.
8. **Findings about a network** ([0226](0226-whether-and-how-long-findings-about-a-network-are-kept.md)). Kept for the life of the run. They are kept longer only for a network the person saved, under the ordinary retention of traces.
9. **Being told by another machine** ([0233](0233-whether-being-told-by-another-machine-is-serving-it.md)). In the first release the product receives no webhook, and the engine on a person's second machine is not reached. Both are covered by [decision 0208](0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md).
10. **The rest of the network design.** The two slices that send nothing and only restrict where a request may go are built first. With questions 6 to 8 answered, the design of what follows them (hearing what a named network announces) may be taken up again, in small pieces, each of which states a limit on the product. If a piece is stopped by a safety check, it is left, and the owner is told.

### Writes and approval

11. **Standing approval** ([0215](0215-whether-a-person-may-pre-approve-an-action-that-leaves-nothing-to-read-back.md)). Allowed narrowly: only for an operation whose declared hazard is none and which is `irreversible` solely because nothing can be read back, only on one machine, and revocable. It is built after measuring how many actions it covers; if that number is small, it is not built.
12. **A person's own code in the proof of a write** ([0238](0238-whether-a-persons-own-code-may-take-part-in-proving-a-write.md)). Not in the first release.
13. **Rule 3's wording.** The rewording of 2026-11-18 stands: every write is recorded first and proven after, and a write that leaves nothing to read back is declared `irreversible`, approved as that, never sent twice, and never called verified.

### Building and releasing

14. **The accessibility permission on the macOS runner in CI.** It may be granted by writing to the runner's own permission settings as an administrator. This holds for the CI runner only, which is a machine made for the run and thrown away after it. No product code and no instruction to a user ever does the same on a person's machine: there the person grants the permission through the system's settings.
15. **The first version.** 0.1.0, published when the first hour works from a published package.
16. **crates.io.** The product is not published there. It is installed through pip and npm.
17. **The publishing setup** remains the owner's own act, as [the steps](../plan/publishing-setup.md) describe. No agent handles a registry token.
18. **The polkit action's identifier** stays as it is.

## Consequences

- The eleven Proposed records named above are accepted in the form each recommended, and say so in their status lines. The questions page keeps only what is still open.
- [SCOPE.md](../../SCOPE.md) changes where an answer changes what the product does or may do: rows that named an open question now name the answer.
- Work that waited on an answer may start: the macOS desktop back end and its CI job; loading a target someone else wrote; integrated sign-in and passkeys as sign-in providers; a screenshot in the scout's build-time evidence; the first network slices.
- AGENTS.md's packaging line is corrected: pip and npm, not crates.io.

## Alternatives weighed

- **Ask the owner to answer each question separately.** The owner had the page with each recommendation and chose to take them together. Asking again would have been waiting, which this repository does not do ([decision 0055](0055-agents-plan-build-and-merge-without-waiting-for-the-owner.md)).
- **Take the direction as covering only ordinary work, and leave the rule questions open.** Safer in the narrow sense, and it would have left the largest pieces of work blocked on answers the owner had just been shown. The recommended answers keep every rule's force: most are "no", and each "yes" is the narrow form the record argued for.
