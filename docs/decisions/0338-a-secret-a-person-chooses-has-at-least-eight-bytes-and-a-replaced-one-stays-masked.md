# 0338. A secret a person chooses has at least eight bytes, and a replaced one stays masked

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** `Vault` of `a2m-vault` (new `Vault::register_chosen`), the failure code `secret_too_short`, the design of [`a2m-vault`](../architecture/crates/a2m-vault.md), and findings 12 and 13 of [the field tests](../reviews/field-tests.md). **Leaves alone:** every other registration, which is of every non-empty secret however short (a one-time code is six digits) and lasts for the life of the process, as does a value `open_secret` returns.

## Context

Field test 2 found two faults in how a value a person stores as a secret is masked.

- **Finding 12.** A password of one character was stored and registered. The redactor then replaced that letter in every answer and log. The design said every non-empty secret is registered however short, "the safer error". For a value a person chooses it is not: it makes every answer unreadable, and the way out for a person is to leave the value out.
- **Finding 13.** After that secret was replaced, a check of a value against the registered secrets still stopped as holding the old secret until the engine restarted. The cause is finding 12: the old value was one character, so it matched in nearly every text. Registration lasting for the life of the process is as designed.

## Alternatives

| | |
| --- | --- |
| **(a) Refuse a chosen secret shorter than eight bytes; keep every registration for the life of the process** | Chosen |
| (b) Release a replaced value at once | Rejected. Rule 2 puts safe before correct, and rule 4 says a secret never reaches a caller. An old token or password may still appear in an answer, a page, or a log after it was rotated, and a person rotates a secret because it may be known. Unmasking it would let it out. A value of eight bytes or more that stays masked is harmless |
| (c) Mask a short secret only as a whole word | A secret's place in text is not known: a token in a header, a password in a URL. A rule that finds it only between spaces leaks it elsewhere |
| (d) Accept a short secret and do not mask it | Rule 4 forbids it |

## Decision

1. **The least a chosen secret may have is eight bytes.** It is the number at which `RedactorHealth` already counted a secret as short. Below it, an exact match is a few characters of an alphabet that appears in ordinary text, so masking damages answers, and leaving it unmasked is not allowed. Eight bytes is half the passphrase floor (16, decision 0138). It is counted in bytes, not characters, so a value of fewer characters in a wider script passes when its bytes reach eight.
2. **`Vault::register_chosen(cx, secret)` refuses a shorter secret** with `input`/`secret_too_short`, whose message says why and whose next step says to choose at least eight bytes. It names no byte of the value. A refused secret is registered nowhere, and the caller seals nothing. A stored credential's `put` calls it before it seals (that change is in `a2m-auth`).
3. **A secret that is not chosen is never refused.** `Vault::register` registers every non-empty secret, however short. A stored value shorter than eight bytes that predates this decision is still masked when it is opened.
4. **Nothing is released.** A registered value, a replaced one included, stays masked for the life of the process.

## Tests

`crates/a2m-vault/tests/chosen_secrets.rs`: values of one byte, seven bytes, and a multi-byte value of seven bytes are refused with `secret_too_short`, the message holds none of them, and nothing is registered in their place; eight bytes are accepted and masked; a replaced value and a value opened before its replacement both stay masked.
