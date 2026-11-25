# 0438. The passphrase route's salt file records the cost its root was derived with

**Status:** Accepted. Design decision, made in the design review round by lens 14, a year later ([report](../reviews/a-year-later-review-2026-11-25.md), finding 2). The owner had not read this record when it was written. It changes the form of one file the vault keeps, and no public type. Built by roadmap package YL3.

**Date:** 2026-11-25

**Changes:** `a2m-vault` (`src/store/passphrase.rs`: `PassphraseSalt` and the derivation's cost); [section 216](../spec/2-kernel/216-the-vault-and-its-keys.md); the design of [`a2m-vault`](../architecture/crates/a2m-vault.md).

## Context

On the passphrase route, which is the default on Linux ([decision 0220](0220-a-first-run-on-linux-needs-no-administrators-file.md)), the vault's root is derived with Argon2id from the person's passphrase and a salt kept in a file. The file holds the salt's 16 bytes and nothing else. The derivation's cost, 64 MiB and three passes on one lane, is a constant in the code (`STANDARD`).

The cost is the one thing about the derivation that is meant to change: it is raised as machines get faster, and lowered where a machine cannot afford it (the measured-system review found the derivation is a large part of a start). If a later release changes the constant, the same passphrase and salt give another root on every machine that upgrades; every key record wrapped under the old root fails to open, as `sealed_item_mismatch`, which by design never says why; and every stored session and credential is lost. Nothing in the file says which cost made the root, so no later build can derive the old one. The envelope of a sealed item has a format number and a cipher id ([`a2m-vault`](../architecture/crates/a2m-vault.md)); only the root's own derivation is unversioned.

## Decision

1. **The salt file is a record of the derivation:** a format number, the algorithm (`argon2id`) and its version (`0x13`), the memory in KiB, the passes, the lanes, and the salt, as one line of text the vault writes whole and reads whole. Nothing in it is secret, as the salt was not.
2. **A root is derived with the cost its file records,** never with the build's constant. The constant is the cost of a new file only.
3. **A file of 16 bare bytes, as every build before this one wrote, reads as format 0 with the cost of today's constant** (64 MiB, three passes, one lane), so an installation made before this record opens unchanged. It is not rewritten; the next file the route makes is the new form.
4. **A file with a format, an algorithm, or a cost this build does not support is refused** as `busy`/`key_store_unreachable`, with the fix to use a build that reads it; it is never derived with another cost. Unknown means stop.
5. **Raising the cost of an existing installation** means deriving a new root and wrapping every key under it, which is `vault.rotate` and is not built. Until it is, a changed constant applies to new installations only.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep the constant fixed forever | Gives up the one setting the standard expects to rise, or turns its first change into a lock-out |
| Keep the cost in the settings file | A setting can be edited apart from the salt it belongs to; a wrong value opens nothing and the vault cannot say why. The cost belongs beside the salt it was used with |
| Try each cost a build knows until the root's identifier matches | Each try costs a full derivation, and a wrong passphrase would cost them all before it is refused |

## Proof

Package YL3: a test in `a2m-vault` derives a root from a file written in the new form with a cost that is not the constant, and gets the root that cost gives; a file of 16 bare bytes gives the root today's builds give, for a fixed passphrase and salt, checked against a value held in the test; a file with an unknown format or algorithm is refused and derives nothing; `older_store.rs` in `a2m-ledger`, whose fixture keeps a salt of 16 bare bytes, still opens its key records.

## What would reopen it

A key-derivation standard that replaces Argon2id, which the record's algorithm field is there to carry.
