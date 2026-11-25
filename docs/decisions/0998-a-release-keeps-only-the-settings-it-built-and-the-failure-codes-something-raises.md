# 0998. A release keeps only the settings it built and the failure codes something raises

**Status:** Accepted. Design decision, made in the design review round by lens 7, less ([report](../reviews/less-review-2026-11-25.md), findings 7, 8, and 11). The owner had not read this record when it was written. It refines point 1 of [decision 0437](0437-what-a-release-publishes-is-kept-and-every-later-build-is-checked-against-it.md), as that record's point 5 already does for actions. Built by package LS2.

**Date:** 2026-11-25

**Changes:** `tools/contracts.py` (the settings it keeps); `tools/failure_codes.py` (a check); `crates/a2m-provider/src/codes.rs` (two codes removed); the technical reference's table of failure codes, regenerated; the [roadmap](../plan/roadmap.md) and [the line](../plan/build-order.md) (package LS2).

## Context

Decision 0437 keeps, from each release, what people and their agents will depend on, and fails any later build that removes or changes it. Its point 5 keeps only the built actions, so that an action not built "may still change its inputs before it is built". Point 1 keeps "the settings keys, each with its type", and the whole failure-code table, with no such limit.

What that freezes at the first release, read from the tree on 2026-11-25:

- **55 of the kernel's settings are not built** (`not_built` in `crates/a2m-actions/src/settings/schema.rs`; section 226 marks 41 of its 83 rows). A value for any of them is refused by name today. `settings(root)` in `tools/contracts.py` keeps every key the schema declares, so each of the 55 would be kept with a type nobody has used: `store.database`, `server.listen`, `runners.certificate_hours`, `locale.messages`, and the rest. Renaming or retyping one when its part is built would be a listed break.
- **Two failure codes are declared and raised by nothing**: `signature_invalid` and `handshake_failed`, both of `a2m-provider`, both for the plug-in host that is not built. A search of every crate's code outside its `failure_codes!` table and its tests finds neither constant and neither string. The 424 others of the 426 are each referred to by the code that raises them or by a pack's manifest.

- **13 of the packs' 28 settings are not kept at all.** `settings(root)` reads the manifests at `crates/*/anything2mcp-provider.toml`. The manifests of the web pack, the web probe, and the HTTP and desktop packs are in their crates' `src/` folders, and the stream pack's is `src/manifest.toml`, so `packs.web.max_read_chars`, `packs.http.max_response_bytes`, `packs.stream.wait_ceiling_seconds`, and ten more would leave the first release's kept contract unchecked, though each is built and a person may have set it.

A failure code is an open vocabulary (0437, point 4): a release may add one. Removing one is a break. So a code declared before anything raises it is a promise made with nothing behind it.

## Decision

1. **A kept release holds the settings that are built.** `settings.json` in `contracts/<version>/` lists each key whose part is built, with its type, and the settings of every pack's manifest, wherever in its crate the manifest is (the one each pack's `include_str!` names). A key marked not built is left out, as an action with no handler is left out of the kept API description. The schema still declares it, and a value for it is still refused by name (rule 7).
2. **A failure code is declared only when something raises it.** `python3 tools/failure_codes.py --check` names a code whose constant and whose string appear in no Rust file outside its own table and outside tests, and in no pack manifest. The two codes today are removed; each comes back with the plug-in host that raises it.
3. **Nothing else in point 1 of 0437 changes.** The vocabularies of `a2m-core` are kept whole, because every member of a closed vocabulary is one a client may branch on.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep every key and code, as now | Freezes 55 types and two codes that nobody has tried, for the reason 0437 already rejected for unbuilt actions |
| Remove the 55 unbuilt settings from the schema | The schema is the one list that refuses a value for a part not built, and says what happens instead; removing them would make such a value an unknown key, which is refused too, but with no word of what it is for |
| Keep codes in the table and mark the two as not raised | A mark the kept contract reads would be a second list beside the code that raises; the check finds the same fact from the code |

## What it costs a user

Nothing they meet: no built setting or raised code changes, and 13 settings a person may have set become ones a later release cannot rename without saying so. A setting built after the first release may still change its type once before its first release carries it.

## Proof

Package LS2: `tools/test_contracts.py` holds that a key marked not built is left out of `settings.json`, a built one is kept, and each of the 28 settings of section 226's per-pack table is kept; `tools/failure_codes.py --check` fails on a tree with a declared code nothing raises, naming it, and passes on today's tree once the two are removed; `python3 tools/failure_codes.py` regenerates the technical reference with 424 codes; the documentation audit reports no findings.

## What would reopen it

A client that reads an unbuilt setting's key from the kept contract to plan for it, or a code that must be declared before its raiser exists, for example because a pack outside this repository raises it.
