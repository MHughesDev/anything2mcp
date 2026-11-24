# 0345. A drafter leaves out a name that perhaps names a secret

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Widens point 1 of [decision 0302](0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md).

**Date:** 2026-11-24

**Changes:** the shared test `names_a_secret` in `a2m-files`, which every pack's drafter asks.

## Context

Decision 0302 had every drafter leave out a field whose name the compiler takes for a certain secret's. That test knows whole words such as `access_token` and `api_key`. Field test 3 (finding 20) found a database column named `api_token`: not one of those words, so the drafter drafted it as an output and a trial returned the stored token, against rule 4. `accessToken` was already known to slip past the same way.

## Decision

1. **A drafter leaves out a name that names a secret for certain or perhaps.** The perhaps words are the compiler's own (`token`, `tokens`, `cookie`, `cookies`, `authorization`, `pin`, `bearer`), found as a word of the name at any place and in any case: split at `_`, `-`, `.`, and a change from lower to upper case. So `api_token`, `accessToken` and `X-Auth-Token` are left out; `tokenizer_mode` and `spinner_count` are not.
2. **The compiler's own refusal of a secret in a file is unchanged.** It still refuses only a certain secret's key, so no target file that compiled before is refused now.
3. A field left out this way is named in the description's sentence, as before, and a person who knows it is not a secret writes it back by hand.

## Why not

| Alternative | Why not |
| --- | --- |
| Add `api_token` to the certain words | The next name of the same kind would slip past again |
| Widen the compiler's refusal too | It would refuse target files that compile today, for names that may be harmless |

## Proof

`api_token` and `accessToken` are left out by the shared test in `a2m-files`; every pack's drafting tests still pass.
