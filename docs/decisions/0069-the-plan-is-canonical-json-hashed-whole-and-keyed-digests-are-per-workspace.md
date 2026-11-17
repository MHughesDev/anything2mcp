# 0069. The plan is canonical JSON, hashed whole, and keyed digests are per workspace

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the open point of [spec section 206](../spec/2-kernel/206-the-compiler-and-the-plan.md) on the plan's encoding, and two open points of the [`a2m-core` design](../architecture/crates/a2m-core.md): whether the encoding is a second outside crate there, and how `KeyedDigest` is keyed. **Changes a sentence of section 206.**

## Context

The plan's types and the check of its hash are in `a2m-core` ([decision 0036](0036-target-files-are-compiled-into-a-hashed-plan-before-anything-runs.md), [decision 0063](0063-the-kernels-interfaces-as-the-crate-designs-settle-them.md)). Section 206 asked for a compact binary encoding, to be chosen by measuring. The compiler and the engine are built next and both need the encoding to exist. What the encoding must give is fixed: the same plan, the same bytes, on three operating systems and in every run.

## Decision

**The encoding**

- **Canonical JSON.** No spaces; the keys of every object sorted by their bytes; whole numbers only; one way to escape text. The writer is about a hundred lines in `a2m-core` over `serde_json`, which the workspace already has. It sorts keys itself, so the result does not depend on how the outside crate was built, and a map with no order of its own cannot leak its order.
- A float is refused when encoding. A decimal travels as text.
- One encoding for everything: the plan, a pack's payload, and the canonical bytes of a `Value`. A pack cannot choose another.
- **The envelope is one line:** `anything2mcp-plan`, a space, the format number, a line feed. The body follows.
- **The hash is the SHA-256 digest of all the bytes,** the first line included. `Plan::decode` compares it before it reads anything, so one changed bit anywhere is refused as `plan_hash_mismatch`.
- After the hash: an unknown format is `plan_format_unsupported`; bytes that are not a plan, that carry a field this format does not have, or that refer to a part the plan does not hold are `plan_unreadable`. `encode` makes the same check of references, so the compiler cannot emit what the engine would refuse.
- A pack's payload is held as canonical JSON text inside the plan, written as one string. The kernel stores and hashes it and never parses it.
- The format number is 1. A binary encoding, if loading misses its budget, is format 2 behind the same first line and the same two functions.
- *Instead of:* a binary encoding crate now. It would be a second outside crate in layer 1, chosen before anything can be measured, and a plan nobody can read. And *instead of* handing `decode` a deserializer, which would let two crates disagree on the bytes.

**Keyed digests**

- **Per workspace.** `KeyedDigest::keyed_digest` takes the workspace, the purpose, and the bytes. The same content in two workspaces has two names, so nothing can be matched across the boundary.
- One construction, in `a2m-core`, so every implementation agrees: HMAC-SHA-256, written on the `sha2` crate already in the tree. The workspace's key is the HMAC of the workspace's identifier under the installation's digest key; the digest is the HMAC, under that, of the purpose's name, a zero byte, and the bytes. The vault holds the key and calls the function.
- Three purposes: `blob_name`, `restricted_value`, `record_key`.
- *Instead of:* one key per installation, which is simpler and lets a guess confirmed in one workspace be confirmed in every other.

**Redaction**

- `Redact` has one required method, `redact`. `contains_secret` has a default that compares the redacted text with the input, which an implementation may replace with something cheaper that gives the same answer. The trait also redacts a whole `Failure`, a `Value`, and `Values`, names included, so that no caller redacts part of a thing.

## Consequences

- Section 206's sentence on the encoding is changed to say this.
- Loading a 500-operation plan must still be measured against its 20 ms target in the slice that first compiles one. JSON is slower to read than a binary form; the format number is the way out.
- `Plan::show` prints the same content as the encoding, in the same order, indented, under the plan's hash.
- The ledger design's `Store::attach` takes a `KeyedDigest` as before; its calls now pass the workspace.
- The vault design's `contains_registered` is the trait's `contains_secret`.

## Alternatives considered

Given beside each choice above.

## Revisit if

Plan loading misses its budget once measured, or a pack's payload is large enough that holding it as text inside the plan costs more than the kernel can afford.

## Spec sections

- [206-the-compiler-and-the-plan.md](../spec/2-kernel/206-the-compiler-and-the-plan.md)
- [224-when-the-engine-itself-fails.md](../spec/2-kernel/224-when-the-engine-itself-fails.md)
- [216-the-vault-and-its-keys.md](../spec/2-kernel/216-the-vault-and-its-keys.md)
- [The `a2m-core` design](../architecture/crates/a2m-core.md)
