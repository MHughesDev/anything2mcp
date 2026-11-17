# 0122. Small things the core crate gives its builders, and a secret is wiped when dropped

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** four requests the builders of the crates above made of `a2m-core`. **Adds an outside crate to `a2m-core`.**

## Context

Crates above `a2m-core` had each written a copy of something it kept private: the shape check of a checked name, a hasher of named fields, a lint code made from a constant behind an `allow`. And [section 216](../spec/2-kernel/216-the-vault-and-its-keys.md) says a secret in memory is wiped when dropped, which `Secret` did not do.

## Decision

- **`Secret` overwrites its bytes with zeros when it is dropped,** through the outside crate `zeroize` 1.9.1 (MIT or Apache-2.0, default features, no further crates). The vault already uses it, so the workspace gains nothing new; `a2m-core` gains its fifth outside crate. It covers the bytes the wrapper holds. A copy made from `expose` is the caller's to wipe.
- *Instead of:* a wipe of our own, which needs `unsafe` to stop the compiler removing the write, and the workspace denies `unsafe`; and leaving the wipe to the vault's own types, which would leave every `Secret` outside the vault in freed memory.
- **`checked_name!` is public.** A crate declares a checked name of its own with it and needs no dependency beyond `a2m-core`. A name is one word, or with `dotted` two words joined by a dot.
- **Every checked name made by the macro has `fixed`,** a `const fn` for a name written in the source. In a `const`, a badly formed name fails the build. The names are now held as borrowed-or-owned text, which costs nothing. `LintCode::fixed` replaces the one `allow` in `a2m-files`.
- **`FieldHasher` is public:** the digest of several named fields, each written with its lengths, so no two sets of fields share bytes. Its encoding is pinned by a test. The ledger's `digest_of` is the same construction and should use it.
- `Mode`, `FindingKind`, and `FailureCode` are written by hand and are unchanged; `FailureCode` already has `fixed`.

## Consequences

- `a2m-auth` uses `a2m-core`'s `AuthProviderName` and `CredentialRef` ([decision 0121](0121-the-plan-carries-what-the-run-time-needs-from-target-files.md)). `a2m-policy`, `a2m-provider`, and `a2m-auth` each still have a name macro or a shape check of their own, which can become `checked_name!`.
- `Secret` has a `Drop`, so a secret cannot be taken apart by pattern; nothing did.

## Alternatives considered

Given beside each choice above.

## Revisit if

A secret must be held in locked memory, which needs more than a wipe.

## Spec sections

- [216. The vault and its keys](../spec/2-kernel/216-the-vault-and-its-keys.md)
- [`a2m-core`](../architecture/crates/a2m-core.md)
