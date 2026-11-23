# 0399. A secret a person chooses has at least eight bytes, and a replaced one is released at once

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** `Vault` and `Redactor` of `a2m-vault` (new `Vault::register_chosen`, `Vault::release_chosen`, `Redactor::register_in`, `Redactor::release`; `Vault::open_secret` registers in a slot), the failure code `secret_too_short`, the design of [`a2m-vault`](../architecture/crates/a2m-vault.md), and findings 12 and 13 of [the field tests](../reviews/field-tests.md). **Leaves alone:** every secret a sign-in, a provider, or a lease places, which is registered however short (a one-time code is six digits) and for the life of the process.

## Context

Field test 2 found two faults in how a value a person stores as a secret is masked.

- **Finding 12.** A password of one character was stored and registered. The redactor then replaced that letter in every answer and log. The design said every non-empty secret is registered however short, "the safer error" ([`a2m-vault`](../architecture/crates/a2m-vault.md)). For a value a person chooses it is not safe: it makes every answer unreadable, and a person's way out is to leave the value unmasked.
- **Finding 13.** After a stored secret was replaced, a check of a value against the registered secrets still stopped as holding the old secret until the engine restarted. The cause: registration lasted for the life of the process, with no way to release a value, so a replaced value stayed registered.

## Alternatives

| | |
| --- | --- |
| **(a) Refuse a chosen secret shorter than eight bytes, and register chosen secrets in a slot per stored item** | Chosen |
| (b) Mask a short secret only as a whole word | A secret's place in text is not known: a token in a header, a password in a URL. A rule that finds it only between spaces leaks it elsewhere, and rule 4 does not allow that |
| (c) Accept a short secret and do not mask it | Rule 4 forbids it |
| (d) Release every value on replace, by opening and unregistering the old one | Needs the old value, which the caller no longer has after a replace; a slot named for the item does not |

## Decision

1. **The least a chosen secret may have is eight bytes.** It is the number at which `RedactorHealth` already counted a secret as short. Below it, an exact match is a few characters of an alphabet that appears in ordinary text, so masking damages answers, and no policy that leaves it unmasked is allowed. Eight bytes is also half the passphrase floor (16, decision 0138). It is counted in bytes, not characters, so a value of fewer characters in a wider script passes when its bytes reach eight.
2. **`Vault::register_chosen(cx, secret)` refuses a shorter secret** with `input`/`secret_too_short`, whose message says why and whose next step says to choose at least eight bytes. It names no byte of the value. A refused secret is registered nowhere, and the caller seals nothing. A stored credential's `put` calls it before it seals (that change is in `a2m-auth`; see the journal).
3. **A secret that is not chosen is never refused.** Registration by `Vault::register` stays as it was: every non-empty secret, however short, for the life of the process. A stored value shorter than eight bytes that predates this decision is still masked when it is opened.
4. **Chosen and opened secrets are registered in a slot named for the item.** A slot holds one value. Registering another in it releases the first at once: the old value, and each encoded form of it no other secret holds, is no longer masked and no longer found by `contains_secret`. A value another slot, or a registration with no slot, also holds stays registered. `Vault::open_secret` registers in the same slot, so opening what was stored under an item replaces what an earlier opening registered. `Vault::release_chosen` releases a slot when its secret is deleted.
5. **The redactor counts holders of each form** so that a form two secrets share stays when one is released.

## What this showed, and the limit

A released value is no longer masked in text written afterwards. A log line already written holds whatever was masked at the time. A secret that was replaced because it was exposed is still a secret; the person is the one who rotates it elsewhere.

## Tests

`crates/a2m-vault/tests/chosen_secrets.rs`: values of one byte, seven bytes, and a multi-byte value of seven bytes are refused with `secret_too_short`, the message holds none of them, and nothing is registered in their place; eight bytes are accepted and masked; a replaced value is no longer masked, `contains_secret` no longer finds it, and the new one is; a value held by a second item stays until it is released too; opening a sealed replacement releases what the earlier opening registered; a slot never releases a value registered with no slot.
